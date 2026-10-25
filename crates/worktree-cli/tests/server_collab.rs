//! End-to-end tests of the server-side collaboration commands (`wt tenant`,
//! `wt trees`, `wt mr`, `wt protect`, `wt license`, `wt audit`, `wt admin`,
//! `wt search`, `wt events`) against the real Go server.
//!
//! The server is built once with `go build`; every test starts its own
//! instance on a free port with a private data directory, and every `wt`
//! user gets a private `WT_HOME`. Without a Go toolchain the tests are
//! skipped with a message.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const ADMIN_PASSWORD: &str = "admin-pass-123";
const BOB_PASSWORD: &str = "bob-pass-12345";

/// The server binary, built on first use; None when `go` is unavailable.
fn server_binary() -> Option<&'static Path> {
    static BINARY: OnceLock<Option<PathBuf>> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../services/server-go");
            let binary = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
                "worktree-server-collab{}",
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

/// A running server; killed on drop.
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
        server.wait_healthy(&addr);
        Some(server)
    }

    fn wait_healthy(&self, addr: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(mut stream) = TcpStream::connect(addr) {
                let _ = stream.write_all(b"GET /healthz HTTP/1.0\r\n\r\n");
                let mut response = String::new();
                let _ = stream.read_to_string(&mut response);
                if response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200") {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the server at {addr} did not become healthy");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One `wt` user: a private `WT_HOME` and a working directory.
struct User {
    home: TempDir,
    dir: TempDir,
}

impl User {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("WT_HOME"),
            dir: tempfile::tempdir().expect("work dir"),
        }
    }

    /// A user logged in to `server`.
    fn login(server: &Server, username: &str, password: &str) -> Self {
        let user = Self::new();
        user.wt(&["login", &server.url, "-u", username, "--password-stdin"])
            .write_stdin(format!("{password}\n"))
            .assert()
            .success();
        user
    }

    fn wt(&self, args: &[&str]) -> Command {
        self.wt_in(self.dir.path(), args)
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

    /// Run with `--json` in `dir`, require success and parse stdout.
    fn json_in(&self, dir: &Path, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let output = self.wt_in(dir, &full).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {full:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "wt {full:?} printed invalid JSON ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    fn json(&self, args: &[&str]) -> Value {
        self.json_in(self.dir.path(), args)
    }

    /// Human output of a successful command.
    fn stdout(&self, args: &[&str]) -> String {
        let output = self.wt(args).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 output")
    }

    /// Run a command that must fail; returns (exit code, stderr).
    fn fails(&self, args: &[&str]) -> (i32, String) {
        let output = self.wt(args).output().expect("run wt");
        assert!(!output.status.success(), "wt {args:?} should fail");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn worktree(&self) -> PathBuf {
        self.dir.path().join("web")
    }

    /// A local worktree "web" with origin acme on `server`, `main` holding
    /// a.txt and `feat` changing a.txt and adding b.txt, both pushed.
    fn push_sample_tree(&self, server: &Server) {
        let wt = self.worktree();
        std::fs::create_dir_all(&wt).expect("worktree dir");
        let run = |args: &[&str]| {
            self.wt_in(&wt, args).assert().success();
        };
        run(&["init", "--name", "web"]);
        run(&["config", "set", "user.name", "Ada"]);
        run(&["config", "set", "user.email", "ada@example.com"]);
        run(&["remote", "add", "origin", &server.url, "--tenant", "acme"]);
        std::fs::write(wt.join("a.txt"), "hello\nworld\n").expect("write");
        run(&["snapshot", "-m", "first"]);
        run(&["push"]);
        run(&["branch", "create", "feat"]);
        run(&["switch", "feat"]);
        std::fs::write(wt.join("a.txt"), "hello\nthere\nworld\n").expect("write");
        std::fs::write(wt.join("b.txt"), "new\n").expect("write");
        run(&["snapshot", "-m", "feat work"]);
        run(&["push"]);
    }

    /// Create account `username` (as a server admin).
    fn create_account(&self, username: &str, password: &str) {
        let email = format!("{username}@example.com");
        self.wt(&[
            "admin",
            "account-create",
            username,
            "--email",
            &email,
            "--password-stdin",
        ])
        .write_stdin(format!("{password}\n"))
        .assert()
        .success();
    }
}

fn names(list: &Value, pointer: &str) -> Vec<String> {
    list.as_array()
        .expect("array")
        .iter()
        .map(|item| {
            item.pointer(pointer)
                .and_then(Value::as_str)
                .expect("name")
                .to_string()
        })
        .collect()
}

macro_rules! start_server {
    () => {
        match Server::start() {
            Some(server) => server,
            None => return,
        }
    };
}

#[test]
fn tenants_and_members() {
    let server = start_server!();
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);

    let tenant = admin.json(&["tenant", "create", "acme", "--name", "Acme"]);
    assert_eq!(tenant["slug"], "acme");
    assert_eq!(tenant["kind"], "org");
    assert!(names(&admin.json(&["tenant", "list"]), "/slug").contains(&"acme".to_string()));
    let list = admin.stdout(&["tenant", "list"]);
    assert!(list.starts_with("SLUG "), "{list}");
    assert!(list.contains("acme   Acme"), "{list}");

    assert_eq!(admin.json(&["tenant", "show", "acme"])["name"], "Acme");
    assert_eq!(
        admin.json(&["tenant", "rename", "acme", "Acme Inc"])["name"],
        "Acme Inc"
    );
    assert_eq!(
        admin.json(&["tenant", "plan", "acme", "pro"])["plan"],
        "pro"
    );

    admin.create_account("bob", BOB_PASSWORD);
    let member = admin.json(&["tenant", "add-member", "acme", "bob"]);
    assert_eq!(member["account"]["username"], "bob");
    assert_eq!(member["role"], "developer");
    let member = admin.json(&["tenant", "set-role", "acme", "bob", "maintainer"]);
    assert_eq!(member["role"], "maintainer");
    let members = admin.json(&["tenant", "members", "acme"]);
    assert_eq!(names(&members, "/account/username"), ["admin", "bob"]);
    assert_eq!(members[1]["role"], "maintainer");

    let removed = admin.json(&["tenant", "remove-member", "acme", "bob"]);
    assert_eq!(removed["removed"], true);
    let members = admin.json(&["tenant", "members", "acme"]);
    assert_eq!(names(&members, "/account/username"), ["admin"]);

    let (code, stderr) = admin.fails(&["tenant", "show", "nope"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("not found"), "{stderr}");
}

#[test]
fn trees_branches_tags_and_history() {
    let server = start_server!();
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    admin.json(&["tenant", "create", "acme"]);

    let tree = admin.json(&[
        "trees",
        "create",
        "acme/web",
        "--description",
        "Site",
        "--visibility",
        "internal",
    ]);
    assert_eq!(tree["name"], "web");
    assert_eq!(tree["visibility"], "internal");
    assert_eq!(tree["default_branch"], "main");
    admin.push_sample_tree(&server);

    let list = admin.json(&["trees", "list", "--tenant", "acme"]);
    assert_eq!(names(&list, "/tree/name"), ["web"]);
    assert_eq!(list[0]["branch_count"], 2);
    // Inside the worktree TREE defaults to origin's tenant + the tree name.
    let wt = admin.worktree();
    let summary = admin.json_in(&wt, &["trees", "show"]);
    assert_eq!(summary["tree"]["description"], "Site");
    assert_eq!(summary["snapshot_count"], 2);
    let updated = admin.json(&["trees", "update", "acme/web", "--description", "New site"]);
    assert_eq!(updated["description"], "New site");
    assert_eq!(updated["visibility"], "internal");

    let branches = admin.json_in(&wt, &["trees", "branches"]);
    assert_eq!(names(&branches, "/name"), ["feat", "main"]);
    let main_tip = branches[1]["tip"].as_str().expect("tip").to_string();
    let hotfix = admin.json(&[
        "trees",
        "branch-create",
        "acme/web",
        "hotfix",
        "--from",
        "main",
    ]);
    assert_eq!(hotfix["tip"], main_tip.as_str());
    let (code, stderr) = admin.fails(&["trees", "branch-delete", "acme/web", "hotfix"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("--yes"), "{stderr}");
    let deleted = admin.json(&["trees", "branch-delete", "acme/web", "hotfix", "--yes"]);
    assert_eq!(deleted["deleted"], true);
    assert_eq!(
        names(&admin.json(&["trees", "branches", "acme/web"]), "/name"),
        ["feat", "main"]
    );

    let tag = admin.json(&[
        "trees",
        "tag-create",
        "acme/web",
        "v1",
        "--target",
        "main",
        "-m",
        "first release",
    ]);
    assert_eq!(tag["target"], main_tip.as_str());
    let tags = admin.json(&["trees", "tags", "acme/web"]);
    assert_eq!(names(&tags, "/name"), ["v1"]);
    assert!(admin
        .stdout(&["trees", "tags", "acme/web"])
        .contains("first release"));
    admin.json(&["trees", "tag-delete", "acme/web", "v1", "--yes"]);
    assert_eq!(
        admin.json(&["trees", "tags", "acme/web"]),
        Value::Array(vec![])
    );

    let log = admin.json(&["trees", "log", "acme/web", "--branch", "feat", "-n", "1"]);
    assert_eq!(names(&log, "/message"), ["feat work"]);
    let cmp = admin.json(&[
        "trees", "compare", "acme/web", "--from", "main", "--to", "feat",
    ]);
    assert_eq!(cmp["stats"]["files"], 2);
    assert_eq!(cmp["stats"]["additions"], 2);
    assert_eq!(names(&cmp["files"], "/path"), ["a.txt", "b.txt"]);
    let patch = admin.stdout(&[
        "trees", "compare", "acme/web", "--from", "main", "--to", "feat", "--patch",
    ]);
    assert!(
        patch.contains("diff --wt a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n"),
        "{patch}"
    );
    assert!(patch.contains("\n+there\n"), "{patch}");
    let stat = admin.stdout(&[
        "trees", "compare", "acme/web", "--from", "main", "--to", "feat", "--stat",
    ]);
    assert!(
        stat.contains(" 2 files changed, 2 insertions(+), 0 deletions(-)"),
        "{stat}"
    );

    let stats = admin.json(&["trees", "stats", "acme/web", "--days", "7"]);
    assert_eq!(stats["branch"], "main");
    assert_eq!(stats["snapshot_count"], 1);
    assert_eq!(stats["activity"].as_array().map(Vec::len), Some(7));
    let history = admin.json(&["trees", "file-log", "acme/web", "a.txt"]);
    assert_eq!(names(&history, "/change"), ["added"]);
    assert_eq!(
        admin.json(&["trees", "staged", "acme/web"]),
        Value::Array(vec![])
    );

    let (code, _) = admin.fails(&["trees", "delete", "acme/web"]);
    assert_eq!(code, 2);
    assert_eq!(
        admin.json(&["trees", "delete", "acme/web", "--yes"])["deleted"],
        true
    );
    let (code, stderr) = admin.fails(&["trees", "show", "acme/web"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("not found"), "{stderr}");
}

#[test]
fn merge_requests_protections_and_licenses() {
    let server = start_server!();
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    admin.json(&["tenant", "create", "acme"]);
    admin.push_sample_tree(&server);
    admin.create_account("bob", BOB_PASSWORD);
    admin.json(&[
        "tenant",
        "add-member",
        "acme",
        "bob",
        "--role",
        "maintainer",
    ]);
    let bob = User::login(&server, "bob", BOB_PASSWORD);

    let rule = admin.json(&[
        "protect",
        "set",
        "acme/web",
        "main",
        "--no-direct-push",
        "--approvals",
        "1",
        "--no-delete",
    ]);
    assert_eq!(rule["required_approvals"], 1);
    assert_eq!(rule["no_direct_push"], true);
    let rules = admin.json(&["protect", "list", "acme/web"]);
    assert_eq!(names(&rules, "/pattern"), ["main"]);

    let mr = admin.json(&[
        "mr", "create", "acme/web", "--source", "feat", "--target", "main", "--title", "Add feat",
    ]);
    assert_eq!(mr["number"], 1);
    assert_eq!(mr["state"], "open");
    let wt = admin.worktree();
    let open = admin.json_in(&wt, &["mr", "list"]);
    assert_eq!(names(&open, "/title"), ["Add feat"]);
    let diff = admin.json(&["mr", "show", "1", "acme/web", "--diff"]);
    assert_eq!(diff["merge_request"]["title"], "Add feat");
    assert_eq!(names(&diff["diff"]["files"], "/path"), ["a.txt", "b.txt"]);
    let shown = admin.stdout(&["mr", "show", "1", "acme/web", "--diff"]);
    assert!(shown.starts_with("!1 Add feat  [open]\n"), "{shown}");
    assert!(shown.contains("+new\n"), "{shown}");

    let (code, stderr) = admin.fails(&["mr", "merge", "1", "acme/web"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("approval"), "{stderr}");
    let approved = bob.json(&["mr", "approve", "1", "acme/web"]);
    assert_eq!(approved["approvals"][0]["username"], "bob");
    let merged = admin.json(&["mr", "merge", "1", "acme/web"]);
    assert_eq!(merged["state"], "merged");
    assert!(merged["merged_snapshot"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert_eq!(
        admin.json(&["mr", "list", "acme/web"]),
        Value::Array(vec![])
    );
    let all = admin.json(&["mr", "list", "acme/web", "--state", "all"]);
    assert_eq!(names(&all, "/state"), ["merged"]);
    let diff = admin.json(&["mr", "show", "1", "acme/web", "--diff"]);
    assert_eq!(diff["diff"]["to"]["id"], merged["merged_snapshot"]);

    admin.json(&[
        "mr",
        "create",
        "acme/web",
        "--source",
        "main",
        "--target",
        "feat",
        "--title",
        "Other",
        "--description",
        "Nothing to see",
    ]);
    assert_eq!(
        admin.json(&["mr", "close", "2", "acme/web"])["state"],
        "closed"
    );

    let (code, _) = admin.fails(&["protect", "remove", "acme/web", "main"]);
    assert_eq!(code, 2);
    assert_eq!(
        admin.json(&["protect", "remove", "acme/web", "main", "--yes"])["removed"],
        true
    );
    assert_eq!(
        admin.json(&["protect", "list", "acme/web"]),
        Value::Array(vec![])
    );

    let license = admin.json(&["license", "set", "acme/web", "vendor/**", "MIT"]);
    assert_eq!(license["grant"], "read-only");
    let license = admin.json_in(
        &wt,
        &[
            "license",
            "set",
            "docs/**",
            "CC-BY-4.0",
            "--grant",
            "redistribute",
        ],
    );
    assert_eq!(license["grant"], "redistribute");
    let licenses = admin.json(&["license", "list", "acme/web"]);
    let mut globs = names(&licenses, "/path_glob");
    globs.sort();
    assert_eq!(globs, ["docs/**", "vendor/**"]);
    let (code, _) = admin.fails(&["license", "remove", "acme/web", "vendor/**"]);
    assert_eq!(code, 2);
    admin.json(&["license", "remove", "acme/web", "vendor/**", "--yes"]);
    assert_eq!(
        names(&admin.json(&["license", "list", "acme/web"]), "/path_glob"),
        ["docs/**"]
    );
}

#[test]
fn audit_admin_and_search() {
    let server = start_server!();
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    admin.json(&["tenant", "create", "acme"]);
    admin.json(&["trees", "create", "acme/web"]);
    admin.create_account("bob", BOB_PASSWORD);

    let accounts = admin.json(&["admin", "accounts"]);
    assert_eq!(names(&accounts, "/username"), ["admin", "bob"]);
    let bob = admin.json(&["admin", "account-set", "bob", "--disabled", "true"]);
    assert_eq!(bob["disabled"], true);
    let bob = admin.json(&[
        "admin",
        "account-set",
        "bob",
        "--disabled",
        "false",
        "--admin",
        "true",
    ]);
    assert_eq!(
        (bob["disabled"].clone(), bob["is_admin"].clone()),
        (false.into(), true.into())
    );
    let (code, _) = admin.fails(&["admin", "account-set", "bob"]);
    assert_eq!(code, 2);
    let changed = admin
        .wt(&["--json", "admin", "password", "bob", "--password-stdin"])
        .write_stdin("bob-new-password\n")
        .output()
        .expect("run wt");
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    User::login(&server, "bob", "bob-new-password");

    let system = admin.json(&["admin", "system"]);
    assert!(system["version"].as_str().is_some_and(|v| !v.is_empty()));
    assert!(admin.stdout(&["admin", "system"]).contains("event bus"));
    let stats = admin.json(&["admin", "stats"]);
    assert_eq!(stats["trees"], 1);
    assert_eq!(stats["accounts"], 2);

    let hits = admin.json(&["search", "acme"]);
    assert_eq!(names(&hits["tenants"], "/slug"), ["acme"]);
    let hits = admin.json(&["search", "web", "-n", "5"]);
    assert_eq!(names(&hits["trees"], "/tenant_slug"), ["acme"]);
    assert!(admin
        .stdout(&["search", "zzz-nothing"])
        .contains("No matches."));

    let events = admin.json(&["audit", "--tenant", "acme", "--action", "tenant.create"]);
    assert_eq!(names(&events, "/action"), ["tenant.create"]);
    assert_eq!(
        admin.json(&["audit", "-n", "1"]).as_array().map(Vec::len),
        Some(1)
    );
    let future = admin.json(&["audit", "--since", "2200-01-01T00:00:00Z"]);
    assert_eq!(future, Value::Array(vec![]));
    let text = admin.stdout(&["audit", "--tenant", "acme"]);
    assert!(text.starts_with("WHEN "), "{text}");
    assert!(text.contains("tree.create"), "{text}");
}

#[test]
fn events_stream_json_lines() {
    let server = start_server!();
    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    admin.json(&["tenant", "create", "acme"]);
    admin.push_sample_tree(&server);

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("wt"))
        .args([
            "--json",
            "events",
            "--tree",
            "acme/web",
            "--type",
            "branch.created",
        ])
        .current_dir(admin.dir.path())
        .env("WT_HOME", admin.home.path())
        .env_remove("WT_SERVER")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start wt events");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    // The subscription starts asynchronously: publish until one arrives.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut received = None;
    for i in 0.. {
        // A tag (filtered out by --type) and a branch.
        admin.json(&[
            "trees",
            "tag-create",
            "acme/web",
            &format!("t{i}"),
            "--target",
            "main",
        ]);
        admin.json(&["trees", "branch-create", "acme/web", &format!("b{i}")]);
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(500)) {
            received = Some(line);
            break;
        }
        assert!(Instant::now() < deadline, "no event arrived");
    }
    let _ = child.kill();
    let _ = child.wait();

    let line = received.expect("event line");
    let event: Value = serde_json::from_str(&line).expect("one JSON object per line");
    assert_eq!(event["type"], "branch.created");
    assert!(event["payload"]["name"]
        .as_str()
        .is_some_and(|n| n.starts_with('b')));
}

#[test]
fn errors_and_defaults() {
    let server = start_server!();
    let anonymous = User::new();
    let (code, stderr) = anonymous.fails(&["tenant", "list"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("wt login"), "{stderr}");

    let admin = User::login(&server, "admin", ADMIN_PASSWORD);
    let (code, stderr) = admin.fails(&["trees", "show"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("TENANT/TREE"), "{stderr}");
    let (code, stderr) = admin.fails(&["trees", "show", "acme/missing"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("not found"), "{stderr}");
    let (code, _) = admin.fails(&["trees", "show", "not-a-tree-spec"]);
    assert_eq!(code, 2);
    let (code, stderr) = admin.fails(&["events", "--tenant", "a", "--tree", "b/c"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("does not match"), "{stderr}");
    assert_eq!(
        admin.json(&["--server", &server.url, "trees", "list"]),
        Value::Array(vec![])
    );
}
