use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::identity;
use crate::persist::{load_state, save_state, SnapshotState};
use crate::workdir::collect_files;
use chrono::Utc;

/// Create a new snapshot of the current state
pub fn create_snapshot(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
    message: &str,
) -> Result<SnapshotState> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    let branch_name = tree.current_branch.clone();
    let parent = tree.current_branch().and_then(|b| b.tip.clone());

    // Collect current files
    let root = engine.root();
    let files = collect_files(root, &tree_name)?;

    // Check for duplicate snapshot (same files as previous)
    if parent.is_some() {
        let last_snap = tree
            .snapshots
            .iter()
            .rfind(|s| s.branch_name == branch_name);
        if let Some(last) = last_snap {
            if last.files.len() == files.len() {
                let mut old_set: Vec<(&str, &str)> = last
                    .files
                    .iter()
                    .map(|f| (f.path.as_str(), f.hash.as_str()))
                    .collect();
                let mut new_set: Vec<(&str, &str)> = files
                    .iter()
                    .map(|f| (f.path.as_str(), f.hash.as_str()))
                    .collect();
                old_set.sort();
                new_set.sort();
                if old_set == new_set {
                    return Err(EngineError::NoChanges);
                }
            }
        }
    }

    let snapshot_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let author = identity::author();

    let snapshot = SnapshotState {
        id: snapshot_id.clone(),
        message: message.to_string(),
        author,
        timestamp: now,
        parents: parent.into_iter().collect(),
        tree_name: tree_name.clone(),
        branch_name: branch_name.clone(),
        files,
        auto_generated: false,
    };

    // Update branch tip
    if let Some(branch) = tree.find_branch_mut(&branch_name) {
        branch.tip = Some(snapshot_id.clone());
    }

    tree.snapshots.push(snapshot.clone());
    save_state(engine, &state)?;

    Ok(snapshot)
}

/// List snapshots for a tree/branch
pub fn list_snapshots(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
    count: usize,
) -> Result<Vec<SnapshotState>> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    let branch = &tree.current_branch;
    let mut snapshots: Vec<_> = tree
        .snapshots_on_branch(branch)
        .into_iter()
        .cloned()
        .collect();
    snapshots.reverse();
    snapshots.truncate(count);
    Ok(snapshots)
}
