//! Repository init/open, configuration and the tree registry.

mod common;

use std::fs;
use std::path::Path;

use common::{snap_tree, write_file, Fixture};
use worktree_sdk::{InitOptions, Repository, SdkError};

#[test]
fn init_creates_layout() {
    let fx = Fixture::new();
    let wt = fx.repo.wt_dir();
    for p in [
        "config.toml",
        "ignore",
        "objects",
        "snapshots",
        "trees/demo/tree.toml",
        "trees/demo/HEAD",
    ] {
        assert!(wt.join(p).exists(), "missing .wt/{p}");
    }
    assert!(wt.join("trees/demo/refs/branches/main").is_file());
    assert_eq!(fx.repo.config().worktree.name, "demo");
    assert_eq!(fx.repo.config().worktree.default_branch, "main");

    let root = fx.repo.root_tree().unwrap();
    assert_eq!(root.name, "demo");
    assert_eq!(root.path, Path::new(""));
    assert_eq!(root.current_branch, "main");
    assert_eq!(root.default_branch, "main");
    assert_eq!(root.remote_name, "demo");
    assert!(uuid::Uuid::parse_str(&root.id).is_ok());

    let branches = fx.repo.branches("demo", false).unwrap();
    assert_eq!(branches.len(), 1);
    assert!(branches[0].is_current);
    assert_eq!(branches[0].tip, None);

    let reflog = fx.repo.reflog("demo", None).unwrap();
    assert_eq!(reflog.len(), 1);
    assert_eq!(reflog[0].action, "init");
}

#[test]
fn init_defaults_name_to_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::init(&tmp.path().join("my-app"), InitOptions::default()).unwrap();
    assert_eq!(repo.root_tree().unwrap().name, "my-app");
    let repo2 = Repository::init(
        &tmp.path().join("other"),
        InitOptions {
            name: None,
            default_branch: Some("trunk".into()),
        },
    )
    .unwrap();
    assert_eq!(repo2.current_branch("other").unwrap(), "trunk");
}

#[test]
fn init_twice_fails_and_bad_options_leave_nothing() {
    let fx = Fixture::new();
    assert!(matches!(
        Repository::init(&fx.root(), InitOptions::default()),
        Err(SdkError::AlreadyInitialized)
    ));

    let tmp = tempfile::tempdir().unwrap();
    let bad = Repository::init(
        tmp.path(),
        InitOptions {
            name: None,
            default_branch: Some("bad name".into()),
        },
    );
    assert!(matches!(bad, Err(SdkError::InvalidArgument(_))));
    assert!(!tmp.path().join(".wt").exists());
    let bad = Repository::init(
        tmp.path(),
        InitOptions {
            name: Some("a/b".into()),
            default_branch: None,
        },
    );
    assert!(bad.is_err());
    assert!(!tmp.path().join(".wt").exists());
}

#[test]
fn open_walks_up_from_subdirectories() {
    let fx = Fixture::new();
    let deep = fx.path("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    let repo = Repository::open(&deep).unwrap();
    assert_eq!(repo.root(), fx.repo.root());
    // A file path works too.
    fx.write("a/file.txt", "x");
    assert_eq!(
        Repository::open(&fx.path("a/file.txt")).unwrap().root(),
        fx.repo.root()
    );
}

#[test]
fn open_outside_a_worktree_fails() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        Repository::open(tmp.path()),
        Err(SdkError::NotAWorktree)
    ));
}

#[test]
fn config_persists_and_drives_author() {
    let mut fx = Fixture::new();
    let mut cfg = fx.repo.config().clone();
    cfg.set("user.name", "Ada Lovelace").unwrap();
    cfg.set("user.email", "ada@example.com").unwrap();
    cfg.set("remote.origin.url", "http://localhost:7420")
        .unwrap();
    cfg.set("remote.origin.tenant", "acme").unwrap();
    fx.repo.set_config(cfg).unwrap();
    assert_eq!(fx.repo.author(), "Ada Lovelace <ada@example.com>");

    let reopened = Repository::open(&fx.root()).unwrap();
    assert_eq!(
        reopened.config().get("remote.origin.tenant").as_deref(),
        Some("acme")
    );
    assert_eq!(
        reopened.config().get("user.name").as_deref(),
        Some("Ada Lovelace")
    );

    fx.write("f.txt", "x");
    let s = fx.snap("first");
    assert_eq!(s.author, "Ada Lovelace <ada@example.com>");
}

#[test]
fn author_falls_back_to_os_user() {
    let fx = Fixture::new();
    let author = fx.repo.author();
    assert!(!author.is_empty());
    assert!(
        !author.contains('<'),
        "no config user, so no email: {author}"
    );
}

#[test]
fn nested_trees_registry() {
    let fx = Fixture::new();
    let lib = fx.repo.add_tree("lib", Path::new("packages/lib")).unwrap();
    assert_eq!(lib.path, Path::new("packages/lib"));
    assert_eq!(lib.current_branch, "main");
    assert!(fx.path("packages/lib/.wt-tree/config.toml").is_file());
    assert_eq!(fx.repo.tree_dir("lib").unwrap(), fx.path("packages/lib"));

    let names: Vec<_> = fx
        .repo
        .trees()
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["demo", "lib"]);

    assert!(matches!(
        fx.repo.add_tree("lib", Path::new("x")),
        Err(SdkError::TreeExists(_))
    ));
    assert!(matches!(
        fx.repo.add_tree("lib2", Path::new("packages/lib")),
        Err(SdkError::TreeExists(_))
    ));
    assert!(matches!(
        fx.repo.add_tree("bad", Path::new("../outside")),
        Err(SdkError::InvalidArgument(_))
    ));
    assert!(matches!(
        fx.repo.add_tree("bad", Path::new("")),
        Err(SdkError::InvalidArgument(_))
    ));
    assert!(matches!(
        fx.repo.tree("nope"),
        Err(SdkError::TreeNotFound(_))
    ));
    assert!(matches!(
        fx.repo.remove_tree("demo"),
        Err(SdkError::InvalidArgument(_))
    ));

    fx.repo.set_tree_remote_name("lib", "shared-lib").unwrap();
    fx.repo.set_tree_id("lib", "server-id-1").unwrap();
    let lib = fx.repo.tree("lib").unwrap();
    assert_eq!(lib.remote_name, "shared-lib");
    assert_eq!(lib.id, "server-id-1");
    let marker = fs::read_to_string(fx.path("packages/lib/.wt-tree/config.toml")).unwrap();
    assert!(marker.contains("server-id-1"));
}

#[test]
fn tree_for_path_picks_deepest_tree() {
    let fx = Fixture::new();
    fx.repo.add_tree("lib", Path::new("packages/lib")).unwrap();
    fx.repo
        .add_tree("inner", Path::new("packages/lib/inner"))
        .unwrap();
    let pick = |rel: &str| fx.repo.tree_for_path(&fx.path(rel)).unwrap().name;
    assert_eq!(pick("README.md"), "demo");
    assert_eq!(pick("packages"), "demo");
    assert_eq!(pick("packages/library/x"), "demo");
    assert_eq!(pick("packages/lib"), "lib");
    assert_eq!(pick("packages/lib/src/a.rs"), "lib");
    assert_eq!(pick("packages/lib/inner/b"), "inner");
    let elsewhere = tempfile::tempdir().unwrap();
    assert_eq!(
        fx.repo.tree_for_path(elsewhere.path()).unwrap().name,
        "demo"
    );
}

#[test]
fn nested_tree_files_are_excluded_from_parent() {
    let fx = Fixture::new();
    fx.write("app.txt", "app");
    fx.write("packages/lib/lib.txt", "lib");
    let first = fx.snap("parent with lib files");
    assert_eq!(
        common::head_paths(&fx.repo, "demo"),
        ["app.txt", "packages/lib/lib.txt"]
    );

    fx.repo.add_tree("lib", Path::new("packages/lib")).unwrap();
    // The parent now sees the nested files as gone.
    let status = fx.repo.status("demo").unwrap();
    assert_eq!(status.changes.len(), 1);
    assert_eq!(status.changes[0].path, "packages/lib/lib.txt");
    fx.snap("drop lib from parent");
    assert_eq!(common::head_paths(&fx.repo, "demo"), ["app.txt"]);

    // The nested tree tracks its own files with tree-relative paths.
    let lib_status = fx.repo.status("lib").unwrap();
    assert_eq!(lib_status.changes.len(), 1);
    assert_eq!(lib_status.changes[0].path, "lib.txt");
    snap_tree(&fx.repo, "lib", "lib first");
    assert_eq!(common::head_paths(&fx.repo, "lib"), ["lib.txt"]);
    assert!(fx.repo.status("lib").unwrap().is_clean());

    // Checking out the old parent snapshot must not touch the nested tree.
    fx.repo.checkout("demo", &first.id, false).unwrap();
    assert_eq!(fx.read("packages/lib/lib.txt"), "lib");

    // After removing the nested tree its files belong to the parent again.
    fx.repo.remove_tree("lib").unwrap();
    assert!(!fx.path("packages/lib/.wt-tree").exists());
    assert!(fx.exists("packages/lib/lib.txt"));
    let status = fx.repo.status("demo").unwrap();
    assert_eq!(
        status.changes,
        vec![worktree_sdk::FileChange {
            path: "packages/lib/lib.txt".into(),
            kind: worktree_sdk::ChangeKind::Added
        }]
    );
    assert!(fx.repo.tree("lib").is_err());
}

#[test]
fn unregistered_marker_directories_are_excluded() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    write_file(
        &fx.path("vendor/.wt-tree/config.toml"),
        "name = 'x'\nid = 'y'\n",
    );
    fx.write("vendor/v.txt", "v");
    let status = fx.repo.status("demo").unwrap();
    let paths: Vec<_> = status.changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["a.txt"]);
}
