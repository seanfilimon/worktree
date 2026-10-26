//! The offline outbox: pushes recorded while the server was unreachable
//! (ENGINEERING_PLAN §15.5).
//!
//! Entries live in `.wt/trees/<tree>/outbox.jsonl`, one [`OutboxEntry`] per
//! line, oldest first. There is at most one entry per `(remote, branch)`:
//! queueing the same branch again moves the recorded tip forward and keeps
//! the original position and queue time. Delivering an entry pushes the
//! branch's *current* tip (`remote::flush_outbox`), so work snapshotted after
//! the push was queued goes out with it.
//!
//! Read-modify-write operations hold the tree lock briefly; the network push
//! itself runs without it.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::SnapshotId;

/// One queued push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxEntry {
    /// Stable id (UUID).
    pub id: String,
    pub remote: String,
    pub branch: String,
    /// Branch tip when the push was (last) queued.
    pub tip: SnapshotId,
    pub queued_at: DateTime<Utc>,
    /// Delivery attempts so far.
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Why the last attempt failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Repository {
    fn outbox_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("outbox.jsonl")
    }

    /// Queued pushes of `tree`, oldest first (`wt sync queue`).
    pub fn outbox(&self, tree: &str) -> Result<Vec<OutboxEntry>> {
        self.load_tree(tree)?;
        self.read_outbox(tree)
    }

    /// Queue a push of `branch` to `remote` (`wt push --queue`, or `wt sync`
    /// while the server is unreachable). The branch must have snapshots. An
    /// existing entry for the same remote and branch is updated in place.
    pub fn outbox_queue(&self, tree: &str, remote: &str, branch: &str) -> Result<OutboxEntry> {
        let _lock = self.lock_tree(tree)?;
        self.load_tree(tree)?;
        let tip = self.read_tip(tree, branch)?.ok_or_else(|| {
            SdkError::InvalidArgument(format!("branch '{branch}' has no snapshots to push"))
        })?;
        let mut entries = self.read_outbox(tree)?;
        let entry = match entries
            .iter_mut()
            .find(|e| e.remote == remote && e.branch == branch)
        {
            Some(existing) => {
                existing.tip = tip;
                existing.clone()
            }
            None => {
                let entry = OutboxEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    remote: remote.to_string(),
                    branch: branch.to_string(),
                    tip,
                    queued_at: Utc::now(),
                    attempts: 0,
                    last_attempt_at: None,
                    last_error: None,
                };
                entries.push(entry.clone());
                entry
            }
        };
        self.write_outbox(tree, &entries)?;
        Ok(entry)
    }

    /// Remove entry number `n` (1-based, as listed by `wt sync queue`)
    /// without delivering it (`wt sync drop N`).
    pub fn outbox_drop(&self, tree: &str, n: usize) -> Result<OutboxEntry> {
        let _lock = self.lock_tree(tree)?;
        let mut entries = self.read_outbox(tree)?;
        if n == 0 || n > entries.len() {
            return Err(SdkError::InvalidArgument(match entries.len() {
                0 => "the outbox is empty".to_string(),
                len => format!("no queued push {n} (the outbox has 1..{len})"),
            }));
        }
        let removed = entries.remove(n - 1);
        self.write_outbox(tree, &entries)?;
        Ok(removed)
    }

    /// Remove the entry with `id` (delivered); whether it was there.
    pub fn outbox_remove(&self, tree: &str, id: &str) -> Result<bool> {
        let _lock = self.lock_tree(tree)?;
        let mut entries = self.read_outbox(tree)?;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Ok(false);
        }
        self.write_outbox(tree, &entries)?;
        Ok(true)
    }

    /// Record a failed delivery attempt of entry `id` (no-op if it is gone).
    pub fn outbox_record_attempt(&self, tree: &str, id: &str, error: Option<&str>) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let mut entries = self.read_outbox(tree)?;
        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
            return Ok(());
        };
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_attempt_at = Some(Utc::now());
        entry.last_error = error.map(str::to_string);
        self.write_outbox(tree, &entries)
    }

    /// Snapshots queued pushes reference (for local garbage collection).
    pub fn outbox_roots(&self) -> Result<Vec<SnapshotId>> {
        let mut out = Vec::new();
        for rec in self.tree_records()? {
            out.extend(self.read_outbox(&rec.name)?.into_iter().map(|e| e.tip));
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn read_outbox(&self, tree: &str) -> Result<Vec<OutboxEntry>> {
        let Some(text) = fsutil::read_string_opt(&self.outbox_path(tree))? else {
            return Ok(Vec::new());
        };
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l).map_err(|e| {
                    SdkError::Serialization(format!(
                        "{}: bad outbox entry: {e}",
                        self.outbox_path(tree).display()
                    ))
                })
            })
            .collect()
    }

    fn write_outbox(&self, tree: &str, entries: &[OutboxEntry]) -> Result<()> {
        let path = self.outbox_path(tree);
        if entries.is_empty() {
            return fsutil::remove_file_if_exists(&path);
        }
        let mut text = String::new();
        for e in entries {
            text.push_str(&serde_json::to_string(e)?);
            text.push('\n');
        }
        fsutil::write_atomic(&path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InitOptions, SnapshotOptions};

    fn repo_with_snapshot() -> (tempfile::TempDir, Repository, String) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path(), InitOptions::default()).unwrap();
        let tree = repo.root_tree().unwrap().name;
        std::fs::write(dir.path().join("a.txt"), "1").unwrap();
        repo.snapshot(
            &tree,
            SnapshotOptions {
                message: "one".into(),
                skip_secret_scan: true,
                ..Default::default()
            },
        )
        .unwrap();
        (dir, repo, tree)
    }

    #[test]
    fn queue_dedups_per_branch_and_drop_is_one_based() {
        let (dir, repo, tree) = repo_with_snapshot();
        assert!(repo.outbox(&tree).unwrap().is_empty());
        let first = repo.outbox_queue(&tree, "origin", "main").unwrap();
        assert_eq!(first.attempts, 0);
        let tip1 = first.tip.clone();

        std::fs::write(dir.path().join("a.txt"), "2").unwrap();
        repo.snapshot(
            &tree,
            SnapshotOptions {
                message: "two".into(),
                skip_secret_scan: true,
                ..Default::default()
            },
        )
        .unwrap();
        let again = repo.outbox_queue(&tree, "origin", "main").unwrap();
        assert_eq!(again.id, first.id, "same entry");
        assert_ne!(again.tip, tip1, "tip moved forward");
        assert_eq!(again.queued_at, first.queued_at);
        repo.outbox_queue(&tree, "backup", "main").unwrap();
        let entries = repo.outbox(&tree).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].remote, "origin");
        assert_eq!(repo.outbox_roots().unwrap(), vec![again.tip.clone()]);

        repo.outbox_record_attempt(&tree, &first.id, Some("offline"))
            .unwrap();
        let e = &repo.outbox(&tree).unwrap()[0];
        assert_eq!(e.attempts, 1);
        assert_eq!(e.last_error.as_deref(), Some("offline"));
        assert!(e.last_attempt_at.is_some());

        assert!(repo.outbox_drop(&tree, 0).is_err());
        assert!(repo.outbox_drop(&tree, 3).is_err());
        let dropped = repo.outbox_drop(&tree, 2).unwrap();
        assert_eq!(dropped.remote, "backup");
        assert!(repo.outbox_remove(&tree, &first.id).unwrap());
        assert!(!repo.outbox_remove(&tree, &first.id).unwrap());
        assert!(repo.outbox(&tree).unwrap().is_empty());
        assert!(
            !repo.outbox_path(&tree).exists(),
            "an empty outbox removes its file"
        );
        assert!(matches!(
            repo.outbox_drop(&tree, 1),
            Err(SdkError::InvalidArgument(m)) if m.contains("empty")
        ));
    }

    #[test]
    fn empty_branch_cannot_be_queued() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path(), InitOptions::default()).unwrap();
        let tree = repo.root_tree().unwrap().name;
        assert!(matches!(
            repo.outbox_queue(&tree, "origin", "main"),
            Err(SdkError::InvalidArgument(_))
        ));
        assert!(matches!(
            repo.outbox_queue(&tree, "origin", "nope"),
            Err(SdkError::BranchNotFound(_))
        ));
    }

    #[test]
    fn corrupt_outbox_is_reported() {
        let (_dir, repo, tree) = repo_with_snapshot();
        std::fs::write(repo.outbox_path(&tree), "{not json}\n").unwrap();
        assert!(matches!(
            repo.outbox(&tree),
            Err(SdkError::Serialization(_))
        ));
    }
}
