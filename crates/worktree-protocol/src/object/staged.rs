use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{SnapshotId, TreeId, BranchId, AccountId};

/// Status of a staged snapshot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StagedStatus {
    Staged,
    Pushed,
    Cleared,
    Expired,
}

impl Default for StagedStatus {
    fn default() -> Self {
        StagedStatus::Staged
    }
}

/// A staged snapshot visible to team members but not yet in branch history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedSnapshot {
    pub id: SnapshotId,
    pub user: AccountId,
    pub tree_id: TreeId,
    pub branch_id: BranchId,
    pub branch_name: String,
    pub files_changed: Vec<String>,
    pub files_added: u32,
    pub files_modified: u32,
    pub files_deleted: u32,
    pub message: Option<String>,
    pub status: StagedStatus,
    pub timestamp: DateTime<Utc>,
}

impl StagedSnapshot {
    pub fn new(
        user: AccountId,
        tree_id: TreeId,
        branch_id: BranchId,
        branch_name: &str,
        files_changed: Vec<String>,
    ) -> Self {
        Self {
            id: SnapshotId::new(),
            user,
            tree_id,
            branch_id,
            branch_name: branch_name.to_string(),
            files_changed,
            files_added: 0,
            files_modified: 0,
            files_deleted: 0,
            message: None,
            status: StagedStatus::Staged,
            timestamp: Utc::now(),
        }
    }

    pub fn with_message(mut self, msg: &str) -> Self {
        self.message = Some(msg.to_string());
        self
    }

    pub fn with_counts(mut self, added: u32, modified: u32, deleted: u32) -> Self {
        self.files_added = added;
        self.files_modified = modified;
        self.files_deleted = deleted;
        self
    }

    pub fn mark_pushed(&mut self) {
        self.status = StagedStatus::Pushed;
    }

    pub fn clear(&mut self) {
        self.status = StagedStatus::Cleared;
    }

    pub fn expire(&mut self) {
        self.status = StagedStatus::Expired;
    }

    pub fn is_staged(&self) -> bool { self.status == StagedStatus::Staged }
    pub fn is_pushed(&self) -> bool { self.status == StagedStatus::Pushed }

    pub fn total_changes(&self) -> u32 {
        self.files_added + self.files_modified + self.files_deleted
    }
}

/// Advisory conflict warning when two users have staged changes to the same files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedConflictWarning {
    pub file_path: String,
    pub your_user: AccountId,
    pub other_user: AccountId,
    pub other_branch: String,
    pub other_timestamp: DateTime<Utc>,
}

/// Index of staged snapshots on the server
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StagedIndex {
    pub snapshots: Vec<StagedSnapshot>,
}

impl StagedIndex {
    pub fn new() -> Self { Self::default() }

    pub fn add(&mut self, snapshot: StagedSnapshot) {
        self.snapshots.push(snapshot);
    }

    pub fn by_user(&self, user: &AccountId) -> Vec<&StagedSnapshot> {
        self.snapshots.iter().filter(|s| s.user == *user && s.is_staged()).collect()
    }

    pub fn by_tree(&self, tree_id: &TreeId) -> Vec<&StagedSnapshot> {
        self.snapshots.iter().filter(|s| s.tree_id == *tree_id && s.is_staged()).collect()
    }

    pub fn by_branch(&self, branch_id: &BranchId) -> Vec<&StagedSnapshot> {
        self.snapshots.iter().filter(|s| s.branch_id == *branch_id && s.is_staged()).collect()
    }

    /// Check for potential conflicts with staged changes from other users
    pub fn check_conflicts(&self, user: &AccountId, files: &[String]) -> Vec<StagedConflictWarning> {
        let mut warnings = Vec::new();
        for snapshot in &self.snapshots {
            if snapshot.user == *user || !snapshot.is_staged() {
                continue;
            }
            for file in files {
                if snapshot.files_changed.contains(file) {
                    warnings.push(StagedConflictWarning {
                        file_path: file.clone(),
                        your_user: *user,
                        other_user: snapshot.user,
                        other_branch: snapshot.branch_name.clone(),
                        other_timestamp: snapshot.timestamp,
                    });
                }
            }
        }
        warnings
    }

    /// Remove expired and cleared snapshots
    pub fn gc(&mut self, retention_days: u32) {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        self.snapshots.retain(|s| {
            match s.status {
                StagedStatus::Pushed => true,
                StagedStatus::Staged => s.timestamp > cutoff,
                StagedStatus::Cleared | StagedStatus::Expired => false,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staged_snapshot_lifecycle() {
        let mut ss = StagedSnapshot::new(
            AccountId::new(), TreeId::new(), BranchId::new(), "feature/x",
            vec!["src/main.rs".to_string()],
        );
        assert!(ss.is_staged());
        ss.mark_pushed();
        assert!(ss.is_pushed());
    }

    #[test]
    fn test_staged_index_conflict_check() {
        let user1 = AccountId::new();
        let user2 = AccountId::new();
        let tree = TreeId::new();
        let branch = BranchId::new();

        let mut index = StagedIndex::new();
        index.add(StagedSnapshot::new(
            user1, tree, branch, "main",
            vec!["shared.rs".to_string(), "other.rs".to_string()],
        ));

        let warnings = index.check_conflicts(&user2, &["shared.rs".to_string()]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].file_path, "shared.rs");
    }
}
