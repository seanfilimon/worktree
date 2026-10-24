//! `Repository` — the local engine over a `.wt/` directory.
//!
//! On-disk layout (see docs/ENGINEERING_PLAN.md §3):
//!
//! ```text
//! .wt/
//!   config.toml                 WorktreeConfig
//!   ignore                      extra ignore patterns (one per line)
//!   objects/<hh>/<62 hex>       blobs + manifests (ObjectStore)
//!   snapshots/<id>.json         Snapshot records
//!   trees/<tree>/tree.toml      TreeInfo (name, id, path, default branch, remote name)
//!   trees/<tree>/HEAD           current branch name
//!   trees/<tree>/refs/branches/<branch>        snapshot id (or empty)
//!   trees/<tree>/refs/branches.meta.json       created_at / archived flags
//!   trees/<tree>/refs/remotes/<remote>/<branch> snapshot id
//!   trees/<tree>/refs/tags/<tag>.json          TagInfo
//!   trees/<tree>/index.json     stat cache: path -> (size, mtime_ns, hash)
//!   trees/<tree>/reflog.jsonl   ReflogEntry per line (append-only)
//!   trees/<tree>/MERGE_STATE.json
//!   conflicts/<tree>.json       machine-readable conflict list during a merge
//! ```
//!
//! Nested trees are directories containing a `.wt-tree/` marker; a tree's
//! snapshot excludes nested trees' paths. `.wt/`, `.wt-tree/` and ignored
//! paths are never snapshotted.

use std::path::{Path, PathBuf};

use crate::config::WorktreeConfig;
use crate::error::Result;
use crate::model::*;
use crate::store::{ObjectHash, ObjectStore};

pub struct Repository {
    root: PathBuf,
    config: WorktreeConfig,
    store: ObjectStore,
}

impl Repository {
    // ----------------------------------------------------------------
    // Lifecycle
    // ----------------------------------------------------------------

    /// Create `.wt/` at `path` (creating the directory if needed) with a root
    /// tree named after the directory, an empty default branch, and an
    /// "init" reflog entry. `AlreadyInitialized` if `.wt/` exists.
    pub fn init(path: &Path, opts: InitOptions) -> Result<Self> {
        let _ = (path, opts);
        todo!("sdk-core: Repository::init")
    }

    /// Open the worktree containing `path` (walks up to find `.wt/`).
    pub fn open(path: &Path) -> Result<Self> {
        let _ = path;
        todo!("sdk-core: Repository::open")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn wt_dir(&self) -> PathBuf {
        self.root.join(".wt")
    }

    pub fn config(&self) -> &WorktreeConfig {
        &self.config
    }

    /// Replace and persist the config.
    pub fn set_config(&mut self, config: WorktreeConfig) -> Result<()> {
        let _ = config;
        todo!("sdk-core: Repository::set_config")
    }

    pub fn store(&self) -> &ObjectStore {
        &self.store
    }

    /// Author string for new snapshots: config user ("Name <email>"), else
    /// the logged-in username for the default remote, else the OS user.
    pub fn author(&self) -> String {
        todo!("sdk-core: Repository::author")
    }

    // ----------------------------------------------------------------
    // Trees
    // ----------------------------------------------------------------

    pub fn trees(&self) -> Result<Vec<TreeInfo>> {
        todo!("sdk-core: Repository::trees")
    }

    pub fn tree(&self, name: &str) -> Result<TreeInfo> {
        let _ = name;
        todo!("sdk-core: Repository::tree")
    }

    /// The root tree (created by init).
    pub fn root_tree(&self) -> Result<TreeInfo> {
        todo!("sdk-core: Repository::root_tree")
    }

    /// Deepest tree whose directory contains `path` (absolute or relative to
    /// the current dir). Falls back to the root tree.
    pub fn tree_for_path(&self, path: &Path) -> Result<TreeInfo> {
        let _ = path;
        todo!("sdk-core: Repository::tree_for_path")
    }

    /// Register `rel_path` (relative to the root; created if missing) as a
    /// nested tree, writing `<rel_path>/.wt-tree/config.toml`.
    pub fn add_tree(&self, name: &str, rel_path: &Path) -> Result<TreeInfo> {
        let _ = (name, rel_path);
        todo!("sdk-core: Repository::add_tree")
    }

    /// Unregister a nested tree (files are kept). The root tree cannot be removed.
    pub fn remove_tree(&self, name: &str) -> Result<()> {
        let _ = name;
        todo!("sdk-core: Repository::remove_tree")
    }

    /// Set the server-side name of a tree.
    pub fn set_tree_remote_name(&self, tree: &str, remote_name: &str) -> Result<()> {
        let _ = (tree, remote_name);
        todo!("sdk-core: Repository::set_tree_remote_name")
    }

    /// Replace a tree's id (used by clone so the local id matches the server).
    pub fn set_tree_id(&self, tree: &str, id: &str) -> Result<()> {
        let _ = (tree, id);
        todo!("sdk-core: Repository::set_tree_id")
    }

    /// Absolute directory of a tree.
    pub fn tree_dir(&self, tree: &str) -> Result<PathBuf> {
        let _ = tree;
        todo!("sdk-core: Repository::tree_dir")
    }

    // ----------------------------------------------------------------
    // Status & snapshots
    // ----------------------------------------------------------------

    /// Compare the working directory with the tree's head snapshot.
    pub fn status(&self, tree: &str) -> Result<StatusReport> {
        let _ = tree;
        todo!("sdk-core: Repository::status")
    }

    /// Snapshot the tree's working state onto its current branch. Runs the
    /// secret scan on added/modified files unless `skip_secret_scan`
    /// (`SecretsDetected`). `NoChanges` if identical to head and not
    /// `allow_empty`. Refuses during a conflicted merge unless all conflict
    /// markers are gone (then it completes the merge with two parents).
    pub fn snapshot(&self, tree: &str, opts: SnapshotOptions) -> Result<Snapshot> {
        let _ = (tree, opts);
        todo!("sdk-core: Repository::snapshot")
    }

    /// Snapshots reachable from `rev` (default: current branch head), newest
    /// first, at most `limit`.
    pub fn log(&self, tree: &str, rev: Option<&str>, limit: Option<usize>) -> Result<Vec<Snapshot>> {
        let _ = (tree, rev, limit);
        todo!("sdk-core: Repository::log")
    }

    /// Load a snapshot by full id or unique prefix (>= 4 chars).
    pub fn get_snapshot(&self, id: &str) -> Result<Snapshot> {
        let _ = id;
        todo!("sdk-core: Repository::get_snapshot")
    }

    pub fn has_snapshot(&self, id: &str) -> bool {
        let _ = id;
        todo!("sdk-core: Repository::has_snapshot")
    }

    /// Resolve a revision in `tree`: "HEAD", "HEAD~N", "<branch>",
    /// "<branch>~N", "origin/<branch>", "<tag>", or an id prefix.
    pub fn resolve(&self, tree: &str, rev: &str) -> Result<Snapshot> {
        let _ = (tree, rev);
        todo!("sdk-core: Repository::resolve")
    }

    pub fn manifest_of(&self, snapshot: &Snapshot) -> Result<Manifest> {
        let _ = snapshot;
        todo!("sdk-core: Repository::manifest_of")
    }

    /// Content of `path` in `snapshot`.
    pub fn read_file_at(&self, snapshot: &Snapshot, path: &str) -> Result<Vec<u8>> {
        let _ = (snapshot, path);
        todo!("sdk-core: Repository::read_file_at")
    }

    // ----------------------------------------------------------------
    // Branches
    // ----------------------------------------------------------------

    /// Non-archived branches (all if `include_archived`), sorted by name.
    pub fn branches(&self, tree: &str, include_archived: bool) -> Result<Vec<BranchInfo>> {
        let _ = (tree, include_archived);
        todo!("sdk-core: Repository::branches")
    }

    pub fn current_branch(&self, tree: &str) -> Result<String> {
        let _ = tree;
        todo!("sdk-core: Repository::current_branch")
    }

    /// Create `name` pointing at `from` (a revision; default current head).
    pub fn create_branch(&self, tree: &str, name: &str, from: Option<&str>) -> Result<BranchInfo> {
        let _ = (tree, name, from);
        todo!("sdk-core: Repository::create_branch")
    }

    /// Switch branches, rewriting the working directory to the target head.
    /// `DirtyWorkingTree` if there are uncommitted changes and not `force`.
    pub fn switch_branch(&self, tree: &str, name: &str, opts: SwitchOptions) -> Result<()> {
        let _ = (tree, name, opts);
        todo!("sdk-core: Repository::switch_branch")
    }

    /// Soft-delete (archive) a branch. History stays reachable; the name can
    /// be reused only after `restore_branch`. Cannot archive the current or
    /// default branch.
    pub fn archive_branch(&self, tree: &str, name: &str) -> Result<()> {
        let _ = (tree, name);
        todo!("sdk-core: Repository::archive_branch")
    }

    pub fn restore_branch(&self, tree: &str, name: &str) -> Result<()> {
        let _ = (tree, name);
        todo!("sdk-core: Repository::restore_branch")
    }

    // ----------------------------------------------------------------
    // Merge / revert / restore
    // ----------------------------------------------------------------

    /// Merge `source` (branch or revision) into the current branch.
    /// Uses `crate::merge` for manifest/text merging. Requires a clean
    /// working tree. On conflicts writes markers into files, saves
    /// MERGE_STATE.json + `.wt/conflicts/<tree>.json`, and returns
    /// `MergeOutcome::Conflicts`.
    pub fn merge(&self, tree: &str, source: &str, opts: MergeOptions) -> Result<MergeOutcome> {
        let _ = (tree, source, opts);
        todo!("sdk-core: Repository::merge")
    }

    pub fn merge_state(&self, tree: &str) -> Result<Option<MergeState>> {
        let _ = tree;
        todo!("sdk-core: Repository::merge_state")
    }

    /// Restore the pre-merge head into the working tree and clear merge state.
    pub fn abort_merge(&self, tree: &str) -> Result<()> {
        let _ = tree;
        todo!("sdk-core: Repository::abort_merge")
    }

    /// Finish a conflicted merge once markers are resolved: creates the
    /// two-parent merge snapshot. `MergeConflict` if markers remain.
    pub fn continue_merge(&self, tree: &str, message: Option<&str>) -> Result<Snapshot> {
        let _ = (tree, message);
        todo!("sdk-core: Repository::continue_merge")
    }

    /// New snapshot on the current branch that undoes `rev`'s changes
    /// relative to its first parent (append-only; history is not rewritten).
    pub fn revert(&self, tree: &str, rev: &str) -> Result<Snapshot> {
        let _ = (tree, rev);
        todo!("sdk-core: Repository::revert")
    }

    /// Overwrite working files at `paths` (all if empty) with their content
    /// at `rev` (default HEAD), deleting files absent there.
    pub fn restore(&self, tree: &str, paths: &[String], rev: Option<&str>) -> Result<Vec<String>> {
        let _ = (tree, paths, rev);
        todo!("sdk-core: Repository::restore")
    }

    // ----------------------------------------------------------------
    // Diff
    // ----------------------------------------------------------------

    /// Working directory vs. `rev` (default HEAD).
    pub fn diff_working(&self, tree: &str, rev: Option<&str>, opts: &DiffOptions) -> Result<Vec<FileDiff>> {
        let _ = (tree, rev, opts);
        todo!("sdk-core: Repository::diff_working")
    }

    /// `from` vs. `to` (both revisions).
    pub fn diff_revs(&self, tree: &str, from: &str, to: &str, opts: &DiffOptions) -> Result<Vec<FileDiff>> {
        let _ = (tree, from, to, opts);
        todo!("sdk-core: Repository::diff_revs")
    }

    // ----------------------------------------------------------------
    // Tags
    // ----------------------------------------------------------------

    pub fn create_tag(&self, tree: &str, name: &str, rev: Option<&str>, message: Option<&str>) -> Result<TagInfo> {
        let _ = (tree, name, rev, message);
        todo!("sdk-core: Repository::create_tag")
    }

    pub fn tags(&self, tree: &str) -> Result<Vec<TagInfo>> {
        let _ = tree;
        todo!("sdk-core: Repository::tags")
    }

    pub fn delete_tag(&self, tree: &str, name: &str) -> Result<()> {
        let _ = (tree, name);
        todo!("sdk-core: Repository::delete_tag")
    }

    // ----------------------------------------------------------------
    // Reflog & ignore
    // ----------------------------------------------------------------

    /// Newest first.
    pub fn reflog(&self, tree: &str, limit: Option<usize>) -> Result<Vec<ReflogEntry>> {
        let _ = (tree, limit);
        todo!("sdk-core: Repository::reflog")
    }

    pub fn append_reflog(&self, entry: &ReflogEntry) -> Result<()> {
        let _ = entry;
        todo!("sdk-core: Repository::append_reflog")
    }

    /// Effective ignore patterns: built-ins, `.wt/ignore`, `.wtignore` files.
    pub fn ignore_patterns(&self) -> Result<Vec<String>> {
        todo!("sdk-core: Repository::ignore_patterns")
    }

    pub fn add_ignore_pattern(&self, pattern: &str) -> Result<()> {
        let _ = pattern;
        todo!("sdk-core: Repository::add_ignore_pattern")
    }

    /// Whether `rel_path` ('/'-separated, relative to root) is ignored.
    pub fn is_ignored(&self, rel_path: &str) -> Result<bool> {
        let _ = rel_path;
        todo!("sdk-core: Repository::is_ignored")
    }

    // ----------------------------------------------------------------
    // Plumbing (used by remote sync and the Git bridge)
    // ----------------------------------------------------------------

    pub fn write_manifest(&self, manifest: &Manifest) -> Result<ObjectHash> {
        let _ = manifest;
        todo!("sdk-core: Repository::write_manifest")
    }

    pub fn read_manifest(&self, hash: &ObjectHash) -> Result<Manifest> {
        let _ = hash;
        todo!("sdk-core: Repository::read_manifest")
    }

    /// Persist a snapshot record (idempotent). Does not move any ref.
    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let _ = snapshot;
        todo!("sdk-core: Repository::write_snapshot")
    }

    pub fn branch_tip(&self, tree: &str, branch: &str) -> Result<Option<SnapshotId>> {
        let _ = (tree, branch);
        todo!("sdk-core: Repository::branch_tip")
    }

    /// Move (or create) a branch ref. Does not touch the working tree.
    pub fn set_branch_tip(&self, tree: &str, branch: &str, tip: Option<&str>) -> Result<()> {
        let _ = (tree, branch, tip);
        todo!("sdk-core: Repository::set_branch_tip")
    }

    pub fn remote_tip(&self, tree: &str, remote: &str, branch: &str) -> Result<Option<SnapshotId>> {
        let _ = (tree, remote, branch);
        todo!("sdk-core: Repository::remote_tip")
    }

    pub fn set_remote_tip(&self, tree: &str, remote: &str, branch: &str, tip: Option<&str>) -> Result<()> {
        let _ = (tree, remote, branch, tip);
        todo!("sdk-core: Repository::set_remote_tip")
    }

    /// Write a tag record directly (used by pull/import).
    pub fn put_tag(&self, tree: &str, tag: &TagInfo) -> Result<()> {
        let _ = (tree, tag);
        todo!("sdk-core: Repository::put_tag")
    }

    /// All snapshots reachable from `tip`, parents before children.
    pub fn ancestry(&self, tip: &str) -> Result<Vec<Snapshot>> {
        let _ = tip;
        todo!("sdk-core: Repository::ancestry")
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let _ = (ancestor, descendant);
        todo!("sdk-core: Repository::is_ancestor")
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<SnapshotId>> {
        let _ = (a, b);
        todo!("sdk-core: Repository::merge_base")
    }

    /// Rewrite the tree's working directory to `snapshot` (removing tracked
    /// files absent from it, never touching ignored or nested-tree paths) and
    /// refresh the index. `DirtyWorkingTree` unless clean or `force`.
    pub fn checkout(&self, tree: &str, snapshot: &str, force: bool) -> Result<()> {
        let _ = (tree, snapshot, force);
        todo!("sdk-core: Repository::checkout")
    }

    /// Build a manifest of the current working directory, storing blobs.
    pub fn manifest_from_working(&self, tree: &str) -> Result<Manifest> {
        let _ = tree;
        todo!("sdk-core: Repository::manifest_from_working")
    }
}
