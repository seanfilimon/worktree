use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{commit_snapshot, load_state, FileEntry, NewSnapshot, SnapshotState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        // Machine-readable conflict metadata (BgProcess.md §12.2) so tools
        // and UIs can present resolution options.
        write_conflict_metadata(
            engine,
            &conflicts,
            source_branch,
            &target_branch,
            &source_files,
            &target_files,
        )?;
        return Err(EngineError::MergeConflict(format!(
            "conflicts in {} file(s): {} (details in .wt/conflicts/)",
            conflicts.len(),
            conflicts.join(", ")
        )));
    }

    // A clean merge clears any stale conflict metadata.
    clear_conflict_metadata(engine)?;

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

/// Write one `.conflict.json` per conflicted path under `.wt/conflicts/`.
fn write_conflict_metadata(
    engine: &WorktreeEngine,
    conflicts: &[String],
    source_branch: &str,
    target_branch: &str,
    source_files: &[FileEntry],
    target_files: &[FileEntry],
) -> Result<()> {
    let dir = engine.wt_dir().join("conflicts");
    std::fs::create_dir_all(&dir)?;

    for path in conflicts {
        let current = target_files.iter().find(|f| &f.path == path);
        let incoming = source_files.iter().find(|f| &f.path == path);
        let metadata = serde_json::json!({
            "file": path,
            "current_branch": target_branch,
            "incoming_branch": source_branch,
            "current_hash": current.map(|f| f.hash.clone()),
            "incoming_hash": incoming.map(|f| f.hash.clone()),
            "kind": "content",
            "detected_at": chrono::Utc::now().to_rfc3339(),
        });
        let slug = path.replace(['/', '\\'], "-");
        std::fs::write(
            dir.join(format!("{slug}.conflict.json")),
            serde_json::to_string_pretty(&metadata)
                .map_err(|e| EngineError::Serialization(e.to_string()))?,
        )?;
    }
    Ok(())
}

/// Remove all conflict metadata (called after a clean merge).
fn clear_conflict_metadata(engine: &WorktreeEngine) -> Result<()> {
    let dir = engine.wt_dir().join("conflicts");
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".conflict.json"))
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}
