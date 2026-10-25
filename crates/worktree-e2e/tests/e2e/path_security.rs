//! Read/write path security (ENGINEERING_PLAN §13.1/§13.2):
//! * a proprietary license rule without a grant on `vendor/**` of a public
//!   tree: an outsider's clone leaves those files out (`wt status` lists
//!   them as restricted, the working tree stays clean across pulls) and the
//!   REST blob answers `451 license_restricted`;
//! * a `tree:read` path-deny policy hides `hr/**` from a member the same
//!   way (`403 path_denied` over REST);
//! * a `tree:write` path-deny policy rejects a push touching
//!   `config/secrets/**` while other paths still push.

use serde_json::{json, Value};
use worktree_e2e::json::pretty;
use worktree_e2e::{require_e2e, Repo, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};
use crate::iam_common::{json_file, looks_denied};

/// `restricted` paths of `wt --json status`.
#[track_caller]
fn restricted(repo: &Repo) -> Vec<String> {
    let v = repo.status_json();
    let mut out: Vec<String> = v["restricted"]
        .as_array()
        .unwrap_or_else(|| panic!("status has no `restricted` list:\n{}", pretty(&v)))
        .iter()
        .filter_map(|p| p.as_str().map(String::from))
        .collect();
    out.sort();
    out
}

fn deny_policy(name: &str, action: &str, path_glob: &str) -> Value {
    json!({
        "name": name,
        "description": format!("bob may not {action} {path_glob}"),
        "enabled": true,
        "statements": [{
            "sid": name,
            "effect": "deny",
            "actions": [action],
            "resources": [format!("wt:path/alice/{TREE}/{path_glob}")],
            "principals": ["user:bob"],
            "conditions": {}
        }]
    })
}

#[test]
fn path_security() {
    require_e2e!();
    let env = TestEnv::with_server("path_security");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# open core\n"),
            ("src/main.c", "int main(void) { return 0; }\n"),
            ("vendor/blob.c", "/* proprietary vendor code */\n"),
            ("vendor/sub/deep.h", "#define SECRET_SAUCE 42\n"),
            ("hr/salaries.txt", "alice: lots\n"),
            ("config/app.toml", "debug = false\n"),
        ],
    );
    let alice_api = alice.api();
    let tree_path = format!("/api/v1/trees/alice/{TREE}");
    alice
        .wt()
        .args([
            "trees",
            "update",
            &format!("alice/{TREE}"),
            "--visibility",
            "public",
        ])
        .ok();

    // --- proprietary rule without a grant (REST: the CLI always sends one)
    alice_api
        .put(
            &format!("{tree_path}/licenses"),
            json!({"path_glob": "vendor/**", "spdx": "LicenseRef-Proprietary", "grant": ""}),
        )
        .assert_ok();
    let rules = a.wt().args(["license", "list"]).json();
    assert!(
        rules.to_string().contains("LicenseRef-Proprietary"),
        "{}",
        pretty(&rules)
    );

    // --- carol: outsider reading the public tree
    let carol = env.user("carol");
    carol.register();
    let c = carol.clone_tree("alice", TREE, TREE);
    assert!(c.exists("README.md") && c.exists("src/main.c"));
    assert!(
        !c.exists("vendor/blob.c") && !c.exists("vendor/sub/deep.h"),
        "license-restricted files are not checked out"
    );
    assert_eq!(
        restricted(&c),
        vec!["vendor/blob.c".to_string(), "vendor/sub/deep.h".to_string()]
    );
    c.assert_clean();
    let text = c.wt().arg("status").ok();
    assert!(
        text.mentions("restricted") && text.stdout.contains("vendor/blob.c"),
        "human status lists the restricted files\n{text}"
    );
    let carol_api = carol.api();
    let head = a.head_id();
    carol_api
        .get(&format!("{tree_path}/blob/{head}/vendor/blob.c"))
        .assert_error(451, "license_restricted");
    let ok = carol_api.get(&format!("{tree_path}/blob/{head}/src/main.c"));
    ok.assert_ok();
    assert_eq!(ok.text(), "int main(void) { return 0; }\n");
    // alice (owner) is not restricted.
    alice_api
        .get(&format!("{tree_path}/blob/{head}/vendor/blob.c"))
        .assert_ok();

    // Pulls keep them out and the working tree clean.
    let tip = a.commit(
        &[
            ("vendor/blob.c", "/* proprietary vendor code v2 */\n"),
            ("src/main.c", "int main(void) { return 1; }\n"),
        ],
        "vendor v2",
    );
    a.push();
    c.pull();
    assert_eq!(c.head_id(), tip);
    assert_eq!(c.read("src/main.c"), "int main(void) { return 1; }\n");
    assert!(!c.exists("vendor/blob.c"));
    assert_eq!(restricted(&c).len(), 2);
    c.assert_clean();

    // --- bob: member with a path-deny read policy on hr/** and a write
    // deny on config/secrets/**
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    for (file, policy) in [
        ("hide-hr.json", deny_policy("hide-hr", "tree:read", "hr/**")),
        (
            "no-secrets.json",
            deny_policy("no-secrets", "tree:write", "config/secrets/**"),
        ),
    ] {
        let path = json_file(&env, file, &policy);
        alice
            .wt()
            .args(["policy", "create", "alice", "--file"])
            .arg(&path)
            .ok();
    }
    let b = bob.clone_tree("alice", TREE, TREE);
    assert!(
        b.exists("vendor/blob.c"),
        "license rules do not restrict members"
    );
    assert!(!b.exists("hr/salaries.txt"), "path-denied file withheld");
    assert_eq!(restricted(&b), vec!["hr/salaries.txt".to_string()]);
    b.assert_clean();
    let bob_api = bob.api();
    bob_api
        .get(&format!("{tree_path}/blob/{tip}/hr/salaries.txt"))
        .assert_error(403, "path_denied");

    // Write deny: touching config/secrets/ is refused, the rest pushes.
    let before = server_tip(&alice_api, "alice", TREE, "main");
    b.commit(
        &[("config/secrets/api.key", "sk-not-really\n")],
        "bob: add a secret",
    );
    let out = b.wt().arg("push").fails();
    assert!(
        looks_denied(&out) && out.mentions("config/secrets/api.key"),
        "push rejection names the denied path\n{out}"
    );
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), before);

    let b2 = bob.clone_tree("alice", TREE, "proj-2");
    let ok_tip = b2.commit(&[("config/app.toml", "debug = true\n")], "bob: config");
    b2.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(ok_tip));
}
