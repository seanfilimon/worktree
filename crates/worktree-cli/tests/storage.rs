//! `wt gc` and `wt fsck` (docs/ENGINEERING_PLAN.md §14.6).

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
    dir: TempDir,
}

impl Sandbox {
    fn init() -> Self {
        let sb = Self {
            home: tempfile::tempdir().expect("home"),
            dir: tempfile::tempdir().expect("dir"),
        };
        sb.wt(&["init", "--name", "proj"]).assert().success();
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
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    /// Run with `--json`; returns (success, stdout document).
    fn json(&self, args: &[&str]) -> (bool, Value) {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.wt(&full).output().expect("run");
        let doc = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "wt {full:?}: {e}\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (out.status.success(), doc)
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, content).expect("write");
    }

    fn objects(&self) -> PathBuf {
        self.path().join(".wt").join("objects")
    }
}

#[test]
fn gc_and_fsck_round_trip() {
    let sb = Sandbox::init();
    sb.write("README.md", &"# project\n".repeat(100));
    sb.wt(&["snapshot", "-m", "first"]).assert().success();

    let (ok, report) = sb.json(&["fsck", "--full"]);
    assert!(ok, "{report}");
    assert_eq!(report["errors"], Value::Array(vec![]));
    assert!(report["objects_checked"].as_u64().unwrap() >= 2);

    // A loose unreachable object: a dry run with --prune 0 would delete it.
    let garbage = worktree_sdk::ObjectStore::open(sb.objects())
        .put(b"unreferenced bytes")
        .unwrap();
    let (ok, dry) = sb.json(&["gc", "--dry-run", "--prune", "0"]);
    assert!(ok);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["objects_deleted"], 1, "{dry}");
    let hex = garbage.to_hex();
    let garbage_file = sb.objects().join(&hex[..2]).join(&hex[2..]);
    assert!(garbage_file.is_file());

    // The default window keeps it (written just now)...
    let (ok, kept) = sb.json(&["gc"]);
    assert!(ok);
    assert_eq!(kept["objects_deleted"], 0, "{kept}");
    assert!(garbage_file.is_file());
    // ...--prune 0 removes it; history stays intact.
    sb.wt(&["gc", "--prune", "0"])
        .assert()
        .success()
        .stdout(predicates::str::contains("deleted"));
    assert!(!garbage_file.exists());
    sb.wt(&["fsck"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No problems found"));
    sb.wt(&["log"]).assert().success();
}

#[test]
fn fsck_reports_corruption_and_exits_1() {
    let sb = Sandbox::init();
    sb.write("a.txt", "some content\n");
    sb.wt(&["snapshot", "-m", "first"]).assert().success();
    let blob = worktree_sdk::store::hash_bytes(b"some content\n").to_hex();
    let file = sb.objects().join(&blob[..2]).join(&blob[2..]);
    fs::write(&file, b"tampered").unwrap();

    // Presence-only checks pass; --full re-hashes and fails.
    sb.wt(&["fsck"]).assert().success();
    let (ok, report) = sb.json(&["fsck", "--full"]);
    assert!(!ok);
    assert_eq!(report["errors"][0]["kind"], "corrupt_object", "{report}");
    assert_eq!(report["errors"][0]["id"], blob.as_str());
    sb.wt(&["fsck", "--full"])
        .assert()
        .code(1)
        .stdout(predicates::str::contains("corrupt object:"))
        .stderr(predicates::str::contains("fsck found 1 problem"));

    // A lost blob: gc still runs (it only decodes manifests and chunk
    // lists) and fsck reports the hole.
    fs::remove_file(&file).unwrap();
    sb.wt(&["gc", "--prune", "0"]).assert().success();
    let (ok, report) = sb.json(&["fsck"]);
    assert!(!ok);
    assert_eq!(report["errors"][0]["kind"], "missing_object");
}
