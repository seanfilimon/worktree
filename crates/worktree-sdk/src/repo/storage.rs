//! Local garbage collection (`wt gc`) and integrity checks (`wt fsck`)
//! (ENGINEERING_PLAN §14.6, `specs/storage/Compression.md`).
//!
//! **Reachability.** An object is live when it is reachable from a root:
//!
//! - every branch tip (archived branches included), remote-tracking ref and
//!   tag of every tree;
//! - the `old` / `new` snapshots of reflog entries newer than the prune
//!   window (default [`GC_DEFAULT_PRUNE_DAYS`] days);
//! - a pending merge (`MERGE_STATE.json`: source, target and base);
//! - stash entries (`trees/<tree>/stash.jsonl`) and every other JSON state
//!   file of a tree (rebase / cherry-pick progress, conflict lists): any
//!   string in them that names a stored snapshot or object is a root, so
//!   state written by newer commands stays protected without gc knowing
//!   its format;
//! - snapshot records and objects written within the in-flight grace
//!   period (one hour, or the prune window when shorter), so a concurrent
//!   snapshot, fetch or merge that has stored objects but not yet moved a
//!   ref never loses them.
//!
//! From a snapshot, its parents, its manifest, every entry's blob or chunk
//! list and every chunk are reachable. gc deletes unreachable objects and
//! unreachable snapshot records, removes stale temp files, and rewrites raw
//! objects in the compressed encoding when that saves space. It holds every
//! tree's lock while it runs, and refuses to delete anything when a
//! reachable manifest or chunk list cannot be decoded (run `wt fsck`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, SystemTime};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::history::is_valid_id;
use super::{fsutil, Repository};
use crate::error::{Result, SdkError};
use crate::model::{Manifest, ManifestEntry, MergeState, Snapshot, SnapshotId};
use crate::store::{ChunkList, ObjectHash};

/// Default prune window of `wt gc`: reflog entries older than this many
/// days stop protecting their snapshots.
pub const GC_DEFAULT_PRUNE_DAYS: u32 = 30;

/// Unreachable objects and snapshot records younger than this (or than the
/// prune window, when shorter) are kept.
pub const GC_IN_FLIGHT_GRACE: StdDuration = StdDuration::from_secs(3600);

/// Per-tree state files that are not GC roots (the reflog has its own
/// window; the stat cache only mirrors working files).
const NON_ROOT_STATE_FILES: &[&str] = &["reflog.jsonl", "index.json"];

/// Options of [`Repository::gc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcOptions {
    /// Report what would be deleted without changing anything.
    pub dry_run: bool,
    /// Reflog entries newer than this many days keep their snapshots.
    pub prune_days: u32,
}

impl Default for GcOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            prune_days: GC_DEFAULT_PRUNE_DAYS,
        }
    }
}

/// Result of [`Repository::gc`]. In a dry run the `*_deleted` counts say
/// what would be deleted; no compaction is attempted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GcReport {
    pub dry_run: bool,
    pub prune_days: u32,
    /// Stored objects examined.
    pub objects_scanned: u64,
    pub objects_reachable: u64,
    /// Unreachable objects kept because they were written within the
    /// in-flight grace period.
    pub objects_recent: u64,
    pub objects_deleted: u64,
    /// Stored bytes of the deleted objects.
    pub bytes_deleted: u64,
    pub snapshots_reachable: u64,
    pub snapshots_deleted: u64,
    /// Raw objects rewritten compressed, and the bytes that saved.
    pub objects_compacted: u64,
    pub bytes_compacted: u64,
    /// Interrupted-write temp files removed.
    pub temp_files_deleted: u64,
    /// Stored object bytes before and after the run.
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// Options of [`Repository::fsck`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FsckOptions {
    /// Also re-hash every stored object (not only the manifests and chunk
    /// lists that history reads).
    pub full: bool,
}

/// Kind of a problem found by [`Repository::fsck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsckIssueKind {
    /// A stored object does not decode to content with its hash.
    CorruptObject,
    /// A manifest entry's blob, chunk list or chunk is not stored.
    MissingObject,
    /// A snapshot's manifest is not stored.
    MissingManifest,
    /// A manifest object does not decode.
    BadManifest,
    /// A chunk list object does not decode.
    BadChunkList,
    /// A snapshot record does not parse or is misnamed.
    BadSnapshot,
    /// A snapshot's parent is not stored (outside a shallow boundary).
    MissingParent,
    /// A branch, remote-tracking ref or tag names a snapshot that is not
    /// stored.
    DanglingRef,
}

/// One problem found by [`Repository::fsck`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsckIssue {
    pub kind: FsckIssueKind,
    /// The object hash, snapshot id or ref the problem is about.
    pub id: String,
    pub detail: String,
}

/// Result of [`Repository::fsck`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsckReport {
    pub full: bool,
    /// Objects whose content was read back and re-hashed.
    pub objects_checked: u64,
    pub snapshots_checked: u64,
    pub manifests_checked: u64,
    pub refs_checked: u64,
    /// Blobs of restricted (stubbed) paths that are absent as expected.
    pub restricted_missing: u64,
    pub errors: Vec<FsckIssue>,
}

impl FsckReport {
    /// Whether no problem was found.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Snapshots and objects that keep content alive.
#[derive(Debug, Default)]
struct Roots {
    snapshots: BTreeSet<SnapshotId>,
    /// Object hashes named directly (stash manifests, state files).
    objects: HashSet<ObjectHash>,
}

/// Everything reachable from the roots.
#[derive(Debug, Default)]
struct Marked {
    snapshots: HashSet<SnapshotId>,
    objects: HashSet<ObjectHash>,
}

impl Repository {
    /// Delete local objects (and snapshot records) unreachable from any
    /// root; see the module documentation for the roots. Takes every
    /// tree's lock unless `opts.dry_run`.
    pub fn gc(&self, opts: &GcOptions) -> Result<GcReport> {
        let records = self.tree_records()?;
        let _locks = if opts.dry_run {
            Vec::new()
        } else {
            let mut names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
            names.sort_unstable();
            names
                .into_iter()
                .map(|n| self.lock_tree(n))
                .collect::<Result<Vec<_>>>()?
        };

        let now = SystemTime::now();
        let prune = StdDuration::from_secs(u64::from(opts.prune_days) * 86_400);
        let grace = GC_IN_FLIGHT_GRACE.min(prune);
        let grace_cutoff = now.checked_sub(grace).unwrap_or(SystemTime::UNIX_EPOCH);
        let reflog_cutoff = Utc::now() - Duration::days(i64::from(opts.prune_days));

        let mut roots = self.gc_roots(reflog_cutoff)?;
        // In-flight protection: recently written snapshot records.
        let records_on_disk = self.snapshot_files()?;
        for (id, path) in &records_on_disk {
            if modified_since(path, grace_cutoff) {
                roots.snapshots.insert(id.clone());
            }
        }
        let marked = self.mark(&roots, true)?;

        let mut report = GcReport {
            dry_run: opts.dry_run,
            prune_days: opts.prune_days,
            snapshots_reachable: marked.snapshots.len() as u64,
            ..GcReport::default()
        };
        let objects = self.store.objects()?;
        let mut survivors = Vec::new();
        for obj in &objects {
            report.objects_scanned += 1;
            report.bytes_before += obj.stored_size;
            if marked.objects.contains(&obj.hash) {
                report.objects_reachable += 1;
                survivors.push(obj.hash);
                continue;
            }
            if obj.modified.is_none_or(|m| m >= grace_cutoff) {
                report.objects_recent += 1;
                survivors.push(obj.hash);
                continue;
            }
            report.objects_deleted += 1;
            report.bytes_deleted += obj.stored_size;
            if !opts.dry_run {
                self.store.delete(&obj.hash)?;
            }
        }
        for (id, path) in &records_on_disk {
            // Records that do not parse are left for `wt fsck` to report.
            if marked.snapshots.contains(id) || !matches!(self.load_snapshot_opt(id), Ok(Some(_))) {
                continue;
            }
            report.snapshots_deleted += 1;
            if !opts.dry_run {
                fsutil::remove_file_if_exists(path)?;
            }
        }
        for (path, _) in self.store.stale_temp_files(grace_cutoff)? {
            report.temp_files_deleted += 1;
            if !opts.dry_run {
                fsutil::remove_file_if_exists(&path)?;
            }
        }
        if !opts.dry_run {
            for hash in &survivors {
                match self.store.recompress(hash) {
                    Ok(Some((old, new))) => {
                        report.objects_compacted += 1;
                        report.bytes_compacted += old.saturating_sub(new);
                    }
                    Ok(None) => {}
                    // Compaction is best effort; the object stays raw.
                    Err(e) => tracing::warn!(%hash, error = %e, "cannot compact object"),
                }
            }
        }
        report.bytes_after = report
            .bytes_before
            .saturating_sub(report.bytes_deleted)
            .saturating_sub(report.bytes_compacted);
        Ok(report)
    }

    /// Check the integrity of the local store and history: snapshot records
    /// parse, their parents and manifests are present, manifests and chunk
    /// lists decode and hash correctly, every referenced blob / chunk is
    /// stored (restricted paths excepted) and refs name stored snapshots.
    /// With `opts.full` every stored object is re-hashed too.
    pub fn fsck(&self, opts: &FsckOptions) -> Result<FsckReport> {
        let mut report = FsckReport {
            full: opts.full,
            ..FsckReport::default()
        };
        let mut corrupt: HashSet<ObjectHash> = HashSet::new();
        if opts.full {
            for obj in self.store.objects()? {
                match self.store.get(&obj.hash) {
                    Ok(_) => report.objects_checked += 1,
                    Err(SdkError::ObjectNotFound(_)) => {}
                    Err(e) => {
                        report.objects_checked += 1;
                        corrupt.insert(obj.hash);
                        report.errors.push(issue(
                            FsckIssueKind::CorruptObject,
                            obj.hash.to_hex(),
                            e.to_string(),
                        ));
                    }
                }
            }
        }

        let records = self.tree_records()?;
        let mut shallow: HashSet<SnapshotId> = HashSet::new();
        let mut stubbed: HashMap<String, BTreeSet<String>> = HashMap::new();
        for rec in &records {
            shallow.extend(self.shallow_boundary(&rec.name)?);
            stubbed.insert(rec.id.clone(), self.stubbed_paths(&rec.name)?);
        }

        // Snapshots, their parents and manifests.
        let mut manifests: BTreeMap<ObjectHash, (SnapshotId, String)> = BTreeMap::new();
        for (id, path) in self.snapshot_files()? {
            report.snapshots_checked += 1;
            let snap: Snapshot = match fs::read(&path)
                .map_err(SdkError::from)
                .and_then(|d| serde_json::from_slice(&d).map_err(SdkError::from))
            {
                Ok(s) => s,
                Err(e) => {
                    report
                        .errors
                        .push(issue(FsckIssueKind::BadSnapshot, id, e.to_string()));
                    continue;
                }
            };
            if snap.id != id {
                report.errors.push(issue(
                    FsckIssueKind::BadSnapshot,
                    id.clone(),
                    format!("the record is named {id} but holds snapshot {}", snap.id),
                ));
            }
            for parent in &snap.parents {
                if !self.has_snapshot(parent) && !shallow.contains(&snap.id) {
                    report.errors.push(issue(
                        FsckIssueKind::MissingParent,
                        snap.id.clone(),
                        format!("parent {parent} is not stored"),
                    ));
                }
            }
            manifests
                .entry(snap.manifest)
                .or_insert((snap.id.clone(), snap.tree_id.clone()));
        }

        // Manifests and the objects their entries reference.
        let mut checked_lists: HashSet<ObjectHash> = HashSet::new();
        for (hash, (snapshot, tree_id)) in &manifests {
            if !self.store.has(hash) {
                report.errors.push(issue(
                    FsckIssueKind::MissingManifest,
                    snapshot.clone(),
                    format!("manifest {hash} is not stored"),
                ));
                continue;
            }
            report.manifests_checked += 1;
            if corrupt.contains(hash) {
                continue;
            }
            let data = match self.store.get(hash) {
                Ok(d) => d,
                Err(e) => {
                    report.errors.push(issue(
                        FsckIssueKind::CorruptObject,
                        hash.to_hex(),
                        e.to_string(),
                    ));
                    continue;
                }
            };
            let manifest = match Manifest::from_bytes(&data) {
                Ok(m) => m,
                Err(e) => {
                    report.errors.push(issue(
                        FsckIssueKind::BadManifest,
                        hash.to_hex(),
                        e.to_string(),
                    ));
                    continue;
                }
            };
            let restricted = stubbed.get(tree_id);
            for entry in &manifest.entries {
                self.fsck_entry(
                    entry,
                    snapshot,
                    restricted,
                    opts.full,
                    &corrupt,
                    &mut checked_lists,
                    &mut report,
                );
            }
        }

        // Refs.
        for rec in &records {
            let tree = rec.name.as_str();
            let mut refs: Vec<(String, SnapshotId)> = Vec::new();
            for b in self.branches(tree, true)? {
                if let Some(tip) = b.tip {
                    refs.push((format!("{tree}: branch {}", b.name), tip));
                }
            }
            for remote in self.remote_names(tree)? {
                for (branch, tip) in self.remote_branches(tree, &remote)? {
                    refs.push((format!("{tree}: remote ref {remote}/{branch}"), tip));
                }
            }
            for t in self.tags(tree)? {
                refs.push((format!("{tree}: tag {}", t.name), t.target));
            }
            for (name, tip) in refs {
                report.refs_checked += 1;
                if !self.has_snapshot(&tip) {
                    report.errors.push(issue(
                        FsckIssueKind::DanglingRef,
                        name,
                        format!("points at snapshot {tip}, which is not stored"),
                    ));
                }
            }
        }
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn fsck_entry(
        &self,
        entry: &ManifestEntry,
        snapshot: &str,
        restricted: Option<&BTreeSet<String>>,
        full: bool,
        corrupt: &HashSet<ObjectHash>,
        checked_lists: &mut HashSet<ObjectHash>,
        report: &mut FsckReport,
    ) {
        let hash = entry.hash;
        if !self.store.has(&hash) {
            if restricted.is_some_and(|r| r.contains(&entry.path)) {
                report.restricted_missing += 1;
            } else {
                report.errors.push(issue(
                    FsckIssueKind::MissingObject,
                    hash.to_hex(),
                    format!("{} in snapshot {snapshot} is not stored", entry.path),
                ));
            }
            return;
        }
        if !entry.chunked || !checked_lists.insert(hash) || corrupt.contains(&hash) {
            return;
        }
        let data = match self.store.get(&hash) {
            Ok(d) => d,
            Err(e) => {
                // Already reported by the full object pass.
                if !full {
                    report.errors.push(issue(
                        FsckIssueKind::CorruptObject,
                        hash.to_hex(),
                        e.to_string(),
                    ));
                }
                return;
            }
        };
        let list = match ChunkList::from_bytes(&data) {
            Ok(l) => l,
            Err(e) => {
                report
                    .errors
                    .push(issue(FsckIssueKind::BadChunkList, hash.to_hex(), e));
                return;
            }
        };
        for chunk in &list.chunks {
            if !self.store.has(&chunk.hash) {
                report.errors.push(issue(
                    FsckIssueKind::MissingObject,
                    chunk.hash.to_hex(),
                    format!(
                        "a chunk of {} in snapshot {snapshot} is not stored",
                        entry.path
                    ),
                ));
            }
        }
    }

    /// Every GC root of the repository.
    fn gc_roots(&self, reflog_cutoff: DateTime<Utc>) -> Result<Roots> {
        let mut roots = Roots::default();
        for rec in self.tree_records()? {
            let tree = rec.name.as_str();
            for b in self.branches(tree, true)? {
                roots.snapshots.extend(b.tip);
            }
            for remote in self.remote_names(tree)? {
                for (_, tip) in self.remote_branches(tree, &remote)? {
                    roots.snapshots.insert(tip);
                }
            }
            for t in self.tags(tree)? {
                roots.snapshots.insert(t.target);
            }
            for e in self.reflog(tree, None)? {
                if e.at >= reflog_cutoff {
                    roots.snapshots.extend(e.old);
                    roots.snapshots.extend(e.new);
                }
            }
            if let Some(MergeState {
                source_tip,
                target_tip,
                base,
                ..
            }) = self.merge_state(tree)?
            {
                roots.snapshots.insert(source_tip);
                roots.snapshots.extend(target_tip);
                roots.snapshots.extend(base);
            }
            // Stash entries and any other JSON state (rebase / cherry-pick
            // progress) of the tree.
            for path in self.state_files(tree)? {
                self.scan_state_file(&path, &mut roots)?;
            }
            let conflicts = self.conflicts_path(tree);
            self.scan_state_file(&conflicts, &mut roots)?;
        }
        Ok(roots)
    }

    /// Mark everything reachable from `roots`. With `strict`, a reachable
    /// manifest or chunk list that cannot be decoded is an error (gc must
    /// not guess what it references); directly named root objects are
    /// always expanded best effort.
    fn mark(&self, roots: &Roots, strict: bool) -> Result<Marked> {
        let mut marked = Marked::default();
        let mut stack: Vec<SnapshotId> = roots.snapshots.iter().cloned().collect();
        while let Some(id) = stack.pop() {
            if marked.snapshots.contains(&id) {
                continue;
            }
            let snap = match self.load_snapshot_opt(&id) {
                Ok(Some(s)) => s,
                // Missing records (shallow history) end the walk.
                Ok(None) => continue,
                Err(e) if strict => {
                    return Err(SdkError::Other(format!(
                        "gc stopped without deleting anything: reachable snapshot {id} cannot be read ({e}); run `wt fsck`"
                    )))
                }
                Err(_) => continue,
            };
            marked.snapshots.insert(id);
            stack.extend(
                snap.parents
                    .iter()
                    .filter(|p| !marked.snapshots.contains(*p))
                    .cloned(),
            );
            self.mark_manifest(&snap.manifest, strict, &mut marked)?;
        }
        for hash in &roots.objects {
            // Unknown kind: keep it, and what it references if it is a
            // manifest.
            self.mark_manifest(hash, false, &mut marked)?;
        }
        Ok(marked)
    }

    fn mark_manifest(&self, hash: &ObjectHash, strict: bool, marked: &mut Marked) -> Result<()> {
        if !marked.objects.insert(*hash) {
            return Ok(());
        }
        let data = match self.store.get(hash) {
            Ok(d) => d,
            Err(SdkError::ObjectNotFound(_)) => return Ok(()),
            Err(e) if strict => return Err(refuse(hash, &e)),
            Err(_) => return Ok(()),
        };
        let manifest = match Manifest::from_bytes(&data) {
            Ok(m) => m,
            Err(e) if strict => return Err(refuse(hash, &e)),
            Err(_) => return Ok(()),
        };
        for entry in &manifest.entries {
            if !marked.objects.insert(entry.hash) || !entry.chunked {
                continue;
            }
            match self.store.read_chunk_list(&entry.hash) {
                Ok(list) => marked.objects.extend(list.chunks.iter().map(|c| c.hash)),
                Err(SdkError::ObjectNotFound(_)) => {}
                Err(e) if strict => return Err(refuse(&entry.hash, &e)),
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Collect snapshot ids and object hashes named anywhere in a JSON or
    /// JSON-lines state file.
    fn scan_state_file(&self, path: &Path, roots: &mut Roots) -> Result<()> {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let mut values = Vec::new();
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => values.push(v),
            Err(_) => values.extend(
                text.lines()
                    .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()),
            ),
        }
        let mut strings = Vec::new();
        for v in &values {
            collect_strings(v, &mut strings);
        }
        for s in strings {
            if let Ok(hash) = s.parse::<ObjectHash>() {
                if s.len() == 64 {
                    roots.objects.insert(hash);
                    continue;
                }
            }
            if is_valid_id(&s) && self.has_snapshot(&s) {
                roots.snapshots.insert(s);
            }
        }
        Ok(())
    }

    /// JSON / JSON-lines files directly in `trees/<tree>/` that may hold
    /// roots (everything but the reflog and the stat cache).
    fn state_files(&self, tree: &str) -> Result<Vec<PathBuf>> {
        let dir = self.tree_meta_dir(tree);
        let entries = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let json = name.ends_with(".json") || name.ends_with(".jsonl");
            if json
                && !name.starts_with(fsutil::TEMP_PREFIX)
                && !NON_ROOT_STATE_FILES.contains(&name)
                && entry.file_type()?.is_file()
            {
                out.push(entry.path());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Names of the remotes with tracking refs in `tree`.
    fn remote_names(&self, tree: &str) -> Result<Vec<String>> {
        let dir = self.tree_meta_dir(tree).join("refs").join("remotes");
        let entries = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                if super::names::validate_remote_name(name).is_ok() {
                    out.push(name.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Every snapshot record file as (id, path).
    fn snapshot_files(&self) -> Result<Vec<(SnapshotId, PathBuf)>> {
        let entries = match fs::read_dir(self.snapshots_dir()) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
                continue;
            };
            if is_valid_id(id) && entry.file_type()?.is_file() {
                out.push((id.to_string(), entry.path()));
            }
        }
        out.sort();
        Ok(out)
    }
}

fn issue(kind: FsckIssueKind, id: impl Into<String>, detail: impl Into<String>) -> FsckIssue {
    FsckIssue {
        kind,
        id: id.into(),
        detail: detail.into(),
    }
}

fn refuse(hash: &ObjectHash, err: &SdkError) -> SdkError {
    SdkError::Other(format!(
        "gc stopped without deleting anything: reachable object {hash} cannot be read ({err}); run `wt fsck`"
    ))
}

/// Whether `path` was modified at or after `cutoff` (unknown = yes).
fn modified_since(path: &Path, cutoff: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|m| m >= cutoff)
        .unwrap_or(true)
}

fn collect_strings(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}
