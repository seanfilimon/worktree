//! Diff computation between two manifests.
//!
//! Provides [`compute_diff()`] which compares two [`Manifest`]s and produces a
//! list of [`Delta`]s describing the changes, including rename detection.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::hash::ContentHash;
use crate::object::delta::{Delta, DeltaKind};
use crate::object::manifest::{EntryKind, Manifest, ManifestEntry};

/// Configuration for diff computation.
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Whether to detect renames (files with the same hash but different paths).
    pub detect_renames: bool,
    /// Whether to detect copies (files in `new` whose hash matches a file still
    /// present in `old`).
    pub detect_copies: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            detect_renames: true,
            detect_copies: false,
        }
    }
}

/// Compute the diff between two manifests, returning a list of deltas.
///
/// The `old` manifest represents the base state and `new` represents the
/// target state.  When `options.detect_renames` is `true`, a deleted path
/// whose content hash appears in an added path will be reported as a single
/// [`DeltaKind::Rename`] instead of a delete + add pair.
///
/// When `options.detect_copies` is `true`, an added path whose content hash
/// matches a file that still exists in the old manifest (i.e., not consumed
/// by a rename) will be reported as a [`DeltaKind::Copy`].
pub fn compute_diff(old: &Manifest, new: &Manifest, options: &DiffOptions) -> Vec<Delta> {
    let old_map: HashMap<&std::path::Path, &ManifestEntry> =
        old.entries.iter().map(|e| (e.path.as_path(), e)).collect();
    let new_map: HashMap<&std::path::Path, &ManifestEntry> =
        new.entries.iter().map(|e| (e.path.as_path(), e)).collect();

    let mut deltas: Vec<Delta> = Vec::new();
    let mut deleted: Vec<&ManifestEntry> = Vec::new();
    let mut added: Vec<&ManifestEntry> = Vec::new();

    // 1. Find deletions and modifications by iterating old entries.
    for old_entry in &old.entries {
        match new_map.get(old_entry.path.as_path()) {
            Some(new_entry) => {
                // Path exists in both — check for modification.
                if old_entry.hash != new_entry.hash
                    || old_entry.size != new_entry.size
                    || old_entry.executable != new_entry.executable
                    || old_entry.kind != new_entry.kind
                {
                    deltas.push(Delta::modify(
                        old_entry.path.clone(),
                        old_entry.hash,
                        new_entry.hash,
                        old_entry.size,
                        new_entry.size,
                    ));
                }
            }
            None => {
                // Path only in old — candidate for deletion (or rename source).
                deleted.push(old_entry);
            }
        }
    }

    // 2. Find additions by iterating new entries.
    for new_entry in &new.entries {
        if !old_map.contains_key(new_entry.path.as_path()) {
            added.push(new_entry);
        }
    }

    // 3. Rename / copy detection.
    if options.detect_renames || options.detect_copies {
        // Build a map from hash -> list of deleted entries for rename matching.
        let mut deleted_by_hash: HashMap<ContentHash, Vec<&ManifestEntry>> = HashMap::new();
        for entry in &deleted {
            if entry.kind == EntryKind::File && entry.hash != ContentHash::ZERO {
                deleted_by_hash
                    .entry(entry.hash)
                    .or_default()
                    .push(entry);
            }
        }

        // Track which deleted entries were consumed by a rename.
        let mut consumed_deleted: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        // Track which added entries were matched.
        let mut matched_added: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();

        if options.detect_renames {
            for add_entry in &added {
                if add_entry.kind != EntryKind::File || add_entry.hash == ContentHash::ZERO {
                    continue;
                }
                if let Some(del_list) = deleted_by_hash.get_mut(&add_entry.hash) {
                    // Find the first not-yet-consumed deleted entry with the same hash.
                    if let Some(pos) = del_list
                        .iter()
                        .position(|d| !consumed_deleted.contains(&d.path))
                    {
                        let del_entry = del_list[pos];
                        deltas.push(Delta::rename(
                            del_entry.path.clone(),
                            add_entry.path.clone(),
                            add_entry.hash,
                            add_entry.size,
                        ));
                        consumed_deleted.insert(del_entry.path.clone());
                        matched_added.insert(add_entry.path.clone());
                    }
                }
            }
        }

        if options.detect_copies {
            // Build a set of hashes from old entries that are still present
            // (not consumed by renames).
            let mut old_hashes: HashMap<ContentHash, &ManifestEntry> = HashMap::new();
            for entry in &old.entries {
                if entry.kind == EntryKind::File
                    && entry.hash != ContentHash::ZERO
                    && !consumed_deleted.contains(&entry.path)
                {
                    old_hashes.entry(entry.hash).or_insert(entry);
                }
            }
            // Also include deleted entries that were not consumed by renames.
            for entry in &deleted {
                if entry.kind == EntryKind::File
                    && entry.hash != ContentHash::ZERO
                    && !consumed_deleted.contains(&entry.path)
                {
                    old_hashes.entry(entry.hash).or_insert(entry);
                }
            }

            for add_entry in &added {
                if matched_added.contains(&add_entry.path) {
                    continue;
                }
                if add_entry.kind != EntryKind::File || add_entry.hash == ContentHash::ZERO {
                    continue;
                }
                if let Some(source) = old_hashes.get(&add_entry.hash) {
                    deltas.push(Delta::copy(
                        source.path.clone(),
                        add_entry.path.clone(),
                        add_entry.hash,
                        add_entry.size,
                    ));
                    matched_added.insert(add_entry.path.clone());
                }
            }
        }

        // 4. Emit remaining deletes (not consumed by renames).
        for entry in &deleted {
            if !consumed_deleted.contains(&entry.path) {
                deltas.push(Delta::delete(entry.path.clone(), entry.hash, entry.size));
            }
        }

        // 5. Emit remaining adds (not matched by renames or copies).
        for entry in &added {
            if !matched_added.contains(&entry.path) {
                deltas.push(Delta::add(entry.path.clone(), entry.hash, entry.size));
            }
        }
    } else {
        // No rename/copy detection: all unmatched are plain deletes and adds.
        for entry in &deleted {
            deltas.push(Delta::delete(entry.path.clone(), entry.hash, entry.size));
        }
        for entry in &added {
            deltas.push(Delta::add(entry.path.clone(), entry.hash, entry.size));
        }
    }

    // Sort by path for deterministic output.
    deltas.sort_by(|a, b| a.path.cmp(&b.path));
    deltas
}

/// Convenience wrapper that calls [`compute_diff`] with default options
/// (rename detection on, copy detection off).
pub fn compute_diff_default(old: &Manifest, new: &Manifest) -> Vec<Delta> {
    compute_diff(old, new, &DiffOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;
    use crate::core::id::TreeId;
    use crate::object::manifest::{EntryKind, Manifest, ManifestEntry};

    fn make_manifest(entries: Vec<ManifestEntry>) -> Manifest {
        let tree_id = TreeId::new();
        let mut m = Manifest::new(tree_id);
        for e in entries {
            m.add_entry(e);
        }
        m
    }

    #[test]
    fn test_no_changes() {
        let hash = hash_bytes(b"content");
        let old = make_manifest(vec![ManifestEntry::file("a.txt", hash, 7)]);
        let new = make_manifest(vec![ManifestEntry::file("a.txt", hash, 7)]);

        let deltas = compute_diff_default(&old, &new);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_add_file() {
        let hash = hash_bytes(b"new file");
        let old = make_manifest(vec![]);
        let new = make_manifest(vec![ManifestEntry::file("added.txt", hash, 8)]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_add());
        assert_eq!(deltas[0].path, PathBuf::from("added.txt"));
        assert_eq!(deltas[0].new_hash, Some(hash));
    }

    #[test]
    fn test_delete_file() {
        let hash = hash_bytes(b"old file");
        let old = make_manifest(vec![ManifestEntry::file("removed.txt", hash, 8)]);
        let new = make_manifest(vec![]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_delete());
        assert_eq!(deltas[0].path, PathBuf::from("removed.txt"));
        assert_eq!(deltas[0].old_hash, Some(hash));
    }

    #[test]
    fn test_modify_file() {
        let old_hash = hash_bytes(b"version 1");
        let new_hash = hash_bytes(b"version 2");
        let old = make_manifest(vec![ManifestEntry::file("file.txt", old_hash, 9)]);
        let new = make_manifest(vec![ManifestEntry::file("file.txt", new_hash, 9)]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_modify());
        assert_eq!(deltas[0].old_hash, Some(old_hash));
        assert_eq!(deltas[0].new_hash, Some(new_hash));
    }

    #[test]
    fn test_modify_file_size_change() {
        let hash_a = hash_bytes(b"short");
        let hash_b = hash_bytes(b"much longer content");
        let old = make_manifest(vec![ManifestEntry::file("file.txt", hash_a, 5)]);
        let new = make_manifest(vec![ManifestEntry::file("file.txt", hash_b, 19)]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_modify());
        assert_eq!(deltas[0].old_size, Some(5));
        assert_eq!(deltas[0].new_size, Some(19));
    }

    #[test]
    fn test_rename_detection() {
        let hash = hash_bytes(b"same content");
        let old = make_manifest(vec![ManifestEntry::file("old_name.rs", hash, 12)]);
        let new = make_manifest(vec![ManifestEntry::file("new_name.rs", hash, 12)]);

        let deltas = compute_diff(&old, &new, &DiffOptions::default());
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_rename());
        assert_eq!(deltas[0].path, PathBuf::from("new_name.rs"));
        if let DeltaKind::Rename { from } = &deltas[0].kind {
            assert_eq!(from, &PathBuf::from("old_name.rs"));
        } else {
            panic!("expected Rename");
        }
    }

    #[test]
    fn test_rename_detection_disabled() {
        let hash = hash_bytes(b"same content");
        let old = make_manifest(vec![ManifestEntry::file("old_name.rs", hash, 12)]);
        let new = make_manifest(vec![ManifestEntry::file("new_name.rs", hash, 12)]);

        let opts = DiffOptions {
            detect_renames: false,
            detect_copies: false,
        };
        let deltas = compute_diff(&old, &new, &opts);
        assert_eq!(deltas.len(), 2);

        let delete = deltas.iter().find(|d| d.is_delete()).unwrap();
        assert_eq!(delete.path, PathBuf::from("old_name.rs"));

        let add = deltas.iter().find(|d| d.is_add()).unwrap();
        assert_eq!(add.path, PathBuf::from("new_name.rs"));
    }

    #[test]
    fn test_copy_detection() {
        let hash = hash_bytes(b"copied content");
        // Old has the file; new has the file AND a copy at a different path.
        let old = make_manifest(vec![ManifestEntry::file("original.rs", hash, 14)]);
        let new = make_manifest(vec![
            ManifestEntry::file("original.rs", hash, 14),
            ManifestEntry::file("copy.rs", hash, 14),
        ]);

        let opts = DiffOptions {
            detect_renames: true,
            detect_copies: true,
        };
        let deltas = compute_diff(&old, &new, &opts);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_copy());
        assert_eq!(deltas[0].path, PathBuf::from("copy.rs"));
        if let DeltaKind::Copy { from } = &deltas[0].kind {
            assert_eq!(from, &PathBuf::from("original.rs"));
        } else {
            panic!("expected Copy");
        }
    }

    #[test]
    fn test_copy_detection_disabled() {
        let hash = hash_bytes(b"copied content");
        let old = make_manifest(vec![ManifestEntry::file("original.rs", hash, 14)]);
        let new = make_manifest(vec![
            ManifestEntry::file("original.rs", hash, 14),
            ManifestEntry::file("copy.rs", hash, 14),
        ]);

        let opts = DiffOptions {
            detect_renames: true,
            detect_copies: false,
        };
        let deltas = compute_diff(&old, &new, &opts);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_add());
        assert_eq!(deltas[0].path, PathBuf::from("copy.rs"));
    }

    #[test]
    fn test_multiple_changes() {
        let hash_a = hash_bytes(b"a");
        let hash_b = hash_bytes(b"b");
        let hash_c = hash_bytes(b"c");
        let hash_d = hash_bytes(b"d");

        let old = make_manifest(vec![
            ManifestEntry::file("keep.txt", hash_a, 1),
            ManifestEntry::file("modify.txt", hash_b, 1),
            ManifestEntry::file("remove.txt", hash_c, 1),
        ]);

        let new = make_manifest(vec![
            ManifestEntry::file("keep.txt", hash_a, 1),
            ManifestEntry::file("modify.txt", hash_d, 1),
            ManifestEntry::file("added.txt", hash_c, 1), // Same hash as remove -> rename
        ]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 2);

        // Should have a rename (remove.txt -> added.txt) and a modify (modify.txt).
        let rename = deltas.iter().find(|d| d.is_rename());
        assert!(rename.is_some());
        let rename = rename.unwrap();
        assert_eq!(rename.path, PathBuf::from("added.txt"));

        let modify = deltas.iter().find(|d| d.is_modify());
        assert!(modify.is_some());
        let modify = modify.unwrap();
        assert_eq!(modify.path, PathBuf::from("modify.txt"));
    }

    #[test]
    fn test_executable_flag_change_is_modification() {
        let hash = hash_bytes(b"script");
        let old = make_manifest(vec![ManifestEntry::file("run.sh", hash, 6)]);
        let new = make_manifest(vec![
            ManifestEntry::file("run.sh", hash, 6).with_executable(true),
        ]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_modify());
    }

    #[test]
    fn test_both_manifests_empty() {
        let old = make_manifest(vec![]);
        let new = make_manifest(vec![]);
        let deltas = compute_diff_default(&old, &new);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_directory_entries_ignored_for_rename() {
        // Directories use ZERO hash — they should never be matched for renames.
        let file_hash = hash_bytes(b"file content");
        let old = make_manifest(vec![
            ManifestEntry::directory("src"),
            ManifestEntry::file("src/main.rs", file_hash, 12),
        ]);
        let new = make_manifest(vec![
            ManifestEntry::directory("lib"),
            ManifestEntry::file("src/main.rs", file_hash, 12),
        ]);

        let deltas = compute_diff_default(&old, &new);
        // src dir deleted, lib dir added (no rename because dirs have ZERO hash)
        let delete = deltas.iter().find(|d| d.is_delete());
        assert!(delete.is_some());
        let add = deltas.iter().find(|d| d.is_add());
        assert!(add.is_some());
    }

    #[test]
    fn test_deterministic_ordering() {
        let h1 = hash_bytes(b"1");
        let h2 = hash_bytes(b"2");
        let h3 = hash_bytes(b"3");

        let old = make_manifest(vec![]);
        let new = make_manifest(vec![
            ManifestEntry::file("c.txt", h3, 1),
            ManifestEntry::file("a.txt", h1, 1),
            ManifestEntry::file("b.txt", h2, 1),
        ]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].path, PathBuf::from("a.txt"));
        assert_eq!(deltas[1].path, PathBuf::from("b.txt"));
        assert_eq!(deltas[2].path, PathBuf::from("c.txt"));
    }

    #[test]
    fn test_kind_change_is_modification() {
        let hash = hash_bytes(b"content");
        let old = make_manifest(vec![ManifestEntry::file("link", hash, 7)]);
        let new = make_manifest(vec![ManifestEntry::symlink("link", hash)]);

        let deltas = compute_diff_default(&old, &new);
        assert_eq!(deltas.len(), 1);
        assert!(deltas[0].is_modify());
    }

    #[test]
    fn test_rename_prefers_first_match() {
        // Two deleted files with the same hash, one added file with that hash.
        // Should only produce one rename, not two.
        let hash = hash_bytes(b"dup");
        let old = make_manifest(vec![
            ManifestEntry::file("a.txt", hash, 3),
            ManifestEntry::file("b.txt", hash, 3),
        ]);
        let new = make_manifest(vec![ManifestEntry::file("c.txt", hash, 3)]);

        let deltas = compute_diff_default(&old, &new);
        // One rename, one delete.
        let renames: Vec<_> = deltas.iter().filter(|d| d.is_rename()).collect();
        let deletes: Vec<_> = deltas.iter().filter(|d| d.is_delete()).collect();
        assert_eq!(renames.len(), 1);
        assert_eq!(deletes.len(), 1);
        assert_eq!(renames[0].path, PathBuf::from("c.txt"));
    }
}
