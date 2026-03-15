use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{SnapshotId, BranchId, TreeId, AccountId};

/// The type of operation that created this reflog entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReflogAction {
    Snapshot,
    Merge,
    BranchCreate,
    BranchSwitch,
    BranchDelete,
    Revert,
    TagCreate,
    TagDelete,
    Sync,
    Restore,
}

/// A single reflog entry recording an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflogEntry {
    pub action: ReflogAction,
    pub tree_id: TreeId,
    pub branch_id: BranchId,
    pub old_snapshot: Option<SnapshotId>,
    pub new_snapshot: Option<SnapshotId>,
    pub actor: AccountId,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

impl ReflogEntry {
    pub fn new(
        action: ReflogAction,
        tree_id: TreeId,
        branch_id: BranchId,
        old_snapshot: Option<SnapshotId>,
        new_snapshot: Option<SnapshotId>,
        actor: AccountId,
        message: &str,
    ) -> Self {
        Self {
            action,
            tree_id,
            branch_id,
            old_snapshot,
            new_snapshot,
            actor,
            message: message.to_string(),
            timestamp: Utc::now(),
        }
    }

    pub fn snapshot(tree_id: TreeId, branch_id: BranchId, old: SnapshotId, new: SnapshotId, actor: AccountId, message: &str) -> Self {
        Self::new(ReflogAction::Snapshot, tree_id, branch_id, Some(old), Some(new), actor, message)
    }

    pub fn merge(tree_id: TreeId, branch_id: BranchId, old: SnapshotId, new: SnapshotId, actor: AccountId, source_branch: &str) -> Self {
        Self::new(ReflogAction::Merge, tree_id, branch_id, Some(old), Some(new), actor, &format!("merge from {}", source_branch))
    }

    pub fn branch_create(tree_id: TreeId, branch_id: BranchId, snapshot: SnapshotId, actor: AccountId, name: &str) -> Self {
        Self::new(ReflogAction::BranchCreate, tree_id, branch_id, None, Some(snapshot), actor, &format!("branch created: {}", name))
    }

    pub fn branch_switch(tree_id: TreeId, branch_id: BranchId, actor: AccountId, name: &str) -> Self {
        Self::new(ReflogAction::BranchSwitch, tree_id, branch_id, None, None, actor, &format!("switched to {}", name))
    }

    pub fn revert(tree_id: TreeId, branch_id: BranchId, old: SnapshotId, new: SnapshotId, actor: AccountId, reverted_id: &str) -> Self {
        Self::new(ReflogAction::Revert, tree_id, branch_id, Some(old), Some(new), actor, &format!("reverted snapshot {}", reverted_id))
    }
}

/// A branch's reflog — ordered list of entries
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Reflog {
    pub entries: Vec<ReflogEntry>,
}

impl Reflog {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn push(&mut self, entry: ReflogEntry) {
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entry by reflog index (0 = most recent)
    pub fn get(&self, index: usize) -> Option<&ReflogEntry> {
        if self.entries.is_empty() {
            return None;
        }
        let rev_index = self.entries.len().checked_sub(1 + index)?;
        self.entries.get(rev_index)
    }

    /// Get the most recent N entries (newest first)
    pub fn recent(&self, count: usize) -> Vec<&ReflogEntry> {
        self.entries.iter().rev().take(count).collect()
    }

    /// Prune entries older than `retention_days`
    pub fn prune(&mut self, retention_days: u32) {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        self.entries.retain(|e| e.timestamp > cutoff);
    }

    /// Prune to keep at most `max_entries`
    pub fn prune_to_max(&mut self, max_entries: usize) {
        if self.entries.len() > max_entries {
            let drain_count = self.entries.len() - max_entries;
            self.entries.drain(0..drain_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(action: ReflogAction, msg: &str) -> ReflogEntry {
        ReflogEntry::new(
            action,
            TreeId::new(),
            BranchId::new(),
            None,
            Some(SnapshotId::new()),
            AccountId::new(),
            msg,
        )
    }

    #[test]
    fn test_reflog_push_and_get() {
        let mut reflog = Reflog::new();
        reflog.push(make_entry(ReflogAction::Snapshot, "first"));
        reflog.push(make_entry(ReflogAction::Snapshot, "second"));
        reflog.push(make_entry(ReflogAction::Merge, "third"));

        assert_eq!(reflog.len(), 3);
        assert_eq!(reflog.get(0).unwrap().message, "third"); // most recent
        assert_eq!(reflog.get(1).unwrap().message, "second");
        assert_eq!(reflog.get(2).unwrap().message, "first");
        assert!(reflog.get(3).is_none());
    }

    #[test]
    fn test_reflog_recent() {
        let mut reflog = Reflog::new();
        for i in 0..5 {
            reflog.push(make_entry(ReflogAction::Snapshot, &format!("entry-{}", i)));
        }
        let recent = reflog.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "entry-4");
        assert_eq!(recent[1].message, "entry-3");
        assert_eq!(recent[2].message, "entry-2");
    }

    #[test]
    fn test_reflog_prune_max() {
        let mut reflog = Reflog::new();
        for i in 0..10 {
            reflog.push(make_entry(ReflogAction::Snapshot, &format!("e{}", i)));
        }
        reflog.prune_to_max(5);
        assert_eq!(reflog.len(), 5);
        assert_eq!(reflog.entries[0].message, "e5"); // oldest retained
    }
}
