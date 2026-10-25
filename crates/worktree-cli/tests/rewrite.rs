//! End-to-end tests of `wt cherry-pick`, `wt rebase`, `wt stash` and
//! `wt switch` with `auto_snapshot.on_branch_switch = "stash"`
//! (ENGINEERING_PLAN §14.5). Each test runs in a private temp dir with its
//! own `WT_HOME`.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn init() -> Self {
        let sb = Self {
            home: tempfile::tempdir().expect("WT_HOME"),
            dir: tempfile::tempdir().expect("work dir"),
        };
        sb.wt(&["init", "--name", "proj"]).assert().success();
        sb.wt(&["config", "set", "user.name", "Test User"])
            .assert()
            .success();
        sb
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
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let output = self.wt(&full).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {full:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("json")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path().join(rel)).expect("read")
    }

    fn snapshot(&self, message: &str) -> String {
        self.json(&["snapshot", "-m", message])["id"]
            .as_str()
            .expect("id")
            .to_string()
    }

    fn head(&self) -> String {
        self.json(&["log", "-n", "1"])[0]["id"]
            .as_str()
            .expect("id")
            .to_string()
    }

    /// main and feature both change line 2 of a.txt; feature then adds b.txt.
    /// Returns the two feature snapshot ids.
    fn diverge(&self) -> (String, String) {
        self.write("a.txt", "1\n2\n3\n");
        self.snapshot("base");
        self.wt(&["switch", "-c", "feature"]).assert().success();
        self.write("a.txt", "1\nfeature\n3\n");
        let f1 = self.snapshot("feature edit");
        self.write("b.txt", "b\n");
        let f2 = self.snapshot("add b");
        self.wt(&["switch", "main"]).assert().success();
        self.write("a.txt", "1\nmain\n3\n");
        self.snapshot("main edit");
        (f1, f2)
    }
}

#[test]
fn cherry_pick_conflict_resolve_continue() {
    let sb = Sandbox::init();
    let (f1, f2) = sb.diverge();
    sb.wt(&["cherry-pick", &f1, &f2])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("CONFLICT (content): a.txt"))
        .stderr(predicate::str::contains("wt cherry-pick --continue"));
    // The conflict is a regular merge conflict for `wt merge status|resolve`.
    sb.wt(&["merge", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unresolved   a.txt (content)"));
    sb.wt(&["merge", "--continue"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cherry-pick is in progress"));
    sb.wt(&["merge", "resolve", "--all", "--theirs"])
        .assert()
        .success();
    let out = sb.json(&["cherry-pick", "--continue"]);
    assert_eq!(out["status"], "completed");
    assert_eq!(out["picked"].as_array().expect("picked").len(), 2);
    assert_eq!(sb.read("a.txt"), "1\nfeature\n3\n");
    assert_eq!(sb.read("b.txt"), "b\n");
    let log = sb.json(&["log", "-n", "3"]);
    assert_eq!(log[0]["message"], "add b");
    assert_eq!(log[1]["message"], "feature edit");
    assert_eq!(log[2]["message"], "main edit");
}

#[test]
fn cherry_pick_abort_and_no_snapshot() {
    let sb = Sandbox::init();
    let (f1, f2) = sb.diverge();
    let head = sb.head();
    sb.wt(&["cherry-pick", &f2, &f1]).assert().code(3);
    sb.wt(&["cherry-pick", "--abort"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cherry-pick aborted"));
    assert_eq!(sb.head(), head);
    assert_eq!(sb.read("a.txt"), "1\nmain\n3\n");
    assert!(!sb.path().join("b.txt").exists());
    sb.wt(&["status"]).assert().success();

    sb.wt(&["cherry-pick", "-n", &f2])
        .assert()
        .success()
        .stdout(predicate::str::contains("no snapshots created"));
    assert_eq!(sb.head(), head);
    assert_eq!(sb.read("b.txt"), "b\n");
    sb.wt(&["cherry-pick"]).assert().code(2);
}

#[test]
fn rebase_onto_main_and_pushed_guard() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n2\n3\n");
    sb.snapshot("base");
    sb.wt(&["switch", "-c", "feature"]).assert().success();
    sb.write("f.txt", "f\n");
    let f = sb.snapshot("feature work");
    sb.wt(&["switch", "main"]).assert().success();
    sb.write("m.txt", "m\n");
    let main_tip = sb.snapshot("main work");
    sb.wt(&["switch", "feature"]).assert().success();

    // Pretend feature was pushed: a remote-tracking ref contains it.
    let remote = sb.path().join(".wt/trees/proj/refs/remotes/origin/feature");
    fs::create_dir_all(remote.parent().expect("parent")).expect("mkdir");
    fs::write(&remote, &f).expect("write ref");
    sb.wt(&["rebase", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));

    let out = sb.json(&["rebase", "main", "--force"]);
    assert_eq!(out["status"], "completed");
    assert_eq!(out["onto"], main_tip.as_str());
    let log = sb.json(&["log", "-n", "2"]);
    assert_eq!(log[0]["message"], "feature work");
    assert_ne!(log[0]["id"], f.as_str());
    assert_eq!(log[1]["id"], main_tip.as_str());
    assert_eq!(sb.read("m.txt"), "m\n");
    let reflog = sb.json(&["reflog", "-n", "1"]);
    assert_eq!(reflog[0]["action"], "rebase");

    sb.wt(&["rebase", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date"));
}

#[test]
fn rebase_conflict_skip() {
    let sb = Sandbox::init();
    sb.diverge();
    sb.wt(&["switch", "feature"]).assert().success();
    sb.wt(&["rebase", "main"]).assert().code(3);
    sb.wt(&["snapshot", "-m", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("rebase is in progress"));
    let out = sb.json(&["rebase", "--skip"]);
    assert_eq!(out["status"], "completed");
    assert_eq!(out["skipped"].as_array().expect("skipped").len(), 1);
    assert_eq!(sb.read("a.txt"), "1\nmain\n3\n");
    assert_eq!(sb.read("b.txt"), "b\n");
    sb.wt(&["rebase", "--abort"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no rebase in progress"));
}

#[test]
fn stash_push_list_show_pop() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n");
    sb.snapshot("base");
    sb.write("a.txt", "changed\n");
    sb.write("new.txt", "n\n");
    sb.wt(&["stash", "-u", "-m", "both"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Saved 2 changed files as stash@{0}: both",
        ));
    assert_eq!(sb.read("a.txt"), "1\n");
    assert!(!sb.path().join("new.txt").exists());

    sb.write("a.txt", "second\n");
    sb.wt(&["stash", "push"]).assert().success();
    let list = sb.json(&["stash", "list"]);
    assert_eq!(list.as_array().expect("list").len(), 2);
    assert_eq!(list[1]["message"], "both");
    sb.wt(&["stash", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stash@{1}: both (main, 2 files"));
    sb.wt(&["stash", "show", "stash@{1}"])
        .assert()
        .success()
        .stdout(predicate::str::contains("new.txt"));
    sb.wt(&["stash", "show", "1", "-p"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+changed"));

    sb.wt(&["stash", "drop"]).assert().success();
    sb.wt(&["stash", "pop"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dropped stash@{0}"));
    assert_eq!(sb.read("a.txt"), "changed\n");
    assert_eq!(sb.read("new.txt"), "n\n");
    sb.wt(&["stash", "pop"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("the stash is empty"));
    sb.wt(&["stash", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dropped 0 stash entries."));
}

#[test]
fn stash_pop_conflict_exits_3_and_keeps_entry() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n2\n");
    sb.snapshot("base");
    sb.write("a.txt", "1\nstash\n");
    sb.wt(&["stash"]).assert().success();
    sb.write("a.txt", "1\nhead\n");
    sb.snapshot("head");
    sb.wt(&["stash", "pop"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("CONFLICT (content): a.txt"));
    assert!(sb.read("a.txt").contains("<<<<<<< ours (working tree)"));
    assert_eq!(
        sb.json(&["stash", "list"]).as_array().expect("list").len(),
        1
    );
}

#[test]
fn switch_auto_stashes_with_config() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n");
    sb.snapshot("base");
    sb.wt(&["branch", "create", "other"]).assert().success();
    sb.write("a.txt", "main tip\n");
    sb.snapshot("main tip");
    sb.write("a.txt", "wip\n");
    // Refused by default.
    sb.wt(&["switch", "other"]).assert().failure();
    sb.wt(&["config", "set", "auto_snapshot.on_branch_switch", "stash"])
        .assert()
        .success();
    let out = sb.json(&["switch", "other"]);
    assert_eq!(out["stashed"]["message"], "auto-stash on main");
    assert_eq!(sb.read("a.txt"), "1\n");
    sb.wt(&["switch", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to branch main"))
        .stdout(predicate::str::contains(
            "Applied stash@{0}: auto-stash on main",
        ));
    assert_eq!(sb.read("a.txt"), "wip\n");
    assert!(sb
        .json(&["stash", "list"])
        .as_array()
        .expect("list")
        .is_empty());
}
