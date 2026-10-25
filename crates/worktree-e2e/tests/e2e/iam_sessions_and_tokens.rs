//! Sessions and scoped API tokens (§12.2, §12.3): `wt session list` marks
//! the CLI's own session, single sessions can be revoked, `revoke-others`
//! keeps only the current one, and a token scoped to `tree:read` can read
//! but not write.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty};
use worktree_e2e::{require_e2e, Api, TestEnv, User};

use crate::common::{publish_tree, TREE};
use crate::iam_common::{api_with_token, assert_push_denied, find_by, string_list, wt_json};

/// Session records (objects with `current`) of `wt --json session list`.
fn sessions(user: &User) -> Vec<Value> {
    let v = wt_json(user, &["session", "list"]);
    find_objects(&v, &["id", "current"])
        .into_iter()
        .cloned()
        .collect()
}

/// Id of the session `api` itself uses (`current` in its own listing).
#[track_caller]
fn own_session_id(api: &Api) -> String {
    let v = api.get("/api/v1/auth/sessions").ok_json();
    find_objects(&v, &["id", "current"])
        .into_iter()
        .find(|s| s["current"] == true)
        .and_then(|s| s["id"].as_str().map(String::from))
        .unwrap_or_else(|| panic!("no current session in\n{}", pretty(&v)))
}

#[test]
fn sessions_and_tokens() {
    require_e2e!();
    let env = TestEnv::with_server("sessions_and_tokens");
    let frank = env.user("frank");
    publish_tree(&env, &frank, &[("README.md", "# sessions\n")]);
    let tree_path = format!("/api/v1/trees/frank/{TREE}");

    // Two more sessions (e.g. other machines).
    let other1 = frank.api();
    let other2 = frank.api();

    // --- list: the CLI's session is current
    let list = sessions(&frank);
    assert!(list.len() >= 3, "three sessions at least:\n{list:#?}");
    let current: Vec<&Value> = list.iter().filter(|s| s["current"] == true).collect();
    assert_eq!(current.len(), 1, "exactly one current session:\n{list:#?}");
    assert!(
        current[0].get("created_at").is_some(),
        "sessions show when they were created:\n{list:#?}"
    );
    if let Some(ip) = current[0].get("last_ip").and_then(Value::as_str) {
        assert!(ip.contains("127.0.0.1"), "last IP of the CLI session: {ip}");
    }
    let cli_session = current[0]["id"].as_str().expect("session id").to_string();
    let id1 = own_session_id(&other1);
    assert_ne!(id1, cli_session);
    assert!(list.iter().any(|s| s["id"] == id1.as_str()));

    // --- revoke one
    frank.wt().args(["session", "revoke", &id1]).confirmed();
    other1.get("/api/v1/auth/me").assert_status(401);
    other2.get("/api/v1/auth/me").assert_ok();
    assert!(
        sessions(&frank).iter().all(|s| s["id"] != id1.as_str()),
        "revoked session is gone"
    );

    // --- scoped API token
    let created = wt_json(
        &frank,
        &[
            "token",
            "create",
            "ci-read",
            "--ttl-hours",
            "2",
            "--scope",
            "tree:read",
        ],
    );
    let token = created["token"]
        .as_str()
        .unwrap_or_else(|| panic!("token create:\n{}", pretty(&created)))
        .to_string();
    let token_id = created["api_token"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("token create:\n{}", pretty(&created)))
        .to_string();
    assert_eq!(
        string_list(&created["api_token"], "scopes"),
        vec!["tree:read".to_string()],
        "{}",
        pretty(&created)
    );
    let tokens = wt_json(&frank, &["token", "list"]);
    let listed = find_by(&tokens, "id", &token_id)
        .unwrap_or_else(|| panic!("token list:\n{}", pretty(&tokens)));
    assert_eq!(listed["name"], "ci-read");
    assert!(
        !serde_json::to_string(&tokens).unwrap().contains(&token),
        "token list must not reveal the raw token"
    );

    let scoped = api_with_token(&env, "frank (tree:read)", &token);
    scoped.get(&tree_path).assert_ok();
    let r = scoped.post(
        "/api/v1/tenants/frank/trees",
        json!({"name": "other", "description": "", "visibility": "private", "default_branch": "main"}),
    );
    r.assert_error(403, "forbidden");
    let r = scoped.patch(
        &tree_path,
        json!({"description": "changed by a read token"}),
    );
    assert_eq!(r.status, 403, "scoped token may not write\n{r}");

    // ... and cannot push.
    let ci = env.user("frank-ci");
    ci.save_token("frank", &token);
    let c = ci.clone_tree("frank", TREE, TREE);
    c.commit(&[("ci.txt", "nope\n")], "ci: read-only push");
    assert_push_denied(&c, &frank.api(), "frank", None);

    // --- revoke all other sessions
    let before = sessions(&frank).len();
    let out = frank
        .wt()
        .args(["--json", "session", "revoke-others"])
        .confirmed();
    let revoked = out.json()["revoked"].as_u64();
    assert!(
        revoked.is_some_and(|n| n >= 1 && n as usize == before - 1),
        "revoked count ({revoked:?}) = other sessions ({})\n{out}",
        before - 1
    );
    other2.get("/api/v1/auth/me").assert_status(401);
    let left = sessions(&frank);
    assert_eq!(
        left.len(),
        1,
        "only the current session remains:\n{left:#?}"
    );
    assert_eq!(left[0]["id"], cli_session.as_str());
    assert_eq!(left[0]["current"], true);
    // API tokens are not sessions.
    scoped.get(&tree_path).assert_ok();

    // --- revoke the token
    frank.wt().args(["token", "revoke", &token_id]).confirmed();
    scoped.get(&tree_path).assert_status(401);
    let tokens = wt_json(&frank, &["token", "list"]);
    assert!(
        find_by(&tokens, "id", &token_id).is_none(),
        "revoked token is not listed:\n{}",
        pretty(&tokens)
    );
}
