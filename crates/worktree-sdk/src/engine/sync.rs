use crate::error::{Result, SdkError};
use base64::Engine;
use std::collections::{HashMap, HashSet};

const SERVER: &str = "http://127.0.0.1:9876";

#[derive(serde::Serialize)]
struct StatusReq {
    tree_id: String,
    root_path: String,
}

#[derive(serde::Serialize)]
struct StagedReq {
    tree_id: String,
    branch_name: String,
    snapshot: super::status::SnapshotState,
    files_changed: Vec<String>,
    files_added: u32,
    files_modified: u32,
    files_deleted: u32,
    files: Vec<StagedFileUpload>,
}

#[derive(serde::Serialize)]
struct StagedFileUpload {
    path: String,
    hash: String,
    size: u64,
    content_base64: String,
}

pub fn push(engine: &super::WorktreeEngine) -> Result<PushResult> {
    push_latest_staged(engine)
}

pub fn push_latest_staged(engine: &super::WorktreeEngine) -> Result<PushResult> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;
    let snaps = tree.snapshots_on_branch(branch);
    let snap = snaps.last().ok_or(SdkError::SnapshotNotFound(
        "no snapshots on current branch".into(),
    ))?;

    push_staged(engine, &snap.id)
}

pub fn push_staged(engine: &super::WorktreeEngine, snapshot_id: &str) -> Result<PushResult> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;
    let snapshot = tree
        .snapshots
        .iter()
        .find(|snap| snap.id == snapshot_id)
        .cloned()
        .ok_or(SdkError::SnapshotNotFound(snapshot_id.to_string()))?;
    let parent = snapshot
        .parents
        .first()
        .and_then(|parent_id| tree.snapshots.iter().find(|snap| &snap.id == parent_id));
    let change_set = snapshot_changes(parent, &snapshot);

    let req = StagedReq {
        tree_id: tree.name.clone(),
        branch_name: branch.clone(),
        snapshot: snapshot.clone(),
        files_changed: change_set.files_changed.clone(),
        files_added: change_set.files_added,
        files_modified: change_set.files_modified,
        files_deleted: change_set.files_deleted,
        files: staged_file_uploads(engine, &snapshot, &change_set.present_files)?,
    };

    reqwest::blocking::Client::new()
        .post(format!("{SERVER}/staged"))
        .json(&req)
        .send()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?
        .error_for_status()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    Ok(PushResult {
        branch: branch.clone(),
        snapshots_pushed: 1,
        server: SERVER.to_string(),
    })
}

pub fn pull(engine: &super::WorktreeEngine) -> Result<PullResult> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let req = StatusReq {
        tree_id: tree.name.clone(),
        root_path: engine.root().to_string_lossy().to_string(),
    };

    let resp: serde_json::Value = reqwest::blocking::Client::new()
        .post(format!("{SERVER}/status"))
        .json(&req)
        .send()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?
        .json()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    let server_changes = resp["changed_files"].as_u64().unwrap_or(0) as usize;

    Ok(PullResult {
        branch: tree.current_branch.clone(),
        new_snapshots: server_changes,
        up_to_date: server_changes == 0,
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

struct SnapshotChanges {
    files_changed: Vec<String>,
    present_files: HashSet<String>,
    files_added: u32,
    files_modified: u32,
    files_deleted: u32,
}

fn snapshot_changes(
    parent: Option<&super::status::SnapshotState>,
    snapshot: &super::status::SnapshotState,
) -> SnapshotChanges {
    let current: HashMap<&str, &super::status::FileEntry> = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let previous: HashMap<&str, &super::status::FileEntry> = parent
        .map(|snap| {
            snap.files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect()
        })
        .unwrap_or_default();

    let mut files_changed = Vec::new();
    let mut present_files = HashSet::new();
    let mut files_added = 0;
    let mut files_modified = 0;
    let mut files_deleted = 0;

    for (path, file) in &current {
        match previous.get(path) {
            Some(old) if old.hash == file.hash => {}
            Some(_) => {
                files_modified += 1;
                files_changed.push((*path).to_string());
                present_files.insert((*path).to_string());
            }
            None => {
                files_added += 1;
                files_changed.push((*path).to_string());
                present_files.insert((*path).to_string());
            }
        }
    }

    for path in previous.keys() {
        if !current.contains_key(path) {
            files_deleted += 1;
            files_changed.push((*path).to_string());
        }
    }

    files_changed.sort();

    SnapshotChanges {
        files_changed,
        present_files,
        files_added,
        files_modified,
        files_deleted,
    }
}

fn staged_file_uploads(
    engine: &super::WorktreeEngine,
    snapshot: &super::status::SnapshotState,
    present_files: &HashSet<String>,
) -> Result<Vec<StagedFileUpload>> {
    let mut uploads = Vec::new();

    for file in &snapshot.files {
        if !present_files.contains(&file.path) {
            continue;
        }

        let path = engine.root().join(&file.path);
        let content = std::fs::read(&path)?;
        let hash = blake3::hash(&content).to_hex().to_string();
        if hash != file.hash {
            return Err(SdkError::Serialization(format!(
                "file hash changed before staged upload: {}",
                file.path
            )));
        }

        uploads.push(StagedFileUpload {
            path: file.path.clone(),
            hash: file.hash.clone(),
            size: file.size,
            content_base64: base64::engine::general_purpose::STANDARD.encode(content),
        });
    }

    Ok(uploads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::status::{FileEntry, SnapshotState};

    fn snap(id: &str, parents: Vec<String>, files: Vec<(&str, &str)>) -> SnapshotState {
        SnapshotState {
            id: id.to_string(),
            message: "test".to_string(),
            author: "tester".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            parents,
            tree_name: "root".to_string(),
            branch_name: "main".to_string(),
            files: files
                .into_iter()
                .map(|(path, hash)| FileEntry {
                    path: path.to_string(),
                    hash: hash.to_string(),
                    size: 1,
                })
                .collect(),
            auto_generated: false,
        }
    }

    #[test]
    fn snapshot_changes_counts_added_modified_and_deleted_files() {
        let parent = snap(
            "a",
            vec![],
            vec![("a.rs", "old"), ("b.rs", "same"), ("gone.rs", "old")],
        );
        let child = snap(
            "b",
            vec!["a".to_string()],
            vec![("a.rs", "new"), ("b.rs", "same"), ("new.rs", "new")],
        );

        let changes = snapshot_changes(Some(&parent), &child);

        assert_eq!(changes.files_added, 1);
        assert_eq!(changes.files_modified, 1);
        assert_eq!(changes.files_deleted, 1);
        assert_eq!(changes.files_changed, vec!["a.rs", "gone.rs", "new.rs"]);
        assert!(changes.present_files.contains("a.rs"));
        assert!(changes.present_files.contains("new.rs"));
        assert!(!changes.present_files.contains("gone.rs"));
    }
}
