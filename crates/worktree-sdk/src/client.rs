use crate::error::Result;
use crate::remote::RemoteClient;
use std::path::{Path, PathBuf};
use worktree_engine::ops;
use worktree_engine::ops::diff::DiffEntry;
use worktree_engine::ops::merge::MergeResult;
use worktree_engine::ops::status::WorkingStatus;
use worktree_engine::ops::sync::{PullResult, PushResult};
use worktree_engine::persist::{BranchState, SnapshotState, TagState, TreeState, WorktreeState};
use worktree_engine::WorktreeEngine;

/// Client for operating on a worktree.
///
/// Prefers a running `worktree-bg` daemon (IPC); falls back to an embedded
/// engine when none is available. Callers never see the difference — every
/// method has the same signature in both modes.
pub struct Client {
    backend: Backend,
}

enum Backend {
    /// Direct in-process engine — degraded mode, or pre-daemon phases.
    Embedded(WorktreeEngine),
    // Remote(RemoteClient) — arrives with the daemon in WT-PHASE-3.
}

impl Client {
    /// Open the worktree containing `path` (walks up to the nearest `.wt/`).
    pub fn open(path: &Path) -> Result<Self> {
        // Prefer the daemon when it is listening for this worktree.
        if RemoteClient::try_connect(path).is_some() {
            unreachable!("IPC backend lands in WT-PHASE-3");
        }
        let engine = WorktreeEngine::open(path)?;
        Ok(Self {
            backend: Backend::Embedded(engine),
        })
    }

    /// Open the worktree containing the current directory.
    pub fn open_current() -> Result<Self> {
        Self::open(Path::new("."))
    }

    /// Initialize a new worktree at `path`.
    pub fn init(path: &Path) -> Result<Self> {
        let engine = WorktreeEngine::init(path)?;
        Ok(Self {
            backend: Backend::Embedded(engine),
        })
    }

    fn engine(&self) -> &WorktreeEngine {
        match &self.backend {
            Backend::Embedded(engine) => engine,
        }
    }

    // --- Worktree layout -------------------------------------------------

    /// Root path of the worktree.
    pub fn root(&self) -> &Path {
        self.engine().root()
    }

    /// Path to the worktree's `.wt/` directory.
    pub fn wt_dir(&self) -> PathBuf {
        self.engine().wt_dir()
    }

    // --- Status & history ------------------------------------------------

    pub fn status(&self) -> Result<WorkingStatus> {
        Ok(ops::status::compute_status(self.engine())?)
    }

    pub fn log(&self, count: usize) -> Result<Vec<SnapshotState>> {
        Ok(ops::log::show_log(self.engine(), count)?)
    }

    pub fn reflog(&self, count: usize) -> Result<Vec<String>> {
        Ok(ops::reflog::show_reflog(self.engine(), count)?)
    }

    /// Full persisted worktree state.
    ///
    /// Escape hatch for commands that inspect state directly (archive,
    /// revert, git interop). Shrinks as those commands get first-class
    /// methods in later phases.
    pub fn state(&self) -> Result<WorktreeState> {
        Ok(worktree_engine::persist::load_state(self.engine())?)
    }

    // --- Snapshots ---------------------------------------------------------

    pub fn snapshot_create(&self, tree_name: Option<&str>, message: &str) -> Result<SnapshotState> {
        Ok(ops::snapshot::create_snapshot(
            self.engine(),
            tree_name,
            message,
        )?)
    }

    // --- Branches ----------------------------------------------------------

    pub fn branch_create(&self, name: &str) -> Result<BranchState> {
        Ok(ops::branch::create_branch(self.engine(), name, None)?)
    }

    /// Returns `(branches, current_branch_name)`.
    pub fn branch_list(&self) -> Result<(Vec<BranchState>, String)> {
        Ok(ops::branch::list_branches(self.engine(), None)?)
    }

    pub fn branch_switch(&self, name: &str) -> Result<()> {
        Ok(ops::branch::switch_branch(self.engine(), name, None)?)
    }

    pub fn branch_delete(&self, name: &str) -> Result<()> {
        Ok(ops::branch::delete_branch(self.engine(), name, None)?)
    }

    // --- Merge & diff ------------------------------------------------------

    pub fn merge(&self, source_branch: &str) -> Result<MergeResult> {
        Ok(ops::merge::merge_branch(self.engine(), source_branch)?)
    }

    pub fn diff_working_tree(&self) -> Result<Vec<DiffEntry>> {
        Ok(ops::diff::diff_working_tree(self.engine())?)
    }

    pub fn diff_snapshots(&self, from: &str, to: &str) -> Result<Vec<DiffEntry>> {
        Ok(ops::diff::diff_snapshots(self.engine(), from, to)?)
    }

    // --- Tags ----------------------------------------------------------------

    pub fn tag_create(&self, name: &str, message: Option<&str>) -> Result<TagState> {
        Ok(ops::tag::create_tag(self.engine(), name, message, None)?)
    }

    pub fn tag_list(&self) -> Result<Vec<TagState>> {
        Ok(ops::tag::list_tags(self.engine(), None)?)
    }

    pub fn tag_delete(&self, name: &str) -> Result<()> {
        Ok(ops::tag::delete_tag(self.engine(), name, None)?)
    }

    // --- Trees -----------------------------------------------------------------

    pub fn tree_add(&self, path: &str) -> Result<TreeState> {
        Ok(ops::tree::add_tree(self.engine(), path)?)
    }

    pub fn tree_list(&self) -> Result<Vec<TreeState>> {
        Ok(ops::tree::list_trees(self.engine())?)
    }

    pub fn tree_remove(&self, name: &str) -> Result<()> {
        Ok(ops::tree::remove_tree(self.engine(), name)?)
    }

    // --- Config & ignore ---------------------------------------------------

    pub fn config_read(&self) -> Result<String> {
        Ok(ops::config::read_config(self.engine())?)
    }

    pub fn ignore_list(&self) -> Result<Vec<String>> {
        Ok(ops::ignore::list_ignored(self.engine())?)
    }

    pub fn dependency_list(&self) -> Result<Vec<String>> {
        Ok(ops::dependency::list_dependencies(self.engine())?)
    }

    // --- Sync ----------------------------------------------------------------

    pub fn sync_push(&self) -> Result<PushResult> {
        Ok(ops::sync::push(self.engine())?)
    }

    pub fn sync_pull(&self) -> Result<PullResult> {
        Ok(ops::sync::pull(self.engine())?)
    }
}
