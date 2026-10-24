use crate::error::Result;

pub fn push(engine: &super::WorktreeEngine) -> Result<PushResult> {
    // For now, local-only operation — log intent
    let state = super::status::load_state(engine)?;
    let tree = state.current_tree()
        .ok_or(crate::error::SdkError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;
    let snapshot_count = tree.snapshots_on_branch(branch).len();

    Ok(PushResult {
        branch: branch.clone(),
        snapshots_pushed: snapshot_count,
        server: state.name.clone(),
    })
}

pub fn pull(engine: &super::WorktreeEngine) -> Result<PullResult> {
    let state = super::status::load_state(engine)?;
    let tree = state.current_tree()
        .ok_or(crate::error::SdkError::TreeNotFound("no current tree".into()))?;

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
