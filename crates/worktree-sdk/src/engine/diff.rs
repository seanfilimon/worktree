use crate::error::{SdkError, Result};
use super::status::{load_state, FileEntry};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub path: String,
    pub status: DiffStatus,
    pub old_hash: Option<String>,
    pub new_hash: Option<String>,
    pub old_size: Option<u64>,
    pub new_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed(String),
}

pub fn diff_working_tree(engine: &super::WorktreeEngine) -> Result<Vec<DiffEntry>> {
    let status = super::status::compute_status(engine)?;
    let mut entries = Vec::new();

    for path in &status.added {
        entries.push(DiffEntry {
            path: path.clone(),
            status: DiffStatus::Added,
            old_hash: None,
            new_hash: None,
            old_size: None,
            new_size: None,
        });
    }

    for path in &status.modified {
        entries.push(DiffEntry {
            path: path.clone(),
            status: DiffStatus::Modified,
            old_hash: None,
            new_hash: None,
            old_size: None,
            new_size: None,
        });
    }

    for path in &status.deleted {
        entries.push(DiffEntry {
            path: path.clone(),
            status: DiffStatus::Deleted,
            old_hash: None,
            new_hash: None,
            old_size: None,
            new_size: None,
        });
    }

    Ok(entries)
}

pub fn diff_snapshots(
    engine: &super::WorktreeEngine,
    from_id: &str,
    to_id: &str,
) -> Result<Vec<DiffEntry>> {
    let state = load_state(engine)?;
    let tree_name = state.current_tree.as_deref()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree(tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.to_string()))?;

    let from_snap = tree.snapshots.iter().find(|s| s.id == from_id)
        .ok_or(SdkError::SnapshotNotFound(from_id.to_string()))?;
    let to_snap = tree.snapshots.iter().find(|s| s.id == to_id)
        .ok_or(SdkError::SnapshotNotFound(to_id.to_string()))?;

    let from_map: HashMap<&str, &FileEntry> = from_snap.files.iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let to_map: HashMap<&str, &FileEntry> = to_snap.files.iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    let mut entries = Vec::new();

    for (path, to_file) in &to_map {
        if let Some(from_file) = from_map.get(path) {
            if from_file.hash != to_file.hash {
                entries.push(DiffEntry {
                    path: path.to_string(),
                    status: DiffStatus::Modified,
                    old_hash: Some(from_file.hash.clone()),
                    new_hash: Some(to_file.hash.clone()),
                    old_size: Some(from_file.size),
                    new_size: Some(to_file.size),
                });
            }
        } else {
            entries.push(DiffEntry {
                path: path.to_string(),
                status: DiffStatus::Added,
                old_hash: None,
                new_hash: Some(to_file.hash.clone()),
                old_size: None,
                new_size: Some(to_file.size),
            });
        }
    }

    for path in from_map.keys() {
        if !to_map.contains_key(path) {
            let from_file = from_map[path];
            entries.push(DiffEntry {
                path: path.to_string(),
                status: DiffStatus::Deleted,
                old_hash: Some(from_file.hash.clone()),
                new_hash: None,
                old_size: Some(from_file.size),
                new_size: None,
            });
        }
    }

    Ok(entries)
}
