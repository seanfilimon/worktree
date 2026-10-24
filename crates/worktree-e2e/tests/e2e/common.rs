//! Setup shared by the multi-user scenarios.

use serde_json::{json, Value};
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{Api, Repo, TestEnv, User};

/// Tree name used by the server scenarios.
pub const TREE: &str = "proj";

/// `alice` registers, creates `proj` with an initial snapshot, adds
/// `origin` (tenant = her personal tenant) and pushes `main`.
pub fn publish_tree(env: &TestEnv, owner: &User, files: &[(&str, &str)]) -> Repo {
    owner.register();
    let repo = owner.init(TREE);
    repo.commit(files, "initial");
    repo.add_remote("origin", &env.url(), &owner.name);
    repo.push();
    repo
}

/// `PUT /api/v1/tenants/{tenant}/members/{username} {role}` as `api`.
pub fn add_member(api: &Api, tenant: &str, username: &str, role: &str) -> Value {
    let v = api
        .put(
            &format!("/api/v1/tenants/{tenant}/members/{username}"),
            json!({ "role": role }),
        )
        .ok_json();
    let members = api
        .get(&format!("/api/v1/tenants/{tenant}/members"))
        .ok_json();
    let found = find_objects(&members, &["account", "role"])
        .into_iter()
        .any(|m| {
            m["account"].get("username").and_then(Value::as_str) == Some(username)
                && m.get("role").and_then(Value::as_str) == Some(role)
        });
    assert!(
        found,
        "{username} not listed as {role} of {tenant}:\n{}",
        pretty(&members)
    );
    v
}

/// Server-side branch tip via `GET /api/v1/trees/{tenant}/{tree}/branches`
/// (None if the branch does not exist or is empty).
pub fn server_tip(api: &Api, tenant: &str, tree: &str, branch: &str) -> Option<String> {
    let v = api
        .get(&format!("/api/v1/trees/{tenant}/{tree}/branches"))
        .ok_json();
    find_objects(&v, &["name", "tip"])
        .into_iter()
        .find(|b| b.get("name").and_then(Value::as_str) == Some(branch))
        .map(|b| str_field(b, "tip"))
        .filter(|t| !t.is_empty())
}

/// Staged entries (`files_changed` records) whose author mentions `who`.
pub fn staged_by<'a>(v: &'a Value, who: &str) -> Vec<&'a Value> {
    find_objects(v, &["files_changed"])
        .into_iter()
        .filter(|s| {
            s.get("author")
                .and_then(Value::as_str)
                .map(|a| a.to_lowercase().contains(&who.to_lowercase()))
                .unwrap_or(false)
        })
        .collect()
}
