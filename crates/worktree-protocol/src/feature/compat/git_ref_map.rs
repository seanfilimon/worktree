//! Git ref mapping.
//!
//! Provides [`GitRefKind`], [`GitRef`], and [`RefMapping`] for bidirectional
//! mapping between Worktree's branch/snapshot identifiers and Git's ref
//! namespace (refs/heads/*, refs/tags/*, etc.).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::core::id::{BranchId, SnapshotId};

// ---------------------------------------------------------------------------
// GitRefKind
// ---------------------------------------------------------------------------

/// The kind of Git ref being mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitRefKind {
    /// A branch head (refs/heads/*).
    Branch,
    /// A lightweight or annotated tag (refs/tags/*).
    Tag,
    /// A remote-tracking branch (refs/remotes/*/*).
    Remote,
    /// HEAD or another symbolic ref.
    Symbolic,
    /// A note ref (refs/notes/*).
    Note,
    /// Any other ref that doesn't fit the above categories.
    Other,
}

impl GitRefKind {
    /// Returns the stable string representation of this kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            GitRefKind::Branch => "branch",
            GitRefKind::Tag => "tag",
            GitRefKind::Remote => "remote",
            GitRefKind::Symbolic => "symbolic",
            GitRefKind::Note => "note",
            GitRefKind::Other => "other",
        }
    }

    /// Infer the ref kind from a full Git ref path.
    ///
    /// For example:
    /// - `refs/heads/main` → `Branch`
    /// - `refs/tags/v1.0` → `Tag`
    /// - `refs/remotes/origin/main` → `Remote`
    /// - `HEAD` → `Symbolic`
    /// - `refs/notes/commits` → `Note`
    pub fn from_ref_path(path: &str) -> Self {
        if path.starts_with("refs/heads/") {
            GitRefKind::Branch
        } else if path.starts_with("refs/tags/") {
            GitRefKind::Tag
        } else if path.starts_with("refs/remotes/") {
            GitRefKind::Remote
        } else if path == "HEAD" || path.starts_with("refs/symbolic/") {
            GitRefKind::Symbolic
        } else if path.starts_with("refs/notes/") {
            GitRefKind::Note
        } else {
            GitRefKind::Other
        }
    }

    /// Returns the ref prefix for this kind.
    pub fn ref_prefix(&self) -> &'static str {
        match self {
            GitRefKind::Branch => "refs/heads/",
            GitRefKind::Tag => "refs/tags/",
            GitRefKind::Remote => "refs/remotes/",
            GitRefKind::Symbolic => "",
            GitRefKind::Note => "refs/notes/",
            GitRefKind::Other => "refs/",
        }
    }
}

impl fmt::Display for GitRefKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// GitRef
// ---------------------------------------------------------------------------

/// A Git ref with its full path and the SHA-1 it points to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GitRef {
    /// The full ref path (e.g. "refs/heads/main", "refs/tags/v1.0").
    pub ref_path: String,

    /// The Git SHA-1 hex string this ref points to (40 characters).
    pub target_sha: String,

    /// The kind of ref, inferred from the path.
    pub kind: GitRefKind,

    /// If this is a symbolic ref, the ref it points to (e.g. HEAD → refs/heads/main).
    pub symbolic_target: Option<String>,
}

impl GitRef {
    /// Create a new Git ref, inferring the kind from the ref path.
    pub fn new(ref_path: impl Into<String>, target_sha: impl Into<String>) -> Self {
        let ref_path = ref_path.into();
        let kind = GitRefKind::from_ref_path(&ref_path);
        Self {
            ref_path,
            target_sha: target_sha.into(),
            kind,
            symbolic_target: None,
        }
    }

    /// Create a new symbolic ref (like HEAD).
    pub fn symbolic(
        ref_path: impl Into<String>,
        target_sha: impl Into<String>,
        symbolic_target: impl Into<String>,
    ) -> Self {
        let ref_path = ref_path.into();
        Self {
            ref_path,
            target_sha: target_sha.into(),
            kind: GitRefKind::Symbolic,
            symbolic_target: Some(symbolic_target.into()),
        }
    }

    /// Create a branch ref.
    pub fn branch(name: impl Into<String>, target_sha: impl Into<String>) -> Self {
        let name = name.into();
        let ref_path = format!("refs/heads/{}", name);
        Self {
            ref_path,
            target_sha: target_sha.into(),
            kind: GitRefKind::Branch,
            symbolic_target: None,
        }
    }

    /// Create a tag ref.
    pub fn tag(name: impl Into<String>, target_sha: impl Into<String>) -> Self {
        let name = name.into();
        let ref_path = format!("refs/tags/{}", name);
        Self {
            ref_path,
            target_sha: target_sha.into(),
            kind: GitRefKind::Tag,
            symbolic_target: None,
        }
    }

    /// Extract the short name from the full ref path.
    ///
    /// For example:
    /// - `refs/heads/main` → `main`
    /// - `refs/tags/v1.0` → `v1.0`
    /// - `refs/remotes/origin/main` → `origin/main`
    /// - `HEAD` → `HEAD`
    pub fn short_name(&self) -> &str {
        let prefix = self.kind.ref_prefix();
        if !prefix.is_empty() && self.ref_path.starts_with(prefix) {
            &self.ref_path[prefix.len()..]
        } else {
            &self.ref_path
        }
    }

    /// Returns `true` if the target SHA looks like a valid 40-character hex string.
    pub fn is_valid_target_sha(&self) -> bool {
        self.target_sha.len() == 40 && self.target_sha.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Returns `true` if this is a symbolic ref.
    pub fn is_symbolic(&self) -> bool {
        self.symbolic_target.is_some()
    }

    /// Returns `true` if this is a branch ref.
    pub fn is_branch(&self) -> bool {
        self.kind == GitRefKind::Branch
    }

    /// Returns `true` if this is a tag ref.
    pub fn is_tag(&self) -> bool {
        self.kind == GitRefKind::Tag
    }

    /// Returns `true` if this is a remote-tracking ref.
    pub fn is_remote(&self) -> bool {
        self.kind == GitRefKind::Remote
    }
}

impl fmt::Display for GitRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sym) = &self.symbolic_target {
            write!(f, "{} -> {} ({})", self.ref_path, sym, self.target_sha)
        } else {
            write!(f, "{} -> {}", self.ref_path, self.target_sha)
        }
    }
}

// ---------------------------------------------------------------------------
// RefMapping
// ---------------------------------------------------------------------------

/// A mapping between a Worktree branch/snapshot and a Git ref.
///
/// This enables bidirectional synchronization between Worktree's
/// branch/snapshot model and Git's ref-based model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefMapping {
    /// The Worktree branch ID this ref maps to, if applicable.
    pub branch_id: Option<BranchId>,

    /// The Worktree snapshot ID this ref currently points to.
    pub snapshot_id: SnapshotId,

    /// The corresponding Git ref.
    pub git_ref: GitRef,
}

impl RefMapping {
    /// Create a new ref mapping for a branch.
    pub fn for_branch(branch_id: BranchId, snapshot_id: SnapshotId, git_ref: GitRef) -> Self {
        Self {
            branch_id: Some(branch_id),
            snapshot_id,
            git_ref,
        }
    }

    /// Create a new ref mapping for a snapshot (e.g. a tag pointing to a snapshot).
    pub fn for_snapshot(snapshot_id: SnapshotId, git_ref: GitRef) -> Self {
        Self {
            branch_id: None,
            snapshot_id,
            git_ref,
        }
    }

    /// Returns the short name of the Git ref.
    pub fn short_name(&self) -> &str {
        self.git_ref.short_name()
    }

    /// Returns `true` if this mapping is for a branch.
    pub fn is_branch_mapping(&self) -> bool {
        self.branch_id.is_some()
    }

    /// Returns `true` if this mapping is for a standalone snapshot (e.g. a tag).
    pub fn is_snapshot_mapping(&self) -> bool {
        self.branch_id.is_none()
    }
}

impl fmt::Display for RefMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(bid) = &self.branch_id {
            write!(
                f,
                "branch:{} snapshot:{} <-> {}",
                bid, self.snapshot_id, self.git_ref
            )
        } else {
            write!(
                f,
                "snapshot:{} <-> {}",
                self.snapshot_id, self.git_ref
            )
        }
    }
}

// ---------------------------------------------------------------------------
// RefMappingIndex
// ---------------------------------------------------------------------------

/// An in-memory index for looking up ref mappings by various keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefMappingIndex {
    /// Mappings indexed by Git ref path.
    by_ref_path: HashMap<String, RefMapping>,
    /// Mappings indexed by Worktree branch ID.
    by_branch_id: HashMap<BranchId, RefMapping>,
    /// Mappings indexed by Worktree snapshot ID. Note that multiple refs
    /// can point to the same snapshot, so this stores the last-inserted one.
    by_snapshot_id: HashMap<SnapshotId, RefMapping>,
}

impl RefMappingIndex {
    /// Create a new, empty index.
    pub fn new() -> Self {
        Self {
            by_ref_path: HashMap::new(),
            by_branch_id: HashMap::new(),
            by_snapshot_id: HashMap::new(),
        }
    }

    /// Insert a ref mapping into the index.
    pub fn insert(&mut self, mapping: RefMapping) {
        self.by_ref_path
            .insert(mapping.git_ref.ref_path.clone(), mapping.clone());
        if let Some(bid) = mapping.branch_id {
            self.by_branch_id.insert(bid, mapping.clone());
        }
        self.by_snapshot_id
            .insert(mapping.snapshot_id, mapping);
    }

    /// Look up a mapping by its Git ref path.
    pub fn get_by_ref_path(&self, ref_path: &str) -> Option<&RefMapping> {
        self.by_ref_path.get(ref_path)
    }

    /// Look up a mapping by its Worktree branch ID.
    pub fn get_by_branch_id(&self, branch_id: &BranchId) -> Option<&RefMapping> {
        self.by_branch_id.get(branch_id)
    }

    /// Look up a mapping by its Worktree snapshot ID.
    pub fn get_by_snapshot_id(&self, snapshot_id: &SnapshotId) -> Option<&RefMapping> {
        self.by_snapshot_id.get(snapshot_id)
    }

    /// Remove a mapping by its Git ref path.
    pub fn remove_by_ref_path(&mut self, ref_path: &str) -> Option<RefMapping> {
        if let Some(mapping) = self.by_ref_path.remove(ref_path) {
            if let Some(bid) = mapping.branch_id {
                self.by_branch_id.remove(&bid);
            }
            self.by_snapshot_id.remove(&mapping.snapshot_id);
            Some(mapping)
        } else {
            None
        }
    }

    /// Remove a mapping by its Worktree branch ID.
    pub fn remove_by_branch_id(&mut self, branch_id: &BranchId) -> Option<RefMapping> {
        if let Some(mapping) = self.by_branch_id.remove(branch_id) {
            self.by_ref_path.remove(&mapping.git_ref.ref_path);
            self.by_snapshot_id.remove(&mapping.snapshot_id);
            Some(mapping)
        } else {
            None
        }
    }

    /// Returns `true` if the index contains a mapping for the given ref path.
    pub fn contains_ref_path(&self, ref_path: &str) -> bool {
        self.by_ref_path.contains_key(ref_path)
    }

    /// Returns `true` if the index contains a mapping for the given branch ID.
    pub fn contains_branch_id(&self, branch_id: &BranchId) -> bool {
        self.by_branch_id.contains_key(branch_id)
    }

    /// Returns the number of mappings in the index.
    pub fn len(&self) -> usize {
        self.by_ref_path.len()
    }

    /// Returns `true` if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.by_ref_path.is_empty()
    }

    /// Iterate over all mappings.
    pub fn iter(&self) -> impl Iterator<Item = &RefMapping> {
        self.by_ref_path.values()
    }

    /// Return all branch mappings.
    pub fn branch_mappings(&self) -> Vec<&RefMapping> {
        self.by_ref_path
            .values()
            .filter(|m| m.is_branch_mapping())
            .collect()
    }

    /// Return all tag/snapshot mappings (no branch associated).
    pub fn tag_mappings(&self) -> Vec<&RefMapping> {
        self.by_ref_path
            .values()
            .filter(|m| m.is_snapshot_mapping())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SHA: &str = "aabbccdd00112233445566778899aabbccddeeff";
    const SAMPLE_SHA2: &str = "1111111111111111111111111111111111111111";

    // ── GitRefKind ──────────────────────────────────────────────────────

    #[test]
    fn test_ref_kind_display() {
        assert_eq!(GitRefKind::Branch.to_string(), "branch");
        assert_eq!(GitRefKind::Tag.to_string(), "tag");
        assert_eq!(GitRefKind::Remote.to_string(), "remote");
        assert_eq!(GitRefKind::Symbolic.to_string(), "symbolic");
        assert_eq!(GitRefKind::Note.to_string(), "note");
        assert_eq!(GitRefKind::Other.to_string(), "other");
    }

    #[test]
    fn test_ref_kind_as_str() {
        let kinds = [
            GitRefKind::Branch,
            GitRefKind::Tag,
            GitRefKind::Remote,
            GitRefKind::Symbolic,
            GitRefKind::Note,
            GitRefKind::Other,
        ];
        for kind in &kinds {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn test_ref_kind_from_ref_path() {
        assert_eq!(
            GitRefKind::from_ref_path("refs/heads/main"),
            GitRefKind::Branch
        );
        assert_eq!(
            GitRefKind::from_ref_path("refs/heads/feature/login"),
            GitRefKind::Branch
        );
        assert_eq!(
            GitRefKind::from_ref_path("refs/tags/v1.0"),
            GitRefKind::Tag
        );
        assert_eq!(
            GitRefKind::from_ref_path("refs/remotes/origin/main"),
            GitRefKind::Remote
        );
        assert_eq!(GitRefKind::from_ref_path("HEAD"), GitRefKind::Symbolic);
        assert_eq!(
            GitRefKind::from_ref_path("refs/notes/commits"),
            GitRefKind::Note
        );
        assert_eq!(
            GitRefKind::from_ref_path("refs/stash"),
            GitRefKind::Other
        );
        assert_eq!(
            GitRefKind::from_ref_path("something/else"),
            GitRefKind::Other
        );
    }

    #[test]
    fn test_ref_kind_ref_prefix() {
        assert_eq!(GitRefKind::Branch.ref_prefix(), "refs/heads/");
        assert_eq!(GitRefKind::Tag.ref_prefix(), "refs/tags/");
        assert_eq!(GitRefKind::Remote.ref_prefix(), "refs/remotes/");
        assert_eq!(GitRefKind::Symbolic.ref_prefix(), "");
        assert_eq!(GitRefKind::Note.ref_prefix(), "refs/notes/");
        assert_eq!(GitRefKind::Other.ref_prefix(), "refs/");
    }

    #[test]
    fn test_ref_kind_serde_roundtrip() {
        for kind in &[
            GitRefKind::Branch,
            GitRefKind::Tag,
            GitRefKind::Remote,
            GitRefKind::Symbolic,
            GitRefKind::Note,
            GitRefKind::Other,
        ] {
            let json = serde_json::to_string(kind).expect("serialize");
            let deserialized: GitRefKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn test_ref_kind_copy_and_hash() {
        use std::collections::HashSet;

        let k = GitRefKind::Branch;
        let k2 = k;
        assert_eq!(k, k2);

        let mut set = HashSet::new();
        set.insert(GitRefKind::Branch);
        set.insert(GitRefKind::Branch);
        set.insert(GitRefKind::Tag);
        assert_eq!(set.len(), 2);
    }

    // ── GitRef ──────────────────────────────────────────────────────────

    #[test]
    fn test_git_ref_new() {
        let r = GitRef::new("refs/heads/main", SAMPLE_SHA);
        assert_eq!(r.ref_path, "refs/heads/main");
        assert_eq!(r.target_sha, SAMPLE_SHA);
        assert_eq!(r.kind, GitRefKind::Branch);
        assert!(r.symbolic_target.is_none());
        assert!(!r.is_symbolic());
        assert!(r.is_branch());
        assert!(!r.is_tag());
        assert!(!r.is_remote());
    }

    #[test]
    fn test_git_ref_new_infers_kind() {
        assert_eq!(
            GitRef::new("refs/heads/dev", SAMPLE_SHA).kind,
            GitRefKind::Branch
        );
        assert_eq!(
            GitRef::new("refs/tags/v2.0", SAMPLE_SHA).kind,
            GitRefKind::Tag
        );
        assert_eq!(
            GitRef::new("refs/remotes/origin/main", SAMPLE_SHA).kind,
            GitRefKind::Remote
        );
        assert_eq!(
            GitRef::new("HEAD", SAMPLE_SHA).kind,
            GitRefKind::Symbolic
        );
        assert_eq!(
            GitRef::new("refs/stash", SAMPLE_SHA).kind,
            GitRefKind::Other
        );
    }

    #[test]
    fn test_git_ref_symbolic() {
        let r = GitRef::symbolic("HEAD", SAMPLE_SHA, "refs/heads/main");
        assert!(r.is_symbolic());
        assert_eq!(r.kind, GitRefKind::Symbolic);
        assert_eq!(r.symbolic_target.as_deref(), Some("refs/heads/main"));
    }

    #[test]
    fn test_git_ref_branch() {
        let r = GitRef::branch("feature/login", SAMPLE_SHA);
        assert_eq!(r.ref_path, "refs/heads/feature/login");
        assert_eq!(r.kind, GitRefKind::Branch);
        assert!(r.is_branch());
        assert_eq!(r.short_name(), "feature/login");
    }

    #[test]
    fn test_git_ref_tag() {
        let r = GitRef::tag("v1.0.0", SAMPLE_SHA);
        assert_eq!(r.ref_path, "refs/tags/v1.0.0");
        assert_eq!(r.kind, GitRefKind::Tag);
        assert!(r.is_tag());
        assert_eq!(r.short_name(), "v1.0.0");
    }

    #[test]
    fn test_git_ref_short_name() {
        assert_eq!(
            GitRef::new("refs/heads/main", SAMPLE_SHA).short_name(),
            "main"
        );
        assert_eq!(
            GitRef::new("refs/tags/v1.0", SAMPLE_SHA).short_name(),
            "v1.0"
        );
        assert_eq!(
            GitRef::new("refs/remotes/origin/main", SAMPLE_SHA).short_name(),
            "origin/main"
        );
        assert_eq!(
            GitRef::new("HEAD", SAMPLE_SHA).short_name(),
            "HEAD"
        );
        assert_eq!(
            GitRef::new("refs/notes/commits", SAMPLE_SHA).short_name(),
            "commits"
        );
    }

    #[test]
    fn test_git_ref_is_valid_target_sha() {
        assert!(GitRef::new("refs/heads/main", SAMPLE_SHA).is_valid_target_sha());
        assert!(!GitRef::new("refs/heads/main", "short").is_valid_target_sha());
        assert!(
            !GitRef::new(
                "refs/heads/main",
                "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
            )
            .is_valid_target_sha()
        );
    }

    #[test]
    fn test_git_ref_display() {
        let r = GitRef::new("refs/heads/main", SAMPLE_SHA);
        let display = r.to_string();
        assert!(display.contains("refs/heads/main"));
        assert!(display.contains(SAMPLE_SHA));

        let sym = GitRef::symbolic("HEAD", SAMPLE_SHA, "refs/heads/main");
        let display = sym.to_string();
        assert!(display.contains("HEAD"));
        assert!(display.contains("refs/heads/main"));
        assert!(display.contains(SAMPLE_SHA));
    }

    #[test]
    fn test_git_ref_serde_json_roundtrip() {
        let r = GitRef::symbolic("HEAD", SAMPLE_SHA, "refs/heads/main");
        let json = serde_json::to_string(&r).expect("serialize");
        let deserialized: GitRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(r, deserialized);
    }

    #[test]
    fn test_git_ref_serde_bincode_roundtrip() {
        let r = GitRef::branch("feature/x", SAMPLE_SHA);
        let encoded = bincode::serialize(&r).expect("serialize");
        let decoded: GitRef = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(r, decoded);
    }

    #[test]
    fn test_git_ref_clone_and_equality() {
        let r = GitRef::new("refs/heads/main", SAMPLE_SHA);
        let cloned = r.clone();
        assert_eq!(r, cloned);
    }

    #[test]
    fn test_git_ref_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        let r1 = GitRef::new("refs/heads/main", SAMPLE_SHA);
        let r2 = GitRef::new("refs/heads/main", SAMPLE_SHA);
        let r3 = GitRef::new("refs/heads/dev", SAMPLE_SHA);

        set.insert(r1.clone());
        set.insert(r2);
        assert_eq!(set.len(), 1);

        set.insert(r3);
        assert_eq!(set.len(), 2);
    }

    // ── RefMapping ──────────────────────────────────────────────────────

    #[test]
    fn test_ref_mapping_for_branch() {
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);

        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref.clone());

        assert_eq!(mapping.branch_id, Some(branch_id));
        assert_eq!(mapping.snapshot_id, snapshot_id);
        assert_eq!(mapping.git_ref, git_ref);
        assert!(mapping.is_branch_mapping());
        assert!(!mapping.is_snapshot_mapping());
        assert_eq!(mapping.short_name(), "main");
    }

    #[test]
    fn test_ref_mapping_for_snapshot() {
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::tag("v1.0", SAMPLE_SHA);

        let mapping = RefMapping::for_snapshot(snapshot_id, git_ref.clone());

        assert_eq!(mapping.branch_id, None);
        assert_eq!(mapping.snapshot_id, snapshot_id);
        assert_eq!(mapping.git_ref, git_ref);
        assert!(!mapping.is_branch_mapping());
        assert!(mapping.is_snapshot_mapping());
        assert_eq!(mapping.short_name(), "v1.0");
    }

    #[test]
    fn test_ref_mapping_display_branch() {
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);

        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);
        let display = mapping.to_string();
        assert!(display.contains("branch:"));
        assert!(display.contains("snapshot:"));
        assert!(display.contains("refs/heads/main"));
    }

    #[test]
    fn test_ref_mapping_display_snapshot() {
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::tag("v2.0", SAMPLE_SHA);

        let mapping = RefMapping::for_snapshot(snapshot_id, git_ref);
        let display = mapping.to_string();
        assert!(!display.contains("branch:"));
        assert!(display.contains("snapshot:"));
        assert!(display.contains("refs/tags/v2.0"));
    }

    #[test]
    fn test_ref_mapping_serde_json_roundtrip() {
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        let json = serde_json::to_string(&mapping).expect("serialize");
        let deserialized: RefMapping = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mapping, deserialized);
    }

    #[test]
    fn test_ref_mapping_serde_bincode_roundtrip() {
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::tag("release", SAMPLE_SHA);
        let mapping = RefMapping::for_snapshot(snapshot_id, git_ref);

        let encoded = bincode::serialize(&mapping).expect("serialize");
        let decoded: RefMapping = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(mapping, decoded);
    }

    // ── RefMappingIndex ─────────────────────────────────────────────────

    #[test]
    fn test_index_new_is_empty() {
        let index = RefMappingIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_index_default() {
        let index = RefMappingIndex::default();
        assert!(index.is_empty());
    }

    #[test]
    fn test_index_insert_and_lookup_by_ref_path() {
        let mut index = RefMappingIndex::new();
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        index.insert(mapping.clone());
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());

        let found = index.get_by_ref_path("refs/heads/main");
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &mapping);
    }

    #[test]
    fn test_index_lookup_by_branch_id() {
        let mut index = RefMappingIndex::new();
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("develop", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        index.insert(mapping.clone());

        let found = index.get_by_branch_id(&branch_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &mapping);
    }

    #[test]
    fn test_index_lookup_by_snapshot_id() {
        let mut index = RefMappingIndex::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::tag("v1.0", SAMPLE_SHA);
        let mapping = RefMapping::for_snapshot(snapshot_id, git_ref);

        index.insert(mapping.clone());

        let found = index.get_by_snapshot_id(&snapshot_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &mapping);
    }

    #[test]
    fn test_index_contains() {
        let mut index = RefMappingIndex::new();
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        assert!(!index.contains_ref_path("refs/heads/main"));
        assert!(!index.contains_branch_id(&branch_id));

        index.insert(mapping);

        assert!(index.contains_ref_path("refs/heads/main"));
        assert!(index.contains_branch_id(&branch_id));
    }

    #[test]
    fn test_index_remove_by_ref_path() {
        let mut index = RefMappingIndex::new();
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("main", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        index.insert(mapping.clone());
        assert_eq!(index.len(), 1);

        let removed = index.remove_by_ref_path("refs/heads/main");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), mapping);
        assert!(index.is_empty());
        assert!(!index.contains_branch_id(&branch_id));
    }

    #[test]
    fn test_index_remove_by_branch_id() {
        let mut index = RefMappingIndex::new();
        let branch_id = BranchId::new();
        let snapshot_id = SnapshotId::new();
        let git_ref = GitRef::branch("feature/x", SAMPLE_SHA);
        let mapping = RefMapping::for_branch(branch_id, snapshot_id, git_ref);

        index.insert(mapping.clone());

        let removed = index.remove_by_branch_id(&branch_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), mapping);
        assert!(index.is_empty());
        assert!(!index.contains_ref_path("refs/heads/feature/x"));
    }

    #[test]
    fn test_index_remove_nonexistent() {
        let mut index = RefMappingIndex::new();

        let removed = index.remove_by_ref_path("refs/heads/nope");
        assert!(removed.is_none());

        let removed = index.remove_by_branch_id(&BranchId::new());
        assert!(removed.is_none());
    }

    #[test]
    fn test_index_multiple_entries() {
        let mut index = RefMappingIndex::new();

        let bid1 = BranchId::new();
        let sid1 = SnapshotId::new();
        let m1 = RefMapping::for_branch(bid1, sid1, GitRef::branch("main", SAMPLE_SHA));

        let bid2 = BranchId::new();
        let sid2 = SnapshotId::new();
        let m2 = RefMapping::for_branch(bid2, sid2, GitRef::branch("develop", SAMPLE_SHA2));

        let sid3 = SnapshotId::new();
        let m3 = RefMapping::for_snapshot(sid3, GitRef::tag("v1.0", SAMPLE_SHA));

        index.insert(m1.clone());
        index.insert(m2.clone());
        index.insert(m3.clone());

        assert_eq!(index.len(), 3);

        assert_eq!(index.get_by_ref_path("refs/heads/main").unwrap(), &m1);
        assert_eq!(index.get_by_branch_id(&bid2).unwrap(), &m2);
        assert_eq!(index.get_by_snapshot_id(&sid3).unwrap(), &m3);
    }

    #[test]
    fn test_index_iter() {
        let mut index = RefMappingIndex::new();

        let bid1 = BranchId::new();
        let sid1 = SnapshotId::new();
        index.insert(RefMapping::for_branch(
            bid1,
            sid1,
            GitRef::branch("a", SAMPLE_SHA),
        ));

        let sid2 = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid2,
            GitRef::tag("b", SAMPLE_SHA),
        ));

        let all: Vec<_> = index.iter().collect();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_index_branch_mappings() {
        let mut index = RefMappingIndex::new();

        let bid = BranchId::new();
        let sid = SnapshotId::new();
        index.insert(RefMapping::for_branch(
            bid,
            sid,
            GitRef::branch("main", SAMPLE_SHA),
        ));

        let sid2 = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid2,
            GitRef::tag("v1.0", SAMPLE_SHA),
        ));

        let branches = index.branch_mappings();
        assert_eq!(branches.len(), 1);
        assert!(branches[0].is_branch_mapping());
    }

    #[test]
    fn test_index_tag_mappings() {
        let mut index = RefMappingIndex::new();

        let bid = BranchId::new();
        let sid = SnapshotId::new();
        index.insert(RefMapping::for_branch(
            bid,
            sid,
            GitRef::branch("main", SAMPLE_SHA),
        ));

        let sid2 = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid2,
            GitRef::tag("v1.0", SAMPLE_SHA),
        ));

        let sid3 = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid3,
            GitRef::tag("v2.0", SAMPLE_SHA2),
        ));

        let tags = index.tag_mappings();
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().all(|m| m.is_snapshot_mapping()));
    }

    #[test]
    fn test_index_snapshot_mapping_not_indexed_by_branch() {
        let mut index = RefMappingIndex::new();
        let sid = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid,
            GitRef::tag("v1.0", SAMPLE_SHA),
        ));

        // Should not be findable by any branch ID since it's a snapshot mapping.
        let random_bid = BranchId::new();
        assert!(index.get_by_branch_id(&random_bid).is_none());
    }

    #[test]
    fn test_index_serde_json_roundtrip() {
        let mut index = RefMappingIndex::new();

        let bid = BranchId::new();
        let sid = SnapshotId::new();
        index.insert(RefMapping::for_branch(
            bid,
            sid,
            GitRef::branch("main", SAMPLE_SHA),
        ));

        let sid2 = SnapshotId::new();
        index.insert(RefMapping::for_snapshot(
            sid2,
            GitRef::tag("v1.0", SAMPLE_SHA2),
        ));

        let json = serde_json::to_string(&index).expect("serialize");
        let deserialized: RefMappingIndex =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.len(), 2);
        assert!(deserialized.contains_ref_path("refs/heads/main"));
        assert!(deserialized.contains_ref_path("refs/tags/v1.0"));
        assert!(deserialized.contains_branch_id(&bid));
    }

    #[test]
    fn test_lookup_nonexistent_returns_none() {
        let index = RefMappingIndex::new();

        assert!(index.get_by_ref_path("refs/heads/nope").is_none());
        assert!(index.get_by_branch_id(&BranchId::new()).is_none());
        assert!(index.get_by_snapshot_id(&SnapshotId::new()).is_none());
    }
}
