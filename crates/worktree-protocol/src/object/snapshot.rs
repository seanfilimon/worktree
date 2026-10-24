use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::hash::ContentHash;
use crate::core::id::{AccountId, SnapshotId, TreeId};

/// A snapshot represents an immutable point-in-time capture of a worktree's state.
///
/// Snapshots form a DAG (directed acyclic graph) via their `parents` field,
/// enabling full history traversal and merge tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique identifier for this snapshot.
    pub id: SnapshotId,
    /// The tree this snapshot belongs to.
    pub tree_id: TreeId,
    /// BLAKE3 hash of the manifest at the time of this snapshot.
    pub manifest_hash: ContentHash,
    /// Parent snapshot(s). Empty for the root snapshot; more than one for merges.
    pub parents: Vec<SnapshotId>,
    /// Human-readable description of the changes captured.
    pub message: String,
    /// The account that authored this snapshot.
    pub author: AccountId,
    /// When this snapshot was created.
    pub timestamp: DateTime<Utc>,
    /// Whether the snapshot was created by an automatic process (e.g. auto-save)
    /// rather than an explicit user action.
    pub auto_generated: bool,
}

impl Snapshot {
    /// Create a new manually-authored snapshot.
    pub fn new(
        tree_id: TreeId,
        manifest_hash: ContentHash,
        parents: Vec<SnapshotId>,
        message: impl Into<String>,
        author: AccountId,
    ) -> Self {
        Self {
            id: SnapshotId::new(),
            tree_id,
            manifest_hash,
            parents,
            message: message.into(),
            author,
            timestamp: Utc::now(),
            auto_generated: false,
        }
    }

    /// Create a new auto-generated snapshot (e.g. periodic auto-save).
    pub fn new_auto(
        tree_id: TreeId,
        manifest_hash: ContentHash,
        parents: Vec<SnapshotId>,
        author: AccountId,
    ) -> Self {
        Self {
            id: SnapshotId::new(),
            tree_id,
            manifest_hash,
            parents,
            message: String::from("auto-generated snapshot"),
            author,
            timestamp: Utc::now(),
            auto_generated: true,
        }
    }

    /// Returns `true` if this is a root snapshot (no parents).
    pub fn is_root(&self) -> bool {
        self.parents.is_empty()
    }

    /// Returns `true` if this is a merge snapshot (two or more parents).
    pub fn is_merge(&self) -> bool {
        self.parents.len() >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;

    #[test]
    fn test_new_snapshot_is_not_auto() {
        let tree_id = TreeId::new();
        let author = AccountId::new();
        let manifest_hash = hash_bytes(b"manifest contents");

        let snap = Snapshot::new(tree_id, manifest_hash, vec![], "initial commit", author);

        assert!(!snap.auto_generated);
        assert_eq!(snap.message, "initial commit");
        assert_eq!(snap.tree_id, tree_id);
        assert_eq!(snap.author, author);
        assert_eq!(snap.manifest_hash, manifest_hash);
    }

    #[test]
    fn test_root_snapshot() {
        let snap = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"root"),
            vec![],
            "root",
            AccountId::new(),
        );
        assert!(snap.is_root());
        assert!(!snap.is_merge());
    }

    #[test]
    fn test_single_parent_is_neither_root_nor_merge() {
        let parent = SnapshotId::new();
        let snap = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"child"),
            vec![parent],
            "child",
            AccountId::new(),
        );
        assert!(!snap.is_root());
        assert!(!snap.is_merge());
    }

    #[test]
    fn test_merge_snapshot() {
        let p1 = SnapshotId::new();
        let p2 = SnapshotId::new();
        let snap = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"merge"),
            vec![p1, p2],
            "merge commit",
            AccountId::new(),
        );
        assert!(!snap.is_root());
        assert!(snap.is_merge());
    }

    #[test]
    fn test_auto_generated_snapshot() {
        let snap = Snapshot::new_auto(
            TreeId::new(),
            hash_bytes(b"auto"),
            vec![],
            AccountId::new(),
        );
        assert!(snap.auto_generated);
        assert_eq!(snap.message, "auto-generated snapshot");
    }

    #[test]
    fn test_snapshot_ids_are_unique() {
        let a = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"a"),
            vec![],
            "a",
            AccountId::new(),
        );
        let b = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"b"),
            vec![],
            "b",
            AccountId::new(),
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let snap = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"serde test"),
            vec![SnapshotId::new()],
            "serde",
            AccountId::new(),
        );
        let json = serde_json::to_string(&snap).expect("serialize");
        let deserialized: Snapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snap.id, deserialized.id);
        assert_eq!(snap.tree_id, deserialized.tree_id);
        assert_eq!(snap.manifest_hash, deserialized.manifest_hash);
        assert_eq!(snap.parents, deserialized.parents);
        assert_eq!(snap.message, deserialized.message);
        assert_eq!(snap.author, deserialized.author);
        assert_eq!(snap.auto_generated, deserialized.auto_generated);
    }
}
