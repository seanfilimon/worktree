//! Content-addressable persistence backend.
//!
//! Schema on top of `worktree-store`'s generic primitives:
//!
//! - **Snapshots** — bincode [`SnapshotRecord`] in the `snapshots/` CAS;
//!   the snapshot's ID *is* its content hash (hex).
//! - **Manifests** — bincode `Vec<FileEntry>` (sorted by path) in
//!   `manifests/`; a snapshot references its manifest by hash.
//! - **Blobs** — raw file bytes in `blobs/` (automatic dedup).
//! - **Branch refs** — JSON [`BranchRef`] at `refs/branches/<tree>/<name>`.
//! - **Tags** — JSON [`TagRecord`] at `refs/tags/<tree>/<name>`.
//! - **Worktree meta** — JSON [`Meta`] at `state/meta.json` (worktree name,
//!   trees, current tree/branch), `state/head` mirrors `<tree>/<branch>`.
//! - **Reflog** — tab-separated lines in `.wt/reflog/` per DotWt.md.
//!
//! Mutations take the store's writer lock; reads are lock-free (objects are
//! immutable, refs/state replace atomically).

use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::identity;
use crate::persist::{BranchState, FileEntry, SnapshotState, TagState, TreeState, WorktreeState};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use worktree_protocol::core::hash::ContentHash;
use worktree_store::{LocalStore, ObjectKind};

const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// On-disk schema
// ---------------------------------------------------------------------------

/// `state/meta.json` — worktree-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    pub name: String,
    pub created_at: String,
    pub current_tree: Option<String>,
    pub trees: Vec<TreeMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TreeMeta {
    pub name: String,
    pub path: String,
    pub current_branch: String,
}

/// JSON value of a branch ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchRef {
    tip: Option<String>,
    created_at: String,
}

/// JSON value of a tag ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TagRecord {
    target_snapshot: String,
    message: Option<String>,
    tagger: Option<String>,
    created_at: String,
}

/// Bincode payload of a snapshot object. Field order is part of the wire
/// format — the content hash of this record is the snapshot's identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotRecord {
    message: String,
    author: String,
    timestamp: String,
    parents: Vec<String>,
    tree_name: String,
    branch_name: String,
    /// Content hash (hex) of the manifest object.
    manifest: String,
    auto_generated: bool,
}

/// Inputs for committing a snapshot.
pub struct NewSnapshot<'a> {
    pub tree_name: &'a str,
    pub branch_name: &'a str,
    pub message: &'a str,
    /// Parent snapshot ids; first parent is the branch tip being advanced.
    pub parents: Vec<String>,
    pub files: Vec<FileEntry>,
    pub auto_generated: bool,
    /// Reflog operation name (DotWt.md): `snapshot`, `merge`, `revert`.
    pub operation: &'a str,
}

// ---------------------------------------------------------------------------
// Store access
// ---------------------------------------------------------------------------

pub(crate) fn open_store(engine: &WorktreeEngine) -> Result<LocalStore> {
    LocalStore::open(engine.root()).map_err(store_err)
}

fn store_err(e: worktree_store::StoreError) -> EngineError {
    match e {
        worktree_store::StoreError::Io(io) => EngineError::Io(io),
        other => EngineError::Serialization(other.to_string()),
    }
}

fn parse_hash(hex: &str) -> Result<ContentHash> {
    hex.parse()
        .map_err(|_| EngineError::SnapshotNotFound(format!("invalid object id '{hex}'")))
}

fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|e| EngineError::Serialization(e.to_string()))
}

fn from_json<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|e| EngineError::Serialization(e.to_string()))
}

fn bin<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(|e| EngineError::Serialization(e.to_string()))
}

fn from_bin<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    bincode::deserialize(bytes).map_err(|e| EngineError::Serialization(e.to_string()))
}

pub(crate) fn read_meta(store: &LocalStore) -> Result<Option<Meta>> {
    let state = store.state().map_err(store_err)?;
    match state.read("meta.json").map_err(store_err)? {
        Some(contents) => Ok(Some(from_json(&contents)?)),
        None => Ok(None),
    }
}

pub(crate) fn write_meta(store: &LocalStore, meta: &Meta) -> Result<()> {
    let state = store.state().map_err(store_err)?;
    state.write("meta.json", &json(meta)?).map_err(store_err)?;
    // Mirror the current checkout into `state/head` (Storage.md layout).
    if let Some(current) = &meta.current_tree {
        if let Some(tree) = meta.trees.iter().find(|t| &t.name == current) {
            state
                .write("head", &format!("{}/{}", tree.name, tree.current_branch))
                .map_err(store_err)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Initialize the store for a fresh worktree: meta, root tree, main branch.
pub fn init_store(engine: &WorktreeEngine, name: &str) -> Result<()> {
    let store = open_store(engine)?;
    let now = chrono::Utc::now().to_rfc3339();

    let branch_ref = BranchRef {
        tip: None,
        created_at: now.clone(),
    };
    store
        .refs()
        .write("branches/root", "main", &json(&branch_ref)?)
        .map_err(store_err)?;

    write_meta(
        &store,
        &Meta {
            name: name.to_string(),
            created_at: now,
            current_tree: Some("root".to_string()),
            trees: vec![TreeMeta {
                name: "root".to_string(),
                path: ".".to_string(),
                current_branch: "main".to_string(),
            }],
        },
    )
}

// ---------------------------------------------------------------------------
// Read path
// ---------------------------------------------------------------------------

/// Load the full worktree state from the store.
pub fn load_state(engine: &WorktreeEngine) -> Result<WorktreeState> {
    super::migrate::migrate_if_needed(engine)?;

    let store = open_store(engine)?;
    let meta = read_meta(&store)?.ok_or(EngineError::NotAWorktree)?;
    let objects = store.objects();
    let refs = store.refs();

    let mut trees = Vec::with_capacity(meta.trees.len());
    for tree_meta in &meta.trees {
        let mut branches = Vec::new();
        for (name, value) in refs
            .list(&format!("branches/{}", tree_meta.name))
            .map_err(store_err)?
        {
            let branch_ref: BranchRef = from_json(&value)?;
            branches.push(BranchState {
                name,
                tip: branch_ref.tip,
                created_at: branch_ref.created_at,
            });
        }

        let mut tags = Vec::new();
        for (name, value) in refs
            .list(&format!("tags/{}", tree_meta.name))
            .map_err(store_err)?
        {
            let record: TagRecord = from_json(&value)?;
            tags.push(TagState {
                name,
                target_snapshot: record.target_snapshot,
                message: record.message,
                tagger: record.tagger,
                created_at: record.created_at,
            });
        }

        // Every snapshot reachable from a branch tip or tag target.
        let roots = branches
            .iter()
            .filter_map(|b| b.tip.clone())
            .chain(tags.iter().map(|t| t.target_snapshot.clone()));
        let mut snapshots = collect_snapshots(&objects, roots)?;
        snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        trees.push(TreeState {
            name: tree_meta.name.clone(),
            path: tree_meta.path.clone(),
            branches,
            current_branch: tree_meta.current_branch.clone(),
            snapshots,
            tags,
        });
    }

    Ok(WorktreeState {
        name: meta.name,
        trees,
        current_tree: meta.current_tree,
        created_at: meta.created_at,
    })
}

fn collect_snapshots(
    objects: &worktree_store::ObjectStore,
    roots: impl Iterator<Item = String>,
) -> Result<Vec<SnapshotState>> {
    let mut queue: VecDeque<String> = roots.collect();
    let mut seen: HashSet<String> = queue.iter().cloned().collect();
    let mut out = Vec::new();

    while let Some(hex) = queue.pop_front() {
        let snapshot = load_snapshot(objects, &hex)?;
        for parent in &snapshot.parents {
            if seen.insert(parent.clone()) {
                queue.push_back(parent.clone());
            }
        }
        out.push(snapshot);
    }
    Ok(out)
}

fn load_snapshot(objects: &worktree_store::ObjectStore, hex: &str) -> Result<SnapshotState> {
    let hash = parse_hash(hex)?;
    let bytes = objects
        .get(ObjectKind::Snapshot, &hash)
        .map_err(store_err)?;
    let record: SnapshotRecord = from_bin(&bytes)?;

    let manifest_hash = parse_hash(&record.manifest)?;
    let manifest_bytes = objects
        .get(ObjectKind::Manifest, &manifest_hash)
        .map_err(store_err)?;
    let files: Vec<FileEntry> = from_bin(&manifest_bytes)?;

    Ok(SnapshotState {
        id: hex.to_string(),
        message: record.message,
        author: record.author,
        timestamp: record.timestamp,
        parents: record.parents,
        tree_name: record.tree_name,
        branch_name: record.branch_name,
        files,
        auto_generated: record.auto_generated,
    })
}

// ---------------------------------------------------------------------------
// Write path
// ---------------------------------------------------------------------------

/// Store a snapshot: blobs + manifest + snapshot object, advance the branch
/// tip, and append reflog entries.
pub fn commit_snapshot(engine: &WorktreeEngine, new: NewSnapshot<'_>) -> Result<SnapshotState> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;
    let objects = store.objects();
    let refs = store.refs();

    // 1. Blobs. Content already in the CAS (e.g. from the other side of a
    //    merge) is skipped; otherwise read from the working directory.
    for entry in &new.files {
        let hash = parse_hash(&entry.hash)?;
        if objects.contains(ObjectKind::Blob, &hash) {
            continue;
        }
        let abs = engine.root().join(&entry.path);
        let data = std::fs::read(&abs).map_err(|e| {
            EngineError::Serialization(format!(
                "cannot store blob for '{}': {e} (content missing from store and working directory)",
                entry.path
            ))
        })?;
        let stored = objects.put(ObjectKind::Blob, &data).map_err(store_err)?;
        if stored != hash {
            return Err(EngineError::Serialization(format!(
                "file '{}' changed while snapshotting (hash mismatch)",
                entry.path
            )));
        }
    }

    // 2. Manifest (sorted for a canonical encoding).
    let mut files = new.files;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest_hash = objects
        .put(ObjectKind::Manifest, &bin(&files)?)
        .map_err(store_err)?;

    // 3. Snapshot object — its content hash is the snapshot id.
    let record = SnapshotRecord {
        message: new.message.to_string(),
        author: identity::author(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        parents: new.parents.clone(),
        tree_name: new.tree_name.to_string(),
        branch_name: new.branch_name.to_string(),
        manifest: manifest_hash.to_hex(),
        auto_generated: new.auto_generated,
    };
    let snapshot_hash = objects
        .put(ObjectKind::Snapshot, &bin(&record)?)
        .map_err(store_err)?;
    let snapshot_id = snapshot_hash.to_hex();

    // 4. Advance the branch tip.
    let namespace = format!("branches/{}", new.tree_name);
    let value = refs
        .read(&namespace, new.branch_name)
        .map_err(store_err)?
        .ok_or_else(|| EngineError::BranchNotFound(new.branch_name.to_string()))?;
    let mut branch_ref: BranchRef = from_json(&value)?;
    let before = branch_ref.tip.clone();
    branch_ref.tip = Some(snapshot_id.clone());
    refs.write(&namespace, new.branch_name, &json(&branch_ref)?)
        .map_err(store_err)?;

    // 5. Reflog.
    append_reflog(
        engine,
        new.branch_name,
        new.operation,
        before.as_deref(),
        Some(&snapshot_id),
        new.message,
    )?;

    Ok(SnapshotState {
        id: snapshot_id,
        message: record.message,
        author: record.author,
        timestamp: record.timestamp,
        parents: record.parents,
        tree_name: record.tree_name,
        branch_name: record.branch_name,
        files,
        auto_generated: record.auto_generated,
    })
}

/// Create a branch pointing at `tip`.
pub fn create_branch(
    engine: &WorktreeEngine,
    tree_name: &str,
    name: &str,
    tip: Option<String>,
) -> Result<BranchState> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let branch_ref = BranchRef {
        tip: tip.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store
        .refs()
        .write(&format!("branches/{tree_name}"), name, &json(&branch_ref)?)
        .map_err(store_err)?;

    append_reflog(
        engine,
        name,
        "branch:create",
        None,
        tip.as_deref(),
        &format!("created branch {name}"),
    )?;

    Ok(BranchState {
        name: name.to_string(),
        tip,
        created_at: branch_ref.created_at,
    })
}

/// Delete a branch ref (guards live in the calling op).
pub fn delete_branch(engine: &WorktreeEngine, tree_name: &str, name: &str) -> Result<()> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let namespace = format!("branches/{tree_name}");
    let before = store
        .refs()
        .read(&namespace, name)
        .map_err(store_err)?
        .and_then(|v| from_json::<BranchRef>(&v).ok())
        .and_then(|r| r.tip);
    store.refs().delete(&namespace, name).map_err(|e| match e {
        worktree_store::StoreError::RefNotFound(_) => EngineError::BranchNotFound(name.to_string()),
        other => store_err(other),
    })?;

    append_reflog(
        engine,
        name,
        "branch:delete",
        before.as_deref(),
        None,
        &format!("deleted branch {name}"),
    )
}

/// Switch the current branch of a tree.
pub fn set_current_branch(engine: &WorktreeEngine, tree_name: &str, branch: &str) -> Result<()> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let mut meta = read_meta(&store)?.ok_or(EngineError::NotAWorktree)?;
    let tree = meta
        .trees
        .iter_mut()
        .find(|t| t.name == tree_name)
        .ok_or_else(|| EngineError::TreeNotFound(tree_name.to_string()))?;
    tree.current_branch = branch.to_string();
    write_meta(&store, &meta)
}

/// Switch the current tree.
pub fn set_current_tree(engine: &WorktreeEngine, tree_name: &str) -> Result<()> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let mut meta = read_meta(&store)?.ok_or(EngineError::NotAWorktree)?;
    if !meta.trees.iter().any(|t| t.name == tree_name) {
        return Err(EngineError::TreeNotFound(tree_name.to_string()));
    }
    meta.current_tree = Some(tree_name.to_string());
    write_meta(&store, &meta)
}

/// Register a new tree (directory scaffolding is the op's job).
pub fn add_tree(engine: &WorktreeEngine, name: &str, path: &str) -> Result<TreeState> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut meta = read_meta(&store)?.ok_or(EngineError::NotAWorktree)?;
    meta.trees.push(TreeMeta {
        name: name.to_string(),
        path: path.to_string(),
        current_branch: "main".to_string(),
    });

    let branch_ref = BranchRef {
        tip: None,
        created_at: now.clone(),
    };
    store
        .refs()
        .write(&format!("branches/{name}"), "main", &json(&branch_ref)?)
        .map_err(store_err)?;
    write_meta(&store, &meta)?;

    Ok(TreeState {
        name: name.to_string(),
        path: path.to_string(),
        branches: vec![BranchState {
            name: "main".to_string(),
            tip: None,
            created_at: now,
        }],
        current_branch: "main".to_string(),
        snapshots: Vec::new(),
        tags: Vec::new(),
    })
}

/// Unregister a tree and drop its refs. Snapshot/blob objects stay in the
/// CAS until garbage collection (WT-PHASE-9).
pub fn remove_tree(engine: &WorktreeEngine, name: &str) -> Result<()> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let mut meta = read_meta(&store)?.ok_or(EngineError::NotAWorktree)?;
    let before = meta.trees.len();
    meta.trees.retain(|t| t.name != name);
    if meta.trees.len() == before {
        return Err(EngineError::TreeNotFound(name.to_string()));
    }
    if meta.current_tree.as_deref() == Some(name) {
        meta.current_tree = Some("root".to_string());
    }
    write_meta(&store, &meta)?;

    for namespace in ["branches", "tags"] {
        let dir = store.dir().join("refs").join(namespace).join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    Ok(())
}

/// Create a tag pointing at `target_snapshot`.
pub fn create_tag(
    engine: &WorktreeEngine,
    tree_name: &str,
    name: &str,
    target_snapshot: &str,
    message: Option<&str>,
) -> Result<TagState> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    let record = TagRecord {
        target_snapshot: target_snapshot.to_string(),
        message: message.map(|m| m.to_string()),
        tagger: identity::author_opt(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    store
        .refs()
        .write(&format!("tags/{tree_name}"), name, &json(&record)?)
        .map_err(store_err)?;

    Ok(TagState {
        name: name.to_string(),
        target_snapshot: record.target_snapshot,
        message: record.message,
        tagger: record.tagger,
        created_at: record.created_at,
    })
}

/// Delete a tag ref.
pub fn delete_tag(engine: &WorktreeEngine, tree_name: &str, name: &str) -> Result<()> {
    let store = open_store(engine)?;
    let state = store.state().map_err(store_err)?;
    let _lock = state.lock_exclusive(LOCK_TIMEOUT).map_err(store_err)?;

    store
        .refs()
        .delete(&format!("tags/{tree_name}"), name)
        .map_err(|e| match e {
            worktree_store::StoreError::RefNotFound(_) => {
                EngineError::TagNotFound(name.to_string())
            }
            other => store_err(other),
        })
}

// ---------------------------------------------------------------------------
// Migration support (see persist::migrate)
// ---------------------------------------------------------------------------

/// Store a legacy snapshot verbatim (original author/timestamp preserved,
/// blobs unavailable — the JSON backend never stored content). Returns the
/// new content-hash id. Caller holds the store lock.
pub(crate) fn store_migrated_snapshot(
    engine: &WorktreeEngine,
    old: &SnapshotState,
    parents: Vec<String>,
) -> Result<String> {
    let store = open_store(engine)?;
    let objects = store.objects();

    let mut files = old.files.clone();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest_hash = objects
        .put(ObjectKind::Manifest, &bin(&files)?)
        .map_err(store_err)?;

    let record = SnapshotRecord {
        message: old.message.clone(),
        author: old.author.clone(),
        timestamp: old.timestamp.clone(),
        parents,
        tree_name: old.tree_name.clone(),
        branch_name: old.branch_name.clone(),
        manifest: manifest_hash.to_hex(),
        auto_generated: old.auto_generated,
    };
    let hash = objects
        .put(ObjectKind::Snapshot, &bin(&record)?)
        .map_err(store_err)?;
    Ok(hash.to_hex())
}

/// Write a branch ref during migration, preserving its creation time.
pub(crate) fn write_migrated_branch(
    engine: &WorktreeEngine,
    tree_name: &str,
    name: &str,
    tip: Option<String>,
    created_at: &str,
) -> Result<()> {
    let store = open_store(engine)?;
    let branch_ref = BranchRef {
        tip,
        created_at: created_at.to_string(),
    };
    store
        .refs()
        .write(&format!("branches/{tree_name}"), name, &json(&branch_ref)?)
        .map_err(store_err)
}

/// Write a tag ref during migration with a remapped target.
pub(crate) fn write_migrated_tag(
    engine: &WorktreeEngine,
    tree_name: &str,
    tag: &TagState,
    target: &str,
) -> Result<()> {
    let store = open_store(engine)?;
    let record = TagRecord {
        target_snapshot: target.to_string(),
        message: tag.message.clone(),
        tagger: tag.tagger.clone(),
        created_at: tag.created_at.clone(),
    };
    store
        .refs()
        .write(&format!("tags/{tree_name}"), &tag.name, &json(&record)?)
        .map_err(store_err)
}

// ---------------------------------------------------------------------------
// Reflog (.wt/reflog/ per DotWt.md)
// ---------------------------------------------------------------------------

/// Append a reflog line to the branch's log and the global log.
///
/// Line format (DotWt.md §reflog):
/// `<ISO-8601>\t<operation>\t<before>\t<after>\t<user>\t<message>`
pub fn append_reflog(
    engine: &WorktreeEngine,
    branch: &str,
    operation: &str,
    before: Option<&str>,
    after: Option<&str>,
    message: &str,
) -> Result<()> {
    let line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        chrono::Utc::now().to_rfc3339(),
        operation,
        before.unwrap_or("-"),
        after.unwrap_or("-"),
        identity::author(),
        message.replace(['\t', '\n'], " "),
    );

    let reflog_dir = engine.reflog_dir();
    for file_name in [format!("{branch}.log"), "_global.log".to_string()] {
        let path = reflog_dir.join(&file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(line.as_bytes())?;
    }
    Ok(())
}

/// Read up to `count` reflog lines for a branch, newest first.
pub fn read_reflog(engine: &WorktreeEngine, branch: &str, count: usize) -> Result<Vec<String>> {
    let path = engine.reflog_dir().join(format!("{branch}.log"));
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(contents
        .lines()
        .rev()
        .take(count)
        .map(|l| l.to_string())
        .collect())
}

// ---------------------------------------------------------------------------
// Doctor
// ---------------------------------------------------------------------------

/// Store health report for `wt doctor`.
pub struct DoctorReport {
    pub store_dir: std::path::PathBuf,
    /// `(kind name, object count)` per object kind.
    pub counts: Vec<(&'static str, usize)>,
    /// `(kind name, object hex, error)` per corrupt object.
    pub corrupt: Vec<(&'static str, String, String)>,
}

/// Count and verify every object in the store.
pub fn doctor(engine: &WorktreeEngine) -> Result<DoctorReport> {
    let store = open_store(engine)?;
    let objects = store.objects();

    let mut counts = Vec::new();
    let mut corrupt = Vec::new();
    for kind in ObjectKind::ALL {
        counts.push((kind.dir_name(), objects.count(kind)));
        for (hex, error) in objects.verify_kind(kind) {
            corrupt.push((kind.dir_name(), hex, error));
        }
    }

    Ok(DoctorReport {
        store_dir: store.dir().to_path_buf(),
        counts,
        corrupt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    /// All engine tests share one process-wide storage base; stores are
    /// isolated per worktree hash underneath it.
    fn init_test_storage() {
        static BASE: OnceLock<PathBuf> = OnceLock::new();
        BASE.get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().to_path_buf();
            std::mem::forget(dir); // keep alive for the whole test process
            std::env::set_var(worktree_store::paths::STORAGE_DIR_ENV, &path);
            path
        });
    }

    fn test_worktree() -> (tempfile::TempDir, WorktreeEngine) {
        init_test_storage();
        let dir = tempfile::tempdir().unwrap();
        let engine = WorktreeEngine::init(dir.path()).unwrap();
        (dir, engine)
    }

    fn entry_for(engine: &WorktreeEngine, rel: &str, contents: &str) -> FileEntry {
        let abs = engine.root().join(rel);
        std::fs::write(&abs, contents).unwrap();
        FileEntry {
            path: rel.to_string(),
            hash: blake3::hash(contents.as_bytes()).to_hex().to_string(),
            size: contents.len() as u64,
        }
    }

    #[test]
    fn commit_and_load_roundtrip() {
        let (_dir, engine) = test_worktree();
        let files = vec![entry_for(&engine, "a.txt", "content a")];

        let snap = commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "first",
                parents: vec![],
                files,
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        // Snapshot id is a content hash.
        assert_eq!(snap.id.len(), 64);

        let state = load_state(&engine).unwrap();
        let tree = state.find_tree("root").unwrap();
        assert_eq!(tree.snapshots.len(), 1);
        assert_eq!(tree.snapshots[0].id, snap.id);
        assert_eq!(tree.snapshots[0].message, "first");
        assert_eq!(
            tree.find_branch("main").unwrap().tip.as_deref(),
            Some(snap.id.as_str())
        );
    }

    #[test]
    fn identical_file_content_stores_one_blob() {
        let (_dir, engine) = test_worktree();
        let files = vec![
            entry_for(&engine, "x.txt", "same bytes"),
            entry_for(&engine, "y.txt", "same bytes"),
        ];

        commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "dedup",
                parents: vec![],
                files,
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        let store = open_store(&engine).unwrap();
        assert_eq!(store.objects().count(ObjectKind::Blob), 1);
    }

    #[test]
    fn parent_chain_is_reachable() {
        let (_dir, engine) = test_worktree();

        let first = commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "one",
                parents: vec![],
                files: vec![entry_for(&engine, "a.txt", "v1")],
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        let second = commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "two",
                parents: vec![first.id.clone()],
                files: vec![entry_for(&engine, "a.txt", "v2")],
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        let state = load_state(&engine).unwrap();
        let tree = state.find_tree("root").unwrap();
        assert_eq!(tree.snapshots.len(), 2);
        // Chronological order.
        assert_eq!(tree.snapshots[0].id, first.id);
        assert_eq!(tree.snapshots[1].id, second.id);
        assert_eq!(tree.snapshots[1].parents, vec![first.id.clone()]);
    }

    #[test]
    fn reflog_records_operations() {
        let (_dir, engine) = test_worktree();
        commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "logged",
                parents: vec![],
                files: vec![entry_for(&engine, "a.txt", "x")],
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        let lines = read_reflog(&engine, "main", 10).unwrap();
        assert_eq!(lines.len(), 1);
        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[1], "snapshot");
        assert_eq!(fields[2], "-"); // no previous tip
        assert_eq!(fields[5], "logged");

        // Global log mirrors it.
        let global = std::fs::read_to_string(engine.reflog_dir().join("_global.log")).unwrap();
        assert!(global.contains("snapshot"));
    }

    #[test]
    fn doctor_reports_counts_and_corruption() {
        let (_dir, engine) = test_worktree();
        commit_snapshot(
            &engine,
            NewSnapshot {
                tree_name: "root",
                branch_name: "main",
                message: "healthy",
                parents: vec![],
                files: vec![entry_for(&engine, "a.txt", "fine")],
                auto_generated: false,
                operation: "snapshot",
            },
        )
        .unwrap();

        let report = doctor(&engine).unwrap();
        assert!(report.corrupt.is_empty());
        let blobs = report.counts.iter().find(|(k, _)| *k == "blobs").unwrap();
        assert_eq!(blobs.1, 1);

        // Corrupt the blob on disk and re-run.
        let store = open_store(&engine).unwrap();
        let hex = store.objects().iter_hex(ObjectKind::Blob).pop().unwrap();
        let path = store
            .dir()
            .join("objects/blobs")
            .join(&hex[..2])
            .join(&hex[2..]);
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&path, bytes).unwrap();

        let report = doctor(&engine).unwrap();
        assert_eq!(report.corrupt.len(), 1);
        assert_eq!(report.corrupt[0].0, "blobs");
    }
}
