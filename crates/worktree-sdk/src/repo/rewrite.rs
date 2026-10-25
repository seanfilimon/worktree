//! History rewriting: cherry-pick and rebase (ENGINEERING_PLAN §14.5).
//!
//! Both operations replay snapshots one at a time: each snapshot's change
//! relative to its first parent is applied with the three-way merge engine
//! (`base` = the snapshot's first parent, `ours` = the progress so far,
//! `theirs` = the snapshot). A clean step becomes a new snapshot (new id,
//! the original's author and message, `reverts` preserved, re-signed when
//! signing is configured); a step that produces no change is skipped.
//!
//! A conflicting step stops the operation exactly like a conflicted merge:
//! conflict markers in the working tree, `MERGE_STATE.json` and
//! `.wt/conflicts/<tree>.json` (so `wt merge status` and `wt merge resolve`
//! work unchanged), plus `REWRITE_STATE.json` with the remaining steps.
//! While it exists, `wt merge --continue|--abort` and `wt snapshot` refuse
//! and point at `wt cherry-pick|rebase --continue|--abort|--skip`.
//!
//! The branch moves as steps complete; `--abort` moves it back and
//! restores the working tree to the state before the operation began.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::merging::{merge_labels, ThreeWay};
use super::snapshotting::compute_changes;
use super::trees::TreeRecord;
use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::*;

/// Which history-rewriting operation a [`RewriteState`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RewriteOp {
    CherryPick,
    Rebase,
}

impl RewriteOp {
    /// The CLI command name (`cherry-pick` / `rebase`).
    pub fn as_str(self) -> &'static str {
        match self {
            RewriteOp::CherryPick => "cherry-pick",
            RewriteOp::Rebase => "rebase",
        }
    }
}

/// Options of [`Repository::cherry_pick`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CherryPickOptions {
    /// Apply the changes to the working tree only; create no snapshots and
    /// leave the branch where it is (`--no-snapshot`).
    pub no_snapshot: bool,
}

/// Options of [`Repository::rebase`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebaseOptions {
    /// Rewrite snapshots even when a remote-tracking ref of the branch
    /// already contains them (`--force`).
    pub force: bool,
}

/// One replayed snapshot: its original id and the id of the snapshot that
/// replaced it (`None` with `--no-snapshot`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewrittenSnapshot {
    pub original: SnapshotId,
    pub new: Option<SnapshotId>,
}

/// How a cherry-pick / rebase call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStatus {
    /// Every step was applied (or skipped).
    Completed,
    /// A step conflicted; resolve it and continue (or abort / skip).
    Conflicts,
    /// Rebase: the branch already contains ONTO.
    UpToDate,
    /// Rebase: the branch had no own snapshots and was fast-forwarded.
    FastForward,
}

/// Result of a cherry-pick / rebase call (`--json` output of the CLI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteOutcome {
    pub op: RewriteOp,
    pub tree: String,
    pub branch: String,
    pub status: RewriteStatus,
    /// Branch tip before the operation started.
    pub old_tip: Option<SnapshotId>,
    /// Branch tip now.
    pub new_tip: Option<SnapshotId>,
    /// Rebase target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onto: Option<SnapshotId>,
    /// Steps applied so far, in order (across `--continue` calls).
    pub picked: Vec<RewrittenSnapshot>,
    /// Steps whose changes were already present (or were skipped).
    pub skipped: Vec<SnapshotId>,
    /// The conflicting step when `status` is `Conflicts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<SnapshotId>,
    pub conflicts: Vec<ConflictInfo>,
    /// Steps still to apply after the stopped one.
    pub remaining: usize,
}

/// `.wt/trees/<tree>/REWRITE_STATE.json`: a cherry-pick or rebase stopped
/// at a conflicting step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteState {
    pub op: RewriteOp,
    pub branch: String,
    /// Branch tip before the operation (restored by `--abort`).
    pub orig_tip: Option<SnapshotId>,
    /// Rebase target.
    #[serde(default)]
    pub onto: Option<SnapshotId>,
    /// The snapshot whose step conflicted.
    pub current: Option<SnapshotId>,
    /// Snapshots still to replay after `current`, in order.
    pub todo: Vec<SnapshotId>,
    pub picked: Vec<RewrittenSnapshot>,
    pub skipped: Vec<SnapshotId>,
    /// `wt cherry-pick --no-snapshot`.
    #[serde(default)]
    pub no_snapshot: bool,
    pub started_at: DateTime<Utc>,
}

/// Where the replay stands: the snapshot new steps are parented on and the
/// manifest the next step merges into ("ours").
struct Progress {
    tip: Option<SnapshotId>,
    manifest: Manifest,
    /// `manifest` differs from `tip`'s (applied `--no-snapshot` steps).
    synthetic: bool,
}

/// A conflicting step, ready to be written out.
struct Stop {
    original: Snapshot,
    ours_id: Option<SnapshotId>,
    result: ThreeWay,
}

impl Repository {
    // ----------------------------------------------------------------
    // Public API
    // ----------------------------------------------------------------

    /// Apply the changes of `revs` (each relative to its first parent), in
    /// order, onto the current branch (`wt cherry-pick REV...`). Each clean
    /// step creates a snapshot with the original's author and message;
    /// steps whose changes are already present are skipped. Requires a
    /// clean working tree. A conflict stops with status `Conflicts` (see
    /// the module docs); continue with [`Repository::cherry_pick_continue`].
    pub fn cherry_pick(
        &self,
        tree: &str,
        revs: &[String],
        opts: &CherryPickOptions,
    ) -> Result<RewriteOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_idle(tree)?;
        if revs.is_empty() {
            return Err(SdkError::InvalidArgument(
                "name at least one snapshot to cherry-pick".into(),
            ));
        }
        let todo = revs
            .iter()
            .map(|rev| self.resolve(tree, rev).map(|s| s.id))
            .collect::<Result<Vec<_>>>()?;
        let (branch, head) = self.head_tip(tree)?;
        let head_manifest = self.require_clean(&rec, head.as_deref())?;
        let state = RewriteState {
            op: RewriteOp::CherryPick,
            branch,
            orig_tip: head.clone(),
            onto: None,
            current: None,
            todo,
            picked: Vec::new(),
            skipped: Vec::new(),
            no_snapshot: opts.no_snapshot,
            started_at: Utc::now(),
        };
        let progress = Progress {
            tip: head,
            manifest: head_manifest.clone(),
            synthetic: false,
        };
        self.replay(&rec, state, progress, &head_manifest)
    }

    /// Replay the current branch's first-parent snapshots since it forked
    /// from `onto` on top of `onto` and move the branch to the result
    /// (`wt rebase ONTO`). Up to date when the branch already contains
    /// `onto`; a fast-forward when the branch has no snapshots of its own.
    /// Refuses (unless `force`) when a remote-tracking ref of the branch
    /// already contains snapshots that would be rewritten. Requires a clean
    /// working tree; conflicts stop as in [`Repository::cherry_pick`].
    pub fn rebase(&self, tree: &str, onto: &str, opts: &RebaseOptions) -> Result<RewriteOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_idle(tree)?;
        let (branch, head) = self.head_tip(tree)?;
        let tip = head.clone().ok_or_else(|| {
            SdkError::InvalidRevision(format!("{branch} has no snapshots to rebase"))
        })?;
        let onto_snap = self.resolve(tree, onto)?;
        let head_manifest = self.require_clean(&rec, Some(&tip))?;
        let mut outcome = RewriteOutcome {
            op: RewriteOp::Rebase,
            tree: tree.to_string(),
            branch: branch.clone(),
            status: RewriteStatus::UpToDate,
            old_tip: head.clone(),
            new_tip: head.clone(),
            onto: Some(onto_snap.id.clone()),
            picked: Vec::new(),
            skipped: Vec::new(),
            stopped_at: None,
            conflicts: Vec::new(),
            remaining: 0,
        };
        if self.is_ancestor(&onto_snap.id, &tip)? {
            return Ok(outcome);
        }
        if self.is_ancestor(&tip, &onto_snap.id)? {
            let target = self.manifest_of(&onto_snap)?;
            self.sync_working(&rec, &head_manifest, &target, &[])?;
            self.write_tip(tree, &branch, Some(&onto_snap.id))?;
            self.rewrite_reflog(
                tree,
                &branch,
                RewriteOp::Rebase,
                head,
                Some(onto_snap.id.clone()),
                format!("rebase: fast-forward to {}", onto_snap.short_id()),
            )?;
            outcome.status = RewriteStatus::FastForward;
            outcome.new_tip = Some(onto_snap.id);
            return Ok(outcome);
        }

        let todo = self.rebase_todo(&tip, &onto_snap.id)?;
        if !opts.force {
            self.check_not_pushed(tree, &branch, &todo)?;
        }
        let state = RewriteState {
            op: RewriteOp::Rebase,
            branch,
            orig_tip: head,
            onto: Some(onto_snap.id.clone()),
            current: None,
            todo,
            picked: Vec::new(),
            skipped: Vec::new(),
            no_snapshot: false,
            started_at: Utc::now(),
        };
        let progress = Progress {
            tip: Some(onto_snap.id.clone()),
            manifest: self.manifest_of(&onto_snap)?,
            synthetic: false,
        };
        self.replay(&rec, state, progress, &head_manifest)
    }

    /// Finish the stopped cherry-pick step (conflict markers must be gone;
    /// conflicts can be resolved with `wt merge resolve`) and apply the
    /// remaining snapshots (`wt cherry-pick --continue`).
    pub fn cherry_pick_continue(&self, tree: &str) -> Result<RewriteOutcome> {
        self.continue_rewrite(tree, RewriteOp::CherryPick)
    }

    /// Drop the stopped cherry-pick step and apply the remaining snapshots.
    pub fn cherry_pick_skip(&self, tree: &str) -> Result<RewriteOutcome> {
        self.skip_rewrite(tree, RewriteOp::CherryPick)
    }

    /// Abandon the cherry-pick: the branch and the working tree return to
    /// their state before `wt cherry-pick` (`--abort`).
    pub fn cherry_pick_abort(&self, tree: &str) -> Result<()> {
        self.abort_rewrite(tree, RewriteOp::CherryPick)
    }

    /// Finish the stopped rebase step and replay the rest (`wt rebase --continue`).
    pub fn rebase_continue(&self, tree: &str) -> Result<RewriteOutcome> {
        self.continue_rewrite(tree, RewriteOp::Rebase)
    }

    /// Drop the stopped rebase step and replay the rest (`wt rebase --skip`).
    pub fn rebase_skip(&self, tree: &str) -> Result<RewriteOutcome> {
        self.skip_rewrite(tree, RewriteOp::Rebase)
    }

    /// Abandon the rebase: the branch and the working tree return to their
    /// state before `wt rebase` (`--abort`).
    pub fn rebase_abort(&self, tree: &str) -> Result<()> {
        self.abort_rewrite(tree, RewriteOp::Rebase)
    }

    /// The stopped cherry-pick or rebase of `tree`, if any.
    pub fn rewrite_state(&self, tree: &str) -> Result<Option<RewriteState>> {
        self.load_tree(tree)?;
        fsutil::read_json_opt(&self.rewrite_state_path(tree))
    }

    /// Snapshots a stopped cherry-pick or rebase still needs (original tip,
    /// the stopped step and the remaining steps) in every tree, for local
    /// garbage collection.
    pub fn rewrite_roots(&self) -> Result<Vec<SnapshotId>> {
        let mut out = Vec::new();
        for rec in self.tree_records()? {
            if let Some(state) =
                fsutil::read_json_opt::<RewriteState>(&self.rewrite_state_path(&rec.name))?
            {
                out.extend(state.orig_tip);
                out.extend(state.onto);
                out.extend(state.current);
                out.extend(state.todo);
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    // ----------------------------------------------------------------
    // Continue / skip / abort
    // ----------------------------------------------------------------

    fn continue_rewrite(&self, tree: &str, op: RewriteOp) -> Result<RewriteOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let mut state = self.expect_rewrite(tree, op)?;
        let merge = self.merge_state(tree)?.ok_or_else(|| {
            SdkError::Other(format!(
                "the {} has no conflict to continue from; run `wt {} --abort`",
                op.as_str(),
                op.as_str()
            ))
        })?;
        let unresolved = self.unresolved_conflicts(&rec, &merge)?;
        if !unresolved.is_empty() {
            return Err(SdkError::MergeConflict(format!(
                "conflict markers remain in: {}",
                unresolved.join(", ")
            )));
        }
        let current = state
            .current
            .take()
            .ok_or_else(|| SdkError::Other(format!("the {} has no stopped step", op.as_str())))?;
        let original = self.get_snapshot(&current)?;
        let tip = self.tip_or_none(tree, &state.branch)?;
        let ours = self.manifest_of_tip(merge.target_tip.as_deref())?;
        let working = self.build_working(&rec, &ours, true, true)?;
        if !working.findings.is_empty() {
            self.save_index(tree, &working.index)?;
            return Err(SdkError::SecretsDetected(format!(
                "{} finding(s) in the resolved files: {}",
                working.findings.len(),
                working
                    .findings
                    .iter()
                    .map(|f| format!("{}:{}", f.path, f.line))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        self.save_index(tree, &working.index)?;
        let resolved = working.manifest;
        let head_manifest = self.manifest_of_tip(tip.as_deref())?;
        let mut progress = Progress {
            tip: tip.clone(),
            manifest: resolved.clone(),
            synthetic: false,
        };
        let mut created = Vec::new();
        if resolved == ours {
            state.skipped.push(current);
            progress.synthetic = resolved != head_manifest;
        } else if state.no_snapshot {
            state.picked.push(RewrittenSnapshot {
                original: current,
                new: None,
            });
            progress.synthetic = resolved != head_manifest;
        } else {
            let mapping = mapping_of(&state.picked);
            let snap = self.rewritten_snapshot(
                &rec,
                &state,
                tip.as_deref(),
                &resolved,
                &original,
                &mapping,
            )?;
            created.push((
                tip.clone(),
                snap.id.clone(),
                original.short_id().to_string(),
            ));
            state.picked.push(RewrittenSnapshot {
                original: current,
                new: Some(snap.id.clone()),
            });
            progress.tip = Some(snap.id);
        }
        self.clear_merge_state(tree)?;
        self.replay_from(&rec, state, progress, &resolved, created)
    }

    fn skip_rewrite(&self, tree: &str, op: RewriteOp) -> Result<RewriteOutcome> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let mut state = self.expect_rewrite(tree, op)?;
        let merge = self.merge_state(tree)?;
        if let Some(current) = state.current.take() {
            state.skipped.push(current);
        }
        let tip = self.tip_or_none(tree, &state.branch)?;
        let head_manifest = self.manifest_of_tip(tip.as_deref())?;
        let ours = match &merge {
            Some(m) => self.manifest_of_tip(m.target_tip.as_deref())?,
            None => head_manifest.clone(),
        };
        // Put the working tree back to "ours", dropping the step's changes.
        let mut sides = vec![ours.clone(), head_manifest.clone()];
        if let Some(m) = &merge {
            sides.push(self.manifest_of_tip(Some(&m.source_tip))?);
        }
        self.reset_working(&rec, &ours, &sides)?;
        self.clear_merge_state(tree)?;
        let progress = Progress {
            tip,
            synthetic: ours != head_manifest,
            manifest: ours.clone(),
        };
        self.replay_from(&rec, state, progress, &ours, Vec::new())
    }

    fn abort_rewrite(&self, tree: &str, op: RewriteOp) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let state = self.expect_rewrite(tree, op)?;
        let merge = self.merge_state(tree)?;
        let tip = self.tip_or_none(tree, &state.branch)?;
        let orig = self.manifest_of_tip(state.orig_tip.as_deref())?;
        let mut sides = vec![self.manifest_of_tip(tip.as_deref())?];
        if let Some(m) = &merge {
            sides.push(self.manifest_of_tip(m.target_tip.as_deref())?);
            sides.push(self.manifest_of_tip(Some(&m.source_tip))?);
        }
        self.reset_working(&rec, &orig, &sides)?;
        if tip != state.orig_tip {
            self.write_tip(tree, &state.branch, state.orig_tip.as_deref())?;
            self.rewrite_reflog(
                tree,
                &state.branch,
                op,
                tip,
                state.orig_tip.clone(),
                format!("{}: aborted", op.as_str()),
            )?;
        }
        if self.head_branch(tree)? != state.branch {
            self.write_head(tree, &state.branch)?;
        }
        self.clear_merge_state(tree)?;
        fsutil::remove_file_if_exists(&self.rewrite_state_path(tree))
    }

    // ----------------------------------------------------------------
    // Replay engine
    // ----------------------------------------------------------------

    fn replay(
        &self,
        rec: &TreeRecord,
        state: RewriteState,
        progress: Progress,
        working_base: &Manifest,
    ) -> Result<RewriteOutcome> {
        self.replay_from(rec, state, progress, working_base, Vec::new())
    }

    /// Apply `state.todo` on top of `progress`. `working_base` is the
    /// manifest the working directory currently matches; `created` lists
    /// snapshots already created during this call as (old tip, new id,
    /// original short id) for the reflog.
    fn replay_from(
        &self,
        rec: &TreeRecord,
        mut state: RewriteState,
        mut progress: Progress,
        working_base: &Manifest,
        mut created: Vec<(Option<SnapshotId>, SnapshotId, String)>,
    ) -> Result<RewriteOutcome> {
        let tree = rec.name.as_str();
        let call_start_tip = self.tip_or_none(tree, &state.branch)?;
        let mut mapping = mapping_of(&state.picked);
        while !state.todo.is_empty() {
            let id = state.todo.remove(0);
            let original = self.get_snapshot(&id)?;
            let parent = original.parents.first().cloned();
            let base = self.manifest_of_tip(parent.as_deref())?;
            let theirs = self.manifest_of(&original)?;
            let mut ours_id = progress.tip.clone();
            let mut result = self.step_merge(
                &state.branch,
                ours_id.as_deref(),
                &original,
                &base,
                &progress.manifest,
                &theirs,
            )?;
            if !result.conflicts.is_empty() {
                if progress.synthetic {
                    // `--no-snapshot`: record the accumulated working state
                    // as an (unreferenced) snapshot so `wt merge resolve
                    // --ours` sees the right side.
                    let inter = self.intermediate_snapshot(
                        rec,
                        &state.branch,
                        progress.tip.as_deref(),
                        &progress.manifest,
                        &original,
                    )?;
                    ours_id = Some(inter.id);
                    result = self.step_merge(
                        &state.branch,
                        ours_id.as_deref(),
                        &original,
                        &base,
                        &progress.manifest,
                        &theirs,
                    )?;
                }
                state.current = Some(id);
                let stop = Stop {
                    original,
                    ours_id,
                    result,
                };
                return self.stop(
                    rec,
                    state,
                    progress,
                    stop,
                    working_base,
                    call_start_tip,
                    created,
                );
            }
            let merged = Manifest::from_entries(result.entries);
            if merged == progress.manifest {
                state.skipped.push(id);
                continue;
            }
            if state.no_snapshot {
                progress.manifest = merged;
                progress.synthetic = true;
                state.picked.push(RewrittenSnapshot {
                    original: id,
                    new: None,
                });
                continue;
            }
            let snap = self.rewritten_snapshot(
                rec,
                &state,
                progress.tip.as_deref(),
                &merged,
                &original,
                &mapping,
            )?;
            mapping.insert(id.clone(), snap.id.clone());
            created.push((
                progress.tip.clone(),
                snap.id.clone(),
                original.short_id().to_string(),
            ));
            state.picked.push(RewrittenSnapshot {
                original: id,
                new: Some(snap.id.clone()),
            });
            progress.tip = Some(snap.id);
            progress.manifest = merged;
        }

        // Completed.
        self.sync_working(rec, working_base, &progress.manifest, &[])?;
        if !state.no_snapshot {
            self.write_tip(tree, &state.branch, progress.tip.as_deref())?;
        }
        let message = match state.op {
            RewriteOp::Rebase => format!(
                "rebase onto {}: finished ({} replayed, {} skipped)",
                short(state.onto.as_deref().unwrap_or_default()),
                state.picked.len(),
                state.skipped.len()
            ),
            RewriteOp::CherryPick => String::new(),
        };
        self.log_moves(tree, &state, call_start_tip, &progress, &created, message)?;
        fsutil::remove_file_if_exists(&self.rewrite_state_path(tree))?;
        let new_tip = self.tip_or_none(tree, &state.branch)?;
        Ok(outcome(
            tree,
            state,
            RewriteStatus::Completed,
            new_tip,
            None,
            Vec::new(),
        ))
    }

    /// Persist a conflicting step: working tree with markers, merge state,
    /// rewrite state, branch at the progress tip.
    #[allow(clippy::too_many_arguments)]
    fn stop(
        &self,
        rec: &TreeRecord,
        state: RewriteState,
        progress: Progress,
        stop: Stop,
        working_base: &Manifest,
        call_start_tip: Option<SnapshotId>,
        created: Vec<(Option<SnapshotId>, SnapshotId, String)>,
    ) -> Result<RewriteOutcome> {
        let tree = rec.name.as_str();
        let Stop {
            original,
            ours_id,
            result,
        } = stop;
        let merged = Manifest::from_entries(result.entries);
        self.sync_working(rec, working_base, &merged, &result.marker_files)?;
        if !state.no_snapshot {
            self.write_tip(tree, &state.branch, progress.tip.as_deref())?;
        }
        let merge = MergeState {
            source_branch: original.short_id().to_string(),
            source_tip: original.id.clone(),
            target_tip: ours_id,
            base: original.parents.first().cloned(),
            conflicts: result.conflicts.clone(),
            message: original.message.clone(),
            resolved: Vec::new(),
        };
        self.write_conflict_state(tree, &state.branch, &merge)?;
        fsutil::write_json(&self.rewrite_state_path(tree), &state)?;
        let message = format!(
            "{}{}: stopped at {} (conflicts)",
            state.op.as_str(),
            match (state.op, &state.onto) {
                (RewriteOp::Rebase, Some(onto)) => format!(" onto {}", short(onto)),
                _ => String::new(),
            },
            original.short_id()
        );
        self.log_moves(tree, &state, call_start_tip, &progress, &created, message)?;
        let new_tip = self.tip_or_none(tree, &state.branch)?;
        Ok(outcome(
            tree,
            state,
            RewriteStatus::Conflicts,
            new_tip,
            Some(original.id),
            result.conflicts,
        ))
    }

    /// The three-way merge of one step.
    fn step_merge(
        &self,
        branch: &str,
        ours_id: Option<&str>,
        original: &Snapshot,
        base: &Manifest,
        ours: &Manifest,
        theirs: &Manifest,
    ) -> Result<ThreeWay> {
        let labels = merge_labels(
            branch,
            ours_id.unwrap_or_default(),
            original.short_id(),
            &original.id,
            original.parents.first().map(String::as_str),
        );
        self.three_way(base, ours, theirs, &labels, MergeStrategy::Manual)
    }

    /// Store the snapshot that replaces `original` in the rewritten history.
    fn rewritten_snapshot(
        &self,
        rec: &TreeRecord,
        state: &RewriteState,
        parent: Option<&str>,
        manifest: &Manifest,
        original: &Snapshot,
        mapping: &HashMap<SnapshotId, SnapshotId>,
    ) -> Result<Snapshot> {
        let reverts = original
            .reverts
            .as_ref()
            .map(|r| mapping.get(r).cloned().unwrap_or_else(|| r.clone()));
        // Dependencies become TODOs on push: a rebase moves them with the
        // snapshot, a cherry-pick (a copy) does not duplicate them.
        let dependencies = match state.op {
            RewriteOp::Rebase => original.dependencies.clone(),
            RewriteOp::CherryPick => Vec::new(),
        };
        let mut snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: rec.id.clone(),
            branch: state.branch.clone(),
            parents: parent.map(str::to_string).into_iter().collect(),
            manifest: self.write_manifest(manifest)?,
            author: original.author.clone(),
            message: original.message.clone(),
            created_at: Utc::now(),
            auto_generated: original.auto_generated,
            dependencies,
            reverts,
            signature: None,
        };
        crate::signing::sign_snapshot(self, &mut snapshot)?;
        self.write_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// An unreferenced snapshot of accumulated `--no-snapshot` changes.
    fn intermediate_snapshot(
        &self,
        rec: &TreeRecord,
        branch: &str,
        parent: Option<&str>,
        manifest: &Manifest,
        next: &Snapshot,
    ) -> Result<Snapshot> {
        let snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: rec.id.clone(),
            branch: branch.to_string(),
            parents: parent.map(str::to_string).into_iter().collect(),
            manifest: self.write_manifest(manifest)?,
            author: self.author(),
            message: format!(
                "cherry-pick --no-snapshot: working state before {}",
                next.short_id()
            ),
            created_at: Utc::now(),
            auto_generated: true,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        };
        self.write_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Reflog entries for the branch moves of one call: one `cherry-pick`
    /// entry per created snapshot, or one `rebase` entry for the call.
    fn log_moves(
        &self,
        tree: &str,
        state: &RewriteState,
        call_start_tip: Option<SnapshotId>,
        progress: &Progress,
        created: &[(Option<SnapshotId>, SnapshotId, String)],
        message: String,
    ) -> Result<()> {
        if state.no_snapshot {
            return Ok(());
        }
        match state.op {
            RewriteOp::CherryPick => {
                for (old, new, original) in created {
                    let subject = self
                        .get_snapshot(new)?
                        .message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    self.rewrite_reflog(
                        tree,
                        &state.branch,
                        RewriteOp::CherryPick,
                        old.clone(),
                        Some(new.clone()),
                        format!("cherry-pick {original}: {subject}"),
                    )?;
                }
                Ok(())
            }
            RewriteOp::Rebase if call_start_tip != progress.tip => self.rewrite_reflog(
                tree,
                &state.branch,
                RewriteOp::Rebase,
                call_start_tip,
                progress.tip.clone(),
                message,
            ),
            RewriteOp::Rebase => Ok(()),
        }
    }

    // ----------------------------------------------------------------
    // Helpers
    // ----------------------------------------------------------------

    fn rewrite_state_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("REWRITE_STATE.json")
    }

    /// `Other` when a cherry-pick or rebase is stopped in `tree` (used by
    /// `snapshot`, `continue_merge` and `abort_merge`).
    pub(crate) fn ensure_no_rewrite(&self, tree: &str) -> Result<()> {
        match fsutil::read_json_opt::<RewriteState>(&self.rewrite_state_path(tree))? {
            Some(state) => Err(rewrite_in_progress(state.op)),
            None => Ok(()),
        }
    }

    /// No merge, cherry-pick or rebase may be in progress.
    fn ensure_idle(&self, tree: &str) -> Result<()> {
        self.ensure_no_rewrite(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        Ok(())
    }

    fn expect_rewrite(&self, tree: &str, op: RewriteOp) -> Result<RewriteState> {
        match fsutil::read_json_opt::<RewriteState>(&self.rewrite_state_path(tree))? {
            Some(state) if state.op == op => Ok(state),
            Some(state) => Err(rewrite_in_progress(state.op)),
            None => Err(SdkError::Other(format!("no {} in progress", op.as_str()))),
        }
    }

    /// The head manifest, or `DirtyWorkingTree` if the working tree differs.
    fn require_clean(&self, rec: &TreeRecord, head: Option<&str>) -> Result<Manifest> {
        let head_manifest = self.manifest_of_tip(head)?;
        let working = self.build_working(rec, &head_manifest, false, false)?;
        let changes = compute_changes(&head_manifest, &working.manifest);
        if !changes.is_empty() {
            return Err(SdkError::DirtyWorkingTree(changes.len()));
        }
        Ok(head_manifest)
    }

    /// Make the working directory (currently matching `base`) match
    /// `target`, then write `markers` over the conflicted files.
    fn sync_working(
        &self,
        rec: &TreeRecord,
        base: &Manifest,
        target: &Manifest,
        markers: &[(String, Vec<u8>)],
    ) -> Result<()> {
        let mut working = self.build_working(rec, base, false, false)?;
        let tracked: HashSet<String> = base.entries.iter().map(|e| e.path.clone()).collect();
        self.apply_manifest(rec, target, &tracked, &working.manifest, &mut working.index)?;
        let tree_dir = self.tree_dir_of(rec);
        for (path, content) in markers {
            fsutil::write_atomic(&tree_dir.join(path), content)?;
            working.index.remove(path);
        }
        self.save_index(&rec.name, &working.index)
    }

    /// Reset the working directory to `target`; files of any of `sides`
    /// count as tracked (removed when `target` lacks them), files the user
    /// created meanwhile are kept.
    fn reset_working(&self, rec: &TreeRecord, target: &Manifest, sides: &[Manifest]) -> Result<()> {
        let tracked: HashSet<String> = sides
            .iter()
            .chain(std::iter::once(target))
            .flat_map(|m| m.entries.iter().map(|e| e.path.clone()))
            .collect();
        let mut working = self.build_working(rec, target, false, false)?;
        self.apply_manifest(rec, target, &tracked, &working.manifest, &mut working.index)?;
        self.save_index(&rec.name, &working.index)
    }

    /// First-parent snapshots of `tip` not reachable from `onto`, oldest first.
    fn rebase_todo(&self, tip: &str, onto: &str) -> Result<Vec<SnapshotId>> {
        let onto_ancestry: HashSet<SnapshotId> =
            self.ancestry(onto)?.into_iter().map(|s| s.id).collect();
        let mut todo = Vec::new();
        let mut cursor = Some(tip.to_string());
        while let Some(id) = cursor {
            if onto_ancestry.contains(&id) {
                break;
            }
            let snap = self.load_snapshot_opt(&id)?.ok_or_else(|| {
                SdkError::InvalidArgument(format!(
                    "cannot rebase: snapshot {} is not stored locally (shallow history); \
                     run `wt fetch --deepen N` and try again",
                    short(&id)
                ))
            })?;
            cursor = snap.parents.first().cloned();
            todo.push(snap.id);
        }
        todo.reverse();
        Ok(todo)
    }

    /// Refuse to rewrite snapshots a remote-tracking ref of `branch` contains.
    fn check_not_pushed(&self, tree: &str, branch: &str, todo: &[SnapshotId]) -> Result<()> {
        let mut remotes: Vec<String> = self.config().remote.keys().cloned().collect();
        if !remotes.iter().any(|r| r == "origin") {
            remotes.push("origin".into());
        }
        for remote in remotes {
            let Ok(Some(remote_tip)) = self.remote_tip(tree, &remote, branch) else {
                continue;
            };
            if !self.has_snapshot(&remote_tip) {
                continue;
            }
            let pushed: HashSet<SnapshotId> = self
                .ancestry(&remote_tip)?
                .into_iter()
                .map(|s| s.id)
                .collect();
            let count = todo.iter().filter(|id| pushed.contains(*id)).count();
            if count > 0 {
                return Err(SdkError::Other(format!(
                    "refusing to rebase: {count} of the {} snapshot(s) to rewrite are already on \
                     {remote}/{branch}; rewriting pushed history makes the branch diverge from the \
                     server (pass --force to rewrite anyway)",
                    todo.len()
                )));
            }
        }
        Ok(())
    }

    fn rewrite_reflog(
        &self,
        tree: &str,
        branch: &str,
        op: RewriteOp,
        old: Option<SnapshotId>,
        new: Option<SnapshotId>,
        message: String,
    ) -> Result<()> {
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: branch.to_string(),
            action: op.as_str().into(),
            old,
            new,
            message,
        })
    }
}

fn rewrite_in_progress(op: RewriteOp) -> SdkError {
    let name = op.as_str();
    SdkError::Other(format!(
        "a {name} is in progress; resolve the conflicts (`wt merge resolve`), then run \
         `wt {name} --continue` (or `wt {name} --skip` / `wt {name} --abort`)"
    ))
}

fn mapping_of(picked: &[RewrittenSnapshot]) -> HashMap<SnapshotId, SnapshotId> {
    picked
        .iter()
        .filter_map(|p| p.new.clone().map(|n| (p.original.clone(), n)))
        .collect()
}

fn outcome(
    tree: &str,
    state: RewriteState,
    status: RewriteStatus,
    new_tip: Option<SnapshotId>,
    stopped_at: Option<SnapshotId>,
    conflicts: Vec<ConflictInfo>,
) -> RewriteOutcome {
    RewriteOutcome {
        op: state.op,
        tree: tree.to_string(),
        branch: state.branch,
        status,
        old_tip: state.orig_tip,
        new_tip,
        onto: state.onto,
        picked: state.picked,
        skipped: state.skipped,
        stopped_at,
        conflicts,
        remaining: state.todo.len(),
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_names_and_serde() {
        assert_eq!(RewriteOp::CherryPick.as_str(), "cherry-pick");
        assert_eq!(
            serde_json::to_string(&RewriteOp::CherryPick).unwrap(),
            "\"cherry-pick\""
        );
        assert_eq!(
            serde_json::to_string(&RewriteStatus::FastForward).unwrap(),
            "\"fast_forward\""
        );
    }

    #[test]
    fn mapping_skips_unsnapshotted_steps() {
        let picked = vec![
            RewrittenSnapshot {
                original: "a".into(),
                new: Some("b".into()),
            },
            RewrittenSnapshot {
                original: "c".into(),
                new: None,
            },
        ];
        let map = mapping_of(&picked);
        assert_eq!(map.len(), 1);
        assert_eq!(map["a"], "b");
    }

    #[test]
    fn in_progress_error_names_the_commands() {
        let msg = rewrite_in_progress(RewriteOp::Rebase).to_string();
        assert!(msg.contains("wt rebase --continue"), "{msg}");
        assert!(msg.contains("wt merge resolve"), "{msg}");
    }
}
