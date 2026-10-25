//! `wt daemon …`, `wt init --watch`, `wt unstage` / `wt stage --discard`
//! and the `wt status` daemon hint (ENGINEERING_PLAN §13.11).
//!
//! Every test uses a private `WT_HOME`. The end-to-end test starts a real
//! `worktree-bg` only when that binary was built next to `wt` (e.g. by
//! `cargo test --workspace`) and always stops the daemon it started.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("WT_HOME"),
            dir: tempfile::tempdir().expect("work dir"),
        }
    }

    fn init() -> Self {
        let sb = Self::new();
        sb.wt(&["init", "--name", "proj"]).assert().success();
        sb
    }

    fn wt(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(self.dir.path())
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.wt(&full).output().expect("run wt");
        assert!(
            out.status.success(),
            "wt {full:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json stdout")
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.wt(args).output().expect("run wt");
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8")
    }
}

#[test]
fn status_hints_when_the_daemon_is_not_running() {
    let sb = Sandbox::init();
    let out = sb.stdout(&["status"]);
    assert!(
        out.contains("hint: auto-snapshot is enabled but the background daemon is not running"),
        "{out}"
    );
    assert!(out.contains("wt daemon start"), "{out}");
    // JSON output stays a single clean document.
    assert!(sb.json(&["status"]).get("branch").is_some());

    sb.wt(&["config", "set", "auto_snapshot.enabled", "false"])
        .assert()
        .success();
    assert!(!sb.stdout(&["status"]).contains("hint:"));
}

#[test]
fn daemon_logs_show_the_tail_of_bg_log() {
    let sb = Sandbox::new();
    sb.wt(&["daemon", "logs"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no daemon log"))
        .stderr(predicate::str::contains("wt daemon start"));

    let lines: Vec<String> = (1..=60).map(|i| format!("line {i}")).collect();
    fs::write(sb.home.path().join("bg.log"), lines.join("\n") + "\n").expect("write log");
    let out = sb.stdout(&["daemon", "logs"]);
    assert_eq!(out.lines().count(), 50, "default is 50 lines");
    assert!(
        out.starts_with("line 11\n") && out.ends_with("line 60\n"),
        "{out}"
    );
    assert_eq!(
        sb.stdout(&["daemon", "logs", "-n", "2"]),
        "line 59\nline 60\n"
    );
    let json = sb.json(&["daemon", "logs", "-n", "1"]);
    assert_eq!(json["lines"], serde_json::json!(["line 60"]));
}

#[test]
fn daemon_control_commands_need_a_running_daemon() {
    let sb = Sandbox::init();
    for args in [
        &["daemon", "snapshot"][..],
        &["daemon", "snapshot", "--all"],
    ] {
        sb.wt(args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("not running"));
    }
}

#[test]
fn daemon_rejects_a_stale_token() {
    // A bg.json pointing at a port nobody serves reads as "not running".
    let sb = Sandbox::init();
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    fs::write(
        sb.home.path().join("bg.json"),
        format!(r#"{{"pid":1,"port":{port},"version":"0.1.0","token":"stale"}}"#),
    )
    .expect("write bg.json");
    sb.wt(&["daemon", "status"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not running"));
}

#[test]
fn unstage_needs_a_remote() {
    let sb = Sandbox::init();
    for args in [
        &["unstage"][..],
        &["stage", "--discard"],
        &["unstage", "--all"],
    ] {
        sb.wt(args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("remote not configured: origin"));
    }
}

/// `worktree-bg` next to the `wt` test binary, if it was built.
fn daemon_binary() -> Option<PathBuf> {
    let wt = assert_cmd::cargo::cargo_bin("wt");
    let bg = wt
        .parent()?
        .join(format!("worktree-bg{}", std::env::consts::EXE_SUFFIX));
    bg.is_file().then_some(bg)
}

/// Stops the daemon of a sandbox when dropped (even if the test panics).
struct StopDaemon<'a>(&'a Sandbox);

impl Drop for StopDaemon<'_> {
    fn drop(&mut self) {
        let _ = self.0.wt(&["daemon", "stop"]).output();
    }
}

fn watched_roots(sb: &Sandbox) -> Vec<String> {
    sb.json(&["daemon", "status"])["watched"]
        .as_array()
        .expect("watched")
        .iter()
        .map(|w| w["root"].as_str().expect("root").to_string())
        .collect()
}

fn same_dir(a: &str, b: &Path) -> bool {
    fs::canonicalize(a).ok() == fs::canonicalize(b).ok()
}

#[test]
fn init_watch_snapshot_restart_and_stop() {
    if daemon_binary().is_none() {
        eprintln!("worktree-bg is not built next to wt; skipping the end-to-end daemon test");
        return;
    }
    let sb = Sandbox::new();
    let _stop = StopDaemon(&sb);
    let out = sb.json(&["init", "--name", "proj", "--watch"]);
    assert!(
        out["watching"]
            .as_str()
            .is_some_and(|m| m.contains("watching")),
        "{out}"
    );
    let roots = watched_roots(&sb);
    assert_eq!(roots.len(), 1);
    assert!(same_dir(&roots[0], sb.dir.path()), "{roots:?}");
    // Running and watching: no status hint.
    assert!(!sb.stdout(&["status"]).contains("hint:"));

    fs::write(sb.dir.path().join("a.txt"), "hello").expect("write");
    let msg = sb.json(&["daemon", "snapshot"]);
    assert_eq!(msg["message"], "created 1 snapshot", "{msg}");
    let log = sb.json(&["log"]);
    assert_eq!(log.as_array().map(Vec::len), Some(1));

    let before = sb.json(&["daemon", "status"])["pid"].clone();
    sb.wt(&["daemon", "restart"]).assert().success();
    let after = sb.json(&["daemon", "status"]);
    assert_ne!(after["pid"], before, "a new daemon process");
    assert_eq!(watched_roots(&sb).len(), 1, "watch list survives a restart");

    let all = sb.json(&["daemon", "snapshot", "--all"]);
    assert_eq!(all["results"][0]["message"], "no changes to snapshot");

    sb.wt(&["daemon", "stop"]).assert().success();
    // Stopping is asynchronous; wait for bg.json to disappear.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while sb.home.path().join("bg.json").exists() {
        assert!(std::time::Instant::now() < deadline, "daemon did not stop");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sb
        .stdout(&["status"])
        .contains("background daemon is not running"));
}
