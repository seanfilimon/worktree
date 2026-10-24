//! Shared fixtures for the integration tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use worktree_sdk::{InitOptions, Repository, Snapshot, SnapshotOptions};

/// A fresh worktree in a temp dir. The repository root is `<tmp>/work`.
pub struct Fixture {
    pub tmp: TempDir,
    pub repo: Repository,
    /// Name of the root tree.
    pub tree: String,
}

impl Fixture {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("work");
        let repo = Repository::init(
            &root,
            InitOptions {
                name: Some("demo".into()),
                default_branch: None,
            },
        )
        .expect("init");
        let tree = repo.root_tree().expect("root tree").name;
        Fixture { tmp, repo, tree }
    }

    pub fn root(&self) -> PathBuf {
        self.repo.root().to_path_buf()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.repo.root().join(rel)
    }

    pub fn write(&self, rel: &str, content: &str) {
        write_file(&self.path(rel), content);
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.path(rel)).expect("remove");
    }

    /// Snapshot the root tree without the secret scan.
    pub fn snap(&self, message: &str) -> Snapshot {
        snap_tree(&self.repo, &self.tree, message)
    }
}

pub fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, content).expect("write");
}

pub fn opts(message: &str) -> SnapshotOptions {
    SnapshotOptions {
        message: message.into(),
        skip_secret_scan: true,
        ..SnapshotOptions::default()
    }
}

pub fn snap_tree(repo: &Repository, tree: &str, message: &str) -> Snapshot {
    repo.snapshot(tree, opts(message))
        .unwrap_or_else(|e| panic!("snapshot {message:?}: {e}"))
}

/// Paths in the tree's head manifest.
pub fn head_paths(repo: &Repository, tree: &str) -> Vec<String> {
    let snap = repo.resolve(tree, "HEAD").expect("HEAD");
    repo.manifest_of(&snap)
        .expect("manifest")
        .entries
        .into_iter()
        .map(|e| e.path)
        .collect()
}
