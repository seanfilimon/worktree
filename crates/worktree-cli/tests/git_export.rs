//! End-to-end tests of the git-export workstream (ENGINEERING_PLAN §13.8):
//! `wt git import|push|remote`, license-aware `wt archive` / `wt git
//! export`, and `wt license detect`. Git remotes are local bare
//! repositories (paths and file:// URLs), so the tests run offline; license
//! rules come from an in-process fake REST server.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fake REST server

#[derive(Default)]
struct FakeState {
    routes: HashMap<(String, String), (u16, Value)>,
    seen: Vec<(String, String, Value)>,
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

    fn on(&self, method: &str, uri: &str, status: u16, body: Value) {
        self.state
            .lock()
            .unwrap()
            .routes
            .insert((method.into(), uri.into()), (status, body));
    }

    fn seen(&self, method: &str, uri: &str) -> Vec<Value> {
        self.state
            .lock()
            .unwrap()
            .seen
            .iter()
            .filter(|(m, u, _)| m == method && u == uri)
            .map(|(_, _, b)| b.clone())
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
    let mut state = state.lock().unwrap();
    state.seen.push((
        method.to_string(),
        uri.clone(),
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    ));
    match state.routes.get(&(method.to_string(), uri.clone())) {
        Some((status, body)) => {
            (StatusCode::from_u16(*status).unwrap(), Json(body.clone())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": uri}})),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Sandbox

struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// A worktree "proj" at <dir>/proj.
    fn init() -> Self {
        let sb = Self::new();
        fs::create_dir_all(sb.proj()).unwrap();
        sb.ok(&sb.proj(), &["init", "--name", "proj"]);
        sb.ok(&sb.proj(), &["config", "set", "user.name", "Test User"]);
        sb.ok(
            &sb.proj(),
            &["config", "set", "user.email", "t@example.com"],
        );
        sb
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn proj(&self) -> PathBuf {
        self.root().join("proj")
    }

    fn wt(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(dir)
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_SERVER")
            .env_remove("WT_PASSWORD")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    fn run(&self, dir: &Path, args: &[&str]) -> std::process::Output {
        self.wt(dir, args).output().expect("run wt")
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run(dir, args);
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        serde_json::from_str(&self.ok(dir, &full)).expect("one JSON document")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.proj().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn snapshot(&self, message: &str) {
        self.ok(&self.proj(), &["snapshot", "-m", message]);
    }

    /// Save a login for `server` and make it the worktree's origin.
    fn connect(&self, server: &FakeRest) {
        fs::write(
            self.home.path().join("credentials.toml"),
            format!(
                "[servers.\"{}\"]\nusername = \"ada\"\ntoken = \"wt_test_token\"\n",
                server.url
            ),
        )
        .unwrap();
        self.ok(
            &self.proj(),
            &["remote", "add", "origin", &server.url, "--tenant", "acme"],
        );
    }

    fn cache_rules(&self, rules: Value) {
        let path = self.proj().join(".wt/trees/proj/licenses.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rules.to_string()).unwrap();
    }
}

fn tar_files(path: &Path) -> HashMap<String, String> {
    let bytes = fs::read(path).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    let mut out = HashMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().replace('\\', "/");
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        out.insert(name, text);
    }
    out
}

fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

fn licensed_files(sb: &Sandbox) {
    sb.write("README.md", "# proj\n");
    sb.write("src/a.rs", "fn a() {}\n");
    sb.write("secret/key.txt", "hunter2\n");
    sb.snapshot("files");
}

fn rules() -> Value {
    json!([
        {"tree_id": "t", "path_glob": "src/**", "spdx": "MIT", "grant": ""},
        {"tree_id": "t", "path_glob": "secret/**", "spdx": "LicenseRef-Proprietary", "grant": "read-only"},
        {"tree_id": "t", "path_glob": "**", "spdx": "GPL-3.0-only", "grant": "", "grantee": "other"}
    ])
}

// ---------------------------------------------------------------------------
// Tests

#[test]
fn archive_uses_cached_license_rules() {
    let sb = Sandbox::init();
    licensed_files(&sb);
    sb.cache_rules(rules());

    let out = sb.run(&sb.proj(), &["--json", "archive", "-o", "../out.tar.gz"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["restricted"], json!(["secret/key.txt"]));
    assert_eq!(report["notice"], true);
    assert_eq!(report["license_rules"], "cache");
    assert_eq!(report["files"], 3);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning: skipped secret/key.txt"),
        "{stderr}"
    );

    let files = tar_files(&sb.root().join("out.tar.gz"));
    assert!(!files.contains_key("secret/key.txt"));
    assert_eq!(files["README.md"], "# proj\n");
    let notice = &files["LICENSES/NOTICE"];
    assert!(notice.contains("Tree: proj\n"), "{notice}");
    assert!(notice.contains("\nMIT\n    src/a.rs\n"), "{notice}");
    assert!(notice.contains("1 path was omitted"), "{notice}");
    // Grantee-specific rules do not change a file's license.
    assert!(!notice.contains("GPL"), "{notice}");

    let report = sb.json(
        &sb.proj(),
        &["archive", "-o", "../keep.zip", "--include-restricted"],
    );
    assert_eq!(report["restricted"], json!([]));
    let bytes = fs::read(sb.root().join("keep.zip")).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("secret/key.txt").is_ok());
    let mut notice = String::new();
    zip.by_name("LICENSES/NOTICE")
        .unwrap()
        .read_to_string(&mut notice)
        .unwrap();
    assert!(notice.contains("LicenseRef-Proprietary\n    secret/key.txt\n"));

    let report = sb.json(
        &sb.proj(),
        &["archive", "-o", "../plain.tar.gz", "--no-license"],
    );
    assert_eq!(report["notice"], false);
    assert_eq!(report["license_rules"], "disabled");
    let files = tar_files(&sb.root().join("plain.tar.gz"));
    assert!(files.contains_key("secret/key.txt"));
    assert!(!files.contains_key("LICENSES/NOTICE"));

    sb.wt(
        &sb.proj(),
        &[
            "archive",
            "-o",
            "x.zip",
            "--no-license",
            "--include-restricted",
        ],
    )
    .assert()
    .code(2);
}

#[test]
fn license_rules_are_fetched_from_the_server_and_cached() {
    let sb = Sandbox::init();
    let server = FakeRest::start();
    licensed_files(&sb);
    sb.connect(&server);
    server.on("GET", "/api/v1/trees/acme/proj/licenses", 200, rules());

    let report = sb.json(&sb.proj(), &["archive", "-o", "../out.tar.gz"]);
    assert_eq!(report["license_rules"], "server");
    assert_eq!(report["restricted"], json!(["secret/key.txt"]));
    let cached = fs::read_to_string(sb.proj().join(".wt/trees/proj/licenses.json")).unwrap();
    assert!(cached.contains("secret/**"));

    // Server down: the cache is used, with a warning.
    server.on(
        "GET",
        "/api/v1/trees/acme/proj/licenses",
        500,
        json!({"error": {"code": "internal", "message": "boom"}}),
    );
    let out = sb.run(&sb.proj(), &["--json", "git", "export", "../out.git"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["license_rules"], "cache");
    assert_eq!(report["restricted"], json!(["secret/key.txt"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("using the cached rules"), "{stderr}");

    // The exported repository does not contain the restricted file.
    let check = sb.root().join("check");
    sb.ok(sb.root(), &["git", "import", "out.git", "check"]);
    assert!(check.join("src/a.rs").is_file());
    assert!(check.join("LICENSES/NOTICE").is_file());
    assert!(!check.join("secret/key.txt").exists());
}

#[test]
fn git_remotes_push_and_url_import_round_trip() {
    let sb = Sandbox::init();
    sb.write("hello.txt", "hello\n");
    sb.snapshot("first");
    let proj = sb.proj();

    // Remotes are stored in .wt/config.toml.
    let added = sb.json(&proj, &["git", "remote", "add", "backup", "../backup.git"]);
    assert!(added["url"].as_str().unwrap().ends_with("backup.git"));
    assert!(Path::new(added["url"].as_str().unwrap()).is_absolute());
    let config = fs::read_to_string(proj.join(".wt/config.toml")).unwrap();
    assert!(config.contains("[git_remote.backup]"), "{config}");
    let list = sb.json(&proj, &["git", "remote", "list"]);
    assert_eq!(list[0]["name"], "backup");
    sb.wt(&proj, &["git", "remote", "add", "backup", "../x.git"])
        .assert()
        .code(1);
    sb.wt(&proj, &["git", "remote", "add", "bad name", "../x.git"])
        .assert()
        .code(2);

    // Push to a missing local path creates a bare repository.
    let pushed = sb.json(&proj, &["git", "push", "backup"]);
    assert_eq!(pushed["branches"], json!(["main"]));
    assert!(sb.root().join("backup.git/HEAD").is_file());

    // Import through a file:// URL (cloned to a temp dir) records origin.
    let url = file_url(&sb.root().join("backup.git"));
    let report = sb.json(sb.root(), &["git", "import", &url, "clone"]);
    assert_eq!(report["snapshots"], 1);
    assert_eq!(report["git_remote"], url.as_str());
    let clone = sb.root().join("clone");
    assert_eq!(
        fs::read_to_string(clone.join("hello.txt")).unwrap(),
        "hello\n"
    );

    // A new snapshot in the clone pushes back as a fast-forward.
    sb.ok(&clone, &["config", "set", "user.name", "Clone User"]);
    fs::write(clone.join("second.txt"), "2\n").unwrap();
    sb.ok(&clone, &["snapshot", "-m", "second"]);
    let pushed = sb.json(&clone, &["git", "push", "origin", "--branch", "main"]);
    assert_eq!(pushed["commits"], 1);
    let log = sb.json(sb.root(), &["git", "import", "backup.git", "again"]);
    assert_eq!(log["snapshots"], 2);

    sb.ok(&proj, &["git", "remote", "remove", "backup"]);
    assert_eq!(sb.json(&proj, &["git", "remote", "list"]), json!([]));
    sb.wt(&proj, &["git", "remote", "remove", "backup"])
        .assert()
        .code(1);
    sb.wt(&proj, &["git", "push", "ftp://example.com/x"])
        .assert()
        .code(2);
}

#[test]
fn import_into_a_new_or_existing_tree() {
    let sb = Sandbox::init();
    sb.write("hello.txt", "hello\n");
    sb.snapshot("first");
    let proj = sb.proj();
    sb.ok(&proj, &["git", "export", "../lib.git"]);

    let host = sb.root().join("host");
    fs::create_dir_all(&host).unwrap();
    sb.ok(&host, &["init", "--name", "host"]);

    // --tree NAME inside a worktree creates a nested tree at ./NAME.
    let report = sb.json(&host, &["--tree", "lib", "git", "import", "../lib.git"]);
    assert_eq!(report["tree"], "lib");
    assert_eq!(report["checked_out"], "main");
    assert_eq!(
        fs::read_to_string(host.join("lib/hello.txt")).unwrap(),
        "hello\n"
    );
    let trees = sb.json(&host, &["tree", "list"]);
    assert!(trees.as_array().unwrap().iter().any(|t| t["name"] == "lib"));

    // Importing again after new commits fast-forwards the existing tree.
    sb.write("more.txt", "more\n");
    sb.snapshot("more");
    sb.ok(&proj, &["git", "export", "../lib.git"]);
    let report = sb.json(&host, &["--tree", "lib", "git", "import", "../lib.git"]);
    assert_eq!(report["snapshots"], 2);
    assert_eq!(
        fs::read_to_string(host.join("lib/more.txt")).unwrap(),
        "more\n"
    );

    // Branch selection.
    let report = sb.json(
        sb.root(),
        &[
            "git",
            "import",
            "lib.git",
            "only-main",
            "--branches",
            "main",
        ],
    );
    assert_eq!(report["branches"], json!(["main"]));
    sb.wt(
        sb.root(),
        &["git", "import", "lib.git", "x", "--branches", "nope"],
    )
    .assert()
    .code(1);
    sb.wt(
        sb.root(),
        &[
            "git",
            "import",
            "lib.git",
            "y",
            "--branches",
            "a",
            "--all-branches",
        ],
    )
    .assert()
    .code(2);
}

#[test]
fn license_detect_and_apply() {
    let sb = Sandbox::init();
    let server = FakeRest::start();
    sb.write("LICENSE", "SPDX-License-Identifier: Apache-2.0\n");
    sb.write(
        "src/main.rs",
        "// SPDX-License-Identifier: Apache-2.0\nfn main() {}\n",
    );
    sb.write("src/gpl.c", "/* SPDX-License-Identifier: GPL-2.0-only */\n");
    sb.write("vendor/x/COPYING", "SPDX-License-Identifier: MIT\n");
    sb.write("README.md", "# no license here\n");
    let proj = sb.proj();

    let report = sb.json(&proj, &["license", "detect"]);
    let detections: Vec<(String, String)> = report["detections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["path"].as_str().unwrap().into(),
                d["spdx"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        detections,
        vec![
            ("LICENSE".into(), "Apache-2.0".into()),
            ("src/gpl.c".into(), "GPL-2.0-only".into()),
            ("src/main.rs".into(), "Apache-2.0".into()),
            ("vendor/x/COPYING".into(), "MIT".into()),
        ]
    );
    assert_eq!(
        report["proposals"],
        json!([
            {"path_glob": "**", "spdx": "Apache-2.0"},
            {"path_glob": "src/gpl.c", "spdx": "GPL-2.0-only"},
            {"path_glob": "vendor/x/**", "spdx": "MIT"},
        ])
    );
    let scoped = sb.json(&proj, &["license", "detect", "vendor"]);
    assert_eq!(scoped["detections"].as_array().unwrap().len(), 1);

    sb.connect(&server);
    let path = "/api/v1/trees/acme/proj/licenses";
    server.on(
        "GET",
        path,
        200,
        json!([{"tree_id": "t", "path_glob": "**", "spdx": "Apache-2.0", "grant": "read-only"}]),
    );
    server.on("PUT", path, 200, json!({}));
    let report = sb.json(
        &proj,
        &[
            "license",
            "detect",
            "--apply",
            "--yes",
            "--grant",
            "redistribute",
        ],
    );
    assert_eq!(report["applied"].as_array().unwrap().len(), 2);
    let puts = server.seen("PUT", path);
    assert_eq!(
        puts,
        vec![
            json!({"path_glob": "src/gpl.c", "spdx": "GPL-2.0-only", "grant": "redistribute"}),
            json!({"path_glob": "vendor/x/**", "spdx": "MIT", "grant": "redistribute"}),
        ]
    );
    sb.wt(&proj, &["license", "detect", "--yes"])
        .assert()
        .code(2);
}
