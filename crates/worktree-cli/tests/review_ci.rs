//! End-to-end tests of the v0.5 review and CI commands (`wt mr review`,
//! `wt mr comment(s)`, `wt mr checks`, `wt mr link|merge-group|groups`,
//! `wt ci report|status`, `wt protect --require-ci ...`, `wt protect tag`)
//! against the real Go server. Without a Go toolchain they are skipped.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const ADMIN_PASSWORD: &str = "admin-pass-123";
const BOB_PASSWORD: &str = "bob-pass-12345";

fn server_binary() -> Option<&'static Path> {
    static BINARY: OnceLock<Option<PathBuf>> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../services/server-go");
            let binary = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
                "worktree-server-review{}",
                std::env::consts::EXE_SUFFIX
            ));
            let status = std::process::Command::new("go")
                .args(["build", "-o"])
                .arg(&binary)
                .arg(".")
                .current_dir(&source)
                .status();
            match status {
                Ok(s) if s.success() => Some(binary),
                Ok(s) => panic!("go build of the server failed: {s}"),
                Err(e) => {
                    eprintln!("skipping server tests: cannot run `go` ({e})");
                    None
                }
            }
        })
        .as_deref()
}

struct Server {
    child: Child,
    url: String,
    _data: TempDir,
}

impl Server {
    fn start() -> Option<Self> {
        let binary = server_binary()?;
        let data = tempfile::tempdir().expect("server data dir");
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("free port")
            .port();
        let addr = format!("127.0.0.1:{port}");
        let log = std::fs::File::create(data.path().join("server.log")).expect("server log");
        let child = std::process::Command::new(binary)
            .args(["--addr", &addr, "--admin-user", "admin"])
            .args(["--admin-password", ADMIN_PASSWORD])
            .arg("--data")
            .arg(data.path().join("data"))
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("start the server");
        let server = Self {
            child,
            url: format!("http://{addr}"),
            _data: data,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(mut stream) = TcpStream::connect(&addr) {
                let _ = stream.write_all(b"GET /healthz HTTP/1.0\r\n\r\n");
                let mut response = String::new();
                let _ = stream.read_to_string(&mut response);
                if response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200") {
                    return Some(server);
                }
            }
            assert!(
                Instant::now() < deadline,
                "the server did not become healthy"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct User {
    home: TempDir,
    dir: TempDir,
}

impl User {
    fn login(server: &Server, username: &str, password: &str) -> Self {
        let user = Self {
            home: tempfile::tempdir().expect("WT_HOME"),
            dir: tempfile::tempdir().expect("work dir"),
        };
        user.wt_in(
            user.dir.path(),
            &["login", &server.url, "-u", username, "--password-stdin"],
        )
        .write_stdin(format!("{password}\n"))
        .assert()
        .success();
        user
    }

    fn wt_in(&self, dir: &Path, args: &[&str]) -> Command {
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

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let output = self.wt_in(self.dir.path(), &full).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {full:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("JSON output")
    }

    fn stdout(&self, args: &[&str]) -> String {
        let output = self.wt_in(self.dir.path(), args).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 output")
    }

    fn fails(&self, args: &[&str]) -> String {
        let output = self.wt_in(self.dir.path(), args).output().expect("run wt");
        assert!(!output.status.success(), "wt {args:?} should fail");
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    /// A worktree "web" (origin acme): `main` with a.txt, `feat` on top.
    fn push_sample_tree(&self, server: &Server) {
        let wt = self.dir.path().join("web");
        std::fs::create_dir_all(&wt).expect("worktree dir");
        let run = |args: &[&str]| {
            self.wt_in(&wt, args).assert().success();
        };
        run(&["init", "--name", "web"]);
        run(&["config", "set", "user.name", "Ada"]);
        run(&["config", "set", "user.email", "ada@example.com"]);
        run(&["remote", "add", "origin", &server.url, "--tenant", "acme"]);
        std::fs::write(wt.join("a.txt"), "hello\n").expect("write");
        run(&["snapshot", "-m", "first"]);
        run(&["push"]);
        run(&["branch", "create", "feat"]);
        run(&["switch", "feat"]);
        std::fs::write(wt.join("b.txt"), "new\n").expect("write");
        run(&["snapshot", "-m", "feat work"]);
        run(&["push"]);
    }
}

#[test]
fn reviews_ci_groups_and_tag_protections() {
    let Some(server) = Server::start() else {
        return;
    };
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    admin.json(&["tenant", "create", "acme"]);
    admin.push_sample_tree(&server);
    admin
        .wt_in(
            admin.dir.path(),
            &[
                "admin",
                "account-create",
                "bob",
                "--email",
                "bob@example.com",
                "--password-stdin",
            ],
        )
        .write_stdin(format!("{BOB_PASSWORD}\n"))
        .assert()
        .success();
    admin.json(&[
        "tenant",
        "add-member",
        "acme",
        "bob",
        "--role",
        "maintainer",
    ]);
    let bob = User::login(&server, "bob", BOB_PASSWORD);
    for b in ["stage", "prod"] {
        admin.json(&["trees", "branch-create", "acme/web", b, "--from", "main"]);
    }

    // Protection with the v0.5 merge requirements.
    let rule = admin.json(&[
        "protect",
        "set",
        "acme/web",
        "main",
        "--check",
        "build",
        "--block-on-todos",
        "--require-linear",
    ]);
    assert_eq!(rule["require_ci_pass"], true);
    assert_eq!(rule["required_checks"], serde_json::json!(["build"]));
    let rules = admin.stdout(&["protect", "list", "acme/web"]);
    assert!(rules.contains("ci (build), linear, todos"), "{rules}");

    // Review decisions.
    admin.json(&[
        "mr", "create", "acme/web", "--source", "feat", "--target", "main", "--title", "Feat",
    ]);
    let reviewed = bob.json(&[
        "mr",
        "review",
        "1",
        "acme/web",
        "--request-changes",
        "-m",
        "add tests",
    ]);
    assert_eq!(reviewed["approvals"][0]["decision"], "changes_requested");
    let checks = admin.json(&["mr", "checks", "1", "acme/web"]);
    assert_eq!(checks["mergeable"], false);
    let human = admin.stdout(&["mr", "checks", "1", "acme/web"]);
    assert!(human.starts_with("!1 cannot be merged\n"), "{human}");
    assert!(human.contains("changes requested by bob"), "{human}");
    let stderr = admin.fails(&["mr", "merge", "1", "acme/web"]);
    assert!(stderr.contains("changes requested by bob"), "{stderr}");
    bob.json(&["mr", "review", "1", "acme/web", "--approve"]);
    let shown = admin.stdout(&["mr", "show", "1", "acme/web"]);
    assert!(shown.contains("approved by bob"), "{shown}");

    // Comments.
    let c = admin.json(&[
        "mr",
        "comment",
        "1",
        "acme/web",
        "-m",
        "see b.txt",
        "--path",
        "b.txt",
        "--line",
        "1",
    ]);
    assert_eq!(c["path"], "b.txt");
    bob.json(&["mr", "review", "1", "acme/web", "--comment", "-m", "nice"]);
    let comments = admin.json(&["mr", "comments", "1", "acme/web"]);
    assert_eq!(comments.as_array().map(Vec::len), Some(2));
    assert!(admin
        .stdout(&["mr", "comments", "1", "acme/web"])
        .contains("b.txt:1"));

    // CI.
    admin.json(&[
        "ci",
        "report",
        "acme/web",
        "--snapshot",
        "feat",
        "--context",
        "build",
        "--state",
        "pending",
    ]);
    let status = admin.json(&["ci", "status", "acme/web", "--branch", "feat"]);
    assert_eq!(status["state"], "pending");
    assert!(admin
        .stdout(&["ci", "status", "acme/web", "--branch", "feat"])
        .contains("build    pending"));
    let stderr = admin.fails(&["mr", "merge", "1", "acme/web"]);
    assert!(stderr.contains("build is pending"), "{stderr}");
    admin.json(&[
        "ci",
        "report",
        "acme/web",
        "--snapshot",
        "feat",
        "--context",
        "build",
        "--state",
        "success",
        "--url",
        "https://ci.example/1",
    ]);
    assert_eq!(
        admin.json(&["mr", "checks", "1", "acme/web"])["mergeable"],
        true
    );
    assert_eq!(
        admin.json(&["mr", "merge", "1", "acme/web"])["state"],
        "merged"
    );

    // Linked groups.
    admin.json(&[
        "mr", "create", "acme/web", "--source", "feat", "--target", "stage", "--title", "Stage",
    ]);
    admin.json(&[
        "mr", "create", "acme/web", "--source", "feat", "--target", "prod", "--title", "Prod",
    ]);
    let group = admin.json(&["mr", "link", "rollout", "acme/web#2", "acme/web#3"]);
    let id = group["id"].as_str().expect("group id").to_string();
    assert_eq!(group["members"][0]["tree"], "acme/web");
    let stderr = admin.fails(&["mr", "merge", "2", "acme/web"]);
    assert!(stderr.contains("linked group"), "{stderr}");
    let listed = admin.stdout(&["mr", "groups", "--tenant", "acme"]);
    assert!(
        listed.contains("rollout") && listed.contains("open"),
        "{listed}"
    );
    let merged = admin.json(&["mr", "merge-group", &id, "--tenant", "acme"]);
    assert_eq!(merged["state"], "merged");
    let branches = admin.json(&["trees", "branches", "acme/web"]);
    let tips: Vec<&str> = branches
        .as_array()
        .expect("branches")
        .iter()
        .map(|b| b["tip"].as_str().unwrap_or_default())
        .collect();
    assert!(
        tips.iter().all(|t| *t == tips[0]),
        "all branches at feat: {branches}"
    );

    // Tag protections.
    let tp = admin.json(&["protect", "tag", "set", "acme/web", "v*", "--no-delete"]);
    assert_eq!(tp["no_delete"], true);
    assert!(admin
        .stdout(&["protect", "tag", "list", "acme/web"])
        .starts_with("PATTERN  MOVE"));
    admin.json(&["trees", "tag-create", "acme/web", "v1", "--target", "main"]);
    let stderr = admin.fails(&["trees", "tag-delete", "acme/web", "v1", "--yes"]);
    assert!(stderr.contains("cannot be deleted"), "{stderr}");
    admin.json(&["protect", "tag", "remove", "acme/web", "v*", "--yes"]);
    admin.json(&["trees", "tag-delete", "acme/web", "v1", "--yes"]);
}
