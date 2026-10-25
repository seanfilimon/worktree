//! Helpers shared by the v0.4 IAM scenarios (ENGINEERING_PLAN §12).

use std::path::PathBuf;

use serde_json::Value;
use worktree_e2e::json::{all_strings, find_objects, pretty};
use worktree_e2e::{fsutil, Api, CmdOutput, Repo, TestEnv, User};

use crate::common::{server_tip, TREE};

/// `tenant/proj` — the CLI's `TREE` argument form.
pub fn tree_ref(tenant: &str) -> String {
    format!("{tenant}/{TREE}")
}

/// `wt --json <args>` as `user` (in the user's working area, which is not a
/// worktree, so the server is the user's only saved login).
#[track_caller]
pub fn wt_json(user: &User, args: &[&str]) -> Value {
    user.wt().args(args).json()
}

/// REST client for the env's server using a raw bearer token.
pub fn api_with_token(env: &TestEnv, who: &str, token: &str) -> Api {
    env.server().api().with_token(who, token)
}

/// REST client using the token `wt login` saved for `user`.
#[track_caller]
pub fn cli_api(env: &TestEnv, user: &User) -> Api {
    let token = user
        .saved_token()
        .unwrap_or_else(|| panic!("{} has no saved credential", user.name));
    api_with_token(env, &format!("{} (cli)", user.name), &token)
}

/// Write `value` as pretty JSON to `<env>/files/<name>` and return the path.
#[track_caller]
pub fn json_file(env: &TestEnv, name: &str, value: &Value) -> PathBuf {
    let path = env.path("files").join(name);
    fsutil::write(&path, pretty(value));
    path
}

/// Whether any string (key or value) anywhere in `v` equals `s`.
pub fn mentions(v: &Value, s: &str) -> bool {
    all_strings(v).iter().any(|x| x == s)
}

/// Username of a `{account, role}` / `{username}` style record.
pub fn username_of(v: &Value) -> Option<&str> {
    v.get("account")
        .and_then(|a| a.get("username"))
        .or_else(|| v.get("username"))
        .and_then(Value::as_str)
}

/// The role `username` has in a members listing (`[{account, role}]`).
pub fn member_role(members: &Value, username: &str) -> Option<String> {
    find_objects(members, &["account", "role"])
        .into_iter()
        .find(|m| username_of(m) == Some(username))
        .and_then(|m| m.get("role").and_then(Value::as_str).map(String::from))
}

/// Tenant members over REST.
#[track_caller]
pub fn rest_member_role(api: &Api, tenant: &str, username: &str) -> Option<String> {
    let v = api
        .get(&format!("/api/v1/tenants/{tenant}/members"))
        .ok_json();
    member_role(&v, username)
}

/// The grant for (`subject_type`, `subject`) in a grants listing.
pub fn find_grant<'a>(grants: &'a Value, subject_type: &str, subject: &str) -> Option<&'a Value> {
    find_objects(grants, &["subject_type", "subject", "role"])
        .into_iter()
        .find(|g| {
            g["subject_type"].as_str() == Some(subject_type)
                && g["subject"].as_str() == Some(subject)
        })
}

/// String values of the array field `key` of the first object having it.
pub fn string_list(v: &Value, key: &str) -> Vec<String> {
    find_objects(v, &[key])
        .into_iter()
        .find_map(|o| o.get(key).and_then(Value::as_array))
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Record with `field == value` among objects having `field`.
pub fn find_by<'a>(v: &'a Value, field: &str, value: &str) -> Option<&'a Value> {
    find_objects(v, &[field])
        .into_iter()
        .find(|o| o.get(field).and_then(Value::as_str) == Some(value))
}

/// Whether a failed command's output reads like an authorization refusal.
pub fn looks_denied(out: &CmdOutput) -> bool {
    [
        "denied",
        "access",
        "permission",
        "forbidden",
        "not allowed",
        "policy",
        "unauthorized",
        "unauthenticated",
        "403",
        "401",
    ]
    .iter()
    .any(|n| out.mentions(n))
}

/// `wt push [--branch B]` must be rejected for lack of access and leave the
/// server's branch tip unchanged.
#[track_caller]
pub fn assert_push_denied(repo: &Repo, api: &Api, tenant: &str, branch: Option<&str>) {
    let name = branch.unwrap_or("main");
    let before = server_tip(api, tenant, TREE, name);
    let mut cmd = repo.wt().arg("push");
    if let Some(b) = branch {
        cmd = cmd.args(["--branch", b]);
    }
    let out = cmd.fails();
    assert!(
        looks_denied(&out),
        "push rejection should explain the missing access\n{out}"
    );
    assert_eq!(
        server_tip(api, tenant, TREE, name),
        before,
        "server branch {name} must be unchanged after a denied push"
    );
}

/// An HTTP status that means "not allowed" (403, or 404 for hidden
/// resources, or 401 for an invalid credential).
pub fn is_refusal(status: u16) -> bool {
    matches!(status, 401 | 403 | 404)
}

/// The value of `effect`/`allowed` in a simulator Decision.
#[track_caller]
pub fn decision_allowed(decision: &Value) -> bool {
    find_objects(decision, &["allowed", "effect"])
        .first()
        .and_then(|d| d["allowed"].as_bool())
        .unwrap_or_else(|| panic!("not a Decision:\n{}", pretty(decision)))
}

/// The Decision object (`{allowed, effect, reasons}`).
#[track_caller]
pub fn decision(v: &Value) -> Value {
    find_objects(v, &["allowed", "effect"])
        .first()
        .map(|d| (*d).clone())
        .unwrap_or_else(|| panic!("not a Decision:\n{}", pretty(v)))
}

/// Whether a Decision has a reason from `source` with `effect`.
pub fn has_reason(decision: &Value, source: &str, effect: &str) -> bool {
    find_objects(decision, &["source", "effect"])
        .into_iter()
        .any(|r| r["source"].as_str() == Some(source) && r["effect"].as_str() == Some(effect))
}

/// `xxxx-xxxx` recovery codes printed in `text` (distinct, in order). Words
/// with more dashes (dates, UUIDs) do not match.
pub fn recovery_codes(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        let parts: Vec<&str> = word.split('-').collect();
        let is_code = parts.len() == 2
            && parts
                .iter()
                .all(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_alphanumeric()));
        if is_code && !out.iter().any(|c| c == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// A random password satisfying any reasonable policy.
pub fn new_password() -> String {
    format!("Nw-{}-7q", worktree_e2e::random_hex(16))
}
