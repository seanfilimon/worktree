use crate::error::{SdkError, Result};
use super::status::{load_state, save_state, SnapshotState, FileEntry};
use chrono::Utc;
use std::collections::HashMap;

pub struct MergeResult {
    pub snapshot: SnapshotState,
    pub files_merged: usize,
    pub conflicts: Vec<String>,
}

pub fn merge_branch(
    engine: &super::WorktreeEngine,
    source_branch: &str,
) -> Result<MergeResult> {
    let mut state = load_state(engine)?;
    let tree_name = state.current_tree.clone()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree_mut(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    let target_branch = tree.current_branch.clone();
    if source_branch == target_branch {
        return Err(SdkError::MergeConflict("cannot merge a branch into itself".into()));
    }

    // Get latest snapshots from both branches
    let source_files: Vec<FileEntry> = tree.snapshots.iter()
        .filter(|s| s.branch_name == source_branch)
        .last()
        .map(|s| s.files.clone())
        .unwrap_or_default();

    let target_files: Vec<FileEntry> = tree.snapshots.iter()
        .filter(|s| s.branch_name == target_branch)
        .last()
        .map(|s| s.files.clone())
        .unwrap_or_default();

    if source_files.is_empty() {
        return Err(SdkError::BranchNotFound(format!("no snapshots on branch '{}'", source_branch)));
    }

    // Simple three-way merge: combine files from both branches
    let mut merged_files: HashMap<String, FileEntry> = HashMap::new();
    let mut conflicts = Vec::new();

    for f in &target_files {
        merged_files.insert(f.path.clone(), f.clone());
    }

    for f in &source_files {
        if let Some(existing) = merged_files.get(&f.path) {
            if existing.hash != f.hash {
                // Content conflict
                conflicts.push(f.path.clone());
            }
        } else {
            merged_files.insert(f.path.clone(), f.clone());
        }
    }

    if !conflicts.is_empty() {
        return Err(SdkError::MergeConflict(format!(
            "conflicts in {} file(s): {}",
            conflicts.len(),
            conflicts.join(", ")
        )));
    }

    let files: Vec<FileEntry> = merged_files.into_values().collect();
    let files_merged = files.len();

    let source_tip = tree.find_branch(source_branch)
        .and_then(|b| b.tip.clone());
    let target_tip = tree.find_branch(&target_branch)
        .and_then(|b| b.tip.clone());

    let parents: Vec<String> = [target_tip, source_tip].into_iter().flatten().collect();
    let snapshot_id = uuid::Uuid::new_v4().to_string();

    let snapshot = SnapshotState {
        id: snapshot_id.clone(),
        message: format!("Merge branch '{}' into '{}'", source_branch, target_branch),
        author: std::env::var("WT_AUTHOR")
            .or_else(|_| std::env::var("USER"))
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string()),
        timestamp: Utc::now().to_rfc3339(),
        parents,
        tree_name: tree_name.clone(),
        branch_name: target_branch.clone(),
        files,
        auto_generated: false,
    };

    if let Some(branch) = tree.find_branch_mut(&target_branch) {
        branch.tip = Some(snapshot_id);
    }

    tree.snapshots.push(snapshot.clone());
    save_state(engine, &state)?;

    Ok(MergeResult {
        snapshot,
        files_merged,
        conflicts: Vec::new(),
    })
}
