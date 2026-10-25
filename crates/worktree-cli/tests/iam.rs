//! HTTP-level tests of the IAM commands (ENGINEERING_PLAN §12.4 [cli-iam])
//! against an in-process fake REST server serving the §12.3 JSON shapes.
//!
//! Each test registers canned responses, runs the real `wt` binary with a
//! private `WT_HOME` holding a saved login, and checks both the requests
//! the CLI sent (method, path + query, body, bearer token) and its output.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use assert_cmd::Command;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "wt_test_token";

/// One request as the fake server saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    /// Path plus query string.
    uri: String,
    auth: Option<String>,
    body: Value,
}

#[derive(Default)]
struct FakeState {
    routes: HashMap<(String, String), (u16, Value)>,
    seen: Vec<Seen>,
}

/// A REST server on 127.0.0.1 running on its own thread (the `wt` process
/// is driven synchronously by the test).
struct FakeRest {
    url: String,
    state: Arc<Mutex<FakeState>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl FakeRest {
    fn start() -> Self {
        let state = Arc::new(Mutex::new(FakeState::default()));
        let app = Router::new().fallback(handle).with_state(state.clone());
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (shutdown, stop) = tokio::sync::oneshot::channel::<()>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                addr_tx.send(listener.local_addr().expect("addr")).unwrap();
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stop.await;
                    })
                    .await
                    .expect("serve");
            });
        });
        let addr = addr_rx.recv().expect("server address");
        Self {
            url: format!("http://{addr}"),
            state,
            shutdown: Some(shutdown),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Answer `method uri` (path plus query) with `status` and `body`.
    fn on(&self, method: &str, uri: &str, status: u16, body: Value) -> &Self {
        self.lock()
            .routes
            .insert((method.to_string(), uri.to_string()), (status, body));
        self
    }

    fn seen(&self) -> Vec<Seen> {
        self.lock().seen.clone()
    }

    /// The only request made to `method uri`.
    fn request(&self, method: &str, uri: &str) -> Seen {
        let matching: Vec<Seen> = self
            .seen()
            .into_iter()
            .filter(|s| s.method == method && s.uri == uri)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected one {method} {uri}, saw {:#?}",
            self.seen()
        );
        matching.into_iter().next().unwrap()
    }
}

impl Drop for FakeRest {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle(
    State(state): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let uri = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.seen.push(Seen {
        method: method.to_string(),
        uri: uri.clone(),
        auth: headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some((204, _)) => StatusCode::NO_CONTENT.into_response(),
        Some((status, body)) => (
            StatusCode::from_u16(*status).unwrap(),
            Json(body.clone()),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": format!("no route {method} {uri}")}})),
        )
            .into_response(),
    }
}

/// A private `WT_HOME` (optionally logged in to the fake server) and work dir.
struct Sandbox {
    home: TempDir,
    dir: TempDir,
    server: FakeRest,
}

impl Sandbox {
    fn logged_in() -> Self {
        let sb = Self::anonymous();
        std::fs::write(
            sb.home.path().join("credentials.toml"),
            format!(
                "[servers.\"{}\"]\nusername = \"ada\"\ntoken = \"{TOKEN}\"\n",
                sb.server.url
            ),
        )
        .unwrap();
        sb
    }

    fn anonymous() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            dir: tempfile::tempdir().unwrap(),
            server: FakeRest::start(),
        }
    }

    fn wt(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(self.dir.path())
            .env("WT_HOME", self.home.path())
            .env("WT_SERVER", &self.server.url)
            .env("NO_COLOR", "1")
            .env_remove("WT_PASSWORD")
            .env_remove("WT_MFA_CODE")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    /// Run, require success, return stdout.
    fn ok(&self, args: &[&str]) -> String {
        self.ok_stdin(args, "")
    }

    fn ok_stdin(&self, args: &[&str], stdin: &str) -> String {
        let out = self.wt(args).write_stdin(stdin).output().unwrap();
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        serde_json::from_str(&self.ok(&full)).expect("one JSON document")
    }

    fn write(&self, name: &str, contents: &str) -> String {
        let path = self.dir.path().join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().into_owned()
    }
}

fn credentials(home: &Path) -> String {
    std::fs::read_to_string(home.join("credentials.toml")).unwrap_or_default()
}

const TS: &str = "2026-10-25T12:00:00Z";

#[test]
fn team_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "GET",
        "/api/v1/tenants/acme/teams",
        200,
        json!([{"id": "t1", "tenant_id": "x", "slug": "core", "name": "Core team",
                "description": "Platform", "created_at": TS, "updated_at": TS}]),
    )
    .on(
        "POST",
        "/api/v1/tenants/acme/teams",
        201,
        json!({"id": "t2", "slug": "web", "name": "web", "description": ""}),
    )
    .on(
        "GET",
        "/api/v1/tenants/acme/teams/core",
        200,
        json!({"team": {"id": "t1", "slug": "core", "name": "Core team", "description": "", "created_at": TS},
               "members": [{"account": {"id": "a1", "username": "ada"}, "role": "maintainer"}],
               "grants": [{"tree_id": "tr", "subject_type": "team", "subject_id": "t1",
                           "subject": "core", "role": "developer", "created_by": "ada", "created_at": TS}]}),
    )
    .on(
        "PUT",
        "/api/v1/tenants/acme/teams/core/members/bob",
        200,
        json!({"account": {"username": "bob"}, "role": "member"}),
    )
    .on("DELETE", "/api/v1/tenants/acme/teams/core", 204, Value::Null);

    let out = sb.ok(&["team", "list", "acme"]);
    assert!(
        out.starts_with("SLUG  NAME       DESCRIPTION  CREATED\n"),
        "{out}"
    );
    assert!(out.contains("core  Core team  Platform"), "{out}");
    assert_eq!(
        s.request("GET", "/api/v1/tenants/acme/teams")
            .auth
            .as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );

    let out = sb.ok(&["team", "create", "acme", "web"]);
    assert!(out.contains("Created team acme/web"), "{out}");
    assert_eq!(
        s.request("POST", "/api/v1/tenants/acme/teams").body,
        json!({"slug": "web", "name": "web", "description": ""})
    );

    let out = sb.ok(&["team", "show", "acme", "core"]);
    assert!(
        out.contains("Members:\nUSERNAME  ROLE\nada       maintainer\n"),
        "{out}"
    );
    assert!(out.contains("core     team  developer  ada"), "{out}");

    sb.ok(&["team", "add", "acme", "core", "bob"]);
    assert_eq!(
        s.request("PUT", "/api/v1/tenants/acme/teams/core/members/bob")
            .body,
        json!({"role": "member"})
    );

    // Destructive without --yes and without a terminal: refused, nothing sent.
    sb.wt(&["team", "delete", "acme", "core"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--yes"));
    assert!(s.seen().iter().all(|r| r.method != "DELETE"));
    let out = sb.ok(&["team", "delete", "acme", "core", "--yes"]);
    assert!(out.contains("Deleted team acme/core"), "{out}");
    s.request("DELETE", "/api/v1/tenants/acme/teams/core");

    // --json prints the server JSON untouched.
    let v = sb.json(&["team", "list", "acme"]);
    assert_eq!(v[0]["tenant_id"], "x");
}

#[test]
fn role_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "GET",
        "/api/v1/tenants/acme/roles",
        200,
        json!([{"name": "owner", "description": "Everything", "permissions": ["*"], "builtin": true},
               {"name": "auditor", "description": "Reads audit", "permissions": ["audit:read"], "builtin": false}]),
    )
    .on(
        "POST",
        "/api/v1/tenants/acme/roles",
        201,
        json!({"name": "auditor", "description": "", "permissions": ["audit:read", "tree:read"]}),
    )
    .on(
        "PATCH",
        "/api/v1/tenants/acme/roles/auditor",
        200,
        json!({"name": "auditor", "permissions": ["tree:*"]}),
    )
    .on(
        "DELETE",
        "/api/v1/tenants/acme/roles/auditor",
        409,
        json!({"error": {"code": "conflict", "message": "role is assigned"}}),
    );

    let out = sb.ok(&["role", "list", "acme"]);
    assert!(out.contains("owner    yes       *"), "{out}");
    assert!(
        out.contains("auditor  -         audit:read   Reads audit"),
        "{out}"
    );

    let out = sb.ok(&[
        "role",
        "create",
        "acme",
        "auditor",
        "--permission",
        "audit:read",
        "--permission",
        "tree:read",
    ]);
    assert!(
        out.contains("Created role auditor in acme (2 permissions)"),
        "{out}"
    );
    assert_eq!(
        s.request("POST", "/api/v1/tenants/acme/roles").body,
        json!({"name": "auditor", "description": "", "permissions": ["audit:read", "tree:read"]})
    );

    sb.ok(&[
        "role",
        "update",
        "acme",
        "auditor",
        "--permission",
        "tree:*",
    ]);
    assert_eq!(
        s.request("PATCH", "/api/v1/tenants/acme/roles/auditor")
            .body,
        json!({"permissions": ["tree:*"]})
    );

    sb.wt(&["role", "delete", "acme", "auditor", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("role is assigned"));
}

#[test]
fn policy_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    let policy = json!({
        "id": "p1", "tenant_id": "x", "name": "no-friday", "description": "", "enabled": false,
        "statements": [{"effect": "deny", "actions": ["branch:push"], "resources": ["wt:branch/acme/*/main"],
                        "principals": ["*"], "conditions": {}}],
        "version": 1, "created_by": "ada", "created_at": TS, "updated_at": TS
    });
    let mut enabled = policy.clone();
    enabled["enabled"] = json!(true);
    enabled["version"] = json!(2);
    s.on("POST", "/api/v1/tenants/acme/policies", 201, policy.clone())
        .on("GET", "/api/v1/tenants/acme/policies/p1", 200, policy.clone())
        .on("PUT", "/api/v1/tenants/acme/policies/p1", 200, enabled)
        .on(
            "POST",
            "/api/v1/tenants/acme/policies/validate",
            200,
            json!({"valid": false, "errors": [{"statement": 0, "field": "effect", "message": "must be allow or deny"}]}),
        )
        .on("GET", "/api/v1/tenants/acme/policies", 200, json!([policy]));

    let doc = r#"{"name": "no-friday", "statements": [{"effect": "deny", "actions": ["branch:push"],
                  "resources": ["wt:branch/acme/*/main"], "principals": ["*"]}]}"#;
    let file = sb.write("policy.json", doc);
    let out = sb.ok(&["policy", "create", "acme", "--file", &file]);
    assert!(out.contains("Created policy no-friday (p1)"), "{out}");
    let sent = s.request("POST", "/api/v1/tenants/acme/policies").body;
    assert_eq!(sent["name"], "no-friday");
    assert_eq!(sent["enabled"], true);
    assert_eq!(sent["description"], "");
    assert_eq!(
        sent["statements"][0]["resources"][0],
        "wt:branch/acme/*/main"
    );

    // From stdin.
    let out = sb.ok_stdin(&["policy", "update", "acme", "p1", "--file", "-"], doc);
    assert!(out.contains("now version 2"), "{out}");
    assert_eq!(
        s.request("PUT", "/api/v1/tenants/acme/policies/p1").body["name"],
        "no-friday"
    );

    // Invalid statements: the errors table, then exit 1.
    sb.wt(&["policy", "validate", "acme", "-f", &file])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "0          effect  must be allow or deny",
        ))
        .stderr(predicate::str::contains("is invalid"));
    assert_eq!(
        s.request("POST", "/api/v1/tenants/acme/policies/validate")
            .body["statements"][0]["effect"],
        "deny"
    );

    let out = sb.ok(&["policy", "list", "acme"]);
    assert!(
        out.contains("p1  no-friday  -        1           1"),
        "{out}"
    );
    let out = sb.ok(&["policy", "show", "acme", "p1"]);
    assert!(out.contains("Statement 1: DENY"), "{out}");

    // A bad file fails before any request.
    let before = s.seen().len();
    let bad = sb.write("bad.json", "{\"statements\": 3}");
    sb.wt(&["policy", "create", "acme", "-f", &bad])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid policy file"));
    assert_eq!(s.seen().len(), before);
}

#[test]
fn policy_enable_resends_the_full_policy() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    let policy = json!({"id": "p1", "name": "office-hours", "description": "d", "enabled": false,
                        "statements": [{"effect": "allow", "actions": ["tree:read"], "resources": ["*"],
                                        "principals": ["authenticated"], "conditions": {"require_mfa": true}}],
                        "version": 4});
    let mut after = policy.clone();
    after["enabled"] = json!(true);
    s.on("GET", "/api/v1/tenants/acme/policies/p1", 200, policy)
        .on("PUT", "/api/v1/tenants/acme/policies/p1", 200, after);
    let out = sb.ok(&["policy", "enable", "acme", "p1"]);
    assert!(out.contains("Enabled policy office-hours (p1)"), "{out}");
    assert_eq!(
        s.request("PUT", "/api/v1/tenants/acme/policies/p1").body,
        json!({"name": "office-hours", "description": "d", "enabled": true,
               "statements": [{"effect": "allow", "actions": ["tree:read"], "resources": ["*"],
                               "principals": ["authenticated"], "conditions": {"require_mfa": true}}]})
    );
}

#[test]
fn grant_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "GET",
        "/api/v1/trees/acme/web/grants",
        200,
        json!([{"tree_id": "t", "subject_type": "user", "subject_id": "a2", "subject": "bob",
                "role": "maintainer", "created_by": "ada", "created_at": TS}]),
    )
    .on(
        "PUT",
        "/api/v1/trees/acme/web/grants",
        200,
        json!({"subject_type": "service_account", "subject": "ci", "role": "developer"}),
    )
    .on(
        "DELETE",
        "/api/v1/trees/acme/web/grants?subject_type=team&subject=core",
        204,
        Value::Null,
    );

    let out = sb.ok(&["grant", "list", "acme/web"]);
    assert!(out.contains("bob      user  maintainer  ada"), "{out}");

    let out = sb.ok(&[
        "grant",
        "set",
        "acme/web",
        "--sa",
        "ci",
        "--role",
        "developer",
    ]);
    assert!(
        out.contains("Granted developer to service_account ci on acme/web"),
        "{out}"
    );
    assert_eq!(
        s.request("PUT", "/api/v1/trees/acme/web/grants").body,
        json!({"subject_type": "service_account", "subject": "ci", "role": "developer"})
    );

    sb.ok(&["grant", "remove", "acme/web", "--team", "core", "--yes"]);
    s.request(
        "DELETE",
        "/api/v1/trees/acme/web/grants?subject_type=team&subject=core",
    );
}

#[test]
fn service_account_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "GET",
        "/api/v1/tenants/acme/service-accounts/ci",
        200,
        json!({"service_account": {"id": "s1", "name": "ci", "role": "developer", "disabled": false,
                                   "description": "CI", "created_by": "ada", "created_at": TS},
               "keys": [{"id": "k1", "name": "deploy", "created_at": TS, "kind": "service",
                         "scopes": ["tree:read"]}]}),
    )
    .on(
        "POST",
        "/api/v1/tenants/acme/service-accounts/ci/keys",
        201,
        json!({"token": "wts_supersecret", "api_token": {"id": "k2", "name": "deploy", "created_at": TS}}),
    )
    .on(
        "PATCH",
        "/api/v1/tenants/acme/service-accounts/ci",
        200,
        json!({"name": "ci", "role": "developer", "disabled": true}),
    )
    .on(
        "DELETE",
        "/api/v1/tenants/acme/service-accounts/ci/keys/k1",
        204,
        Value::Null,
    );

    let out = sb.ok(&["sa", "show", "acme", "ci"]);
    assert!(out.contains("status:          active"), "{out}");
    assert!(out.contains("k1  deploy  tree:read"), "{out}");

    let out = sb.ok(&[
        "sa",
        "key-create",
        "acme",
        "ci",
        "--name",
        "deploy",
        "--ttl-hours",
        "24",
        "--scope",
        "tree:read",
    ]);
    assert_eq!(out.matches("wts_supersecret").count(), 1, "{out}");
    assert!(out.contains("shown only once"), "{out}");
    assert_eq!(
        s.request("POST", "/api/v1/tenants/acme/service-accounts/ci/keys")
            .body,
        json!({"name": "deploy", "ttl_hours": 24, "scopes": ["tree:read"]})
    );

    let out = sb.ok(&["sa", "update", "acme", "ci", "--disable"]);
    assert!(out.contains("(role developer, disabled)"), "{out}");
    assert_eq!(
        s.request("PATCH", "/api/v1/tenants/acme/service-accounts/ci")
            .body,
        json!({"disabled": true})
    );

    sb.ok(&["sa", "key-delete", "acme", "ci", "k1", "--yes"]);
    s.request("DELETE", "/api/v1/tenants/acme/service-accounts/ci/keys/k1");
}

#[test]
fn token_and_session_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "POST",
        "/api/v1/auth/tokens",
        201,
        json!({"token": "wt_newsecret", "api_token": {"id": "t9", "name": "laptop", "scopes": ["tree:*"]}}),
    )
    .on(
        "GET",
        "/api/v1/auth/tokens",
        200,
        json!([{"id": "t9", "name": "laptop", "created_at": TS, "scopes": ["tree:*"]}]),
    )
    .on("DELETE", "/api/v1/auth/tokens/t9", 204, Value::Null)
    .on(
        "GET",
        "/api/v1/auth/sessions",
        200,
        json!([{"id": "s1", "name": "session", "kind": "session", "created_at": TS, "last_ip": "10.1.1.1",
                "user_agent": "wt", "mfa": true, "current": true},
               {"id": "s2", "created_at": TS, "current": false}]),
    )
    .on("DELETE", "/api/v1/auth/sessions/s2", 204, Value::Null)
    .on("POST", "/api/v1/auth/sessions/revoke-others", 200, json!({"revoked": 3}));

    let out = sb.ok(&["token", "create", "laptop", "--scope", "tree:*"]);
    assert!(
        out.contains("    wt_newsecret\n") && out.contains("shown only once"),
        "{out}"
    );
    assert_eq!(
        s.request("POST", "/api/v1/auth/tokens").body,
        json!({"name": "laptop", "scopes": ["tree:*"]})
    );
    let out = sb.ok(&["token", "list"]);
    assert!(out.contains("t9  laptop  tree:*"), "{out}");
    sb.ok(&["token", "revoke", "t9", "--yes"]);
    s.request("DELETE", "/api/v1/auth/tokens/t9");

    let out = sb.ok(&["session", "list"]);
    assert!(out.contains("s1  *"), "{out}");
    assert!(out.contains("10.1.1.1  yes  wt"), "{out}");
    sb.ok(&["session", "revoke", "s2", "-y"]);
    s.request("DELETE", "/api/v1/auth/sessions/s2");
    let out = sb.ok(&["session", "revoke-others", "--yes"]);
    assert!(out.contains("Revoked 3 other session(s)"), "{out}");
    let v = sb.json(&["session", "revoke-others", "--yes"]);
    assert_eq!(v, json!({"revoked": 3}));
}

#[test]
fn mfa_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    let url = "otpauth://totp/W0rkTree:ada?secret=JBSWY3DPEHPK3PXP&issuer=W0rkTree";
    s.on(
        "POST",
        "/api/v1/auth/mfa/totp/enroll",
        200,
        json!({"secret": "JBSWY3DPEHPK3PXP", "otpauth_url": url}),
    )
    .on(
        "POST",
        "/api/v1/auth/mfa/totp/confirm",
        200,
        json!({"recovery_codes": ["aaaa-bbbb", "cccc-dddd"]}),
    )
    .on(
        "GET",
        "/api/v1/auth/mfa",
        200,
        json!({"account_id": "a1", "totp_enabled": true, "enabled_at": TS, "recovery_codes_left": 10}),
    )
    .on("DELETE", "/api/v1/auth/mfa/totp", 204, Value::Null)
    .on(
        "POST",
        "/api/v1/auth/mfa/recovery-codes",
        200,
        json!({"recovery_codes": ["eeee-ffff"]}),
    );

    let out = sb.ok_stdin(&["mfa", "enable"], "123 456\n");
    assert!(out.contains("secret: JBSWY3DPEHPK3PXP"), "{out}");
    assert!(out.contains(url), "{out}");
    assert!(out.contains('█'), "QR code expected:\n{out}");
    assert!(out.contains("    aaaa-bbbb\n    cccc-dddd\n"), "{out}");
    assert_eq!(
        s.request("POST", "/api/v1/auth/mfa/totp/confirm").body,
        json!({"code": "123456"})
    );

    // A malformed code is rejected before confirming.
    sb.wt(&["mfa", "enable"])
        .write_stdin("12\n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("6 digits"));
    assert_eq!(
        s.seen()
            .iter()
            .filter(|r| r.uri == "/api/v1/auth/mfa/totp/confirm")
            .count(),
        1
    );

    // --json: enrollment on stderr, one document on stdout.
    let out = sb
        .wt(&["--json", "mfa", "enable"])
        .write_stdin("654321\n")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["recovery_codes"], json!(["aaaa-bbbb", "cccc-dddd"]));
    assert_eq!(v["otpauth_url"], url);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Scan this QR code"));

    let out = sb.ok(&["mfa", "status"]);
    assert!(out.contains("recovery codes left: 10"), "{out}");

    sb.ok_stdin(&["mfa", "disable", "--password-stdin"], "hunter2\n");
    assert_eq!(
        s.request("DELETE", "/api/v1/auth/mfa/totp").body,
        json!({"password": "hunter2"})
    );

    let out = sb.ok_stdin(&["mfa", "recovery-codes"], "111111\n");
    assert!(out.contains("    eeee-ffff\n"), "{out}");
    assert_eq!(
        s.request("POST", "/api/v1/auth/mfa/recovery-codes").body,
        json!({"code": "111111"})
    );
}

#[test]
fn invite_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    let invitation = json!({"id": "i1", "tenant_id": "x", "email": "bo@x.io", "role": "viewer",
                            "invited_by": "ada", "created_at": TS, "expires_at": "2999-01-01T00:00:00Z",
                            "revoked": false});
    s.on(
        "POST",
        "/api/v1/tenants/acme/invitations",
        201,
        json!({"invitation": invitation, "code": "wti_abc123"}),
    )
    .on(
        "GET",
        "/api/v1/tenants/acme/invitations",
        200,
        json!([invitation]),
    )
    .on(
        "GET",
        "/api/v1/invitations/wti_abc123",
        200,
        json!({"invitation": invitation, "tenant": {"id": "x", "slug": "acme", "name": "Acme"}}),
    )
    .on(
        "POST",
        "/api/v1/invitations/wti_abc123/accept",
        200,
        invitation,
    )
    .on(
        "DELETE",
        "/api/v1/tenants/acme/invitations/i1",
        204,
        Value::Null,
    );

    let out = sb.ok(&[
        "invite", "create", "acme", "--email", "bo@x.io", "--role", "viewer",
    ]);
    assert!(out.contains("    wti_abc123\n"), "{out}");
    assert!(
        out.contains(&format!("wt --server {} invite accept wti_abc123", s.url)),
        "{out}"
    );
    assert_eq!(
        s.request("POST", "/api/v1/tenants/acme/invitations").body,
        json!({"email": "bo@x.io", "role": "viewer"})
    );

    let out = sb.ok(&["invite", "list", "acme"]);
    assert!(out.contains("i1  bo@x.io  viewer"), "{out}");
    assert!(out.contains("pending"), "{out}");
    let out = sb.ok(&["invite", "show", "wti_abc123"]);
    assert!(out.contains("tenant:     acme (Acme)"), "{out}");
    let out = sb.ok(&["invite", "accept", "wti_abc123"]);
    assert!(out.contains("you joined as viewer"), "{out}");
    s.request("POST", "/api/v1/invitations/wti_abc123/accept");
    sb.ok(&["invite", "revoke", "acme", "i1", "--yes"]);
    s.request("DELETE", "/api/v1/tenants/acme/invitations/i1");
}

#[test]
fn iam_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    s.on(
        "GET",
        "/api/v1/iam/actions",
        200,
        json!([{"action": "tree:read", "description": "Read a tree", "area": "tree"}]),
    )
    .on(
        "GET",
        "/api/v1/iam/effective?tenant=acme&tree=web&principal=sa%3Aacme%2Fci",
        200,
        json!({"role": "developer", "team_roles": [], "grants": [], "permissions": ["tree:read", "tree:write"]}),
    )
    .on(
        "POST",
        "/api/v1/iam/simulate",
        200,
        json!({"allowed": false, "effect": "deny", "reasons": [
            {"source": "tenant_role", "detail": "developer allows tree:write", "effect": "allow"},
            {"source": "policy", "detail": "no-friday: statement 1", "effect": "deny"}]}),
    );

    let out = sb.ok(&["iam", "actions"]);
    assert!(out.contains("tree:read  tree  Read a tree"), "{out}");

    let out = sb.ok(&[
        "iam",
        "effective",
        "--tree",
        "acme/web",
        "--principal",
        "sa:acme/ci",
    ]);
    assert!(out.contains("tenant role: developer"), "{out}");
    assert!(out.contains("  tree:write\n"), "{out}");

    let out = sb.ok(&[
        "iam",
        "simulate",
        "--principal",
        "user:bob",
        "--action",
        "branch:push",
        "--tenant",
        "acme",
        "--tree",
        "acme/web",
        "--branch",
        "main",
        "--ip",
        "10.0.0.7",
        "--mfa",
    ]);
    assert!(
        out.starts_with("DENIED (deny): user:bob branch:push on acme/web branch main\n"),
        "{out}"
    );
    assert!(
        out.contains("policy       deny    no-friday: statement 1"),
        "{out}"
    );
    assert_eq!(
        s.request("POST", "/api/v1/iam/simulate").body,
        json!({"principal": {"type": "user", "name": "bob"}, "action": "branch:push", "tenant": "acme",
               "tree": "web", "branch": "main", "context": {"ip": "10.0.0.7", "mfa": true}})
    );
    let v = sb.json(&[
        "iam",
        "simulate",
        "--principal",
        "user:bob",
        "--action",
        "tree:read",
        "--tenant",
        "acme",
    ]);
    assert_eq!(v["allowed"], false);
}

#[test]
fn sso_commands() {
    let sb = Sandbox::logged_in();
    let s = &sb.server;
    let config = json!({"enabled": false, "name": "Okta", "issuer": "https://idp.example", "client_id": "c1",
                        "has_client_secret": false, "redirect_url": "https://wt/api/v1/auth/sso/callback",
                        "scopes": ["openid", "email", "profile"], "allowed_domains": [],
                        "auto_create": false, "username_claim": "preferred_username", "updated_at": TS});
    let mut saved = config.clone();
    saved["enabled"] = json!(true);
    saved["has_client_secret"] = json!(true);
    s.on("GET", "/api/v1/admin/sso", 200, config)
        .on("PUT", "/api/v1/admin/sso", 200, saved);

    let out = sb.ok(&["sso", "show"]);
    assert!(
        out.contains("issuer:          https://idp.example"),
        "{out}"
    );
    let out = sb.ok_stdin(
        &[
            "sso",
            "set",
            "--enabled",
            "true",
            "--client-secret-stdin",
            "--allowed-domain",
            "acme.io",
        ],
        "s3cret\n",
    );
    assert!(
        out.contains("SSO configuration saved.") && out.contains("(set)"),
        "{out}"
    );
    let put = s.request("PUT", "/api/v1/admin/sso").body;
    assert_eq!(put["enabled"], true);
    assert_eq!(put["client_secret"], "s3cret");
    assert_eq!(put["allowed_domains"], json!(["acme.io"]));
    assert_eq!(put["issuer"], "https://idp.example");
    assert!(put.get("has_client_secret").is_none(), "{put}");
}

#[test]
fn login_with_mfa() {
    let sb = Sandbox::anonymous();
    let s = &sb.server;
    s.on(
        "POST",
        "/api/v1/auth/login",
        200,
        json!({"mfa_required": true, "mfa_token": "mfa-1", "account": {"username": "ada"}}),
    )
    .on(
        "POST",
        "/api/v1/auth/login/mfa",
        200,
        json!({"token": "wt_session", "account": {"id": "a1", "username": "ada"}}),
    );

    // Code from --mfa-code.
    let out = sb.ok_stdin(
        &[
            "login",
            &s.url,
            "-u",
            "ada",
            "--password-stdin",
            "--mfa-code",
            "123456",
        ],
        "pw\n",
    );
    assert!(out.contains("Logged in to"), "{out}");
    assert_eq!(
        s.request("POST", "/api/v1/auth/login").body,
        json!({"username": "ada", "password": "pw"})
    );
    assert_eq!(
        s.seen().last().unwrap().body,
        json!({"mfa_token": "mfa-1", "code": "123456"})
    );
    assert!(credentials(sb.home.path()).contains("wt_session"));

    // Code from WT_MFA_CODE.
    sb.wt(&["login", &s.url, "-u", "ada", "--password-stdin"])
        .env("WT_MFA_CODE", "222222")
        .write_stdin("pw\n")
        .assert()
        .success();
    assert_eq!(s.seen().last().unwrap().body["code"], "222222");

    // Prompted: the line after the password.
    let out = sb
        .wt(&["login", &s.url, "-u", "ada", "--password-stdin"])
        .write_stdin("pw\nabcd-efgh\n")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("Two-factor code"));
    assert_eq!(s.seen().last().unwrap().body["code"], "abcd-efgh");

    // A rejected code is an authentication failure.
    s.on(
        "POST",
        "/api/v1/auth/login/mfa",
        401,
        json!({"error": {"code": "unauthenticated", "message": "invalid MFA code"}}),
    );
    sb.wt(&[
        "login",
        &s.url,
        "-u",
        "ada",
        "--password-stdin",
        "--mfa-code",
        "000000",
    ])
    .write_stdin("pw\n")
    .assert()
    .code(1)
    .stderr(predicate::str::contains("invalid MFA code"));
}

#[test]
fn login_without_mfa_is_unchanged() {
    let sb = Sandbox::anonymous();
    sb.server.on(
        "POST",
        "/api/v1/auth/login",
        200,
        json!({"token": "wt_plain", "account": {"id": "a1", "username": "ada"}}),
    );
    sb.ok_stdin(
        &["login", &sb.server.url, "-u", "ada", "--password-stdin"],
        "pw\n",
    );
    assert_eq!(sb.server.seen().len(), 1);
    assert!(credentials(sb.home.path()).contains("wt_plain"));
}

#[test]
fn password_reset() {
    let sb = Sandbox::anonymous();
    let s = &sb.server;
    s.on("POST", "/api/v1/auth/password-reset", 204, Value::Null);
    let out = sb.ok_stdin(
        &["password-reset", "wtr_code", &s.url, "--password-stdin"],
        "n3w\n",
    );
    assert!(out.contains("Password changed"), "{out}");
    let seen = s.request("POST", "/api/v1/auth/password-reset");
    assert_eq!(
        seen.body,
        json!({"code": "wtr_code", "new_password": "n3w"})
    );
    assert_eq!(seen.auth, None, "the reset is anonymous");

    s.on(
        "POST",
        "/api/v1/auth/password-reset",
        400,
        json!({"error": {"code": "bad_request", "message": "reset code expired"}}),
    );
    sb.wt(&["password-reset", "wtr_old", &s.url, "--password-stdin"])
        .write_stdin("n3w\n")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("reset code expired"));
}

#[test]
fn server_errors_and_missing_login() {
    let sb = Sandbox::anonymous();
    sb.wt(&["team", "list", "acme"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("wt login"));

    let sb = Sandbox::logged_in();
    sb.server.on(
        "GET",
        "/api/v1/tenants/acme/teams",
        403,
        json!({"error": {"code": "forbidden", "message": "iam:read required"}}),
    );
    sb.wt(&["team", "list", "acme"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("iam:read required"));
}

// --- signing (ENGINEERING_PLAN §13.6): `wt identity register|revoke` ---

#[test]
fn identity_register_and_revoke() {
    let sb = Sandbox::logged_in();
    let key = sb.json(&["identity", "generate", "laptop"]);
    let fingerprint = key["fingerprint"].as_str().unwrap().to_string();
    sb.server.on(
        "POST",
        "/api/v1/auth/keys",
        201,
        json!({"id": "key-1", "account_id": "acct-ada", "name": "laptop", "algorithm": "ed25519",
               "public_key": "AA==", "fingerprint": fingerprint, "created_at": "2026-01-02T03:04:05Z"}),
    );
    let out = sb.ok(&["identity", "register", "laptop"]);
    assert!(
        out.contains("Registered signing key laptop as key-1"),
        "{out}"
    );
    let req = sb.server.request("POST", "/api/v1/auth/keys");
    assert_eq!(
        req.auth.as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(req.body["name"], "laptop");
    assert_eq!(req.body["algorithm"], "ed25519");
    assert_eq!(req.body["public_key"].as_str().unwrap().len(), 44);

    let shown = sb.json(&["identity", "show", "laptop"]);
    assert_eq!(shown["key_id"], "key-1");
    assert_eq!(shown["server"], sb.server.url.as_str());

    // Revoke by local key name resolves the server id.
    sb.server
        .on("DELETE", "/api/v1/auth/keys/key-1", 204, json!(null));
    let out = sb.json(&["identity", "revoke", "laptop", "--yes"]);
    assert_eq!(out["id"], "key-1");
    assert_eq!(out["local_key"], "laptop");
    sb.server.request("DELETE", "/api/v1/auth/keys/key-1");
    let shown = sb.json(&["identity", "show", "laptop"]);
    assert!(shown.get("key_id").is_none(), "{shown}");

    // Conflicts surface the server message.
    sb.server.on(
        "POST",
        "/api/v1/auth/keys",
        409,
        json!({"error": {"code": "conflict", "message": "public key is already registered"}}),
    );
    sb.wt(&["identity", "register", "laptop"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already registered"));
}
// --- end signing ---
