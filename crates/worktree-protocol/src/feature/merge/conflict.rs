//! Merge conflict representation.
//!
//! Provides [`MergeConflict`], [`ConflictKind`], and [`ConflictSide`] for
//! describing conflicts that arise during merge operations when the same
//! path has been modified in incompatible ways on both sides.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use crate::core::hash::ContentHash;

// ---------------------------------------------------------------------------
// ConflictKind
// ---------------------------------------------------------------------------

/// The kind of conflict that occurred during a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConflictKind {
    /// Both sides modified the same file with different content.
    ContentConflict,

    /// One side modified a file while the other side deleted it.
    ModifyDelete,

    /// One side deleted a file while the other side modified it.
    DeleteModify,

    /// Both sides added a file at the same path with different content.
    AddAdd,

    /// Both sides renamed different source files to the same destination path.
    RenameRename,

    /// One side renamed a file while the other side modified the original path.
    RenameModify,

    /// One side renamed a file while the other side deleted the original path.
    RenameDelete,

    /// A directory on one side conflicts with a file on the other side.
    DirectoryFileConflict,

    /// Both sides changed the file mode (e.g. executable flag) differently.
    ModeConflict,
}

impl ConflictKind {
    /// Returns the stable string representation of this conflict kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictKind::ContentConflict => "content-conflict",
            ConflictKind::ModifyDelete => "modify-delete",
            ConflictKind::DeleteModify => "delete-modify",
            ConflictKind::AddAdd => "add-add",
            ConflictKind::RenameRename => "rename-rename",
            ConflictKind::RenameModify => "rename-modify",
            ConflictKind::RenameDelete => "rename-delete",
            ConflictKind::DirectoryFileConflict => "directory-file",
            ConflictKind::ModeConflict => "mode-conflict",
        }
    }

    /// Returns `true` if this conflict involves content differences.
    pub fn is_content_based(&self) -> bool {
        matches!(
            self,
            ConflictKind::ContentConflict | ConflictKind::AddAdd
        )
    }

    /// Returns `true` if this conflict involves a deletion on one side.
    pub fn involves_deletion(&self) -> bool {
        matches!(
            self,
            ConflictKind::ModifyDelete
                | ConflictKind::DeleteModify
                | ConflictKind::RenameDelete
        )
    }

    /// Returns `true` if this conflict involves a rename on one or both sides.
    pub fn involves_rename(&self) -> bool {
        matches!(
            self,
            ConflictKind::RenameRename
                | ConflictKind::RenameModify
                | ConflictKind::RenameDelete
        )
    }
}

impl fmt::Display for ConflictKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// ConflictSide
// ---------------------------------------------------------------------------

/// Describes one side of a merge conflict (either "ours" or "theirs").
///
/// Contains the content hash and size for the version of the file on that
/// side of the merge, if the file exists.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConflictSide {
    /// The content hash on this side, or `None` if the file was deleted /
    /// does not exist on this side.
    pub hash: Option<ContentHash>,

    /// The file size on this side, or `None` if the file was deleted /
    /// does not exist on this side.
    pub size: Option<u64>,

    /// The original path if this side involved a rename, or `None` if it
    /// was not renamed.
    pub original_path: Option<PathBuf>,
}

impl ConflictSide {
    /// Create a new conflict side with a hash and size.
    pub fn new(hash: Option<ContentHash>, size: Option<u64>) -> Self {
        Self {
            hash,
            size,
            original_path: None,
        }
    }

    /// Create a conflict side representing a deleted / non-existent file.
    pub fn absent() -> Self {
        Self {
            hash: None,
            size: None,
            original_path: None,
        }
    }

    /// Create a conflict side with rename information.
    pub fn with_rename(
        hash: Option<ContentHash>,
        size: Option<u64>,
        original_path: PathBuf,
    ) -> Self {
        Self {
            hash,
            size,
            original_path: Some(original_path),
        }
    }

    /// Returns `true` if this side has content (i.e., the file exists).
    pub fn is_present(&self) -> bool {
        self.hash.is_some()
    }

    /// Returns `true` if this side has no content (i.e., the file was deleted
    /// or does not exist).
    pub fn is_absent(&self) -> bool {
        self.hash.is_none()
    }

    /// Returns `true` if this side involved a rename.
    pub fn is_renamed(&self) -> bool {
        self.original_path.is_some()
    }
}

impl fmt::Display for ConflictSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.hash, &self.original_path) {
            (Some(hash), Some(orig)) => write!(f, "{} (renamed from {})", hash, orig.display()),
            (Some(hash), None) => write!(f, "{}", hash),
            (None, _) => write!(f, "<absent>"),
        }
    }
}

// ---------------------------------------------------------------------------
// MergeConflict
// ---------------------------------------------------------------------------

/// A single merge conflict at a specific path.
///
/// Describes the conflicting state on both sides of the merge ("ours" and
/// "theirs") along with the kind of conflict and optional base (ancestor)
/// information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflict {
    /// The path where the conflict occurred.
    pub path: PathBuf,

    /// The kind of conflict.
    pub kind: ConflictKind,

    /// The state on the "ours" (target / current branch) side.
    pub ours: ConflictSide,

    /// The state on the "theirs" (source / incoming branch) side.
    pub theirs: ConflictSide,

    /// The state at the common ancestor (base), if known.
    pub base: Option<ConflictSide>,

    /// Whether this conflict has been resolved (e.g. by the user choosing
    /// a side or providing a manual resolution).
    pub resolved: bool,

    /// The resolution hash, if the conflict was resolved. This is the
    /// content hash of the resolved version.
    pub resolution_hash: Option<ContentHash>,

    /// Optional human-readable message describing the conflict.
    pub message: String,
}

impl MergeConflict {
    /// Create a new merge conflict.
    pub fn new(
        path: PathBuf,
        kind: ConflictKind,
        ours: ConflictSide,
        theirs: ConflictSide,
    ) -> Self {
        Self {
            path,
            kind,
            ours,
            theirs,
            base: None,
            resolved: false,
            resolution_hash: None,
            message: String::new(),
        }
    }

    /// Set the base (ancestor) side.
    pub fn with_base(mut self, base: ConflictSide) -> Self {
        self.base = Some(base);
        self
    }

    /// Set a human-readable message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Mark this conflict as resolved with the given content hash.
    pub fn resolve(&mut self, resolution_hash: ContentHash) {
        self.resolved = true;
        self.resolution_hash = Some(resolution_hash);
    }

    /// Resolve this conflict by choosing the "ours" side.
    ///
    /// Returns `true` if the ours side had content to use as the resolution.
    pub fn resolve_ours(&mut self) -> bool {
        if let Some(hash) = self.ours.hash {
            self.resolve(hash);
            true
        } else {
            false
        }
    }

    /// Resolve this conflict by choosing the "theirs" side.
    ///
    /// Returns `true` if the theirs side had content to use as the resolution.
    pub fn resolve_theirs(&mut self) -> bool {
        if let Some(hash) = self.theirs.hash {
            self.resolve(hash);
            true
        } else {
            false
        }
    }

    /// Returns `true` if the conflict has been resolved.
    pub fn is_resolved(&self) -> bool {
        self.resolved
    }

    /// Returns `true` if the conflict is still unresolved.
    pub fn is_unresolved(&self) -> bool {
        !self.resolved
    }

    /// Create a content conflict at the given path.
    pub fn content_conflict(
        path: impl Into<PathBuf>,
        ours_hash: ContentHash,
        ours_size: u64,
        theirs_hash: ContentHash,
        theirs_size: u64,
    ) -> Self {
        Self::new(
            path.into(),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(ours_size)),
            ConflictSide::new(Some(theirs_hash), Some(theirs_size)),
        )
    }

    /// Create a modify-delete conflict at the given path.
    pub fn modify_delete(
        path: impl Into<PathBuf>,
        modified_hash: ContentHash,
        modified_size: u64,
    ) -> Self {
        Self::new(
            path.into(),
            ConflictKind::ModifyDelete,
            ConflictSide::new(Some(modified_hash), Some(modified_size)),
            ConflictSide::absent(),
        )
    }

    /// Create a delete-modify conflict at the given path.
    pub fn delete_modify(
        path: impl Into<PathBuf>,
        modified_hash: ContentHash,
        modified_size: u64,
    ) -> Self {
        Self::new(
            path.into(),
            ConflictKind::DeleteModify,
            ConflictSide::absent(),
            ConflictSide::new(Some(modified_hash), Some(modified_size)),
        )
    }

    /// Create an add-add conflict at the given path.
    pub fn add_add(
        path: impl Into<PathBuf>,
        ours_hash: ContentHash,
        ours_size: u64,
        theirs_hash: ContentHash,
        theirs_size: u64,
    ) -> Self {
        Self::new(
            path.into(),
            ConflictKind::AddAdd,
            ConflictSide::new(Some(ours_hash), Some(ours_size)),
            ConflictSide::new(Some(theirs_hash), Some(theirs_size)),
        )
    }
}

impl fmt::Display for MergeConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "conflict at {}: {} (ours={}, theirs={})",
            self.path.display(),
            self.kind,
            self.ours,
            self.theirs,
        )?;
        if self.resolved {
            write!(f, " [resolved]")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;

    // ── ConflictKind tests ──────────────────────────────────────────────

    #[test]
    fn test_conflict_kind_display() {
        assert_eq!(ConflictKind::ContentConflict.to_string(), "content-conflict");
        assert_eq!(ConflictKind::ModifyDelete.to_string(), "modify-delete");
        assert_eq!(ConflictKind::DeleteModify.to_string(), "delete-modify");
        assert_eq!(ConflictKind::AddAdd.to_string(), "add-add");
        assert_eq!(ConflictKind::RenameRename.to_string(), "rename-rename");
        assert_eq!(ConflictKind::RenameModify.to_string(), "rename-modify");
        assert_eq!(ConflictKind::RenameDelete.to_string(), "rename-delete");
        assert_eq!(
            ConflictKind::DirectoryFileConflict.to_string(),
            "directory-file"
        );
        assert_eq!(ConflictKind::ModeConflict.to_string(), "mode-conflict");
    }

    #[test]
    fn test_conflict_kind_as_str() {
        let kinds = [
            ConflictKind::ContentConflict,
            ConflictKind::ModifyDelete,
            ConflictKind::DeleteModify,
            ConflictKind::AddAdd,
            ConflictKind::RenameRename,
            ConflictKind::RenameModify,
            ConflictKind::RenameDelete,
            ConflictKind::DirectoryFileConflict,
            ConflictKind::ModeConflict,
        ];
        for kind in &kinds {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn test_is_content_based() {
        assert!(ConflictKind::ContentConflict.is_content_based());
        assert!(ConflictKind::AddAdd.is_content_based());
        assert!(!ConflictKind::ModifyDelete.is_content_based());
        assert!(!ConflictKind::RenameRename.is_content_based());
        assert!(!ConflictKind::ModeConflict.is_content_based());
    }

    #[test]
    fn test_involves_deletion() {
        assert!(ConflictKind::ModifyDelete.involves_deletion());
        assert!(ConflictKind::DeleteModify.involves_deletion());
        assert!(ConflictKind::RenameDelete.involves_deletion());
        assert!(!ConflictKind::ContentConflict.involves_deletion());
        assert!(!ConflictKind::AddAdd.involves_deletion());
        assert!(!ConflictKind::RenameRename.involves_deletion());
    }

    #[test]
    fn test_involves_rename() {
        assert!(ConflictKind::RenameRename.involves_rename());
        assert!(ConflictKind::RenameModify.involves_rename());
        assert!(ConflictKind::RenameDelete.involves_rename());
        assert!(!ConflictKind::ContentConflict.involves_rename());
        assert!(!ConflictKind::ModifyDelete.involves_rename());
        assert!(!ConflictKind::AddAdd.involves_rename());
    }

    #[test]
    fn test_conflict_kind_serde_roundtrip() {
        let kinds = [
            ConflictKind::ContentConflict,
            ConflictKind::ModifyDelete,
            ConflictKind::DeleteModify,
            ConflictKind::AddAdd,
            ConflictKind::RenameRename,
            ConflictKind::RenameModify,
            ConflictKind::RenameDelete,
            ConflictKind::DirectoryFileConflict,
            ConflictKind::ModeConflict,
        ];
        for kind in &kinds {
            let json = serde_json::to_string(kind).expect("serialize");
            let deserialized: ConflictKind =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn test_conflict_kind_copy_and_hash() {
        use std::collections::HashSet;

        let k = ConflictKind::ContentConflict;
        let k2 = k; // Copy
        assert_eq!(k, k2);

        let mut set = HashSet::new();
        set.insert(ConflictKind::ContentConflict);
        set.insert(ConflictKind::ContentConflict);
        set.insert(ConflictKind::AddAdd);
        assert_eq!(set.len(), 2);
    }

    // ── ConflictSide tests ──────────────────────────────────────────────

    #[test]
    fn test_conflict_side_new() {
        let hash = hash_bytes(b"side");
        let side = ConflictSide::new(Some(hash), Some(4));

        assert!(side.is_present());
        assert!(!side.is_absent());
        assert!(!side.is_renamed());
        assert_eq!(side.hash, Some(hash));
        assert_eq!(side.size, Some(4));
        assert!(side.original_path.is_none());
    }

    #[test]
    fn test_conflict_side_absent() {
        let side = ConflictSide::absent();

        assert!(!side.is_present());
        assert!(side.is_absent());
        assert!(!side.is_renamed());
        assert!(side.hash.is_none());
        assert!(side.size.is_none());
    }

    #[test]
    fn test_conflict_side_with_rename() {
        let hash = hash_bytes(b"renamed");
        let side = ConflictSide::with_rename(
            Some(hash),
            Some(7),
            PathBuf::from("old_name.rs"),
        );

        assert!(side.is_present());
        assert!(side.is_renamed());
        assert_eq!(side.original_path.as_deref(), Some(std::path::Path::new("old_name.rs")));
    }

    #[test]
    fn test_conflict_side_display_present() {
        let hash = hash_bytes(b"display");
        let side = ConflictSide::new(Some(hash), Some(7));
        let display = side.to_string();
        assert_eq!(display, hash.to_string());
    }

    #[test]
    fn test_conflict_side_display_absent() {
        let side = ConflictSide::absent();
        assert_eq!(side.to_string(), "<absent>");
    }

    #[test]
    fn test_conflict_side_display_renamed() {
        let hash = hash_bytes(b"renamed display");
        let side = ConflictSide::with_rename(
            Some(hash),
            Some(15),
            PathBuf::from("original.txt"),
        );
        let display = side.to_string();
        assert!(display.contains(&hash.to_string()));
        assert!(display.contains("renamed from"));
        assert!(display.contains("original.txt"));
    }

    #[test]
    fn test_conflict_side_serde_roundtrip() {
        let hash = hash_bytes(b"serde side");
        let side = ConflictSide::with_rename(
            Some(hash),
            Some(10),
            PathBuf::from("old.rs"),
        );

        let json = serde_json::to_string(&side).expect("serialize");
        let deserialized: ConflictSide = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(side, deserialized);
    }

    #[test]
    fn test_conflict_side_equality() {
        let h1 = hash_bytes(b"a");
        let h2 = hash_bytes(b"b");

        let s1 = ConflictSide::new(Some(h1), Some(1));
        let s2 = ConflictSide::new(Some(h1), Some(1));
        let s3 = ConflictSide::new(Some(h2), Some(1));

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    // ── MergeConflict tests ─────────────────────────────────────────────

    #[test]
    fn test_merge_conflict_new() {
        let ours_hash = hash_bytes(b"ours");
        let theirs_hash = hash_bytes(b"theirs");

        let conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(4)),
            ConflictSide::new(Some(theirs_hash), Some(6)),
        );

        assert_eq!(conflict.path, PathBuf::from("file.txt"));
        assert_eq!(conflict.kind, ConflictKind::ContentConflict);
        assert!(conflict.is_unresolved());
        assert!(!conflict.is_resolved());
        assert!(conflict.base.is_none());
        assert!(conflict.resolution_hash.is_none());
        assert!(conflict.message.is_empty());
    }

    #[test]
    fn test_merge_conflict_with_base() {
        let base_hash = hash_bytes(b"base");
        let conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"ours")), Some(4)),
            ConflictSide::new(Some(hash_bytes(b"theirs")), Some(6)),
        )
        .with_base(ConflictSide::new(Some(base_hash), Some(4)));

        assert!(conflict.base.is_some());
        assert_eq!(conflict.base.unwrap().hash, Some(base_hash));
    }

    #[test]
    fn test_merge_conflict_with_message() {
        let conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(hash_bytes(b"ours")), Some(4)),
            ConflictSide::new(Some(hash_bytes(b"theirs")), Some(6)),
        )
        .with_message("Both sides modified the same function");

        assert_eq!(conflict.message, "Both sides modified the same function");
    }

    #[test]
    fn test_merge_conflict_resolve() {
        let ours_hash = hash_bytes(b"ours");
        let theirs_hash = hash_bytes(b"theirs");
        let resolution_hash = hash_bytes(b"resolved");

        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(4)),
            ConflictSide::new(Some(theirs_hash), Some(6)),
        );

        assert!(conflict.is_unresolved());

        conflict.resolve(resolution_hash);

        assert!(conflict.is_resolved());
        assert!(!conflict.is_unresolved());
        assert_eq!(conflict.resolution_hash, Some(resolution_hash));
    }

    #[test]
    fn test_merge_conflict_resolve_ours() {
        let ours_hash = hash_bytes(b"ours");
        let theirs_hash = hash_bytes(b"theirs");

        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(4)),
            ConflictSide::new(Some(theirs_hash), Some(6)),
        );

        let success = conflict.resolve_ours();
        assert!(success);
        assert!(conflict.is_resolved());
        assert_eq!(conflict.resolution_hash, Some(ours_hash));
    }

    #[test]
    fn test_merge_conflict_resolve_theirs() {
        let ours_hash = hash_bytes(b"ours");
        let theirs_hash = hash_bytes(b"theirs");

        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(4)),
            ConflictSide::new(Some(theirs_hash), Some(6)),
        );

        let success = conflict.resolve_theirs();
        assert!(success);
        assert!(conflict.is_resolved());
        assert_eq!(conflict.resolution_hash, Some(theirs_hash));
    }

    #[test]
    fn test_merge_conflict_resolve_ours_absent() {
        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::DeleteModify,
            ConflictSide::absent(),
            ConflictSide::new(Some(hash_bytes(b"theirs")), Some(6)),
        );

        let success = conflict.resolve_ours();
        assert!(!success);
        assert!(conflict.is_unresolved());
    }

    #[test]
    fn test_merge_conflict_resolve_theirs_absent() {
        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ModifyDelete,
            ConflictSide::new(Some(hash_bytes(b"ours")), Some(4)),
            ConflictSide::absent(),
        );

        let success = conflict.resolve_theirs();
        assert!(!success);
        assert!(conflict.is_unresolved());
    }

    #[test]
    fn test_content_conflict_shorthand() {
        let ours = hash_bytes(b"ours");
        let theirs = hash_bytes(b"theirs");

        let conflict = MergeConflict::content_conflict("file.txt", ours, 4, theirs, 6);

        assert_eq!(conflict.path, PathBuf::from("file.txt"));
        assert_eq!(conflict.kind, ConflictKind::ContentConflict);
        assert_eq!(conflict.ours.hash, Some(ours));
        assert_eq!(conflict.ours.size, Some(4));
        assert_eq!(conflict.theirs.hash, Some(theirs));
        assert_eq!(conflict.theirs.size, Some(6));
    }

    #[test]
    fn test_modify_delete_shorthand() {
        let modified = hash_bytes(b"modified");
        let conflict = MergeConflict::modify_delete("file.txt", modified, 8);

        assert_eq!(conflict.kind, ConflictKind::ModifyDelete);
        assert!(conflict.ours.is_present());
        assert!(conflict.theirs.is_absent());
    }

    #[test]
    fn test_delete_modify_shorthand() {
        let modified = hash_bytes(b"modified");
        let conflict = MergeConflict::delete_modify("file.txt", modified, 8);

        assert_eq!(conflict.kind, ConflictKind::DeleteModify);
        assert!(conflict.ours.is_absent());
        assert!(conflict.theirs.is_present());
    }

    #[test]
    fn test_add_add_shorthand() {
        let ours = hash_bytes(b"ours_add");
        let theirs = hash_bytes(b"theirs_add");
        let conflict = MergeConflict::add_add("new.txt", ours, 9, theirs, 10);

        assert_eq!(conflict.kind, ConflictKind::AddAdd);
        assert_eq!(conflict.ours.hash, Some(ours));
        assert_eq!(conflict.theirs.hash, Some(theirs));
    }

    #[test]
    fn test_merge_conflict_display() {
        let ours = hash_bytes(b"ours");
        let theirs = hash_bytes(b"theirs");
        let conflict = MergeConflict::content_conflict("src/main.rs", ours, 4, theirs, 6);

        let display = conflict.to_string();
        assert!(display.contains("src/main.rs"));
        assert!(display.contains("content-conflict"));
        assert!(!display.contains("[resolved]"));
    }

    #[test]
    fn test_merge_conflict_display_resolved() {
        let ours = hash_bytes(b"ours");
        let theirs = hash_bytes(b"theirs");
        let mut conflict = MergeConflict::content_conflict("file.txt", ours, 4, theirs, 6);
        conflict.resolve_ours();

        let display = conflict.to_string();
        assert!(display.contains("[resolved]"));
    }

    #[test]
    fn test_merge_conflict_serde_json_roundtrip() {
        let ours_hash = hash_bytes(b"ours");
        let theirs_hash = hash_bytes(b"theirs");
        let base_hash = hash_bytes(b"base");

        let mut conflict = MergeConflict::new(
            PathBuf::from("file.txt"),
            ConflictKind::ContentConflict,
            ConflictSide::new(Some(ours_hash), Some(4)),
            ConflictSide::new(Some(theirs_hash), Some(6)),
        )
        .with_base(ConflictSide::new(Some(base_hash), Some(4)))
        .with_message("test conflict");

        conflict.resolve(hash_bytes(b"resolved"));

        let json = serde_json::to_string(&conflict).expect("serialize");
        let deserialized: MergeConflict = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(conflict.path, deserialized.path);
        assert_eq!(conflict.kind, deserialized.kind);
        assert_eq!(conflict.ours, deserialized.ours);
        assert_eq!(conflict.theirs, deserialized.theirs);
        assert_eq!(conflict.base, deserialized.base);
        assert_eq!(conflict.resolved, deserialized.resolved);
        assert_eq!(conflict.resolution_hash, deserialized.resolution_hash);
        assert_eq!(conflict.message, deserialized.message);
    }

    #[test]
    fn test_merge_conflict_serde_bincode_roundtrip() {
        let conflict = MergeConflict::content_conflict(
            "test.rs",
            hash_bytes(b"ours"),
            4,
            hash_bytes(b"theirs"),
            6,
        );

        let encoded = bincode::serialize(&conflict).expect("serialize");
        let decoded: MergeConflict = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(conflict, decoded);
    }

    #[test]
    fn test_merge_conflict_clone_and_equality() {
        let conflict = MergeConflict::content_conflict(
            "file.txt",
            hash_bytes(b"a"),
            1,
            hash_bytes(b"b"),
            1,
        );
        let cloned = conflict.clone();
        assert_eq!(conflict, cloned);
    }

    #[test]
    fn test_multiple_conflicts() {
        let c1 = MergeConflict::content_conflict(
            "a.txt",
            hash_bytes(b"a1"),
            2,
            hash_bytes(b"a2"),
            2,
        );
        let c2 = MergeConflict::modify_delete("b.txt", hash_bytes(b"b"), 1);
        let c3 = MergeConflict::add_add(
            "c.txt",
            hash_bytes(b"c1"),
            2,
            hash_bytes(b"c2"),
            2,
        );

        let conflicts = vec![c1, c2, c3];
        assert_eq!(conflicts.len(), 3);

        let content_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| c.kind.is_content_based())
            .collect();
        assert_eq!(content_conflicts.len(), 2);

        let deletion_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| c.kind.involves_deletion())
            .collect();
        assert_eq!(deletion_conflicts.len(), 1);
    }
}
