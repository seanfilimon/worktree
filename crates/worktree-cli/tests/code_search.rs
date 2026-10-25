//! End-to-end tests of `wt search code` (ENGINEERING_PLAN §14.3) and the
//! unchanged `wt search QUERY`: the real `wt` binary against an in-process
//! fake REST server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use axum::extract::State;
use axum::http::{StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;

#[derive(Default)]
struct FakeState {
    /// Replies by request path (the query string is ignored).
    routes: HashMap<String, Value>,
    /// Every request URI (path and query).
    seen: Vec<String>,
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

    fn on(&self, path: &str, body: Value) {
        self.state
            .lock()
            .unwrap()
            .routes
            .insert(path.to_string(), body);
    }

    fn seen(&self) -> Vec<String> {
        self.state.lock().unwrap().seen.clone()
    }
}

impl Drop for FakeRest {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle(State(state): State<Arc<Mutex<FakeState>>>, uri: Uri) -> impl IntoResponse {
    let mut state = state.lock().unwrap();
    state.seen.push(
        uri.path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_default(),
    );
    match state.routes.get(uri.path()) {
        Some(body) => (StatusCode::OK, Json(body.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": "no route"}})),
        )
            .into_response(),
    }
}

/// Decoded query parameters of a request URI.
fn params(uri: &str) -> Vec<(String, String)> {
    let query = uri.split_once('?').map(|(_, q)| q).unwrap_or("");
    url_decode_pairs(query)
}

fn url_decode_pairs(query: &str) -> Vec<(String, String)> {
    fn decode(s: &str) -> String {
        let bytes = s.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }
    query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (decode(k), decode(v))
        })
        .collect()
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
                "[servers.\"{}\"]\nusername = \"ada\"\ntoken = \"wt_test_token\"\n",
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
}

fn hits() -> Value {
    json!({
        "hits": [
            {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "s1",
             "path": "src/main.rs", "line": 2, "text": "fn main() {",
             "before": ["// entry"], "after": ["    run();"]},
            {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "s1",
             "path": "src/lib.rs", "line": 9, "text": "pub fn main_loop() {"}
        ],
        "truncated": true,
        "searched_trees": 1
    })
}

#[test]
fn code_search_sends_the_query_and_prints_grep_lines() {
    let sb = Sandbox::new();
    sb.server.on("/api/v1/search/code", hits());
    let out = sb.ok(&[
        "search",
        "code",
        "fn main",
        "--tree",
        "acme/web",
        "--path",
        "src/**/*.rs",
        "-i",
        "--context",
        "1",
        "-n",
        "2",
    ]);
    assert_eq!(
        out,
        "acme/web:src/main.rs-1-// entry\nacme/web:src/main.rs:2:fn main() {\nacme/web:src/main.rs-3-    run();\n\
         --\nacme/web:src/lib.rs:9:pub fn main_loop() {\n\
         (showing the first 2 matches in 1 trees; narrow the query or raise --limit)\n"
    );
    let seen = sb.server.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].starts_with("/api/v1/search/code?"), "{seen:?}");
    let p = params(&seen[0]);
    let get = |k: &str| p.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    assert_eq!(get("q"), Some("fn main"));
    assert_eq!(get("tree"), Some("acme/web"));
    assert_eq!(get("path"), Some("src/**/*.rs"));
    assert_eq!(get("case"), Some("false"));
    assert_eq!(get("context"), Some("1"));
    assert_eq!(get("limit"), Some("2"));
    assert_eq!(get("regex"), None);
    assert_eq!(get("tenant"), None);
}

#[test]
fn code_search_json_and_regex() {
    let sb = Sandbox::new();
    sb.server.on("/api/v1/search/code", hits());
    let out = sb.ok(&[
        "--json", "search", "code", "-E", "ma+in", "--tenant", "acme",
    ]);
    let doc: Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(doc, hits());
    let p = params(&sb.server.seen()[0]);
    assert!(p.contains(&("regex".into(), "true".into())), "{p:?}");
    assert!(p.contains(&("tenant".into(), "acme".into())), "{p:?}");
    assert!(p.contains(&("case".into(), "true".into())), "{p:?}");
    assert!(p.contains(&("context".into(), "0".into())), "{p:?}");

    sb.server.on(
        "/api/v1/search/code",
        json!({"hits": [], "truncated": false, "searched_trees": 4}),
    );
    assert_eq!(sb.ok(&["search", "code", "nothing"]), "No matches.\n");
}

#[test]
fn code_search_usage_errors() {
    let sb = Sandbox::new();
    for args in [
        &["search", "code", "ab"][..],
        &["search", "code", "abc", "--tree", "web"],
        &[
            "search", "code", "abc", "--tree", "acme/web", "--tenant", "beta",
        ],
    ] {
        let out = sb.wt(args).output().unwrap();
        assert!(!out.status.success(), "{args:?} succeeded");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    assert!(sb.server.seen().is_empty(), "{:?}", sb.server.seen());
}

#[test]
fn name_search_is_unchanged() {
    let sb = Sandbox::new();
    sb.server.on(
        "/api/v1/search",
        json!({"tenants": [], "trees": [], "branches": [], "accounts": []}),
    );
    assert_eq!(sb.ok(&["search", "web", "-n", "5"]), "No matches.\n");
    let seen = sb.server.seen();
    let p = params(&seen[0]);
    assert!(seen[0].starts_with("/api/v1/search?"), "{seen:?}");
    assert!(p.contains(&("q".into(), "web".into())), "{p:?}");
    assert!(p.contains(&("limit".into(), "5".into())), "{p:?}");
}
