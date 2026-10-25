//! Working-directory scanning, the stat-cache index, and materializing
//! manifests into the working directory.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ignore_rules::{is_reserved_name, IgnoreStack};
use super::trees::TreeRecord;
use super::{fsutil, names, Repository, TREE_MARKER_DIR};
use crate::error::{Result, SdkError};
use crate::model::{validate_manifest_path, Manifest, ManifestEntry};
use crate::scan::SecretFinding;
use crate::store::{hash_bytes, prepare_file, ObjectHash};

/// Version 2 added `chunked` (large files hash to a chunk list).
const INDEX_VERSION: u32 = 2;

/// Entries whose mtime falls within this window before the index was saved
/// are re-hashed: a write in the same timestamp tick (2 s on FAT) as our
/// read would otherwise go unnoticed ("racy" entries, as in Git).
const RACY_WINDOW_NS: u64 = 2_000_000_000;

/// Stat cache: lets `status` skip re-hashing files whose size and mtime are
/// unchanged since they were last hashed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Index {
    version: u32,
    saved_at_ns: u64,
    entries: BTreeMap<String, IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IndexEntry {
    size: u64,
    mtime_ns: u64,
    hash: ObjectHash,
    executable: bool,
    /// `hash` names a chunk list (ENGINEERING_PLAN §13.2).
    #[serde(default)]
    chunked: bool,
}

impl Index {
    /// The cached entry for `path` if it is still trustworthy for a file
    /// with this size and mtime.
    fn fresh(&self, path: &str, size: u64, mtime_ns: u64) -> Option<&IndexEntry> {
        self.entries.get(path).filter(|e| {
            e.size == size
                && e.mtime_ns == mtime_ns
                && mtime_ns.saturating_add(RACY_WINDOW_NS) < self.saved_at_ns
        })
    }

    pub(crate) fn remove(&mut self, path: &str) {
        self.entries.remove(path);
    }

    pub(crate) fn insert(&mut self, path: String, entry: IndexEntry) {
        self.entries.insert(path, entry);
    }
}

/// Result of scanning a tree's working directory.
pub(crate) struct WorkingState {
    /// Every tracked-able file (not ignored, outside nested trees).
    pub(crate) manifest: Manifest,
    /// Refreshed stat cache covering exactly `manifest`'s files.
    pub(crate) index: Index,
    /// Secret-scan findings (only when scanning was requested).
    pub(crate) findings: Vec<SecretFinding>,
}

/// A file found by the walker, before hashing.
struct FoundFile {
    path: String,
    abs: PathBuf,
    size: u64,
    mtime_ns: u64,
    exec_bit: Option<bool>,
}

#[cfg(unix)]
fn exec_bit(meta: &fs::Metadata) -> Option<bool> {
    Some(fsutil::is_executable(meta))
}

#[cfg(not(unix))]
fn exec_bit(_meta: &fs::Metadata) -> Option<bool> {
    None
}

impl Repository {
    /// Build a manifest of the current working directory, storing blobs.
    pub fn manifest_from_working(&self, tree: &str) -> Result<Manifest> {
        let rec = self.load_tree(tree)?;
        let head = self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?;
        let working = self.build_working(&rec, &head, true, false)?;
        self.save_index(tree, &working.index)?;
        Ok(working.manifest)
    }

    /// Walk files under the root-relative directory `start` (""= root),
    /// applying ignore rules, skipping `skip_dirs` (root-relative) and, if
    /// `skip_marked`, any directory containing a `.wt-tree/` marker.
    /// `visit` receives (root-relative path, absolute path, metadata).
    /// Directory symlinks are not followed; file symlinks are read through.
    pub(crate) fn walk_files(
        &self,
        start: &str,
        skip_dirs: &[String],
        skip_marked: bool,
        visit: &mut dyn FnMut(&str, &Path, &fs::Metadata) -> Result<()>,
    ) -> Result<()> {
        let mut stack = IgnoreStack::for_root(&self.root)?;
        if !start.is_empty() {
            let parts: Vec<&str> = start.split('/').collect();
            for i in 1..parts.len() {
                stack.push_dir(&self.root, &parts[..i].join("/"))?;
            }
        }
        let walker = Walker {
            root: &self.root,
            skip_dirs,
            skip_marked,
        };
        walker.walk_dir(&mut stack, start, visit)
    }

    /// Scan the working directory of `rec`, hashing files (using the stat
    /// cache where possible). With `store`, blobs are written to the object
    /// store; with `scan`, files added or modified relative to `head` are
    /// secret-scanned and files with findings are not stored.
    pub(crate) fn build_working(
        &self,
        rec: &TreeRecord,
        head: &Manifest,
        store: bool,
        scan: bool,
    ) -> Result<WorkingState> {
        let old_index = self.load_index(&rec.name);
        let nested = self.nested_tree_paths(rec)?;
        let mut found = Vec::new();
        self.walk_files(&rec.path, &nested, true, &mut |rel, abs, meta| {
            let path = tree_relative(&rec.path, rel).to_string();
            found.push(FoundFile {
                path,
                abs: abs.to_path_buf(),
                size: meta.len(),
                mtime_ns: fsutil::mtime_ns(meta),
                exec_bit: exec_bit(meta),
            });
            Ok(())
        })?;

        let mut entries = Vec::with_capacity(found.len());
        let mut index = Index {
            version: INDEX_VERSION,
            saved_at_ns: 0,
            entries: BTreeMap::new(),
        };
        let mut findings = Vec::new();
        for file in found {
            let head_entry = head.get(&file.path);
            let fallback_exec = old_index
                .entries
                .get(&file.path)
                .map(|e| e.executable)
                .or(head_entry.map(|e| e.executable))
                .unwrap_or(false);
            let executable = file.exec_bit.unwrap_or(fallback_exec);
            let cached = old_index
                .fresh(&file.path, file.size, file.mtime_ns)
                .map(|e| (e.hash, e.chunked))
                .filter(|(h, _)| !store || self.store.has(h));
            let needs_read = match cached {
                None => true,
                Some((h, _)) => scan && head_entry.is_none_or(|e| e.hash != h),
            };
            let (hash, size, chunked) = match (cached, needs_read) {
                (Some((h, chunked)), false) => (h, file.size, chunked),
                _ => {
                    let data = match fs::read(&file.abs) {
                        Ok(d) => d,
                        // Deleted while we were scanning.
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e.into()),
                    };
                    // Large files hash to a chunk list (ENGINEERING_PLAN
                    // §13.2). A large file recorded as a single blob (before
                    // chunking existed, or by an importer) keeps that form
                    // while its content is unchanged.
                    let prepared = prepare_file(&data);
                    let legacy = prepared.chunked
                        && head_entry.is_some_and(|e| {
                            !e.chunked && e.size == data.len() as u64 && e.hash == hash_bytes(&data)
                        });
                    let (hash, chunked) = match head_entry {
                        Some(e) if legacy => (e.hash, false),
                        _ => (prepared.hash, prepared.chunked),
                    };
                    let changed = head_entry.is_none_or(|e| e.hash != hash);
                    let mut clean = true;
                    if scan && changed {
                        let hits = crate::scan::scan_secrets(&file.path, &data);
                        clean = hits.is_empty();
                        findings.extend(hits);
                    }
                    if store && clean {
                        if legacy {
                            self.store.put_hashed(&hash, &data)?;
                        } else {
                            self.store.put_prepared(&prepared, &data)?;
                        }
                    }
                    (hash, data.len() as u64, chunked)
                }
            };
            index.entries.insert(
                file.path.clone(),
                IndexEntry {
                    size,
                    mtime_ns: file.mtime_ns,
                    hash,
                    executable,
                    chunked,
                },
            );
            entries.push(ManifestEntry {
                path: file.path,
                hash,
                size,
                executable,
                chunked,
            });
        }
        // Restricted ("stubbed") files are never written to the working
        // directory (ENGINEERING_PLAN §13.2); while absent and unreadable
        // they keep their head entry, so they neither show as deleted nor
        // disappear from the next snapshot.
        let stubbed = self.stubbed_paths(&rec.name)?;
        if !stubbed.is_empty() {
            let present: HashSet<String> = entries.iter().map(|e| e.path.clone()).collect();
            for path in &stubbed {
                if let Some(e) = head.get(path) {
                    if !present.contains(path) && !self.store.has_entry(e) {
                        entries.push(e.clone());
                    }
                }
            }
        }
        Ok(WorkingState {
            manifest: Manifest::from_entries(entries),
            index,
            findings,
        })
    }

    /// Make the working directory of `rec` match `target`: delete files in
    /// `tracked` that `target` lacks (only if they are currently present in
    /// `current`, i.e. not ignored and outside nested trees), then write
    /// every target entry whose working copy differs. Updates `index`.
    pub(crate) fn apply_manifest(
        &self,
        rec: &TreeRecord,
        target: &Manifest,
        tracked: &HashSet<String>,
        current: &Manifest,
        index: &mut Index,
    ) -> Result<()> {
        let tree_dir = self.tree_dir_of(rec);
        let nested = self.nested_tree_rel_paths(rec)?;
        let stubbed = self.stubbed_paths(&rec.name)?;
        for entry in &current.entries {
            if target.get(&entry.path).is_none() && tracked.contains(&entry.path) {
                self.remove_working_file(&tree_dir, &entry.path)?;
                index.remove(&entry.path);
            }
        }
        for entry in &target.entries {
            if nested
                .iter()
                .any(|n| names::path_has_prefix(&entry.path, n))
            {
                continue;
            }
            if current.get(&entry.path) == Some(entry) {
                continue;
            }
            // Restricted files whose content the server withheld are not
            // written (ENGINEERING_PLAN §13.2).
            if stubbed.contains(&entry.path) && !self.store.has_entry(entry) {
                self.remove_working_file(&tree_dir, &entry.path)?;
                index.remove(&entry.path);
                continue;
            }
            let cached = self.materialize(&tree_dir, entry)?;
            index.insert(entry.path.clone(), cached);
        }
        Ok(())
    }

    /// Paths of trees nested inside `rec`, relative to `rec`'s directory.
    pub(crate) fn nested_tree_rel_paths(&self, rec: &TreeRecord) -> Result<Vec<String>> {
        Ok(self
            .nested_tree_paths(rec)?
            .iter()
            .map(|p| tree_relative(&rec.path, p).to_string())
            .collect())
    }

    /// Write one manifest entry's blob into the working directory.
    pub(crate) fn materialize(&self, tree_dir: &Path, entry: &ManifestEntry) -> Result<IndexEntry> {
        validate_manifest_path(&entry.path)?;
        let data = self.store.read_entry(entry)?;
        let abs = tree_dir.join(&entry.path);
        if abs.is_dir() {
            return Err(SdkError::InvalidArgument(format!(
                "cannot write {}: a directory is in the way",
                entry.path
            )));
        }
        fsutil::write_atomic(&abs, &data)?;
        fsutil::set_executable(&abs, entry.executable)?;
        let meta = fs::metadata(&abs)?;
        Ok(IndexEntry {
            size: data.len() as u64,
            mtime_ns: fsutil::mtime_ns(&meta),
            hash: entry.hash,
            executable: entry.executable,
            chunked: entry.chunked,
        })
    }

    /// Delete a working file and prune directories it leaves empty.
    pub(crate) fn remove_working_file(&self, tree_dir: &Path, path: &str) -> Result<()> {
        validate_manifest_path(path)?;
        let abs = tree_dir.join(path);
        fsutil::remove_file_if_exists(&abs)?;
        if let Some(parent) = abs.parent() {
            fsutil::prune_empty_dirs(parent, tree_dir);
        }
        Ok(())
    }

    fn index_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("index.json")
    }

    /// Load the stat cache; a missing, outdated or corrupt index is treated
    /// as empty (it is only a cache).
    pub(crate) fn load_index(&self, tree: &str) -> Index {
        match fsutil::read_json_opt::<Index>(&self.index_path(tree)) {
            Ok(Some(index)) if index.version == INDEX_VERSION => index,
            Ok(_) => Index::default(),
            Err(err) => {
                tracing::warn!(tree, %err, "discarding unreadable index");
                Index::default()
            }
        }
    }

    pub(crate) fn save_index(&self, tree: &str, index: &Index) -> Result<()> {
        let mut index = index.clone();
        index.version = INDEX_VERSION;
        index.saved_at_ns = fsutil::now_ns();
        fsutil::write_json(&self.index_path(tree), &index)
    }
}

/// Strip a tree's root-relative directory prefix from a root-relative path.
fn tree_relative<'a>(tree_path: &str, rel: &'a str) -> &'a str {
    if tree_path.is_empty() {
        rel
    } else {
        rel.strip_prefix(tree_path)
            .and_then(|r| r.strip_prefix('/'))
            .unwrap_or(rel)
    }
}

struct Walker<'a> {
    root: &'a Path,
    skip_dirs: &'a [String],
    skip_marked: bool,
}

impl Walker<'_> {
    fn walk_dir(
        &self,
        stack: &mut IgnoreStack,
        dir_rel: &str,
        visit: &mut dyn FnMut(&str, &Path, &fs::Metadata) -> Result<()>,
    ) -> Result<()> {
        let abs = if dir_rel.is_empty() {
            self.root.to_path_buf()
        } else {
            self.root.join(dir_rel)
        };
        let pushed = !dir_rel.is_empty() && stack.push_dir(self.root, dir_rel)?;
        let result = self.walk_entries(stack, dir_rel, &abs, visit);
        stack.pop(pushed);
        result
    }

    fn walk_entries(
        &self,
        stack: &mut IgnoreStack,
        dir_rel: &str,
        abs: &Path,
        visit: &mut dyn FnMut(&str, &Path, &fs::Metadata) -> Result<()>,
    ) -> Result<()> {
        let read = match fs::read_dir(abs) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut entries = read.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let file_name = entry.file_name();
            // Manifest paths are '/'-separated UTF-8; names that cannot be
            // represented portably are skipped rather than corrupting a
            // manifest.
            let Some(name) = file_name.to_str().filter(|n| !n.contains('\\')) else {
                tracing::warn!(path = %entry.path().display(), "skipping unrepresentable file name");
                continue;
            };
            // In-flight temp files from atomic writes are never content.
            if is_reserved_name(name) || name.starts_with(fsutil::TEMP_PREFIX) {
                continue;
            }
            let rel = if dir_rel.is_empty() {
                name.to_string()
            } else {
                format!("{dir_rel}/{name}")
            };
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let (is_dir, followed) = if ft.is_symlink() {
                match fs::metadata(entry.path()) {
                    Ok(m) if m.is_file() => (false, Some(m)),
                    // Directory symlinks are not followed; dangling links skipped.
                    _ => continue,
                }
            } else if ft.is_dir() {
                (true, None)
            } else if ft.is_file() {
                (false, None)
            } else {
                continue;
            };
            if stack.is_ignored(&rel, is_dir) {
                continue;
            }
            let path = entry.path();
            if is_dir {
                if self.skip_dirs.iter().any(|d| d == &rel)
                    || (self.skip_marked && path.join(TREE_MARKER_DIR).is_dir())
                {
                    continue;
                }
                self.walk_dir(stack, &rel, visit)?;
            } else {
                let meta = match followed {
                    Some(m) => m,
                    None => match entry.metadata() {
                        Ok(m) => m,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e.into()),
                    },
                };
                visit(&rel, &path, &meta)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_relative_paths() {
        assert_eq!(tree_relative("", "a/b"), "a/b");
        assert_eq!(tree_relative("sub", "sub/a"), "a");
        assert_eq!(tree_relative("sub", "subx/a"), "subx/a");
    }

    #[test]
    fn fresh_respects_racy_window() {
        let h = hash_bytes(b"x");
        let mut index = Index {
            version: INDEX_VERSION,
            saved_at_ns: 10 * RACY_WINDOW_NS,
            entries: BTreeMap::new(),
        };
        index.entries.insert(
            "old".into(),
            IndexEntry {
                size: 1,
                mtime_ns: RACY_WINDOW_NS,
                hash: h,
                executable: false,
                chunked: false,
            },
        );
        index.entries.insert(
            "racy".into(),
            IndexEntry {
                size: 1,
                mtime_ns: 9 * RACY_WINDOW_NS + 1,
                hash: h,
                executable: false,
                chunked: false,
            },
        );
        assert!(index.fresh("old", 1, RACY_WINDOW_NS).is_some());
        assert!(
            index.fresh("old", 2, RACY_WINDOW_NS).is_none(),
            "size changed"
        );
        assert!(
            index.fresh("old", 1, RACY_WINDOW_NS + 1).is_none(),
            "mtime changed"
        );
        assert!(index.fresh("racy", 1, 9 * RACY_WINDOW_NS + 1).is_none());
        assert!(index.fresh("missing", 1, 0).is_none());
    }
}
