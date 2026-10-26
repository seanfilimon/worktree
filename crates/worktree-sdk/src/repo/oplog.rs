//! The local operation log `.wt/oplog.jsonl` and `wt undo` / `wt redo`
//! (ENGINEERING_PLAN §16.10, `specs/tree/Undo.md`).
//!
//! Every local mutation runs under the per-tree lock (`lock_tree`). The lock
//! guard ([`OpGuard`]) captures the tree's refs when it is taken — `HEAD`,
//! every branch (tip and archived flag), every tag and the stash — and
//! compares them when it is released: when anything changed it appends one
//! [`OpLogEntry`] with the before/after value of each changed ref. Snapshot,
//! merge, revert, rebase, cherry-pick, reset (branch moves), switch, branch
//! create/archive/restore, tags and stash push/pop/drop are all recorded
//! this way without per-operation code. The entry is named after the
//! reflog entries written while the lock was held.
//!
//! `undo` restores the `old` values of the newest operations of a tree and
//! checks the restored head out; `redo` re-applies undone operations.
//! Both refuse while a merge, cherry-pick or rebase is in progress and,
//! unless forced, when the working tree has uncommitted changes (with
//! `force` those changes are discarded). Undoing a stash push re-applies
//! the stashed changes to the working tree; redoing it stashes them again.
//! Undo and redo are recorded as entries of kind `undo` / `redo`, so the
//! log stays append-only. Snapshots are never deleted, so every state the
//! log mentions stays reachable (objects of stash entries only until a
//! `wt gc` after they were dropped).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::lock::TreeLock;
use super::snapshotting::compute_changes;
use super::stash::StashEntry;
use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::{ReflogEntry, TagInfo};

/// The oplog is trimmed to its newer half once it grows beyond this size.
const OPLOG_MAX_BYTES: u64 = 4 << 20;

/// Operation names of the stash (set by `repo/stash.rs` through
/// [`OpGuard::name_op`]).
pub(crate) const STASH_PUSH: &str = "stash.push";
pub(crate) const STASH_POP: &str = "stash.pop";
pub(crate) const STASH_DROP: &str = "stash.drop";
pub(crate) const STASH_CLEAR: &str = "stash.clear";

/// What an [`OpLogEntry`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind {
    /// A local operation (snapshot, merge, switch, ...).
    Op,
    /// `wt undo` of the operation named by `target`.
    Undo,
    /// `wt redo` of the operation named by `target`.
    Redo,
}

/// A branch ref as recorded in the oplog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRef {
    pub tip: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

/// One ref an operation changed (`None` = the ref did not exist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ref", rename_all = "snake_case")]
pub enum RefChange {
    /// The current branch (`HEAD`).
    Head {
        old: Option<String>,
        new: Option<String>,
    },
    Branch {
        name: String,
        old: Option<BranchRef>,
        new: Option<BranchRef>,
    },
    Tag {
        name: String,
        old: Option<Box<TagInfo>>,
        new: Option<Box<TagInfo>>,
    },
    /// The raw content of `stash.jsonl`.
    Stash { old: String, new: String },
}

impl RefChange {
    /// The change with `old` and `new` swapped.
    fn inverted(&self) -> RefChange {
        match self.clone() {
            RefChange::Head { old, new } => RefChange::Head { old: new, new: old },
            RefChange::Branch { name, old, new } => RefChange::Branch {
                name,
                old: new,
                new: old,
            },
            RefChange::Tag { name, old, new } => RefChange::Tag {
                name,
                old: new,
                new: old,
            },
            RefChange::Stash { old, new } => RefChange::Stash { old: new, new: old },
        }
    }

    /// Short human-readable form, e.g. `main: 1a2b3c4d -> 5e6f7a8b`.
    pub fn describe(&self) -> String {
        fn tip(b: &Option<BranchRef>) -> String {
            match b {
                None => "(none)".into(),
                Some(b) => {
                    let t = b.tip.as_deref().map(short).unwrap_or("(empty)");
                    if b.archived {
                        format!("{t} [archived]")
                    } else {
                        t.to_string()
                    }
                }
            }
        }
        match self {
            RefChange::Head { old, new } => format!(
                "HEAD: {} -> {}",
                old.as_deref().unwrap_or("(none)"),
                new.as_deref().unwrap_or("(none)")
            ),
            RefChange::Branch { name, old, new } => {
                format!("{name}: {} -> {}", tip(old), tip(new))
            }
            RefChange::Tag { name, old, new } => format!(
                "tag {name}: {} -> {}",
                old.as_ref().map(|t| short(&t.target)).unwrap_or("(none)"),
                new.as_ref().map(|t| short(&t.target)).unwrap_or("(none)")
            ),
            RefChange::Stash { old, new } => format!(
                "stash: {} -> {} entr{}",
                stash_len(old),
                stash_len(new),
                if stash_len(new) == 1 { "y" } else { "ies" }
            ),
        }
    }
}

/// One line of `.wt/oplog.jsonl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpLogEntry {
    /// Stable id (UUID).
    pub id: String,
    pub at: DateTime<Utc>,
    pub tree: String,
    pub kind: OpKind,
    /// Operation name: the reflog action ("snapshot", "merge", "switch",
    /// ...), "stash", "update", or "undo" / "redo".
    pub op: String,
    pub description: String,
    /// For undo / redo entries: the id of the operation undone or redone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub changes: Vec<RefChange>,
}

/// Options of [`Repository::undo`] and [`Repository::redo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoOptions {
    /// How many operations to undo / redo (at least 1).
    pub count: usize,
    /// Proceed although the working tree has uncommitted changes (they are
    /// discarded) or the refs changed outside the log.
    pub force: bool,
}

impl Default for UndoOptions {
    fn default() -> Self {
        UndoOptions {
            count: 1,
            force: false,
        }
    }
}

/// Result of [`Repository::undo`] / [`Repository::redo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReport {
    pub tree: String,
    /// "undo" or "redo".
    pub action: String,
    /// The operations undone or redone, in the order they were processed.
    pub operations: Vec<OpLogEntry>,
    /// Current branch and its tip afterwards.
    pub branch: String,
    pub head: Option<String>,
    /// Paths of stash entries re-applied to the working tree (undoing a
    /// stash push).
    pub restored_paths: Vec<String>,
}

/// The refs of one tree at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefState {
    head: Option<String>,
    branches: BTreeMap<String, BranchRef>,
    tags: BTreeMap<String, TagInfo>,
    stash: String,
}

/// The lock guard returned by `lock_tree`: holds the tree lock and records
/// the operation when dropped (before the lock is released).
#[derive(Debug)]
pub(crate) struct OpGuard<'a> {
    recorder: Option<Recorder<'a>>,
    _lock: TreeLock,
}

#[derive(Debug)]
struct Recorder<'a> {
    repo: &'a Repository,
    tree: String,
    before: RefState,
    reflog_len: u64,
    /// (op, description) set by the operation itself.
    hint: Option<(String, String)>,
}

impl<'a> OpGuard<'a> {
    /// Wrap `lock`; with `record`, capture the refs of `tree` now.
    pub(crate) fn new(repo: &'a Repository, tree: &str, lock: TreeLock, record: bool) -> Self {
        let recorder = if record {
            match repo.capture_refs(tree) {
                Ok(before) => Some(Recorder {
                    repo,
                    tree: tree.to_string(),
                    before,
                    reflog_len: fs::metadata(repo.reflog_file(tree))
                        .map(|m| m.len())
                        .unwrap_or(0),
                    hint: None,
                }),
                Err(err) => {
                    tracing::warn!(tree, %err, "oplog: cannot capture refs; operation not recorded");
                    None
                }
            }
        } else {
            None
        };
        OpGuard {
            recorder,
            _lock: lock,
        }
    }

    /// Name the operation (instead of deriving the name from the reflog).
    pub(crate) fn name_op(&mut self, op: &str, description: impl Into<String>) {
        if let Some(rec) = &mut self.recorder {
            rec.hint = Some((op.to_string(), description.into()));
        }
    }
}

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        if let Some(rec) = self.recorder.take() {
            if let Err(err) = rec.finish() {
                tracing::warn!(tree = %rec.tree, %err, "oplog: recording the operation failed");
            }
        }
    }
}

impl Recorder<'_> {
    fn finish(&self) -> Result<()> {
        let after = self.repo.capture_refs(&self.tree)?;
        let changes = diff_states(&self.before, &after);
        if changes.is_empty() {
            return Ok(());
        }
        let (op, description) = self.name(&changes);
        self.repo.append_oplog(&OpLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            at: Utc::now(),
            tree: self.tree.clone(),
            kind: OpKind::Op,
            op,
            description,
            target: None,
            changes,
        })
    }

    /// Operation name and description: the newest reflog entry written
    /// while the lock was held, else derived from the changes.
    fn name(&self, changes: &[RefChange]) -> (String, String) {
        if let Some(hint) = &self.hint {
            return hint.clone();
        }
        let path = self.repo.reflog_file(&self.tree);
        let tail = fs::read(&path)
            .ok()
            .and_then(|data| data.get(self.reflog_len as usize..).map(<[u8]>::to_vec))
            .unwrap_or_default();
        let newest = String::from_utf8_lossy(&tail)
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str::<ReflogEntry>(l).ok());
        if let Some(e) = newest {
            return (e.action, e.message);
        }
        let only_stash = changes.iter().all(|c| matches!(c, RefChange::Stash { .. }));
        if only_stash {
            return ("stash".into(), "stash changed".into());
        }
        let text: Vec<String> = changes.iter().map(RefChange::describe).collect();
        ("update".into(), text.join(", "))
    }
}

fn diff_states(before: &RefState, after: &RefState) -> Vec<RefChange> {
    let mut out = Vec::new();
    if before.head != after.head {
        out.push(RefChange::Head {
            old: before.head.clone(),
            new: after.head.clone(),
        });
    }
    let names: std::collections::BTreeSet<&String> = before
        .branches
        .keys()
        .chain(after.branches.keys())
        .collect();
    for name in names {
        let (old, new) = (before.branches.get(name), after.branches.get(name));
        if old != new {
            out.push(RefChange::Branch {
                name: name.clone(),
                old: old.cloned(),
                new: new.cloned(),
            });
        }
    }
    let names: std::collections::BTreeSet<&String> =
        before.tags.keys().chain(after.tags.keys()).collect();
    for name in names {
        let (old, new) = (before.tags.get(name), after.tags.get(name));
        if old != new {
            out.push(RefChange::Tag {
                name: name.clone(),
                old: old.cloned().map(Box::new),
                new: new.cloned().map(Box::new),
            });
        }
    }
    if before.stash != after.stash {
        out.push(RefChange::Stash {
            old: before.stash.clone(),
            new: after.stash.clone(),
        });
    }
    out
}

impl Repository {
    /// The operations of `tree` that `wt undo` can undo, newest first (the
    /// position + 1 is the `N` of `wt undo N`).
    pub fn oplog(&self, tree: &str) -> Result<Vec<OpLogEntry>> {
        self.load_tree(tree)?;
        let (mut done, _) = stacks(self.read_oplog(tree)?);
        done.reverse();
        Ok(done)
    }

    /// Undone operations of `tree` that `wt redo` can re-apply, next first.
    pub fn redo_list(&self, tree: &str) -> Result<Vec<OpLogEntry>> {
        self.load_tree(tree)?;
        let (_, mut undone) = stacks(self.read_oplog(tree)?);
        undone.reverse();
        Ok(undone)
    }

    /// Every entry of `.wt/oplog.jsonl` for `tree` (operations, undos and
    /// redos), oldest first.
    pub fn oplog_entries(&self, tree: &str) -> Result<Vec<OpLogEntry>> {
        self.load_tree(tree)?;
        self.read_oplog(tree)
    }

    /// Undo the newest `opts.count` operations of `tree` (`wt undo [N]`).
    pub fn undo(&self, tree: &str, opts: UndoOptions) -> Result<UndoReport> {
        self.undo_redo(tree, opts, OpKind::Undo)
    }

    /// Re-apply the `opts.count` most recently undone operations of `tree`
    /// (`wt redo [N]`); any new operation clears what can be redone.
    pub fn redo(&self, tree: &str, opts: UndoOptions) -> Result<UndoReport> {
        self.undo_redo(tree, opts, OpKind::Redo)
    }

    fn undo_redo(&self, tree: &str, opts: UndoOptions, kind: OpKind) -> Result<UndoReport> {
        let action = if kind == OpKind::Undo { "undo" } else { "redo" };
        let _lock = self.lock_tree_unrecorded(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_rewrite(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let count = opts.count.max(1);
        let (done, undone) = stacks(self.read_oplog(tree)?);
        let stack = if kind == OpKind::Undo { done } else { undone };
        if stack.is_empty() {
            return Err(SdkError::InvalidArgument(format!("nothing to {action}")));
        }
        if stack.len() < count {
            return Err(SdkError::InvalidArgument(format!(
                "only {} operation{} can be {action}ne",
                stack.len(),
                if stack.len() == 1 { "" } else { "s" }
            )));
        }
        let targets: Vec<OpLogEntry> = stack.into_iter().rev().take(count).collect();
        // The changes to apply, in order: undo applies `old`, redo `new`.
        let steps: Vec<Vec<RefChange>> = targets
            .iter()
            .map(|op| {
                if kind == OpKind::Undo {
                    op.changes.iter().map(RefChange::inverted).collect()
                } else {
                    op.changes.clone()
                }
            })
            .collect();

        // Stash semantics: undoing a push (redoing a pop) takes entries out
        // of the stash and re-applies them to the working tree; undoing a
        // pop (redoing a push) puts entries back, and the working tree may
        // then hold exactly their changes.
        let (reapply_op, restash_op) = match kind {
            OpKind::Undo => (STASH_PUSH, STASH_POP),
            _ => (STASH_POP, STASH_PUSH),
        };
        let mut reapply = Vec::new();
        let mut restashed = Vec::new();
        for (op, step) in targets.iter().zip(&steps) {
            for c in step {
                if let RefChange::Stash { old, new } = c {
                    if op.op == reapply_op {
                        reapply.extend(entries_only_in(old, new));
                    }
                    if op.op == restash_op {
                        restashed.extend(entries_only_in(new, old));
                    }
                }
            }
        }

        // Refs must still be where the operations left them.
        if !opts.force {
            let mut state = self.capture_refs(tree)?;
            for (op, step) in targets.iter().zip(&steps) {
                if let Some(c) = step.iter().find(|c| !state.matches_old(c)) {
                    return Err(SdkError::InvalidArgument(format!(
                        "cannot {action} \"{}\": {} changed since (use --force)",
                        op.description,
                        c.describe()
                    )));
                }
                for c in step {
                    state.apply(c);
                }
            }
        }
        // Working tree: clean, unless forced or holding exactly a stash
        // entry these steps put back into the stash.
        let (_, head_tip) = self.head_tip(tree)?;
        let head = self.manifest_of_tip(head_tip.as_deref())?;
        let working = self.build_working(&rec, &head, true, false)?;
        let dirty = compute_changes(&head, &working.manifest);
        if !dirty.is_empty() && !opts.force {
            let working_hash = self.write_manifest(&working.manifest)?;
            if !restashed.iter().any(|e| e.manifest == working_hash) {
                return Err(SdkError::DirtyWorkingTree(dirty.len()));
            }
        }

        for step in &steps {
            for c in step {
                self.restore_ref(tree, c)?;
            }
        }

        // Check the (possibly new) head out, discarding working changes.
        let (branch, new_tip) = self.head_tip(tree)?;
        let target = self.manifest_of_tip(new_tip.as_deref())?;
        let mut tracked: HashSet<String> = head.entries.iter().map(|e| e.path.clone()).collect();
        tracked.extend(working.manifest.entries.iter().map(|e| e.path.clone()));
        let mut working = working;
        self.apply_manifest(
            &rec,
            &target,
            &tracked,
            &working.manifest,
            &mut working.index,
        )?;
        self.save_index(tree, &working.index)?;
        let mut restored_paths = Vec::new();
        for entry in &reapply {
            let outcome = self.apply_stash_entry_unlocked(&rec, entry, 0)?;
            restored_paths.extend(outcome.changed);
        }
        restored_paths.sort();
        restored_paths.dedup();

        for (op, step) in targets.iter().zip(&steps) {
            self.append_oplog(&OpLogEntry {
                id: uuid::Uuid::new_v4().to_string(),
                at: Utc::now(),
                tree: tree.to_string(),
                kind,
                op: action.into(),
                description: format!("{action} {}: {}", op.op, op.description),
                target: Some(op.id.clone()),
                changes: step.clone(),
            })?;
        }
        Ok(UndoReport {
            tree: tree.to_string(),
            action: action.into(),
            operations: targets,
            branch,
            head: new_tip,
            restored_paths,
        })
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    /// The per-tree lock without oplog recording (undo/redo record
    /// themselves).
    pub(crate) fn lock_tree_unrecorded(&self, tree: &str) -> Result<OpGuard<'_>> {
        let lock = self.acquire_tree_lock(tree)?;
        Ok(OpGuard::new(self, tree, lock, false))
    }

    fn oplog_path(&self) -> PathBuf {
        self.wt_dir().join("oplog.jsonl")
    }

    fn reflog_file(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("reflog.jsonl")
    }

    fn read_stash_text(&self, tree: &str) -> Result<String> {
        Ok(
            fsutil::read_string_opt(&self.tree_meta_dir(tree).join("stash.jsonl"))?
                .unwrap_or_default(),
        )
    }

    pub(crate) fn capture_refs(&self, tree: &str) -> Result<RefState> {
        let branches = self
            .branches(tree, true)?
            .into_iter()
            .map(|b| {
                (
                    b.name,
                    BranchRef {
                        tip: b.tip,
                        archived: b.archived,
                    },
                )
            })
            .collect();
        let tags = self
            .tags(tree)?
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect();
        Ok(RefState {
            head: self.head_branch(tree).ok(),
            branches,
            tags,
            stash: self.read_stash_text(tree)?,
        })
    }

    fn restore_ref(&self, tree: &str, change: &RefChange) -> Result<()> {
        match change {
            RefChange::Head { new, .. } => match new {
                Some(branch) => self.write_head(tree, branch),
                None => Ok(()),
            },
            RefChange::Branch { name, new, .. } => {
                self.restore_branch_ref(tree, name, new.as_ref())
            }
            RefChange::Tag { name, new, .. } => self.restore_tag_ref(tree, name, new.as_deref()),
            RefChange::Stash { new, .. } => {
                let path = self.tree_meta_dir(tree).join("stash.jsonl");
                if new.is_empty() {
                    fsutil::remove_file_if_exists(&path)
                } else {
                    fsutil::write_atomic(&path, new.as_bytes())
                }
            }
        }
    }

    fn read_oplog(&self, tree: &str) -> Result<Vec<OpLogEntry>> {
        let Some(text) = fsutil::read_string_opt(&self.oplog_path())? else {
            return Ok(Vec::new());
        };
        Ok(text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| match serde_json::from_str::<OpLogEntry>(l) {
                Ok(e) => Some(e),
                Err(err) => {
                    tracing::warn!(%err, "skipping malformed oplog line");
                    None
                }
            })
            .filter(|e| e.tree == tree)
            .collect())
    }

    /// Append one entry (under `.wt/oplog.lock`), trimming the log to its
    /// newer half when it exceeds [`OPLOG_MAX_BYTES`].
    fn append_oplog(&self, entry: &OpLogEntry) -> Result<()> {
        let _lock = TreeLock::acquire(&self.wt_dir().join("oplog.lock"))?;
        let path = self.oplog_path();
        fsutil::append_line(&path, &serde_json::to_string(entry)?)?;
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > OPLOG_MAX_BYTES {
            let text = fs::read_to_string(&path)?;
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            let mut kept = lines[lines.len() / 2..].join("\n");
            kept.push('\n');
            fsutil::write_atomic(&path, kept.as_bytes())?;
        }
        Ok(())
    }
}

impl RefState {
    /// Whether the ref `c` touches currently has the value `c` replaces.
    fn matches_old(&self, c: &RefChange) -> bool {
        match c {
            RefChange::Head { old, .. } => &self.head == old,
            RefChange::Branch { name, old, .. } => self.branches.get(name) == old.as_ref(),
            RefChange::Tag { name, old, .. } => self.tags.get(name) == old.as_deref(),
            RefChange::Stash { old, .. } => &self.stash == old,
        }
    }

    fn apply(&mut self, c: &RefChange) {
        match c {
            RefChange::Head { new, .. } => self.head = new.clone(),
            RefChange::Branch { name, new, .. } => match new {
                Some(b) => {
                    self.branches.insert(name.clone(), b.clone());
                }
                None => {
                    self.branches.remove(name);
                }
            },
            RefChange::Tag { name, new, .. } => match new {
                Some(t) => {
                    self.tags.insert(name.clone(), (**t).clone());
                }
                None => {
                    self.tags.remove(name);
                }
            },
            RefChange::Stash { new, .. } => self.stash = new.clone(),
        }
    }
}

/// (done, undone): the operations currently applied (oldest first) and the
/// undone ones that can be redone (the next redo last).
fn stacks(entries: Vec<OpLogEntry>) -> (Vec<OpLogEntry>, Vec<OpLogEntry>) {
    let mut done: Vec<OpLogEntry> = Vec::new();
    let mut undone: Vec<OpLogEntry> = Vec::new();
    for e in entries {
        match e.kind {
            OpKind::Op => {
                done.push(e);
                undone.clear();
            }
            OpKind::Undo => {
                if let Some(pos) = done.iter().rposition(|d| Some(&d.id) == e.target.as_ref()) {
                    undone.push(done.remove(pos));
                }
            }
            OpKind::Redo => {
                if let Some(pos) = undone
                    .iter()
                    .rposition(|d| Some(&d.id) == e.target.as_ref())
                {
                    done.push(undone.remove(pos));
                }
            }
        }
    }
    (done, undone)
}

fn parse_stash(text: &str) -> Vec<StashEntry> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<StashEntry>(l).ok())
        .collect()
}

/// Stash entries of the stash text `a` that are not in `b`.
fn entries_only_in(a: &str, b: &str) -> Vec<StashEntry> {
    let other: HashSet<String> = parse_stash(b).into_iter().map(|e| e.id).collect();
    parse_stash(a)
        .into_iter()
        .filter(|e| !other.contains(&e.id))
        .collect()
}

fn stash_len(text: &str) -> usize {
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, kind: OpKind, target: Option<&str>) -> OpLogEntry {
        OpLogEntry {
            id: id.into(),
            at: Utc::now(),
            tree: "t".into(),
            kind,
            op: "snapshot".into(),
            description: id.into(),
            target: target.map(str::to_string),
            changes: vec![],
        }
    }

    fn ids(v: &[OpLogEntry]) -> Vec<&str> {
        v.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn undo_and_redo_stacks() {
        let (done, undone) = stacks(vec![
            op("a", OpKind::Op, None),
            op("b", OpKind::Op, None),
            op("u1", OpKind::Undo, Some("b")),
        ]);
        assert_eq!((ids(&done), ids(&undone)), (vec!["a"], vec!["b"]));
        let (done, undone) = stacks(vec![
            op("a", OpKind::Op, None),
            op("b", OpKind::Op, None),
            op("u1", OpKind::Undo, Some("b")),
            op("u2", OpKind::Undo, Some("a")),
            op("r1", OpKind::Redo, Some("a")),
        ]);
        assert_eq!((ids(&done), ids(&undone)), (vec!["a"], vec!["b"]));
        // A new operation clears the redo stack.
        let (done, undone) = stacks(vec![
            op("a", OpKind::Op, None),
            op("u1", OpKind::Undo, Some("a")),
            op("c", OpKind::Op, None),
        ]);
        assert_eq!((ids(&done), ids(&undone)), (vec!["c"], Vec::<&str>::new()));
    }

    #[test]
    fn changes_invert_and_describe() {
        let c = RefChange::Branch {
            name: "main".into(),
            old: Some(BranchRef {
                tip: Some("0123456789".into()),
                archived: false,
            }),
            new: None,
        };
        assert_eq!(c.describe(), "main: 01234567 -> (none)");
        assert_eq!(c.inverted().inverted(), c);
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["ref"], "branch");
        let s = RefChange::Stash {
            old: String::new(),
            new: "{}\n".into(),
        };
        assert_eq!(s.describe(), "stash: 0 -> 1 entry");
    }

    #[test]
    fn diff_detects_every_ref_kind() {
        let mut before = RefState {
            head: Some("main".into()),
            branches: BTreeMap::new(),
            tags: BTreeMap::new(),
            stash: String::new(),
        };
        before.branches.insert(
            "main".into(),
            BranchRef {
                tip: Some("a".into()),
                archived: false,
            },
        );
        let mut after = before.clone();
        assert!(diff_states(&before, &after).is_empty());
        after.head = Some("dev".into());
        after.branches.insert(
            "dev".into(),
            BranchRef {
                tip: Some("a".into()),
                archived: false,
            },
        );
        after.stash = "x\n".into();
        let changes = diff_states(&before, &after);
        assert_eq!(changes.len(), 3);
        let mut state = before.clone();
        for c in &changes {
            assert!(state.matches_old(c));
            state.apply(c);
        }
        assert_eq!(state, after);
    }
}
