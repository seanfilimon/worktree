use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{commit_snapshot, load_state, FileEntry, NewSnapshot, SnapshotState};
use std::collections::HashMap;

pub struct MergeResult {
    pub snapshot: SnapshotState,
    pub files_merged: usize,
    pub conflicts: Vec<String>,
}

pub fn merge_branch(engine: &WorktreeEngine, source_branch: &str) -> Result<MergeResult> {
    let state = load_state(engine)?;
    let tree_name = state
        .current_tree
        .clone()
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    let target_branch = tree.current_branch.clone();
    if source_branch == target_branch {
        return Err(EngineError::MergeConflict(
            "cannot merge a branch into itself".into(),
        ));
    }

    // Get latest snapshots from both branches
    let source_files: Vec<FileEntry> = tree
        .snapshots
        .iter()
        .rfind(|s| s.branch_name == source_branch)
        .map(|s| s.files.clone())
        .unwrap_or_default();

    let target_files: Vec<FileEntry> = tree
        .snapshots
        .iter()
        .rfind(|s| s.branch_name == target_branch)
        .map(|s| s.files.clone())
        .unwrap_or_default();

    if source_files.is_empty() {
        return Err(EngineError::BranchNotFound(format!(
            "no snapshots on branch '{}'",
            source_branch
        )));
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
        return Err(EngineError::MergeConflict(format!(
            "conflicts in {} file(s): {}",
            conflicts.len(),
            conflicts.join(", ")
        )));
    }

    let files: Vec<FileEntry> = merged_files.into_values().collect();
    let files_merged = files.len();

    let source_tip = tree.find_branch(source_branch).and_then(|b| b.tip.clone());
    let target_tip = tree.find_branch(&target_branch).and_then(|b| b.tip.clone());
    let parents: Vec<String> = [target_tip, source_tip].into_iter().flatten().collect();

    let message = format!("Merge branch '{}' into '{}'", source_branch, target_branch);
    let snapshot = commit_snapshot(
        engine,
        NewSnapshot {
            tree_name: &tree_name,
            branch_name: &target_branch,
            message: &message,
            parents,
            files,
            auto_generated: false,
            operation: "merge",
        },
    )?;

    Ok(MergeResult {
        snapshot,
        files_merged,
        conflicts: Vec::new(),
    })
}
