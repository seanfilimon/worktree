//! Local engine configuration (docs/ENGINEERING_PLAN.md §13.9): layered
//! config, ignore semantics, configurable secret scanning, hooks and tree
//! layout checks.

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use common::{write_file, Fixture};
use worktree_sdk::config::{ConfigLevel, ConfigScope, Severity, WorktreeConfig};
use worktree_sdk::{SdkError, SnapshotOptions};

/// Every test in this binary shares one empty `WT_HOME` (no user config).
fn isolate_home() {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("home").keep();
        std::env::set_var("WT_HOME", &dir);
        dir
    });
}

fn fixture() -> Fixture {
    isolate_home();
    Fixture::new()
}

/// A fixture with a nested tree "svc" at `services/svc`.
fn with_tree() -> Fixture {
    let fx = fixture();
    fx.repo
        .add_tree("svc", Path::new("services/svc"))
        .expect("add tree");
    fx
}

fn scan_opts(message: &str) -> SnapshotOptions {
    SnapshotOptions {
        message: message.into(),
        ..SnapshotOptions::default()
    }
}

fn aws_line() -> String {
    // Assembled at runtime so no literal key is stored in the source.
    format!("AWS_ACCESS_KEY_ID={}{}\n", "AKIA", "Z4Q7RJ2KXW3MPLQ9")
}

// ---------------------------------------------------------------------------
// Config hierarchy
// ---------------------------------------------------------------------------

#[test]
fn tree_config_overrides_worktree_with_origins() {
    let mut fx = with_tree();
    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "auto_snapshot.inactivity_secs",
            "40",
        )
        .unwrap();
    fx.repo
        .set_config_value(
            &ConfigScope::Tree("svc".into()),
            "auto_snapshot.inactivity_timeout_secs",
            "5",
        )
        .unwrap();

    let root_cfg = fx.repo.effective_config(Some(&fx.tree)).unwrap();
    assert_eq!(root_cfg.auto_snapshot.inactivity_secs, 40);
    let svc_cfg = fx.repo.effective_config(Some("svc")).unwrap();
    assert_eq!(svc_cfg.auto_snapshot.inactivity_secs, 5);

    let stack = fx.repo.config_stack(Some("svc")).unwrap();
    let entry = stack.get("auto_snapshot.inactivity_secs").unwrap().unwrap();
    assert_eq!(entry.value, "5");
    assert_eq!(entry.origin.level, ConfigLevel::Tree);
    assert_eq!(entry.origin.tree.as_deref(), Some("svc"));
    assert!(entry.origin.to_string().starts_with("tree svc:"));
    let name = stack.get("worktree.name").unwrap().unwrap();
    assert_eq!(name.origin.level, ConfigLevel::Worktree);
    let hooks = stack.get("hooks.timeout_secs").unwrap().unwrap();
    assert_eq!(hooks.origin.level, ConfigLevel::Default);
    assert_eq!(hooks.origin.to_string(), "default");
    assert!(stack
        .entries()
        .unwrap()
        .iter()
        .any(|e| e.key == "auto_snapshot.inactivity_secs" && e.value == "5"));

    // The tree file keeps its marker identity and stores the canonical key.
    let text = std::fs::read_to_string(fx.path("services/svc/.wt-tree/config.toml")).unwrap();
    assert!(text.contains("name = \"svc\""), "{text}");
    assert!(text.contains("inactivity_secs = 5"), "{text}");
    assert!(!text.contains("inactivity_timeout_secs"), "{text}");

    // Sections a tree may not set are refused.
    let err = fx
        .repo
        .set_config_value(&ConfigScope::Tree("svc".into()), "user.name", "x")
        .unwrap_err();
    assert!(matches!(err, SdkError::InvalidConfig(_)), "{err}");

    // Unset falls back to the worktree value.
    assert!(fx
        .repo
        .unset_config_value(
            &ConfigScope::Tree("svc".into()),
            "auto_snapshot.inactivity_secs"
        )
        .unwrap());
    assert_eq!(
        fx.repo
            .effective_config(Some("svc"))
            .unwrap()
            .auto_snapshot
            .inactivity_secs,
        40
    );
}

#[test]
fn tree_scope_on_root_tree_writes_worktree_config() {
    let mut fx = fixture();
    let root = fx.tree.clone();
    fx.repo
        .set_config_value(&ConfigScope::Tree(root), "hooks.timeout_secs", "7")
        .unwrap();
    assert_eq!(fx.repo.config().hooks.timeout_secs, 7);
}

#[test]
fn nested_tree_inherits_parent_tree_config() {
    let mut fx = with_tree();
    fx.repo
        .add_tree("inner", Path::new("services/svc/inner"))
        .unwrap();
    fx.repo
        .set_config_value(&ConfigScope::Tree("svc".into()), "hooks.timeout_secs", "9")
        .unwrap();
    let inner = fx.repo.effective_config(Some("inner")).unwrap();
    assert_eq!(inner.hooks.timeout_secs, 9);
}

#[test]
fn set_config_value_preserves_unknown_keys_and_explicit_defaults() {
    let mut fx = fixture();
    let path = fx.path(".wt/config.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n[experimental]\nturbo = true\n");
    std::fs::write(&path, text).unwrap();
    // Reopen so the in-memory config matches the file.
    fx.repo = worktree_sdk::Repository::open(&fx.root()).unwrap();

    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "secret_scanning.block_on_match",
            "true",
        )
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("[experimental]") && text.contains("turbo = true"),
        "{text}"
    );
    assert!(
        text.contains("block_on_match = true"),
        "explicit default kept: {text}"
    );

    // A struct-level save (e.g. `wt remote add`) keeps them too.
    let mut cfg = fx.repo.config().clone();
    cfg.set("remote.origin.url", "http://localhost:7420")
        .unwrap();
    cfg.set("remote.origin.tenant", "acme").unwrap();
    fx.repo.set_config(cfg).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("turbo = true"), "{text}");
    assert!(text.contains("block_on_match = true"), "{text}");
    assert!(WorktreeConfig::load(&path).is_ok());

    assert!(matches!(
        fx.repo.set_config_value(
            &ConfigScope::Worktree,
            "auto_snapshot.max_changed_files",
            "x"
        ),
        Err(SdkError::InvalidConfig(_))
    ));
    assert!(fx
        .repo
        .unset_config_value(&ConfigScope::Worktree, "worktree.name")
        .is_err());
}

#[test]
fn broken_tree_config_is_reported_with_its_path() {
    let fx = with_tree();
    write_file(
        &fx.path("services/svc/.wt-tree/config.toml"),
        "name = \"svc\"\n[hooks]\ntimeout_secs = \"soon\"\n",
    );
    let err = fx.repo.effective_config(Some("svc")).unwrap_err();
    assert!(err.to_string().contains("config.toml"), "{err}");
}

// ---------------------------------------------------------------------------
// Ignore rules
// ---------------------------------------------------------------------------

#[test]
fn root_ignore_cannot_be_negated_by_nested_wtignore() {
    let fx = fixture();
    fx.write(".wtignore", "*.secret\n");
    fx.write("sub/.wtignore", "!*.secret\n");
    fx.write("sub/a.secret", "x");
    fx.write("sub/keep.txt", "x");
    assert!(fx.repo.is_ignored("sub/a.secret").unwrap());
    let status = fx.repo.status(&fx.tree).unwrap();
    let paths: Vec<_> = status.changes.iter().map(|c| c.path.as_str()).collect();
    assert!(paths.contains(&"sub/keep.txt"), "{paths:?}");
    assert!(!paths.contains(&"sub/a.secret"), "{paths:?}");
}

#[test]
fn tree_ignore_file_is_additive_and_soft_defaults_apply() {
    let fx = with_tree();
    fx.write(".wtignore", "*.tmp\n");
    fx.repo.add_tree_ignore_pattern("svc", "fixtures/").unwrap();
    fx.repo.add_tree_ignore_pattern("svc", "!*.tmp").unwrap();
    fx.write("services/svc/fixtures/data.json", "{}");
    fx.write("services/svc/a.tmp", "x");
    fx.write("services/svc/app.log", "x");
    fx.write("services/svc/dist/bundle.js", "x");
    fx.write("services/svc/main.rs", "fn main() {}");
    let status = fx.repo.status("svc").unwrap();
    let paths: Vec<_> = status.changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, vec!["main.rs"]);

    // Root rules may re-include a soft default.
    fx.repo.add_ignore_pattern("!dist/").unwrap();
    assert!(!fx.repo.is_ignored("services/svc/dist/bundle.js").unwrap());
    assert!(fx.repo.is_ignored(".git/config").unwrap());

    let rules = fx.repo.ignore_rules().unwrap();
    assert!(rules
        .iter()
        .any(|r| r.pattern == "services/svc/**/fixtures/"
            && r.source == "services/svc/.wt-tree/ignore"));
    assert!(rules
        .iter()
        .any(|r| r.pattern == "*.log" && r.source == "soft"));
    assert!(rules
        .iter()
        .any(|r| r.pattern == ".git/" && r.source == "hard"));
}

#[test]
fn tree_config_survives_set_tree_id() {
    let mut fx = with_tree();
    fx.repo
        .set_config_value(&ConfigScope::Tree("svc".into()), "hooks.enabled", "false")
        .unwrap();
    fx.repo.set_tree_id("svc", "new-id").unwrap();
    let text = std::fs::read_to_string(fx.path("services/svc/.wt-tree/config.toml")).unwrap();
    assert!(text.contains("new-id"), "{text}");
    assert!(!fx.repo.effective_config(Some("svc")).unwrap().hooks.enabled);
}

// ---------------------------------------------------------------------------
// Secret scanning
// ---------------------------------------------------------------------------

#[test]
fn secret_scanning_blocks_by_default() {
    let fx = fixture();
    fx.write("cfg.txt", &aws_line());
    assert!(matches!(
        fx.repo.snapshot(&fx.tree, scan_opts("x")),
        Err(SdkError::SecretsDetected(_))
    ));
}

#[test]
fn secret_scanning_warn_only_disabled_and_excluded() {
    let mut fx = fixture();
    fx.write("cfg.txt", &aws_line());
    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "secret_scanning.block_on_match",
            "false",
        )
        .unwrap();
    let snap = fx.repo.snapshot(&fx.tree, scan_opts("warn only")).unwrap();
    let manifest = fx.repo.manifest_of(&snap).unwrap();
    assert!(manifest.get("cfg.txt").is_some());

    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "secret_scanning.block_on_match",
            "true",
        )
        .unwrap();
    fx.write("fixtures/keys.txt", &aws_line());
    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "secret_scanning.exclude",
            "fixtures/",
        )
        .unwrap();
    fx.repo.snapshot(&fx.tree, scan_opts("excluded")).unwrap();

    fx.write("other.txt", &aws_line());
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "secret_scanning.enabled", "false")
        .unwrap();
    fx.repo.snapshot(&fx.tree, scan_opts("disabled")).unwrap();
}

#[test]
fn custom_secret_patterns_and_tree_level_exclude() {
    let mut fx = with_tree();
    fx.repo
        .set_config_value(
            &ConfigScope::Worktree,
            "secret_scanning.patterns.acme",
            r"ACME-[0-9]{8}",
        )
        .unwrap();
    fx.write("services/svc/src/a.rs", "let id = \"ACME-12345678\";\n");
    match fx.repo.snapshot("svc", scan_opts("x")) {
        Err(SdkError::SecretsDetected(msg)) => assert!(msg.contains("[acme]"), "{msg}"),
        other => panic!("expected SecretsDetected, got {other:?}"),
    }
    // A tree-relative exclude in the tree's own config.
    fx.repo
        .set_config_value(
            &ConfigScope::Tree("svc".into()),
            "secret_scanning.exclude",
            "src/",
        )
        .unwrap();
    fx.repo.snapshot("svc", scan_opts("now excluded")).unwrap();
}

// ---------------------------------------------------------------------------
// Hooks (Windows: .cmd scripts; Unix: sh scripts)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn write_hook(dir: &Path, phase: &str, body: &str) {
    write_file(
        &dir.join(format!("{phase}.cmd")),
        &format!("@echo off\r\n{body}\r\n"),
    );
}

#[cfg(unix)]
fn write_hook(dir: &Path, phase: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(phase);
    write_file(&path, &format!("#!/bin/sh\n{body}\n"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(windows)]
mod script {
    pub const FAIL: &str = "echo lint failed 1>&2\r\nexit /b 3";
    pub const DUMP_ENV: &str = "echo %WT_HOOK% %WT_TREE% %WT_BRANCH% %WT_SNAPSHOT%> \"%WT_ROOT%\\hook-env.txt\"\r\ncd > \"%WT_ROOT%\\hook-cwd.txt\"";
    pub const SLEEP: &str = "ping -n 30 127.0.0.1 >nul";
    pub const MARK: &str = "echo ran> \"%WT_ROOT%\\hook-ran.txt\"";
}

#[cfg(unix)]
mod script {
    pub const FAIL: &str = "echo lint failed >&2\nexit 3";
    pub const DUMP_ENV: &str = "echo \"$WT_HOOK $WT_TREE $WT_BRANCH $WT_SNAPSHOT\" > \"$WT_ROOT/hook-env.txt\"\npwd > \"$WT_ROOT/hook-cwd.txt\"";
    pub const SLEEP: &str = "sleep 30";
    pub const MARK: &str = "echo ran > \"$WT_ROOT/hook-ran.txt\"";
}

#[test]
fn failing_pre_hook_aborts_and_no_verify_skips() {
    let fx = fixture();
    write_hook(&fx.path(".wt/hooks"), "pre-snapshot", script::FAIL);
    fx.write("a.txt", "a");
    let err = fx
        .repo
        .snapshot(&fx.tree, scan_opts("blocked"))
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.starts_with("pre-snapshot hook failed:"), "{msg}");
    assert!(msg.contains("exit code 3"), "{msg}");
    assert!(msg.contains("lint failed"), "{msg}");
    assert!(
        fx.repo.log(&fx.tree, None, Some(10)).unwrap().is_empty(),
        "no snapshot written"
    );

    let snap = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                skip_hooks: true,
                ..scan_opts("skipped")
            },
        )
        .unwrap();
    assert_eq!(snap.message, "skipped");
}

#[test]
fn hooks_disabled_in_config_do_not_run() {
    let mut fx = fixture();
    write_hook(&fx.path(".wt/hooks"), "pre-snapshot", script::FAIL);
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "hooks.enabled", "false")
        .unwrap();
    fx.write("a.txt", "a");
    fx.repo.snapshot(&fx.tree, scan_opts("ok")).unwrap();
}

#[test]
fn pre_hook_timeout_aborts() {
    let mut fx = fixture();
    write_hook(&fx.path(".wt/hooks"), "pre-snapshot", script::SLEEP);
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "hooks.timeout_secs", "1")
        .unwrap();
    fx.write("a.txt", "a");
    let started = std::time::Instant::now();
    let msg = fx
        .repo
        .snapshot(&fx.tree, scan_opts("slow"))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("timed out after 1s"), "{msg}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "hook was not killed: {:?}",
        started.elapsed()
    );
}

#[test]
fn post_hook_gets_environment_and_failures_are_not_fatal() {
    let fx = with_tree();
    let tree_hooks = fx.path("services/svc/.wt-tree/hooks");
    write_hook(&tree_hooks, "post-snapshot", script::DUMP_ENV);
    write_hook(&fx.path(".wt/hooks"), "post-snapshot", script::FAIL);
    write_hook(&tree_hooks, "pre-snapshot", script::MARK);
    fx.write("services/svc/a.txt", "a");
    let snap = fx.repo.snapshot("svc", scan_opts("post")).unwrap();
    assert!(fx.exists("hook-ran.txt"), "tree pre hook ran");
    let env = fx.read("hook-env.txt");
    assert_eq!(
        env.trim(),
        format!("post-snapshot svc main {}", snap.id),
        "{env}"
    );
    let cwd = fx.read("hook-cwd.txt");
    assert!(
        cwd.trim().replace('\\', "/").ends_with("services/svc"),
        "cwd = {cwd}"
    );
    // The root tree does not run the nested tree's hooks.
    std::fs::remove_file(fx.path("hook-ran.txt")).unwrap();
    fx.write("root.txt", "r");
    fx.repo.snapshot(&fx.tree, scan_opts("root")).unwrap();
    assert!(!fx.exists("hook-ran.txt"));
    let hooks =
        worktree_sdk::hooks::find_hooks(&fx.repo, "svc", worktree_sdk::hooks::Phase::PostSnapshot)
            .unwrap();
    assert_eq!(hooks.len(), 2, "{hooks:?}");
    assert!(hooks[0].starts_with(fx.path(".wt")));
}

// ---------------------------------------------------------------------------
// Tree layout
// ---------------------------------------------------------------------------

#[test]
fn validate_and_init_tree_config() {
    let fx = with_tree();
    assert!(fx.repo.validate_tree("svc").unwrap().is_empty());
    assert!(fx.repo.validate_tree(&fx.tree).unwrap().is_empty());

    let created = fx.repo.init_tree_config("svc").unwrap();
    let names: Vec<String> = created
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["ignore", "hooks", "access"],
        "config.toml exists already"
    );
    assert!(
        fx.repo.init_tree_config("svc").unwrap().is_empty(),
        "idempotent"
    );
    assert!(fx.repo.init_tree_config(&fx.tree).is_err());
    assert!(fx.repo.validate_tree("svc").unwrap().is_empty());

    fx.write("services/svc/.wt-tree/ignore", "ok/\nbad[z-a]\n");
    std::fs::create_dir_all(fx.path("services/svc/vendored/.wt")).unwrap();
    fx.write(
        "services/svc/.wt-tree/config.toml",
        "name = \"other\"\nid = \"x\"\n[user]\nname = \"n\"\n",
    );
    let problems = fx.repo.validate_tree("svc").unwrap();
    let text: Vec<String> = problems
        .iter()
        .map(|p| format!("{:?} {} {}", p.severity, p.path, p.message))
        .collect();
    assert!(text.iter().any(|t| t.contains("line 2")), "{text:?}");
    assert!(text.iter().any(|t| t.contains("vendored/.wt")), "{text:?}");
    assert!(text.iter().any(|t| t.contains("registered as")), "{text:?}");
    assert!(
        text.iter().any(|t| t.contains("[user] is not read")),
        "{text:?}"
    );
    assert!(problems.iter().any(|p| p.severity == Severity::Error));

    std::fs::remove_dir_all(fx.path("services/svc/.wt-tree")).unwrap();
    let problems = fx.repo.validate_tree("svc").unwrap();
    assert!(problems.iter().any(|p| p.message.contains("init-config")));
    let created = fx.repo.init_tree_config("svc").unwrap();
    assert_eq!(created.len(), 4);
    let text = fx.read("services/svc/.wt-tree/config.toml");
    assert!(
        text.contains("name = \"svc\"") && text.contains("# [auto_snapshot]"),
        "{text}"
    );
    assert!(fx.repo.effective_config(Some("svc")).is_ok());
}
