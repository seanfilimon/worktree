//! Declarative access config and tenant status (ENGINEERING_PLAN §13.7):
//! `.wt/access/{roles,policies}.toml` → `wt access validate|plan|apply`, the
//! applied deny policy refuses bob's push to main, config-managed policies
//! are read-only over the API, `wt access pull` round-trips the document,
//! and `wt tenant suspend|resume` (server admin) make the tenant read-only
//! and writable again.

use serde_json::Value;
use worktree_e2e::fsutil;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};
use crate::iam_common::{assert_push_denied, looks_denied};

const ROLES: &str = r#"
[[role]]
name = "release-manager"
description = "Cut releases"
permissions = ["tree:read", "release:*", "mr:merge"]
"#;

fn policies() -> String {
    format!(
        r#"
[[policy]]
name = "protect-main"
description = "Only alice pushes to main"
enabled = true
[[policy.statement]]
sid = "no-direct-main"
effect = "deny"
principals = ["user:bob"]
actions = ["sync:push"]
resources = ["wt:branch/alice/{TREE}/main"]
"#
    )
}

/// `changes` of a plan / apply JSON document.
fn changes(v: &Value) -> Vec<String> {
    v["changes"]
        .as_array()
        .map(|c| {
            c.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn access_config() {
    require_e2e!();
    let env = TestEnv::with_server("access_config");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# access\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // bob can push to main before any policy.
    let bob_tip = b.commit(&[("bob.txt", "before the policy\n")], "bob: before");
    b.push();
    a.pull();

    // --- an invalid document is rejected locally
    let access_dir = a.dir.join(".wt").join("access");
    fsutil::write(
        &access_dir.join("policies.toml"),
        "[[policy]]\nname = \"bad\"\n[[policy.statement]]\neffect = \"deny\"\nprincipals = [\"???\"]\nactions = [\"no:such\"]\nresources = [\"nope\"]\n",
    );
    let bad = a.wt().args(["access", "validate"]).fails();
    assert!(
        bad.mentions("E20"),
        "validation errors carry E20xx codes\n{bad}"
    );

    // --- valid files: validate, plan, apply
    fsutil::write(&access_dir.join("roles.toml"), ROLES);
    fsutil::write(&access_dir.join("policies.toml"), policies());
    let valid = a.wt().args(["access", "validate"]).json();
    assert_eq!(valid["valid"], Value::Bool(true), "{}", pretty(&valid));
    assert!(
        valid.to_string().contains("branch:push"),
        "the sync:push alias is translated:\n{}",
        pretty(&valid)
    );

    let plan = a.wt().args(["access", "plan"]).json();
    assert_eq!(plan["accepted"], Value::Bool(true), "{}", pretty(&plan));
    let planned = changes(&plan);
    assert!(
        planned
            .iter()
            .any(|c| c.contains("protect-main") && c.contains("create"))
            && planned
                .iter()
                .any(|c| c.contains("release-manager") && c.contains("create")),
        "plan creates the role and the policy:\n{}",
        pretty(&plan)
    );
    // plan is a dry run.
    let policies_now = alice.wt().args(["policy", "list", "alice"]).json();
    assert!(
        !policies_now.to_string().contains("protect-main"),
        "plan must not change anything:\n{}",
        pretty(&policies_now)
    );

    let applied = a.wt().args(["access", "apply", "--yes"]).json();
    assert_eq!(
        applied["accepted"],
        Value::Bool(true),
        "{}",
        pretty(&applied)
    );
    assert_eq!(
        changes(&applied).len(),
        planned.len(),
        "{}",
        pretty(&applied)
    );
    let again = a.wt().args(["access", "plan"]).json();
    assert!(
        changes(&again).is_empty(),
        "nothing left to change after apply:\n{}",
        pretty(&again)
    );
    let roles = alice.wt().args(["role", "list", "alice"]).json();
    assert!(
        roles.to_string().contains("release-manager"),
        "custom role created:\n{}",
        pretty(&roles)
    );

    // --- the policy takes effect: bob may not push to main any more
    b.commit(&[("bob.txt", "after the policy\n")], "bob: after");
    assert_push_denied(&b, &alice_api, "alice", None);
    b.wt().args(["switch", "-c", "bob-work"]).ok();
    b.wt().args(["push", "--branch", "bob-work"]).ok();
    // alice is not affected.
    let alice_tip = a.commit(&[("alice.txt", "alice is fine\n")], "alice: push");
    a.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(alice_tip.clone())
    );
    assert_ne!(alice_tip, bob_tip);

    // Config-managed policies are read-only through the API.
    let list = alice.wt().args(["policy", "list", "alice"]).json();
    let id = find_objects(&list, &["id", "statements"])
        .into_iter()
        .find(|p| p["name"] == "protect-main")
        .map(|p| str_field(p, "id"))
        .unwrap_or_else(|| panic!("protect-main in\n{}", pretty(&list)));
    let out = alice
        .wt()
        .args(["policy", "delete", "alice", &id])
        .run_confirmed();
    assert!(
        !out.success() && out.mentions("config"),
        "deleting a config-managed policy is refused\n{out}"
    );

    // --- pull round trip into a fresh directory
    let pulled_dir = env.path("pulled-access");
    let pulled = a
        .wt()
        .args(["access", "pull", "--dir"])
        .arg(&pulled_dir)
        .json();
    assert!(
        pulled["written"].as_array().map(|w| w.len()) == Some(2),
        "roles.toml and policies.toml written:\n{}",
        pretty(&pulled)
    );
    let pulled_policies = fsutil::read_string(&pulled_dir.join("policies.toml"));
    assert!(
        pulled_policies.contains("protect-main") && pulled_policies.contains("branch:push"),
        "pulled policies.toml:\n{pulled_policies}"
    );
    assert!(fsutil::read_string(&pulled_dir.join("roles.toml")).contains("release-manager"));
    let replan = a
        .wt()
        .args(["access", "plan", "--dir"])
        .arg(&pulled_dir)
        .json();
    assert!(
        changes(&replan).is_empty(),
        "the pulled files describe exactly the applied state:\n{}",
        pretty(&replan)
    );
    // A second pull finds nothing to write.
    let pulled = a
        .wt()
        .args(["access", "pull", "--dir"])
        .arg(&pulled_dir)
        .json();
    assert_eq!(
        pulled["written"].as_array().map(|w| w.len()),
        Some(0),
        "{}",
        pretty(&pulled)
    );

    // --- tenant suspension (server admin): read-only, then writable again
    let admin = env.admin_user();
    admin.login();
    alice
        .wt()
        .args(["tenant", "suspend", "alice", "--reason", "billing"])
        .fails(); // server admins only
    admin
        .wt()
        .args(["tenant", "suspend", "alice", "--reason", "billing overdue"])
        .ok();
    let before = server_tip(&alice_api, "alice", TREE, "main");
    a.commit(
        &[("suspended.txt", "while suspended\n")],
        "during suspension",
    );
    let out = a.wt().arg("push").fails();
    assert!(
        looks_denied(&out) || out.mentions("suspend"),
        "push to a suspended tenant is refused\n{out}"
    );
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), before);
    // Reads still work.
    a.wt().arg("fetch").ok();
    alice_api
        .get(&format!("/api/v1/trees/alice/{TREE}"))
        .assert_ok();

    admin.wt().args(["tenant", "resume", "alice"]).ok();
    a.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id()),
        "push works again after resume"
    );
}
