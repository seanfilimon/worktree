use crate::error::Result;
use crate::remote::RemoteClient;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use worktree_engine::ops;
use worktree_engine::ops::diff::DiffEntry;
use worktree_engine::ops::merge::MergeResult;
use worktree_engine::ops::status::WorkingStatus;
use worktree_engine::ops::sync::{PullResult, PushResult};
use worktree_engine::persist::{BranchState, SnapshotState, TagState, TreeState, WorktreeState};
use worktree_engine::WorktreeEngine;
use worktree_ipc::Command;

/// Client for operating on a worktree.
///
/// Prefers a running `worktree-bg` daemon (BgProcess.md: the daemon is the
/// single writer while it runs); falls back to an embedded engine when none
/// is available. Commands without an IPC equivalent yet go through the
/// embedded engine either way — safe, because every engine mutation takes
/// the store's writer lock.
pub struct Client {
    /// Present when a daemon is serving this worktree.
    remote: Option<RemoteClient>,
    /// Embedded engine — the fallback path and the host for commands that
    /// have no IPC form yet.
    engine: WorktreeEngine,
}

/// Daemon runtime information (from `daemon.info`).
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub version: String,
    pub root: String,
    pub uptime_secs: u64,
    pub snapshots_created: u64,
    pub watcher_active: bool,
}

impl Client {
    /// Open the worktree containing `path` (walks up to the nearest `.wt/`).
    pub fn open(path: &Path) -> Result<Self> {
        let engine = WorktreeEngine::open(path)?;
        let remote = RemoteClient::try_connect(engine.root());
        Ok(Self { remote, engine })
    }

    /// Open the worktree containing the current directory.
    pub fn open_current() -> Result<Self> {
        Self::open(Path::new("."))
    }

    /// Initialize a new worktree at `path`.
    pub fn init(path: &Path) -> Result<Self> {
        let engine = WorktreeEngine::init(path)?;
        Ok(Self {
            remote: None,
            engine,
        })
    }

    /// True when requests are served by a running daemon.
    pub fn is_daemon_backed(&self) -> bool {
        self.remote.is_some()
    }

    // --- Worktree layout -------------------------------------------------

    /// Root path of the worktree.
    pub fn root(&self) -> &Path {
        self.engine.root()
    }

    /// Path to the worktree's `.wt/` directory.
    pub fn wt_dir(&self) -> PathBuf {
        self.engine.wt_dir()
    }

    // --- Status & history ------------------------------------------------

    pub fn status(&self) -> Result<WorkingStatus> {
        match &self.remote {
            Some(remote) => remote.call(Command::Status, Value::Null),
            None => Ok(ops::status::compute_status(&self.engine)?),
        }
    }

    pub fn log(&self, count: usize) -> Result<Vec<SnapshotState>> {
        match &self.remote {
            Some(remote) => remote.call(Command::LogQuery, json!({ "count": count })),
            None => Ok(ops::log::show_log(&self.engine, count)?),
        }
    }

    pub fn reflog(&self, count: usize) -> Result<Vec<String>> {
        match &self.remote {
            Some(remote) => remote.call(Command::ReflogQuery, json!({ "count": count })),
            None => Ok(ops::reflog::show_reflog(&self.engine, count)?),
        }
    }

    /// Full persisted worktree state.
    ///
    /// Escape hatch for commands that inspect state directly (archive,
    /// revert, git interop). Read-only, so the embedded engine serves it
    /// even when a daemon runs.
    pub fn state(&self) -> Result<WorktreeState> {
        Ok(worktree_engine::persist::load_state(&self.engine)?)
    }

    /// Daemon runtime info; [`crate::SdkError::DaemonUnavailable`] when no
    /// daemon is running.
    pub fn daemon_info(&self) -> Result<DaemonInfo> {
        match &self.remote {
            Some(remote) => remote.call(Command::DaemonInfo, Value::Null),
            None => Err(crate::SdkError::DaemonUnavailable),
        }
    }

    /// Ask the daemon to shut down. `Ok(false)` when none was running.
    pub fn daemon_stop(&self) -> Result<bool> {
        match &self.remote {
            Some(remote) => {
                let _: Value = remote.call(Command::DaemonShutdown, Value::Null)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // --- Snapshots ---------------------------------------------------------

    pub fn snapshot_create(&self, tree_name: Option<&str>, message: &str) -> Result<SnapshotState> {
        match &self.remote {
            Some(remote) => remote.call(
                Command::SnapshotCreate,
                json!({ "tree": tree_name, "message": message }),
            ),
            None => Ok(ops::snapshot::create_snapshot(
                &self.engine,
                tree_name,
                message,
            )?),
        }
    }

    // --- Branches ----------------------------------------------------------

    pub fn branch_create(&self, name: &str) -> Result<BranchState> {
        match &self.remote {
            Some(remote) => remote.call(Command::BranchCreate, json!({ "name": name })),
            None => Ok(ops::branch::create_branch(&self.engine, name, None)?),
        }
    }

    /// Returns `(branches, current_branch_name)`.
    pub fn branch_list(&self) -> Result<(Vec<BranchState>, String)> {
        match &self.remote {
            Some(remote) => {
                #[derive(Deserialize)]
                struct Listing {
                    branches: Vec<BranchState>,
                    current: String,
                }
                let listing: Listing = remote.call(Command::BranchList, Value::Null)?;
                Ok((listing.branches, listing.current))
            }
            None => Ok(ops::branch::list_branches(&self.engine, None)?),
        }
    }

    pub fn branch_switch(&self, name: &str) -> Result<()> {
        match &self.remote {
            Some(remote) => {
                let _: Value = remote.call(Command::BranchSwitch, json!({ "name": name }))?;
                Ok(())
            }
            None => Ok(ops::branch::switch_branch(&self.engine, name, None)?),
        }
    }

    pub fn branch_delete(&self, name: &str) -> Result<()> {
        match &self.remote {
            Some(remote) => {
                let _: Value = remote.call(Command::BranchDelete, json!({ "name": name }))?;
                Ok(())
            }
            None => Ok(ops::branch::delete_branch(&self.engine, name, None)?),
        }
    }

    // --- Merge & diff ------------------------------------------------------

    pub fn merge(&self, source_branch: &str) -> Result<MergeResult> {
        match &self.remote {
            Some(remote) => remote.call(Command::MergeStart, json!({ "source": source_branch })),
            None => Ok(ops::merge::merge_branch(&self.engine, source_branch)?),
        }
    }

    pub fn diff_working_tree(&self) -> Result<Vec<DiffEntry>> {
        match &self.remote {
            Some(remote) => remote.call(Command::DiffCompute, Value::Null),
            None => Ok(ops::diff::diff_working_tree(&self.engine)?),
        }
    }

    pub fn diff_snapshots(&self, from: &str, to: &str) -> Result<Vec<DiffEntry>> {
        match &self.remote {
            Some(remote) => remote.call(Command::DiffCompute, json!({ "from": from, "to": to })),
            None => Ok(ops::diff::diff_snapshots(&self.engine, from, to)?),
        }
    }

    // --- Tags (no IPC form yet — embedded engine, guarded by store lock) ---

    pub fn tag_create(&self, name: &str, message: Option<&str>) -> Result<TagState> {
        Ok(ops::tag::create_tag(&self.engine, name, message, None)?)
    }

    pub fn tag_list(&self) -> Result<Vec<TagState>> {
        Ok(ops::tag::list_tags(&self.engine, None)?)
    }

    pub fn tag_delete(&self, name: &str) -> Result<()> {
        Ok(ops::tag::delete_tag(&self.engine, name, None)?)
    }

    // --- Trees (no IPC form yet) -------------------------------------------

    pub fn tree_add(&self, path: &str) -> Result<TreeState> {
        Ok(ops::tree::add_tree(&self.engine, path)?)
    }

    pub fn tree_list(&self) -> Result<Vec<TreeState>> {
        Ok(ops::tree::list_trees(&self.engine)?)
    }

    pub fn tree_remove(&self, name: &str) -> Result<()> {
        Ok(ops::tree::remove_tree(&self.engine, name)?)
    }

    // --- Config & ignore ---------------------------------------------------

    pub fn config_read(&self) -> Result<String> {
        Ok(ops::config::read_config(&self.engine)?)
    }

    pub fn ignore_list(&self) -> Result<Vec<String>> {
        Ok(ops::ignore::list_ignored(&self.engine)?)
    }

    pub fn dependency_list(&self) -> Result<Vec<String>> {
        Ok(ops::dependency::list_dependencies(&self.engine)?)
    }

    /// Store health: object counts + integrity verification.
    pub fn doctor(&self) -> Result<worktree_engine::persist::DoctorReport> {
        Ok(ops::doctor::run(&self.engine)?)
    }

    // --- Sync ----------------------------------------------------------------

    pub fn sync_push(&self) -> Result<PushResult> {
        match &self.remote {
            Some(remote) => remote.call(Command::SyncPush, Value::Null),
            None => Ok(ops::sync::push(&self.engine)?),
        }
    }

    pub fn sync_pull(&self) -> Result<PullResult> {
        match &self.remote {
            Some(remote) => remote.call(Command::SyncTrigger, Value::Null),
            None => Ok(ops::sync::pull(&self.engine)?),
        }
    }
}
