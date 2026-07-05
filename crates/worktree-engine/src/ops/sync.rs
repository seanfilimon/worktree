//! Local bookkeeping for sync operations.
//!
//! Real server sync belongs to the daemon (`worktree-bg`); these functions
//! only report local branch state. They gain server awareness in WT-PHASE-5
//! when the daemon's sync client lands.

use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::load_state;

pub fn push(engine: &WorktreeEngine) -> Result<PushResult> {
    let state = load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;
    let snapshot_count = tree.snapshots_on_branch(branch).len();

    Ok(PushResult {
        branch: branch.clone(),
        snapshots_pushed: snapshot_count,
        server: state.name.clone(),
    })
}

pub fn pull(engine: &WorktreeEngine) -> Result<PullResult> {
    let state = load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    Ok(PullResult {
        branch: tree.current_branch.clone(),
        new_snapshots: 0,
        up_to_date: true,
    })
}

#[derive(Debug)]
pub struct PushResult {
    pub branch: String,
    pub snapshots_pushed: usize,
    pub server: String,
}

#[derive(Debug)]
pub struct PullResult {
    pub branch: String,
    pub new_snapshots: usize,
    pub up_to_date: bool,
}
