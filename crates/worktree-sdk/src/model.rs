//! Data model shared by every SDK consumer.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::ObjectHash;

/// Snapshot ids are UUID v4 strings (matches `SnapshotId.value` on the wire).
pub type SnapshotId = String;

/// One file in a manifest. `path` is '/'-separated and relative to the tree root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub hash: ObjectHash,
    pub size: u64,
    pub executable: bool,
}

/// The complete file listing of a snapshot. Entries are sorted by path and
/// unique. Stored in the object store as a protobuf-encoded
/// `worktree.sync.v1.ManifestRecord`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Build from entries in any order (sorts + dedups by path, last wins).
    pub fn from_entries(entries: Vec<ManifestEntry>) -> Self {
        let _ = entries;
        todo!("sdk-core: Manifest::from_entries")
    }

    pub fn get(&self, path: &str) -> Option<&ManifestEntry> {
        let _ = path;
        todo!("sdk-core: Manifest::get")
    }

    /// Canonical protobuf encoding (ManifestRecord).
    pub fn to_bytes(&self) -> Vec<u8> {
        todo!("sdk-core: Manifest::to_bytes")
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let _ = data;
        todo!("sdk-core: Manifest::from_bytes")
    }

    /// BLAKE3 of `to_bytes()`.
    pub fn hash(&self) -> ObjectHash {
        todo!("sdk-core: Manifest::hash")
    }
}

/// An immutable point-in-time record of a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    /// Stable tree id (UUID string); also the server's tree id once linked.
    pub tree_id: String,
    pub branch: String,
    pub parents: Vec<SnapshotId>,
    pub manifest: ObjectHash,
    pub author: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub auto_generated: bool,
}

impl Snapshot {
    /// First 8 characters of the id.
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }

    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Convert to the wire record.
    pub fn to_record(&self) -> worktree_protocol::proto::sync::SnapshotRecord {
        todo!("sdk-core: Snapshot::to_record")
    }

    /// Convert from the wire record.
    pub fn from_record(rec: &worktree_protocol::proto::sync::SnapshotRecord) -> Result<Self> {
        let _ = rec;
        todo!("sdk-core: Snapshot::from_record")
    }
}

/// A tree (independently versioned sub-project) inside the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeInfo {
    pub name: String,
    pub id: String,
    /// Path relative to the worktree root ("" for the root tree).
    pub path: PathBuf,
    pub current_branch: String,
    pub default_branch: String,
    /// Name of the tree on the server (defaults to `name`).
    pub remote_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub tip: Option<SnapshotId>,
    pub is_current: bool,
    pub created_at: DateTime<Utc>,
    /// Soft-deleted ("archived") branches are hidden from listings by default.
    pub archived: bool,
    /// Tip of `refs/remotes/origin/<name>` if known.
    pub upstream_tip: Option<SnapshotId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub target: SnapshotId,
    pub message: Option<String>,
    pub tagger: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub tree: String,
    pub branch: String,
    pub head: Option<SnapshotId>,
    /// Working directory vs. head, sorted by path.
    pub changes: Vec<FileChange>,
    pub merge: Option<MergeState>,
}

impl StatusReport {
    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOptions {
    pub message: String,
    /// Overrides the configured author.
    pub author: Option<String>,
    pub auto_generated: bool,
    /// Create a snapshot even if nothing changed.
    pub allow_empty: bool,
    /// Skip the pre-snapshot secret scan.
    pub skip_secret_scan: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchOptions {
    /// Create the branch at the current head first.
    pub create: bool,
    /// Discard uncommitted changes.
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOptions {
    pub message: Option<String>,
    pub no_fast_forward: bool,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides changed overlapping lines; markers written to the file.
    Content,
    /// One side modified, the other deleted.
    ModifyDelete,
    /// Both sides added the path with different content.
    AddAdd,
    /// Binary file changed on both sides; "ours" kept in the working tree.
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub path: String,
    pub kind: ConflictKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeOutcome {
    UpToDate,
    FastForward { to: SnapshotId },
    Merged { snapshot: Snapshot },
    /// Working tree contains the partial merge; `.wt/conflicts/` describes it.
    Conflicts { conflicts: Vec<ConflictInfo> },
}

/// Persisted while a conflicted merge awaits resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeState {
    pub source_branch: String,
    pub source_tip: SnapshotId,
    pub target_tip: Option<SnapshotId>,
    pub base: Option<SnapshotId>,
    pub conflicts: Vec<ConflictInfo>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffOptions {
    /// Lines of context in patches.
    pub context: usize,
    /// Skip patch generation.
    pub name_only: bool,
    /// Restrict to these paths (prefix match); empty = all.
    pub paths: Vec<String>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { context: 3, name_only: false, paths: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub kind: ChangeKind,
    pub old_hash: Option<ObjectHash>,
    pub new_hash: Option<ObjectHash>,
    pub binary: bool,
    /// Unified diff text (None when name_only or binary).
    pub patch: Option<String>,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogEntry {
    pub at: DateTime<Utc>,
    pub tree: String,
    pub branch: String,
    /// "init", "snapshot", "auto-snapshot", "switch", "branch.create",
    /// "branch.archive", "merge", "fast-forward", "revert", "restore",
    /// "pull", "push", "tag.create", "tag.delete".
    pub action: String,
    pub old: Option<SnapshotId>,
    pub new: Option<SnapshotId>,
    pub message: String,
}

/// Options for `Repository::init`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Name of the worktree and its root tree (default: directory name).
    pub name: Option<String>,
    /// Default branch (default: "main").
    pub default_branch: Option<String>,
}
