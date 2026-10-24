//! High-level Git <-> W0rkTree bridge used by `wt git import|export`.
//!
//! Contract (docs/ENGINEERING_PLAN.md): import converts every Git branch's
//! commit DAG into snapshots on the root tree (preserving parents, author,
//! timestamp, message), converts lightweight/annotated tags, and converts
//! `.gitignore` into `.wt/ignore`. Export writes snapshots as Git commits;
//! the snapshot-id <-> commit-sha mapping is kept in `.wt/git-map.json` so
//! repeated exports are incremental and deterministic.

use std::path::{Path, PathBuf};

use worktree_sdk::Repository;

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Only import these branches (default: all local branches).
    pub branches: Vec<String>,
    /// Name for the new worktree (default: destination dir name).
    pub name: Option<String>,
    /// Import tags.
    pub include_tags: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub dest: PathBuf,
    pub branches: Vec<String>,
    pub snapshots: usize,
    pub blobs: usize,
    pub tags: usize,
    /// Branch checked out in the working directory.
    pub checked_out: Option<String>,
}

/// Create a new worktree at `dest` from the Git repository at `git_path`.
pub fn import_repository(git_path: &Path, dest: &Path, opts: &ImportOptions) -> Result<ImportReport> {
    let _ = (git_path, dest, opts);
    todo!("git-bridge: import_repository")
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Tree to export (default: root tree).
    pub tree: Option<String>,
    /// Only export these branches (default: all).
    pub branches: Vec<String>,
    pub include_tags: bool,
    /// Create the target as a bare repository if it does not exist.
    pub bare: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub git_path: PathBuf,
    pub branches: Vec<String>,
    /// Newly written commits (already-mapped snapshots are skipped).
    pub commits: usize,
    pub tags: usize,
}

/// Export `repo` into the Git repository at `git_path` (created if missing).
pub fn export_repository(repo: &Repository, git_path: &Path, opts: &ExportOptions) -> Result<ExportReport> {
    let _ = (repo, git_path, opts);
    todo!("git-bridge: export_repository")
}
