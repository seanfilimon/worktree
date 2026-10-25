//! `Repository` — the local engine over a `.wt/` directory.
//!
//! On-disk layout (see docs/ENGINEERING_PLAN.md §3):
//!
//! ```text
//! .wt/
//!   config.toml                 WorktreeConfig
//!   ignore                      extra ignore patterns (one per line)
//!   objects/<hh>/<62 hex>       blobs, manifests, chunk lists (ObjectStore)
//!   snapshots/<id>.json         Snapshot records
//!   trees/<tree>/tree.toml      TreeInfo (name, id, path, default branch, remote name)
//!   trees/<tree>/HEAD           current branch name
//!   trees/<tree>/refs/branches/<branch>        snapshot id (or empty)
//!   trees/<tree>/refs/branches.meta.json       created_at / archived flags
//!   trees/<tree>/refs/remotes/<remote>/<branch> snapshot id
//!   trees/<tree>/refs/tags/<tag>.json          TagInfo
//!   trees/<tree>/index.json     stat cache: path -> (size, mtime_ns, hash)
//!   trees/<tree>/reflog.jsonl   ReflogEntry per line (append-only)
//!   trees/<tree>/shallow        boundary snapshot ids of a shallow history (one per line)
//!   trees/<tree>/stubbed        paths whose content the server withheld (one per line)
//!   trees/<tree>/sparse         sparse checkout patterns (§15.4; absent = everything)
//!   trees/<tree>/partial        remote a sparse fetch skipped content from (§15.4)
//!   trees/<tree>/MERGE_STATE.json
//!   trees/<tree>/REWRITE_STATE.json  stopped cherry-pick / rebase (§14.5)
//!   trees/<tree>/stash.jsonl         StashEntry per line, oldest first (§14.5)
//!   trees/<tree>/LOCK           advisory lock held by mutating operations
//!   conflicts/<tree>.json       machine-readable conflict list during a merge
//! ```
//!
//! Nested trees are directories containing a `.wt-tree/` marker; a tree's
//! snapshot excludes nested trees' paths. `.wt/`, `.wt-tree/` and ignored
//! paths are never snapshotted.
//!
//! The implementation is split by concern:
//!
//! - `trees` — tree registry and nested-tree markers
//! - `ignore_rules` / `working` — working-directory scanning, ignore rules,
//!   the stat-cache index and file materialization
//! - `refs` — branches, remote-tracking refs and tags
//! - `history` — snapshot records, revision resolution, log and ancestry
//! - `snapshotting` — status and snapshot creation
//! - `merging` — merge, merge state, revert and restore
//! - `diffing` — working/revision diffs
//! - `reflog` — the append-only per-tree reflog
//!
//! Mutating operations take a per-tree OS file lock (`trees/<tree>/LOCK`)
//! so the CLI and the background daemon never interleave writes to the same
//! tree; the lock is released automatically if a process dies.

mod blame; // v0.6 blame workstream (§14.4)
mod diffing;
pub(crate) mod fsutil;
mod history;
// local-config (§13.9): public for `IgnoreRule` and the default-pattern lists.
pub mod ignore_rules;
mod lock;
mod merging;
pub(crate) mod names;
mod partial; // v0.5 chunks workstream: shallow boundary + stubbed paths
mod reflog;
mod refs;
mod snapshotting;
// storage (§14.6): local gc and fsck (`GcOptions`, `GcReport`, `FsckReport`).
pub mod storage;
mod trees;
mod working;
// rewrite (§14.5): cherry-pick, rebase and stash.
mod rewrite;
mod stash;
pub use rewrite::{
    CherryPickOptions, RebaseOptions, RewriteOp, RewriteOutcome, RewriteState, RewriteStatus,
    RewrittenSnapshot,
};
pub use stash::{AutoStashSwitch, StashApplyOutcome, StashEntry, StashOptions};
// sparse (§15.4): sparse checkout patterns and partial clones.
mod sparse;
pub use sparse::{normalize_sparse_patterns, SparseMatcher, SparseReport, SparseStatus};

pub(crate) use trees::TreeRecord;

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::WorktreeConfig;
use crate::error::{Result, SdkError};
use crate::model::*;
use crate::store::ObjectStore;

/// Name of the metadata directory at the worktree root.
pub const WT_DIR: &str = ".wt";
/// Name of the marker directory that turns a directory into a nested tree.
pub const TREE_MARKER_DIR: &str = ".wt-tree";
/// Name of per-directory ignore files.
pub const IGNORE_FILE_NAME: &str = ".wtignore";

/// Default branch name for new worktrees.
const DEFAULT_BRANCH: &str = "main";

/// Header written to a fresh `.wt/ignore`.
const IGNORE_FILE_HEADER: &str = "\
# W0rkTree ignore patterns (gitignore syntax), relative to the worktree root.
# These root rules are authoritative: nested .wtignore and .wt-tree/ignore
# files cannot re-include what they ignore.
# Always ignored: .wt/ .wt-tree/ .git/
# Soft defaults (re-include here with !pattern, e.g. !dist/):
#   node_modules/ target/ __pycache__/ .DS_Store Thumbs.db *.pyc *.pyo
#   *.swp *.swo *~ .env .venv/ dist/ build/ *.log
";

/// A W0rkTree worktree on disk: its root directory, configuration and
/// object store. All local version-control operations are methods on this
/// type.
#[derive(Debug)]
pub struct Repository {
    root: PathBuf,
    config: WorktreeConfig,
    store: ObjectStore,
}

impl Repository {
    // ----------------------------------------------------------------
    // Lifecycle
    // ----------------------------------------------------------------

    /// Create `.wt/` at `path` (creating the directory if needed) with a root
    /// tree named after the directory, an empty default branch, and an
    /// "init" reflog entry. `AlreadyInitialized` if `.wt/` exists.
    pub fn init(path: &Path, opts: InitOptions) -> Result<Self> {
        fs::create_dir_all(path)?;
        let root = std::path::absolute(path)?;
        let wt = root.join(WT_DIR);
        if wt.exists() {
            return Err(SdkError::AlreadyInitialized);
        }

        let name = match opts.name {
            Some(name) => {
                names::validate_tree_name(&name)?;
                name
            }
            None => derive_tree_name(&root),
        };
        let default_branch = opts
            .default_branch
            .unwrap_or_else(|| DEFAULT_BRANCH.to_string());
        names::validate_ref_name("branch", &default_branch)?;

        fs::create_dir(&wt)?;
        let result = (|| {
            let config = WorktreeConfig::new(name.clone(), default_branch.clone());
            config.save(&wt.join("config.toml"))?;
            fs::create_dir_all(wt.join("objects"))?;
            fs::create_dir_all(wt.join("snapshots"))?;
            fs::create_dir_all(wt.join("trees"))?;
            fsutil::write_atomic(&wt.join("ignore"), IGNORE_FILE_HEADER.as_bytes())?;
            let repo = Repository {
                store: ObjectStore::open(wt.join("objects")),
                root: root.clone(),
                config,
            };
            repo.create_tree_record(&name, "", &default_branch)?;
            Ok(repo)
        })();
        if result.is_err() {
            // Leave no half-initialized worktree behind.
            let _ = fs::remove_dir_all(&wt);
        }
        result
    }

    /// Open the worktree containing `path` (walks up to find `.wt/`).
    pub fn open(path: &Path) -> Result<Self> {
        let start = std::path::absolute(path)?;
        let mut candidate = Some(start.as_path());
        while let Some(dir) = candidate {
            let wt = dir.join(WT_DIR);
            if wt.is_dir() {
                let config = WorktreeConfig::load(&wt.join("config.toml"))?;
                let mut repo = Repository {
                    root: dir.to_path_buf(),
                    config,
                    store: ObjectStore::open(wt.join("objects")),
                };
                // sparse (§15.4): partial clones download missing content on demand.
                repo.install_lazy_fetcher();
                return Ok(repo);
            }
            candidate = dir.parent();
        }
        Err(SdkError::NotAWorktree)
    }

    /// The worktree root (the directory containing `.wt/`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The `.wt/` metadata directory.
    pub fn wt_dir(&self) -> PathBuf {
        self.root.join(WT_DIR)
    }

    /// The loaded `.wt/config.toml`.
    pub fn config(&self) -> &WorktreeConfig {
        &self.config
    }

    /// Replace and persist the config.
    pub fn set_config(&mut self, config: WorktreeConfig) -> Result<()> {
        config.save(&self.wt_dir().join("config.toml"))?;
        self.config = config;
        Ok(())
    }

    /// The object store under `.wt/objects`.
    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    /// Author string for new snapshots: config user ("Name <email>"), else
    /// the OS user (`USERNAME` / `USER`), else `"unknown"`.
    pub fn author(&self) -> String {
        // local-config (§13.9): fall back to `[user]` in `<wt home>/config.toml`.
        let user = crate::config::effective_user(&self.config.user);
        let name = user.name.trim();
        let email = user.email.trim();
        match (name.is_empty(), email.is_empty()) {
            (false, false) => return format!("{name} <{email}>"),
            (false, true) => return name.to_string(),
            (true, false) => return email.to_string(),
            (true, true) => {}
        }
        ["USERNAME", "USER"]
            .iter()
            .filter_map(|var| std::env::var(var).ok())
            .map(|v| v.trim().to_string())
            .find(|v| !v.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }

    // ----------------------------------------------------------------
    // Layout helpers
    // ----------------------------------------------------------------

    fn snapshots_dir(&self) -> PathBuf {
        self.wt_dir().join("snapshots")
    }

    /// `.wt/trees/<tree>` (does not check existence).
    fn tree_meta_dir(&self, tree: &str) -> PathBuf {
        self.wt_dir().join("trees").join(tree)
    }

    fn merge_state_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("MERGE_STATE.json")
    }

    fn conflicts_path(&self, tree: &str) -> PathBuf {
        self.wt_dir().join("conflicts").join(format!("{tree}.json"))
    }
}

/// Derive a valid tree name from a directory name, replacing characters
/// that are not allowed; falls back to `"root"`.
fn derive_tree_name(root: &Path) -> String {
    let raw = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string();
    if names::validate_tree_name(&cleaned).is_ok() {
        cleaned
    } else {
        "root".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_tree_name_sanitizes() {
        assert_eq!(derive_tree_name(Path::new("/x/my-project")), "my-project");
        assert_eq!(derive_tree_name(Path::new("/x/.hidden")), "hidden");
        assert_eq!(derive_tree_name(Path::new("/x/a:b")), "a-b");
        assert_eq!(derive_tree_name(Path::new("/")), "root");
    }
}
