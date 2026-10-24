use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{AccountId, BranchId, SnapshotId, TreeId};

/// A named branch within a worktree, pointing to a tip snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// Unique identifier for this branch.
    pub id: BranchId,
    /// The tree this branch belongs to.
    pub tree_id: TreeId,
    /// Human-readable branch name (e.g. "main", "feature/login").
    pub name: String,
    /// The snapshot at the tip of this branch.
    pub tip: SnapshotId,
    /// When this branch was created.
    pub created_at: DateTime<Utc>,
    /// The account that created this branch.
    pub created_by: AccountId,
}

impl Branch {
    /// Create a new branch pointing at the given tip snapshot.
    pub fn new(
        tree_id: TreeId,
        name: impl Into<String>,
        tip: SnapshotId,
        created_by: AccountId,
    ) -> Self {
        Self {
            id: BranchId::new(),
            tree_id,
            name: name.into(),
            tip,
            created_at: Utc::now(),
            created_by,
        }
    }

    /// Advance the branch tip to a new snapshot, returning the previous tip.
    pub fn advance(&mut self, new_tip: SnapshotId) -> SnapshotId {
        let old_tip = self.tip;
        self.tip = new_tip;
        old_tip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_branch() {
        let tree_id = TreeId::new();
        let tip = SnapshotId::new();
        let author = AccountId::new();
        let branch = Branch::new(tree_id, "main", tip, author);

        assert_eq!(branch.tree_id, tree_id);
        assert_eq!(branch.name, "main");
        assert_eq!(branch.tip, tip);
        assert_eq!(branch.created_by, author);
    }

    #[test]
    fn test_advance() {
        let tree_id = TreeId::new();
        let tip1 = SnapshotId::new();
        let tip2 = SnapshotId::new();
        let author = AccountId::new();
        let mut branch = Branch::new(tree_id, "develop", tip1, author);

        let old = branch.advance(tip2);
        assert_eq!(old, tip1);
        assert_eq!(branch.tip, tip2);
    }

    #[test]
    fn test_advance_multiple_times() {
        let tree_id = TreeId::new();
        let s1 = SnapshotId::new();
        let s2 = SnapshotId::new();
        let s3 = SnapshotId::new();
        let author = AccountId::new();
        let mut branch = Branch::new(tree_id, "feature/x", s1, author);

        branch.advance(s2);
        let old = branch.advance(s3);
        assert_eq!(old, s2);
        assert_eq!(branch.tip, s3);
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let tree_id = TreeId::new();
        let tip = SnapshotId::new();
        let author = AccountId::new();
        let branch = Branch::new(tree_id, "main", tip, author);

        let json = serde_json::to_string(&branch).expect("serialize");
        let deserialized: Branch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(branch, deserialized);
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let tree_id = TreeId::new();
        let tip = SnapshotId::new();
        let author = AccountId::new();
        let branch = Branch::new(tree_id, "release/v1", tip, author);

        let bytes = bincode::serialize(&branch).expect("serialize");
        let deserialized: Branch = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(branch, deserialized);
    }
}
