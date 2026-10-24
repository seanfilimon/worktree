//! Pure merge algorithms (no I/O). Used by `Repository::merge`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ConflictKind, Manifest, ManifestEntry, SnapshotId};

/// Labels used in conflict markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeLabels {
    pub ours: String,   // e.g. "main"
    pub base: String,   // e.g. "base"
    pub theirs: String, // e.g. "feature"
}

impl Default for MergeLabels {
    fn default() -> Self {
        Self { ours: "ours".into(), base: "base".into(), theirs: "theirs".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRegion {
    /// 1-based line in the merged output where `<<<<<<<` appears.
    pub start_line: usize,
    pub ours: Vec<String>,
    pub base: Vec<String>,
    pub theirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextMergeResult {
    Clean(String),
    /// `merged` contains diff3-style markers:
    /// `<<<<<<< ours` / `||||||| base` / `=======` / `>>>>>>> theirs`.
    Conflicted { merged: String, conflicts: Vec<ConflictRegion> },
}

/// Line-based three-way merge (diff3 semantics). Non-overlapping changes
/// from both sides are combined; identical changes on both sides are taken
/// once.
pub fn merge_text(base: &str, ours: &str, theirs: &str, labels: &MergeLabels) -> TextMergeResult {
    let _ = (base, ours, theirs, labels);
    todo!("sdk-merge-diff: merge_text")
}

/// What to do with one path when merging manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolution {
    /// Keep ours unchanged (includes "both sides identical").
    KeepOurs(ManifestEntry),
    /// Take theirs (only theirs changed / added).
    TakeTheirs(ManifestEntry),
    /// Delete the path (deleted on one side, unchanged on the other).
    Delete,
    /// Both modified a text file: caller must run `merge_text` on the blobs.
    MergeContent { base: Option<ManifestEntry>, ours: ManifestEntry, theirs: ManifestEntry },
    /// Unresolvable without the user.
    Conflict {
        kind: ConflictKind,
        base: Option<ManifestEntry>,
        ours: Option<ManifestEntry>,
        theirs: Option<ManifestEntry>,
    },
}

/// Three-way merge of manifests. Returns one resolution per path present in
/// any input (except paths absent from both ours and theirs), sorted by path.
/// AddAdd with identical hashes is KeepOurs; AddAdd with different content
/// is MergeContent with base None (caller treats it as a 2-way merge and
/// reports ConflictKind::AddAdd if it conflicts).
pub fn merge_manifests(
    base: &Manifest,
    ours: &Manifest,
    theirs: &Manifest,
) -> Vec<(String, PathResolution)> {
    let _ = (base, ours, theirs);
    todo!("sdk-merge-diff: merge_manifests")
}

/// Best common ancestor of `a` and `b` in a snapshot DAG described by
/// `parents` (id -> parent ids). Picks the ancestor reachable from both that
/// is not an ancestor of another common ancestor; ties broken by the
/// smallest BFS distance from `a`, then by id. `None` if unrelated.
pub fn merge_base(
    a: &str,
    b: &str,
    parents: &HashMap<SnapshotId, Vec<SnapshotId>>,
) -> Option<SnapshotId> {
    let _ = (a, b, parents);
    todo!("sdk-merge-diff: merge_base")
}
