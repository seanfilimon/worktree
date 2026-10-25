//! Custom roles (§12.1, §12.3): a tenant role with `tree:read` + `mr:create`
//! lets a member clone and open merge requests but not push or approve;
//! updating the role's permissions takes effect immediately; a role cannot
//! be deleted while assigned.

use std::collections::BTreeSet;

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};
use crate::iam_common::{assert_push_denied, find_by, rest_member_role, tree_ref, wt_json};

fn permissions(role: &Value) -> BTreeSet<String> {
    role["permissions"]
        .as_array()
        .unwrap_or_else(|| panic!("role without permissions:\n{}", pretty(role)))
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn custom_roles() {
    require_e2e!();
    let env = TestEnv::with_server("custom_roles");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# roles\n")]);
    let alice_api = alice.api();
    let tree = tree_ref("alice");
    let tree_path = format!("/api/v1/trees/alice/{TREE}");

    // A feature branch for the MR (pushed by the owner).
    a.wt().args(["switch", "-c", "feature"]).ok();
    let feature_tip = a.commit(&[("feature.txt", "idea\n")], "feature: idea");
    a.wt().args(["push", "--branch", "feature"]).ok();
    a.wt().args(["switch", "main"]).ok();

    // --- create the role
    let created = wt_json(
        &alice,
        &[
            "role",
            "create",
            "alice",
            "reviewer",
            "--permission",
            "tree:read",
            "--permission",
            "mr:create",
            "--description",
            "Reads code and proposes changes",
        ],
    );
    let role = find_by(&created, "name", "reviewer")
        .unwrap_or_else(|| panic!("role create output:\n{}", pretty(&created)));
    assert_eq!(permissions(role), set(&["tree:read", "mr:create"]));

    // Unknown actions and built-in names are rejected.
    alice
        .wt()
        .args([
            "role",
            "create",
            "alice",
            "bogus",
            "--permission",
            "nope:zzz",
        ])
        .fails();
    alice
        .wt()
        .args([
            "role",
            "create",
            "alice",
            "developer",
            "--permission",
            "tree:read",
        ])
        .fails();

    let roles = wt_json(&alice, &["role", "list", "alice"]);
    let all = find_objects(&roles, &["name", "permissions"]);
    let builtin = |name: &str| {
        all.iter()
            .find(|r| r["name"] == name)
            .and_then(|r| r["builtin"].as_bool())
    };
    assert_eq!(builtin("owner"), Some(true), "roles:\n{}", pretty(&roles));
    assert_eq!(
        builtin("developer"),
        Some(true),
        "roles:\n{}",
        pretty(&roles)
    );
    assert_eq!(
        builtin("reviewer"),
        Some(false),
        "roles:\n{}",
        pretty(&roles)
    );
    let first_custom = all.iter().position(|r| r["builtin"] == false);
    let last_builtin = all.iter().rposition(|r| r["builtin"] == true);
    assert!(
        first_custom > last_builtin,
        "built-ins are listed first:\n{}",
        pretty(&roles)
    );
    let shown = wt_json(&alice, &["role", "show", "alice", "reviewer"]);
    let shown = find_by(&shown, "name", "reviewer").expect("role show");
    assert_eq!(permissions(shown), set(&["tree:read", "mr:create"]));

    // --- bob becomes a reviewer member
    let bob = env.user("bob");
    bob.register();
    alice
        .wt()
        .args(["tenant", "add-member", "alice", "bob", "--role", "reviewer"])
        .ok();
    assert_eq!(
        rest_member_role(&alice_api, "alice", "bob").as_deref(),
        Some("reviewer")
    );

    // Reads work.
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_eq!(b.read("README.md"), "# roles\n");
    bob.api().get(&tree_path).assert_ok();

    // Pushes do not.
    b.commit(&[("bob.txt", "nope\n")], "bob: try to push");
    assert_push_denied(&b, &alice_api, "alice", None);

    // MRs can be opened (but not approved or merged).
    let mr = wt_json(
        &bob,
        &[
            "mr",
            "create",
            &tree,
            "--source",
            "feature",
            "--target",
            "main",
            "--title",
            "Adopt the idea",
            "--description",
            "From a reviewer",
        ],
    );
    let mr = find_objects(&mr, &["number", "state"])
        .first()
        .map(|m| (*m).clone())
        .unwrap_or_else(|| panic!("mr create output:\n{}", pretty(&mr)));
    let number = mr["number"].as_u64().expect("MR number");
    assert_eq!(mr["author"], "bob", "{}", pretty(&mr));
    assert_eq!(mr["state"], "open");
    let n = number.to_string();
    let listed = wt_json(&bob, &["mr", "list", &tree]);
    assert_eq!(
        find_objects(&listed, &["number", "state"]).len(),
        1,
        "mr list:\n{}",
        pretty(&listed)
    );
    bob.wt().args(["mr", "approve", &n, &tree]).fails();
    bob.wt().args(["mr", "merge", &n, &tree]).fails();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id())
    );
    assert_ne!(
        Some(feature_tip),
        server_tip(&alice_api, "alice", TREE, "main")
    );

    // --- extending the role takes effect immediately
    alice
        .wt()
        .args([
            "role",
            "update",
            "alice",
            "reviewer",
            "--permission",
            "tree:read",
            "--permission",
            "mr:create",
            "--permission",
            "tree:write",
        ])
        .ok();
    let shown = wt_json(&alice, &["role", "show", "alice", "reviewer"]);
    assert_eq!(
        permissions(find_by(&shown, "name", "reviewer").expect("role show")),
        set(&["tree:read", "mr:create", "tree:write"])
    );
    let tip = b.head_id();
    b.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(tip));

    // --- an assigned role cannot be deleted
    let refused = alice
        .wt()
        .args(["role", "delete", "alice", "reviewer"])
        .run_confirmed();
    assert!(
        !refused.success(),
        "deleting an assigned role must fail\n{refused}"
    );
    alice_api
        .get("/api/v1/tenants/alice/roles/reviewer")
        .assert_ok();

    // After reassigning bob it can.
    alice
        .wt()
        .args(["tenant", "set-role", "alice", "bob", "viewer"])
        .ok();
    assert_eq!(
        rest_member_role(&alice_api, "alice", "bob").as_deref(),
        Some("viewer")
    );
    alice
        .wt()
        .args(["role", "delete", "alice", "reviewer"])
        .confirmed();
    let roles = wt_json(&alice, &["role", "list", "alice"]);
    assert!(
        find_by(&roles, "name", "reviewer").is_none(),
        "role deleted:\n{}",
        pretty(&roles)
    );
    alice_api
        .get("/api/v1/tenants/alice/roles/reviewer")
        .assert_error(404, "not_found");

    // Viewer: read-only again.
    b.commit(&[("bob.txt", "as viewer\n")], "bob: viewer push");
    assert_push_denied(&b, &alice_api, "alice", None);
}
