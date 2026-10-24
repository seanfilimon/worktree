//! Patch construction and application.
//!
//! A [`Patch`] is a self-contained description of how to transform one manifest
//! into another. It is built from a list of [`Delta`]s and can be applied to a
//! [`Manifest`] to produce a new manifest state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::core::hash::ContentHash;
use crate::object::delta::{Delta, DeltaKind};
use crate::object::manifest::{EntryKind, Manifest, ManifestEntry};

// ---------------------------------------------------------------------------
// PatchApplyError
// ---------------------------------------------------------------------------

/// Errors that can occur when applying a [`Patch`] to a [`Manifest`].
#[derive(Debug, Clone, Error)]
pub enum PatchApplyError {
    /// Attempted to add a path that already exists.
    #[error("path already exists: {0}")]
    PathAlreadyExists(PathBuf),

    /// Attempted to modify or delete a path that does not exist.
    #[error("path not found: {0}")]
    PathNotFound(PathBuf),

    /// The expected old hash did not match the current manifest entry.
    #[error("hash mismatch at {path}: expected {expected}, found {found}")]
    HashMismatch {
        /// The path whose hash did not match.
        path: PathBuf,
        /// The expected old hash from the delta.
        expected: ContentHash,
        /// The actual hash found in the manifest.
        found: ContentHash,
    },

    /// Attempted to rename from a path that does not exist.
    #[error("rename source not found: {0}")]
    RenameSourceNotFound(PathBuf),

    /// Attempted to rename to a path that already exists.
    #[error("rename destination already exists: {0}")]
    RenameDestinationExists(PathBuf),

    /// Attempted to copy from a path that does not exist.
    #[error("copy source not found: {0}")]
    CopySourceNotFound(PathBuf),

    /// The patch contains conflicting operations on the same path.
    #[error("conflicting operations on path: {0}")]
    ConflictingOperations(PathBuf),
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

/// A patch that can be applied to a manifest to transform it into a new state.
///
/// The patch is essentially an ordered list of [`Delta`]s together with
/// metadata about the transformation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    /// The deltas that comprise this patch.
    pub deltas: Vec<Delta>,
    /// Optional description of the patch.
    pub description: String,
    /// The manifest hash this patch was computed against (base state).
    pub base_hash: Option<ContentHash>,
}

impl Patch {
    /// Create a new patch from a list of deltas.
    pub fn new(deltas: Vec<Delta>) -> Self {
        Self {
            deltas,
            description: String::new(),
            base_hash: None,
        }
    }

    /// Create a new patch with a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the base manifest hash for this patch.
    pub fn with_base_hash(mut self, hash: ContentHash) -> Self {
        self.base_hash = Some(hash);
        self
    }

    /// Returns `true` if the patch contains no deltas.
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }

    /// Returns the number of deltas in this patch.
    pub fn len(&self) -> usize {
        self.deltas.len()
    }

    /// Apply this patch to the given manifest, producing a new manifest.
    ///
    /// The application proceeds in phases:
    /// 1. **Validate** — check that all preconditions are met.
    /// 2. **Renames** — process rename operations (remove source, add destination).
    /// 3. **Copies** — process copy operations (add destination).
    /// 4. **Deletes** — remove entries.
    /// 5. **Adds** — insert new entries.
    /// 6. **Modifies** — update existing entries.
    ///
    /// If `verify_hashes` is `true`, old hashes in deltas are compared against
    /// the manifest and a [`PatchApplyError::HashMismatch`] is returned on
    /// mismatch.
    pub fn apply(
        &self,
        manifest: &Manifest,
        verify_hashes: bool,
    ) -> Result<Manifest, PatchApplyError> {
        // Build a mutable map of path -> entry for efficient manipulation.
        let mut entries: HashMap<PathBuf, ManifestEntry> = manifest
            .entries
            .iter()
            .map(|e| (e.path.clone(), e.clone()))
            .collect();

        // Separate deltas by kind for phased application.
        let mut renames: Vec<&Delta> = Vec::new();
        let mut copies: Vec<&Delta> = Vec::new();
        let mut deletes: Vec<&Delta> = Vec::new();
        let mut adds: Vec<&Delta> = Vec::new();
        let mut modifies: Vec<&Delta> = Vec::new();

        for delta in &self.deltas {
            match &delta.kind {
                DeltaKind::Rename { .. } => renames.push(delta),
                DeltaKind::Copy { .. } => copies.push(delta),
                DeltaKind::Delete => deletes.push(delta),
                DeltaKind::Add => adds.push(delta),
                DeltaKind::Modify => modifies.push(delta),
            }
        }

        // Phase 1: Renames
        for delta in &renames {
            if let DeltaKind::Rename { from } = &delta.kind {
                let source = entries
                    .remove(from)
                    .ok_or_else(|| PatchApplyError::RenameSourceNotFound(from.clone()))?;

                if verify_hashes {
                    if let Some(expected) = delta.old_hash {
                        if source.hash != expected {
                            return Err(PatchApplyError::HashMismatch {
                                path: from.clone(),
                                expected,
                                found: source.hash,
                            });
                        }
                    }
                }

                if entries.contains_key(&delta.path) {
                    return Err(PatchApplyError::RenameDestinationExists(
                        delta.path.clone(),
                    ));
                }

                let new_hash = delta.new_hash.unwrap_or(source.hash);
                let new_size = delta.new_size.unwrap_or(source.size);

                let new_entry = ManifestEntry {
                    path: delta.path.clone(),
                    kind: source.kind,
                    hash: new_hash,
                    size: new_size,
                    executable: source.executable,
                };
                entries.insert(delta.path.clone(), new_entry);
            }
        }

        // Phase 2: Copies
        for delta in &copies {
            if let DeltaKind::Copy { from } = &delta.kind {
                let source = entries
                    .get(from)
                    .ok_or_else(|| PatchApplyError::CopySourceNotFound(from.clone()))?
                    .clone();

                if entries.contains_key(&delta.path) {
                    return Err(PatchApplyError::PathAlreadyExists(delta.path.clone()));
                }

                let new_hash = delta.new_hash.unwrap_or(source.hash);
                let new_size = delta.new_size.unwrap_or(source.size);

                let new_entry = ManifestEntry {
                    path: delta.path.clone(),
                    kind: source.kind,
                    hash: new_hash,
                    size: new_size,
                    executable: source.executable,
                };
                entries.insert(delta.path.clone(), new_entry);
            }
        }

        // Phase 3: Deletes
        for delta in &deletes {
            let existing = entries
                .get(&delta.path)
                .ok_or_else(|| PatchApplyError::PathNotFound(delta.path.clone()))?;

            if verify_hashes {
                if let Some(expected) = delta.old_hash {
                    if existing.hash != expected {
                        return Err(PatchApplyError::HashMismatch {
                            path: delta.path.clone(),
                            expected,
                            found: existing.hash,
                        });
                    }
                }
            }

            entries.remove(&delta.path);
        }

        // Phase 4: Adds
        for delta in &adds {
            if entries.contains_key(&delta.path) {
                return Err(PatchApplyError::PathAlreadyExists(delta.path.clone()));
            }

            let hash = delta.new_hash.unwrap_or(ContentHash::ZERO);
            let size = delta.new_size.unwrap_or(0);

            let entry = ManifestEntry {
                path: delta.path.clone(),
                kind: EntryKind::File,
                hash,
                size,
                executable: false,
            };
            entries.insert(delta.path.clone(), entry);
        }

        // Phase 5: Modifies
        for delta in &modifies {
            let existing = entries
                .get(&delta.path)
                .ok_or_else(|| PatchApplyError::PathNotFound(delta.path.clone()))?;

            if verify_hashes {
                if let Some(expected) = delta.old_hash {
                    if existing.hash != expected {
                        return Err(PatchApplyError::HashMismatch {
                            path: delta.path.clone(),
                            expected,
                            found: existing.hash,
                        });
                    }
                }
            }

            let new_hash = delta.new_hash.unwrap_or(existing.hash);
            let new_size = delta.new_size.unwrap_or(existing.size);

            let updated = ManifestEntry {
                path: delta.path.clone(),
                kind: existing.kind,
                hash: new_hash,
                size: new_size,
                executable: existing.executable,
            };
            entries.insert(delta.path.clone(), updated);
        }

        // Build the result manifest.
        let mut result = Manifest::new(manifest.tree_id);
        let mut sorted_entries: Vec<ManifestEntry> = entries.into_values().collect();
        sorted_entries.sort_by(|a, b| a.path.cmp(&b.path));
        for entry in sorted_entries {
            result.add_entry(entry);
        }

        Ok(result)
    }

    /// Convenience method: apply without hash verification.
    pub fn apply_unchecked(&self, manifest: &Manifest) -> Result<Manifest, PatchApplyError> {
        self.apply(manifest, false)
    }

    /// Return the set of paths affected by this patch.
    pub fn affected_paths(&self) -> Vec<&Path> {
        let mut paths: Vec<&Path> = Vec::new();
        for delta in &self.deltas {
            paths.push(&delta.path);
            match &delta.kind {
                DeltaKind::Rename { from } => paths.push(from),
                DeltaKind::Copy { from } => paths.push(from),
                _ => {}
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    /// Return a summary string describing the patch.
    pub fn summary(&self) -> String {
        let mut adds = 0usize;
        let mut modifies = 0usize;
        let mut deletes = 0usize;
        let mut renames = 0usize;
        let mut copies = 0usize;

        for delta in &self.deltas {
            match &delta.kind {
                DeltaKind::Add => adds += 1,
                DeltaKind::Modify => modifies += 1,
                DeltaKind::Delete => deletes += 1,
                DeltaKind::Rename { .. } => renames += 1,
                DeltaKind::Copy { .. } => copies += 1,
            }
        }

        format!(
            "{} add(s), {} modify(ies), {} delete(s), {} rename(s), {} copy(ies)",
            adds, modifies, deletes, renames, copies
        )
    }
}

impl fmt::Display for Patch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.description.is_empty() {
            write!(f, "Patch({} delta(s))", self.deltas.len())
        } else {
            write!(
                f,
                "Patch({} delta(s)): {}",
                self.deltas.len(),
                self.description
            )
        }
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;
    use crate::core::id::TreeId;
    use crate::object::delta::Delta;
    use crate::object::manifest::{Manifest, ManifestEntry};

    fn make_manifest(entries: Vec<ManifestEntry>) -> Manifest {
        let tree_id = TreeId::new();
        let mut m = Manifest::new(tree_id);
        for e in entries {
            m.add_entry(e);
        }
        m
    }

    #[test]
    fn test_empty_patch() {
        let patch = Patch::new(vec![]);
        assert!(patch.is_empty());
        assert_eq!(patch.len(), 0);
        assert_eq!(patch.to_string(), "Patch(0 delta(s))");
    }

    #[test]
    fn test_patch_display_with_description() {
        let patch = Patch::new(vec![Delta::add("a.txt", hash_bytes(b"a"), 1)])
            .with_description("add a.txt");
        assert_eq!(patch.to_string(), "Patch(1 delta(s)): add a.txt");
    }

    #[test]
    fn test_apply_add() {
        let hash = hash_bytes(b"new content");
        let manifest = make_manifest(vec![]);

        let patch = Patch::new(vec![Delta::add("new.txt", hash, 11)]);
        let result = patch.apply(&manifest, false).unwrap();

        assert_eq!(result.len(), 1);
        let entry = result.find_entry(Path::new("new.txt")).unwrap();
        assert_eq!(entry.hash, hash);
        assert_eq!(entry.size, 11);
    }

    #[test]
    fn test_apply_add_path_already_exists() {
        let hash = hash_bytes(b"content");
        let manifest = make_manifest(vec![ManifestEntry::file("existing.txt", hash, 7)]);

        let patch = Patch::new(vec![Delta::add("existing.txt", hash, 7)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::PathAlreadyExists(p) => {
                assert_eq!(p, PathBuf::from("existing.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_delete() {
        let hash = hash_bytes(b"doomed");
        let manifest = make_manifest(vec![ManifestEntry::file("doomed.txt", hash, 6)]);

        let patch = Patch::new(vec![Delta::delete("doomed.txt", hash, 6)]);
        let result = patch.apply(&manifest, false).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_apply_delete_path_not_found() {
        let manifest = make_manifest(vec![]);
        let hash = hash_bytes(b"ghost");

        let patch = Patch::new(vec![Delta::delete("ghost.txt", hash, 5)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::PathNotFound(p) => {
                assert_eq!(p, PathBuf::from("ghost.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_modify() {
        let old_hash = hash_bytes(b"old");
        let new_hash = hash_bytes(b"new");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", old_hash, 3)]);

        let patch = Patch::new(vec![Delta::modify("file.txt", old_hash, new_hash, 3, 3)]);
        let result = patch.apply(&manifest, true).unwrap();

        assert_eq!(result.len(), 1);
        let entry = result.find_entry(Path::new("file.txt")).unwrap();
        assert_eq!(entry.hash, new_hash);
    }

    #[test]
    fn test_apply_modify_path_not_found() {
        let manifest = make_manifest(vec![]);
        let old_hash = hash_bytes(b"old");
        let new_hash = hash_bytes(b"new");

        let patch = Patch::new(vec![Delta::modify("missing.txt", old_hash, new_hash, 3, 3)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::PathNotFound(p) => {
                assert_eq!(p, PathBuf::from("missing.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_modify_hash_mismatch() {
        let actual_hash = hash_bytes(b"actual");
        let expected_hash = hash_bytes(b"expected");
        let new_hash = hash_bytes(b"new");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", actual_hash, 6)]);

        let patch = Patch::new(vec![Delta::modify(
            "file.txt",
            expected_hash,
            new_hash,
            8,
            3,
        )]);
        let result = patch.apply(&manifest, true);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::HashMismatch {
                path,
                expected,
                found,
            } => {
                assert_eq!(path, PathBuf::from("file.txt"));
                assert_eq!(expected, expected_hash);
                assert_eq!(found, actual_hash);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_modify_hash_mismatch_skipped_when_not_verifying() {
        let actual_hash = hash_bytes(b"actual");
        let expected_hash = hash_bytes(b"expected");
        let new_hash = hash_bytes(b"new");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", actual_hash, 6)]);

        let patch = Patch::new(vec![Delta::modify(
            "file.txt",
            expected_hash,
            new_hash,
            8,
            3,
        )]);
        let result = patch.apply(&manifest, false);
        assert!(result.is_ok());
        let entry = result
            .unwrap()
            .find_entry(Path::new("file.txt"))
            .unwrap()
            .clone();
        assert_eq!(entry.hash, new_hash);
    }

    #[test]
    fn test_apply_rename() {
        let hash = hash_bytes(b"rename me");
        let manifest = make_manifest(vec![ManifestEntry::file("old.txt", hash, 9)]);

        let patch = Patch::new(vec![Delta::rename("old.txt", "new.txt", hash, 9)]);
        let result = patch.apply(&manifest, true).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result.find_entry(Path::new("old.txt")).is_none());
        let entry = result.find_entry(Path::new("new.txt")).unwrap();
        assert_eq!(entry.hash, hash);
        assert_eq!(entry.size, 9);
    }

    #[test]
    fn test_apply_rename_source_not_found() {
        let manifest = make_manifest(vec![]);
        let hash = hash_bytes(b"phantom");

        let patch = Patch::new(vec![Delta::rename("phantom.txt", "target.txt", hash, 7)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::RenameSourceNotFound(p) => {
                assert_eq!(p, PathBuf::from("phantom.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_rename_destination_exists() {
        let hash = hash_bytes(b"content");
        let manifest = make_manifest(vec![
            ManifestEntry::file("src.txt", hash, 7),
            ManifestEntry::file("dst.txt", hash, 7),
        ]);

        let patch = Patch::new(vec![Delta::rename("src.txt", "dst.txt", hash, 7)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::RenameDestinationExists(p) => {
                assert_eq!(p, PathBuf::from("dst.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_copy() {
        let hash = hash_bytes(b"copy me");
        let manifest = make_manifest(vec![ManifestEntry::file("original.txt", hash, 7)]);

        let patch = Patch::new(vec![Delta::copy("original.txt", "clone.txt", hash, 7)]);
        let result = patch.apply(&manifest, false).unwrap();

        assert_eq!(result.len(), 2);
        let original = result.find_entry(Path::new("original.txt")).unwrap();
        assert_eq!(original.hash, hash);
        let copy = result.find_entry(Path::new("clone.txt")).unwrap();
        assert_eq!(copy.hash, hash);
    }

    #[test]
    fn test_apply_copy_source_not_found() {
        let manifest = make_manifest(vec![]);
        let hash = hash_bytes(b"nowhere");

        let patch = Patch::new(vec![Delta::copy("nowhere.txt", "clone.txt", hash, 7)]);
        let result = patch.apply(&manifest, false);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::CopySourceNotFound(p) => {
                assert_eq!(p, PathBuf::from("nowhere.txt"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_multiple_operations() {
        let hash_a = hash_bytes(b"a");
        let hash_b = hash_bytes(b"b");
        let hash_c = hash_bytes(b"c");
        let hash_d = hash_bytes(b"d");

        let manifest = make_manifest(vec![
            ManifestEntry::file("keep.txt", hash_a, 1),
            ManifestEntry::file("modify.txt", hash_b, 1),
            ManifestEntry::file("delete.txt", hash_c, 1),
            ManifestEntry::file("rename_me.txt", hash_d, 1),
        ]);

        let patch = Patch::new(vec![
            Delta::rename("rename_me.txt", "renamed.txt", hash_d, 1),
            Delta::delete("delete.txt", hash_c, 1),
            Delta::add("new.txt", hash_bytes(b"new"), 3),
            Delta::modify("modify.txt", hash_b, hash_bytes(b"modified"), 1, 8),
        ]);

        let result = patch.apply(&manifest, true).unwrap();

        // Should have: keep.txt, modify.txt (updated), renamed.txt, new.txt
        assert_eq!(result.len(), 4);

        assert!(result.find_entry(Path::new("keep.txt")).is_some());
        assert!(result.find_entry(Path::new("delete.txt")).is_none());
        assert!(result.find_entry(Path::new("rename_me.txt")).is_none());

        let modified = result.find_entry(Path::new("modify.txt")).unwrap();
        assert_eq!(modified.hash, hash_bytes(b"modified"));

        let renamed = result.find_entry(Path::new("renamed.txt")).unwrap();
        assert_eq!(renamed.hash, hash_d);

        let new = result.find_entry(Path::new("new.txt")).unwrap();
        assert_eq!(new.hash, hash_bytes(b"new"));
    }

    #[test]
    fn test_apply_empty_patch() {
        let hash = hash_bytes(b"content");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", hash, 7)]);

        let patch = Patch::new(vec![]);
        let result = patch.apply(&manifest, true).unwrap();

        assert_eq!(result.len(), 1);
        let entry = result.find_entry(Path::new("file.txt")).unwrap();
        assert_eq!(entry.hash, hash);
    }

    #[test]
    fn test_apply_unchecked() {
        let actual_hash = hash_bytes(b"actual");
        let wrong_hash = hash_bytes(b"wrong");
        let new_hash = hash_bytes(b"new");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", actual_hash, 6)]);

        let patch = Patch::new(vec![Delta::modify("file.txt", wrong_hash, new_hash, 5, 3)]);
        // apply_unchecked skips hash verification
        let result = patch.apply_unchecked(&manifest).unwrap();
        let entry = result.find_entry(Path::new("file.txt")).unwrap();
        assert_eq!(entry.hash, new_hash);
    }

    #[test]
    fn test_apply_preserves_sort_order() {
        let manifest = make_manifest(vec![]);
        let patch = Patch::new(vec![
            Delta::add("c.txt", hash_bytes(b"c"), 1),
            Delta::add("a.txt", hash_bytes(b"a"), 1),
            Delta::add("b.txt", hash_bytes(b"b"), 1),
        ]);

        let result = patch.apply(&manifest, false).unwrap();
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("a.txt"),
                PathBuf::from("b.txt"),
                PathBuf::from("c.txt"),
            ]
        );
    }

    #[test]
    fn test_affected_paths() {
        let hash = hash_bytes(b"x");
        let patch = Patch::new(vec![
            Delta::add("new.txt", hash, 1),
            Delta::delete("old.txt", hash, 1),
            Delta::rename("src.txt", "dst.txt", hash, 1),
        ]);

        let paths = patch.affected_paths();
        // Should include: dst.txt, new.txt, old.txt, src.txt (sorted, deduped)
        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0], Path::new("dst.txt"));
        assert_eq!(paths[1], Path::new("new.txt"));
        assert_eq!(paths[2], Path::new("old.txt"));
        assert_eq!(paths[3], Path::new("src.txt"));
    }

    #[test]
    fn test_summary() {
        let hash = hash_bytes(b"x");
        let patch = Patch::new(vec![
            Delta::add("a.txt", hash, 1),
            Delta::add("b.txt", hash, 1),
            Delta::modify("c.txt", hash, hash, 1, 1),
            Delta::delete("d.txt", hash, 1),
            Delta::rename("e.txt", "f.txt", hash, 1),
            Delta::copy("g.txt", "h.txt", hash, 1),
        ]);

        let summary = patch.summary();
        assert_eq!(
            summary,
            "2 add(s), 1 modify(ies), 1 delete(s), 1 rename(s), 1 copy(ies)"
        );
    }

    #[test]
    fn test_with_base_hash() {
        let hash = hash_bytes(b"base manifest");
        let patch = Patch::new(vec![]).with_base_hash(hash);
        assert_eq!(patch.base_hash, Some(hash));
    }

    #[test]
    fn test_default_patch() {
        let patch = Patch::default();
        assert!(patch.is_empty());
        assert!(patch.description.is_empty());
        assert!(patch.base_hash.is_none());
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let hash_a = hash_bytes(b"a");
        let hash_b = hash_bytes(b"b");

        let patch = Patch::new(vec![
            Delta::add("new.txt", hash_a, 1),
            Delta::modify("mod.txt", hash_a, hash_b, 1, 1),
            Delta::delete("del.txt", hash_a, 1),
            Delta::rename("old.txt", "new_name.txt", hash_a, 1),
        ])
        .with_description("test patch")
        .with_base_hash(hash_a);

        let json = serde_json::to_string(&patch).expect("serialize");
        let deserialized: Patch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(patch, deserialized);
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let hash = hash_bytes(b"bincode");
        let patch = Patch::new(vec![Delta::add("file.txt", hash, 7)]);

        let encoded = bincode::serialize(&patch).expect("serialize");
        let decoded: Patch = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(patch, decoded);
    }

    #[test]
    fn test_apply_rename_with_hash_verification() {
        let hash = hash_bytes(b"rename hash check");
        let wrong_hash = hash_bytes(b"wrong hash");
        let manifest = make_manifest(vec![ManifestEntry::file("src.txt", hash, 16)]);

        // Construct a rename delta with wrong old_hash
        let delta = Delta {
            path: PathBuf::from("dst.txt"),
            kind: DeltaKind::Rename {
                from: PathBuf::from("src.txt"),
            },
            old_hash: Some(wrong_hash),
            new_hash: Some(hash),
            old_size: Some(16),
            new_size: Some(16),
        };

        let patch = Patch::new(vec![delta]);
        let result = patch.apply(&manifest, true);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::HashMismatch {
                path,
                expected,
                found,
            } => {
                assert_eq!(path, PathBuf::from("src.txt"));
                assert_eq!(expected, wrong_hash);
                assert_eq!(found, hash);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_delete_with_hash_verification() {
        let actual = hash_bytes(b"actual content");
        let expected = hash_bytes(b"expected content");
        let manifest = make_manifest(vec![ManifestEntry::file("file.txt", actual, 14)]);

        let patch = Patch::new(vec![Delta::delete("file.txt", expected, 16)]);
        let result = patch.apply(&manifest, true);

        assert!(result.is_err());
        match result.unwrap_err() {
            PatchApplyError::HashMismatch {
                path,
                expected: exp,
                found,
            } => {
                assert_eq!(path, PathBuf::from("file.txt"));
                assert_eq!(exp, expected);
                assert_eq!(found, actual);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_apply_preserves_executable_flag_on_rename() {
        let hash = hash_bytes(b"executable script");
        let manifest = make_manifest(vec![
            ManifestEntry::file("run.sh", hash, 17).with_executable(true),
        ]);

        let patch = Patch::new(vec![Delta::rename("run.sh", "execute.sh", hash, 17)]);
        let result = patch.apply(&manifest, false).unwrap();

        let entry = result.find_entry(Path::new("execute.sh")).unwrap();
        assert!(entry.executable);
    }

    #[test]
    fn test_apply_preserves_executable_flag_on_copy() {
        let hash = hash_bytes(b"executable copy");
        let manifest = make_manifest(vec![
            ManifestEntry::file("run.sh", hash, 15).with_executable(true),
        ]);

        let patch = Patch::new(vec![Delta::copy("run.sh", "run_copy.sh", hash, 15)]);
        let result = patch.apply(&manifest, false).unwrap();

        let original = result.find_entry(Path::new("run.sh")).unwrap();
        assert!(original.executable);

        let copy = result.find_entry(Path::new("run_copy.sh")).unwrap();
        assert!(copy.executable);
    }

    #[test]
    fn test_error_display() {
        let err = PatchApplyError::PathAlreadyExists(PathBuf::from("dup.txt"));
        assert_eq!(err.to_string(), "path already exists: dup.txt");

        let err = PatchApplyError::PathNotFound(PathBuf::from("missing.txt"));
        assert_eq!(err.to_string(), "path not found: missing.txt");

        let err = PatchApplyError::RenameSourceNotFound(PathBuf::from("src.txt"));
        assert_eq!(err.to_string(), "rename source not found: src.txt");

        let err = PatchApplyError::RenameDestinationExists(PathBuf::from("dst.txt"));
        assert_eq!(err.to_string(), "rename destination already exists: dst.txt");

        let err = PatchApplyError::CopySourceNotFound(PathBuf::from("orig.txt"));
        assert_eq!(err.to_string(), "copy source not found: orig.txt");

        let err = PatchApplyError::ConflictingOperations(PathBuf::from("conflict.txt"));
        assert_eq!(
            err.to_string(),
            "conflicting operations on path: conflict.txt"
        );

        let h1 = hash_bytes(b"expected");
        let h2 = hash_bytes(b"found");
        let err = PatchApplyError::HashMismatch {
            path: PathBuf::from("file.txt"),
            expected: h1,
            found: h2,
        };
        let msg = err.to_string();
        assert!(msg.contains("hash mismatch at file.txt"));
        assert!(msg.contains(&h1.to_hex()));
        assert!(msg.contains(&h2.to_hex()));
    }
}
