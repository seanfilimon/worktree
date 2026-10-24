//! Merge strategies and results.
//!
//! Provides [`MergeStrategy`] for selecting how divergent branches are combined,
//! and [`MergeResult`] to represent the outcome of a merge operation.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use crate::core::hash::ContentHash;
use crate::core::id::SnapshotId;
use crate::merge::conflict::MergeConflict;
use crate::object::delta::Delta;

// ---------------------------------------------------------------------------
// MergeStrategy
// ---------------------------------------------------------------------------

/// The strategy used to combine two divergent branch histories.
///
/// Different strategies produce different results when the same path has been
/// modified on both sides of the merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// Standard three-way merge. Applies changes from both sides relative to
    /// the common ancestor. Conflicts are reported when both sides modify the
    /// same path in incompatible ways.
    ThreeWay,

    /// "Ours" strategy — when there is a conflict, always keep the version
    /// from the current (target) branch.
    Ours,

    /// "Theirs" strategy — when there is a conflict, always keep the version
    /// from the source branch being merged in.
    Theirs,

    /// Fast-forward only. The merge is only allowed if the target branch is a
    /// direct ancestor of the source branch. No merge snapshot is created;
    /// the branch pointer simply advances.
    FastForward,

    /// Union merge — for text files, take lines from both sides (useful for
    /// changelogs or append-only files). For binary files, fall back to
    /// three-way conflict reporting.
    Union,
}

impl MergeStrategy {
    /// Returns `true` if this strategy can produce conflicts that require
    /// manual resolution.
    pub fn can_conflict(&self) -> bool {
        match self {
            MergeStrategy::ThreeWay => true,
            MergeStrategy::Ours => false,
            MergeStrategy::Theirs => false,
            MergeStrategy::FastForward => false,
            MergeStrategy::Union => true,
        }
    }

    /// Returns `true` if this strategy always produces a merge snapshot
    /// (as opposed to fast-forward which does not).
    pub fn creates_merge_snapshot(&self) -> bool {
        !matches!(self, MergeStrategy::FastForward)
    }

    /// Returns the stable string representation of this strategy.
    pub fn as_str(&self) -> &'static str {
        match self {
            MergeStrategy::ThreeWay => "three-way",
            MergeStrategy::Ours => "ours",
            MergeStrategy::Theirs => "theirs",
            MergeStrategy::FastForward => "fast-forward",
            MergeStrategy::Union => "union",
        }
    }
}

impl Default for MergeStrategy {
    fn default() -> Self {
        MergeStrategy::ThreeWay
    }
}

impl fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// MergeOutcome
// ---------------------------------------------------------------------------

/// The high-level outcome of a merge attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MergeOutcome {
    /// The merge completed cleanly with no conflicts.
    Clean,
    /// The merge completed but had conflicts that were auto-resolved by the
    /// chosen strategy (e.g. `Ours` or `Theirs`).
    AutoResolved,
    /// The merge has unresolved conflicts that require manual intervention.
    Conflicted,
    /// The merge was a fast-forward — no new snapshot was needed.
    FastForwarded,
    /// The merge was not possible (e.g. fast-forward requested but branches
    /// have diverged).
    NotPossible,
}

impl fmt::Display for MergeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeOutcome::Clean => write!(f, "clean"),
            MergeOutcome::AutoResolved => write!(f, "auto-resolved"),
            MergeOutcome::Conflicted => write!(f, "conflicted"),
            MergeOutcome::FastForwarded => write!(f, "fast-forwarded"),
            MergeOutcome::NotPossible => write!(f, "not-possible"),
        }
    }
}

// ---------------------------------------------------------------------------
// MergeResult
// ---------------------------------------------------------------------------

/// The result of a merge operation.
///
/// Contains the merged deltas, any conflicts, and the resulting manifest hash
/// when the merge is successful.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeResult {
    /// The strategy that was used.
    pub strategy: MergeStrategy,

    /// The high-level outcome.
    pub outcome: MergeOutcome,

    /// The common ancestor snapshot, if one was found.
    pub base_snapshot: Option<SnapshotId>,

    /// The snapshot on the target (current) branch.
    pub ours_snapshot: SnapshotId,

    /// The snapshot on the source (incoming) branch.
    pub theirs_snapshot: SnapshotId,

    /// The content hash of the merged manifest (if the merge was successful).
    pub merged_manifest_hash: Option<ContentHash>,

    /// The merged deltas — the set of changes to apply to the target branch.
    pub deltas: Vec<Delta>,

    /// Any conflicts that arose during the merge.
    pub conflicts: Vec<MergeConflict>,

    /// Paths that were auto-resolved by the strategy.
    pub auto_resolved_paths: Vec<PathBuf>,
}

impl MergeResult {
    /// Create a new merge result.
    pub fn new(
        strategy: MergeStrategy,
        ours_snapshot: SnapshotId,
        theirs_snapshot: SnapshotId,
    ) -> Self {
        Self {
            strategy,
            outcome: MergeOutcome::Clean,
            base_snapshot: None,
            ours_snapshot,
            theirs_snapshot,
            merged_manifest_hash: None,
            deltas: Vec::new(),
            conflicts: Vec::new(),
            auto_resolved_paths: Vec::new(),
        }
    }

    /// Create a clean merge result with the given deltas and manifest hash.
    pub fn clean(
        strategy: MergeStrategy,
        ours_snapshot: SnapshotId,
        theirs_snapshot: SnapshotId,
        deltas: Vec<Delta>,
        manifest_hash: ContentHash,
    ) -> Self {
        Self {
            strategy,
            outcome: MergeOutcome::Clean,
            base_snapshot: None,
            ours_snapshot,
            theirs_snapshot,
            merged_manifest_hash: Some(manifest_hash),
            deltas,
            conflicts: Vec::new(),
            auto_resolved_paths: Vec::new(),
        }
    }

    /// Create a conflicted merge result.
    pub fn conflicted(
        strategy: MergeStrategy,
        ours_snapshot: SnapshotId,
        theirs_snapshot: SnapshotId,
        conflicts: Vec<MergeConflict>,
        deltas: Vec<Delta>,
    ) -> Self {
        Self {
            strategy,
            outcome: MergeOutcome::Conflicted,
            base_snapshot: None,
            ours_snapshot,
            theirs_snapshot,
            merged_manifest_hash: None,
            deltas,
            conflicts,
            auto_resolved_paths: Vec::new(),
        }
    }

    /// Create a fast-forward merge result.
    pub fn fast_forward(
        ours_snapshot: SnapshotId,
        theirs_snapshot: SnapshotId,
    ) -> Self {
        Self {
            strategy: MergeStrategy::FastForward,
            outcome: MergeOutcome::FastForwarded,
            base_snapshot: None,
            ours_snapshot,
            theirs_snapshot,
            merged_manifest_hash: None,
            deltas: Vec::new(),
            conflicts: Vec::new(),
            auto_resolved_paths: Vec::new(),
        }
    }

    /// Create a not-possible merge result.
    pub fn not_possible(
        strategy: MergeStrategy,
        ours_snapshot: SnapshotId,
        theirs_snapshot: SnapshotId,
    ) -> Self {
        Self {
            strategy,
            outcome: MergeOutcome::NotPossible,
            base_snapshot: None,
            ours_snapshot,
            theirs_snapshot,
            merged_manifest_hash: None,
            deltas: Vec::new(),
            conflicts: Vec::new(),
            auto_resolved_paths: Vec::new(),
        }
    }

    /// Set the base snapshot.
    pub fn with_base_snapshot(mut self, base: SnapshotId) -> Self {
        self.base_snapshot = Some(base);
        self
    }

    /// Returns `true` if the merge was clean (no conflicts).
    pub fn is_clean(&self) -> bool {
        self.outcome == MergeOutcome::Clean
    }

    /// Returns `true` if the merge has unresolved conflicts.
    pub fn is_conflicted(&self) -> bool {
        self.outcome == MergeOutcome::Conflicted
    }

    /// Returns `true` if the merge was a fast-forward.
    pub fn is_fast_forward(&self) -> bool {
        self.outcome == MergeOutcome::FastForwarded
    }

    /// Returns `true` if the merge was not possible.
    pub fn is_not_possible(&self) -> bool {
        self.outcome == MergeOutcome::NotPossible
    }

    /// Returns `true` if there were any auto-resolved paths.
    pub fn has_auto_resolved(&self) -> bool {
        !self.auto_resolved_paths.is_empty()
    }

    /// Returns the number of conflicts.
    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// Returns the number of deltas.
    pub fn delta_count(&self) -> usize {
        self.deltas.len()
    }

    /// Add a conflict to this result, updating the outcome to `Conflicted`.
    pub fn add_conflict(&mut self, conflict: MergeConflict) {
        self.conflicts.push(conflict);
        self.outcome = MergeOutcome::Conflicted;
    }

    /// Record an auto-resolved path.
    pub fn add_auto_resolved(&mut self, path: PathBuf) {
        self.auto_resolved_paths.push(path);
        if self.outcome == MergeOutcome::Clean {
            self.outcome = MergeOutcome::AutoResolved;
        }
    }

    /// Add a delta to the merged deltas list.
    pub fn add_delta(&mut self, delta: Delta) {
        self.deltas.push(delta);
    }
}

impl fmt::Display for MergeResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MergeResult({}, strategy={}, deltas={}, conflicts={})",
            self.outcome,
            self.strategy,
            self.deltas.len(),
            self.conflicts.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;
    use crate::core::id::SnapshotId;
    use crate::merge::conflict::{ConflictKind, ConflictSide};
    use crate::object::delta::Delta;

    #[test]
    fn test_strategy_default_is_three_way() {
        assert_eq!(MergeStrategy::default(), MergeStrategy::ThreeWay);
    }

    #[test]
    fn test_strategy_display() {
        assert_eq!(MergeStrategy::ThreeWay.to_string(), "three-way");
        assert_eq!(MergeStrategy::Ours.to_string(), "ours");
        assert_eq!(MergeStrategy::Theirs.to_string(), "theirs");
        assert_eq!(MergeStrategy::FastForward.to_string(), "fast-forward");
        assert_eq!(MergeStrategy::Union.to_string(), "union");
    }

    #[test]
    fn test_strategy_as_str() {
        for strategy in &[
            MergeStrategy::ThreeWay,
            MergeStrategy::Ours,
            MergeStrategy::Theirs,
            MergeStrategy::FastForward,
            MergeStrategy::Union,
        ] {
            assert_eq!(strategy.to_string(), strategy.as_str());
        }
    }

    #[test]
    fn test_can_conflict() {
        assert!(MergeStrategy::ThreeWay.can_conflict());
        assert!(!MergeStrategy::Ours.can_conflict());
        assert!(!MergeStrategy::Theirs.can_conflict());
        assert!(!MergeStrategy::FastForward.can_conflict());
        assert!(MergeStrategy::Union.can_conflict());
    }

    #[test]
    fn test_creates_merge_snapshot() {
        assert!(MergeStrategy::ThreeWay.creates_merge_snapshot());
        assert!(MergeStrategy::Ours.creates_merge_snapshot());
        assert!(MergeStrategy::Theirs.creates_merge_snapshot());
        assert!(!MergeStrategy::FastForward.creates_merge_snapshot());
        assert!(MergeStrategy::Union.creates_merge_snapshot());
    }

    #[test]
    fn test_outcome_display() {
        assert_eq!(MergeOutcome::Clean.to_string(), "clean");
        assert_eq!(MergeOutcome::AutoResolved.to_string(), "auto-resolved");
        assert_eq!(MergeOutcome::Conflicted.to_string(), "conflicted");
        assert_eq!(MergeOutcome::FastForwarded.to_string(), "fast-forwarded");
        assert_eq!(MergeOutcome::NotPossible.to_string(), "not-possible");
    }

    #[test]
    fn test_merge_result_new() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        assert!(result.is_clean());
        assert!(!result.is_conflicted());
        assert!(!result.is_fast_forward());
        assert!(!result.is_not_possible());
        assert_eq!(result.conflict_count(), 0);
        assert_eq!(result.delta_count(), 0);
        assert_eq!(result.ours_snapshot, ours);
        assert_eq!(result.theirs_snapshot, theirs);
        assert!(result.base_snapshot.is_none());
        assert!(result.merged_manifest_hash.is_none());
    }

    #[test]
    fn test_merge_result_clean() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let hash = hash_bytes(b"merged manifest");
        let deltas = vec![Delta::add("new.txt", hash_bytes(b"new"), 3)];

        let result = MergeResult::clean(
            MergeStrategy::ThreeWay,
            ours,
            theirs,
            deltas.clone(),
            hash,
        );

        assert!(result.is_clean());
        assert_eq!(result.merged_manifest_hash, Some(hash));
        assert_eq!(result.delta_count(), 1);
        assert_eq!(result.conflict_count(), 0);
    }

    #[test]
    fn test_merge_result_conflicted() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let conflict = MergeConflict::new(
            PathBuf::from("conflict.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"ours")), Some(10)),
            ConflictSide::new(Some(hash_bytes(b"theirs")), Some(12)),
        );

        let result = MergeResult::conflicted(
            MergeStrategy::ThreeWay,
            ours,
            theirs,
            vec![conflict],
            vec![],
        );

        assert!(result.is_conflicted());
        assert!(!result.is_clean());
        assert_eq!(result.conflict_count(), 1);
        assert!(result.merged_manifest_hash.is_none());
    }

    #[test]
    fn test_merge_result_fast_forward() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let result = MergeResult::fast_forward(ours, theirs);

        assert!(result.is_fast_forward());
        assert!(!result.is_clean());
        assert!(!result.is_conflicted());
        assert_eq!(result.strategy, MergeStrategy::FastForward);
    }

    #[test]
    fn test_merge_result_not_possible() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let result = MergeResult::not_possible(MergeStrategy::FastForward, ours, theirs);

        assert!(result.is_not_possible());
        assert!(!result.is_clean());
        assert!(!result.is_conflicted());
        assert!(!result.is_fast_forward());
    }

    #[test]
    fn test_with_base_snapshot() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let base = SnapshotId::new();

        let result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs)
            .with_base_snapshot(base);

        assert_eq!(result.base_snapshot, Some(base));
    }

    #[test]
    fn test_add_conflict_changes_outcome() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let mut result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        assert!(result.is_clean());

        let conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"a")), Some(1)),
            ConflictSide::new(Some(hash_bytes(b"b")), Some(1)),
        );
        result.add_conflict(conflict);

        assert!(result.is_conflicted());
        assert_eq!(result.conflict_count(), 1);
    }

    #[test]
    fn test_add_auto_resolved_changes_outcome() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let mut result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        assert!(result.is_clean());
        assert!(!result.has_auto_resolved());

        result.add_auto_resolved(PathBuf::from("resolved.txt"));

        assert!(result.has_auto_resolved());
        assert_eq!(result.outcome, MergeOutcome::AutoResolved);
        assert_eq!(result.auto_resolved_paths.len(), 1);
    }

    #[test]
    fn test_add_auto_resolved_does_not_override_conflicted() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let mut result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        let conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"a")), Some(1)),
            ConflictSide::new(Some(hash_bytes(b"b")), Some(1)),
        );
        result.add_conflict(conflict);
        result.add_auto_resolved(PathBuf::from("other.txt"));

        // Outcome should still be Conflicted, not AutoResolved.
        assert!(result.is_conflicted());
    }

    #[test]
    fn test_add_delta() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let mut result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        assert_eq!(result.delta_count(), 0);

        result.add_delta(Delta::add("new.txt", hash_bytes(b"new"), 3));
        assert_eq!(result.delta_count(), 1);

        result.add_delta(Delta::delete("old.txt", hash_bytes(b"old"), 3));
        assert_eq!(result.delta_count(), 2);
    }

    #[test]
    fn test_merge_result_display() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let result = MergeResult::new(MergeStrategy::ThreeWay, ours, theirs);

        let display = result.to_string();
        assert!(display.contains("clean"));
        assert!(display.contains("three-way"));
        assert!(display.contains("deltas=0"));
        assert!(display.contains("conflicts=0"));
    }

    #[test]
    fn test_serde_json_roundtrip_strategy() {
        for strategy in &[
            MergeStrategy::ThreeWay,
            MergeStrategy::Ours,
            MergeStrategy::Theirs,
            MergeStrategy::FastForward,
            MergeStrategy::Union,
        ] {
            let json = serde_json::to_string(strategy).expect("serialize");
            let deserialized: MergeStrategy = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*strategy, deserialized);
        }
    }

    #[test]
    fn test_serde_json_roundtrip_outcome() {
        for outcome in &[
            MergeOutcome::Clean,
            MergeOutcome::AutoResolved,
            MergeOutcome::Conflicted,
            MergeOutcome::FastForwarded,
            MergeOutcome::NotPossible,
        ] {
            let json = serde_json::to_string(outcome).expect("serialize");
            let deserialized: MergeOutcome = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*outcome, deserialized);
        }
    }

    #[test]
    fn test_serde_json_roundtrip_merge_result() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let base = SnapshotId::new();
        let hash = hash_bytes(b"merged");

        let conflict = MergeConflict::new(
            PathBuf::from("c.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"ours")), Some(4)),
            ConflictSide::new(Some(hash_bytes(b"theirs")), Some(6)),
        );

        let mut result = MergeResult::conflicted(
            MergeStrategy::ThreeWay,
            ours,
            theirs,
            vec![conflict],
            vec![Delta::add("new.txt", hash, 6)],
        )
        .with_base_snapshot(base);
        result.add_auto_resolved(PathBuf::from("auto.txt"));

        let json = serde_json::to_string(&result).expect("serialize");
        let deserialized: MergeResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(result.strategy, deserialized.strategy);
        assert_eq!(result.outcome, deserialized.outcome);
        assert_eq!(result.base_snapshot, deserialized.base_snapshot);
        assert_eq!(result.ours_snapshot, deserialized.ours_snapshot);
        assert_eq!(result.theirs_snapshot, deserialized.theirs_snapshot);
        assert_eq!(result.deltas.len(), deserialized.deltas.len());
        assert_eq!(result.conflicts.len(), deserialized.conflicts.len());
        assert_eq!(
            result.auto_resolved_paths.len(),
            deserialized.auto_resolved_paths.len()
        );
    }

    #[test]
    fn test_serde_bincode_roundtrip_merge_result() {
        let ours = SnapshotId::new();
        let theirs = SnapshotId::new();
        let hash = hash_bytes(b"bincode merge");

        let result = MergeResult::clean(
            MergeStrategy::Ours,
            ours,
            theirs,
            vec![Delta::add("file.txt", hash, 13)],
            hash,
        );

        let encoded = bincode::serialize(&result).expect("serialize");
        let decoded: MergeResult = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(result, decoded);
    }

    #[test]
    fn test_strategy_equality_and_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(MergeStrategy::ThreeWay);
        set.insert(MergeStrategy::ThreeWay);
        set.insert(MergeStrategy::Ours);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_strategy_copy() {
        let s = MergeStrategy::Theirs;
        let copy = s;
        assert_eq!(s, copy);
    }
}
