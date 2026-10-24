use crate::error::ServerError;

/// Request payload for initializing a new Worktree tree.
#[derive(Debug, Clone)]
pub struct InitRequest {
    /// Human-readable name for the new tree.
    pub name: String,
    /// Filesystem path the tree should track.
    pub root_path: String,
}

/// Response returned after successfully initializing a tree.
#[derive(Debug, Clone)]
pub struct InitResponse {
    /// The unique identifier assigned to the newly created tree.
    pub tree_id: String,
}

/// Request payload for querying the status of a tree.
#[derive(Debug, Clone)]
pub struct StatusRequest {
    /// The tree to query.
    pub tree_id: String,
}

/// Response describing the current status of a tree.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    /// The tree to snapshot.
    pub tree_id: String,
    /// Optional human-readable message describing the snapshot.
    pub message: Option<String>,
}

/// Response returned after a snapshot is created.
#[derive(Debug, Clone)]
pub struct SnapshotResponse {
    /// The unique identifier of the newly created snapshot.
    pub snapshot_id: String,
    /// The content hash of the snapshot's root manifest.
    pub manifest_hash: String,
}

/// Request payload for creating or switching branches.
#[derive(Debug, Clone)]
pub struct BranchRequest {
    /// The tree the branch belongs to.
    pub tree_id: String,
    /// The name of the branch to create or switch to.
    pub branch_name: String,
    /// If `true`, create a new branch; if `false`, switch to an existing one.
    pub create: bool,
}

/// Response returned after a branch operation.
#[derive(Debug, Clone)]
pub struct BranchResponse {
    /// The unique identifier of the branch.
    pub branch_id: String,
    /// The name of the branch.
    pub branch_name: String,
}

/// Handle an `init` request — create a new Worktree tree and begin tracking
/// the specified directory.
///
/// This will:
/// 1. Validate the root path exists and is a directory.
/// 2. Create a new `Tree` with a unique ID.
/// 3. Register the tree with the watcher and storage subsystems.
/// 4. Return the tree's identifier.
pub async fn handle_init(request: InitRequest) -> Result<InitResponse, ServerError> {
    let _ = request;
    todo!("validate root_path, create Tree, register with watcher and storage, return tree_id")
}

/// Handle a `status` request — return the current state of a tracked tree.
///
/// This queries the watcher and engine to determine how many files have
/// changed since the last snapshot and whether the watcher is active.
pub async fn handle_status(request: StatusRequest) -> Result<StatusResponse, ServerError> {
    let _ = request;
    todo!("look up tree by id, query watcher and engine for dirty file count, return status")
}

/// Handle a `snapshot` request — create a new snapshot (commit) for a tree.
///
/// This will:
/// 1. Gather all changed files from the watcher/engine.
/// 2. Hash and store new/modified blobs.
/// 3. Build a manifest for the tree's current state.
/// 4. Create a `Snapshot` object pointing to the manifest.
/// 5. Advance the current branch tip.
pub async fn handle_snapshot(request: SnapshotRequest) -> Result<SnapshotResponse, ServerError> {
    let _ = request;
    todo!("collect changes, hash blobs, build manifest, create snapshot, advance branch tip")
}

/// Handle a `branch` request — create a new branch or switch the active branch.
///
/// When `create` is `true`, a new branch is forked from the current branch tip.
/// When `create` is `false`, the active branch pointer is updated to the named
/// branch (which must already exist).
pub async fn handle_branch(request: BranchRequest) -> Result<BranchResponse, ServerError> {
    let _ = request;
    todo!("create or switch branch, update active branch pointer, return branch info")
}
