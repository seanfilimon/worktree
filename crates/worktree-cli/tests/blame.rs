//! End-to-end tests of `wt blame` (ENGINEERING_PLAN §14.4). Each test runs
//! in a private temp dir with its own `WT_HOME`.

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

    /// Snapshot and return the new snapshot id.
    fn snapshot(&self, message: &str) -> String {
        let v = self.json(&["snapshot", "-m", message]);
        v["id"]
            .as_str()
            .or_else(|| v["snapshot"]["id"].as_str())
            .unwrap_or_else(|| panic!("no snapshot id in {v}"))
            .to_string()
    }
}

/// Two snapshots: `b` changed and `new` inserted by the second.
fn history(sb: &Sandbox) -> (String, String) {
    sb.write("src/f.txt", "a\nb\nc\n");
    let s1 = sb.snapshot("first");
    sb.write("src/f.txt", "a\nB\nnew\nc\n");
    let s2 = sb.snapshot("second\n\nbody");
    (s1, s2)
}

#[test]
fn blame_json_attributes_lines() {
    let sb = Sandbox::init();
    let (s1, s2) = history(&sb);
    let v = sb.json(&["blame", "src/f.txt"]);
    assert_eq!(v["path"], "src/f.txt");
    assert_eq!(v["snapshot"]["id"], s2.as_str());
    let lines = v["lines"].as_array().unwrap();
    let got: Vec<(u64, &str, &str)> = lines
        .iter()
        .map(|l| {
            (
                l["line"].as_u64().unwrap(),
                l["text"].as_str().unwrap(),
                l["snapshot_id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (1, "a", s1.as_str()),
            (2, "B", s2.as_str()),
            (3, "new", s2.as_str()),
            (4, "c", s1.as_str()),
        ]
    );
    assert_eq!(lines[1]["message"], "second");
    assert_eq!(lines[0]["author"], "Test User");
    assert!(lines[0]["created_at"].is_string());
}

#[test]
fn blame_human_output_rev_and_ranges() {
    let sb = Sandbox::init();
    let (s1, s2) = history(&sb);
    let out = sb.stdout(&["blame", "src/f.txt"]);
    let rows: Vec<&str> = out.lines().collect();
    assert_eq!(rows.len(), 4, "{out}");
    assert!(
        rows[0].starts_with(&format!("{} (Test User ", &s1[..8])),
        "{out}"
    );
    assert!(rows[0].ends_with(" 1) a"), "{out}");
    assert!(rows[1].starts_with(&s2[..8]), "{out}");
    assert!(rows[2].ends_with(" 3) new"), "{out}");

    // -L START,END / START,+COUNT / START, select lines.
    let v = sb.json(&["blame", "src/f.txt", "-L", "2,3"]);
    let nums: Vec<u64> = v["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["line"].as_u64().unwrap())
        .collect();
    assert_eq!(nums, [2, 3]);
    let v = sb.json(&["blame", "src/f.txt", "-L", "3,+5"]);
    assert_eq!(v["lines"].as_array().unwrap().len(), 2);
    let v = sb.json(&["blame", "src/f.txt", "-L", "4,"]);
    assert_eq!(v["lines"][0]["text"], "c");

    // --rev blames an older revision.
    let v = sb.json(&["blame", "src/f.txt", "--rev", "HEAD~1"]);
    assert_eq!(v["snapshot"]["id"], s1.as_str());
    assert_eq!(v["lines"].as_array().unwrap().len(), 3);

    // Paths are relative to the current directory.
    let mut cmd = sb.wt(&["--json", "blame", "f.txt"]);
    cmd.current_dir(sb.path().join("src"));
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["path"], "src/f.txt");
}

#[test]
fn blame_porcelain() {
    let sb = Sandbox::init();
    let (s1, _) = history(&sb);
    let out = sb.stdout(&["blame", "src/f.txt", "--porcelain", "-L", "1,1"]);
    let expected_head = format!("{s1} 1\nauthor Test User\nauthor-time ");
    assert!(out.starts_with(&expected_head), "{out}");
    assert!(
        out.contains("\nsummary first\nfilename src/f.txt\n\ta\n"),
        "{out}"
    );
    assert!(out.contains("\ncreated-at "), "{out}");
    sb.wt(&["--json", "blame", "src/f.txt", "--porcelain"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--porcelain"));
}

#[test]
fn blame_errors() {
    let sb = Sandbox::init();
    sb.wt(&["blame", "x.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no snapshots"));
    history(&sb);
    sb.wt(&["blame", "missing.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
    fs::write(sb.path().join("bin.dat"), [0u8, 159, 146, 150]).unwrap();
    sb.snapshot("binary");
    sb.wt(&["blame", "bin.dat"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("binary"));
    sb.wt(&["blame", "src/f.txt", "-L", "9,10"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("has only 4 lines"));
    sb.wt(&["blame", "src/f.txt", "-L", "0,1"])
        .assert()
        .failure();
    sb.wt(&["blame", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("name a file"));
}
