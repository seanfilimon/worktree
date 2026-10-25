//! Policy history (ENGINEERING_PLAN §14.7): creating and updating a policy
//! records versions (`wt policy history`), `wt policy diff` shows the
//! statement-level change, `wt policy rollback` restores an earlier version
//! as a new one, and each step's effect on bob's pushes to main is
//! observed. A deleted policy stays listed (`wt policy deleted`) and a
//! rollback restores it with its id.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};
use crate::iam_common::{assert_push_denied, json_file};

fn read_only() -> Value {
    json!({
        "name": "bob-rules",
        "description": "bob reads the tree",
        "enabled": true,
        "statements": [{
            "sid": "bob-read",
            "effect": "allow",
            "actions": ["tree:read"],
            "resources": [format!("wt:tree/alice/{TREE}")],
            "principals": ["user:bob"],
            "conditions": {}
        }]
    })
}

fn frozen() -> Value {
    let mut p = read_only();
    p["description"] = json!("bob may not push to main");
    p["statements"].as_array_mut().unwrap().push(json!({
        "sid": "no-main",
        "effect": "deny",
        "actions": ["branch:push"],
        "resources": [format!("wt:branch/alice/{TREE}/main")],
        "principals": ["user:bob"],
        "conditions": {}
    }));
    p
}

/// (version, change) of every entry of `wt --json policy history`.
fn versions(v: &Value) -> Vec<(u64, String)> {
    find_objects(v, &["version", "change"])
        .into_iter()
        .map(|e| (e["version"].as_u64().unwrap_or(0), str_field(e, "change")))
        .collect()
}

/// (op, sid) of every change of `wt --json policy diff`.
fn changes(v: &Value) -> Vec<(String, String)> {
    v["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("no changes in\n{}", pretty(v)))
        .iter()
        .map(|c| (str_field(c, "op"), str_field(c, "sid")))
        .collect()
}

/// bob pushes a new snapshot to main (must succeed).
#[track_caller]
fn bob_pushes(b: &Repo, api: &worktree_e2e::Api, n: u32) {
    b.pull();
    let tip = b.commit(&[(&format!("bob-{n}.txt"), "bob\n")], &format!("bob {n}"));
    b.push();
    assert_eq!(server_tip(api, "alice", TREE, "main"), Some(tip));
}

#[test]
fn policy_history() {
    require_e2e!();
    let env = TestEnv::with_server("policy_history");
    let alice = env.user("alice");
    publish_tree(&env, &alice, &[("README.md", "# history\n")]);
    let api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    let pol = |args: &[&str]| alice.wt().arg("policy").args(args).json();

    // --- v1: create (no effect on pushes)
    let f1 = json_file(&env, "v1.json", &read_only());
    let created = pol(&["create", "alice", "--file", f1.to_str().unwrap()]);
    let id = str_field(&created, "id");
    bob_pushes(&b, &api, 1);

    // --- v2: update adds a deny on main
    let f2 = json_file(&env, "v2.json", &frozen());
    pol(&["update", "alice", &id, "--file", f2.to_str().unwrap()]);
    b.commit(&[("blocked.txt", "no\n")], "blocked");
    assert_push_denied(&b, &api, "alice", None);

    let history = pol(&["history", &id, "--tenant", "alice"]);
    assert_eq!(
        versions(&history),
        vec![(1, "create".to_string()), (2, "update".to_string())],
        "{}",
        pretty(&history)
    );
    assert!(
        find_objects(&history, &["version", "change"])
            .iter()
            .all(|e| e["changed_by"] == "alice"),
        "{}",
        pretty(&history)
    );
    let text = alice
        .wt()
        .args(["policy", "history", &id, "--tenant", "alice"])
        .ok();
    assert!(text.mentions("create") && text.mentions("update"), "{text}");

    // --- diff: latest vs previous, and from the empty policy
    let diff = pol(&["diff", &id, "--tenant", "alice"]);
    assert_eq!(
        changes(&diff),
        vec![("add".to_string(), "no-main".to_string())],
        "{}",
        pretty(&diff)
    );
    assert_eq!(diff["from"].as_u64(), Some(1), "{}", pretty(&diff));
    assert_eq!(diff["to"].as_u64(), Some(2), "{}", pretty(&diff));
    let diff0 = pol(&["diff", &id, "--tenant", "alice", "--from", "0", "--to", "1"]);
    assert_eq!(
        changes(&diff0),
        vec![("add".to_string(), "bob-read".to_string())],
        "{}",
        pretty(&diff0)
    );
    let back = pol(&["diff", &id, "--tenant", "alice", "--from", "2", "--to", "1"]);
    assert_eq!(
        changes(&back),
        vec![("remove".to_string(), "no-main".to_string())],
        "{}",
        pretty(&back)
    );
    let v2 = pol(&["show-version", &id, "2", "--tenant", "alice"]);
    assert!(v2.to_string().contains("no-main"), "{}", pretty(&v2));

    // --- rollback to v1: recorded as v3, bob can push again
    bob.wt()
        .args(["policy", "rollback", &id, "1", "--tenant", "alice"])
        .fails(); // a developer has no iam:admin
    let rolled = pol(&[
        "rollback",
        &id,
        "1",
        "--tenant",
        "alice",
        "-m",
        "unfreeze main",
    ]);
    assert!(
        !rolled.to_string().contains("no-main"),
        "{}",
        pretty(&rolled)
    );
    let shown = pol(&["show", "alice", &id]);
    assert_eq!(
        find_objects(&shown, &["sid", "effect"]).len(),
        1,
        "{}",
        pretty(&shown)
    );
    b.push();
    bob_pushes(&b, &api, 2);
    let history = pol(&["history", &id, "--tenant", "alice"]);
    assert_eq!(
        versions(&history),
        vec![
            (1, "create".to_string()),
            (2, "update".to_string()),
            (3, "rollback".to_string())
        ],
        "{}",
        pretty(&history)
    );
    let v3 = find_objects(&history, &["version", "change"])
        .into_iter()
        .find(|e| e["version"] == 3)
        .unwrap();
    assert!(
        str_field(v3, "comment").contains("unfreeze main"),
        "{}",
        pretty(v3)
    );
    let diff = pol(&["diff", &id, "--tenant", "alice"]);
    assert_eq!(
        changes(&diff),
        vec![("remove".to_string(), "no-main".to_string())]
    );

    // --- delete, list deleted, roll back to v2 (restores the deny, same id)
    alice
        .wt()
        .args(["policy", "delete", "alice", &id])
        .confirmed();
    bob_pushes(&b, &api, 3);
    let deleted = pol(&["deleted", "--tenant", "alice"]);
    assert!(deleted.to_string().contains(&id), "{}", pretty(&deleted));
    let history = pol(&["history", &id, "--tenant", "alice"]);
    assert_eq!(
        versions(&history).last().cloned(),
        Some((4, "delete".to_string())),
        "{}",
        pretty(&history)
    );
    let restored = pol(&["rollback", &id, "2", "--tenant", "alice"]);
    assert_eq!(str_field(&restored, "change"), "rollback");
    assert_eq!(
        str_field(&restored["policy"], "id"),
        id,
        "{}",
        pretty(&restored)
    );
    let shown = pol(&["show", "alice", &id]);
    assert_eq!(
        find_objects(&shown, &["sid", "effect"]).len(),
        2,
        "{}",
        pretty(&shown)
    );
    b.commit(&[("blocked-again.txt", "no\n")], "blocked again");
    assert_push_denied(&b, &api, "alice", None);
    assert_eq!(
        versions(&pol(&["history", &id, "--tenant", "alice"])).len(),
        5
    );
}
