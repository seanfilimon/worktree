//! `wt config` layers, `wt ignore`, `wt tree validate|init-config` and
//! `wt snapshot --no-verify` (docs/ENGINEERING_PLAN.md §13.9).

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

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.wt(args).output().expect("run");
        assert!(
            out.status.success(),
            "wt {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, content).expect("write");
    }
}

#[test]
fn config_layers_and_origins() {
    let sb = Sandbox::init();
    sb.wt(&["tree", "add", "web", "apps/web"])
        .assert()
        .success();

    // --global writes <wt home>/config.toml.
    sb.wt(&["config", "set", "--global", "user.name", "Global Ada"])
        .assert()
        .success();
    assert!(fs::read_to_string(sb.home.path().join("config.toml"))
        .expect("user config")
        .contains("Global Ada"));
    let got = sb.json(&["config", "get", "--show-origin", "user.name"]);
    assert_eq!(got["value"], "Global Ada");
    assert_eq!(got["origin"]["level"], "user");
    // The user config cannot hold worktree-only sections.
    sb.wt(&["config", "set", "--global", "sync.remote", "x"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot be set in the user config"));

    // --tree writes the tree's .wt-tree/config.toml.
    sb.wt(&[
        "--tree",
        "web",
        "config",
        "set",
        "auto_snapshot.inactivity_timeout_secs",
        "3",
    ])
    .assert()
    .success();
    let tree_cfg = fs::read_to_string(sb.path().join("apps/web/.wt-tree/config.toml")).unwrap();
    assert!(tree_cfg.contains("inactivity_secs = 3"), "{tree_cfg}");
    let got = sb.json(&[
        "--tree",
        "web",
        "config",
        "get",
        "--show-origin",
        "auto_snapshot.inactivity_secs",
    ]);
    assert_eq!(got["value"], "3");
    assert_eq!(got["origin"]["level"], "tree");
    assert_eq!(got["origin"]["tree"], "web");
    // The root tree keeps the worktree value.
    assert_eq!(
        sb.stdout(&["config", "get", "auto_snapshot.inactivity_secs"]),
        "30\n"
    );
    let text = sb.stdout(&["--tree", "web", "config", "list", "--show-origin"]);
    assert!(text.contains("default\thooks.timeout_secs=60"), "{text}");
    assert!(text.contains("worktree:"), "{text}");
    let list = sb.json(&["config", "list", "--show-origin"]);
    assert!(list
        .as_array()
        .expect("rows")
        .iter()
        .any(|r| r["key"] == "worktree.name"));

    sb.wt(&[
        "--tree",
        "web",
        "config",
        "unset",
        "auto_snapshot.inactivity_secs",
    ])
    .assert()
    .success();
    assert_eq!(
        sb.stdout(&[
            "--tree",
            "web",
            "config",
            "get",
            "auto_snapshot.inactivity_secs"
        ]),
        "30\n"
    );
    sb.wt(&[
        "--tree",
        "web",
        "config",
        "set",
        "--global",
        "user.name",
        "x",
    ])
    .assert()
    .code(2);
}

#[test]
fn config_set_keeps_unknown_keys() {
    let sb = Sandbox::init();
    let path = sb.path().join(".wt/config.toml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("\n[plugin]\nmode = \"fast\"\n");
    fs::write(&path, text).unwrap();
    sb.wt(&["config", "set", "user.email", "a@example.com"])
        .assert()
        .success();
    sb.wt(&[
        "remote",
        "add",
        "origin",
        "http://localhost:7420",
        "--tenant",
        "acme",
    ])
    .assert()
    .success();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("[plugin]") && text.contains("mode = \"fast\""),
        "{text}"
    );
}

#[test]
fn ignore_tree_file_and_sources() {
    let sb = Sandbox::init();
    sb.wt(&["tree", "add", "web", "apps/web"])
        .assert()
        .success();
    let added = sb.json(&["--tree", "web", "ignore", "add", "--in-tree", "cache/"]);
    assert_eq!(added["file"], "apps/web/.wt-tree/ignore");
    let rules = sb.json(&["ignore", "list", "--sources"]);
    let rules = rules.as_array().expect("rules");
    assert!(
        rules
            .iter()
            .any(|r| r["source"] == "apps/web/.wt-tree/ignore"
                && r["pattern"] == "apps/web/**/cache/")
    );
    assert!(rules
        .iter()
        .any(|r| r["source"] == "soft" && r["pattern"] == "dist/"));
    assert_eq!(
        sb.json(&["ignore", "check", "apps/web/cache/x"])["ignored"],
        true
    );
    // Root rules cannot be re-included from a nested .wtignore.
    sb.wt(&["ignore", "add", "*.secret"]).assert().success();
    sb.write("sub/.wtignore", "!*.secret\n");
    assert_eq!(
        sb.json(&["ignore", "check", "sub/a.secret"])["ignored"],
        true
    );
}

#[test]
fn tree_validate_and_init_config() {
    let sb = Sandbox::init();
    sb.wt(&["tree", "add", "web", "apps/web"])
        .assert()
        .success();
    let report = sb.json(&["tree", "validate"]);
    assert!(report
        .as_array()
        .expect("report")
        .iter()
        .all(|r| r["problems"].as_array().expect("problems").is_empty()));

    let created = sb.json(&["tree", "init-config", "web"]);
    let created: Vec<&str> = created["created"]
        .as_array()
        .expect("created")
        .iter()
        .map(|v| v.as_str().expect("path"))
        .collect();
    assert_eq!(
        created,
        // hardening (§15.10): `wt tree add` writes only the local
        // identity file, so init-config also scaffolds config.toml.
        vec![
            "apps/web/.wt-tree/config.toml",
            "apps/web/.wt-tree/ignore",
            "apps/web/.wt-tree/hooks",
            "apps/web/.wt-tree/access"
        ]
    );
    sb.wt(&["tree", "init-config", "proj"]).assert().code(1);

    sb.write("apps/web/.wt-tree/ignore", "bad[z-a]\n");
    sb.wt(&["tree", "validate", "web"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("line 1"))
        .stderr(predicate::str::contains("1 error(s)"));
}

#[cfg(windows)]
fn write_failing_hook(sb: &Sandbox) {
    sb.write(
        ".wt/hooks/pre-snapshot.cmd",
        "@echo off\r\necho no snapshots today 1>&2\r\nexit /b 1\r\n",
    );
}

#[cfg(unix)]
fn write_failing_hook(sb: &Sandbox) {
    use std::os::unix::fs::PermissionsExt;
    sb.write(
        ".wt/hooks/pre-snapshot",
        "#!/bin/sh\necho no snapshots today >&2\nexit 1\n",
    );
    let path = sb.path().join(".wt/hooks/pre-snapshot");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn snapshot_no_verify_skips_failing_hook() {
    let sb = Sandbox::init();
    write_failing_hook(&sb);
    sb.write("a.txt", "a\n");
    sb.wt(&["snapshot", "-m", "blocked"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("pre-snapshot hook failed"))
        .stderr(predicate::str::contains("no snapshots today"));
    let snap = sb.json(&["snapshot", "-m", "forced", "--no-verify"]);
    assert_eq!(snap["message"], "forced");
}

#[test]
fn warn_only_secret_scanning_snapshots_with_a_warning() {
    let sb = Sandbox::init();
    sb.wt(&["config", "set", "secret_scanning.block_on_match", "false"])
        .assert()
        .success();
    sb.write(
        "config.env",
        &format!("AWS_ACCESS_KEY_ID={}{}\n", "AKIA", "Z4Q7RJ2KXW3MPLQ9"),
    );
    sb.wt(&["snapshot", "-m", "warned"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: possible secret config.env:1",
        ));
}
