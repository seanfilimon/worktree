//! End-to-end tests of the `wt` binary.
//!
//! Every test runs in its own temporary directory with `WT_HOME` pointing
//! at a private temporary home, so no per-user state leaks in or out.
//! Tests that exercise SDK functionality still being implemented by other
//! workstreams are `#[ignore]`d with the workstream they need.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// An isolated sandbox: a working directory plus a private `WT_HOME`.
struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("create WT_HOME"),
            dir: tempfile::tempdir().expect("create work dir"),
        }
    }

    /// A sandbox with an initialized worktree named "proj" on branch main.
    fn init() -> Self {
        let sb = Self::new();
        sb.wt(&["init", "--name", "proj"]).assert().success();
        sb.wt(&["config", "set", "user.name", "Test User"])
            .assert()
            .success();
        sb.wt(&["config", "set", "user.email", "test@example.com"])
            .assert()
            .success();
        sb
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn wt(&self, args: &[&str]) -> Command {
        self.wt_in(self.path(), args)
    }

    fn wt_in(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(dir)
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_PASSWORD")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    /// Run with `--json`, require success, and parse stdout as exactly one
    /// JSON document.
    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let output = self.wt(&full).output().expect("run wt");
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

    /// stdout of a successful run.
    fn stdout(&self, args: &[&str]) -> String {
        let output = self.wt(args).output().expect("run wt");
        assert!(
            output.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("utf-8 stdout")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, content).expect("write file");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path().join(rel)).expect("read file")
    }

    fn snapshot(&self, message: &str) -> Value {
        self.json(&["snapshot", "-m", message])
    }
}

fn ids(log: &Value) -> Vec<String> {
    log.as_array()
        .expect("log is an array")
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Argument handling (no SDK involvement)
// ---------------------------------------------------------------------------

#[test]
fn help_lists_every_command_and_global_flag() {
    let sb = Sandbox::new();
    let assert = sb.wt(&["--help"]).assert().success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    for cmd in [
        "init", "status", "snapshot", "log", "show", "diff", "branch", "switch", "merge", "revert",
        "restore", "tag", "tree", "reflog", "ignore", "config", "remote", "login", "logout",
        "whoami", "register", "push", "fetch", "pull", "clone", "stage", "archive", "git",
        "daemon",
    ] {
        assert!(
            out.contains(&format!("  {cmd} ")),
            "help is missing `{cmd}`:\n{out}"
        );
    }
    for flag in ["--json", "-C <PATH>", "--tree <NAME>", "--no-color"] {
        assert!(out.contains(flag), "help is missing `{flag}`:\n{out}");
    }
}

#[test]
fn every_subcommand_has_help() {
    let sb = Sandbox::new();
    for cmd in [
        &["init"][..],
        &["status"],
        &["snapshot"],
        &["log"],
        &["show"],
        &["diff"],
        &["branch"],
        &["branch", "create"],
        &["switch"],
        &["merge"],
        &["revert"],
        &["restore"],
        &["tag", "create"],
        &["tree", "add"],
        &["reflog"],
        &["ignore", "check"],
        &["config", "set"],
        &["remote", "add"],
        &["login"],
        &["logout"],
        &["whoami"],
        &["register"],
        &["push"],
        &["fetch"],
        &["pull"],
        &["clone"],
        &["stage"],
        &["archive"],
        &["git", "import"],
        &["git", "export"],
        &["daemon", "start"],
        &["daemon", "watch"],
    ] {
        let mut args = cmd.to_vec();
        args.push("--help");
        sb.wt(&args)
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage: wt"));
    }
}

#[test]
fn version_flag() {
    Sandbox::new()
        .wt(&["--version"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(format!(
            "wt {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn usage_errors_exit_with_code_2() {
    let sb = Sandbox::new();
    for args in [
        &[][..],
        &["frobnicate"],
        &["status", "--bogus"],
        &["snapshot"],
        &["merge"],
        &["merge", "feature", "--abort"],
        &["merge", "--continue", "--abort"],
        &["diff", "--stat", "--name-only"],
        &["archive", "-o", "out.rar", "--format", "rar"],
        &["log", "-n", "lots"],
        &["remote", "add", "origin", "http://localhost:7420"],
        &["register", "http://localhost:7420", "-u", "ada"],
    ] {
        sb.wt(args).assert().code(2);
    }
}

#[test]
fn clone_rejects_a_malformed_tree_spec() {
    Sandbox::new()
        .wt(&["clone", "http://localhost:7420", "no-slash"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "error: invalid tree 'no-slash': expected TENANT/TREE",
        ))
        .stderr(predicate::str::contains("hint:"));
}

#[test]
fn login_with_password_stdin_requires_a_username() {
    Sandbox::new()
        .wt(&["login", "http://localhost:7420", "--password-stdin"])
        .write_stdin("secret\n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--username is required"));
}

#[test]
fn dash_c_with_a_missing_directory_fails() {
    Sandbox::new()
        .wt(&["-C", "does/not/exist", "status"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("error: cannot change to"));
}

#[test]
fn git_import_of_a_missing_repository_fails() {
    Sandbox::new()
        .wt(&["git", "import", "no-such-repo"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "error: no Git repository at 'no-such-repo'",
        ));
}

// ---------------------------------------------------------------------------
// Local workflow
// ---------------------------------------------------------------------------

#[test]
fn init_creates_a_worktree() {
    let sb = Sandbox::new();
    sb.wt(&["init", "--name", "proj", "--branch", "trunk"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized empty worktree proj"));
    assert!(sb.path().join(".wt").is_dir());

    sb.wt(&["init"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already initialized"));

    let status = sb.json(&["status"]);
    assert_eq!(status["tree"], "proj");
    assert_eq!(status["branch"], "trunk");
    assert_eq!(status["head"], Value::Null);
}

#[test]
fn init_json_and_explicit_path() {
    let sb = Sandbox::new();
    let out = sb.json(&["init", "sub/project"]);
    assert_eq!(out["tree"]["name"], "project");
    assert_eq!(out["tree"]["default_branch"], "main");
    assert_eq!(out["tree"]["current_branch"], "main");
    assert!(out["root"].as_str().expect("root").contains("project"));
    assert!(sb.path().join("sub/project/.wt").is_dir());
}

#[test]
fn commands_outside_a_worktree_suggest_init() {
    let sb = Sandbox::new();
    for args in [
        &["status"][..],
        &["log"],
        &["snapshot", "-m", "x"],
        &["branch"],
    ] {
        sb.wt(args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains("error: not a worktree"))
            .stderr(predicate::str::contains("hint: run `wt init`"));
    }
}

#[test]
fn snapshot_status_and_log() {
    let sb = Sandbox::init();
    sb.write("a.txt", "one\n");
    sb.write("src/lib.rs", "fn main() {}\n");

    let status = sb.json(&["status"]);
    assert_eq!(status["changes"].as_array().expect("changes").len(), 2);
    assert_eq!(status["changes"][0]["path"], "a.txt");
    assert_eq!(status["changes"][0]["kind"], "added");
    assert_eq!(status["merge"], Value::Null);

    let human = sb.stdout(&["status"]);
    assert!(human.contains("On tree proj, branch main"), "{human}");
    assert!(
        human.contains("added:") && human.contains("src/lib.rs"),
        "{human}"
    );

    let first = sb.snapshot("first snapshot");
    assert_eq!(first["branch"], "main");
    assert_eq!(first["message"], "first snapshot");
    assert_eq!(first["parents"], serde_json::json!([]));
    assert_eq!(first["auto_generated"], false);
    assert!(first["author"]
        .as_str()
        .expect("author")
        .contains("Test User"));

    let clean = sb.stdout(&["status"]);
    assert!(
        clean.contains("nothing to snapshot, working tree clean"),
        "{clean}"
    );
    sb.wt(&["snapshot", "-m", "nothing"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no changes"))
        .stderr(predicate::str::contains("--allow-empty"));
    sb.wt(&["snapshot", "-m", "empty", "--allow-empty"])
        .assert()
        .success();

    sb.write("a.txt", "one\ntwo\n");
    fs::remove_file(sb.path().join("src/lib.rs")).expect("rm");
    let status = sb.json(&["status"]);
    let kinds: Vec<_> = status["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|c| {
            (
                c["path"].as_str().expect("path").to_string(),
                c["kind"].as_str().expect("kind").to_string(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("a.txt".into(), "modified".into()),
            ("src/lib.rs".into(), "deleted".into())
        ]
    );
    sb.wt(&["snapshot", "-m", "third"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[main "))
        .stdout(predicate::str::contains("] third"));

    let log = sb.json(&["log"]);
    assert_eq!(log.as_array().expect("log").len(), 3);
    assert_eq!(log[0]["message"], "third");
    assert_eq!(log[2]["id"], first["id"]);
    assert_eq!(
        sb.json(&["log", "-n", "1"]).as_array().expect("log").len(),
        1
    );

    let oneline = sb.stdout(&["log", "--oneline"]);
    let lines: Vec<_> = oneline.lines().collect();
    assert_eq!(lines.len(), 3, "{oneline}");
    assert!(
        lines[0].contains("(HEAD -> main)") && lines[0].ends_with("third"),
        "{oneline}"
    );
    assert!(
        lines[2].starts_with(&first["id"].as_str().expect("id")[..8]),
        "{oneline}"
    );

    let full = sb.stdout(&["log", "-n", "1"]);
    assert!(full.starts_with("snapshot "), "{full}");
    assert!(full.contains("Author: Test User"), "{full}");
    assert!(full.contains("Date:   "), "{full}");
    assert!(full.contains("    third"), "{full}");

    let from_rev = sb.json(&["log", "HEAD~1"]);
    assert_eq!(from_rev.as_array().expect("log").len(), 2);
}

#[test]
fn show_and_reflog() {
    let sb = Sandbox::init();
    sb.write("a.txt", "one\n");
    sb.snapshot("first");
    sb.write("a.txt", "one\ntwo\n");
    sb.snapshot("second");

    let show = sb.json(&["show", "HEAD"]);
    assert_eq!(show["snapshot"]["message"], "second");
    assert_eq!(show["files"][0]["path"], "a.txt");
    assert_eq!(show["files"][0]["kind"], "modified");
    assert_eq!(show["files"][0]["added"], 1);

    let human = sb.stdout(&["show", "HEAD"]);
    assert!(human.contains("diff --wt a/a.txt b/a.txt"), "{human}");
    assert!(human.contains("+two"), "{human}");
    let stat = sb.stdout(&["show", "HEAD", "--stat"]);
    assert!(stat.contains("a.txt | 1 +"), "{stat}");
    assert!(
        stat.contains("1 file changed, 1 insertion(+), 0 deletions(-)"),
        "{stat}"
    );

    let root = sb.json(&["show", "HEAD~1"]);
    assert_eq!(root["files"][0]["kind"], "added");
    assert_eq!(root["files"][0]["added"], 1);

    let reflog = sb.json(&["reflog"]);
    let actions: Vec<_> = reflog
        .as_array()
        .expect("reflog")
        .iter()
        .map(|e| e["action"].as_str().expect("action").to_string())
        .collect();
    assert_eq!(actions.first().map(String::as_str), Some("snapshot"));
    assert!(actions.contains(&"init".to_string()), "{actions:?}");
    assert_eq!(
        sb.json(&["reflog", "-n", "1"])
            .as_array()
            .expect("reflog")
            .len(),
        1
    );
    assert!(sb
        .stdout(&["reflog"])
        .contains("main@{0}: snapshot: second"));
}

#[test]
fn branches_and_switching() {
    let sb = Sandbox::init();
    sb.write("a.txt", "base\n");
    let base = sb.snapshot("base");

    let created = sb.json(&["branch", "create", "feature"]);
    assert_eq!(created["name"], "feature");
    assert_eq!(created["tip"], base["id"]);
    sb.wt(&["branch", "create", "feature"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));

    let list = sb.json(&["branch"]);
    let names: Vec<_> = list
        .as_array()
        .expect("branches")
        .iter()
        .map(|b| b["name"].as_str().expect("name").to_string())
        .collect();
    assert_eq!(names, ["feature", "main"]);
    assert!(sb.stdout(&["branch", "list"]).contains("* main"));

    sb.wt(&["switch", "feature"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to branch feature"));
    sb.write("a.txt", "base\nfeature work\n");
    sb.snapshot("feature work");

    sb.write("a.txt", "dirty edits that are longer\n");
    sb.wt(&["switch", "main"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("uncommitted changes"))
        .stderr(predicate::str::contains("--force"));
    sb.wt(&["switch", "main", "--force"]).assert().success();
    assert_eq!(sb.read("a.txt"), "base\n");

    let switched = sb.json(&["switch", "-c", "topic"]);
    assert_eq!(switched["branch"], "topic");
    assert_eq!(switched["created"], true);
    assert_eq!(sb.json(&["status"])["branch"], "topic");

    sb.wt(&["switch", "main"]).assert().success();
    sb.json(&["branch", "archive", "topic"]);
    let visible = sb.json(&["branch"]);
    assert!(visible
        .as_array()
        .expect("branches")
        .iter()
        .all(|b| b["name"] != "topic"));
    let all = sb.json(&["branch", "-a"]);
    assert!(all
        .as_array()
        .expect("branches")
        .iter()
        .any(|b| b["name"] == "topic" && b["archived"] == true));
    sb.json(&["branch", "restore", "topic"]);
    assert!(sb
        .json(&["branch"])
        .as_array()
        .expect("branches")
        .iter()
        .any(|b| b["name"] == "topic"));

    let from = sb.json(&["branch", "create", "old", "--from", "HEAD"]);
    assert_eq!(from["tip"], base["id"]);
}

#[test]
fn merge_fast_forward_and_three_way() {
    let sb = Sandbox::init();
    sb.write("a.txt", "base\n");
    sb.snapshot("base");
    sb.wt(&["switch", "-c", "feature"]).assert().success();
    sb.write("b.txt", "feature file\n");
    let feature_tip = sb.snapshot("add b");
    sb.wt(&["switch", "main"]).assert().success();

    let ff = sb.json(&["merge", "feature"]);
    assert_eq!(ff["FastForward"]["to"], feature_tip["id"]);
    assert_eq!(sb.read("b.txt"), "feature file\n");
    assert_eq!(
        sb.json(&["merge", "feature"]),
        Value::String("UpToDate".into())
    );

    sb.wt(&["switch", "feature"]).assert().success();
    sb.write("c.txt", "only on feature\n");
    sb.snapshot("add c");
    sb.wt(&["switch", "main"]).assert().success();
    sb.write("d.txt", "only on main\n");
    sb.snapshot("add d");

    let merged = sb.json(&["merge", "feature", "-m", "merge feature"]);
    let snapshot = &merged["Merged"]["snapshot"];
    assert_eq!(snapshot["message"], "merge feature");
    assert_eq!(snapshot["parents"].as_array().expect("parents").len(), 2);
    assert_eq!(sb.read("c.txt"), "only on feature\n");
    assert_eq!(sb.read("d.txt"), "only on main\n");
    assert!(sb.stdout(&["log", "-n", "1"]).contains("Merge:  "));
}

#[test]
fn merge_conflicts_exit_with_code_3() {
    let sb = Sandbox::init();
    sb.write("a.txt", "line one\nshared line\nline three\n");
    sb.snapshot("base");
    sb.wt(&["switch", "-c", "feature"]).assert().success();
    sb.write(
        "a.txt",
        "line one\nshared line changed on feature\nline three\n",
    );
    sb.snapshot("feature edit");
    sb.wt(&["switch", "main"]).assert().success();
    sb.write(
        "a.txt",
        "line one\nshared line edited on main\nline three\n",
    );
    sb.snapshot("main edit");

    sb.wt(&["merge", "feature"])
        .assert()
        .code(3)
        .stdout(predicate::str::contains("CONFLICT (content): a.txt"))
        .stderr(predicate::str::contains(
            "error: automatic merge failed with 1 conflict",
        ))
        .stderr(predicate::str::contains("wt merge --continue"));
    let conflicted = sb.read("a.txt");
    assert!(
        conflicted.contains("<<<<<<<") && conflicted.contains(">>>>>>>"),
        "{conflicted}"
    );

    let status = sb.json(&["status"]);
    assert_eq!(status["merge"]["source_branch"], "feature");
    assert_eq!(status["merge"]["conflicts"][0]["path"], "a.txt");
    assert_eq!(status["merge"]["conflicts"][0]["kind"], "content");
    assert!(sb.stdout(&["status"]).contains("Merging feature"));

    // Markers still present: continuing is refused with the conflict exit code.
    sb.wt(&["merge", "--continue"]).assert().code(3);

    sb.json(&["merge", "--abort"]);
    assert_eq!(
        sb.read("a.txt"),
        "line one\nshared line edited on main\nline three\n"
    );
    assert_eq!(sb.json(&["status"])["merge"], Value::Null);

    // The JSON outcome is still printed when the merge conflicts.
    let output = sb
        .wt(&["--json", "merge", "feature"])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(3));
    let outcome: Value = serde_json::from_slice(&output.stdout).expect("json outcome");
    assert_eq!(outcome["Conflicts"]["conflicts"][0]["path"], "a.txt");

    sb.write(
        "a.txt",
        "line one\nshared line resolved by hand\nline three\n",
    );
    let merged = sb.json(&["merge", "--continue", "-m", "resolved"]);
    assert_eq!(merged["message"], "resolved");
    assert_eq!(merged["parents"].as_array().expect("parents").len(), 2);
    assert!(sb.json(&["status"])["changes"]
        .as_array()
        .expect("changes")
        .is_empty());
}

#[test]
fn diff_modes() {
    let sb = Sandbox::init();
    sb.write("a.txt", "old line\nkept\n");
    sb.write("docs/guide.md", "guide\n");
    sb.snapshot("first");
    sb.write("a.txt", "new line\nkept\n");
    sb.write("docs/guide.md", "guide\nmore\n");

    let patch = sb.stdout(&["diff"]);
    assert!(patch.contains("diff --wt a/a.txt b/a.txt"), "{patch}");
    assert!(
        patch.contains("-old line") && patch.contains("+new line"),
        "{patch}"
    );
    assert!(patch.contains("@@"), "{patch}");

    assert_eq!(
        sb.stdout(&["diff", "--name-only"]),
        "a.txt\ndocs/guide.md\n"
    );
    let stat = sb.stdout(&["diff", "--stat"]);
    assert!(
        stat.contains("2 files changed, 2 insertions(+), 1 deletion(-)"),
        "{stat}"
    );

    let json = sb.json(&["diff"]);
    assert_eq!(json[0]["path"], "a.txt");
    assert_eq!(json[0]["kind"], "modified");
    assert_eq!(
        (json[0]["added"].as_u64(), json[0]["removed"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(json[0]["binary"], false);

    // Path filters: positional (existing path) and after `--`.
    assert_eq!(
        sb.stdout(&["diff", "--name-only", "docs"]),
        "docs/guide.md\n"
    );
    assert_eq!(
        sb.stdout(&["diff", "--name-only", "--", "a.txt"]),
        "a.txt\n"
    );
    let in_docs = sb
        .wt_in(
            &sb.path().join("docs"),
            &["diff", "--name-only", "guide.md"],
        )
        .output()
        .expect("run");
    assert_eq!(String::from_utf8_lossy(&in_docs.stdout), "docs/guide.md\n");

    sb.wt(&["diff", "no-such-thing"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown revision or path"));

    sb.snapshot("second");
    assert_eq!(sb.stdout(&["diff"]), "");
    assert_eq!(
        sb.stdout(&["diff", "--name-only", "HEAD~1"]),
        "a.txt\ndocs/guide.md\n"
    );
    assert_eq!(
        sb.stdout(&["diff", "--name-only", "HEAD~1", "HEAD", "a.txt"]),
        "a.txt\n"
    );
    let reversed = sb.json(&["diff", "HEAD", "HEAD~1", "--", "docs"]);
    assert_eq!(reversed[0]["removed"], 1);
}

#[test]
fn tags() {
    let sb = Sandbox::init();
    sb.write("a.txt", "v1\n");
    let first = sb.snapshot("first");
    sb.write("a.txt", "v2 longer\n");
    sb.snapshot("second");

    let tag = sb.json(&["tag", "create", "v1", "HEAD~1", "-m", "first release"]);
    assert_eq!(tag["name"], "v1");
    assert_eq!(tag["target"], first["id"]);
    assert_eq!(tag["message"], "first release");
    sb.json(&["tag", "create", "latest"]);
    sb.wt(&["tag", "create", "v1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));

    let names: Vec<_> = sb
        .json(&["tag"])
        .as_array()
        .expect("tags")
        .iter()
        .map(|t| t["name"].as_str().expect("name").to_string())
        .collect();
    assert_eq!(names, ["latest", "v1"]);
    assert!(sb.stdout(&["tag", "list"]).contains("first release"));
    assert!(sb.stdout(&["log", "--oneline"]).contains("tag: v1"));

    // Tags are revisions.
    assert_eq!(sb.json(&["show", "v1"])["snapshot"]["id"], first["id"]);

    sb.json(&["tag", "delete", "latest"]);
    assert_eq!(sb.json(&["tag", "list"]).as_array().expect("tags").len(), 1);
    sb.wt(&["tag", "delete", "latest"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tag not found"));
}

#[test]
fn revert_and_restore() {
    let sb = Sandbox::init();
    sb.write("a.txt", "version one\n");
    sb.snapshot("one");
    sb.write("a.txt", "version two, longer\n");
    sb.write("b.txt", "added in two\n");
    sb.snapshot("two");

    let revert = sb.json(&["revert", "HEAD"]);
    assert_eq!(revert["parents"].as_array().expect("parents").len(), 1);
    assert_eq!(sb.read("a.txt"), "version one\n");
    assert!(!sb.path().join("b.txt").exists());
    assert_eq!(sb.json(&["log"]).as_array().expect("log").len(), 3);

    sb.write("a.txt", "scribbled over the file\n");
    let restored = sb.json(&["restore", "a.txt"]);
    assert_eq!(restored, serde_json::json!(["a.txt"]));
    assert_eq!(sb.read("a.txt"), "version one\n");

    sb.json(&["restore", "--from", "HEAD~1"]);
    assert_eq!(sb.read("a.txt"), "version two, longer\n");
    assert_eq!(sb.read("b.txt"), "added in two\n");

    sb.write("a.txt", "again\n");
    let human = sb.stdout(&["restore"]);
    assert!(human.contains("restored: a.txt"), "{human}");
}

#[test]
fn config_get_set_list() {
    let sb = Sandbox::init();
    assert_eq!(sb.stdout(&["config", "get", "user.name"]), "Test User\n");
    let kv = sb.json(&["config", "get", "user.email"]);
    assert_eq!(
        kv,
        serde_json::json!({"key": "user.email", "value": "test@example.com"})
    );

    sb.wt(&["config", "set", "auto_snapshot.enabled", "false"])
        .assert()
        .success();
    assert_eq!(
        sb.stdout(&["config", "get", "auto_snapshot.enabled"]),
        "false\n"
    );

    let list = sb.json(&["config", "list"]);
    assert_eq!(list["user.name"], "Test User");
    assert_eq!(list["worktree.name"], "proj");
    assert!(sb
        .stdout(&["config", "list"])
        .contains("user.name=Test User"));

    sb.wt(&["config", "set", "no.such.key", "1"])
        .assert()
        .code(1);
    sb.wt(&["config", "get", "no.such.key"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not set"));
}

#[test]
fn remotes() {
    let sb = Sandbox::init();
    assert_eq!(sb.json(&["remote", "list"]), serde_json::json!([]));
    let added = sb.json(&[
        "remote",
        "add",
        "origin",
        "localhost:7420/",
        "--tenant",
        "acme",
    ]);
    assert_eq!(
        added,
        serde_json::json!({"name": "origin", "url": "http://localhost:7420", "tenant": "acme"})
    );
    sb.wt(&["remote", "add", "origin", "http://x", "--tenant", "t"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
    assert_eq!(
        sb.stdout(&["config", "get", "remote.origin.url"]),
        "http://localhost:7420\n"
    );
    assert!(sb
        .stdout(&["remote", "list"])
        .contains("origin  http://localhost:7420  (tenant acme)"));
    sb.json(&["remote", "remove", "origin"]);
    sb.wt(&["remote", "remove", "origin"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("remote not configured"));
}

#[test]
fn ignore_patterns() {
    let sb = Sandbox::init();
    sb.json(&["ignore", "add", "*.log"]);
    let patterns = sb.json(&["ignore", "list"]);
    assert!(
        patterns
            .as_array()
            .expect("patterns")
            .iter()
            .any(|p| p == "*.log"),
        "{patterns}"
    );

    assert_eq!(
        sb.json(&["ignore", "check", "debug.log"]),
        serde_json::json!({"path": "debug.log", "ignored": true})
    );
    assert_eq!(sb.json(&["ignore", "check", "main.rs"])["ignored"], false);
    assert_eq!(sb.json(&["ignore", "check", "target/x"])["ignored"], true);
    assert!(sb
        .stdout(&["ignore", "check", "debug.log"])
        .contains("debug.log is ignored"));

    sb.write("debug.log", "noise\n");
    sb.write("keep.txt", "signal\n");
    let changes = sb.json(&["status"])["changes"].clone();
    assert_eq!(changes.as_array().expect("changes").len(), 1);
    assert_eq!(changes[0]["path"], "keep.txt");
}

#[test]
fn nested_trees() {
    let sb = Sandbox::init();
    sb.write("root.txt", "root\n");
    sb.snapshot("root");

    let tree = sb.json(&["tree", "add", "web", "apps/web"]);
    assert_eq!(tree["name"], "web");
    assert!(sb.path().join("apps/web/.wt-tree").is_dir());
    let names: Vec<_> = sb
        .json(&["tree", "list"])
        .as_array()
        .expect("trees")
        .iter()
        .map(|t| t["name"].as_str().expect("name").to_string())
        .collect();
    assert!(
        names.contains(&"proj".to_string()) && names.contains(&"web".to_string()),
        "{names:?}"
    );
    assert!(sb.stdout(&["tree", "list"]).contains("* proj"));

    sb.write("apps/web/index.html", "<html></html>\n");
    // From inside the nested directory the nested tree is selected.
    let web_dir = sb.path().join("apps/web");
    let out = sb
        .wt_in(&web_dir, &["--json", "status"])
        .output()
        .expect("run");
    let status: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(status["tree"], "web");
    assert_eq!(status["changes"][0]["path"], "index.html");
    // The root tree does not see the nested tree's files.
    assert!(sb.json(&["status"])["changes"]
        .as_array()
        .expect("changes")
        .is_empty());
    // --tree overrides the cwd-based selection.
    assert_eq!(sb.json(&["--tree", "web", "status"])["tree"], "web");
    sb.wt_in(&web_dir, &["snapshot", "-m", "web first"])
        .assert()
        .success();
    assert_eq!(
        sb.json(&["--tree", "web", "log"])[0]["message"],
        "web first"
    );
    assert_eq!(sb.json(&["log"])[0]["message"], "root");

    sb.wt(&["--tree", "nope", "status"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tree not found"));
    sb.json(&["tree", "remove", "web"]);
    assert!(sb.path().join("apps/web/index.html").exists());
}

#[test]
fn dash_c_runs_elsewhere() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.path().join("proj")).expect("mkdir");
    sb.wt(&["-C", "proj", "init"]).assert().success();
    assert!(sb.path().join("proj/.wt").is_dir());
    assert_eq!(sb.json(&["-C", "proj", "status"])["tree"], "proj");
}

#[test]
fn archive_tar_gz_and_zip() {
    let sb = Sandbox::init();
    sb.write("README.md", "# readme\n");
    sb.write("src/main.rs", "fn main() {}\n");
    let snap = sb.snapshot("files");
    sb.write("README.md", "# changed after the snapshot\n");

    let out = sb.json(&["archive", "-o", "out.tar.gz"]);
    assert_eq!(out["format"], "tar.gz");
    assert_eq!(out["files"], 2);
    assert_eq!(out["snapshot"], snap["id"]);
    let bytes = fs::read(sb.path().join("out.tar.gz")).expect("archive");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    let mut files = Vec::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let path = entry
            .path()
            .expect("path")
            .to_string_lossy()
            .replace('\\', "/");
        let mut content = String::new();
        entry.read_to_string(&mut content).expect("read");
        files.push((path, content));
    }
    files.sort();
    assert_eq!(
        files,
        [
            ("README.md".into(), "# readme\n".into()),
            ("src/main.rs".into(), "fn main() {}\n".into())
        ]
    );

    let out = sb.json(&["archive", "HEAD", "-o", "out.zip"]);
    assert_eq!(out["format"], "zip");
    let bytes = fs::read(sb.path().join("out.zip")).expect("zip");
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip archive");
    let mut readme = String::new();
    zip.by_name("README.md")
        .expect("readme")
        .read_to_string(&mut readme)
        .expect("read");
    assert_eq!(readme, "# readme\n");

    let explicit = sb.json(&["archive", "-o", "snapshot.bin", "--format", "zip"]);
    assert_eq!(explicit["format"], "zip");
    assert!(PathBuf::from(explicit["output"].as_str().expect("output")).ends_with("snapshot.bin"));

    sb.wt(&["archive", "nope", "-o", "x.zip"]).assert().code(1);
}

#[test]
fn json_output_is_one_document_per_command() {
    let sb = Sandbox::init();
    sb.write("a.txt", "a\n");
    sb.snapshot("first");
    for args in [
        &["status"][..],
        &["log"],
        &["log", "--oneline"],
        &["show", "HEAD"],
        &["diff"],
        &["branch"],
        &["tag"],
        &["tree", "list"],
        &["reflog"],
        &["ignore", "list"],
        &["config", "list"],
        &["remote", "list"],
    ] {
        // `json` asserts the whole of stdout parses as a single value.
        let value = sb.json(args);
        assert!(value.is_object() || value.is_array(), "{args:?}: {value}");
    }
}

#[test]
fn secret_scan_blocks_snapshots() {
    let sb = Sandbox::init();
    sb.write("config.env", "AWS_ACCESS_KEY_ID=AKIAZ4Q7RJ2KXW3MPLQ9\n");
    sb.wt(&["snapshot", "-m", "oops"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("secrets"))
        .stderr(predicate::str::contains("--no-scan"));
    sb.wt(&["snapshot", "-m", "deliberate", "--no-scan"])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Remote, daemon and Git bridge (other workstreams)
// ---------------------------------------------------------------------------

#[test]
fn whoami_without_credentials() {
    Sandbox::new()
        .wt(&["whoami", "http://127.0.0.1:9"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not logged in"))
        .stderr(predicate::str::contains(
            "hint: run `wt login http://127.0.0.1:9`",
        ));
}

#[test]
fn logout_without_credentials() {
    let sb = Sandbox::new();
    let out = sb.json(&["logout", "http://127.0.0.1:9"]);
    assert_eq!(
        out,
        serde_json::json!({"server": "http://127.0.0.1:9", "was_logged_in": false})
    );
}

#[test]
fn login_to_an_unreachable_server_fails() {
    Sandbox::new()
        .wt(&[
            "login",
            "http://127.0.0.1:9",
            "-u",
            "ada",
            "--password-stdin",
        ])
        .write_stdin("secret\n")
        .assert()
        .code(1)
        .stderr(predicate::str::starts_with("error: "));
}

#[test]
fn push_without_a_remote() {
    let sb = Sandbox::init();
    sb.write("a.txt", "a\n");
    sb.snapshot("first");
    sb.wt(&["push"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("remote not configured: origin"))
        .stderr(predicate::str::contains("wt remote add"));
}

#[test]
fn daemon_status_when_not_running() {
    Sandbox::new()
        .wt(&["daemon", "status"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not running"))
        .stderr(predicate::str::contains("wt daemon start"));
}

#[test]
fn git_import_and_export() {
    let sb = Sandbox::new();
    let git_dir = sb.path().join("src").join("gitrepo");
    fs::create_dir_all(&git_dir).expect("mkdir");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&git_dir)
            .env("GIT_AUTHOR_NAME", "Git User")
            .env("GIT_AUTHOR_EMAIL", "git@example.com")
            .env("GIT_COMMITTER_NAME", "Git User")
            .env("GIT_COMMITTER_EMAIL", "git@example.com")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    fs::write(git_dir.join("hello.txt"), "hello\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first commit"]);
    git(&["tag", "v1"]);

    // Default destination: ./<repository name>.
    let report = sb.json(&["git", "import", "src/gitrepo"]);
    assert_eq!(report["snapshots"], 1);
    assert_eq!(report["tags"], 1);
    assert_eq!(report["branches"], serde_json::json!(["main"]));
    assert_eq!(report["checked_out"], "main");
    assert!(PathBuf::from(report["dest"].as_str().expect("dest")).ends_with("gitrepo"));
    assert!(sb.path().join("gitrepo/.wt").is_dir());

    let imported = sb.path().join("imported");
    sb.json(&["git", "import", "src/gitrepo", "imported"]);
    assert_eq!(
        fs::read_to_string(imported.join("hello.txt")).expect("read"),
        "hello\n"
    );
    let log = sb
        .wt_in(&imported, &["--json", "log"])
        .output()
        .expect("run");
    let log: Value = serde_json::from_slice(&log.stdout).expect("json");
    assert_eq!(ids(&log).len(), 1);
    assert_eq!(
        log[0]["message"].as_str().map(str::trim),
        Some("first commit")
    );

    let export = sb
        .wt_in(
            &imported,
            &[
                "--json",
                "git",
                "export",
                "../exported.git",
                "--rev",
                "main",
            ],
        )
        .output()
        .expect("run");
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    let export: Value = serde_json::from_slice(&export.stdout).expect("json");
    assert_eq!(export["branches"], serde_json::json!(["main"]));
    sb.wt_in(
        &imported,
        &["git", "export", "../exported.git", "--rev", "nope"],
    )
    .assert()
    .code(1)
    .stderr(predicate::str::contains("not a branch"));
}
