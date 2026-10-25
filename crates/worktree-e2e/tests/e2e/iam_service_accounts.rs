//! Service accounts (§12.2, §12.3): a `developer` service account's key
//! pushes like a user credential; a key scoped to `tree:read` can clone but
//! not push; deleted keys and disabled or deleted service accounts no longer
//! authenticate.

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, server_tip, TREE};
use crate::iam_common::{api_with_token, assert_push_denied, find_by, string_list, wt_json};

/// (`token`, `api_token.id`) from `wt --json sa key-create`.
fn key_of(v: &Value) -> (String, String) {
    let token = find_objects(v, &["token"])
        .into_iter()
        .find_map(|o| o["token"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("no raw key in\n{}", pretty(v)));
    let id = v
        .get("api_token")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| panic!("no api_token.id in\n{}", pretty(v)));
    (token, id)
}

#[test]
fn service_accounts() {
    require_e2e!();
    let env = TestEnv::with_server("service_accounts");
    let alice = env.user("alice");
    publish_tree(&env, &alice, &[("README.md", "# bots\n")]);
    let alice_api = alice.api();
    let tree_path = format!("/api/v1/trees/alice/{TREE}");

    // --- create the service account and a full-access key
    let sa = wt_json(
        &alice,
        &[
            "sa",
            "create",
            "alice",
            "ci",
            "--role",
            "developer",
            "--description",
            "CI pipeline",
        ],
    );
    let rec = find_by(&sa, "name", "ci").unwrap_or_else(|| panic!("sa create:\n{}", pretty(&sa)));
    assert_eq!(rec["role"], "developer");
    assert_eq!(rec["disabled"], false);
    let list = wt_json(&alice, &["sa", "list", "alice"]);
    assert!(find_by(&list, "name", "ci").is_some(), "{}", pretty(&list));

    let key = wt_json(
        &alice,
        &[
            "sa",
            "key-create",
            "alice",
            "ci",
            "--name",
            "deploy",
            "--ttl-hours",
            "24",
        ],
    );
    let (ci_token, ci_key_id) = key_of(&key);
    assert!(ci_token.starts_with("wt_"), "raw key format: {ci_token}");

    // Raw keys are never shown again.
    let shown = alice
        .wt()
        .args(["--json", "sa", "show", "alice", "ci"])
        .ok();
    assert!(
        !shown.stdout.contains(&ci_token),
        "sa show must not reveal key material\n{shown}"
    );
    let shown = shown.json();
    assert!(
        find_by(&shown, "id", &ci_key_id).is_some(),
        "key listed by id:\n{}",
        pretty(&shown)
    );

    // --- the key acts as the service account over REST and sync
    let ci_api = api_with_token(&env, "sa:alice/ci", &ci_token);
    ci_api.get(&tree_path).assert_ok();
    let ci = env.user("ci-runner");
    ci.save_token("sa:alice/ci", &ci_token);
    let c = ci.clone_tree("alice", TREE, TREE);
    assert_eq!(c.read("README.md"), "# bots\n");
    let tip = c.commit(&[("build.txt", "build 1\n")], "ci: build 1");
    c.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(tip));
    let pushes = alice_api.get("/api/v1/audit?action=branch.push").ok_json();
    assert!(
        find_objects(&pushes, &["action", "actor"])
            .iter()
            .any(|e| e["actor"]
                .as_str()
                .is_some_and(|a| a.starts_with("sa:") || a.contains("alice/ci"))),
        "the push is attributed to the service account:\n{}",
        pretty(&pushes)
    );

    // --- a key scoped to tree:read can read but not push
    let ro = wt_json(
        &alice,
        &[
            "sa",
            "key-create",
            "alice",
            "ci",
            "--name",
            "readonly",
            "--scope",
            "tree:read",
        ],
    );
    let (ro_token, ro_key_id) = key_of(&ro);
    assert_eq!(
        string_list(&ro, "scopes"),
        vec!["tree:read".to_string()],
        "key scopes:\n{}",
        pretty(&ro)
    );
    let ro_api = api_with_token(&env, "sa:alice/ci (ro)", &ro_token);
    ro_api.get(&tree_path).assert_ok();
    let reader = env.user("ci-reader");
    reader.save_token("sa:alice/ci", &ro_token);
    let r = reader.clone_tree("alice", TREE, TREE);
    r.commit(&[("build.txt", "should not land\n")], "ci: read-only push");
    assert_push_denied(&r, &alice_api, "alice", None);

    // Deleting the key revokes it.
    alice
        .wt()
        .args(["sa", "key-delete", "alice", "ci", &ro_key_id])
        .confirmed();
    let resp = ro_api.get(&tree_path);
    assert!(
        matches!(resp.status, 401 | 403),
        "deleted key no longer works\n{resp}"
    );
    ci_api.get(&tree_path).assert_ok();

    // --- disabled service accounts cannot authenticate
    alice
        .wt()
        .args(["sa", "update", "alice", "ci", "--disable"])
        .ok();
    let shown = wt_json(&alice, &["sa", "show", "alice", "ci"]);
    assert_eq!(
        find_by(&shown, "name", "ci").map(|s| s["disabled"].clone()),
        Some(Value::Bool(true)),
        "{}",
        pretty(&shown)
    );
    let resp = ci_api.get(&tree_path);
    assert!(
        matches!(resp.status, 401 | 403),
        "disabled service account is refused\n{resp}"
    );
    c.commit(&[("build.txt", "build 2\n")], "ci: build 2");
    let before = server_tip(&alice_api, "alice", TREE, "main");
    c.wt().arg("push").fails();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), before);

    // Re-enabled: works again.
    alice
        .wt()
        .args(["sa", "update", "alice", "ci", "--enable"])
        .ok();
    ci_api.get(&tree_path).assert_ok();
    let tip = c.head_id();
    c.push();
    assert_eq!(server_tip(&alice_api, "alice", TREE, "main"), Some(tip));

    // --- deleting the service account removes its keys
    alice.wt().args(["sa", "delete", "alice", "ci"]).confirmed();
    let resp = ci_api.get(&tree_path);
    assert!(
        matches!(resp.status, 401 | 403),
        "deleted service account's key is invalid\n{resp}"
    );
    let list = wt_json(&alice, &["sa", "list", "alice"]);
    assert!(find_by(&list, "name", "ci").is_none(), "{}", pretty(&list));
}
