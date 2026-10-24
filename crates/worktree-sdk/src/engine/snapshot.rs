use std::path::Path;
use chrono::Utc;
use crate::error::{SdkError, Result};
use super::status::{self, SnapshotState, FileEntry, WorktreeState, load_state, save_state};

/// Create a new snapshot of the current state
pub fn create_snapshot(
    engine: &super::WorktreeEngine,
    tree_name: Option<&str>,
    message: &str,
) -> Result<SnapshotState> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree_mut(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    let branch_name = tree.current_branch.clone();
    let parent = tree.current_branch()
        .and_then(|b| b.tip.clone());

    // Collect current files
    let root = engine.root();
    let files = collect_files(root, &tree_name)?;

    // Check for duplicate snapshot (same files as previous)
    if let Some(_parent_id) = &parent {
        let last_snap = tree.snapshots.iter()
            .filter(|s| s.branch_name == branch_name)
            .last();
        if let Some(last) = last_snap {
            if last.files.len() == files.len() {
                let mut old_set: Vec<(&str, &str)> = last.files.iter()
                    .map(|f| (f.path.as_str(), f.hash.as_str()))
                    .collect();
                let mut new_set: Vec<(&str, &str)> = files.iter()
                    .map(|f| (f.path.as_str(), f.hash.as_str()))
                    .collect();
                old_set.sort();
                new_set.sort();
                if old_set == new_set {
                    return Err(SdkError::NoChanges);
                }
            }
        }
    }

    let snapshot_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let author = whoami();

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
    engine: &super::WorktreeEngine,
    tree_name: Option<&str>,
    count: usize,
) -> Result<Vec<SnapshotState>> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    let branch = &tree.current_branch;
    let mut snapshots: Vec<_> = tree.snapshots_on_branch(branch)
        .into_iter()
        .cloned()
        .collect();
    snapshots.reverse();
    snapshots.truncate(count);
    Ok(snapshots)
}

/// Check if a relative path segment matches an ignored directory name exactly
fn is_ignored_dir(rel_str: &str, dir_name: &str) -> bool {
    rel_str == dir_name
        || rel_str.starts_with(&format!("{}/", dir_name))
        || rel_str.starts_with(&format!("{}\\", dir_name))
        || rel_str.contains(&format!("/{}/", dir_name))
        || rel_str.contains(&format!("\\{}\\", dir_name))
        || rel_str.contains(&format!("/{}/", dir_name))
        || rel_str.contains(&format!("\\{}/", dir_name))
        || rel_str.contains(&format!("/{}\\", dir_name))
}

fn collect_files(root: &Path, tree_name: &str) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    let scan_root = if tree_name == "root" {
        root.to_path_buf()
    } else {
        root.join(tree_name)
    };

    if !scan_root.exists() {
        return Ok(files);
    }

    for entry in walkdir::WalkDir::new(&scan_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if is_ignored_dir(&rel_str, ".wt")
            || is_ignored_dir(&rel_str, ".git")
            || is_ignored_dir(&rel_str, "node_modules")
        {
            continue;
        }

        let metadata = std::fs::metadata(path)?;
        let data = std::fs::read(path)?;
        let hash = blake3::hash(&data).to_hex().to_string();

        files.push(FileEntry {
            path: rel_str.to_string(),
            hash,
            size: metadata.len(),
        });
    }

    Ok(files)
}

fn whoami() -> String {
    std::env::var("WT_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
