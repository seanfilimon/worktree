//! Snapshot hooks: executables under `.wt/hooks/<phase>` (worktree-wide)
//! and `<tree>/.wt-tree/hooks/<phase>` (per tree), run around snapshots.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13.9): the owning workstream
//! implements discovery, the `WT_*` environment, timeouts and abort-on-
//! failure for pre hooks.

use crate::error::Result;
use crate::model::Snapshot;
use crate::repo::Repository;
use crate::repo::TreeRecord;

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the working state is read; a non-zero exit aborts the snapshot.
    PreSnapshot,
    /// After the snapshot is written; failures are reported, not fatal.
    PostSnapshot,
}

impl Phase {
    /// File name of the hook (`pre-snapshot`, `post-snapshot`).
    pub fn file_name(self) -> &'static str {
        match self {
            Phase::PreSnapshot => "pre-snapshot",
            Phase::PostSnapshot => "post-snapshot",
        }
    }
}

/// Run the hooks of `phase` for a tree. `snapshot` is set for post hooks.
pub(crate) fn run(
    _repo: &Repository,
    _tree: &TreeRecord,
    _phase: Phase,
    _branch: &str,
    _snapshot: Option<&Snapshot>,
) -> Result<()> {
    Ok(())
}
