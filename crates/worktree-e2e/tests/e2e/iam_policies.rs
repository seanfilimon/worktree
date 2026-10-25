//! Access policies (§12.1 step 5): an explicit deny on
//! `wt:branch/<t>/<tree>/main` for a team blocks direct pushes to main but
//! not to feature branches, the simulator explains it, disabling the policy
//! restores access, and an allow statement whose IP condition does not match
//! the request grants nothing.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};
use crate::iam_common::{
    assert_push_denied, decision, decision_allowed, find_by, has_reason, is_refusal, json_file,
    tree_ref, wt_json,
};

fn freeze_main(enabled: bool) -> Value {
    json!({
        "name": "freeze-main",
        "description": "Contractors go through merge requests",
        "enabled": enabled,
        "statements": [{
            "sid": "no-direct-main",
            "effect": "deny",
            "actions": ["branch:push"],
            "resources": [format!("wt:branch/alice/{TREE}/main")],
            "principals": ["team:contractors"],
            "conditions": {}
        }]
    })
}

fn office_read(cidr: &str) -> Value {
    json!({
        "name": "office-read",
        "description": "Carol may read from the office network",
        "enabled": true,
        "statements": [{
            "sid": "carol-office",
            "effect": "allow",
            "actions": ["tree:read"],
            "resources": [format!("wt:tree/alice/{TREE}")],
            "principals": ["user:carol"],
            "conditions": {"ip_ranges": [cidr]}
        }]
    })
}

/// The `id` of a policy record in CLI output.
fn policy_id(v: &Value, name: &str) -> String {
    find_objects(v, &["id", "statements"])
        .into_iter()
        .find(|p| p["name"] == name)
        .and_then(|p| p["id"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("policy {name} not found in\n{}", pretty(v)))
}

#[test]
fn policies() {
    require_e2e!();
    let env = TestEnv::with_server("policies");
    let alice = env.user("alice");
    publish_tree(&env, &alice, &[("README.md", "# policies\n")]);
    let alice_api = alice.api();
    let tree = tree_ref("alice");

    // bob reaches the tree through team "contractors" (developer grant).
    let bob = env.user("bob");
    bob.register();
    alice
        .wt()
        .args([
            "team",
            "create",
            "alice",
            "contractors",
            "--name",
            "Contractors",
        ])
        .ok();
    alice
        .wt()
        .args(["team", "add", "alice", "contractors", "bob"])
        .ok();
    alice
        .wt()
        .args([
            "grant",
            "set",
            &tree,
            "--team",
            "contractors",
            "--role",
            "developer",
        ])
        .ok();
    let b = bob.clone_tree("alice", TREE, TREE);

    // --- validation
    let good = json_file(&env, "freeze-main.json", &freeze_main(true));
    let valid = wt_json(
        &alice,
        &[
            "policy",
            "validate",
            "alice",
            "--file",
            good.to_str().unwrap(),
        ],
    );
    assert_eq!(
        find_objects(&valid, &["valid"])
            .first()
            .map(|v| v["valid"].clone()),
        Some(json!(true)),
        "validate:\n{}",
        pretty(&valid)
    );
    let bad = json_file(
        &env,
        "bad.json",
        &json!({"name": "bad", "enabled": true, "statements": [{
            "effect": "maybe", "actions": ["no:such"], "resources": ["nope"], "principals": ["???"], "conditions": {}
        }]}),
    );
    let out = alice
        .wt()
        .args([
            "--json",
            "policy",
            "validate",
            "alice",
            "--file",
            bad.to_str().unwrap(),
        ])
        .run();
    if out.success() {
        let v = out.json();
        let report = find_objects(&v, &["valid"]);
        assert_eq!(
            report.first().map(|r| r["valid"].clone()),
            Some(json!(false)),
            "invalid policy reported valid:\n{}",
            pretty(&v)
        );
        assert!(
            !find_objects(&v, &["message"]).is_empty(),
            "validation errors listed:\n{}",
            pretty(&v)
        );
    }
    alice
        .wt()
        .args(["policy", "create", "alice", "--file", bad.to_str().unwrap()])
        .fails();

    // --- create the deny policy
    let created = wt_json(
        &alice,
        &[
            "policy",
            "create",
            "alice",
            "--file",
            good.to_str().unwrap(),
        ],
    );
    let id = policy_id(&created, "freeze-main");
    let list = wt_json(&alice, &["policy", "list", "alice"]);
    assert_eq!(policy_id(&list, "freeze-main"), id);

    // Direct push to main: denied.
    b.commit(&[("bob.txt", "straight to main\n")], "bob: direct");
    assert_push_denied(&b, &alice_api, "alice", None);

    // Feature branches: fine.
    b.wt().args(["switch", "-c", "feature"]).ok();
    let feature_tip = b.commit(&[("feature.txt", "via MR\n")], "bob: feature");
    b.wt().args(["push", "--branch", "feature"]).ok();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "feature"),
        Some(feature_tip)
    );

    // The simulator explains both.
    let sim = |branch: &str| {
        wt_json(
            &alice,
            &[
                "iam",
                "simulate",
                "--principal",
                "user:bob",
                "--action",
                "branch:push",
                "--tenant",
                "alice",
                "--tree",
                &tree,
                "--branch",
                branch,
            ],
        )
    };
    let denied = decision(&sim("main"));
    assert!(!decision_allowed(&denied), "{}", pretty(&denied));
    assert_eq!(
        denied["effect"],
        "deny",
        "explicit deny:\n{}",
        pretty(&denied)
    );
    assert!(
        has_reason(&denied, "policy", "deny"),
        "a policy reason:\n{}",
        pretty(&denied)
    );
    assert!(
        find_objects(&denied, &["source", "effect"])
            .iter()
            .any(|r| r["source"] == "team_role" || r["source"] == "tree_grant"),
        "the overridden team grant is listed too:\n{}",
        pretty(&denied)
    );
    let human = alice
        .wt()
        .args([
            "iam",
            "simulate",
            "--principal",
            "user:bob",
            "--action",
            "branch:push",
            "--tenant",
            "alice",
            "--tree",
            &tree,
            "--branch",
            "main",
        ])
        .ok();
    assert!(
        human.stdout.contains("DENIED") && human.mentions("policy"),
        "human simulate output shows DENIED and the policy\n{human}"
    );
    let allowed = decision(&sim("feature"));
    assert!(decision_allowed(&allowed), "{}", pretty(&allowed));

    // --- disable restores access
    alice.wt().args(["policy", "disable", "alice", &id]).ok();
    let shown = wt_json(&alice, &["policy", "show", "alice", &id]);
    assert_eq!(
        find_by(&shown, "id", &id).map(|p| p["enabled"].clone()),
        Some(json!(false)),
        "policy disabled:\n{}",
        pretty(&shown)
    );
    b.wt().args(["switch", "main"]).ok();
    let main_tip = b.head_id();
    b.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(main_tip)
    );
    assert!(decision_allowed(&decision(&sim("main"))));

    // Re-enabling denies again; deleting removes it.
    alice.wt().args(["policy", "enable", "alice", &id]).ok();
    assert!(!decision_allowed(&decision(&sim("main"))));
    alice
        .wt()
        .args(["policy", "delete", "alice", &id])
        .confirmed();
    let list = wt_json(&alice, &["policy", "list", "alice"]);
    assert!(
        find_by(&list, "id", &id).is_none(),
        "policy deleted:\n{}",
        pretty(&list)
    );
    assert!(decision_allowed(&decision(&sim("main"))));

    // --- IP condition: an allow that does not match the request's address
    let carol = env.user("carol");
    carol.register();
    let carol_api = carol.api();
    let tree_path = format!("/api/v1/trees/alice/{TREE}");
    let r = carol_api.get(&tree_path);
    assert!(is_refusal(r.status), "carol has no access yet\n{r}");

    let remote = json_file(&env, "office-remote.json", &office_read("203.0.113.0/24"));
    let created = wt_json(
        &alice,
        &[
            "policy",
            "create",
            "alice",
            "--file",
            remote.to_str().unwrap(),
        ],
    );
    let office_id = policy_id(&created, "office-read");
    let version = find_by(&created, "id", &office_id).and_then(|p| p["version"].as_i64());
    // The e2e client connects from 127.0.0.1, outside 203.0.113.0/24.
    let r = carol_api.get(&tree_path);
    assert!(
        is_refusal(r.status),
        "an allow statement whose CIDR does not match grants nothing\n{r}"
    );
    let sim_ip = |ip: &str| {
        decision(&wt_json(
            &alice,
            &[
                "iam",
                "simulate",
                "--principal",
                "user:carol",
                "--action",
                "tree:read",
                "--tenant",
                "alice",
                "--tree",
                &tree,
                "--ip",
                ip,
            ],
        ))
    };
    let outside = sim_ip("127.0.0.1");
    assert!(!decision_allowed(&outside), "{}", pretty(&outside));
    let inside = sim_ip("203.0.113.9");
    assert!(decision_allowed(&inside), "{}", pretty(&inside));
    assert!(
        has_reason(&inside, "policy", "allow"),
        "allowed by the policy:\n{}",
        pretty(&inside)
    );

    // Widening the CIDR to loopback (full replace) grants the read.
    let local = json_file(&env, "office-local.json", &office_read("127.0.0.0/8"));
    alice
        .wt()
        .args([
            "policy",
            "update",
            "alice",
            &office_id,
            "--file",
            local.to_str().unwrap(),
        ])
        .ok();
    let shown = wt_json(&alice, &["policy", "show", "alice", &office_id]);
    let new_version = find_by(&shown, "id", &office_id).and_then(|p| p["version"].as_i64());
    if let (Some(v0), Some(v1)) = (version, new_version) {
        assert!(v1 > v0, "update bumps the version:\n{}", pretty(&shown));
    }
    carol_api.get(&tree_path).assert_ok();
    // Still read-only for carol.
    let r = carol_api.patch(&tree_path, json!({"description": "carol was here"}));
    assert!(is_refusal(r.status), "policy allows only tree:read\n{r}");
}
