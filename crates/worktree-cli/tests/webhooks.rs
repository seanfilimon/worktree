//! End-to-end tests of `wt webhook` (ENGINEERING_PLAN §14.2): the real
//! `wt` binary against an in-process fake REST server serving the §14.2
//! JSON shapes.

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

const TOKEN: &str = "wt_test_token";

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    uri: String,
    body: Vec<u8>,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
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

    fn on(&self, method: &str, uri: &str, status: u16, body: Value) -> &Self {
        self.lock()
            .routes
            .insert((method.to_string(), uri.to_string()), (status, body));
        self
    }

    fn request(&self, method: &str, uri: &str) -> Seen {
        let seen = self.lock().seen.clone();
        let matching: Vec<Seen> = seen
            .iter()
            .filter(|s| s.method == method && s.uri == uri)
            .cloned()
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected one {method} {uri}, saw {seen:#?}"
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
        body: body.to_vec(),
    });
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some((204, _)) => StatusCode::NO_CONTENT.into_response(),
        Some((status, body)) => {
            (StatusCode::from_u16(*status).unwrap(), Json(body.clone())).into_response()
        }
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
                "[servers.\"{}\"]\nusername = \"ada\"\ntoken = \"{TOKEN}\"\n",
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

    fn ok(&self, args: &[&str]) -> String {
        let out = self.wt(args).output().unwrap();
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn fails(&self, args: &[&str], code: i32) -> String {
        let out = self.wt(args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(code),
            "wt {args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }
}

const TENANT_HOOKS: &str = "/api/v1/tenants/acme/webhooks";
const TREE_HOOKS: &str = "/api/v1/trees/acme/web/webhooks";

fn hook_json(id: &str, active: bool) -> Value {
    json!({
        "id": id, "tenant_id": "t-acme", "url": "https://ci.example/hook",
        "events": ["branch.pushed", "mr.*"], "active": active, "has_secret": true,
        "created_by": "ada", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
        "failure_count": 0
    })
}

fn delivery_json(id: &str, ok: bool) -> Value {
    json!({
        "id": id, "webhook_id": "h1", "event": "ping", "attempts": 1,
        "status_code": if ok { 200 } else { 500 }, "response": "ok",
        "error": if ok { "" } else { "receiver answered HTTP 500" },
        "duration_ms": 7, "succeeded": ok, "created_at": "2026-01-01T00:00:00Z",
        "payload": {"id": id, "event": "ping", "tenant": "acme", "payload": {"message": "hi"}}
    })
}

#[test]
fn create_list_show_update_delete() {
    let sb = Sandbox::new();
    let mut created = hook_json("h1", true);
    created["secret"] = json!("whsec_abc");
    sb.server
        .on("POST", TENANT_HOOKS, 201, created)
        .on(
            "GET",
            TENANT_HOOKS,
            200,
            json!({"webhooks": [hook_json("h1", true)]}),
        )
        .on(
            "GET",
            &format!("{TREE_HOOKS}/h1"),
            200,
            hook_json("h1", false),
        )
        .on(
            "PATCH",
            &format!("{TREE_HOOKS}/h1"),
            200,
            hook_json("h1", true),
        )
        .on("DELETE", &format!("{TENANT_HOOKS}/h1"), 204, Value::Null);

    let out = sb.ok(&[
        "webhook",
        "create",
        "https://ci.example/hook",
        "--event",
        "branch.pushed,mr.*",
        "--description",
        "CI",
        "--tenant",
        "acme",
    ]);
    assert!(out.contains("Created webhook h1 on tenant acme"), "{out}");
    assert!(out.contains("secret  whsec_abc"), "{out}");
    assert_eq!(
        sb.server.request("POST", TENANT_HOOKS).json(),
        json!({"url": "https://ci.example/hook", "events": ["branch.pushed", "mr.*"], "description": "CI", "active": true})
    );

    let out = sb.ok(&["webhook", "list", "--tenant", "acme"]);
    assert!(
        out.contains("h1  https://ci.example/hook  branch.pushed,mr.*  active"),
        "{out}"
    );
    let json: Value =
        serde_json::from_str(&sb.ok(&["--json", "webhook", "list", "--tenant", "acme"])).unwrap();
    assert_eq!(json["webhooks"][0]["id"], "h1");

    let out = sb.ok(&["webhook", "show", "h1", "--tree", "acme/web"]);
    assert!(out.starts_with("h1  [inactive]"), "{out}");
    assert!(out.contains("tree acme/web"), "{out}");

    sb.ok(&[
        "webhook", "update", "h1", "--tree", "acme/web", "--active", "-e", "*",
    ]);
    assert_eq!(
        sb.server
            .request("PATCH", &format!("{TREE_HOOKS}/h1"))
            .json(),
        json!({"events": ["*"], "active": true})
    );
    let out = sb.fails(&["webhook", "update", "h1", "--tree", "acme/web"], 2);
    assert!(out.contains("nothing to change"), "{out}");

    let out = sb.ok(&["webhook", "delete", "h1", "--tenant", "acme", "--yes"]);
    assert!(out.contains("Deleted webhook h1"), "{out}");
    sb.server.request("DELETE", &format!("{TENANT_HOOKS}/h1"));
}

#[test]
fn deliveries_redeliver_and_test() {
    let sb = Sandbox::new();
    let hook = format!("{TREE_HOOKS}/h1");
    sb.server
        .on(
            "GET",
            &format!("{hook}/deliveries?limit=5"),
            200,
            json!({"deliveries": [delivery_json("d2", false), delivery_json("d1", true)]}),
        )
        .on(
            "GET",
            &format!("{hook}/deliveries/d1"),
            200,
            delivery_json("d1", true),
        )
        .on(
            "POST",
            &format!("{hook}/deliveries/d1/redeliver"),
            201,
            delivery_json("d3", true),
        )
        .on(
            "POST",
            &format!("{hook}/test"),
            201,
            delivery_json("d4", false),
        );

    let out = sb.ok(&[
        "webhook",
        "deliveries",
        "h1",
        "--tree",
        "acme/web",
        "-n",
        "5",
    ]);
    assert!(out.contains("d2  ping   failed"), "{out}");
    assert!(out.contains("d1  ping   ok"), "{out}");

    let out = sb.ok(&["webhook", "deliveries", "h1", "d1", "--tree", "acme/web"]);
    assert!(out.starts_with("d1 ping  [ok]"), "{out}");
    assert!(out.contains("Request body:"), "{out}");
    assert!(out.contains("\"message\": \"hi\""), "{out}");

    let out = sb.ok(&["webhook", "redeliver", "h1", "d1", "--tree", "acme/web"]);
    assert!(
        out.contains("ping delivered: HTTP 200 in 7 ms (delivery d3)"),
        "{out}"
    );

    // A failed test ping prints the reason and exits 1.
    let out = sb.fails(&["webhook", "test", "h1", "--tree", "acme/web"], 1);
    assert!(out.contains("failed: receiver answered HTTP 500"), "{out}");
}

#[test]
fn scope_is_required_outside_a_worktree() {
    let sb = Sandbox::new();
    let out = sb.fails(&["webhook", "list"], 2);
    assert!(out.contains("not inside a worktree"), "{out}");
    let out = sb.fails(&["webhook", "list", "--tree", "acme"], 2);
    assert!(out.contains("TENANT/TREE"), "{out}");
}
