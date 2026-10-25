//! CLI tests of the v0.5 access commands (ENGINEERING_PLAN §13.7):
//! `wt access validate` (local), and the REST-backed `wt tenant
//! suspend|resume|deactivate`, `wt grant set --tenant` and `wt policy
//! simulate` against an in-process fake REST server.
//! (`wt access plan|apply|pull` speak gRPC; the SDK tests in
//! worktree-sdk/tests/remote_access.rs cover that wire.)

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use assert_cmd::Command;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    uri: String,
    body: Value,
}

#[derive(Default)]
struct FakeState {
    routes: HashMap<(String, String), (u16, Value)>,
    seen: Vec<Seen>,
}

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
        Self {
            url: format!("http://{}", addr_rx.recv().expect("server address")),
            state,
            shutdown: Some(shutdown),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn on(&self, method: &str, uri: &str, status: u16, body: Value) -> &Self {
        self.lock()
            .routes
            .insert((method.to_string(), uri.to_string()), (status, body));
        self
    }

    fn requests(&self, method: &str, uri: &str) -> Vec<Seen> {
        self.lock()
            .seen
            .iter()
            .filter(|s| s.method == method && s.uri == uri)
            .cloned()
            .collect()
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
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some((status, body)) => (StatusCode::from_u16(*status).unwrap(), Json(body.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": format!("no route {method} {uri}")}})),
        )
            .into_response(),
    }
}

struct Sandbox {
    home: TempDir,
    dir: TempDir,
    server: FakeRest,
}

impl Sandbox {
    fn new() -> Self {
        let sb = Self {
            home: tempfile::tempdir().unwrap(),
            dir: tempfile::tempdir().unwrap(),
            server: FakeRest::start(),
        };
        std::fs::write(
            sb.home.path().join("credentials.toml"),
            format!(
                "[servers.\"{}\"]\nusername = \"ada\"\ntoken = \"t0k\"\n",
                sb.server.url
            ),
        )
        .unwrap();
        sb
    }

    fn wt(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(self.dir.path())
            .env("WT_HOME", self.home.path())
            .env("WT_SERVER", &self.server.url)
            .env("NO_COLOR", "1")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    fn run(&self, args: &[&str]) -> (bool, String, String) {
        let out = self.wt(args).output().unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn ok(&self, args: &[&str]) -> String {
        let (ok, out, err) = self.run(args);
        assert!(ok, "wt {args:?} failed: {out}{err}");
        out
    }

    fn write(&self, rel: &str, contents: &str) -> String {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().into_owned()
    }
}

const ROLES: &str = r#"
[[role]]
name = "release-manager"
description = "Cut releases"
permissions = ["tree:read", "release:*", "mr:merge"]
"#;

const POLICIES: &str = r#"
[[policy]]
name = "protect-main"
[[policy.statement]]
sid = "no-direct-main"
effect = "deny"
principals = ["role:developer", "tenant:partner"]
actions = ["sync:push"]
resources = ["wt:branch/acme/web/main"]
conditions = { mfa = true }
"#;

#[test]
fn access_validate_accepts_valid_files() {
    let sb = Sandbox::new();
    sb.write("cfg/roles.toml", ROLES);
    sb.write("cfg/policies.toml", POLICIES);
    let (ok, out, err) = sb.run(&["access", "validate", "--dir", "cfg"]);
    assert!(ok, "{out}{err}");
    assert!(out.contains("are valid: 1 role(s), 1 policy."), "{out}");
    assert!(
        err.contains("warning: W2101"),
        "alias warning on stderr: {err}"
    );

    let out = sb.ok(&["--json", "access", "validate", "--dir", "cfg"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["valid"], true);
    assert_eq!(v["warnings"][0]["code"], "W2101");
    assert_eq!(
        v["document"]["policies"][0]["statements"][0]["actions"][0],
        "branch:push"
    );
    assert_eq!(
        v["document"]["policies"][0]["statements"][0]["conditions"]["require_mfa"],
        true
    );
    // Only one of the files is fine too.
    sb.write("only/policies.toml", POLICIES);
    sb.ok(&["access", "validate", "--dir", "only"]);
}

#[test]
fn access_validate_reports_errors_with_codes() {
    let sb = Sandbox::new();
    sb.write(
        "bad/roles.toml",
        "[[role]]\nname = \"admin\"\npermissions = [\"tree:fly\"]\n",
    );
    sb.write("bad/policies.toml", "[[policy]\n");
    // A syntax error in one file stops validation of the documents but is
    // reported per file.
    let (ok, out, err) = sb.run(&["access", "validate", "--dir", "bad"]);
    assert!(!ok);
    assert!(out.contains("E2001 policies.toml"), "{out}");
    assert!(err.contains("error(s) in the access files"), "{err}");

    sb.write(
        "bad/policies.toml",
        "[[policy]]\nname = \"p\"\n[[policy.statement]]\neffect = \"permit\"\nactions = [\"*\"]\nresources = [\"wt:tree/acme\"]\nprincipals = [\"group:x\"]\n",
    );
    let (ok, out, _) = sb.run(&["--json", "access", "validate", "--dir", "bad"]);
    assert!(!ok);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["valid"], false);
    let codes: Vec<&str> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap())
        .collect();
    for code in ["E2009", "E2004", "E2010", "E2006", "E2005"] {
        assert!(codes.contains(&code), "{code} missing from {codes:?}");
    }

    // An empty directory is an error, and so is having no worktree.
    std::fs::create_dir_all(sb.dir.path().join("empty")).unwrap();
    let (ok, _, err) = sb.run(&["access", "validate", "--dir", "empty"]);
    assert!(!ok && err.contains("no access files"), "{err}");
    let (ok, _, err) = sb.run(&["access", "validate"]);
    assert!(!ok && err.contains("not inside a worktree"), "{err}");
    let (ok, _, err) = sb.run(&["access", "plan", "--tree-doc"]);
    assert!(!ok && err.contains("--tree-doc needs a worktree"), "{err}");
}

#[test]
fn access_plan_stops_on_local_errors_before_contacting_the_server() {
    let sb = Sandbox::new();
    sb.write("cfg/policies.toml", "[[policy]]\nname = \"a/b\"\n");
    let (ok, out, _) = sb.run(&["access", "plan", "--dir", "cfg", "--tenant", "acme"]);
    assert!(!ok);
    assert!(out.contains("E2009"), "{out}");
    assert!(sb.server.lock().seen.is_empty());
}

#[test]
fn tenant_status_commands() {
    let sb = Sandbox::new();
    let tenant = |status: &str, reason: &str| {
        json!({"id": "t1", "slug": "acme", "name": "Acme", "kind": "org", "plan": "free",
               "created_at": "2026-01-01T00:00:00Z", "status": status, "status_reason": reason})
    };
    sb.server.on(
        "PATCH",
        "/api/v1/admin/tenants/acme",
        200,
        tenant("suspended", "unpaid"),
    );
    let out = sb.ok(&["tenant", "suspend", "acme", "--reason", "unpaid"]);
    assert!(out.contains("acme is now suspended"), "{out}");
    assert!(out.contains("reason: unpaid"), "{out}");
    let seen = sb.server.requests("PATCH", "/api/v1/admin/tenants/acme");
    assert_eq!(
        seen[0].body,
        json!({"status": "suspended", "status_reason": "unpaid"})
    );

    sb.server.on(
        "PATCH",
        "/api/v1/admin/tenants/acme",
        200,
        tenant("deactivated", ""),
    );
    let out = sb.ok(&["tenant", "deactivate", "acme"]);
    assert!(out.contains("acme is now deactivated"), "{out}");
    assert_eq!(
        sb.server.requests("PATCH", "/api/v1/admin/tenants/acme")[1].body,
        json!({"status": "deactivated"})
    );

    sb.server
        .on("PATCH", "/api/v1/admin/tenants/acme", 200, tenant("", ""));
    let out = sb.ok(&["tenant", "resume", "acme"]);
    assert!(out.contains("acme is now active"), "{out}");
    assert_eq!(
        sb.server.requests("PATCH", "/api/v1/admin/tenants/acme")[2].body,
        json!({"status": "active", "status_reason": ""})
    );

    sb.server.on(
        "PATCH",
        "/api/v1/admin/tenants/acme",
        403,
        json!({"error": {"code": "forbidden", "message": "server admin required"}}),
    );
    let (ok, _, err) = sb.run(&["tenant", "suspend", "acme"]);
    assert!(!ok && err.contains("server admin required"), "{err}");

    sb.server.on(
        "GET",
        "/api/v1/tenants/acme",
        200,
        tenant("suspended", "unpaid"),
    );
    let out = sb.ok(&["tenant", "show", "acme"]);
    assert!(out.contains("suspended (unpaid)"), "{out}");
}

#[test]
fn grant_to_a_tenant() {
    let sb = Sandbox::new();
    sb.server.on(
        "PUT",
        "/api/v1/trees/acme/web/grants",
        200,
        json!({"tree_id": "t", "subject_type": "tenant", "subject_id": "t-partner", "subject": "partner", "role": "viewer"}),
    );
    let out = sb.ok(&[
        "grant", "set", "acme/web", "--tenant", "partner", "--role", "viewer",
    ]);
    assert!(
        out.contains("Granted viewer to tenant partner on acme/web"),
        "{out}"
    );
    assert_eq!(
        sb.server.requests("PUT", "/api/v1/trees/acme/web/grants")[0].body,
        json!({"subject_type": "tenant", "subject": "partner", "role": "viewer"})
    );
}

#[test]
fn policy_simulate_sends_candidates() {
    let sb = Sandbox::new();
    let file = sb.write(
        "draft.json",
        r#"{"name": "Lockdown", "statements": [{"effect": "deny", "actions": ["tree:write"], "resources": ["*"], "principals": ["*"]}]}"#,
    );
    sb.server
        .on(
            "GET",
            "/api/v1/tenants/acme/policies",
            200,
            json!([{"id": "p1", "name": "lockdown", "enabled": true, "statements": []},
                   {"id": "p2", "name": "other", "enabled": true, "statements": []}]),
        )
        .on(
            "POST",
            "/api/v1/iam/simulate",
            200,
            json!({"allowed": false, "effect": "deny", "reasons": [
                {"source": "policy", "detail": "policy \"Lockdown\" denies tree:write", "effect": "deny"}]}),
        );
    let out = sb.ok(&[
        "policy",
        "simulate",
        "acme",
        "--file",
        &file,
        "--principal",
        "user:ada",
        "--action",
        "tree:write",
        "--tree",
        "acme/web",
    ]);
    assert!(out.contains("user:ada tree:write on acme/web"), "{out}");
    assert!(out.contains("with the change:    DENIED (deny)"), "{out}");
    let sims = sb.server.requests("POST", "/api/v1/iam/simulate");
    assert_eq!(sims.len(), 2);
    assert!(sims[0].body.get("candidate_policies").is_none());
    assert_eq!(sims[0].body["tree"], "web");
    assert_eq!(
        sims[0].body["principal"],
        json!({"type": "user", "name": "ada"})
    );
    let cands = sims[1].body["candidate_policies"].as_array().unwrap();
    let names: Vec<&str> = cands.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["other", "Lockdown"],
        "same-named policy replaced"
    );

    // --only sends just the file's policies (and does not list the tenant's).
    sb.ok(&[
        "--json",
        "policy",
        "simulate",
        "acme",
        "-f",
        &file,
        "--only",
        "--principal",
        "sa:acme/ci",
        "--action",
        "tree:write",
    ]);
    assert_eq!(
        sb.server
            .requests("GET", "/api/v1/tenants/acme/policies")
            .len(),
        1
    );
    let sims = sb.server.requests("POST", "/api/v1/iam/simulate");
    assert_eq!(
        sims[3].body["candidate_policies"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        sims[3].body["principal"],
        json!({"type": "service_account", "name": "ci", "tenant": "acme"})
    );
}
