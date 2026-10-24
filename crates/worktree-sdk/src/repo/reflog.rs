//! Append-only per-tree reflog (`trees/<tree>/reflog.jsonl`).

use std::path::PathBuf;

use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::ReflogEntry;

impl Repository {
    /// Reflog entries of `tree`, newest first, at most `limit`. Lines that
    /// fail to parse (e.g. a torn write after a crash) are skipped.
    pub fn reflog(&self, tree: &str, limit: Option<usize>) -> Result<Vec<ReflogEntry>> {
        self.load_tree(tree)?;
        let Some(text) = fsutil::read_string_opt(&self.reflog_path(tree))? else {
            return Ok(Vec::new());
        };
        let entries = text
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| match serde_json::from_str::<ReflogEntry>(line) {
                Ok(e) => Some(e),
                Err(err) => {
                    tracing::warn!(tree, %err, "skipping malformed reflog line");
                    None
                }
            })
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

    fn reflog_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("reflog.jsonl")
    }
}
