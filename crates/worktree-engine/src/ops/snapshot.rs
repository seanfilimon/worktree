use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{commit_snapshot, load_state, NewSnapshot, SnapshotState};
use crate::workdir::collect_files;

/// Create a new snapshot of the current state
pub fn create_snapshot(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
    message: &str,
) -> Result<SnapshotState> {
    create_snapshot_inner(engine, tree_name, message, false)
}

/// Create a snapshot marked as auto-generated (used by the daemon).
pub fn create_auto_snapshot(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
    message: &str,
) -> Result<SnapshotState> {
    create_snapshot_inner(engine, tree_name, message, true)
}

fn create_snapshot_inner(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
    message: &str,
    auto_generated: bool,
) -> Result<SnapshotState> {
    use crate::hooks::{self, Hook};

    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    let branch_name = tree.current_branch.clone();
    let parent = tree.current_branch().and_then(|b| b.tip.clone());

    // Collect current files
    let files = collect_files(engine.root(), &tree_name)?;

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

    // Gating pre-snapshot hook (DotWt.md).
    let hook_env = [
        ("WT_TREE", tree_name.as_str()),
        ("WT_BRANCH", branch_name.as_str()),
        ("WT_MESSAGE", message),
    ];
    hooks::run(engine, Hook::PreSnapshot, &hook_env)?;

    let snapshot = commit_snapshot(
        engine,
        NewSnapshot {
            tree_name: &tree_name,
            branch_name: &branch_name,
            message,
            parents: parent.into_iter().collect(),
            files,
            auto_generated,
            operation: "snapshot",
        },
    )?;

    // Informational post-snapshot hook — failures logged, never fatal.
    let post_env = [
        ("WT_TREE", tree_name.as_str()),
        ("WT_BRANCH", branch_name.as_str()),
        ("WT_MESSAGE", message),
        ("WT_SNAPSHOT_ID", snapshot.id.as_str()),
    ];
    let _ = hooks::run(engine, Hook::PostSnapshot, &post_env);

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
