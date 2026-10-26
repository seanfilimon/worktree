//! Shared `.wt-tree/` files (docs/ENGINEERING_PLAN.md §15.10): a tree's own
//! `.wt-tree/{config.toml,ignore,hooks/**,access/**}` are snapshotted and
//! materialized like any other file, the identity marker (`local.toml`)
//! stays local, legacy identity lines are migrated out of `config.toml`,
//! and a root `.wt-tree/` (as left by a clone) applies to the root tree.

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use common::{head_paths, snap_tree, Fixture};

fn isolate_home() {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("home").keep();
        std::env::set_var("WT_HOME", &dir);
        dir
    });
}

/// A fixture with a nested tree "svc" at `services/svc`.
fn with_tree() -> Fixture {
    isolate_home();
    let fx = Fixture::new();
    fx.repo
        .add_tree("svc", Path::new("services/svc"))
        .expect("add tree");
    fx
}

fn write_shared(fx: &Fixture, dir: &str) {
    fx.write(
        &format!("{dir}/.wt-tree/config.toml"),
        "[hooks]\ntimeout_secs = 7\n",
    );
    fx.write(&format!("{dir}/.wt-tree/ignore"), "*.cache\n");
    fx.write(
        &format!("{dir}/.wt-tree/hooks/post-snapshot.cmd"),
        "@echo off\r\n",
    );
    fx.write(
        &format!("{dir}/.wt-tree/access/policies.toml"),
        "# no policies\n",
    );
    fx.write(&format!("{dir}/.wt-tree/scratch.txt"), "not shared\n");
}

#[test]
fn nested_tree_tracks_its_shared_files_but_not_its_identity() {
    let fx = with_tree();
    fx.write("root.txt", "root\n");
    fx.write("services/svc/main.rs", "fn main() {}\n");
    write_shared(&fx, "services/svc");
    assert!(fx.exists("services/svc/.wt-tree/local.toml"));
    assert!(
        !fx.exists("services/svc/.wt-tree/config.toml")
            || !fx
                .read("services/svc/.wt-tree/config.toml")
                .contains("name ="),
        "the identity is not in the shared config"
    );

    snap_tree(&fx.repo, "svc", "svc");
    let paths = head_paths(&fx.repo, "svc");
    assert_eq!(
        paths,
        vec![
            ".wt-tree/access/policies.toml",
            ".wt-tree/config.toml",
            ".wt-tree/hooks/post-snapshot.cmd",
            ".wt-tree/ignore",
            "main.rs",
        ]
    );
    // The tree's ignore file applies to its own files.
    fx.write("services/svc/x.cache", "c\n");
    assert!(fx.repo.status("svc").unwrap().is_clean());

    assert!(!fx
        .repo
        .is_ignored("services/svc/.wt-tree/config.toml")
        .unwrap());
    assert!(!fx
        .repo
        .is_ignored("services/svc/.wt-tree/hooks/post-snapshot.cmd")
        .unwrap());
    assert!(fx
        .repo
        .is_ignored("services/svc/.wt-tree/local.toml")
        .unwrap());
    assert!(fx
        .repo
        .is_ignored("elsewhere/.wt-tree/config.toml")
        .unwrap());

    // The parent never sees the nested tree, marker included.
    fx.snap("root");
    assert_eq!(head_paths(&fx.repo, &fx.tree), vec!["root.txt"]);

    // Shared hook scripts are recorded executable even without an exec bit.
    let head = fx.repo.resolve("svc", "HEAD").unwrap();
    let manifest = fx.repo.manifest_of(&head).unwrap();
    let hook = manifest.get(".wt-tree/hooks/post-snapshot.cmd").unwrap();
    assert!(hook.executable, "{hook:?}");
    assert!(!manifest.get(".wt-tree/config.toml").unwrap().executable);
}

#[test]
fn checkout_materializes_shared_files() {
    let fx = with_tree();
    fx.write("services/svc/main.rs", "fn main() {}\n");
    write_shared(&fx, "services/svc");
    let first = snap_tree(&fx.repo, "svc", "with config");

    fx.write(
        "services/svc/.wt-tree/config.toml",
        "[hooks]\ntimeout_secs = 9\n",
    );
    std::fs::remove_dir_all(fx.path("services/svc/.wt-tree/hooks")).unwrap();
    snap_tree(&fx.repo, "svc", "changed config");
    assert_eq!(
        fx.repo
            .effective_config(Some("svc"))
            .unwrap()
            .hooks
            .timeout_secs,
        9
    );

    fx.repo.checkout("svc", &first.id, true).unwrap();
    assert_eq!(
        fx.read("services/svc/.wt-tree/config.toml"),
        "[hooks]\ntimeout_secs = 7\n"
    );
    assert!(fx.exists("services/svc/.wt-tree/hooks/post-snapshot.cmd"));
    assert!(
        fx.exists("services/svc/.wt-tree/local.toml"),
        "the identity survives"
    );
    assert_eq!(
        fx.repo
            .effective_config(Some("svc"))
            .unwrap()
            .hooks
            .timeout_secs,
        7
    );
    // Branch refs do not move: relative to the branch head the restored
    // files are changes.
    let changed: Vec<_> = fx
        .repo
        .status("svc")
        .unwrap()
        .changes
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(
        changed,
        vec![".wt-tree/config.toml", ".wt-tree/hooks/post-snapshot.cmd"]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fx.path("services/svc/.wt-tree/hooks/post-snapshot.cmd"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "hooks keep their executable bit");
    }
}

#[test]
fn legacy_identity_lines_are_migrated_on_the_next_scan() {
    let fx = with_tree();
    // A pre-v0.7 marker: identity and settings in one config.toml.
    std::fs::remove_file(fx.path("services/svc/.wt-tree/local.toml")).unwrap();
    let id = fx.repo.tree("svc").unwrap().id;
    fx.write(
        "services/svc/.wt-tree/config.toml",
        &format!(
            "# Marker identity (written by wt tree add)\nname = \"svc\"\nid = \"{id}\"\n\n# tree settings\n[hooks]\ntimeout_secs = 5\n"
        ),
    );
    fx.write("services/svc/a.txt", "a\n");

    let status = fx.repo.status("svc").unwrap();
    let changed: Vec<_> = status.changes.iter().map(|c| c.path.clone()).collect();
    assert!(
        changed.contains(&".wt-tree/config.toml".to_string()),
        "{changed:?}"
    );
    let config = fx.read("services/svc/.wt-tree/config.toml");
    assert_eq!(config, "# tree settings\n[hooks]\ntimeout_secs = 5\n");
    let identity = fx.read("services/svc/.wt-tree/local.toml");
    assert!(
        identity.contains("name = \"svc\"") && identity.contains(&id),
        "{identity}"
    );
    assert_eq!(
        fx.repo
            .effective_config(Some("svc"))
            .unwrap()
            .hooks
            .timeout_secs,
        5
    );
    assert!(fx.repo.validate_tree("svc").unwrap().is_empty());

    snap_tree(&fx.repo, "svc", "start tracking");
    assert!(head_paths(&fx.repo, "svc").contains(&".wt-tree/config.toml".to_string()));
}

#[test]
fn identity_only_legacy_config_is_removed() {
    let fx = with_tree();
    std::fs::remove_file(fx.path("services/svc/.wt-tree/local.toml")).unwrap();
    let id = fx.repo.tree("svc").unwrap().id;
    fx.write(
        "services/svc/.wt-tree/config.toml",
        &format!("name = \"svc\"\nid = \"{id}\"\n"),
    );
    assert!(fx.repo.status("svc").unwrap().is_clean());
    assert!(!fx.exists("services/svc/.wt-tree/config.toml"));
    assert!(fx.read("services/svc/.wt-tree/local.toml").contains(&id));
}

#[test]
fn root_wt_tree_from_a_clone_applies_to_the_root_tree() {
    isolate_home();
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    write_shared(&fx, ".");
    fx.write(".wt-tree/config.toml", "[hooks]\ntimeout_secs = 11\n");
    fx.write("b.cache", "ignored by the shared ignore file\n");
    fx.snap("root with shared config");
    assert_eq!(
        head_paths(&fx.repo, &fx.tree),
        vec![
            ".wt-tree/access/policies.toml",
            ".wt-tree/config.toml",
            ".wt-tree/hooks/post-snapshot.cmd",
            ".wt-tree/ignore",
            "a.txt",
        ]
    );
    assert_eq!(
        fx.repo
            .effective_config(Some(&fx.tree))
            .unwrap()
            .hooks
            .timeout_secs,
        11
    );
    let rules = fx.repo.ignore_rules().unwrap();
    assert!(
        rules
            .iter()
            .any(|r| r.pattern == "*.cache" && r.source == ".wt-tree/ignore"),
        "{rules:?}"
    );
    assert!(fx.repo.validate_tree(&fx.tree).unwrap().is_empty());
}
