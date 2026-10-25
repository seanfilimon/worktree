//! End-to-end tests of the v0.5 merge, diff and history commands
//! (ENGINEERING_PLAN §13.10). Each test runs in a private temp dir with
//! its own `WT_HOME`.

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

    fn stdout(&self, args: &[&str]) -> String {
        let output = self.wt(args).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8")
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

    fn snapshot(&self, message: &str) -> Value {
        self.json(&["snapshot", "-m", message])
    }

    /// main and feature both change line 2 of a.txt; feature also changes
    /// line 4.
    fn diverge(&self) {
        self.write("a.txt", "1\n2\n3\n4\n");
        self.snapshot("base");
        self.wt(&["switch", "-c", "feature"]).assert().success();
        self.write("a.txt", "1\nfeature\n3\nfour\n");
        self.snapshot("feature edit");
        self.wt(&["switch", "main"]).assert().success();
        self.write("a.txt", "1\nmain\n3\n4\n");
        self.snapshot("main edit");
    }
}

#[test]
fn merge_strategy_theirs_completes() {
    let sb = Sandbox::init();
    sb.diverge();
    let merged = sb.json(&["merge", "feature", "--strategy", "theirs"]);
    assert_eq!(
        merged["Merged"]["snapshot"]["parents"]
            .as_array()
            .expect("parents")
            .len(),
        2
    );
    assert_eq!(sb.read("a.txt"), "1\nfeature\n3\nfour\n");
}

#[test]
fn merge_conflict_status_resolve_continue() {
    let sb = Sandbox::init();
    sb.diverge();
    let output = sb
        .wt(&["--json", "merge", "feature"])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let outcome: Value = serde_json::from_slice(&output.stdout).expect("json");
    let hunks = &outcome["Conflicts"]["conflicts"][0]["hunks"];
    assert_eq!(hunks[0]["ours_start"], 2);
    assert_eq!(hunks[0]["theirs_len"], 1);
    let text = sb.read("a.txt");
    assert!(text.contains("<<<<<<< ours (main "), "{text}");
    assert!(text.contains(">>>>>>> theirs (feature "), "{text}");

    let status = sb.stdout(&["merge", "status"]);
    assert!(status.contains("Merging feature"), "{status}");
    assert!(
        status.contains("unresolved   a.txt (content) [ours 2,1 / theirs 2,1]"),
        "{status}"
    );
    let status = sb.json(&["merge", "status"]);
    assert_eq!(status["conflicts"][0]["resolved"], false);

    sb.wt(&["merge", "resolve", "a.txt"]).assert().code(2);
    sb.wt(&["merge", "resolve", "nope.txt", "--ours"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("has no merge conflict"));
    let resolved = sb.json(&["merge", "resolve", "a.txt", "--ours"]);
    assert_eq!(resolved["resolved"], serde_json::json!(["a.txt"]));
    assert_eq!(resolved["remaining"], serde_json::json!([]));
    // Conflicting line from main, clean change (line 4) from feature.
    assert_eq!(sb.read("a.txt"), "1\nmain\n3\nfour\n");
    assert_eq!(
        sb.json(&["merge", "status"])["conflicts"][0]["resolved"],
        true
    );

    let merged = sb.json(&["merge", "--continue"]);
    assert_eq!(merged["parents"].as_array().expect("parents").len(), 2);
    assert_eq!(sb.stdout(&["merge", "status"]), "No merge in progress.\n");
    assert_eq!(sb.json(&["merge", "status"]), Value::Null);
}

#[test]
fn merge_resolve_file_all_and_abort_subcommand() {
    let sb = Sandbox::init();
    sb.diverge();
    sb.wt(&["merge", "feature"]).assert().code(3);
    // Outside the worktree, so it does not dirty the tree for the next merge.
    let fixed = sb.home.path().join("fixed.txt");
    fs::write(&fixed, "hand made\n").expect("write");
    sb.json(&[
        "merge",
        "resolve",
        "a.txt",
        "--file",
        fixed.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(sb.read("a.txt"), "hand made\n");
    sb.json(&["merge", "abort"]);
    assert_eq!(sb.read("a.txt"), "1\nmain\n3\n4\n");

    sb.wt(&["merge", "feature"]).assert().code(3);
    let all = sb.json(&["merge", "resolve", "--all", "--theirs"]);
    assert_eq!(all["resolved"], serde_json::json!(["a.txt"]));
    assert_eq!(sb.read("a.txt"), "1\nfeature\n3\nfour\n");
    sb.json(&["merge", "--continue", "-m", "took theirs"]);
}

#[test]
fn revert_message_and_parent() {
    let sb = Sandbox::init();
    sb.write("a.txt", "a\n");
    sb.snapshot("base");
    sb.wt(&["switch", "-c", "feature"]).assert().success();
    sb.write("b.txt", "b\n");
    sb.snapshot("add b");
    sb.wt(&["switch", "main"]).assert().success();
    sb.write("c.txt", "c\n");
    sb.snapshot("add c");
    let merge = sb.json(&["merge", "feature"]);
    let merge_id = merge["Merged"]["snapshot"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    sb.wt(&["revert", "HEAD"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--parent"));
    let revert = sb.json(&["revert", "HEAD", "--parent", "1", "-m", "drop feature"]);
    assert_eq!(revert["message"], "drop feature");
    assert_eq!(revert["reverts"], merge_id.as_str());
    assert!(!sb.path().join("b.txt").exists());

    let show = sb.stdout(&["show", "HEAD"]);
    assert!(show.contains(&format!("Reverts: {merge_id}")), "{show}");
}

#[test]
fn diff_renames_word_diff_and_name_status() {
    let sb = Sandbox::init();
    let body: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    sb.write("old.txt", &body);
    sb.write("words.txt", "the quick brown fox\n");
    sb.snapshot("one");
    fs::remove_file(sb.path().join("old.txt")).expect("rm");
    sb.write("new.txt", &body);
    sb.write("words.txt", "the slow brown fox\n");

    assert_eq!(
        sb.stdout(&["diff", "--name-status"]),
        "A\tnew.txt\nD\told.txt\nM\twords.txt\n"
    );
    assert_eq!(
        sb.stdout(&["diff", "--name-status", "--find-renames"]),
        "R100\told.txt\tnew.txt\nM\twords.txt\n"
    );
    let patch = sb.stdout(&["diff", "-M"]);
    assert!(
        patch.contains(
            "diff --wt a/old.txt b/new.txt\nrenamed: old.txt -> new.txt (similarity 100%)\n"
        ),
        "{patch}"
    );
    let json = sb.json(&["diff", "--find-renames=90"]);
    assert_eq!(json[0]["old_path"], "old.txt");
    assert_eq!(json[0]["similarity"], 100);
    let stat = sb.stdout(&["diff", "--stat", "-M"]);
    assert!(stat.contains("old.txt => new.txt"), "{stat}");
    assert!(stat.contains("2 files changed"), "{stat}");

    let words = sb.stdout(&["diff", "--word-diff", "--", "words.txt"]);
    assert!(
        words.contains("the [-quick-]{+slow+} brown fox\n"),
        "{words}"
    );
}

#[test]
fn reflog_branch_limit_revisions_and_expire() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n");
    let one = sb.snapshot("one");
    sb.write("a.txt", "2\n");
    sb.snapshot("two");
    sb.wt(&["switch", "-c", "feature"]).assert().success();
    sb.write("a.txt", "3\n");
    sb.snapshot("three");

    let main = sb.json(&["reflog", "--branch", "main"]);
    let main = main.as_array().expect("entries");
    assert!(main.iter().all(|e| e["branch"] == "main"));
    assert_eq!(
        sb.json(&["reflog", "--limit", "2"])
            .as_array()
            .expect("e")
            .len(),
        2
    );
    let human = sb.stdout(&["reflog", "--branch", "main"]);
    assert!(human.contains("main@{1}: snapshot: one"), "{human}");
    sb.wt(&["reflog", "--branch", "nope"]).assert().code(1);

    // Revisions through the reflog.
    let show = sb.json(&["show", "main@{1}"]);
    assert_eq!(show["snapshot"]["id"], one["id"]);
    let log = sb.json(&["log", "-n", "1", "@{0}"]);
    assert_eq!(log[0]["message"], "three");

    let expired = sb.json(&["reflog", "expire", "--keep", "2"]);
    assert_eq!(expired["kept"], 2);
    assert!(expired["removed"].as_u64().expect("removed") > 0);
    assert_eq!(sb.json(&["reflog"]).as_array().expect("e").len(), 2);
    let all = sb.json(&["reflog", "expire", "--older-than", "now", "--all-trees"]);
    assert_eq!(all["kept"], 0);
    sb.wt(&["reflog", "expire", "--older-than", "soon"])
        .assert()
        .code(2);
}

#[test]
fn log_show_signature_and_show_dependencies() {
    let sb = Sandbox::init();
    sb.write("a.txt", "1\n");
    sb.snapshot("one");
    let human = sb.stdout(&["log", "--show-signature"]);
    assert!(human.contains("Signature: unsigned"), "{human}");
    let oneline = sb.stdout(&["log", "--oneline", "--show-signature"]);
    assert!(oneline.contains("[unsigned]"), "{oneline}");
    let json = sb.json(&["log", "--show-signature"]);
    assert_eq!(json[0]["signature_status"], "unsigned");
    assert_eq!(json[0]["message"], "one");
    // Without the flag the JSON shape is unchanged.
    assert!(sb.json(&["log"])[0].get("signature_status").is_none());
}
