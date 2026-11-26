//! Working-directory scanning, the stat-cache index, and materializing
//! manifests into the working directory.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use super::ignore_rules::{is_reserved_name, IgnoreStack};
use super::trees::TreeRecord;
use super::{fsutil, names, Repository, TREE_MARKER_DIR};
use crate::error::{Result, SdkError};
use crate::model::{is_shared_tree_config_path, validate_manifest_path, Manifest, ManifestEntry};
use crate::scan::SecretFinding;
use crate::store::{hash_bytes, prepare_file, ObjectHash};

/// Version 2 added `chunked` (large files hash to a chunk list).
const INDEX_VERSION: u32 = 2;

/// Entries whose mtime falls within this window before the index was saved
/// are re-hashed: a write in the same timestamp tick (2 s on FAT) as our
/// read would otherwise go unnoticed ("racy" entries, as in Git).
const RACY_WINDOW_NS: u64 = 2_000_000_000;

/// Below this many files with I/O to do, scans and checkouts stay on the
/// calling thread (starting the worker pool costs more than it saves).
const PARALLEL_MIN_FILES: usize = 64;

/// Stat cache: lets `status` skip re-hashing files whose size and mtime are
/// unchanged since they were last hashed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Index {
    version: u32,
    saved_at_ns: u64,
    entries: BTreeMap<String, IndexEntry>,
    /// Set by a scan that found exactly the loaded entries, all of them
    /// already trusted: saving would only bump `saved_at_ns`, which changes
    /// nothing, so [`Repository::save_index`] skips the write.
    #[serde(skip)]
    unchanged: bool,
}

/// Borrowed view of an [`Index`] for saving (no copy of the entries).
#[derive(Serialize)]
struct IndexRef<'a> {
    version: u32,
    saved_at_ns: u64,
    entries: &'a BTreeMap<String, IndexEntry>,
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
        self.unchanged = false;
        self.entries.remove(path);
    }

    pub(crate) fn insert(&mut self, path: String, entry: IndexEntry) {
        self.unchanged = false;
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

/// Shared, read-only inputs of [`Repository::scan_found_file`].
#[derive(Clone, Copy)]
struct ScanCtx<'a> {
    rec: &'a TreeRecord,
    head: &'a Manifest,
    old_index: &'a Index,
    scanner: Option<&'a crate::scan::SecretScanner>,
    store: bool,
    scan: bool,
}

/// One hashed file (`entry.path` is unset; the path is `path`).
struct ScannedFile {
    path: String,
    mtime_ns: u64,
    entry: ManifestEntry,
    /// Blocking secret findings (the blob was not stored).
    findings: Vec<SecretFinding>,
    /// Non-blocking findings, printed in walk order by the caller.
    warnings: Vec<SecretFinding>,
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
    /// With `skip_marked` (a tree scan), the shared files of the tree's own
    /// `<start>/.wt-tree/` are visited too (hardening §15.10).
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
            marker_root: skip_marked.then_some(start),
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
        // local-config (§13.9): the effective `[secret_scanning]` of the tree.
        let scanner = if scan {
            Some(self.secret_scanner(&rec.name)?).filter(|s| s.enabled())
        } else {
            None
        };
        let scan = scanner.is_some();
        // hardening (§15.10): move legacy identity lines out of the shared
        // `.wt-tree/config.toml` before it is scanned.
        self.migrate_tree_marker(rec)?;
        let old_index = self.load_index(&rec.name);
        let nested = self.nested_tree_paths(rec)?;
        // sparse (§15.4): files outside the sparse set are not scanned.
        let sparse = self.sparse_matcher(&rec.name)?;
        let mut found = Vec::new();
        self.walk_files(&rec.path, &nested, true, &mut |rel, abs, meta| {
            let path = tree_relative(&rec.path, rel).to_string();
            if sparse.as_ref().is_some_and(|s| !s.includes(&path)) {
                return Ok(());
            }
            found.push(FoundFile {
                path,
                abs: abs.to_path_buf(),
                size: meta.len(),
                mtime_ns: fsutil::mtime_ns(meta),
                exec_bit: exec_bit(meta),
            });
            Ok(())
        })?;

        // Files are read, hashed, scanned and stored in parallel (each one is
        // independent and the store is safe for concurrent writers); the
        // results are then folded in walk order, so the manifest, index and
        // findings are exactly what a sequential pass produces.
        let ctx = ScanCtx {
            rec,
            head,
            old_index: &old_index,
            scanner: scanner.as_ref(),
            store,
            scan,
        };
        // Threads only pay off with real I/O to overlap: files to read, or
        // (when storing) an object check per cached file. A clean `status`
        // is a few map lookups per file and stays on this thread.
        let io_files = if store {
            found.len()
        } else {
            found
                .iter()
                .filter(|f| old_index.fresh(&f.path, f.size, f.mtime_ns).is_none())
                .count()
        };
        let results: Vec<Result<Option<ScannedFile>>> = if io_files >= PARALLEL_MIN_FILES {
            found
                .into_par_iter()
                .map(|file| self.scan_found_file(&ctx, file))
                .collect()
        } else {
            found
                .into_iter()
                .map(|file| self.scan_found_file(&ctx, file))
                .collect()
        };

        let mut entries = Vec::with_capacity(results.len());
        let mut index = Index {
            version: INDEX_VERSION,
            saved_at_ns: 0,
            entries: BTreeMap::new(),
            unchanged: false,
        };
        let mut findings = Vec::new();
        for result in results {
            let Some(file) = result? else { continue };
            crate::scan::warn_findings(&file.warnings);
            findings.extend(file.findings);
            let entry = file.entry;
            index.entries.insert(
                file.path.clone(),
                IndexEntry {
                    size: entry.size,
                    mtime_ns: file.mtime_ns,
                    hash: entry.hash,
                    executable: entry.executable,
                    chunked: entry.chunked,
                },
            );
            entries.push(ManifestEntry {
                path: file.path,
                ..entry
            });
        }
        if old_index.version == INDEX_VERSION
            && index.entries == old_index.entries
            && index
                .entries
                .values()
                .all(|e| e.mtime_ns.saturating_add(RACY_WINDOW_NS) < old_index.saved_at_ns)
        {
            index.saved_at_ns = old_index.saved_at_ns;
            index.unchanged = true;
        }
        // sparse (§15.4): head files outside the sparse set keep their head
        // entries — never reported as deleted, kept by the next snapshot.
        if let Some(sparse) = &sparse {
            entries.extend(
                head.entries
                    .iter()
                    .filter(|e| !sparse.includes(&e.path))
                    .cloned(),
            );
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

    /// Hash (and, per `ctx`, scan and store) one walked file. `None` when
    /// the file disappeared during the scan. Runs on a worker thread.
    fn scan_found_file(&self, ctx: &ScanCtx<'_>, file: FoundFile) -> Result<Option<ScannedFile>> {
        let ScanCtx {
            rec,
            head,
            old_index,
            scanner,
            store,
            scan,
        } = *ctx;
        let head_entry = head.get(&file.path);
        let fallback_exec = old_index
            .entries
            .get(&file.path)
            .map(|e| e.executable)
            .or(head_entry.map(|e| e.executable))
            .unwrap_or(false);
        // hardening (§15.10): without an exec bit (Windows), new shared
        // hook scripts are recorded executable so they run elsewhere.
        let fallback_exec = fallback_exec
            || (!old_index.entries.contains_key(&file.path)
                && head_entry.is_none()
                && crate::model::is_shared_tree_hook_path(&file.path));
        // Shared hook scripts exist to be run by everyone who checks the
        // tree out, so they are executable whatever the mode of the file
        // they were first written as.
        let executable = file.exec_bit.unwrap_or(fallback_exec)
            || crate::model::is_shared_tree_hook_path(&file.path);
        let cached = old_index
            .fresh(&file.path, file.size, file.mtime_ns)
            .map(|e| (e.hash, e.chunked))
            .filter(|(h, _)| !store || self.store.has(h));
        let needs_read = match cached {
            None => true,
            Some((h, _)) => scan && head_entry.is_none_or(|e| e.hash != h),
        };
        let mut findings = Vec::new();
        let mut warnings = Vec::new();
        let (hash, size, chunked) = match (cached, needs_read) {
            (Some((h, chunked)), false) => (h, file.size, chunked),
            _ => {
                let data = match fs::read(&file.abs) {
                    Ok(d) => d,
                    // Deleted while we were scanning.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
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
                if let Some(scanner) = scanner.filter(|_| changed) {
                    let root_rel = if rec.path.is_empty() {
                        file.path.clone()
                    } else {
                        format!("{}/{}", rec.path, file.path)
                    };
                    if !scanner.is_excluded(&[&file.path, &root_rel]) {
                        let hits = scanner.scan(&file.path, &data);
                        if scanner.blocks() {
                            clean = hits.is_empty();
                            findings = hits;
                        } else {
                            warnings = hits;
                        }
                    }
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
        Ok(Some(ScannedFile {
            entry: ManifestEntry {
                path: String::new(),
                hash,
                size,
                executable,
                chunked,
            },
            path: file.path,
            mtime_ns: file.mtime_ns,
            findings,
            warnings,
        }))
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
        // sparse (§15.4): paths outside the sparse set are never touched.
        let sparse = self.sparse_matcher(&rec.name)?;
        let outside = |path: &str| sparse.as_ref().is_some_and(|s| !s.includes(path));
        for entry in &current.entries {
            if target.get(&entry.path).is_none()
                && tracked.contains(&entry.path)
                && !outside(&entry.path)
            {
                self.remove_working_file(&tree_dir, &entry.path)?;
                index.remove(&entry.path);
            }
        }
        let mut to_write = Vec::new();
        for entry in &target.entries {
            if nested
                .iter()
                .any(|n| names::path_has_prefix(&entry.path, n))
            {
                continue;
            }
            if outside(&entry.path) || current.get(&entry.path) == Some(entry) {
                continue;
            }
            // Restricted files whose content the server withheld are not
            // written (ENGINEERING_PLAN §13.2).
            if stubbed.contains(&entry.path) && !self.store.has_entry(entry) {
                self.remove_working_file(&tree_dir, &entry.path)?;
                index.remove(&entry.path);
                continue;
            }
            to_write.push(entry);
        }
        // sparse (§15.4): one batched download for a partial clone.
        self.store.prefetch_entries(to_write.iter().copied())?;
        // Each entry writes a distinct path; temp names are unique per
        // writer and directory creation tolerates races, so files are
        // written in parallel and the index updated in order afterwards.
        let written: Vec<Result<IndexEntry>> = if to_write.len() >= PARALLEL_MIN_FILES {
            to_write
                .par_iter()
                .map(|entry| self.materialize(&tree_dir, entry))
                .collect()
        } else {
            to_write
                .iter()
                .map(|entry| self.materialize(&tree_dir, entry))
                .collect()
        };
        for (entry, cached) in to_write.into_iter().zip(written) {
            index.insert(entry.path.clone(), cached?);
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
        fsutil::write_atomic_volatile(&abs, &data)?;
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
        if index.unchanged {
            return Ok(());
        }
        // Compact JSON: the index is a cache read by every command and can
        // hold hundreds of thousands of entries.
        let data = serde_json::to_vec(&IndexRef {
            version: INDEX_VERSION,
            saved_at_ns: fsutil::now_ns(),
            entries: &index.entries,
        })?;
        fsutil::write_atomic_volatile(&self.index_path(tree), &data)
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
    /// Root-relative directory whose own `.wt-tree/` shared files are
    /// visited (hardening §15.10); `None` = `.wt-tree/` is never entered.
    marker_root: Option<&'a str>,
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
            // hardening (§15.10): the tree's own `.wt-tree/` is shared.
            if name == TREE_MARKER_DIR && self.marker_root == Some(dir_rel) {
                if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                    let rel = if dir_rel.is_empty() {
                        name.to_string()
                    } else {
                        format!("{dir_rel}/{name}")
                    };
                    self.walk_marker(&rel, "", &entry.path(), visit)?;
                }
                continue;
            }
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

    /// Visit the shared files below a tree's `.wt-tree/` directory
    /// (`marker_rel`, root-relative): `config.toml`, `ignore` and every
    /// regular file below `hooks/` and `access/`
    /// ([`is_shared_tree_config_path`]). `sub` is the path of `abs` below
    /// `.wt-tree/` ("" for the marker itself). Ignore rules do not apply
    /// here; directory symlinks are not followed.
    fn walk_marker(
        &self,
        marker_rel: &str,
        sub: &str,
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
            let Some(name) = file_name.to_str().filter(|n| !n.contains('\\')) else {
                continue;
            };
            if is_reserved_name(name) || name.starts_with(fsutil::TEMP_PREFIX) {
                continue;
            }
            let child = if sub.is_empty() {
                name.to_string()
            } else {
                format!("{sub}/{name}")
            };
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let path = entry.path();
            if ft.is_dir() {
                // Only hooks/ and access/ hold shared content below them.
                let top = child.split('/').next().unwrap_or_default();
                if top == "hooks" || top == "access" {
                    self.walk_marker(marker_rel, &child, &path, visit)?;
                }
                continue;
            }
            if !is_shared_tree_config_path(&format!("{TREE_MARKER_DIR}/{child}")) {
                continue;
            }
            let meta = match fs::metadata(&path) {
                Ok(m) if m.is_file() => m,
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            visit(&format!("{marker_rel}/{child}"), &path, &meta)?;
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
            unchanged: false,
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
