use crate::error::{Result, SdkError};
use base64::Engine;
use std::collections::{HashMap, HashSet};

#[derive(serde::Serialize)]
struct CanonicalPullReq {
    tenant: String,
    worktree: String,
    tree_id: String,
    root_path: String,
    branch: String,
    remote_tip: Option<String>,
}

#[derive(serde::Serialize)]
struct CanonicalPushReq {
    snapshot_id: String,
    tenant: String,
    worktree: String,
    tree_id: String,
    branch: String,
    remote_tip: Option<String>,
    objects: Vec<CanonicalObjectUpload>,
}

#[derive(serde::Serialize)]
struct CanonicalObjectUpload {
    path: String,
    hash: String,
    size: u64,
    content: String, // base64-encoded bytes; Go []byte json field auto-decodes base64
}

fn server_url() -> String {
    server_url_with_env(std::env::var("WT_SERVER_URL").ok().as_deref())
}

fn server_url_with_env(override_val: Option<&str>) -> String {
    override_val.unwrap_or("http://127.0.0.1:8080").to_string()
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

pub fn push_unpushed(engine: &super::WorktreeEngine) -> Result<PushResult> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let branch = &tree.current_branch;
    let branch_state = tree.branches.iter().find(|b| &b.name == branch);
    let remote_tip = branch_state.and_then(|b| b.remote_tip.clone());

    let to_push = find_unpushed_snapshots(&tree, branch, remote_tip.as_deref());

    if engine.wt_dir().join("cache").join("sync_paused").exists() {
        return Ok(PushResult {
            branch: branch.clone(),
            snapshots_pushed: 0,
            server: server_url(),
        });
    }

    let mut pushed = 0;
    for snap_id in to_push {
        let res = push_staged(engine, &snap_id)?;
        if res.snapshots_pushed == 0 {
            break;
        }
        pushed += 1;
    }

    Ok(PushResult {
        branch: branch.clone(),
        snapshots_pushed: pushed,
        server: server_url(),
    })
}

pub fn push_staged(engine: &super::WorktreeEngine, snapshot_id: &str) -> Result<PushResult> {
    let mut state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    if engine.wt_dir().join("cache").join("sync_paused").exists() {
        return Ok(PushResult {
            branch: tree.current_branch.clone(),
            snapshots_pushed: 0,
            server: server_url(),
        });
    }

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
    let tenant = std::env::var("WT_TENANT").unwrap_or_else(|_| "default".to_string());

    let branch_state = tree.branches.iter().find(|b| &b.name == branch);
    let remote_tip = branch_state.and_then(|b| b.remote_tip.clone());

    let req = CanonicalPushReq {
        snapshot_id: snapshot.id.clone(),
        tenant,
        worktree: tree.name.clone(),
        tree_id: tree.name.clone(),
        branch: branch.clone(),
        remote_tip,
        objects: canonical_object_uploads(engine, &snapshot, &change_set.present_files)?,
    };

    let server = server_url();
    let client = reqwest::blocking::Client::new();
    let mut request = client
        .post(format!("{server}/staged"))
        .header("x-wt-tree-id", &req.tree_id)
        .json(&req);
    let token = std::env::var("WT_SERVER_AUTH_TOKEN")
        .ok()
        .or_else(|| {
            std::fs::read_to_string(engine.wt_dir().join("cache").join("auth_token"))
                .ok()
                .map(|t| t.trim().to_string())
        })
        .or_else(|| Some("dev-secret".to_string()));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let resp = request
        .send()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(SdkError::NetworkError(format!(
            "server rejected staged upload ({}): {}",
            status, body
        )));
    }

    let branch_name = req.branch.clone();
    let tree_name = req.tree_id.clone();
    if let Some(t) = state.trees.iter_mut().find(|t| t.name == tree_name) {
        if let Some(b) = t.branches.iter_mut().find(|b| b.name == branch_name) {
            b.remote_tip = Some(snapshot_id.to_string());
        }
    }
    super::status::save_state(engine, &state)?;

    Ok(PushResult {
        branch: branch_name,
        snapshots_pushed: 1,
        server,
    })
}

pub fn pull(engine: &super::WorktreeEngine) -> Result<PullResult> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let branch_state = tree.branches.iter().find(|b| b.name == tree.current_branch);
    let remote_tip = branch_state.and_then(|b| b.remote_tip.clone());
    let tenant = std::env::var("WT_TENANT").unwrap_or_else(|_| "default".to_string());

    let req = CanonicalPullReq {
        tenant,
        worktree: tree.name.clone(),
        tree_id: tree.name.clone(),
        root_path: engine.root().to_string_lossy().to_string(),
        branch: tree.current_branch.clone(),
        remote_tip,
    };

    let token = std::env::var("WT_SERVER_AUTH_TOKEN")
        .ok()
        .or_else(|| {
            std::fs::read_to_string(engine.wt_dir().join("cache").join("auth_token"))
                .ok()
                .map(|t| t.trim().to_string())
        })
        .or_else(|| Some("dev-secret".to_string()));

    let mut request = reqwest::blocking::Client::new()
        .post(format!("{}/api/pull", server_url()))
        .header("x-wt-tree-id", &req.tree_id)
        .json(&req);

    if let Some(t) = token {
        request = request.bearer_auth(t);
    }

    let resp = request
        .send()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(SdkError::NetworkError(format!(
            "server rejected pull ({}): {}",
            status, body
        )));
    }

    let resp: serde_json::Value = resp
        .json()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    let server_changes = resp["changed_files"].as_u64().unwrap_or(0) as usize;

    Ok(PullResult {
        branch: tree.current_branch.clone(),
        new_snapshots: server_changes,
        up_to_date: server_changes == 0,
    })
}

fn find_unpushed_snapshots(
    tree: &super::status::TreeState,
    branch: &str,
    remote_tip: Option<&str>,
) -> Vec<String> {
    let snaps = tree.snapshots_on_branch(branch);
    let mut to_push = Vec::new();

    let mut snap_map = std::collections::HashMap::new();
    for snap in &tree.snapshots {
        snap_map.insert(&snap.id, snap);
    }

    let mut known_set = std::collections::HashSet::new();
    if let Some(rtip) = remote_tip {
        let mut queue = vec![rtip];
        while let Some(current_id) = queue.pop() {
            if known_set.contains(current_id) {
                continue;
            }
            known_set.insert(current_id);
            if let Some(snap) = snap_map.get(&current_id.to_string()) {
                for parent_id in &snap.parents {
                    queue.push(parent_id);
                }
            }
        }
    }

    let local_tip = snaps.last();
    if let Some(tip) = local_tip {
        let mut unpushed_set = std::collections::HashSet::new();
        let mut queue = vec![tip.id.as_str()];
        let mut visited = std::collections::HashSet::new();

        while let Some(current_id) = queue.pop() {
            if visited.contains(current_id) {
                continue;
            }
            visited.insert(current_id);

            if known_set.contains(current_id) {
                continue;
            }

            unpushed_set.insert(current_id.to_string());

            if let Some(snap) = snap_map.get(&current_id.to_string()) {
                for parent_id in &snap.parents {
                    queue.push(parent_id);
                }
            }
        }

        let mut in_degree: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut children_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for id in &unpushed_set {
            in_degree.insert(id.clone(), 0);
            children_map.insert(id.clone(), Vec::new());
        }

        for id in &unpushed_set {
            if let Some(snap) = snap_map.get(id) {
                let mut parent_count = 0;
                for parent_id in &snap.parents {
                    if unpushed_set.contains(parent_id) {
                        parent_count += 1;
                        children_map
                            .entry(parent_id.clone())
                            .or_default()
                            .push(id.clone());
                    }
                }
                in_degree.insert(id.clone(), parent_count);
            }
        }

        let mut process_queue: std::collections::VecDeque<String> =
            std::collections::VecDeque::new();
        for snap in &tree.snapshots {
            if let Some(&deg) = in_degree.get(&snap.id) {
                if deg == 0 {
                    process_queue.push_back(snap.id.clone());
                }
            }
        }

        while let Some(id) = process_queue.pop_front() {
            to_push.push(id.clone());
            if let Some(children) = children_map.get(&id) {
                for child_id in children {
                    if let Some(deg) = in_degree.get_mut(child_id) {
                        *deg -= 1;
                        if *deg == 0 {
                            process_queue.push_back(child_id.clone());
                        }
                    }
                }
            }
        }
    }

    to_push
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
    present_files: HashSet<String>,
    // kept for tests and future server-side delta reporting
    #[allow(dead_code)]
    files_changed: Vec<String>,
    #[allow(dead_code)]
    files_added: u32,
    #[allow(dead_code)]
    files_modified: u32,
    #[allow(dead_code)]
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

    let mut present_files = HashSet::new();
    let mut files_changed = Vec::new();
    let mut files_added = 0u32;
    let mut files_modified = 0u32;
    let mut files_deleted = 0u32;

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
        present_files,
        files_changed,
        files_added,
        files_modified,
        files_deleted,
    }
}

pub fn push_staged_dirty(
    engine: &super::WorktreeEngine,
    snapshot_id: &str,
    dirty_paths: &[std::path::PathBuf],
) -> Result<()> {
    let state = super::status::load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    if engine.wt_dir().join("cache").join("sync_paused").exists() {
        return Ok(());
    }

    let branch = &tree.current_branch;
    let tenant = std::env::var("WT_TENANT").unwrap_or_else(|_| "default".to_string());

    let branch_state = tree.branches.iter().find(|b| &b.name == branch);
    let remote_tip = branch_state.and_then(|b| b.remote_tip.clone());

    let mut uploads = Vec::new();
    for path in dirty_paths {
        // Compute relative path
        let relative = if path.is_absolute() {
            path.strip_prefix(engine.root())
                .unwrap_or(path)
                .to_string_lossy()
                .to_string()
        } else {
            path.to_string_lossy().to_string()
        };

        let content = std::fs::read(path).unwrap_or_default();
        let hash = blake3::hash(&content).to_hex().to_string();
        uploads.push(CanonicalObjectUpload {
            path: relative.replace('\\', "/"),
            hash,
            size: content.len() as u64,
            content: base64::engine::general_purpose::STANDARD.encode(&content),
        });
    }

    let req = CanonicalPushReq {
        snapshot_id: snapshot_id.to_string(),
        tenant,
        worktree: tree.name.clone(),
        tree_id: tree.name.clone(),
        branch: branch.clone(),
        remote_tip,
        objects: uploads,
    };

    let server = server_url();
    let client = reqwest::blocking::Client::new();
    let mut request = client
        .post(format!("{server}/staged"))
        .header("x-wt-tree-id", &req.tree_id)
        .json(&req);
    let token = std::env::var("WT_SERVER_AUTH_TOKEN")
        .ok()
        .or_else(|| {
            std::fs::read_to_string(engine.wt_dir().join("cache").join("auth_token"))
                .ok()
                .map(|t| t.trim().to_string())
        })
        .or_else(|| Some("dev-secret".to_string()));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let resp = request
        .send()
        .map_err(|e| SdkError::NetworkError(e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(SdkError::NetworkError(format!(
            "server rejected staged upload ({}): {}",
            status, body
        )));
    }

    Ok(())
}

fn canonical_object_uploads(
    engine: &super::WorktreeEngine,
    snapshot: &super::status::SnapshotState,
    present_files: &HashSet<String>,
) -> Result<Vec<CanonicalObjectUpload>> {
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

        uploads.push(CanonicalObjectUpload {
            path: file.path.clone(),
            hash: file.hash.clone(),
            size: file.size,
            content: base64::engine::general_purpose::STANDARD.encode(&content),
        });
    }

    Ok(uploads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::status::{FileEntry, SnapshotState};

    #[test]
    fn staged_req_serializes_to_spec_field_names() {
        let req = CanonicalPushReq {
            snapshot_id: "snap-1".to_string(),
            tenant: "acme".to_string(),
            worktree: "my-tree".to_string(),
            tree_id: "my-tree".to_string(),
            branch: "main".to_string(),
            remote_tip: Some("snap-0".to_string()),
            objects: vec![CanonicalObjectUpload {
                path: "foo.rs".to_string(),
                hash: "a".repeat(64),
                size: 3,
                content: "aGVsbG8=".to_string(),
            }],
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("snapshot_id").is_some(), "missing snapshot_id");
        assert!(v.get("tenant").is_some(), "missing tenant");
        assert!(v.get("worktree").is_some(), "missing worktree");
        assert!(v.get("tree_id").is_some(), "missing tree_id");
        assert!(v.get("branch").is_some(), "missing branch");
        assert!(v.get("objects").is_some(), "missing objects");
        assert!(v.get("files").is_none(), "must not have old 'files' field");
        assert!(
            v.get("branch_name").is_none(),
            "must not have old 'branch_name' field"
        );
        assert!(
            v.get("snapshot").is_none(),
            "must not have old nested 'snapshot' field"
        );
        let obj = &v["objects"][0];
        assert!(obj.get("content").is_some(), "missing content");
        assert!(
            obj.get("content_base64").is_none(),
            "must not have old 'content_base64' field"
        );
    }

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
    fn server_url_reads_env_var() {
        let url = server_url_with_env(None);
        assert_eq!(url, "http://127.0.0.1:8080");

        let url = server_url_with_env(Some("http://prod.example.com:9000"));
        assert_eq!(url, "http://prod.example.com:9000");
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

    #[test]
    fn find_unpushed_snapshots_linear() {
        use crate::engine::status::{BranchState, TreeState};
        let tree = TreeState {
            name: "test".into(),
            path: "".into(),
            branches: vec![BranchState {
                name: "main".into(),
                tip: Some("snap-3".into()),
                remote_tip: Some("snap-1".into()),
                created_at: "".into(),
            }],
            current_branch: "main".into(),
            tags: vec![],
            snapshots: vec![
                snap("snap-1", vec![], vec![]),
                snap("snap-2", vec!["snap-1".into()], vec![]),
                snap("snap-3", vec!["snap-2".into()], vec![]),
            ],
        };
        let to_push = super::find_unpushed_snapshots(&tree, "main", Some("snap-1"));
        assert_eq!(to_push, vec!["snap-2".to_string(), "snap-3".to_string()]);
    }

    #[test]
    fn find_unpushed_snapshots_merged_branch() {
        use crate::engine::status::{BranchState, TreeState};
        let mut snap_x = snap("snap-X", vec!["snap-A".into()], vec![]);
        snap_x.branch_name = "feature".into();
        let mut snap_y = snap("snap-Y", vec!["snap-X".into()], vec![]);
        snap_y.branch_name = "feature".into();

        let tree = TreeState {
            name: "test".into(),
            path: "".into(),
            branches: vec![BranchState {
                name: "main".into(),
                tip: Some("snap-C".into()),
                remote_tip: Some("snap-B".into()),
                created_at: "".into(),
            }],
            current_branch: "main".into(),
            tags: vec![],
            snapshots: vec![
                snap("snap-A", vec![], vec![]),
                snap("snap-B", vec!["snap-A".into()], vec![]),
                snap_x,
                snap_y,
                snap("snap-C", vec!["snap-B".into(), "snap-Y".into()], vec![]),
            ],
        };

        let to_push = super::find_unpushed_snapshots(&tree, "main", Some("snap-B"));
        assert_eq!(
            to_push,
            vec![
                "snap-X".to_string(),
                "snap-Y".to_string(),
                "snap-C".to_string(),
            ]
        );
    }
}
