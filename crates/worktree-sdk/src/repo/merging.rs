//! Merge, merge-state bookkeeping, revert and restore.
//!
//! The pure algorithms live in [`crate::merge`]; this module applies them
//! to the repository: loading manifests and blobs, writing results into the
//! working directory and recording snapshots and refs.

use std::collections::HashSet;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::snapshotting::{compute_changes, CommitKind};
use super::trees::TreeRecord;
use super::{fsutil, names, Repository};
use crate::diff::is_binary;
use crate::error::{Result, SdkError};
use crate::merge::{merge_manifests, merge_text, MergeLabels, PathResolution, TextMergeResult};
use crate::model::*;

/// Conflict-marker prefixes that must be gone before a merge can complete.
/// (`=======` alone is not checked: it is a common Markdown underline.)
const MARKER_PREFIXES: [&str; 3] = ["<<<<<<<", "|||||||", ">>>>>>>"];

/// Contents of `.wt/conflicts/<tree>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConflictReport {
    tree: String,
    branch: String,
    source_branch: String,
    source_tip: SnapshotId,
    target_tip: Option<SnapshotId>,
    base: Option<SnapshotId>,
    conflicts: Vec<ConflictInfo>,
}

/// Outcome of a three-way manifest merge, before touching the disk.
struct ThreeWay {
    /// Resolved entries; for conflicted paths, the side kept in the
    /// working tree.
    entries: Vec<ManifestEntry>,
    conflicts: Vec<ConflictInfo>,
    /// Text files whose working copy gets diff3 conflict markers.
    marker_files: Vec<(String, Vec<u8>)>,
}

impl Repository {
    // ----------------------------------------------------------------
    // Merge
    // ----------------------------------------------------------------

    /// Merge `source` (branch or revision) into the current branch.
    /// Uses `crate::merge` for manifest/text merging. Requires a clean
    /// working tree. On conflicts writes markers into files, saves
    /// MERGE_STATE.json + `.wt/conflicts/<tree>.json`, and returns
    /// `MergeOutcome::Conflicts`.
    pub fn merge(&self, tree: &str, source: &str, opts: MergeOptions) -> Result<MergeOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let (branch, ours_tip) = self.head_tip(tree)?;
        let theirs = self.resolve(tree, source)?;
        let ours_manifest = self.manifest_of_tip(ours_tip.as_deref())?;
        let mut working = self.build_working(&rec, &ours_manifest, false, false)?;
        let changes = compute_changes(&ours_manifest, &working.manifest);
        if !changes.is_empty() {
            return Err(SdkError::DirtyWorkingTree(changes.len()));
        }
        let tracked: HashSet<String> = ours_manifest
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect();

        let ours = match ours_tip {
            Some(id) if id == theirs.id || self.is_ancestor(&theirs.id, &id)? => {
                return Ok(MergeOutcome::UpToDate)
            }
            Some(id) if opts.no_fast_forward || !self.is_ancestor(&id, &theirs.id)? => id,
            old => {
                // Fast-forward (an empty branch always fast-forwards).
                let target = self.manifest_of(&theirs)?;
                self.apply_manifest(
                    &rec,
                    &target,
                    &tracked,
                    &working.manifest,
                    &mut working.index,
                )?;
                self.write_tip(tree, &branch, Some(&theirs.id))?;
                self.save_index(tree, &working.index)?;
                self.append_reflog(&ReflogEntry {
                    at: Utc::now(),
                    tree: tree.to_string(),
                    branch: branch.clone(),
                    action: "fast-forward".into(),
                    old,
                    new: Some(theirs.id.clone()),
                    message: format!("fast-forward {branch} to {source}"),
                })?;
                return Ok(MergeOutcome::FastForward { to: theirs.id });
            }
        };

        let base = self.merge_base(&ours, &theirs.id)?;
        let base_manifest = self.manifest_of_tip(base.as_deref())?;
        let theirs_manifest = self.manifest_of(&theirs)?;
        let labels = MergeLabels {
            ours: branch.clone(),
            base: base.as_deref().map(short).unwrap_or("base").to_string(),
            theirs: source.to_string(),
        };
        let result = self.three_way(&base_manifest, &ours_manifest, &theirs_manifest, &labels)?;
        let merged = Manifest::from_entries(result.entries);
        let message = opts
            .message
            .clone()
            .unwrap_or_else(|| format!("Merge {source} into {branch}"));

        self.apply_manifest(
            &rec,
            &merged,
            &tracked,
            &working.manifest,
            &mut working.index,
        )?;

        if result.conflicts.is_empty() {
            let snapshot = Snapshot {
                id: uuid::Uuid::new_v4().to_string(),
                tree_id: rec.id.clone(),
                branch: branch.clone(),
                parents: vec![ours.clone(), theirs.id.clone()],
                manifest: self.write_manifest(&merged)?,
                author: opts.author.clone().unwrap_or_else(|| self.author()),
                message,
                created_at: Utc::now(),
                auto_generated: false,
            };
            self.write_snapshot(&snapshot)?;
            self.write_tip(tree, &branch, Some(&snapshot.id))?;
            self.save_index(tree, &working.index)?;
            self.append_reflog(&ReflogEntry {
                at: snapshot.created_at,
                tree: tree.to_string(),
                branch,
                action: "merge".into(),
                old: Some(ours),
                new: Some(snapshot.id.clone()),
                message: snapshot
                    .message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            })?;
            return Ok(MergeOutcome::Merged { snapshot });
        }

        let tree_dir = self.tree_dir_of(&rec);
        for (path, content) in &result.marker_files {
            fsutil::write_atomic(&tree_dir.join(path), content)?;
            // Drop the cache entry so the marker content is hashed fresh.
            working.index.remove(path);
        }
        self.save_index(tree, &working.index)?;
        let state = MergeState {
            source_branch: source.to_string(),
            source_tip: theirs.id.clone(),
            target_tip: Some(ours.clone()),
            base: base.clone(),
            conflicts: result.conflicts.clone(),
            message,
        };
        fsutil::write_json(&self.merge_state_path(tree), &state)?;
        fsutil::write_json(
            &self.conflicts_path(tree),
            &ConflictReport {
                tree: tree.to_string(),
                branch,
                source_branch: source.to_string(),
                source_tip: theirs.id,
                target_tip: Some(ours),
                base,
                conflicts: result.conflicts.clone(),
            },
        )?;
        Ok(MergeOutcome::Conflicts {
            conflicts: result.conflicts,
        })
    }

    /// The pending conflicted merge of `tree`, if any.
    pub fn merge_state(&self, tree: &str) -> Result<Option<MergeState>> {
        self.load_tree(tree)?;
        fsutil::read_json_opt(&self.merge_state_path(tree))
    }

    /// Restore the pre-merge head into the working tree and clear merge state.
    pub fn abort_merge(&self, tree: &str) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let state = self.merge_state(tree)?.ok_or(SdkError::NoMergeInProgress)?;
        let head = self.manifest_of_tip(state.target_tip.as_deref())?;
        let theirs = self.manifest_of_tip(Some(&state.source_tip))?;
        // Files the merge brought in from the source are tracked too, so
        // they are removed again; files the user created since are kept.
        let tracked: HashSet<String> = head
            .entries
            .iter()
            .chain(&theirs.entries)
            .map(|e| e.path.clone())
            .collect();
        let mut working = self.build_working(&rec, &head, false, false)?;
        self.apply_manifest(&rec, &head, &tracked, &working.manifest, &mut working.index)?;
        self.save_index(tree, &working.index)?;
        self.clear_merge_state(tree)
    }

    /// Finish a conflicted merge once markers are resolved: creates the
    /// two-parent merge snapshot. `MergeConflict` if markers remain.
    pub fn continue_merge(&self, tree: &str, message: Option<&str>) -> Result<Snapshot> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let state = self.merge_state(tree)?.ok_or(SdkError::NoMergeInProgress)?;
        let unresolved = self.unresolved_conflicts(&rec, &state)?;
        if !unresolved.is_empty() {
            return Err(SdkError::MergeConflict(format!(
                "conflict markers remain in: {}",
                unresolved.join(", ")
            )));
        }
        let opts = SnapshotOptions {
            message: message.unwrap_or_default().to_string(),
            ..SnapshotOptions::default()
        };
        self.commit_working(&rec, &opts, CommitKind::MergeCompletion(&state))
    }

    // ----------------------------------------------------------------
    // Revert / restore
    // ----------------------------------------------------------------

    /// New snapshot on the current branch that undoes `rev`'s changes
    /// relative to its first parent (append-only; history is not rewritten).
    /// Requires a clean working tree; `MergeConflict` if later snapshots
    /// changed the same lines, `NoChanges` if nothing is left to undo.
    pub fn revert(&self, tree: &str, rev: &str) -> Result<Snapshot> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let (branch, head) = self.head_tip(tree)?;
        let head = head.ok_or_else(|| {
            SdkError::InvalidRevision(format!("{branch} has no snapshots to revert"))
        })?;
        let target = self.resolve(tree, rev)?;
        let target_manifest = self.manifest_of(&target)?;
        let parent_manifest = self.manifest_of_tip(target.parents.first().map(String::as_str))?;
        let head_manifest = self.manifest_of_tip(Some(&head))?;
        let mut working = self.build_working(&rec, &head_manifest, false, false)?;
        let changes = compute_changes(&head_manifest, &working.manifest);
        if !changes.is_empty() {
            return Err(SdkError::DirtyWorkingTree(changes.len()));
        }

        let result = match simple_revert(&target_manifest, &parent_manifest, &head_manifest) {
            Some(entries) => Manifest::from_entries(entries),
            None => {
                // Later snapshots touched the same files: undo via a
                // three-way merge with base = rev, theirs = rev's parent.
                let labels = MergeLabels {
                    ours: branch.clone(),
                    base: target.short_id().to_string(),
                    theirs: format!("parent of {}", target.short_id()),
                };
                let merged =
                    self.three_way(&target_manifest, &head_manifest, &parent_manifest, &labels)?;
                if !merged.conflicts.is_empty() {
                    let paths: Vec<_> = merged.conflicts.iter().map(|c| c.path.as_str()).collect();
                    return Err(SdkError::MergeConflict(format!(
                        "reverting {} conflicts in: {}",
                        target.short_id(),
                        paths.join(", ")
                    )));
                }
                Manifest::from_entries(merged.entries)
            }
        };
        if result == head_manifest {
            return Err(SdkError::NoChanges);
        }

        let tracked: HashSet<String> = head_manifest
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect();
        self.apply_manifest(
            &rec,
            &result,
            &tracked,
            &working.manifest,
            &mut working.index,
        )?;
        let summary = target.message.lines().next().unwrap_or_default();
        let snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: rec.id.clone(),
            branch: branch.clone(),
            parents: vec![head.clone()],
            manifest: self.write_manifest(&result)?,
            author: self.author(),
            message: format!(
                "Revert \"{summary}\"\n\nThis reverts snapshot {}.",
                target.id
            ),
            created_at: Utc::now(),
            auto_generated: false,
        };
        self.write_snapshot(&snapshot)?;
        self.write_tip(tree, &branch, Some(&snapshot.id))?;
        self.save_index(tree, &working.index)?;
        self.append_reflog(&ReflogEntry {
            at: snapshot.created_at,
            tree: tree.to_string(),
            branch,
            action: "revert".into(),
            old: Some(head),
            new: Some(snapshot.id.clone()),
            message: format!("revert {}", target.short_id()),
        })?;
        Ok(snapshot)
    }

    /// Overwrite working files at `paths` (all if empty) with their content
    /// at `rev` (default HEAD), deleting files absent there. Paths are
    /// tree-relative and match as prefixes (a directory restores
    /// everything below it). Returns the paths that were changed, sorted.
    pub fn restore(&self, tree: &str, paths: &[String], rev: Option<&str>) -> Result<Vec<String>> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let (branch, head) = self.head_tip(tree)?;
        let source = match rev {
            Some(rev) => self.manifest_of(&self.resolve(tree, rev)?)?,
            None => self.manifest_of_tip(head.as_deref())?,
        };
        let head_manifest = self.manifest_of_tip(head.as_deref())?;
        let scopes: Vec<String> = paths.iter().map(|p| names::normalize_rel_path(p)).collect();
        let in_scope =
            |p: &str| scopes.is_empty() || scopes.iter().any(|s| names::path_has_prefix(p, s));
        let mut working = self.build_working(&rec, &head_manifest, false, false)?;
        let tree_dir = self.tree_dir_of(&rec);
        let nested = self.nested_tree_rel_paths(&rec)?;

        let mut changed = Vec::new();
        for entry in working
            .manifest
            .entries
            .iter()
            .filter(|e| in_scope(&e.path))
        {
            if source.get(&entry.path).is_none() {
                self.remove_working_file(&tree_dir, &entry.path)?;
                working.index.remove(&entry.path);
                changed.push(entry.path.clone());
            }
        }
        for entry in source.entries.iter().filter(|e| in_scope(&e.path)) {
            if nested
                .iter()
                .any(|n| names::path_has_prefix(&entry.path, n))
            {
                continue;
            }
            if working.manifest.get(&entry.path) == Some(entry) {
                continue;
            }
            let cached = self.materialize(&tree_dir, entry)?;
            working.index.insert(entry.path.clone(), cached);
            changed.push(entry.path.clone());
        }
        self.save_index(tree, &working.index)?;
        changed.sort();
        if !changed.is_empty() {
            self.append_reflog(&ReflogEntry {
                at: Utc::now(),
                tree: tree.to_string(),
                branch,
                action: "restore".into(),
                old: head.clone(),
                new: head,
                message: format!(
                    "restored {} path(s) from {}",
                    changed.len(),
                    rev.unwrap_or("HEAD")
                ),
            })?;
        }
        Ok(changed)
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    pub(crate) fn clear_merge_state(&self, tree: &str) -> Result<()> {
        fsutil::remove_file_if_exists(&self.merge_state_path(tree))?;
        fsutil::remove_file_if_exists(&self.conflicts_path(tree))
    }

    /// Conflicted text files that still contain conflict markers.
    pub(crate) fn unresolved_conflicts(
        &self,
        rec: &TreeRecord,
        state: &MergeState,
    ) -> Result<Vec<String>> {
        let tree_dir = self.tree_dir_of(rec);
        let mut out = Vec::new();
        for conflict in &state.conflicts {
            if !matches!(conflict.kind, ConflictKind::Content | ConflictKind::AddAdd) {
                continue;
            }
            let Some(text) = fsutil::read_string_opt(&tree_dir.join(&conflict.path))
                .ok()
                .flatten()
            else {
                continue;
            };
            if has_conflict_markers(&text) {
                out.push(conflict.path.clone());
            }
        }
        Ok(out)
    }

    /// Resolve every path of a three-way merge, merging text content and
    /// storing cleanly merged blobs.
    fn three_way(
        &self,
        base: &Manifest,
        ours: &Manifest,
        theirs: &Manifest,
        labels: &MergeLabels,
    ) -> Result<ThreeWay> {
        let mut out = ThreeWay {
            entries: Vec::new(),
            conflicts: Vec::new(),
            marker_files: Vec::new(),
        };
        for (path, resolution) in merge_manifests(base, ours, theirs) {
            match resolution {
                PathResolution::KeepOurs(e) | PathResolution::TakeTheirs(e) => out.entries.push(e),
                PathResolution::Delete => {}
                PathResolution::Conflict {
                    kind, ours, theirs, ..
                } => {
                    if let Some(e) = ours.or(theirs) {
                        out.entries.push(e);
                    }
                    out.conflicts.push(ConflictInfo { path, kind });
                }
                PathResolution::MergeContent { base, ours, theirs } => {
                    let ours_data = self.store.get(&ours.hash)?;
                    let theirs_data = self.store.get(&theirs.hash)?;
                    let base_data = match &base {
                        Some(b) => self.store.get(&b.hash)?,
                        None => Vec::new(),
                    };
                    let texts = (
                        String::from_utf8(base_data),
                        String::from_utf8(ours_data),
                        String::from_utf8(theirs_data),
                    );
                    let (Ok(base_text), Ok(ours_text), Ok(theirs_text)) = texts else {
                        out.entries.push(ours);
                        out.conflicts.push(ConflictInfo {
                            path,
                            kind: ConflictKind::Binary,
                        });
                        continue;
                    };
                    if [&base_text, &ours_text, &theirs_text]
                        .iter()
                        .any(|t| is_binary(t.as_bytes()))
                    {
                        out.entries.push(ours);
                        out.conflicts.push(ConflictInfo {
                            path,
                            kind: ConflictKind::Binary,
                        });
                        continue;
                    }
                    let executable = match &base {
                        Some(b) if b.executable == ours.executable => theirs.executable,
                        _ => ours.executable,
                    };
                    match merge_text(&base_text, &ours_text, &theirs_text, labels) {
                        TextMergeResult::Clean(text) => {
                            let hash = self.store.put(text.as_bytes())?;
                            out.entries.push(ManifestEntry {
                                path,
                                hash,
                                size: text.len() as u64,
                                executable,
                            });
                        }
                        TextMergeResult::Conflicted { merged, .. } => {
                            let kind = if base.is_some() {
                                ConflictKind::Content
                            } else {
                                ConflictKind::AddAdd
                            };
                            out.entries.push(ManifestEntry { executable, ..ours });
                            out.conflicts.push(ConflictInfo {
                                path: path.clone(),
                                kind,
                            });
                            out.marker_files.push((path, merged.into_bytes()));
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

/// Undo `target`'s changes (relative to `parent`) on top of `head` when no
/// later snapshot touched the same paths. `None` if a content merge is needed.
fn simple_revert(
    target: &Manifest,
    parent: &Manifest,
    head: &Manifest,
) -> Option<Vec<ManifestEntry>> {
    let mut result: std::collections::BTreeMap<String, ManifestEntry> = head
        .entries
        .iter()
        .map(|e| (e.path.clone(), e.clone()))
        .collect();
    for change in compute_changes(parent, target) {
        let path = &change.path;
        let in_head = head.get(path);
        let in_target = target.get(path);
        let in_parent = parent.get(path);
        if in_head == in_target {
            match in_parent {
                Some(e) => result.insert(path.clone(), e.clone()),
                None => result.remove(path),
            };
        } else if in_head != in_parent {
            return None;
        }
    }
    Some(result.into_values().collect())
}

fn has_conflict_markers(text: &str) -> bool {
    text.lines()
        .any(|l| MARKER_PREFIXES.iter().any(|m| l.starts_with(m)))
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::hash_bytes;

    fn m(entries: &[(&str, &str)]) -> Manifest {
        Manifest::from_entries(
            entries
                .iter()
                .map(|(p, c)| ManifestEntry {
                    path: (*p).into(),
                    hash: hash_bytes(c.as_bytes()),
                    size: c.len() as u64,
                    executable: false,
                })
                .collect(),
        )
    }

    #[test]
    fn simple_revert_undoes_untouched_changes() {
        let parent = m(&[("a", "1"), ("b", "2")]);
        let target = m(&[("a", "1x"), ("c", "3")]); // modify a, delete b, add c
        let head = m(&[("a", "1x"), ("c", "3"), ("d", "4")]);
        let out = Manifest::from_entries(simple_revert(&target, &parent, &head).unwrap());
        assert_eq!(out, m(&[("a", "1"), ("b", "2"), ("d", "4")]));
    }

    #[test]
    fn simple_revert_skips_already_reverted_and_detects_overlap() {
        let parent = m(&[("a", "1")]);
        let target = m(&[("a", "2")]);
        assert_eq!(
            Manifest::from_entries(simple_revert(&target, &parent, &parent).unwrap()),
            parent
        );
        let head = m(&[("a", "3")]);
        assert!(simple_revert(&target, &parent, &head).is_none());
    }

    #[test]
    fn marker_detection() {
        assert!(has_conflict_markers(
            "a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
        ));
        assert!(!has_conflict_markers("Title\n=======\nbody\n"));
        assert!(!has_conflict_markers("plain"));
    }
}
