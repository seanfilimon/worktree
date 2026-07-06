use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::load_state;
use crate::workdir::{hash_file_quick, walk_files};
use std::collections::HashMap;

/// Compute the current working tree status
pub fn compute_status(engine: &WorktreeEngine) -> Result<WorkingStatus> {
    let state = load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;

    // Get files from last snapshot
    let last_snapshot = tree.snapshots_on_branch(branch).last().cloned();
    let mut known_files: HashMap<String, String> = HashMap::new();
    if let Some(snap) = &last_snapshot {
        for f in &snap.files {
            known_files.insert(f.path.clone(), f.hash.clone());
        }
    }

    // Walk the working directory
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    let root = engine.root();
    for file_path in walk_files(root) {
        let rel_path = file_path
            .strip_prefix(root)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .replace('\\', "/");

        let hash = hash_file_quick(&file_path);
        if let Some(old_hash) = known_files.remove(&rel_path) {
            if hash != old_hash {
                modified.push(rel_path);
            }
        } else {
            added.push(rel_path);
        }
    }

    // Remaining known files are deleted
    for path in known_files.keys() {
        deleted.push(path.clone());
    }

    Ok(WorkingStatus {
        tree_name: tree.name.clone(),
        branch_name: branch.clone(),
        added,
        modified,
        deleted,
        snapshot_count: tree.snapshots_on_branch(branch).len(),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkingStatus {
    pub tree_name: String,
    pub branch_name: String,
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub snapshot_count: usize,
}

impl WorkingStatus {
    pub fn is_clean(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn total_changes(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}
