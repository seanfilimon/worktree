//! The stash: working changes set aside and restored later
//! (ENGINEERING_PLAN §14.5).
//!
//! Entries live in `.wt/trees/<tree>/stash.jsonl`, one [`StashEntry`] per
//! line, oldest first; entry `0` is the newest. An entry references two
//! stored manifests: the head it was taken on (`base_manifest`) and the
//! stashed working state (`manifest`), so it stays applicable even when
//! the base snapshot is later rewritten. [`Repository::stash_roots`] lists
//! both for garbage collection.
//!
//! "Untracked" files are files absent from the head snapshot (new files);
//! they are only stashed with `include_untracked`. Applying an entry is a
//! three-way merge of the stashed state into the working tree (base = the
//! stash's base); conflicting text files get conflict markers, other
//! conflicts keep the working copy. No merge state is recorded: fix the
//! files and snapshot as usual. `pop` keeps the entry when there were
//! conflicts.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::snapshotting::compute_changes;
use super::trees::TreeRecord;
use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::merge::MergeLabels;
use crate::model::*;
use crate::store::ObjectHash;

/// One stash entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    /// Stable id (UUID); positions shift as entries are added and dropped.
    pub id: String,
    pub message: String,
    /// Branch the changes were stashed on.
    pub branch: String,
    /// Head snapshot the changes were made against.
    pub base: Option<SnapshotId>,
    /// Manifest of `base` (empty manifest for an empty branch).
    pub base_manifest: ObjectHash,
    /// Manifest of the stashed working state.
    pub manifest: ObjectHash,
    /// Whether new files were stashed too.
    pub include_untracked: bool,
    /// Created by `wt switch` with `auto_snapshot.on_branch_switch = "stash"`.
    #[serde(default)]
    pub auto: bool,
    pub created_at: DateTime<Utc>,
    /// The stashed changes relative to `base`.
    #[serde(default)]
    pub changes: Vec<FileChange>,
}

/// Options of [`Repository::stash_push`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StashOptions {
    /// Default: `WIP on <branch>: <short id> <subject>`.
    pub message: Option<String>,
    /// Also stash (and remove) files that are not in the head snapshot.
    pub include_untracked: bool,
    /// Mark the entry as an automatic branch-switch stash.
    pub auto: bool,
}

/// Result of [`Repository::stash_apply`] / [`Repository::stash_pop`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashApplyOutcome {
    pub entry: StashEntry,
    /// Position the entry had (0 = newest).
    pub index: usize,
    /// Working files written or deleted, sorted.
    pub changed: Vec<String>,
    /// Paths that conflicted (text files carry markers).
    pub conflicts: Vec<ConflictInfo>,
    /// The entry was removed (`pop` without conflicts).
    pub dropped: bool,
}

/// What [`Repository::switch_branch_autostash`] did besides switching.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoStashSwitch {
    /// The dirty changes of the branch left behind.
    pub stashed: Option<StashEntry>,
    /// The auto-stash of the target branch that was popped.
    pub restored: Option<StashApplyOutcome>,
}

impl Repository {
    /// Set the working changes aside and reset the working tree to the head
    /// snapshot (`wt stash [push]`). New files stay unless
    /// `include_untracked`. `Other("no local changes to stash")` when there
    /// is nothing to stash. Refused during a merge, cherry-pick or rebase.
    pub fn stash_push(&self, tree: &str, opts: &StashOptions) -> Result<StashEntry> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_rewrite(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let (branch, head) = self.head_tip(tree)?;
        let head_manifest = self.manifest_of_tip(head.as_deref())?;
        let mut working = self.build_working(&rec, &head_manifest, true, false)?;
        let all = compute_changes(&head_manifest, &working.manifest);
        let changes: Vec<FileChange> = all
            .iter()
            .filter(|c| opts.include_untracked || c.kind != ChangeKind::Added)
            .cloned()
            .collect();
        if changes.is_empty() {
            self.save_index(tree, &working.index)?;
            let hint = if all.is_empty() {
                ""
            } else {
                " (new files are only stashed with --include-untracked)"
            };
            return Err(SdkError::Other(format!("no local changes to stash{hint}")));
        }

        let tracked: HashSet<String> = changes.iter().map(|c| c.path.clone()).collect();
        let mut entries: Vec<ManifestEntry> = head_manifest
            .entries
            .iter()
            .filter(|e| !tracked.contains(&e.path))
            .cloned()
            .collect();
        entries.extend(
            working
                .manifest
                .entries
                .iter()
                .filter(|e| tracked.contains(&e.path))
                .cloned(),
        );
        let stashed = Manifest::from_entries(entries);
        let message = match &opts.message {
            Some(m) if !m.trim().is_empty() => m.trim().to_string(),
            _ => match &head {
                Some(id) => {
                    let snap = self.get_snapshot(id)?;
                    format!(
                        "WIP on {branch}: {} {}",
                        snap.short_id(),
                        snap.message.lines().next().unwrap_or_default()
                    )
                }
                None => format!("WIP on {branch}"),
            },
        };
        let entry = StashEntry {
            id: uuid::Uuid::new_v4().to_string(),
            message,
            branch,
            base: head,
            base_manifest: self.write_manifest(&head_manifest)?,
            manifest: self.write_manifest(&stashed)?,
            include_untracked: opts.include_untracked,
            auto: opts.auto,
            created_at: Utc::now(),
            changes,
        };
        fsutil::append_line(&self.stash_path(tree), &serde_json::to_string(&entry)?)?;

        // Reset the stashed paths to the head snapshot.
        self.apply_manifest(
            &rec,
            &head_manifest,
            &tracked,
            &working.manifest,
            &mut working.index,
        )?;
        self.save_index(tree, &working.index)?;
        Ok(entry)
    }

    /// Stash entries of `tree`, newest first (position = `N` of `stash@{N}`).
    pub fn stash_list(&self, tree: &str) -> Result<Vec<StashEntry>> {
        self.load_tree(tree)?;
        let mut entries = self.read_stash(tree)?;
        entries.reverse();
        Ok(entries)
    }

    /// Entry `index` (0 = newest).
    pub fn stash_get(&self, tree: &str, index: usize) -> Result<StashEntry> {
        let entries = self.stash_list(tree)?;
        entries
            .get(index)
            .cloned()
            .ok_or_else(|| no_entry(index, entries.len()))
    }

    /// Entry `index` and its changes relative to its base (`wt stash show`).
    pub fn stash_show(
        &self,
        tree: &str,
        index: usize,
        opts: &DiffOptions,
    ) -> Result<(StashEntry, Vec<FileDiff>)> {
        let entry = self.stash_get(tree, index)?;
        let base = self.read_manifest(&entry.base_manifest)?;
        let stashed = self.read_manifest(&entry.manifest)?;
        let diffs = self.diff_manifests(&base, &stashed, &super::diffing::NewSide::Store, opts)?;
        Ok((entry, diffs))
    }

    /// Three-way merge entry `index` into the working tree (`wt stash
    /// apply [N]`); the entry is kept.
    pub fn stash_apply(&self, tree: &str, index: usize) -> Result<StashApplyOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.apply_stash_unlocked(&rec, index)
    }

    /// [`Repository::stash_apply`], then drop the entry unless the merge
    /// conflicted (`wt stash pop [N]`).
    pub fn stash_pop(&self, tree: &str, index: usize) -> Result<StashApplyOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let mut outcome = self.apply_stash_unlocked(&rec, index)?;
        if outcome.conflicts.is_empty() {
            self.remove_stash_entries(tree, |e| e.id == outcome.entry.id)?;
            outcome.dropped = true;
        }
        Ok(outcome)
    }

    /// Remove entry `index` (`wt stash drop [N]`); returns it.
    pub fn stash_drop(&self, tree: &str, index: usize) -> Result<StashEntry> {
        let _lock = self.lock_tree(tree)?;
        self.load_tree(tree)?;
        let entry = self.stash_get(tree, index)?;
        self.remove_stash_entries(tree, |e| e.id == entry.id)?;
        Ok(entry)
    }

    /// Remove every entry (`wt stash clear`); returns how many there were.
    pub fn stash_clear(&self, tree: &str) -> Result<usize> {
        let _lock = self.lock_tree(tree)?;
        self.load_tree(tree)?;
        let count = self.read_stash(tree)?.len();
        fsutil::remove_file_if_exists(&self.stash_path(tree))?;
        Ok(count)
    }

    /// Manifests referenced by the stash entries of every tree (stashed
    /// states and their bases). Local garbage collection must keep these
    /// manifests and the objects they reference.
    pub fn stash_roots(&self) -> Result<Vec<ObjectHash>> {
        let mut out = Vec::new();
        for rec in self.tree_records()? {
            for entry in self.read_stash(&rec.name)? {
                out.push(entry.manifest);
                out.push(entry.base_manifest);
            }
        }
        out.sort_by_key(|h| h.to_hex());
        out.dedup();
        Ok(out)
    }

    /// `wt switch` with `auto_snapshot.on_branch_switch = "stash"`: dirty
    /// changes (new files included) are stashed as `auto-stash on
    /// <branch>` before leaving, and the newest auto-stash of the target
    /// branch is popped after arriving. With `force` or `create`, or when
    /// staying on the same branch, this is a plain
    /// [`Repository::switch_branch`].
    pub fn switch_branch_autostash(
        &self,
        tree: &str,
        name: &str,
        opts: SwitchOptions,
    ) -> Result<AutoStashSwitch> {
        let current = self.current_branch(tree)?;
        let mut report = AutoStashSwitch::default();
        if opts.force || opts.create || name == current {
            self.switch_branch(tree, name, opts)?;
            return Ok(report);
        }
        // Fail before stashing when the switch cannot happen anyway.
        self.branch_tip(tree, name)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        if !self.status(tree)?.is_clean() {
            let entry = self.stash_push(
                tree,
                &StashOptions {
                    message: Some(format!("auto-stash on {current}")),
                    include_untracked: true,
                    auto: true,
                },
            )?;
            report.stashed = Some(entry);
        }
        if let Err(err) = self.switch_branch(tree, name, opts) {
            if report.stashed.is_some() {
                // Put the changes back where they were.
                self.stash_pop(tree, 0)?;
            }
            return Err(err);
        }
        let pending = self
            .stash_list(tree)?
            .iter()
            .position(|e| e.auto && e.branch == name);
        if let Some(index) = pending {
            report.restored = Some(self.stash_pop(tree, index)?);
        }
        Ok(report)
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    fn stash_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("stash.jsonl")
    }

    /// Entries oldest first; malformed lines are skipped.
    fn read_stash(&self, tree: &str) -> Result<Vec<StashEntry>> {
        let Some(text) = fsutil::read_string_opt(&self.stash_path(tree))? else {
            return Ok(Vec::new());
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| match serde_json::from_str::<StashEntry>(line) {
                Ok(e) => Some(e),
                Err(err) => {
                    tracing::warn!(tree, %err, "skipping malformed stash entry");
                    None
                }
            })
            .collect())
    }

    fn remove_stash_entries(&self, tree: &str, drop: impl Fn(&StashEntry) -> bool) -> Result<()> {
        let kept: Vec<StashEntry> = self
            .read_stash(tree)?
            .into_iter()
            .filter(|e| !drop(e))
            .collect();
        let path = self.stash_path(tree);
        if kept.is_empty() {
            return fsutil::remove_file_if_exists(&path);
        }
        let mut data = String::new();
        for entry in &kept {
            data.push_str(&serde_json::to_string(entry)?);
            data.push('\n');
        }
        fsutil::write_atomic(&path, data.as_bytes())
    }

    fn apply_stash_unlocked(&self, rec: &TreeRecord, index: usize) -> Result<StashApplyOutcome> {
        let tree = rec.name.as_str();
        self.ensure_no_rewrite(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let entry = self.stash_get(tree, index)?;
        let base = self.read_manifest(&entry.base_manifest)?;
        let theirs = self.read_manifest(&entry.manifest)?;
        let head = self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?;
        // Store the working blobs so the merge can read "ours".
        let mut working = self.build_working(rec, &head, true, false)?;
        let ours = working.manifest.clone();
        let labels = MergeLabels {
            ours: "ours (working tree)".into(),
            base: match &entry.base {
                Some(id) => format!("base ({})", &id[..id.len().min(8)]),
                None => "base".into(),
            },
            theirs: format!(
                "theirs (stash@{{{index}}}: {})",
                entry.message.lines().next().unwrap_or_default()
            ),
        };
        let result = self.three_way(&base, &ours, &theirs, &labels, MergeStrategy::Manual)?;
        let merged = Manifest::from_entries(result.entries);
        let tracked: HashSet<String> = ours.entries.iter().map(|e| e.path.clone()).collect();
        self.apply_manifest(rec, &merged, &tracked, &ours, &mut working.index)?;
        let tree_dir = self.tree_dir_of(rec);
        for (path, content) in &result.marker_files {
            fsutil::write_atomic(&tree_dir.join(path), content)?;
            working.index.remove(path);
        }
        self.save_index(tree, &working.index)?;
        let mut changed: Vec<String> = compute_changes(&ours, &merged)
            .into_iter()
            .map(|c| c.path)
            .chain(result.marker_files.iter().map(|(p, _)| p.clone()))
            .collect();
        changed.sort();
        changed.dedup();
        Ok(StashApplyOutcome {
            entry,
            index,
            changed,
            conflicts: result.conflicts,
            dropped: false,
        })
    }
}

fn no_entry(index: usize, len: usize) -> SdkError {
    if len == 0 {
        SdkError::InvalidArgument("the stash is empty".into())
    } else {
        SdkError::InvalidArgument(format!(
            "no stash entry {index} (the stash has {len} entr{})",
            if len == 1 { "y" } else { "ies" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_entry_messages() {
        assert_eq!(
            no_entry(0, 0).to_string(),
            "invalid argument: the stash is empty"
        );
        assert!(no_entry(3, 1).to_string().contains("has 1 entry"));
        assert!(no_entry(3, 2).to_string().contains("has 2 entries"));
    }
}
