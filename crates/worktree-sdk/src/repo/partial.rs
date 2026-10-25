//! Partial local copies of a tree (ENGINEERING_PLAN §13.2):
//!
//! - **Shallow history** — `.wt/trees/<tree>/shallow` lists the boundary
//!   snapshots: stored locally, but with at least one parent that is not
//!   (it was cut off by `wt clone/fetch --depth`). History walks stop there.
//! - **Stubbed paths** — `.wt/trees/<tree>/stubbed` lists paths whose
//!   content the server withheld (license rules, `PullResponse.stubbed_paths`).
//!   They are not written on checkout and `wt status` reports them as
//!   restricted.
//!
//! Both files hold one entry per line, sorted.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::{fsutil, Repository};
use crate::error::Result;
use crate::model::SnapshotId;

const SHALLOW_FILE: &str = "shallow";
const STUBBED_FILE: &str = "stubbed";

impl Repository {
    /// Boundary snapshots of a shallow tree (empty when the history is
    /// complete).
    pub fn shallow_boundary(&self, tree: &str) -> Result<BTreeSet<SnapshotId>> {
        read_lines(&self.partial_file(tree, SHALLOW_FILE))
    }

    /// Whether the tree's local history is shallow.
    pub fn is_shallow(&self, tree: &str) -> Result<bool> {
        Ok(!self.shallow_boundary(tree)?.is_empty())
    }

    /// Recompute the boundary from the current boundary plus `candidates`
    /// (e.g. snapshots just fetched): every stored snapshot among them with
    /// a parent that is not stored. Returns the new boundary.
    pub(crate) fn update_shallow_boundary<'a>(
        &self,
        tree: &str,
        candidates: impl IntoIterator<Item = &'a str>,
    ) -> Result<BTreeSet<SnapshotId>> {
        let mut all = self.shallow_boundary(tree)?;
        all.extend(candidates.into_iter().map(str::to_string));
        let mut boundary = BTreeSet::new();
        for id in all {
            let Some(snap) = self.load_snapshot_opt(&id)? else {
                continue;
            };
            if snap.parents.iter().any(|p| !self.has_snapshot(p)) {
                boundary.insert(id);
            }
        }
        write_lines(&self.partial_file(tree, SHALLOW_FILE), &boundary)?;
        Ok(boundary)
    }

    /// Paths of the tree whose content the server withheld.
    pub fn stubbed_paths(&self, tree: &str) -> Result<BTreeSet<String>> {
        read_lines(&self.partial_file(tree, STUBBED_FILE))
    }

    /// Replace the stubbed path list.
    pub(crate) fn set_stubbed_paths(&self, tree: &str, paths: &BTreeSet<String>) -> Result<()> {
        write_lines(&self.partial_file(tree, STUBBED_FILE), paths)
    }

    /// Stubbed paths of the tree's head snapshot whose content is not
    /// available locally — what `wt status` reports as "restricted".
    pub fn restricted_paths(&self, tree: &str) -> Result<Vec<String>> {
        let stubbed = self.stubbed_paths(tree)?;
        if stubbed.is_empty() {
            return Ok(Vec::new());
        }
        let head = self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?;
        Ok(stubbed
            .into_iter()
            .filter(|p| head.get(p).is_some_and(|e| !self.store.has_entry(e)))
            .collect())
    }

    fn partial_file(&self, tree: &str, name: &str) -> PathBuf {
        self.tree_meta_dir(tree).join(name)
    }
}

fn read_lines(path: &std::path::Path) -> Result<BTreeSet<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(e.into()),
    }
}

/// Write the set (sorted, one per line); an empty set removes the file.
fn write_lines(path: &std::path::Path, lines: &BTreeSet<String>) -> Result<()> {
    if lines.is_empty() {
        return fsutil::remove_file_if_exists(path);
    }
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fsutil::write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_files_round_trip_and_empty_removes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("list");
        assert!(read_lines(&path).unwrap().is_empty());
        let set: BTreeSet<String> = ["b", "a"].iter().map(|s| s.to_string()).collect();
        write_lines(&path, &set).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\n");
        assert_eq!(read_lines(&path).unwrap(), set);
        write_lines(&path, &BTreeSet::new()).unwrap();
        assert!(!path.exists());
    }
}
