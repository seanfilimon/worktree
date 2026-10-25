//! End-to-end tests of `wt snapshot --depend`, `wt todo`, `wt deps` and
//! `wt release` (ENGINEERING_PLAN §13.5): the real `wt` binary against an
//! in-process fake REST server serving the §13.5 JSON shapes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use assert_cmd::Command;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "wt_test_token";

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    uri: String,
    auth: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

#[derive(Clone)]
enum Reply {
    Json(u16, Value),
    Raw(Vec<u8>),
}

#[derive(Default)]
struct FakeState {
    routes: HashMap<(String, String), Reply>,
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
        self.lock().routes.insert(
            (method.to_string(), uri.to_string()),
            Reply::Json(status, body),
        );
        self
    }

    fn on_raw(&self, method: &str, uri: &str, body: &[u8]) -> &Self {
        self.lock().routes.insert(
            (method.to_string(), uri.to_string()),
            Reply::Raw(body.to_vec()),
        );
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
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let uri = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.seen.push(Seen {
        method: method.to_string(),
        uri: uri.clone(),
        auth: header("authorization"),
        content_type: header("content-type"),
        body: body.to_vec(),
    });
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some(Reply::Json(204, _)) => StatusCode::NO_CONTENT.into_response(),
        Some(Reply::Json(status, body)) => {
            (StatusCode::from_u16(*status).unwrap(), Json(body.clone())).into_response()
        }
        Some(Reply::Raw(bytes)) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes.clone(),
        )
            .into_response(),
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

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        serde_json::from_str(&self.ok(&full)).expect("one JSON document")
    }
}

const TREE: &str = "/api/v1/trees/acme/web";

fn todo_json(number: u32, status: &str) -> Value {
    json!({
        "id": format!("id-{number}"), "tenant_id": "t", "tree_id": "tree-web", "number": number,
        "target_branch": "main", "title": format!("Task {number}"), "priority": "high",
        "blocking": number == 1, "linked": false, "status": status,
        "source_tenant_id": "t", "source_tree_id": "tree-api", "source_branch": "main",
        "source_snapshot_id": "0123456789abcdef", "created_by": "a", "author": "ada",
        "assignee": if status == "in_progress" { "ada" } else { "" },
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
    })
}

#[test]
fn snapshot_records_dependencies() {
    let sb = Sandbox::new();
    sb.ok(&["init", "--name", "web"]);
    std::fs::write(sb.dir.path().join("a.txt"), "hello").unwrap();
    let snap = sb.json(&[
        "snapshot",
        "-m",
        "Use the v2 API\n\nbody",
        "--depend",
        "api@develop",
        "--todo",
        "Add v2",
        "--priority",
        "high",
        "--blocking",
        "--depend",
        "lib",
        "--depend-tenant",
        "other",
    ]);
    let deps = snap["dependencies"].as_array().expect("dependencies");
    assert_eq!(deps.len(), 2, "{snap}");
    assert_eq!(deps[0]["tree"], "api");
    assert_eq!(deps[0]["branch"], "develop");
    assert_eq!(deps[0]["title"], "Add v2");
    assert_eq!(deps[0]["priority"], "high");
    assert_eq!(deps[0]["blocking"], true);
    assert_eq!(deps[1]["tenant"], "other");
    assert_eq!(deps[1]["title"], "Use the v2 API");

    let out = sb
        .wt(&["snapshot", "-m", "x", "--todo", "orphan"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--todo must follow a --depend"));
}

#[test]
fn todo_commands() {
    let sb = Sandbox::new();
    sb.server
        .on(
            "GET",
            &format!("{TREE}/todos?status=active"),
            200,
            json!({"todos": [todo_json(1, "open"), todo_json(2, "in_progress")]}),
        )
        .on("GET", &format!("{TREE}/todos/1"), 200, todo_json(1, "open"))
        .on("POST", &format!("{TREE}/todos/1/claim"), 200, todo_json(1, "in_progress"))
        .on(
            "POST",
            &format!("{TREE}/todos/1/complete"),
            200,
            json!({"number": 1, "title": "Task 1", "status": "done", "resolved_by": "fedcba9876543210"}),
        )
        .on("POST", &format!("{TREE}/todos/2/cancel"), 200, todo_json(2, "cancelled"))
        .on("POST", &format!("{TREE}/todos"), 201, todo_json(3, "open"))
        .on(
            "GET",
            "/api/v1/tenants/acme/trees",
            200,
            json!([{"tree": {"name": "web"}}, {"tree": {"name": "api"}}]),
        )
        .on(
            "GET",
            "/api/v1/trees/acme/api/todos?status=all",
            200,
            json!({"todos": []}),
        )
        .on(
            "GET",
            &format!("{TREE}/todos?status=all"),
            200,
            json!({"todos": [todo_json(1, "done")]}),
        );

    let list = sb.ok(&["todo", "list", "acme/web"]);
    assert!(list.contains("#1    Task 1 [blocking]  high"), "{list}");
    assert!(list.contains("in_progress"), "{list}");
    assert_eq!(
        sb.server
            .request("GET", &format!("{TREE}/todos?status=active"))
            .auth
            .as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );

    let show = sb.ok(&["todo", "show", "1", "acme/web"]);
    assert!(show.starts_with("#1 Task 1  [open]"), "{show}");
    assert!(show.contains("snapshot 01234567 on main"), "{show}");

    assert!(sb
        .ok(&["todo", "claim", "1", "acme/web"])
        .contains("Claimed #1 Task 1"));
    let done = sb.ok(&["todo", "complete", "1", "acme/web", "--snapshot", "main"]);
    assert!(done.contains("Completed #1 Task 1 by fedcba98"), "{done}");
    assert_eq!(
        sb.server
            .request("POST", &format!("{TREE}/todos/1/complete"))
            .json(),
        json!({"snapshot": "main"})
    );
    assert!(sb
        .ok(&["todo", "cancel", "2", "acme/web"])
        .contains("Cancelled #2"));

    let created = sb.json(&[
        "todo",
        "create",
        "Write docs",
        "acme/web",
        "--priority",
        "low",
        "--blocking",
        "--details",
        "d",
    ]);
    assert_eq!(created["number"], 3);
    assert_eq!(
        sb.server.request("POST", &format!("{TREE}/todos")).json(),
        json!({"title": "Write docs", "details": "d", "priority": "low", "blocking": true, "target_branch": ""})
    );

    let all = sb.json(&["todo", "list", "acme/web", "--all-trees", "--status", "all"]);
    assert_eq!(all[0]["tree"], "acme/web");
    assert_eq!(all[0]["todos"][0]["status"], "done");
    assert_eq!(all[1]["tree"], "acme/api");
}

#[test]
fn deps_commands() {
    let sb = Sandbox::new();
    sb.server.on(
        "GET",
        &format!("{TREE}/dependencies?branch=dev"),
        200,
        json!({
            "branch": "dev",
            "outgoing": [{"snapshot_id": "aaaaaaaaaaaa", "dependency": {"tree": "api", "title": "Add v2"},
                          "todo": {"tenant": "acme", "tree": "api", "number": 4, "status": "open"}}],
            "incoming": [todo_json(1, "open")],
            "trees": {"tree-web": "acme/web", "tree-api": "acme/api"}
        }),
    );
    let status = sb.ok(&["deps", "status", "acme/web", "--branch", "dev"]);
    assert!(
        status.contains("acme/web dev needs from other trees:"),
        "{status}"
    );
    assert!(
        status.contains("aaaaaaaa  acme/api  Add v2  #4    open"),
        "{status}"
    );
    assert!(status.contains("#1    acme/api  Task 1"), "{status}");
    let dot = sb.ok(&[
        "deps", "graph", "acme/web", "--branch", "dev", "--format", "dot",
    ]);
    assert!(
        dot.contains("\"acme/web\" -> \"acme/api\" [label=\"#4 Add v2 (open)\"];"),
        "{dot}"
    );
    assert!(
        dot.contains("\"acme/api\" -> \"acme/web\" [label=\"#1 Task 1 (open)\"];"),
        "{dot}"
    );
}

#[test]
fn release_commands() {
    let sb = Sandbox::new();
    let rel = json!({
        "id": "r1", "tree_id": "tree-web", "tag": "release/1.0", "snapshot_id": "0123456789abcdef",
        "name": "One", "notes": "Notes", "draft": false, "prerelease": false,
        "assets": [{"name": "app.zip", "size": 3, "hash": "00", "content_type": "application/zip", "uploaded_at": ""}],
        "created_by": "a", "author": "ada", "created_at": "2026-01-01T00:00:00Z"
    });
    let rel_path = format!("{TREE}/releases/release%2F1.0");
    sb.server
        .on("GET", &format!("{TREE}/releases"), 200, json!({"releases": [rel.clone()]}))
        .on("POST", &format!("{TREE}/releases"), 201, rel.clone())
        .on("GET", &rel_path, 200, rel.clone())
        .on("PATCH", &rel_path, 200, rel.clone())
        .on("DELETE", &rel_path, 204, json!(null))
        .on(
            "PUT",
            &format!("{rel_path}/assets/notes.txt"),
            201,
            json!({"name": "notes.txt", "size": 5, "content_type": "text/plain", "uploaded_at": ""}),
        )
        .on_raw("GET", &format!("{rel_path}/assets/app.zip"), b"PK!");

    let list = sb.ok(&["release", "list", "acme/web"]);
    assert!(
        list.contains("release/1.0  One   release  01234567  1"),
        "{list}"
    );

    std::fs::write(sb.dir.path().join("NOTES.md"), "From a file").unwrap();
    sb.ok(&[
        "release",
        "create",
        "release/1.0",
        "acme/web",
        "--notes-file",
        "NOTES.md",
        "--snapshot",
        "main",
        "--prerelease",
    ]);
    assert_eq!(
        sb.server
            .request("POST", &format!("{TREE}/releases"))
            .json(),
        json!({"tag": "release/1.0", "name": "", "notes": "From a file", "draft": false, "prerelease": true, "snapshot": "main"})
    );

    let show = sb.ok(&["release", "show", "release/1.0", "acme/web"]);
    assert!(show.contains("app.zip  3 B   application/zip"), "{show}");

    sb.ok(&[
        "release",
        "edit",
        "release/1.0",
        "acme/web",
        "--publish",
        "--name",
        "Uno",
    ]);
    assert_eq!(
        sb.server.request("PATCH", &rel_path).json(),
        json!({"name": "Uno", "draft": false})
    );
    let out = sb
        .wt(&["release", "edit", "release/1.0", "acme/web"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));

    std::fs::write(sb.dir.path().join("notes.txt"), "hello").unwrap();
    let up = sb.ok(&[
        "release",
        "upload",
        "release/1.0",
        "notes.txt",
        "--in",
        "acme/web",
    ]);
    assert!(up.contains("Uploaded notes.txt (5 B)"), "{up}");
    let seen = sb
        .server
        .request("PUT", &format!("{rel_path}/assets/notes.txt"));
    assert_eq!(seen.body, b"hello");
    assert_eq!(
        seen.content_type.as_deref(),
        Some("text/plain; charset=utf-8")
    );

    let dl = sb.ok(&[
        "release",
        "download",
        "release/1.0",
        "--in",
        "acme/web",
        "-o",
        "dist",
    ]);
    assert!(dl.contains("Downloaded"), "{dl}");
    assert_eq!(
        std::fs::read(sb.dir.path().join("dist").join("app.zip")).unwrap(),
        b"PK!"
    );

    sb.ok(&["release", "delete", "release/1.0", "acme/web", "--yes"]);
    sb.server.request("DELETE", &rel_path);
}
