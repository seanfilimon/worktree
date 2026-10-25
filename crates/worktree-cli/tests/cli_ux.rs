//! End-to-end tests of the CLI ergonomics commands (ENGINEERING_PLAN §14.8):
//! aliases, `wt completions`, `wt open --print` and `wt doctor` (against an
//! in-process fake REST server).

use std::path::Path;

use assert_cmd::Command;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::{Json, Router};
use predicates::prelude::*;
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "wt_ux_token";

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

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn wt(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(self.path())
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_PASSWORD")
            .env_remove("WT_SERVER")
            .env_remove("WT_TOKEN")
            .env_remove("WT_LOG")
            .env_remove("BROWSER")
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

    fn init(&self) {
        self.ok(&["init", "--name", "web"]);
    }
}

#[test]
fn aliases_from_user_and_worktree_config() {
    let sb = Sandbox::new();
    // --global works outside a worktree.
    sb.ok(&["config", "set", "--global", "alias.st", "status"]);
    sb.ok(&["config", "set", "--global", "alias.status", "log"]);
    sb.init();
    sb.ok(&["config", "set", "alias.lg", "log --oneline -n 1"]);
    sb.ok(&[
        "config",
        "set",
        "alias.wip",
        "snapshot --allow-empty -m 'work in progress'",
    ]);

    let status: Value = serde_json::from_str(&sb.ok(&["--json", "st"])).unwrap();
    assert!(
        status.get("branch").is_some() || status.is_object(),
        "{status}"
    );

    sb.ok(&["wip"]);
    let log = sb.ok(&["lg"]);
    assert!(log.contains("work in progress"), "{log}");
    assert_eq!(log.lines().count(), 1, "{log}");

    // Built-ins are never shadowed: `status` is still the status command.
    let out = sb.ok(&["status"]);
    assert!(out.contains("On tree web"), "{out}");

    // Aliases show up in `wt config list`.
    let list = sb.ok(&["config", "list"]);
    assert!(list.contains("alias.lg=log --oneline -n 1"), "{list}");
    assert!(list.contains("alias.st=status"), "{list}");
}

#[test]
fn alias_cycles_and_unknown_commands_are_usage_errors() {
    let sb = Sandbox::new();
    sb.init();
    sb.ok(&["config", "set", "alias.a", "b"]);
    sb.ok(&["config", "set", "alias.b", "a"]);
    sb.wt(&["a"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("more than 5 levels"));
    // Unknown commands keep clap's suggestion.
    sb.wt(&["stauts"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("status"));
}

#[test]
fn completions_for_every_shell() {
    let sb = Sandbox::new();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let script = sb.ok(&["completions", shell]);
        assert!(script.len() > 1000, "{shell}");
        assert!(script.contains("snapshot"), "{shell}");
    }
    sb.wt(&["completions", "tcsh"]).assert().code(2);
}

#[test]
fn open_prints_admin_urls() {
    let sb = Sandbox::new();
    sb.init();
    sb.ok(&[
        "remote",
        "add",
        "origin",
        "http://localhost:7420",
        "--tenant",
        "acme",
    ]);
    let base = "http://localhost:7420/admin/trees/acme/web";
    assert_eq!(sb.ok(&["open", "--print"]).trim(), base);
    assert_eq!(
        sb.ok(&["open", "mr", "4", "--print"]).trim(),
        format!("{base}/merge-requests/4")
    );
    std::fs::create_dir_all(sb.path().join("src")).unwrap();
    std::fs::write(sb.path().join("src/a b.txt"), "x").unwrap();
    assert_eq!(
        sb.ok(&["open", "file", "src/a b.txt", "--print"]).trim(),
        format!("{base}/files?path=src/a%20b.txt&branch=main")
    );
    sb.ok(&["snapshot", "-m", "first"]);
    let json: Value =
        serde_json::from_str(&sb.ok(&["--json", "open", "snapshot", "HEAD", "--print"])).unwrap();
    let url = json["url"].as_str().unwrap();
    assert!(url.starts_with(&format!("{base}/snapshots/")), "{url}");
    assert_eq!(json["opened"], false);

    sb.wt(&["open", "--remote", "nope", "--print"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("remote 'nope'"));
}

async fn fake(uri: Uri, headers: HeaderMap) -> impl IntoResponse {
    let authed = headers.get("authorization").and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"));
    match uri.path() {
        "/healthz" => (StatusCode::OK, Json(json!("ok"))),
        _ if !authed => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"code": "unauthenticated", "message": "no"}})),
        ),
        "/api/v1/auth/me" => (
            StatusCode::OK,
            Json(json!({"account": {"username": "ada"}, "memberships": []})),
        ),
        "/api/v1/auth/sessions" => (StatusCode::OK, Json(json!([]))),
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": "no"}})),
        ),
    }
}

fn start_fake() -> (String, tokio::sync::oneshot::Sender<()>) {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            addr_tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, Router::new().fallback(fake))
                .with_graceful_shutdown(async {
                    let _ = stop.await;
                })
                .await
                .unwrap();
        });
    });
    (format!("http://{}", addr_rx.recv().unwrap()), stop_tx)
}

#[test]
fn doctor_offline_and_against_a_server() {
    let sb = Sandbox::new();
    // Outside a worktree, offline: nothing is an error.
    let report: Value = serde_json::from_str(&sb.ok(&["--json", "doctor", "--offline"])).unwrap();
    assert_eq!(report["ok"], true, "{report:#}");
    let names: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"worktree") && names.contains(&"daemon"),
        "{names:?}"
    );

    sb.init();
    sb.ok(&["config", "set", "user.name", "Ada"]);
    sb.ok(&["config", "set", "user.email", "ada@example.com"]);
    let text = sb.ok(&["doctor", "--offline"]);
    assert!(text.contains("✓ worktree"), "{text}");
    assert!(text.contains("Ada <ada@example.com>"), "{text}");

    // A broken tree-level or user config is an error (exit 1).
    std::fs::write(sb.home.path().join("config.toml"), "[user\nname=").unwrap();
    sb.wt(&["doctor", "--offline"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("✗ config.user"));
    std::fs::remove_file(sb.home.path().join("config.toml")).unwrap();

    // Against the fake REST server: HTTP and /auth/me work, gRPC does not.
    let (url, _stop) = start_fake();
    sb.ok(&["remote", "add", "origin", &url, "--tenant", "acme"]);
    std::fs::write(
        sb.home.path().join("credentials.toml"),
        format!("[servers.\"{url}\"]\nusername = \"ada\"\ntoken = \"{TOKEN}\"\n"),
    )
    .unwrap();
    let out = sb
        .wt(&["--json", "doctor", "--timeout", "3"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    let status = |name: &str| {
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .map(|c| c["status"].as_str().unwrap().to_string())
            .unwrap_or_else(|| panic!("no check {name}: {report:#}"))
    };
    assert_eq!(status("origin: server"), "ok");
    assert_eq!(status("origin: sync"), "error");
    assert_eq!(status("origin: clock"), "ok");
    assert_eq!(status("origin: credential"), "ok");
    assert_eq!(status("origin: token"), "skip");
}
