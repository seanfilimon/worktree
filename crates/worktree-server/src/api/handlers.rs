use crate::error::ServerError;
use crate::storage::backend::StorageBackend;
use crate::storage::disk::DiskStorage;
use crate::storage::staged::StagedStore;
use base64::Engine;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use worktree_protocol::core::hash::{hash_bytes, ContentHash};
use worktree_protocol::core::id::{AccountId, BranchId, SnapshotId, TreeId};
use worktree_protocol::object::staged::StagedSnapshot;

fn sdk_err(e: worktree_sdk::SdkError) -> ServerError {
    ServerError::Engine(e.to_string())
}

/// Request payload for initializing a new Worktree tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InitRequest {
    /// Human-readable name for the new tree.
    pub name: String,
    /// Filesystem path the tree should track.
    pub root_path: String,
}

/// Response returned after successfully initializing a tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InitResponse {
    /// The unique identifier assigned to the newly created tree.
    pub tree_id: String,
}

/// Request payload for querying the status of a tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatusRequest {
    /// The tree to query.
    pub tree_id: String,
    /// Filesystem path of the tree root.
    pub root_path: String,
}

/// Response describing the current status of a tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StatusResponse {
    /// The tree identifier.
    pub tree_id: String,
    /// Current branch name.
    pub branch: String,
    /// Number of changed (dirty) files since the last snapshot.
    pub changed_files: usize,
    /// Whether the watcher is currently active for this tree.
    pub watcher_active: bool,
}

/// Request payload for creating a new snapshot (commit).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotRequest {
    /// The tree to snapshot.
    pub tree_id: String,
    /// Optional human-readable message describing the snapshot.
    pub message: Option<String>,
    /// Filesystem path of the tree root.
    pub root_path: String,
}

/// Response returned after a snapshot is created.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotResponse {
    /// The unique identifier of the newly created snapshot.
    pub snapshot_id: String,
    /// The content hash of the snapshot's root manifest.
    pub manifest_hash: String,
}

/// Request payload for creating or switching branches.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BranchRequest {
    /// The tree the branch belongs to.
    pub tree_id: String,
    /// The name of the branch to create or switch to.
    pub branch_name: String,
    /// If `true`, create a new branch; if `false`, switch to an existing one.
    pub create: bool,
    /// Filesystem path of the tree root.
    pub root_path: String,
}

/// Response returned after a branch operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BranchResponse {
    /// The unique identifier of the branch.
    pub branch_id: String,
    /// The name of the branch.
    pub branch_name: String,
}

/// Request payload for uploading a local snapshot as server-side staged work.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedRequest {
    /// Tree identifier or demo tree name.
    pub tree_id: String,
    /// Branch name the staged snapshot belongs to.
    pub branch_name: String,
    /// Local SDK snapshot metadata.
    pub snapshot: worktree_sdk::engine::status::SnapshotState,
    /// Paths changed relative to the parent snapshot.
    pub files_changed: Vec<String>,
    /// Number of added files.
    pub files_added: u32,
    /// Number of modified files.
    pub files_modified: u32,
    /// Number of deleted files.
    pub files_deleted: u32,
    /// Uploaded bytes for added/modified files.
    pub files: Vec<StagedFileUpload>,
}

/// File object uploaded as part of a staged snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedFileUpload {
    /// Relative file path inside the tree.
    pub path: String,
    /// Expected BLAKE3 hash in lowercase hex.
    pub hash: String,
    /// Expected byte size.
    pub size: u64,
    /// Raw file bytes encoded as base64.
    pub content_base64: String,
}

/// Response returned after a staged snapshot is accepted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StagedResponse {
    /// Snapshot/staged identifier accepted by the server.
    pub staged_snapshot_id: String,
    /// Number of newly stored content-addressed objects.
    pub stored_objects: usize,
    /// Number of uploaded objects already present in server storage.
    pub already_present: usize,
}

pub async fn handle_init(request: InitRequest) -> Result<InitResponse, ServerError> {
    let path = std::path::Path::new(&request.root_path);
    let engine = worktree_sdk::WorktreeEngine::init(path).map_err(sdk_err)?;
    let state = worktree_sdk::engine::status::load_state(&engine).map_err(sdk_err)?;
    let tree_id = state.current_tree.unwrap_or_else(|| "root".to_string());
    Ok(InitResponse { tree_id })
}

pub async fn handle_status(request: StatusRequest) -> Result<StatusResponse, ServerError> {
    let path = std::path::Path::new(&request.root_path);
    let engine = worktree_sdk::WorktreeEngine::open(path).map_err(sdk_err)?;
    let status = worktree_sdk::engine::status::compute_status(&engine).map_err(sdk_err)?;
    let changed_files = status.total_changes();
    Ok(StatusResponse {
        tree_id: request.tree_id,
        branch: status.branch_name,
        changed_files,
        watcher_active: false,
    })
}

pub async fn handle_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, ServerError> {
    let path = std::path::Path::new(&request.root_path);
    let engine = worktree_sdk::WorktreeEngine::open(path).map_err(sdk_err)?;
    let message = request.message.as_deref().unwrap_or("manual snapshot");
    let snap =
        worktree_sdk::engine::snapshot::create_snapshot(&engine, None, message).map_err(sdk_err)?;
    let combined: String = snap.files.iter().map(|f| f.hash.as_str()).collect();
    let manifest_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();
    Ok(SnapshotResponse {
        snapshot_id: snap.id,
        manifest_hash,
    })
}

pub async fn handle_branch(request: BranchRequest) -> Result<BranchResponse, ServerError> {
    let path = std::path::Path::new(&request.root_path);
    let engine = worktree_sdk::WorktreeEngine::open(path).map_err(sdk_err)?;
    if request.create {
        worktree_sdk::engine::branch::create_branch(&engine, &request.branch_name, None)
            .map_err(sdk_err)?;
    } else {
        worktree_sdk::engine::branch::switch_branch(&engine, &request.branch_name, None)
            .map_err(sdk_err)?;
    }
    Ok(BranchResponse {
        branch_id: uuid::Uuid::new_v4().to_string(),
        branch_name: request.branch_name,
    })
}

pub async fn handle_staged(request: StagedRequest) -> Result<StagedResponse, ServerError> {
    handle_staged_with_store(request, default_server_storage_root()).await
}

pub async fn handle_staged_with_store(
    request: StagedRequest,
    storage_root: PathBuf,
) -> Result<StagedResponse, ServerError> {
    let object_store = DiskStorage::new(storage_root.clone());
    let staged_store = StagedStore::new(storage_root);
    let mut stored_objects = 0;
    let mut already_present = 0;

    for upload in &request.files {
        validate_relative_path(&upload.path)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&upload.content_base64)
            .map_err(|e| ServerError::Api(format!("invalid base64 for {}: {}", upload.path, e)))?;
        if bytes.len() as u64 != upload.size {
            return Err(ServerError::Api(format!(
                "size mismatch for {}: expected {}, got {}",
                upload.path,
                upload.size,
                bytes.len()
            )));
        }

        let expected_hash = ContentHash::from_str(&upload.hash)
            .map_err(|e| ServerError::Api(format!("invalid hash for {}: {}", upload.path, e)))?;
        let actual_hash = hash_bytes(&bytes);
        if actual_hash != expected_hash {
            return Err(ServerError::Api(format!(
                "hash mismatch for {}: expected {}, got {}",
                upload.path, expected_hash, actual_hash
            )));
        }

        if object_store.exists(&expected_hash) {
            already_present += 1;
        } else {
            object_store.store(&expected_hash, &bytes)?;
            stored_objects += 1;
        }
    }

    let snapshot_bytes = serde_json::to_vec(&request.snapshot)
        .map_err(|e| ServerError::Api(format!("serialize staged snapshot metadata: {}", e)))?;
    let snapshot_hash = hash_bytes(&snapshot_bytes);
    if object_store.exists(&snapshot_hash) {
        already_present += 1;
    } else {
        object_store.store(&snapshot_hash, &snapshot_bytes)?;
        stored_objects += 1;
    }

    let staged = staged_snapshot_from_request(&request)?;
    let staged_id = staged.id.to_string();
    staged_store.add(staged)?;

    Ok(StagedResponse {
        staged_snapshot_id: staged_id,
        stored_objects,
        already_present,
    })
}

fn default_server_storage_root() -> PathBuf {
    std::env::var("W0RKTREE_SERVER_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".w0rktree-server")
        })
}

fn staged_snapshot_from_request(request: &StagedRequest) -> Result<StagedSnapshot, ServerError> {
    let user = std::env::var("WT_ACCOUNT_ID")
        .ok()
        .and_then(|id| AccountId::from_str(&id).ok())
        .unwrap_or_else(AccountId::nil);
    let tree_id = TreeId::from_str(&request.tree_id).unwrap_or_else(|_| TreeId::nil());
    let branch_id = BranchId::nil();
    let snapshot_id = SnapshotId::from_str(&request.snapshot.id).map_err(|e| {
        ServerError::Api(format!(
            "invalid snapshot id {}: {}",
            request.snapshot.id, e
        ))
    })?;

    let mut staged = StagedSnapshot::new(
        user,
        tree_id,
        branch_id,
        &request.branch_name,
        request.files_changed.clone(),
    )
    .with_counts(
        request.files_added,
        request.files_modified,
        request.files_deleted,
    );

    staged.id = snapshot_id;
    if !request.snapshot.message.is_empty() {
        staged = staged.with_message(&request.snapshot.message);
    }
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&request.snapshot.timestamp) {
        staged.timestamp = timestamp.with_timezone(&chrono::Utc);
    }

    Ok(staged)
}

fn validate_relative_path(path: &str) -> Result<(), ServerError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() || path.split(['/', '\\']).any(|part| part == "..") {
        return Err(ServerError::Api(format!(
            "invalid staged file path: {}",
            path
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[tokio::test]
    async fn handle_init_creates_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let req = InitRequest {
            name: "test".to_string(),
            root_path: dir.path().to_str().unwrap().to_string(),
        };
        let resp = handle_init(req).await.unwrap();
        assert!(!resp.tree_id.is_empty());
        assert!(dir.path().join(".wt").exists());
    }

    #[tokio::test]
    async fn handle_status_returns_branch_name() {
        let dir = tempfile::tempdir().unwrap();
        worktree_sdk::WorktreeEngine::init(dir.path()).unwrap();
        let req = StatusRequest {
            tree_id: "root".to_string(),
            root_path: dir.path().to_str().unwrap().to_string(),
        };
        let resp = handle_status(req).await.unwrap();
        assert_eq!(resp.branch, "main");
    }

    #[tokio::test]
    async fn handle_branch_create_and_switch() {
        let dir = tempfile::tempdir().unwrap();
        worktree_sdk::WorktreeEngine::init(dir.path()).unwrap();
        let create_req = BranchRequest {
            tree_id: "root".to_string(),
            branch_name: "feature".to_string(),
            create: true,
            root_path: dir.path().to_str().unwrap().to_string(),
        };
        let resp = handle_branch(create_req).await.unwrap();
        assert_eq!(resp.branch_name, "feature");

        let switch_req = BranchRequest {
            tree_id: "root".to_string(),
            branch_name: "feature".to_string(),
            create: false,
            root_path: dir.path().to_str().unwrap().to_string(),
        };
        handle_branch(switch_req).await.unwrap();
    }

    #[tokio::test]
    async fn handle_staged_stores_uploaded_objects_and_index_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = b"hello staged";
        let hash = blake3::hash(content).to_hex().to_string();
        let snapshot_id = uuid::Uuid::new_v4().to_string();
        let req = StagedRequest {
            tree_id: "root".to_string(),
            branch_name: "main".to_string(),
            snapshot: worktree_sdk::engine::status::SnapshotState {
                id: snapshot_id.clone(),
                message: "auto snapshot".to_string(),
                author: "tester".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                parents: vec![],
                tree_name: "root".to_string(),
                branch_name: "main".to_string(),
                files: vec![worktree_sdk::engine::status::FileEntry {
                    path: "src/lib.rs".to_string(),
                    hash: hash.clone(),
                    size: content.len() as u64,
                }],
                auto_generated: true,
            },
            files_changed: vec!["src/lib.rs".to_string()],
            files_added: 1,
            files_modified: 0,
            files_deleted: 0,
            files: vec![StagedFileUpload {
                path: "src/lib.rs".to_string(),
                hash,
                size: content.len() as u64,
                content_base64: base64::engine::general_purpose::STANDARD.encode(content),
            }],
        };

        let resp = handle_staged_with_store(req, dir.path().to_path_buf())
            .await
            .unwrap();

        assert_eq!(resp.staged_snapshot_id, snapshot_id);
        assert_eq!(resp.stored_objects, 2);

        let staged = StagedStore::new(dir.path().to_path_buf())
            .load_index()
            .unwrap();
        assert_eq!(staged.snapshots.len(), 1);
        assert_eq!(staged.snapshots[0].files_changed, vec!["src/lib.rs"]);
    }

    #[tokio::test]
    async fn handle_staged_rejects_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let req = StagedRequest {
            tree_id: "root".to_string(),
            branch_name: "main".to_string(),
            snapshot: worktree_sdk::engine::status::SnapshotState {
                id: uuid::Uuid::new_v4().to_string(),
                message: "bad".to_string(),
                author: "tester".to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                parents: vec![],
                tree_name: "root".to_string(),
                branch_name: "main".to_string(),
                files: vec![],
                auto_generated: true,
            },
            files_changed: vec!["src/lib.rs".to_string()],
            files_added: 1,
            files_modified: 0,
            files_deleted: 0,
            files: vec![StagedFileUpload {
                path: "src/lib.rs".to_string(),
                hash: blake3::hash(b"expected").to_hex().to_string(),
                size: 6,
                content_base64: base64::engine::general_purpose::STANDARD.encode(b"actual"),
            }],
        };

        let err = handle_staged_with_store(req, dir.path().to_path_buf())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("hash mismatch"));
    }
}
