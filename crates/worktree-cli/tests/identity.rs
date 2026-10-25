//! `wt identity` and `wt snapshot --sign` against a private `WT_HOME`
//! (ENGINEERING_PLAN §13.6). Server registration is covered in tests/iam.rs.

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
            home: tempfile::tempdir().expect("home"),
            dir: tempfile::tempdir().expect("dir"),
        };
        sb.wt(&["init", "--name", "proj"]).assert().success();
        sb
    }

    fn wt(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("wt").expect("wt binary");
        cmd.current_dir(self.dir.path())
            .env("WT_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .env_remove("WT_SERVER")
            .env_remove("WT_LOG")
            .args(args);
        cmd
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut full = vec!["--json"];
        full.extend_from_slice(args);
        let out = self.wt(&full).output().expect("run");
        assert!(
            out.status.success(),
            "wt {full:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("json")
    }

    fn write(&self, rel: &str, content: &str) {
        std::fs::write(self.dir.path().join(rel), content).expect("write");
    }

    fn keys(&self) -> std::path::PathBuf {
        self.home.path().join("keys")
    }
}

fn exists(path: &Path) -> bool {
    path.is_file()
}

#[test]
fn generate_list_show() {
    let sb = Sandbox::init();
    let empty = sb.json(&["identity", "list"]);
    assert_eq!(empty, Value::Array(vec![]));

    let key = sb.json(&["identity", "generate"]);
    assert_eq!(key["name"], "default");
    assert_eq!(key["algorithm"], "ed25519");
    let fp = key["fingerprint"].as_str().unwrap().to_string();
    assert!(fp.starts_with("SHA256:"), "{fp}");
    assert!(exists(&sb.keys().join("default.key")));
    assert!(exists(&sb.keys().join("default.pub")));

    // A second generate of the same name needs --force.
    sb.wt(&["identity", "generate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    let replaced = sb.json(&["identity", "generate", "--force"]);
    assert_ne!(replaced["fingerprint"].as_str().unwrap(), fp);

    sb.json(&["identity", "generate", "work"]);
    let list = sb.json(&["identity", "list"]);
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["default", "work"]);

    let shown = sb.json(&["identity", "show", "work"]);
    assert_eq!(shown["name"], "work");
    sb.wt(&["identity", "show", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fingerprint:"))
        .stdout(predicate::str::contains("public key:"));
    sb.wt(&["identity", "show", "nope"]).assert().failure();
    sb.wt(&["identity", "generate", "../evil"])
        .assert()
        .failure();
}

#[test]
fn snapshot_sign_and_verify() {
    let sb = Sandbox::init();
    sb.write("a.txt", "a\n");

    // --sign without a key fails and creates nothing.
    sb.wt(&["snapshot", "-m", "first", "--sign"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("wt identity generate"));

    sb.json(&["identity", "generate"]);
    let snap = sb.json(&["snapshot", "-m", "first", "--sign"]);
    assert!(snap["signature"].is_object(), "{snap}");

    let verified = sb.json(&["identity", "verify"]);
    assert_eq!(verified["status"], "valid");
    assert_eq!(verified["snapshot"], snap["id"]);
    sb.wt(&["identity", "verify", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::contains("good signature by default"));

    // Unsigned snapshots do not verify.
    sb.write("a.txt", "b\n");
    sb.json(&["snapshot", "-m", "second"]);
    sb.wt(&["identity", "verify"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("unsigned"));

    // [signing] auto signs every snapshot.
    let config = sb.dir.path().join(".wt").join("config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str("\n[signing]\nkey = \"default\"\nauto = true\n");
    std::fs::write(&config, text).unwrap();
    sb.write("a.txt", "c\n");
    let auto = sb.json(&["snapshot", "-m", "third"]);
    assert!(auto["signature"].is_object(), "{auto}");
    sb.wt(&["identity", "verify"]).assert().success();

    // Without the key the signature cannot be checked.
    std::fs::remove_dir_all(sb.keys()).unwrap();
    sb.wt(&["identity", "verify"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("unknown key"));
}
