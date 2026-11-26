//! Git -> W0rkTree conversion (`wt git import`).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use git2::{BranchType, FileMode, ObjectType, Oid, Sort};
use tracing::{info, warn};
use worktree_sdk::{
    InitOptions, Manifest, ManifestEntry, ObjectHash, ObjectStore, Repository, Snapshot, TagInfo,
    TreeInfo,
};

use crate::bridge::{ImportOptions, ImportReport};
use crate::config::gitignore;
use crate::error::{GitCompatError, Result};
use crate::ident;
use crate::lfs::{Decision, LfsImport, LfsPointer, LfsReport, LfsResolver, MAX_POINTER_SIZE};
use crate::map::GitMap;
use crate::submodule::{self, SubmoduleMode, PINS_FILE};

/// How a converted file is stored: (hash, size, chunked).
type Stored = (ObjectHash, u64, bool);

/// Destination for blob contents read from Git.
pub(crate) trait BlobSink {
    fn put_blob(&self, data: &[u8]) -> Result<ObjectHash>;

    /// Store file content as a manifest entry references it: `(hash,
    /// chunked)` (v0.5: large files become chunk lists; §13.2).
    fn put_file(&self, data: &[u8]) -> Result<(ObjectHash, bool)> {
        Ok((self.put_blob(data)?, false))
    }
}

impl BlobSink for ObjectStore {
    fn put_blob(&self, data: &[u8]) -> Result<ObjectHash> {
        Ok(self.put(data)?)
    }

    fn put_file(&self, data: &[u8]) -> Result<(ObjectHash, bool)> {
        Ok(ObjectStore::put_file(self, data)?)
    }
}

/// Converts Git trees into manifests, storing each distinct blob once.
pub(crate) struct TreeConverter<'a, S: BlobSink + ?Sized> {
    git: &'a git2::Repository,
    sink: &'a S,
    blobs: HashMap<Oid, Stored>,
    warned: HashSet<String>,
    /// Git LFS handling (`None`: pointer files are ordinary files).
    lfs: Option<LfsImport>,
    /// Root of the tree being converted (for `.gitattributes` lookups).
    root: Option<Oid>,
    /// Stored LFS content by SHA-256, and stored pointer blobs.
    lfs_content: HashMap<String, Stored>,
    pointer_blobs: HashMap<Oid, Stored>,
    /// Gitlinks of the tree being converted (`None`: skip gitlinks).
    gitlinks: Option<Vec<(String, Oid)>>,
    pins_blobs: HashSet<ObjectHash>,
}

impl<'a, S: BlobSink + ?Sized> TreeConverter<'a, S> {
    /// A converter that imports pointer files as they are and skips
    /// gitlinks; see [`Self::with_lfs`] and [`Self::recording_gitlinks`].
    pub fn new(git: &'a git2::Repository, sink: &'a S) -> Self {
        Self {
            git,
            sink,
            blobs: HashMap::new(),
            warned: HashSet::new(),
            lfs: None,
            root: None,
            lfs_content: HashMap::new(),
            pointer_blobs: HashMap::new(),
            gitlinks: None,
            pins_blobs: HashSet::new(),
        }
    }

    /// Replace Git LFS pointer files by their content ([`crate::lfs`]).
    pub fn with_lfs(mut self, lfs: LfsImport) -> Self {
        self.lfs = Some(lfs);
        self
    }

    /// Record gitlinks in a [`PINS_FILE`] entry instead of skipping them.
    pub fn recording_gitlinks(mut self) -> Self {
        self.gitlinks = Some(Vec::new());
        self
    }

    /// Number of distinct blobs written to the sink so far.
    pub fn blob_count(&self) -> usize {
        self.blobs.len() + self.lfs_content.len() + self.pointer_blobs.len() + self.pins_blobs.len()
    }

    /// What happened to Git LFS files so far.
    pub fn lfs_report(&self) -> LfsReport {
        self.lfs.as_ref().map(LfsImport::report).unwrap_or_default()
    }

    /// Manifest of `tree` (entries sorted by path). Regular and executable
    /// files are converted; symlinks and non-UTF-8 names are skipped with a
    /// warning; gitlinks are recorded in [`PINS_FILE`] or skipped with a
    /// warning.
    pub fn manifest(&mut self, tree: &git2::Tree<'_>) -> Result<Manifest> {
        self.root = Some(tree.id());
        let mut entries = Vec::new();
        self.collect(tree, "", &mut entries)?;
        if let Some(links) = self.gitlinks.as_mut().map(std::mem::take) {
            if !links.is_empty() {
                let pins = self.pins_entry(tree, &links, &entries)?;
                entries.push(pins);
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Manifest { entries })
    }

    fn gitlink(&mut self, path: String, commit: Oid) {
        match self.gitlinks.as_mut() {
            Some(links) => links.push((path, commit)),
            None => self.warn_once(&path, "skipping submodule (gitlink)"),
        }
    }

    /// The [`PINS_FILE`] entry for the gitlinks of `tree`.
    fn pins_entry(
        &mut self,
        tree: &git2::Tree<'_>,
        links: &[(String, Oid)],
        entries: &[ManifestEntry],
    ) -> Result<ManifestEntry> {
        if entries.iter().any(|e| e.path == PINS_FILE) {
            return Err(GitCompatError::ImportError(format!(
                "Git tree {} has submodules and a {PINS_FILE} file; cannot record the submodule pins",
                tree.id()
            )));
        }
        let gitmodules = root_text_file(self.git, tree, ".gitmodules")?;
        let data = submodule::encode_pins(&submodule::pins_for(links, gitmodules.as_deref()))?;
        let (hash, chunked) = self.sink.put_file(&data)?;
        self.pins_blobs.insert(hash);
        Ok(ManifestEntry {
            path: PINS_FILE.to_owned(),
            hash,
            size: data.len() as u64,
            executable: false,
            chunked,
        })
    }

    fn collect(
        &mut self,
        tree: &git2::Tree<'_>,
        prefix: &str,
        out: &mut Vec<ManifestEntry>,
    ) -> Result<()> {
        for entry in tree.iter() {
            let Some(name) = entry.name() else {
                let lossy = String::from_utf8_lossy(entry.name_bytes()).into_owned();
                self.warn_once(
                    &join(prefix, &lossy),
                    "skipping entry with a non-UTF-8 name",
                );
                continue;
            };
            let path = join(prefix, name);
            let mode = entry.filemode();
            if mode == i32::from(FileMode::Tree) {
                let subtree = self.git.find_tree(entry.id())?;
                self.collect(&subtree, &path, out)?;
            } else if mode == i32::from(FileMode::Blob)
                || mode == i32::from(FileMode::BlobGroupWritable)
            {
                out.push(self.file(path, entry.id(), false)?);
            } else if mode == i32::from(FileMode::BlobExecutable) {
                out.push(self.file(path, entry.id(), true)?);
            } else if mode == i32::from(FileMode::Link) {
                self.warn_once(&path, "skipping symbolic link (not supported by W0rkTree)");
            } else if mode == i32::from(FileMode::Commit) {
                self.gitlink(path, entry.id());
            } else {
                self.warn_once(&path, &format!("skipping entry with unknown mode {mode:o}"));
            }
        }
        Ok(())
    }

    fn file(&mut self, path: String, oid: Oid, executable: bool) -> Result<ManifestEntry> {
        let (hash, size, chunked) = match self.blobs.get(&oid) {
            Some(known) => *known,
            None => {
                let blob = self.git.find_blob(oid)?;
                match self.lfs_file(&path, oid, blob.content())? {
                    Some(stored) => stored,
                    None => {
                        let (hash, chunked) = self.sink.put_file(blob.content())?;
                        let stored = (hash, blob.size() as u64, chunked);
                        self.blobs.insert(oid, stored);
                        stored
                    }
                }
            }
        };
        Ok(ManifestEntry {
            path,
            hash,
            size,
            executable,
            chunked,
        })
    }

    /// How a Git LFS pointer blob at `path` is stored (its content, or the
    /// pointer itself); `None` if `data` is not a pointer or LFS handling
    /// is off. Pointer blobs are never cached by blob id alone: whether
    /// one is an LFS file depends on the path's attributes.
    fn lfs_file(&mut self, path: &str, oid: Oid, data: &[u8]) -> Result<Option<Stored>> {
        let git = self.git;
        let (Some(lfs), Some(root)) = (self.lfs.as_mut(), self.root) else {
            return Ok(None);
        };
        if data.len() >= MAX_POINTER_SIZE {
            return Ok(None);
        }
        let Some(pointer) = LfsPointer::parse(data) else {
            return Ok(None);
        };
        let root = git.find_tree(root)?;
        match lfs.decide(git, &root, path, &pointer)? {
            Decision::Known => {
                let stored = self.lfs_content.get(&pointer.oid).copied().ok_or_else(|| {
                    GitCompatError::ImportError(format!(
                        "Git LFS object {} was resolved but not stored",
                        pointer.oid
                    ))
                })?;
                Ok(Some(stored))
            }
            Decision::Content(content) => {
                let (hash, chunked) = self.sink.put_file(&content)?;
                let stored = (hash, content.len() as u64, chunked);
                self.lfs_content.insert(pointer.oid, stored);
                Ok(Some(stored))
            }
            Decision::Pointer => {
                if let Some(stored) = self.pointer_blobs.get(&oid) {
                    return Ok(Some(*stored));
                }
                let (hash, chunked) = self.sink.put_file(data)?;
                let stored = (hash, data.len() as u64, chunked);
                self.pointer_blobs.insert(oid, stored);
                Ok(Some(stored))
            }
        }
    }

    fn warn_once(&mut self, path: &str, what: &str) {
        if self.warned.insert(path.to_owned()) {
            warn!(path, "{what}");
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Snapshot record for `commit`. Parents outside `known` (e.g. cut off by a
/// shallow clone) are dropped with a warning.
pub(crate) fn snapshot_from_commit(
    commit: &git2::Commit<'_>,
    tree_id: &str,
    branch: &str,
    manifest: ObjectHash,
    known: &HashSet<Oid>,
) -> Snapshot {
    let parents = commit
        .parent_ids()
        .filter(|parent| {
            let present = known.contains(parent);
            if !present {
                warn!(commit = %commit.id(), %parent, "parent commit not available; dropping it");
            }
            present
        })
        .map(ident::snapshot_id_for_commit)
        .collect();
    Snapshot {
        id: ident::snapshot_id_for_commit(commit.id()),
        tree_id: tree_id.to_owned(),
        branch: branch.to_owned(),
        parents,
        manifest,
        author: ident::author_of(&commit.author()),
        message: ident::message_from_git(commit.message_bytes()),
        created_at: ident::git_time_to_utc(commit.time()),
        auto_generated: false,
        dependencies: Vec::new(),
        reverts: None,
        signature: None,
    }
}

/// Remote-tracking branches (`refs/remotes/<remote>/<name>`, without the
/// symbolic `HEAD`) as (name without the remote prefix, tip), `origin`
/// first, then other remotes by name.
fn remote_branches(git: &git2::Repository) -> Result<Vec<(String, Oid)>> {
    let mut found: Vec<(bool, String, String, Oid)> = Vec::new();
    for item in git.branches(Some(BranchType::Remote))? {
        let (branch, _) = item?;
        if branch.get().symbolic_target().is_some() {
            continue;
        }
        let Some(full) = branch.name()?.map(str::to_owned) else {
            warn!("skipping remote branch with a non-UTF-8 name");
            continue;
        };
        let Some((remote, name)) = full.split_once('/') else {
            continue;
        };
        if name == "HEAD" {
            continue;
        }
        let Ok(commit) = branch.get().peel_to_commit() else {
            continue;
        };
        found.push((
            remote != "origin",
            remote.to_owned(),
            name.to_owned(),
            commit.id(),
        ));
    }
    found.sort();
    Ok(found
        .into_iter()
        .map(|(_, _, name, oid)| (name, oid))
        .collect())
}

/// Branches to import, sorted by name, with their tip commits: the
/// requested ones (local branches first, else a remote-tracking branch of
/// that name), else every local branch, plus — with `all` — every
/// remote-tracking branch without a local branch of the same name.
pub(crate) fn select_branches(
    git: &git2::Repository,
    requested: &[String],
    all: bool,
) -> Result<Vec<(String, Oid)>> {
    let mut selected: Vec<(String, Oid)> = Vec::new();
    if requested.is_empty() {
        for item in git.branches(Some(BranchType::Local))? {
            let (branch, _) = item?;
            let Some(name) = branch.name()?.map(str::to_owned) else {
                warn!("skipping branch with a non-UTF-8 name");
                continue;
            };
            selected.push((name, branch.get().peel_to_commit()?.id()));
        }
        if all {
            for (name, oid) in remote_branches(git)? {
                if !selected.iter().any(|(n, _)| *n == name) {
                    selected.push((name, oid));
                }
            }
        }
    } else {
        let remotes = remote_branches(git)?;
        for name in requested {
            match git.find_branch(name, BranchType::Local) {
                Ok(branch) => selected.push((name.clone(), branch.get().peel_to_commit()?.id())),
                Err(e) if e.code() == git2::ErrorCode::NotFound => {
                    let (_, oid) = remotes.iter().find(|(n, _)| n == name).ok_or_else(|| {
                        GitCompatError::ImportError(format!(
                            "branch not found in Git repository: {name}"
                        ))
                    })?;
                    selected.push((name.clone(), *oid));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    selected.sort();
    selected.dedup_by(|a, b| a.0 == b.0);
    Ok(selected)
}

/// Branch HEAD points at (even if unborn), if HEAD is symbolic.
pub(crate) fn head_branch(git: &git2::Repository) -> Option<String> {
    let head = git.find_reference("HEAD").ok()?;
    head.symbolic_target()?
        .strip_prefix("refs/heads/")
        .map(str::to_owned)
}

/// Every commit reachable from `branches`, parents before children.
pub(crate) fn commits_oldest_first(
    git: &git2::Repository,
    branches: &[(String, Oid)],
) -> Result<Vec<Oid>> {
    let mut walk = git.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
    for (_, tip) in branches {
        walk.push(*tip)?;
    }
    Ok(walk.collect::<std::result::Result<_, _>>()?)
}

/// Branch each commit is attributed to.
///
/// A commit belongs to the first branch in `priority` order whose
/// *first-parent* chain contains it (the branch it was made on, as with
/// `git log --first-parent`). Commits reachable only through merge parents
/// fall back to the first branch in `priority` order whose full history
/// contains them.
pub(crate) fn assign_branches(
    git: &git2::Repository,
    priority: &[(String, Oid)],
) -> Result<HashMap<Oid, String>> {
    let mut assigned = HashMap::new();
    for (name, tip) in priority {
        let mut walk = git.revwalk()?;
        walk.push(*tip)?;
        walk.simplify_first_parent()?;
        for oid in walk {
            assigned.entry(oid?).or_insert_with(|| name.clone());
        }
    }
    let mut done: Vec<Oid> = Vec::new();
    for (name, tip) in priority {
        let mut walk = git.revwalk()?;
        walk.push(*tip)?;
        for previous in &done {
            walk.hide(*previous)?;
        }
        for oid in walk {
            assigned.entry(oid?).or_insert_with(|| name.clone());
        }
        done.push(*tip);
    }
    Ok(assigned)
}

/// Result of converting the selected history into snapshots of a tree.
struct Converted {
    snapshots: usize,
    blobs: usize,
    known: HashSet<Oid>,
    lfs: LfsReport,
}

/// Write a snapshot for every commit reachable from `branches` (commits
/// shared with `preferred` are attributed to it first) and record them in
/// `.wt/git-map.json`. Branch refs are not touched.
///
/// Commits whose snapshot already exists (an earlier import into this
/// worktree) are not converted again: snapshot records are immutable, so
/// re-importing a commit keeps the content it was first imported with
/// (e.g. LFS pointer files whose content was unavailable then).
fn convert_history(
    git: &git2::Repository,
    repo: &Repository,
    tree: &TreeInfo,
    branches: &[(String, Oid)],
    preferred: Option<&String>,
    opts: &ImportOptions,
) -> Result<Converted> {
    let mut priority = branches.to_vec();
    priority.sort_by_key(|(name, _)| Some(name) != preferred);
    let assigned = assign_branches(git, &priority)?;
    let order = commits_oldest_first(git, branches)?;
    let known: HashSet<Oid> = order.iter().copied().collect();

    let mut map = GitMap::load(&repo.wt_dir())?;
    let lfs = LfsImport::new(opts.lfs, LfsResolver::for_repository(git, opts.lfs_fetch));
    let mut converter = TreeConverter::new(git, repo.store())
        .with_lfs(lfs)
        .recording_gitlinks();
    let mut manifests: HashMap<Oid, ObjectHash> = HashMap::new();
    for oid in &order {
        let id = ident::snapshot_id_for_commit(*oid);
        if repo.has_snapshot(&id) {
            map.insert(&id, &oid.to_string());
            continue;
        }
        let commit = git.find_commit(*oid)?;
        let manifest = match manifests.get(&commit.tree_id()) {
            Some(hash) => *hash,
            None => {
                let manifest = converter.manifest(&commit.tree()?)?;
                let hash = repo.write_manifest(&manifest)?;
                manifests.insert(commit.tree_id(), hash);
                hash
            }
        };
        let branch = assigned.get(oid).ok_or_else(|| {
            GitCompatError::ImportError(format!("commit {oid} is not on any selected branch"))
        })?;
        let snapshot = snapshot_from_commit(&commit, &tree.id, branch, manifest, &known);
        repo.write_snapshot(&snapshot)?;
        map.insert(&snapshot.id, &oid.to_string());
    }
    map.save(&repo.wt_dir())?;
    Ok(Converted {
        snapshots: order.len(),
        blobs: converter.blob_count(),
        known,
        lfs: converter.lfs_report(),
    })
}

fn tip_of(branches: &[(String, Oid)], branch: &str) -> Result<Oid> {
    branches
        .iter()
        .find(|(name, _)| name == branch)
        .map(|(_, tip)| *tip)
        .ok_or_else(|| GitCompatError::ImportError(format!("branch {branch} vanished")))
}

pub(crate) fn import_repository(
    git_path: &Path,
    dest: &Path,
    opts: &ImportOptions,
) -> Result<ImportReport> {
    let git = git2::Repository::open(git_path)?;
    let branches = select_branches(&git, &opts.branches, opts.all_branches)?;
    let checked_out = head_branch(&git)
        .filter(|head| branches.iter().any(|(name, _)| name == head))
        .or_else(|| branches.first().map(|(name, _)| name.clone()));

    // Validation is done; from here on `dest` is being created.
    let dest_existed = dest.exists();
    let repo = Repository::init(
        dest,
        InitOptions {
            name: opts.name.clone(),
            default_branch: checked_out.clone(),
        },
    )?;
    let tree = repo.root_tree()?;
    let converted = match convert_history(&git, &repo, &tree, &branches, checked_out.as_ref(), opts)
    {
        Ok(converted) => converted,
        Err(e) => {
            // Nothing is checked out yet: leave no half-imported
            // worktree behind (e.g. `--lfs require` with missing content).
            let _ = std::fs::remove_dir_all(repo.wt_dir());
            if !dest_existed {
                let _ = std::fs::remove_dir(dest);
            }
            return Err(e);
        }
    };

    for (name, tip) in &branches {
        repo.set_branch_tip(&tree.name, name, Some(&ident::snapshot_id_for_commit(*tip)))?;
    }
    let tags = if opts.include_tags {
        import_tags(&git, &repo, &tree.name, &converted.known)?
    } else {
        0
    };

    let mut submodules = Vec::new();
    if let Some(branch) = &checked_out {
        let tip = tip_of(&branches, branch)?;
        repo.checkout(&tree.name, &ident::snapshot_id_for_commit(tip), true)?;
        let tip_tree = git.find_commit(tip)?.tree()?;
        import_ignore_rules(&git, &repo, &tip_tree)?;
        let pins = submodule::pins_of_tree(&git, &tip_tree)?;
        submodules = match opts.submodules {
            SubmoduleMode::Record => submodule::recorded(&pins),
            SubmoduleMode::Nested => submodule::import_nested(&git, &repo, &tree, &pins, opts),
        };
    }

    info!(
        snapshots = converted.snapshots,
        blobs = converted.blobs,
        tags,
        "imported Git repository"
    );
    Ok(ImportReport {
        dest: dest.to_path_buf(),
        tree: tree.name,
        branches: branches.into_iter().map(|(name, _)| name).collect(),
        snapshots: converted.snapshots,
        blobs: converted.blobs,
        tags,
        checked_out,
        lfs: converted.lfs,
        submodules,
    })
}

/// Import into the existing tree `tree_name` of `repo`.
///
/// Snapshot ids are derived from commit SHAs, so importing the same
/// repository again only adds the new commits. Existing branches are moved
/// only when the import fast-forwards them; a diverged branch fails the
/// import before any ref moves. The tree's files follow its current branch
/// when that branch is updated (the working directory must be clean).
pub(crate) fn import_into_tree(
    repo: &Repository,
    tree_name: &str,
    git_path: &Path,
    opts: &ImportOptions,
) -> Result<ImportReport> {
    let git = git2::Repository::open(git_path)?;
    let tree = repo.tree(tree_name)?;
    let branches = select_branches(&git, &opts.branches, opts.all_branches)?;
    let current = repo.current_branch(&tree.name)?;
    let current_tip = repo.branch_tip(&tree.name, &current).ok().flatten();
    let current_updated = branches
        .iter()
        .find(|(name, _)| *name == current)
        .is_some_and(|(_, tip)| {
            current_tip.as_deref() != Some(&ident::snapshot_id_for_commit(*tip))
        });
    if current_updated && current_tip.is_some() && !repo.status(&tree.name)?.is_clean() {
        return Err(GitCompatError::ImportError(format!(
            "tree {} has uncommitted changes on branch {current}; snapshot or discard them first",
            tree.name
        )));
    }

    let preferred = branches
        .iter()
        .any(|(name, _)| *name == current)
        .then(|| current.clone())
        .or_else(|| head_branch(&git));
    let converted = convert_history(&git, repo, &tree, &branches, preferred.as_ref(), opts)?;

    // Refuse to rewrite diverged branches before moving anything.
    let existing: HashMap<String, Option<String>> = repo
        .branches(&tree.name, true)?
        .into_iter()
        .map(|b| (b.name, b.tip))
        .collect();
    let mut diverged = Vec::new();
    for (name, tip) in &branches {
        let new = ident::snapshot_id_for_commit(*tip);
        if let Some(Some(old)) = existing.get(name) {
            if *old != new && !repo.is_ancestor(old, &new)? {
                diverged.push(name.clone());
            }
        }
    }
    if !diverged.is_empty() {
        return Err(GitCompatError::ImportError(format!(
            "branches of tree {} have diverged from the Git repository: {}",
            tree.name,
            diverged.join(", ")
        )));
    }

    // Files first (relative to the old tip), then the refs.
    let mut checked_out = None;
    if current_updated {
        let tip = ident::snapshot_id_for_commit(tip_of(&branches, &current)?);
        repo.checkout(&tree.name, &tip, current_tip.is_none())?;
        checked_out = Some(current.clone());
    }
    for (name, tip) in &branches {
        repo.set_branch_tip(&tree.name, name, Some(&ident::snapshot_id_for_commit(*tip)))?;
    }
    if current_tip.is_none() && checked_out.is_none() {
        // An empty tree adopts the Git repository's checked-out branch.
        if let Some(head) = preferred.filter(|h| branches.iter().any(|(n, _)| n == h)) {
            let switch = worktree_sdk::SwitchOptions {
                create: false,
                force: false,
            };
            match repo.switch_branch(&tree.name, &head, switch) {
                Ok(()) => checked_out = Some(head),
                Err(e) => {
                    warn!(branch = %head, error = %e, "could not switch the tree to the imported branch")
                }
            }
        }
    }
    let tags = if opts.include_tags {
        import_tags(&git, repo, &tree.name, &converted.known)?
    } else {
        0
    };
    let mut submodules = Vec::new();
    if let Some(branch) = &checked_out {
        let tip_tree = git.find_commit(tip_of(&branches, branch)?)?.tree()?;
        if tree.path.as_os_str().is_empty() {
            import_ignore_rules(&git, repo, &tip_tree)?;
        }
        // Nested-tree mapping is for new worktrees only; here pins are
        // recorded (and reported).
        submodules = submodule::recorded(&submodule::pins_of_tree(&git, &tip_tree)?);
    }

    info!(
        tree = %tree.name,
        snapshots = converted.snapshots,
        blobs = converted.blobs,
        tags,
        "imported Git repository into existing tree"
    );
    Ok(ImportReport {
        dest: repo.tree_dir(&tree.name)?,
        tree: tree.name,
        branches: branches.into_iter().map(|(name, _)| name).collect(),
        snapshots: converted.snapshots,
        blobs: converted.blobs,
        tags,
        checked_out,
        lfs: converted.lfs,
        submodules,
    })
}

/// Convert tags pointing at imported commits; returns how many were written.
fn import_tags(
    git: &git2::Repository,
    repo: &Repository,
    tree: &str,
    known: &HashSet<Oid>,
) -> Result<usize> {
    let mut count = 0;
    for name in git.tag_names(None)?.iter().flatten() {
        let reference = git.find_reference(&format!("refs/tags/{name}"))?;
        let Ok(commit) = reference.peel_to_commit() else {
            warn!(tag = name, "skipping tag that does not point at a commit");
            continue;
        };
        if !known.contains(&commit.id()) {
            warn!(tag = name, commit = %commit.id(), "skipping tag outside the imported branches");
            continue;
        }
        let annotated = reference.peel_to_tag().ok();
        let tagger = annotated.as_ref().and_then(|t| t.tagger());
        let info = TagInfo {
            name: name.to_owned(),
            target: ident::snapshot_id_for_commit(commit.id()),
            message: annotated
                .as_ref()
                .map(|t| ident::message_from_git(t.message_bytes().unwrap_or_default())),
            tagger: ident::author_of(tagger.as_ref().unwrap_or(&commit.author())),
            created_at: ident::git_time_to_utc(
                tagger.as_ref().map_or_else(|| commit.time(), |t| t.when()),
            ),
            signature: None,
        };
        repo.put_tag(tree, &info)?;
        count += 1;
    }
    Ok(count)
}

/// Copy the root `.gitignore` patterns into `.wt/ignore`.
fn import_ignore_rules(
    git: &git2::Repository,
    repo: &Repository,
    tree: &git2::Tree<'_>,
) -> Result<()> {
    if let Some(content) = root_text_file(git, tree, ".gitignore")? {
        for pattern in gitignore::gitignore_patterns(&content) {
            repo.add_ignore_pattern(&pattern)?;
        }
    }
    Ok(())
}

fn root_text_file(
    git: &git2::Repository,
    tree: &git2::Tree<'_>,
    name: &str,
) -> Result<Option<String>> {
    let Some(entry) = tree.get_name(name) else {
        return Ok(None);
    };
    if entry.kind() != Some(ObjectType::Blob) {
        return Ok(None);
    }
    let blob = git.find_blob(entry.id())?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{GitFixture, MemStore, Spec};

    #[test]
    fn manifest_converts_files_and_skips_links_and_submodules() {
        let fx = GitFixture::new();
        let other = fx.commit(None, &[Spec::File("x", b"x")], &[], "other", 1);
        let tree_oid = fx.tree(&[
            Spec::File("README.md", b"hello\n"),
            Spec::Exec("bin/run.sh", b"#!/bin/sh\necho hi\n"),
            Spec::File("src/lib/deep/mod.rs", b"pub fn f() {}\n"),
            Spec::File("src/a.rs", b"a"),
            Spec::Link("link", "README.md"),
            Spec::Gitlink("vendor/sub", other),
        ]);
        let store = MemStore::default();
        let mut conv = TreeConverter::new(&fx.repo, &store);
        let manifest = conv
            .manifest(&fx.repo.find_tree(tree_oid).unwrap())
            .unwrap();

        let paths: Vec<_> = manifest.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["README.md", "bin/run.sh", "src/a.rs", "src/lib/deep/mod.rs"]
        );
        let run = &manifest.entries[1];
        assert!(run.executable);
        assert_eq!(run.size, 18);
        assert!(!manifest.entries[0].executable);
        assert_eq!(
            store.get(&manifest.entries[0].hash).as_deref(),
            Some(&b"hello\n"[..])
        );
        assert_eq!(conv.blob_count(), 4);
        assert_eq!(store.len(), 4);
    }

    #[test]
    fn manifest_entries_use_bytewise_path_order() {
        // Git sorts "a" (a tree) as "a/", after "a.txt"; manifests sort by path.
        let fx = GitFixture::new();
        let tree_oid = fx.tree(&[
            Spec::File("a.txt", b"1"),
            Spec::File("a/b.txt", b"2"),
            Spec::File("a-b", b"3"),
        ]);
        let store = MemStore::default();
        let manifest = TreeConverter::new(&fx.repo, &store)
            .manifest(&fx.repo.find_tree(tree_oid).unwrap())
            .unwrap();
        let paths: Vec<_> = manifest.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["a-b", "a.txt", "a/b.txt"]);
    }

    #[test]
    fn identical_blobs_are_stored_once() {
        let fx = GitFixture::new();
        let t1 = fx.tree(&[Spec::File("a", b"same"), Spec::File("b", b"same")]);
        let t2 = fx.tree(&[Spec::File("c", b"same"), Spec::File("d", b"new")]);
        let store = MemStore::default();
        let mut conv = TreeConverter::new(&fx.repo, &store);
        conv.manifest(&fx.repo.find_tree(t1).unwrap()).unwrap();
        let m2 = conv.manifest(&fx.repo.find_tree(t2).unwrap()).unwrap();
        assert_eq!(conv.blob_count(), 2);
        assert_eq!(m2.entries.len(), 2);
    }

    #[test]
    fn snapshot_maps_commit_metadata() {
        let fx = GitFixture::new();
        let root = fx.commit(None, &[Spec::File("a", b"1")], &[], "root\n", 1_000);
        let side = fx.commit(None, &[Spec::File("a", b"2")], &[root], "side", 2_000);
        let merge = fx.commit(
            None,
            &[Spec::File("a", b"3")],
            &[root, side],
            "Merge side\n\nDetails here.\n",
            3_000,
        );
        let commit = fx.repo.find_commit(merge).unwrap();
        let known: HashSet<Oid> = [root, side, merge].into_iter().collect();
        let manifest = worktree_sdk::store::hash_bytes(b"manifest");
        let snap = snapshot_from_commit(&commit, "tree-1", "main", manifest, &known);

        assert_eq!(snap.id, ident::snapshot_id_for_commit(merge));
        assert_eq!(snap.tree_id, "tree-1");
        assert_eq!(snap.branch, "main");
        assert_eq!(
            snap.parents,
            vec![
                ident::snapshot_id_for_commit(root),
                ident::snapshot_id_for_commit(side)
            ]
        );
        assert_eq!(snap.manifest, manifest);
        assert_eq!(snap.author, "Ada Lovelace <ada@example.com>");
        assert_eq!(snap.message, "Merge side\n\nDetails here.");
        assert_eq!(snap.created_at.timestamp(), 3_000);
        assert!(!snap.auto_generated);
        assert!(snap.is_merge());
    }

    #[test]
    fn unknown_parents_are_dropped() {
        let fx = GitFixture::new();
        let root = fx.commit(None, &[Spec::File("a", b"1")], &[], "root", 1);
        let child = fx.commit(None, &[Spec::File("a", b"2")], &[root], "child", 2);
        let commit = fx.repo.find_commit(child).unwrap();
        let known: HashSet<Oid> = [child].into_iter().collect();
        let snap = snapshot_from_commit(&commit, "t", "main", ObjectHash::ZERO, &known);
        assert!(snap.parents.is_empty());
    }

    #[test]
    fn walk_orders_parents_first_and_converts_shared_commits_once() {
        let fx = GitFixture::new();
        let c1 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"1")],
            &[],
            "c1",
            10,
        );
        let c2 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"2")],
            &[c1],
            "c2",
            20,
        );
        let f1 = fx.commit(
            Some("refs/heads/feature"),
            &[Spec::File("b", b"1")],
            &[c1],
            "f1",
            15,
        );
        let m = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("b", b"1")],
            &[c2, f1],
            "m",
            30,
        );

        let branches = select_branches(&fx.repo, &[], false).unwrap();
        assert_eq!(
            branches,
            vec![("feature".to_owned(), f1), ("main".to_owned(), m)]
        );
        let order = commits_oldest_first(&fx.repo, &branches).unwrap();
        assert_eq!(order.len(), 4);
        let pos = |oid: Oid| order.iter().position(|o| *o == oid).unwrap();
        assert!(pos(c1) < pos(c2) && pos(c1) < pos(f1));
        assert!(pos(c2) < pos(m) && pos(f1) < pos(m));

        // main has priority: its first-parent chain (m, c2, c1) belongs to it;
        // f1 was made on feature and keeps that attribution after the merge.
        let priority = vec![("main".to_owned(), m), ("feature".to_owned(), f1)];
        let assigned = assign_branches(&fx.repo, &priority).unwrap();
        assert_eq!(assigned.len(), 4);
        assert_eq!(assigned[&m], "main");
        assert_eq!(assigned[&c2], "main");
        assert_eq!(assigned[&c1], "main");
        assert_eq!(assigned[&f1], "feature");

        // feature first: c1 and f1 belong to feature, the rest to main.
        let priority = vec![("feature".to_owned(), f1), ("main".to_owned(), m)];
        let assigned = assign_branches(&fx.repo, &priority).unwrap();
        assert_eq!(assigned[&c1], "feature");
        assert_eq!(assigned[&f1], "feature");
        assert_eq!(assigned[&c2], "main");
        assert_eq!(assigned[&m], "main");
    }

    #[test]
    fn select_requested_branches() {
        let fx = GitFixture::new();
        let c1 = fx.commit(
            Some("refs/heads/main"),
            &[Spec::File("a", b"1")],
            &[],
            "c1",
            10,
        );
        fx.commit(
            Some("refs/heads/dev"),
            &[Spec::File("a", b"2")],
            &[c1],
            "d",
            20,
        );
        let only =
            select_branches(&fx.repo, &["main".to_owned(), "main".to_owned()], false).unwrap();
        assert_eq!(only, vec![("main".to_owned(), c1)]);
        let err = select_branches(&fx.repo, &["nope".to_owned()], false).unwrap_err();
        assert!(matches!(err, GitCompatError::ImportError(msg) if msg.contains("nope")));
        assert_eq!(head_branch(&fx.repo).as_deref(), Some("main"));
    }

    #[test]
    fn root_text_file_reads_only_root_blobs() {
        let fx = GitFixture::new();
        let tree_oid = fx.tree(&[
            Spec::File(".gitignore", b"target/\n*.log\n"),
            Spec::File("sub/.gitattributes", b"*.bin filter=lfs\n"),
        ]);
        let tree = fx.repo.find_tree(tree_oid).unwrap();
        assert_eq!(
            root_text_file(&fx.repo, &tree, ".gitignore")
                .unwrap()
                .as_deref(),
            Some("target/\n*.log\n")
        );
        assert_eq!(
            root_text_file(&fx.repo, &tree, ".gitattributes").unwrap(),
            None
        );
        assert_eq!(root_text_file(&fx.repo, &tree, "sub").unwrap(), None);
    }
}
