//! Append-only per-tree reflog (`trees/<tree>/reflog.jsonl`), reflog
//! revisions (`@{N}`, `BRANCH@{N}`, `HEAD@{N}`) and expiry.

use std::path::PathBuf;

use chrono::{Duration, Utc};

use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::{ReflogEntry, ReflogExpireReport, Snapshot};

impl Repository {
    /// Default reflog retention: entries older than this many days are
    /// dropped by `expire_reflog` / the daemon's maintenance task.
    pub const REFLOG_DEFAULT_MAX_AGE_DAYS: i64 = 90;
    /// Default reflog retention: at most this many entries per tree.
    pub const REFLOG_DEFAULT_KEEP: usize = 1000;

    /// Reflog entries of `tree`, newest first, at most `limit`. Lines that
    /// fail to parse (e.g. a torn write after a crash) are skipped.
    pub fn reflog(&self, tree: &str, limit: Option<usize>) -> Result<Vec<ReflogEntry>> {
        self.reflog_entries(tree, None, limit)
    }

    /// Like [`Repository::reflog`], restricted to entries recorded for
    /// `branch` (when given). The position of an entry in this list is the
    /// `N` of `BRANCH@{N}`.
    pub fn reflog_entries(
        &self,
        tree: &str,
        branch: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ReflogEntry>> {
        self.load_tree(tree)?;
        let Some(text) = fsutil::read_string_opt(&self.reflog_path(tree))? else {
            return Ok(Vec::new());
        };
        let entries = text
            .lines()
            .rev()
            .filter_map(|line| parse_line(tree, line))
            .filter(|e| branch.is_none_or(|b| e.branch == b))
            .take(limit.unwrap_or(usize::MAX))
            .collect();
        Ok(entries)
    }

    /// Append one entry to the reflog of `entry.tree`.
    pub fn append_reflog(&self, entry: &ReflogEntry) -> Result<()> {
        if super::names::validate_tree_name(&entry.tree).is_err()
            || !self.tree_meta_dir(&entry.tree).join("tree.toml").is_file()
        {
            return Err(SdkError::TreeNotFound(entry.tree.clone()));
        }
        let line = serde_json::to_string(entry)?;
        fsutil::append_line(&self.reflog_path(&entry.tree), &line)
    }

    /// Drop reflog entries of `tree` older than `older_than` and all but the
    /// newest `keep` entries (malformed lines are dropped too). Defaults:
    /// [`Repository::REFLOG_DEFAULT_MAX_AGE_DAYS`] /
    /// [`Repository::REFLOG_DEFAULT_KEEP`]. Snapshots are never deleted.
    pub fn expire_reflog(
        &self,
        tree: &str,
        older_than: Duration,
        keep: usize,
    ) -> Result<ReflogExpireReport> {
        let _lock = self.lock_tree(tree)?;
        self.load_tree(tree)?;
        let path = self.reflog_path(tree);
        let Some(text) = fsutil::read_string_opt(&path)? else {
            return Ok(ReflogExpireReport::default());
        };
        let cutoff = Utc::now() - older_than;
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        // Newest first, so `keep` counts from the newest entry.
        let mut kept: Vec<&str> = lines
            .iter()
            .rev()
            .filter(|line| parse_line(tree, line).is_some_and(|e| e.at >= cutoff))
            .take(keep)
            .copied()
            .collect();
        let report = ReflogExpireReport {
            removed: lines.len() - kept.len(),
            kept: kept.len(),
        };
        if report.removed > 0 {
            kept.reverse();
            let mut data = kept.join("\n");
            if !data.is_empty() {
                data.push('\n');
            }
            fsutil::write_atomic(&path, data.as_bytes())?;
        }
        Ok(report)
    }

    /// [`Repository::expire_reflog`] for every tree (the daemon's
    /// maintenance task). Returns the summed report.
    pub fn expire_all_reflogs(
        &self,
        older_than: Duration,
        keep: usize,
    ) -> Result<ReflogExpireReport> {
        let mut total = ReflogExpireReport::default();
        for tree in self.trees()? {
            let report = self.expire_reflog(&tree.name, older_than, keep)?;
            total.removed += report.removed;
            total.kept += report.kept;
        }
        Ok(total)
    }

    /// [`Repository::expire_all_reflogs`] with the default retention.
    pub fn expire_reflogs_default(&self) -> Result<ReflogExpireReport> {
        self.expire_all_reflogs(
            Duration::days(Self::REFLOG_DEFAULT_MAX_AGE_DAYS),
            Self::REFLOG_DEFAULT_KEEP,
        )
    }

    /// Resolve `NAME@{N}`: the snapshot recorded by the N-th newest reflog
    /// entry (0 = newest) of the current branch (`NAME` empty), of every
    /// branch of the tree (`NAME` = `HEAD`), or of branch `NAME`.
    pub(crate) fn resolve_reflog_rev(
        &self,
        tree: &str,
        name: &str,
        n: usize,
        rev: &str,
    ) -> Result<Snapshot> {
        let branch = match name {
            "HEAD" => None,
            "" => Some(self.head_branch(tree)?),
            other => {
                super::names::validate_ref_name("branch", other)
                    .map_err(|_| SdkError::InvalidRevision(rev.to_string()))?;
                Some(other.to_string())
            }
        };
        let entries = self.reflog_entries(tree, branch.as_deref(), Some(n.saturating_add(1)))?;
        let label = branch.as_deref().unwrap_or("HEAD");
        let entry = entries.get(n).ok_or_else(|| {
            SdkError::InvalidRevision(format!(
                "{rev}: the reflog of {label} has only {} entr{}",
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" }
            ))
        })?;
        let id = entry.new.as_deref().ok_or_else(|| {
            SdkError::InvalidRevision(format!(
                "{rev}: reflog entry \"{}\" records no snapshot",
                entry.action
            ))
        })?;
        self.get_snapshot(id)
    }

    fn reflog_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("reflog.jsonl")
    }
}

fn parse_line(tree: &str, line: &str) -> Option<ReflogEntry> {
    if line.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<ReflogEntry>(line) {
        Ok(e) => Some(e),
        Err(err) => {
            tracing::warn!(tree, %err, "skipping malformed reflog line");
            None
        }
    }
}

/// Split a reflog revision base `NAME@{N}` into `(NAME, N)`. `None` when
/// `base` is not of that form; `Err` when the `{...}` part is not a number.
pub(crate) fn parse_reflog_rev(base: &str) -> Option<std::result::Result<(&str, usize), ()>> {
    let inner = base.strip_suffix('}')?;
    let at = inner.rfind("@{")?;
    let (name, n) = (&inner[..at], &inner[at + 2..]);
    Some(n.parse().map(|n| (name, n)).map_err(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reflog_revisions() {
        assert_eq!(parse_reflog_rev("@{0}"), Some(Ok(("", 0))));
        assert_eq!(parse_reflog_rev("main@{12}"), Some(Ok(("main", 12))));
        assert_eq!(parse_reflog_rev("HEAD@{1}"), Some(Ok(("HEAD", 1))));
        assert_eq!(parse_reflog_rev("feat/x@{3}"), Some(Ok(("feat/x", 3))));
        assert_eq!(parse_reflog_rev("main@{yesterday}"), Some(Err(())));
        assert_eq!(parse_reflog_rev("main"), None);
        assert_eq!(parse_reflog_rev("abc}"), None);
    }
}
