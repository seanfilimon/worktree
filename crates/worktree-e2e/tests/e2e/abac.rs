//! ABAC attributes (v0.8, ENGINEERING_PLAN §16.5): `wt attr` on tenants,
//! trees and accounts (with their permission rules), attribute conditions
//! written in `.wt/access/policies.toml` (`wt access validate|apply`),
//! enforcement on REST reads, the simulator's per-condition explanation and
//! `--attr` overrides, the next request seeing an attribute change, and the
//! audit log.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{fsutil, require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, TREE};
use crate::iam_common::{decision, decision_allowed, is_refusal, json_file, wt_json};

/// Deny developers without clearance on trees classified secret; allow
/// engineers in an EU tenant to administer them.
fn policies_toml() -> String {
    format!(
        r#"
[[policy]]
name = "clearance"
description = "Secret trees need clearance"

[[policy.statement]]
sid = "need-clearance"
effect = "deny"
principals = ["role:developer"]
actions = ["tree:*"]
resources = ["wt:tree/alice/{TREE}"]

[[policy.statement.conditions.attributes]]
key = "tree.classification"
op = "equals"
value = "secret"

[[policy.statement.conditions.attributes]]
key = "principal.clearance"
op = "not_in"
values = ["secret", "top-secret"]

[[policy.statement]]
sid = "eu-engineers"
effect = "allow"
principals = ["authenticated"]
actions = ["tree:admin"]
resources = ["wt:tree/alice/{TREE}"]
conditions = {{ attributes = [{{ key = "principal.department", op = "equals", values = ["eng"] }}, {{ key = "tenant.region", op = "starts_with", values = ["eu-"] }}] }}
"#
    )
}

/// The `attributes` map of a `wt attr --json` result.
#[track_caller]
fn attrs(v: &Value) -> Value {
    find_objects(v, &["attributes"])
        .first()
        .map(|o| o["attributes"].clone())
        .unwrap_or_else(|| panic!("no attributes in\n{}", pretty(v)))
}

/// The simulator's `conditions` list.
fn conditions(v: &Value) -> Vec<Value> {
    find_objects(v, &["key", "op", "matched"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn abac() {
    require_e2e!();
    let env = TestEnv::with_server("abac");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# abac\n")]);
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let bob_api = bob.api();
    let tree = format!("alice/{TREE}");
    let tree_path = format!("/api/v1/trees/alice/{TREE}");

    // --- wt attr on tenants and trees
    let v = wt_json(
        &alice,
        &["attr", "set", "tenant", "alice", "region=eu-west"],
    );
    assert_eq!(attrs(&v), json!({"region": "eu-west"}), "{}", pretty(&v));
    wt_json(
        &alice,
        &[
            "attr",
            "set",
            "tree",
            &tree,
            "classification=secret",
            "owner=ops",
        ],
    );
    let v = wt_json(&alice, &["attr", "unset", "tree", &tree, "owner"]);
    assert_eq!(
        attrs(&v),
        json!({"classification": "secret"}),
        "{}",
        pretty(&v)
    );
    let shown = alice.wt().args(["attr", "show", "tree", &tree]).ok();
    assert!(
        shown.stdout.contains("classification") && shown.stdout.contains("secret"),
        "human output lists the attribute\n{shown}"
    );
    assert_eq!(
        bob_api.get(&format!("{tree_path}/attributes")).ok_json()["attributes"],
        json!({"classification": "secret"}),
        "tree readers see the tree's attributes"
    );

    // Permission rules: tree:admin for trees, server admins for accounts,
    // nobody for their own account.
    bob.wt()
        .args(["attr", "set", "tree", &tree, "classification=public"])
        .fails();
    bob.wt()
        .args(["attr", "set", "tenant", "alice", "region=us"])
        .fails();
    bob.wt()
        .args(["attr", "set", "account", "bob", "clearance=secret"])
        .fails();
    bob.wt()
        .args(["attr", "set", "account", "me", "clearance=secret"])
        .fails();
    alice
        .wt()
        .args(["attr", "set", "tree", &tree, "Bad Key=x"])
        .fails();
    let mine = wt_json(&bob, &["attr", "show", "account", "me"]);
    assert_eq!(attrs(&mine), json!({}), "{}", pretty(&mine));

    // --- attribute conditions in declarative access files
    let access_dir = a.dir.join(".wt").join("access");
    fsutil::write(
        &access_dir.join("policies.toml"),
        "[[policy]]\nname = \"bad\"\n[[policy.statement]]\neffect = \"deny\"\nprincipals = [\"*\"]\nactions = [\"tree:read\"]\nresources = [\"*\"]\nconditions = { attributes = [{ key = \"department\", op = \"equals\" }] }\n",
    );
    let bad = a.wt().args(["access", "validate"]).fails();
    assert!(
        bad.mentions("E2011") && bad.mentions("attributes[0]"),
        "attribute conditions are validated locally with located errors\n{bad}"
    );
    fsutil::write(&access_dir.join("policies.toml"), policies_toml());
    let valid = a.wt().args(["access", "validate"]).json();
    assert_eq!(valid["valid"], Value::Bool(true), "{}", pretty(&valid));
    let applied = a.wt().args(["access", "apply", "--yes"]).json();
    assert_eq!(
        applied["accepted"],
        Value::Bool(true),
        "{}",
        pretty(&applied)
    );
    let shown = alice.wt().args(["policy", "list", "alice"]).json();
    assert!(
        shown.to_string().contains("principal.clearance"),
        "the applied policy keeps its attribute conditions:\n{}",
        pretty(&shown)
    );

    // --- enforcement: bob (developer, no clearance) cannot read the tree
    let r = bob_api.get(&tree_path);
    assert!(is_refusal(r.status), "no clearance: denied\n{r}");
    alice_api.get(&tree_path).assert_ok();

    // --- the simulator explains every condition
    let sim = |extra: &[&str]| {
        let mut args = vec![
            "iam",
            "simulate",
            "--principal",
            "user:bob",
            "--action",
            "tree:read",
            "--tenant",
            "alice",
            "--tree",
            TREE,
        ];
        args.extend_from_slice(extra);
        wt_json(&alice, &args)
    };
    let v = sim(&[]);
    let d = decision(&v);
    assert!(!decision_allowed(&d), "{}", pretty(&v));
    assert_eq!(d["effect"], "deny", "{}", pretty(&v));
    let conds = conditions(&v);
    assert!(
        conds.iter().any(|c| c["key"] == "tree.classification"
            && c["value"] == "secret"
            && c["matched"] == true)
            && conds.iter().any(|c| c["key"] == "principal.clearance"
                && c["value"].is_null()
                && c["matched"] == true),
        "both conditions of the deny are explained:\n{}",
        pretty(&v)
    );
    // --attr overrides: with clearance the deny no longer applies.
    let v = sim(&["--attr", "principal.clearance=secret"]);
    assert!(decision_allowed(&decision(&v)), "{}", pretty(&v));
    // ...and with the classification unset neither.
    let v = sim(&["--attr", "tree.classification"]);
    assert!(decision_allowed(&decision(&v)), "{}", pretty(&v));
    let human = alice
        .wt()
        .args([
            "iam",
            "simulate",
            "--principal",
            "user:bob",
            "--action",
            "tree:read",
            "--tenant",
            "alice",
            "--tree",
            TREE,
        ])
        .ok();
    assert!(
        human.stdout.contains("DENIED")
            && human.stdout.contains("Attribute conditions:")
            && human.stdout.contains("(unset)"),
        "human simulate output explains the attribute conditions\n{human}"
    );
    // wt policy simulate takes --attr too.
    let candidate = json_file(
        &env,
        "noop.json",
        &json!({"name": "noop", "enabled": true, "statements": []}),
    );
    let v = wt_json(
        &alice,
        &[
            "policy",
            "simulate",
            "alice",
            "--file",
            candidate.to_str().unwrap(),
            "--principal",
            "user:bob",
            "--action",
            "tree:read",
            "--tree",
            TREE,
            "--attr",
            "principal.clearance=top-secret",
        ],
    );
    assert_eq!(
        v["candidate"]["allowed"],
        Value::Bool(true),
        "{}",
        pretty(&v)
    );

    // --- a server admin grants clearance; the next request sees it
    let admin = env.admin_user();
    admin.login();
    let v = wt_json(
        &admin,
        &[
            "attr",
            "set",
            "account",
            "bob",
            "clearance=secret",
            "department=eng",
        ],
    );
    assert_eq!(attrs(&v)["clearance"], "secret", "{}", pretty(&v));
    bob_api.get(&tree_path).assert_ok();
    let mine = wt_json(&bob, &["attr", "show", "account", "me"]);
    assert_eq!(attrs(&mine)["department"], "eng", "{}", pretty(&mine));

    // The allow on department + tenant region grants bob tree:admin...
    bob_api
        .patch(&tree_path, json!({"description": "managed by engineering"}))
        .assert_ok();
    // ...until the tenant leaves the EU.
    wt_json(
        &alice,
        &["attr", "set", "tenant", "alice", "region=us-east"],
    );
    let r = bob_api.patch(&tree_path, json!({"description": "again"}));
    assert!(is_refusal(r.status), "tenant.region no longer matches\n{r}");

    // --- audit: changed keys, never values
    let audit = wt_json(
        &alice,
        &[
            "audit",
            "--tenant",
            "alice",
            "--action",
            "tree.attributes.update",
        ],
    );
    let entries = find_objects(&audit, &["action", "details"]);
    assert!(
        entries
            .iter()
            .any(|e| e["action"] == "tree.attributes.update"
                && e["details"]["set"] == "classification,owner"),
        "tree attribute changes are audited:\n{}",
        pretty(&audit)
    );
    assert!(
        !audit.to_string().contains("\"secret\""),
        "audit details carry keys only:\n{}",
        pretty(&audit)
    );
}
