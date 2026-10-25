//! `wt api` (ENGINEERING_PLAN §14.10) against an in-process fake REST
//! server: the request the CLI sends (method, path, query, headers, body,
//! bearer token) and how it prints responses and errors.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use assert_cmd::Command;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use tempfile::TempDir;

const TOKEN: &str = "wt_api_test_token";

/// One request as the fake server saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    uri: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// A canned answer.
#[derive(Clone)]
struct Canned {
    status: u16,
    content_type: &'static str,
    body: String,
}

#[derive(Default)]
struct FakeState {
    routes: HashMap<(String, String), Canned>,
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

    fn on(&self, method: &str, uri: &str, status: u16, content_type: &'static str, body: &str) {
        self.lock().routes.insert(
            (method.to_string(), uri.to_string()),
            Canned {
                status,
                content_type,
                body: body.to_string(),
            },
        );
    }

    /// The only request seen.
    fn only(&self) -> Seen {
        let seen = self.lock().seen.clone();
        assert_eq!(seen.len(), 1, "expected one request, saw {seen:#?}");
        seen.into_iter().next().unwrap()
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
) -> Response {
    let uri = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    state.seen.push(Seen {
        method: method.to_string(),
        uri: uri.clone(),
        headers,
        body: body.to_vec(),
    });
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some(c) if c.status == 204 => StatusCode::NO_CONTENT.into_response(),
        Some(c) => (
            StatusCode::from_u16(c.status).unwrap(),
            [("content-type", c.content_type)],
            c.body.clone(),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            [("content-type", "application/json")],
            format!(r#"{{"error":{{"code":"not_found","message":"no route {method} {uri}"}}}}"#),
        )
            .into_response(),
    }
}

/// A private `WT_HOME` logged in to the fake server, and a work dir.
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
            .env_remove("WT_TOKEN")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    /// Run; return (exit code, stdout, stderr).
    fn run(&self, args: &[&str], stdin: &str) -> (i32, String, String) {
        let out = self.wt(args).write_stdin(stdin).output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
}

fn header(seen: &Seen, name: &str) -> Option<String> {
    seen.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

#[test]
fn get_relative_path_pretty_prints_json_in_order() {
    let sb = Sandbox::new();
    sb.server.on(
        "GET",
        "/api/v1/tenants?limit=2",
        200,
        "application/json; charset=utf-8",
        r#"[{"slug":"acme","id":"t1","n":1.50}]"#,
    );
    let (code, out, err) = sb.run(&["api", "get", "tenants?limit=2"], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "[\n  {\n    \"slug\": \"acme\",\n    \"id\": \"t1\",\n    \"n\": 1.50\n  }\n]\n"
    );
    let seen = sb.server.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.uri, "/api/v1/tenants?limit=2");
    assert_eq!(
        header(&seen, "authorization").as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert!(seen.body.is_empty());
}

#[test]
fn raw_prints_the_body_verbatim() {
    let sb = Sandbox::new();
    sb.server.on(
        "GET",
        "/api/v1/auth/me",
        200,
        "application/json",
        r#"{"a":1}"#,
    );
    let (code, out, _) = sb.run(&["api", "GET", "/api/v1/auth/me", "--raw"], "");
    assert_eq!(code, 0);
    assert_eq!(out, r#"{"a":1}"#);
}

#[test]
fn non_json_bodies_are_written_unchanged() {
    let sb = Sandbox::new();
    sb.server.on(
        "GET",
        "/api/v1/openapi.yaml",
        200,
        "application/yaml",
        "openapi: 3.1.0\n",
    );
    let (code, out, _) = sb.run(&["api", "GET", "openapi.yaml"], "");
    assert_eq!(code, 0);
    assert_eq!(out, "openapi: 3.1.0\n");
}

#[test]
fn post_inline_json_body() {
    let sb = Sandbox::new();
    sb.server.on(
        "POST",
        "/api/v1/tenants",
        201,
        "application/json",
        r#"{"slug":"acme"}"#,
    );
    let (code, out, err) = sb.run(
        &[
            "api",
            "POST",
            "tenants",
            "-d",
            r#"{"slug":"acme","name":"Acme"}"#,
            "-H",
            "X-Trace: abc",
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "{\n  \"slug\": \"acme\"\n}\n");
    let seen = sb.server.only();
    assert_eq!(seen.method, "POST");
    assert_eq!(
        header(&seen, "content-type").as_deref(),
        Some("application/json")
    );
    assert_eq!(header(&seen, "x-trace").as_deref(), Some("abc"));
    assert_eq!(seen.body, br#"{"slug":"acme","name":"Acme"}"#);
}

#[test]
fn body_from_file_and_stdin() {
    let sb = Sandbox::new();
    sb.server.on(
        "PUT",
        "/api/v1/trees/acme/web/protections",
        200,
        "application/json",
        "{}",
    );
    std::fs::write(sb.dir.path().join("rule.json"), r#"{"pattern":"main"}"#).unwrap();
    let (code, out, err) = sb.run(
        &[
            "api",
            "PUT",
            "trees/acme/web/protections",
            "-d",
            "@rule.json",
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "{}\n");
    assert_eq!(sb.server.only().body, br#"{"pattern":"main"}"#);

    let sb = Sandbox::new();
    sb.server.on(
        "PATCH",
        "/api/v1/trees/acme/web",
        200,
        "application/json",
        r#"{"name":"web"}"#,
    );
    let (code, _, err) = sb.run(
        &["api", "PATCH", "/api/v1/trees/acme/web", "-d", "@-"],
        r#"{"description":"d"}"#,
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(sb.server.only().body, br#"{"description":"d"}"#);
}

#[test]
fn custom_content_type_sends_raw_bytes() {
    let sb = Sandbox::new();
    sb.server.on(
        "PUT",
        "/api/v1/trees/acme/web/releases/v1/assets/a.bin",
        201,
        "application/json",
        r#"{"name":"a.bin"}"#,
    );
    std::fs::write(sb.dir.path().join("a.bin"), [0u8, 1, 2, 255]).unwrap();
    let (code, _, err) = sb.run(
        &[
            "api",
            "PUT",
            "trees/acme/web/releases/v1/assets/a.bin",
            "-d",
            "@a.bin",
            "-H",
            "Content-Type: application/octet-stream",
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    let seen = sb.server.only();
    assert_eq!(
        header(&seen, "content-type").as_deref(),
        Some("application/octet-stream")
    );
    assert_eq!(seen.body, [0u8, 1, 2, 255]);
}

#[test]
fn invalid_json_body_is_refused_before_sending() {
    let sb = Sandbox::new();
    let (code, _, err) = sb.run(&["api", "POST", "tenants", "-d", "{nope"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("not valid JSON"), "{err}");
    assert!(sb.server.lock().seen.is_empty());
}

#[test]
fn error_envelope_exits_1_with_its_message() {
    let sb = Sandbox::new();
    sb.server.on(
        "DELETE",
        "/api/v1/trees/acme/web",
        403,
        "application/json",
        r#"{"error":{"code":"forbidden","message":"tree:admin required"}}"#,
    );
    let (code, out, err) = sb.run(&["api", "DELETE", "trees/acme/web"], "");
    assert_eq!(code, 1);
    assert!(out.is_empty(), "{out}");
    assert!(
        err.contains("HTTP 403 forbidden: tree:admin required"),
        "{err}"
    );
}

#[test]
fn no_content_prints_nothing() {
    let sb = Sandbox::new();
    sb.server.on(
        "DELETE",
        "/api/v1/auth/tokens/t1",
        204,
        "application/json",
        "",
    );
    let (code, out, err) = sb.run(&["api", "DELETE", "auth/tokens/t1"], "");
    assert_eq!(code, 0, "{err}");
    assert!(out.is_empty(), "{out}");
}

#[test]
fn explicit_authorization_header_wins() {
    let sb = Sandbox::new();
    sb.server
        .on("GET", "/api/v1/auth/me", 200, "application/json", "{}");
    let (code, _, err) = sb.run(
        &["api", "GET", "auth/me", "-H", "Authorization: Bearer other"],
        "",
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        header(&sb.server.only(), "authorization").as_deref(),
        Some("Bearer other")
    );
}

#[test]
fn usage_errors() {
    let sb = Sandbox::new();
    let (code, _, err) = sb.run(&["api", "FETCH", "tenants"], "");
    assert_eq!(code, 2, "{err}");
    let (code, _, err) = sb.run(&["api", "GET", "http://elsewhere/api/v1/tenants"], "");
    assert_eq!(code, 2, "{err}");
    let (code, _, err) = sb.run(&["api", "GET", "tenants", "-H", "novalue"], "");
    assert_eq!(code, 2, "{err}");
    assert!(sb.server.lock().seen.is_empty());
}
