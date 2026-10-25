//! Merge, merge-state bookkeeping (status, per-path resolution), revert and
//! restore.
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
use crate::merge::{
    merge_executable, merge_manifests, merge_text_with, MergeLabels, PathResolution,
    TextMergeResult,
};
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
    /// Paths resolved with `wt merge resolve`.
    #[serde(default)]
    resolved: Vec<String>,
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

/// The three manifests of an in-progress merge.
struct MergeSides {
    base: Manifest,
    ours: Manifest,
    theirs: Manifest,
    labels: MergeLabels,
}

impl Repository {
    // ----------------------------------------------------------------
    // Merge
    // ----------------------------------------------------------------

    /// Merge `source` (branch or revision) into the current branch.
    /// Uses `crate::merge` for manifest/text merging. Requires a clean
    /// working tree. With `MergeStrategy::Manual` (the default) conflicts
    /// write markers into files, save MERGE_STATE.json +
    /// `.wt/conflicts/<tree>.json` and return `MergeOutcome::Conflicts`;
    /// `Ours` / `Theirs` resolve every conflict in favour of that side.
    /// Conflict markers are labelled with short ids:
    /// `<<<<<<< ours (main 1a2b3c4d)` / `>>>>>>> theirs (feature 5e6f7a8b)`.
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
        let labels = merge_labels(&branch, &ours, source, &theirs.id, base.as_deref());
        let result = self.three_way(
            &base_manifest,
            &ours_manifest,
            &theirs_manifest,
            &labels,
            opts.strategy,
        )?;
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
            let mut snapshot = Snapshot {
                id: uuid::Uuid::new_v4().to_string(),
                tree_id: rec.id.clone(),
                branch: branch.clone(),
                parents: vec![ours.clone(), theirs.id.clone()],
                manifest: self.write_manifest(&merged)?,
                author: opts.author.clone().unwrap_or_else(|| self.author()),
                message,
                created_at: Utc::now(),
                auto_generated: false,
                dependencies: Vec::new(),
                reverts: None,
                signature: None,
            };
            crate::signing::sign_snapshot(self, &mut snapshot)?;
            self.write_snapshot(&snapshot)?;
            self.write_tip(tree, &branch, Some(&snapshot.id))?;
            self.save_index(tree, &working.index)?;
            let strategy = match opts.strategy {
                MergeStrategy::Manual => String::new(),
                other => format!(" (strategy {})", other.as_str()),
            };
            self.append_reflog(&ReflogEntry {
                at: snapshot.created_at,
                tree: tree.to_string(),
                branch,
                action: "merge".into(),
                old: Some(ours),
                new: Some(snapshot.id.clone()),
                message: format!(
                    "{}{strategy}",
                    snapshot.message.lines().next().unwrap_or_default()
                ),
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
            resolved: Vec::new(),
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
                resolved: Vec::new(),
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

    /// The pending conflicted merge of `tree` with the resolution status of
    /// every conflict (`wt merge status`); `None` when no merge is in
    /// progress. Text conflicts count as resolved once their markers are
    /// gone; file-level conflicts (modify/delete, binary) once resolved with
    /// [`Repository::resolve_conflict`].
    pub fn merge_status(&self, tree: &str) -> Result<Option<MergeStatus>> {
        let rec = self.load_tree(tree)?;
        let Some(state) = self.merge_state(tree)? else {
            return Ok(None);
        };
        let with_markers: HashSet<String> = self
            .unresolved_conflicts(&rec, &state)?
            .into_iter()
            .collect();
        let conflicts = state
            .conflicts
            .iter()
            .map(|c| ConflictStatus {
                path: c.path.clone(),
                kind: c.kind,
                hunks: c.hunks.clone(),
                resolved: state.resolved.contains(&c.path)
                    || (is_text_conflict(c.kind) && !with_markers.contains(&c.path)),
            })
            .collect();
        Ok(Some(MergeStatus {
            tree: tree.to_string(),
            branch: self.head_branch(tree)?,
            source_branch: state.source_branch,
            source_tip: state.source_tip,
            target_tip: state.target_tip,
            base: state.base,
            message: state.message,
            conflicts,
        }))
    }

    /// Resolve one conflicted path of the in-progress merge (`wt merge
    /// resolve PATH --ours|--theirs|--file F`). `Ours` / `Theirs` re-merge a
    /// text conflict resolving only the conflicting hunks in favour of that
    /// side (clean changes from the other side are kept) and take that
    /// side's version for file-level conflicts (deleting the file when that
    /// side deleted it); `Content` writes the given bytes. The path is then
    /// recorded as resolved. `InvalidArgument` if `path` has no conflict.
    pub fn resolve_conflict(
        &self,
        tree: &str,
        path: &str,
        resolution: &ConflictResolution,
    ) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let mut state = self.merge_state(tree)?.ok_or(SdkError::NoMergeInProgress)?;
        let path = names::normalize_rel_path(path);
        let conflict = state
            .conflicts
            .iter()
            .find(|c| c.path == path)
            .cloned()
            .ok_or_else(|| SdkError::InvalidArgument(format!("{path} has no merge conflict")))?;
        let sides = self.merge_sides(tree, &state)?;
        let mut index = self.load_index(tree);
        self.apply_resolution(&rec, &sides, &conflict, resolution, &mut index)?;
        self.save_index(tree, &index)?;
        if !state.resolved.contains(&path) {
            state.resolved.push(path);
        }
        self.save_merge_state(tree, &state)
    }

    /// Resolve every still-unresolved conflict (see
    /// [`Repository::merge_status`]) with `Ours` or `Theirs` (`wt merge
    /// resolve --all`). Returns the paths resolved, in conflict order.
    pub fn resolve_all_conflicts(
        &self,
        tree: &str,
        resolution: &ConflictResolution,
    ) -> Result<Vec<String>> {
        if matches!(resolution, ConflictResolution::Content(_)) {
            return Err(SdkError::InvalidArgument(
                "resolving every conflict needs --ours or --theirs".into(),
            ));
        }
        let status = self
            .merge_status(tree)?
            .ok_or(SdkError::NoMergeInProgress)?;
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let mut state = self.merge_state(tree)?.ok_or(SdkError::NoMergeInProgress)?;
        let sides = self.merge_sides(tree, &state)?;
        let mut index = self.load_index(tree);
        let mut done = Vec::new();
        for pending in status.unresolved() {
            let Some(conflict) = state.conflicts.iter().find(|c| c.path == pending.path) else {
                continue;
            };
            self.apply_resolution(&rec, &sides, conflict, resolution, &mut index)?;
            done.push(pending.path.clone());
        }
        self.save_index(tree, &index)?;
        for path in &done {
            if !state.resolved.contains(path) {
                state.resolved.push(path.clone());
            }
        }
        self.save_merge_state(tree, &state)?;
        Ok(done)
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
    /// Merge snapshots need [`Repository::revert_with`] and a parent.
    pub fn revert(&self, tree: &str, rev: &str) -> Result<Snapshot> {
        self.revert_with(tree, rev, &RevertOptions::default())
    }

    /// [`Repository::revert`] with options (`wt revert REV [--message M]
    /// [--parent N]`). A merge snapshot is reverted relative to its
    /// `parent`-th parent (1-based), which is required for merges and
    /// rejected for ordinary snapshots. The new snapshot's `reverts` is set
    /// to the reverted snapshot.
    pub fn revert_with(&self, tree: &str, rev: &str, opts: &RevertOptions) -> Result<Snapshot> {
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
        let parent = revert_parent(&target, opts.parent)?;
        let target_manifest = self.manifest_of(&target)?;
        let parent_manifest = self.manifest_of_tip(parent.map(|(_, id)| id))?;
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
                    ours: format!("ours ({branch} {})", short(&head)),
                    base: format!("base ({})", target.short_id()),
                    theirs: format!("theirs (parent of {})", target.short_id()),
                };
                let merged = self.three_way(
                    &target_manifest,
                    &head_manifest,
                    &parent_manifest,
                    &labels,
                    MergeStrategy::Manual,
                )?;
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
        let message = match &opts.message {
            Some(message) if !message.trim().is_empty() => message.clone(),
            _ => {
                let mut message = format!(
                    "Revert \"{summary}\"\n\nThis reverts snapshot {}",
                    target.id
                );
                match parent {
                    Some((n, id)) if target.is_merge() => message.push_str(&format!(
                        ", reversing the changes made to parent {n} ({}).",
                        short(id)
                    )),
                    _ => message.push('.'),
                }
                message
            }
        };
        let mut snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: rec.id.clone(),
            branch: branch.clone(),
            parents: vec![head.clone()],
            manifest: self.write_manifest(&result)?,
            author: self.author(),
            message,
            created_at: Utc::now(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: Some(target.id.clone()),
            signature: None,
        };
        crate::signing::sign_snapshot(self, &mut snapshot)?;
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

    /// Persist `state` and mirror its resolved paths into the conflict
    /// report.
    fn save_merge_state(&self, tree: &str, state: &MergeState) -> Result<()> {
        fsutil::write_json(&self.merge_state_path(tree), state)?;
        let path = self.conflicts_path(tree);
        if let Some(mut report) = fsutil::read_json_opt::<ConflictReport>(&path)? {
            report.resolved = state.resolved.clone();
            fsutil::write_json(&path, &report)?;
        }
        Ok(())
    }

    /// Conflicted text files that still contain conflict markers (paths
    /// resolved with `resolve_conflict` are never reported).
    pub(crate) fn unresolved_conflicts(
        &self,
        rec: &TreeRecord,
        state: &MergeState,
    ) -> Result<Vec<String>> {
        let tree_dir = self.tree_dir_of(rec);
        let mut out = Vec::new();
        for conflict in &state.conflicts {
            if !is_text_conflict(conflict.kind) || state.resolved.contains(&conflict.path) {
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

    /// Manifests and labels of an in-progress merge.
    fn merge_sides(&self, tree: &str, state: &MergeState) -> Result<MergeSides> {
        let branch = self.head_branch(tree)?;
        let ours_id = state.target_tip.clone().unwrap_or_default();
        Ok(MergeSides {
            base: self.manifest_of_tip(state.base.as_deref())?,
            ours: self.manifest_of_tip(state.target_tip.as_deref())?,
            theirs: self.manifest_of_tip(Some(&state.source_tip))?,
            labels: merge_labels(
                &branch,
                &ours_id,
                &state.source_branch,
                &state.source_tip,
                state.base.as_deref(),
            ),
        })
    }

    /// Write the resolution of one conflict into the working tree.
    fn apply_resolution(
        &self,
        rec: &TreeRecord,
        sides: &MergeSides,
        conflict: &ConflictInfo,
        resolution: &ConflictResolution,
        index: &mut super::working::Index,
    ) -> Result<()> {
        let tree_dir = self.tree_dir_of(rec);
        let path = conflict.path.as_str();
        let strategy = match resolution {
            ConflictResolution::Content(data) => {
                validate_manifest_path(path)?;
                fsutil::write_atomic(&tree_dir.join(path), data)?;
                index.remove(path);
                return Ok(());
            }
            ConflictResolution::Ours => MergeStrategy::Ours,
            ConflictResolution::Theirs => MergeStrategy::Theirs,
        };
        let (ours, theirs) = (sides.ours.get(path), sides.theirs.get(path));
        if let (true, Some(o), Some(t)) = (is_text_conflict(conflict.kind), ours, theirs) {
            let base = sides.base.get(path);
            let texts = (
                base.map(|b| self.store.read_entry(b)).transpose()?,
                self.store.read_entry(o)?,
                self.store.read_entry(t)?,
            );
            if let (Ok(base_text), Ok(ours_text), Ok(theirs_text)) = (
                String::from_utf8(texts.0.unwrap_or_default()),
                String::from_utf8(texts.1),
                String::from_utf8(texts.2),
            ) {
                let merged = match merge_text_with(
                    &base_text,
                    &ours_text,
                    &theirs_text,
                    &sides.labels,
                    strategy,
                ) {
                    TextMergeResult::Clean(text)
                    | TextMergeResult::Conflicted { merged: text, .. } => text,
                };
                let abs = tree_dir.join(path);
                fsutil::write_atomic(&abs, merged.as_bytes())?;
                fsutil::set_executable(&abs, merge_executable(base, o, t))?;
                index.remove(path);
                return Ok(());
            }
        }
        let chosen = match strategy {
            MergeStrategy::Theirs => theirs,
            _ => ours,
        };
        match chosen {
            Some(entry) => {
                let cached = self.materialize(&tree_dir, entry)?;
                index.insert(entry.path.clone(), cached);
            }
            None => {
                self.remove_working_file(&tree_dir, path)?;
                index.remove(path);
            }
        }
        Ok(())
    }

    /// Resolve every path of a three-way merge, merging text content and
    /// storing cleanly merged blobs. `strategy` decides conflicts.
    fn three_way(
        &self,
        base: &Manifest,
        ours: &Manifest,
        theirs: &Manifest,
        labels: &MergeLabels,
        strategy: MergeStrategy,
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
                } => match strategy {
                    MergeStrategy::Ours => out.entries.extend(ours),
                    MergeStrategy::Theirs => out.entries.extend(theirs),
                    MergeStrategy::Manual => {
                        if let Some(e) = ours.or(theirs) {
                            out.entries.push(e);
                        }
                        out.conflicts.push(ConflictInfo {
                            path,
                            kind,
                            hunks: Vec::new(),
                        });
                    }
                },
                PathResolution::MergeContent { base, ours, theirs } => {
                    let ours_data = self.store.read_entry(&ours)?;
                    let theirs_data = self.store.read_entry(&theirs)?;
                    let base_data = match &base {
                        Some(b) => self.store.read_entry(b)?,
                        None => Vec::new(),
                    };
                    let binary = [&base_data, &ours_data, &theirs_data]
                        .iter()
                        .any(|d| is_binary(d));
                    let texts = (
                        String::from_utf8(base_data),
                        String::from_utf8(ours_data),
                        String::from_utf8(theirs_data),
                    );
                    let (false, Ok(base_text), Ok(ours_text), Ok(theirs_text)) =
                        (binary, texts.0, texts.1, texts.2)
                    else {
                        match strategy {
                            MergeStrategy::Theirs => out.entries.push(theirs),
                            MergeStrategy::Ours => out.entries.push(ours),
                            MergeStrategy::Manual => {
                                out.entries.push(ours);
                                out.conflicts.push(ConflictInfo {
                                    path,
                                    kind: ConflictKind::Binary,
                                    hunks: Vec::new(),
                                });
                            }
                        }
                        continue;
                    };
                    let executable = merge_executable(base.as_ref(), &ours, &theirs);
                    match merge_text_with(&base_text, &ours_text, &theirs_text, labels, strategy) {
                        TextMergeResult::Clean(text) => {
                            let (hash, chunked) = self.store.put_file(text.as_bytes())?;
                            out.entries.push(ManifestEntry {
                                path,
                                hash,
                                size: text.len() as u64,
                                executable,
                                chunked,
                            });
                        }
                        TextMergeResult::Conflicted { merged, conflicts } => {
                            let kind = if base.is_some() {
                                ConflictKind::Content
                            } else {
                                ConflictKind::AddAdd
                            };
                            out.entries.push(ManifestEntry { executable, ..ours });
                            out.conflicts.push(ConflictInfo {
                                path: path.clone(),
                                kind,
                                hunks: conflicts.iter().map(|c| c.hunk()).collect(),
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

/// Conflict-marker labels with short ids: `ours (main 1a2b3c4d)`,
/// `base (0f0f0f0f)`, `theirs (feature 5e6f7a8b)`.
fn merge_labels(
    branch: &str,
    ours: &str,
    source: &str,
    theirs: &str,
    base: Option<&str>,
) -> MergeLabels {
    let side = |name: &str, label: &str, id: &str| {
        if id.is_empty() {
            format!("{name} ({label})")
        } else if !label.is_empty() && id.starts_with(label) {
            // The source was given as an id prefix: show it once.
            format!("{name} ({})", short(id))
        } else {
            format!("{name} ({label} {})", short(id))
        }
    };
    MergeLabels {
        ours: side("ours", branch, ours),
        base: base.map_or_else(|| "base".to_string(), |b| format!("base ({})", short(b))),
        theirs: side("theirs", source, theirs),
    }
}

/// The parent a revert undoes changes against: `(1-based index, id)`, or
/// `None` for a root snapshot.
fn revert_parent(target: &Snapshot, parent: Option<usize>) -> Result<Option<(usize, &str)>> {
    let count = target.parents.len();
    match parent {
        None if count > 1 => Err(SdkError::InvalidArgument(format!(
            "snapshot {} is a merge with {count} parents; pass --parent N (1..={count}) to choose \
             the parent whose line of history is kept",
            target.short_id()
        ))),
        None => Ok(target.parents.first().map(|p| (1, p.as_str()))),
        Some(_) if count <= 1 => Err(SdkError::InvalidArgument(format!(
            "--parent was given but snapshot {} is not a merge",
            target.short_id()
        ))),
        Some(n) if n == 0 || n > count => Err(SdkError::InvalidArgument(format!(
            "snapshot {} has no parent {n} (it has {count})",
            target.short_id()
        ))),
        Some(n) => Ok(Some((n, target.parents[n - 1].as_str()))),
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

/// Conflicts whose working copy carries diff3 markers.
fn is_text_conflict(kind: ConflictKind) -> bool {
    matches!(kind, ConflictKind::Content | ConflictKind::AddAdd)
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
                    chunked: false,
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

    #[test]
    fn labels_use_short_ids() {
        let labels = merge_labels(
            "main",
            "1a2b3c4d-0000",
            "feature",
            "5e6f7a8b-1111",
            Some("0f0f0f0f-2222"),
        );
        assert_eq!(labels.ours, "ours (main 1a2b3c4d)");
        assert_eq!(labels.theirs, "theirs (feature 5e6f7a8b)");
        assert_eq!(labels.base, "base (0f0f0f0f)");
        let by_id = merge_labels("main", "1a2b3c4d-0000", "5e6f", "5e6f7a8b-1111", None);
        assert_eq!(by_id.theirs, "theirs (5e6f7a8b)");
        assert_eq!(by_id.base, "base");
    }

    fn snap(parents: &[&str]) -> Snapshot {
        Snapshot {
            id: "abcdef0123".into(),
            tree_id: String::new(),
            branch: "main".into(),
            parents: parents.iter().map(|p| (*p).to_string()).collect(),
            manifest: hash_bytes(b""),
            author: String::new(),
            message: String::new(),
            created_at: Utc::now(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        }
    }

    #[test]
    fn revert_parent_rules() {
        assert_eq!(revert_parent(&snap(&["p"]), None).unwrap(), Some((1, "p")));
        assert_eq!(revert_parent(&snap(&[]), None).unwrap(), None);
        assert!(revert_parent(&snap(&["p"]), Some(1)).is_err());
        let merge = snap(&["p1", "p2"]);
        let err = revert_parent(&merge, None).unwrap_err().to_string();
        assert!(err.contains("--parent"), "{err}");
        assert_eq!(revert_parent(&merge, Some(2)).unwrap(), Some((2, "p2")));
        assert!(revert_parent(&merge, Some(0)).is_err());
        assert!(revert_parent(&merge, Some(3)).is_err());
    }
}
