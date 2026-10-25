//! W0rkTree -> Git conversion (`wt git export`).
//!
//! Commits are built from snapshot data only (manifest, parents, author,
//! timestamp in UTC, message), so exporting the same snapshots always
//! produces the same commit SHAs. Snapshots already recorded in
//! `.wt/git-map.json` whose commit exists in the target are reused.

use std::cell::RefCell;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{FileMode, ObjectType, Oid, RepositoryInitOptions, StatusOptions};
use tracing::{info, warn};
use worktree_sdk::{
    Manifest, ManifestEntry, ObjectHash, ObjectStore, Repository, Snapshot, SnapshotId, TreeInfo,
};

use crate::bridge::{ExportOptions, ExportReport};
use crate::error::{GitCompatError, Result};
use crate::hash_index::store::InMemoryHashIndex;
use crate::ident;
use crate::license::LicensePolicy;
use crate::map::GitMap;

/// Source of file contents referenced by manifests.
pub(crate) trait BlobSource {
    fn read_blob(&self, entry: &ManifestEntry) -> Result<Vec<u8>>;
}

impl BlobSource for ObjectStore {
    fn read_blob(&self, entry: &ManifestEntry) -> Result<Vec<u8>> {
        // read_entry reassembles chunked files (§13.2).
        Ok(self.read_entry(entry)?)
    }
}

/// The object store plus generated files (the license notice) that exist
/// only in the export.
pub(crate) struct ExportSource<'a> {
    store: &'a ObjectStore,
    generated: RefCell<HashMap<ObjectHash, Vec<u8>>>,
}

impl<'a> ExportSource<'a> {
    pub fn new(store: &'a ObjectStore) -> Self {
        Self {
            store,
            generated: RefCell::new(HashMap::new()),
        }
    }

    /// Make `data` available under its hash (for entries not in the store).
    pub fn add_generated(&self, hash: ObjectHash, data: Vec<u8>) {
        self.generated.borrow_mut().insert(hash, data);
    }
}

impl BlobSource for ExportSource<'_> {
    fn read_blob(&self, entry: &ManifestEntry) -> Result<Vec<u8>> {
        if let Some(data) = self.generated.borrow().get(&entry.hash) {
            return Ok(data.clone());
        }
        self.store.read_blob(entry)
    }
}

/// Writes manifests as (nested) Git trees, writing each distinct blob once.
pub(crate) struct TreeWriter<'a, S: BlobSource + ?Sized> {
    git: &'a git2::Repository,
    source: &'a S,
    blobs: InMemoryHashIndex,
}

enum Node<'m> {
    File(&'m ManifestEntry),
    Dir(Dir<'m>),
}

#[derive(Default)]
struct Dir<'m> {
    children: BTreeMap<&'m str, Node<'m>>,
}

impl<'a, S: BlobSource + ?Sized> TreeWriter<'a, S> {
    pub fn new(git: &'a git2::Repository, source: &'a S) -> Self {
        Self {
            git,
            source,
            blobs: InMemoryHashIndex::new(),
        }
    }

    /// Write `manifest` as a Git tree and return its id.
    pub fn write(&mut self, manifest: &Manifest) -> Result<Oid> {
        let root = layout(manifest)?;
        self.write_dir(&root)
    }

    fn write_dir(&mut self, dir: &Dir<'_>) -> Result<Oid> {
        let git = self.git;
        let mut builder = git.treebuilder(None)?;
        for (name, node) in &dir.children {
            let (oid, mode) = match node {
                Node::File(entry) => {
                    let mode = if entry.executable {
                        FileMode::BlobExecutable
                    } else {
                        FileMode::Blob
                    };
                    (self.blob(entry)?, mode)
                }
                Node::Dir(sub) => (self.write_dir(sub)?, FileMode::Tree),
            };
            builder.insert(name, oid, i32::from(mode))?;
        }
        Ok(builder.write()?)
    }

    fn blob(&mut self, entry: &ManifestEntry) -> Result<Oid> {
        if let Some(oid) = self.blobs.oid_for(&entry.hash) {
            return Ok(oid);
        }
        let data = self.source.read_blob(entry)?;
        let oid = self.git.blob(&data)?;
        self.blobs.insert_oid(entry.hash, oid)?;
        Ok(oid)
    }
}

/// Arrange manifest entries into a directory hierarchy.
fn layout(manifest: &Manifest) -> Result<Dir<'_>> {
    let mut root = Dir::default();
    for entry in &manifest.entries {
        let components = split_path(&entry.path)?;
        let Some((file, parents)) = components.split_last() else {
            return Err(invalid_path(&entry.path));
        };
        let mut dir = &mut root;
        for component in parents {
            let node = dir
                .children
                .entry(component)
                .or_insert_with(|| Node::Dir(Dir::default()));
            dir = match node {
                Node::Dir(sub) => sub,
                Node::File(_) => return Err(path_conflict(&entry.path)),
            };
        }
        match dir.children.entry(file) {
            Entry::Vacant(slot) => {
                slot.insert(Node::File(entry));
            }
            Entry::Occupied(_) => return Err(path_conflict(&entry.path)),
        }
    }
    Ok(root)
}

fn split_path(path: &str) -> Result<Vec<&str>> {
    let components: Vec<&str> = path.split('/').collect();
    let valid = components.iter().all(|c| {
        !c.is_empty()
            && *c != "."
            && *c != ".."
            && !c.eq_ignore_ascii_case(".git")
            && !c.contains('\0')
    });
    if valid {
        Ok(components)
    } else {
        Err(invalid_path(path))
    }
}

fn invalid_path(path: &str) -> GitCompatError {
    GitCompatError::ExportError(format!("invalid path in manifest: {path:?}"))
}

fn path_conflict(path: &str) -> GitCompatError {
    GitCompatError::ExportError(format!(
        "manifest path {path:?} is used both as a file and as a directory"
    ))
}

/// Write the commit for `snapshot` (tree and parents already in `git`).
/// Returns the commit id and whether the object was newly written.
pub(crate) fn write_commit(
    git: &git2::Repository,
    snapshot: &Snapshot,
    tree: Oid,
    parents: &[Oid],
) -> Result<(Oid, bool)> {
    let sig = ident::signature(&snapshot.author, snapshot.created_at)?;
    let tree = git.find_tree(tree)?;
    let parents = parents
        .iter()
        .map(|p| git.find_commit(*p))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
    let buffer = git.commit_create_buffer(
        &sig,
        &sig,
        &ident::message_to_git(&snapshot.message),
        &tree,
        &parent_refs,
    )?;
    let odb = git.odb()?;
    let oid = Oid::hash_object(ObjectType::Commit, &buffer)?;
    if odb.exists(oid) {
        return Ok((oid, false));
    }
    Ok((odb.write(ObjectType::Commit, &buffer)?, true))
}

/// Open the Git repository at `path`, or create it (HEAD -> `head_branch`).
/// Returns whether it was created.
pub(crate) fn open_or_init(
    path: &Path,
    bare: bool,
    head_branch: &str,
) -> Result<(git2::Repository, bool)> {
    match git2::Repository::open(path) {
        Ok(git) => Ok((git, false)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            let mut opts = RepositoryInitOptions::new();
            opts.bare(bare).mkpath(true).initial_head(head_branch);
            Ok((git2::Repository::init_opts(path, &opts)?, true))
        }
        Err(e) => Err(e.into()),
    }
}

/// Whether the index and tracked files match HEAD (untracked files ignored).
pub(crate) fn work_tree_is_clean(git: &git2::Repository) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(false)
        .include_ignored(false)
        .exclude_submodules(true);
    Ok(git.statuses(Some(&mut opts))?.is_empty())
}

pub(crate) fn head_commit(git: &git2::Repository) -> Option<Oid> {
    git.head().ok().and_then(|head| head.target())
}

/// After refs moved, bring a non-bare repository's files up to date with
/// HEAD, but only if it had no local modifications beforehand.
pub(crate) fn refresh_work_tree(
    git: &git2::Repository,
    old_head: Option<Oid>,
    was_clean: bool,
) -> Result<()> {
    if git.is_bare() {
        return Ok(());
    }
    let new_head = head_commit(git);
    if new_head.is_none() || new_head == old_head {
        return Ok(());
    }
    if was_clean {
        git.checkout_head(Some(CheckoutBuilder::new().force()))?;
    } else {
        warn!(
            path = %git.workdir().unwrap_or_else(|| git.path()).display(),
            "Git working tree has local changes; HEAD moved but files were left untouched"
        );
    }
    Ok(())
}

struct Exporter<'a> {
    repo: &'a Repository,
    git: &'a git2::Repository,
    map: GitMap,
    resolved: HashMap<SnapshotId, Oid>,
    trees: HashMap<ObjectHash, Oid>,
    source: &'a ExportSource<'a>,
    writer: TreeWriter<'a, ExportSource<'a>>,
    commits: usize,
    /// License-aware export: commits are built from filtered manifests
    /// (plus the notice), so `.wt/git-map.json` — which maps snapshots to
    /// unfiltered commits — is neither consulted nor updated.
    license: Option<&'a LicensePolicy>,
    restricted: BTreeSet<String>,
}

impl<'a> Exporter<'a> {
    /// Commit already representing `id` in the target repository.
    fn existing(&mut self, id: &str) -> Option<Oid> {
        if let Some(oid) = self.resolved.get(id) {
            return Some(*oid);
        }
        if self.license.is_some() {
            return None;
        }
        let oid = Oid::from_str(self.map.commit_for(id)?).ok()?;
        self.git.find_commit(oid).ok()?;
        self.resolved.insert(id.to_owned(), oid);
        Some(oid)
    }

    /// Export `tip` and all its ancestors; returns the tip's commit.
    fn export_history(&mut self, tip: &str) -> Result<Oid> {
        if let Some(oid) = self.existing(tip) {
            return Ok(oid);
        }
        for snapshot in self.repo.ancestry(tip)? {
            if self.existing(&snapshot.id).is_none() {
                self.export_snapshot(&snapshot)?;
            }
        }
        self.resolved.get(tip).copied().ok_or_else(|| {
            GitCompatError::ExportError(format!("snapshot {tip} is not in its own ancestry"))
        })
    }

    fn export_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        let mut parents = Vec::with_capacity(snapshot.parents.len());
        for parent in &snapshot.parents {
            match self.existing(parent) {
                Some(oid) => parents.push(oid),
                None => warn!(
                    snapshot = %snapshot.id,
                    parent = %parent,
                    "parent snapshot not available locally; dropping it from the commit"
                ),
            }
        }
        let tree = match self.trees.get(&snapshot.manifest) {
            Some(tree) => *tree,
            None => {
                let mut manifest = self.repo.read_manifest(&snapshot.manifest)?;
                if let Some(policy) = self.license {
                    // No snapshot-specific header: the tree cache is keyed by
                    // manifest hash and exports must stay deterministic.
                    let licensed = policy.apply(&manifest, &[]);
                    if let Some(notice) = licensed.notice {
                        self.source
                            .add_generated(worktree_sdk::store::hash_bytes(&notice), notice);
                    }
                    self.restricted.extend(licensed.restricted);
                    manifest = licensed.manifest;
                }
                let tree = self.writer.write(&manifest)?;
                self.trees.insert(snapshot.manifest, tree);
                tree
            }
        };
        let (oid, new) = write_commit(self.git, snapshot, tree, &parents)?;
        if new {
            self.commits += 1;
        }
        self.resolved.insert(snapshot.id.clone(), oid);
        if self.license.is_none() {
            self.map.insert_if_absent(&snapshot.id, &oid.to_string());
        }
        Ok(())
    }

    /// Export the tree's tags; returns how many refs were created or moved.
    fn export_tags(&mut self, tree: &str) -> Result<usize> {
        let mut written = 0;
        for tag in self.repo.tags(tree)? {
            let refname = format!("refs/tags/{}", tag.name);
            if !git2::Reference::is_valid_name(&refname) {
                warn!(tag = %tag.name, "skipping tag whose name is not a valid Git ref");
                continue;
            }
            if !self.repo.has_snapshot(&tag.target) {
                warn!(tag = %tag.name, target = %tag.target, "skipping tag with a missing target");
                continue;
            }
            let target = self.export_history(&tag.target)?;
            let before = self.git.refname_to_id(&refname).ok();
            let after = match &tag.message {
                Some(message) => {
                    let object = self.git.find_object(target, Some(ObjectType::Commit))?;
                    let tagger = ident::signature(&tag.tagger, tag.created_at)?;
                    self.git.tag(
                        &tag.name,
                        &object,
                        &tagger,
                        &ident::message_to_git(message),
                        true,
                    )?
                }
                None => {
                    self.git
                        .reference(&refname, target, true, "wt git export: tag")?;
                    target
                }
            };
            if before != Some(after) {
                written += 1;
            }
        }
        Ok(written)
    }
}

/// Branches to export with their tips; empty branches are skipped.
fn select_branches(
    repo: &Repository,
    tree: &TreeInfo,
    requested: &[String],
) -> Result<Vec<(String, SnapshotId)>> {
    let all = repo.branches(&tree.name, !requested.is_empty())?;
    let mut selected = Vec::new();
    if requested.is_empty() {
        selected.extend(all.into_iter().filter_map(|b| Some((b.name, b.tip?))));
    } else {
        for name in requested {
            let branch = all.iter().find(|b| &b.name == name).ok_or_else(|| {
                GitCompatError::ExportError(format!(
                    "branch not found in tree {}: {name}",
                    tree.name
                ))
            })?;
            match &branch.tip {
                Some(tip) => selected.push((name.clone(), tip.clone())),
                None => warn!(branch = %name, "skipping branch without snapshots"),
            }
        }
    }
    for (name, _) in &selected {
        if !git2::Reference::is_valid_name(&format!("refs/heads/{name}")) {
            return Err(GitCompatError::ExportError(format!(
                "branch name is not a valid Git ref: {name}"
            )));
        }
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

pub(crate) fn export_repository(
    repo: &Repository,
    git_path: &Path,
    opts: &ExportOptions,
) -> Result<ExportReport> {
    let tree = match &opts.tree {
        Some(name) => repo.tree(name)?,
        None => repo.root_tree()?,
    };
    let branches = select_branches(repo, &tree, &opts.branches)?;
    let (git, created) = open_or_init(git_path, opts.bare, &tree.current_branch)?;
    let was_clean = created || git.is_bare() || work_tree_is_clean(&git)?;
    let old_head = head_commit(&git);

    let source = ExportSource::new(repo.store());
    let mut exporter = Exporter {
        repo,
        git: &git,
        map: GitMap::load(&repo.wt_dir())?,
        resolved: HashMap::new(),
        trees: HashMap::new(),
        source: &source,
        writer: TreeWriter::new(&git, &source),
        commits: 0,
        license: opts.license.as_ref(),
        restricted: BTreeSet::new(),
    };
    let mut exported = Vec::with_capacity(branches.len());
    for (name, tip) in &branches {
        let oid = exporter.export_history(tip)?;
        git.reference(&format!("refs/heads/{name}"), oid, true, "wt git export")?;
        exported.push(name.clone());
    }
    let tags = if opts.include_tags {
        exporter.export_tags(&tree.name)?
    } else {
        0
    };
    if exporter.license.is_none() {
        exporter.map.save(&repo.wt_dir())?;
    }
    let commits = exporter.commits;
    let restricted: Vec<String> = std::mem::take(&mut exporter.restricted)
        .into_iter()
        .collect();
    drop(exporter);
    refresh_work_tree(&git, old_head, was_clean)?;

    info!(commits, tags, branches = exported.len(), "exported to Git");
    Ok(ExportReport {
        git_path: git_path.to_path_buf(),
        branches: exported,
        commits,
        tags,
        restricted,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;

    use chrono::DateTime;
    use worktree_sdk::store::hash_bytes;

    use super::*;
    use crate::import::{BlobSink, TreeConverter};
    use crate::test_support::{GitFixture, MemStore, Spec};

    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    fn entry(store: &MemStore, path: &str, data: &[u8], executable: bool) -> ManifestEntry {
        ManifestEntry {
            path: path.to_owned(),
            hash: store.put_blob(data).unwrap(),
            size: data.len() as u64,
            executable,
            chunked: false,
        }
    }

    fn snapshot(id: &str, parents: &[&str], message: &str, at: i64) -> Snapshot {
        Snapshot {
            id: id.to_owned(),
            tree_id: "tree".to_owned(),
            branch: "main".to_owned(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            manifest: ObjectHash::ZERO,
            author: "Grace Hopper <grace@example.com>".to_owned(),
            message: message.to_owned(),
            created_at: DateTime::from_timestamp(at, 0).unwrap(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        }
    }

    #[test]
    fn manifest_tree_matches_git_built_tree() {
        let fx = GitFixture::new();
        let expected = fx.tree(&[
            Spec::File("README.md", b"hello\n"),
            Spec::Exec("bin/run", b"#!/bin/sh\n"),
            Spec::File("src/a/b/c.rs", b"c"),
            Spec::File("src/a.rs", b"a"),
            Spec::File("src/a-b.rs", b"ab"),
        ]);

        let store = MemStore::default();
        let manifest = Manifest {
            entries: vec![
                entry(&store, "README.md", b"hello\n", false),
                entry(&store, "bin/run", b"#!/bin/sh\n", true),
                entry(&store, "src/a-b.rs", b"ab", false),
                entry(&store, "src/a.rs", b"a", false),
                entry(&store, "src/a/b/c.rs", b"c", false),
            ],
        };
        let out = GitFixture::bare();
        let tree = TreeWriter::new(&out.repo, &store).write(&manifest).unwrap();
        assert_eq!(tree, expected);

        let written = out.repo.find_tree(tree).unwrap();
        let run = written.get_path(Path::new("bin/run")).unwrap();
        assert_eq!(run.filemode(), i32::from(FileMode::BlobExecutable));
        let readme = written.get_path(Path::new("README.md")).unwrap();
        assert_eq!(readme.filemode(), i32::from(FileMode::Blob));
    }

    #[test]
    fn tree_round_trip_through_manifest_is_identical() {
        let src = GitFixture::new();
        let tree_oid = src.tree(&[
            Spec::File(".gitignore", b"target/\n"),
            Spec::Exec("scripts/build.sh", b"#!/bin/sh\nmake\n"),
            Spec::File("docs/guide/intro.md", b"# Intro\n"),
            Spec::File("docs/guide.md", b"guide"),
            Spec::File("empty.txt", b""),
            Spec::File("bin.dat", &[0, 159, 146, 150, 255]),
        ]);
        let store = MemStore::default();
        let manifest = TreeConverter::new(&src.repo, &store)
            .manifest(&src.repo.find_tree(tree_oid).unwrap())
            .unwrap();

        let dst = GitFixture::bare();
        let rebuilt = TreeWriter::new(&dst.repo, &store).write(&manifest).unwrap();
        assert_eq!(rebuilt, tree_oid);
    }

    #[test]
    fn empty_manifest_is_the_empty_tree() {
        let out = GitFixture::bare();
        let store = MemStore::default();
        let tree = TreeWriter::new(&out.repo, &store)
            .write(&Manifest::default())
            .unwrap();
        assert_eq!(tree.to_string(), EMPTY_TREE);
    }

    #[test]
    fn blobs_are_read_once_per_writer() {
        let out = GitFixture::bare();
        let store = MemStore::default();
        let m1 = Manifest {
            entries: vec![
                entry(&store, "a", b"shared", false),
                entry(&store, "b", b"shared", false),
            ],
        };
        let m2 = Manifest {
            entries: vec![
                entry(&store, "a", b"shared", false),
                entry(&store, "c", b"other", false),
            ],
        };
        let mut writer = TreeWriter::new(&out.repo, &store);
        writer.write(&m1).unwrap();
        writer.write(&m2).unwrap();
        assert_eq!(store.reads(), 2);
    }

    #[test]
    fn missing_blob_is_an_error() {
        let out = GitFixture::bare();
        let store = MemStore::default();
        let manifest = Manifest {
            entries: vec![ManifestEntry {
                path: "ghost".to_owned(),
                hash: hash_bytes(b"never stored"),
                size: 12,
                executable: false,
                chunked: false,
            }],
        };
        let err = TreeWriter::new(&out.repo, &store)
            .write(&manifest)
            .unwrap_err();
        assert!(matches!(
            err,
            GitCompatError::SdkError(worktree_sdk::SdkError::ObjectNotFound(_))
        ));
    }

    #[test]
    fn invalid_and_conflicting_paths_are_rejected() {
        let store = MemStore::default();
        for bad in [
            "",
            "/abs",
            "a//b",
            "a/",
            "../x",
            "a/./b",
            ".git/config",
            "x/.GIT/y",
        ] {
            let manifest = Manifest {
                entries: vec![entry(&store, bad, b"1", false)],
            };
            assert!(
                matches!(layout(&manifest), Err(GitCompatError::ExportError(_))),
                "{bad:?} should be rejected"
            );
        }
        let conflict = Manifest {
            entries: vec![
                entry(&store, "a", b"1", false),
                entry(&store, "a/b", b"2", false),
            ],
        };
        let err = layout(&conflict).err().expect("conflict");
        assert!(err
            .to_string()
            .contains("both as a file and as a directory"));
        let reverse = Manifest {
            entries: vec![
                entry(&store, "a/b", b"2", false),
                entry(&store, "a", b"1", false),
            ],
        };
        assert!(layout(&reverse).is_err());
        let duplicate = Manifest {
            entries: vec![
                entry(&store, "a", b"1", false),
                entry(&store, "a", b"2", false),
            ],
        };
        assert!(layout(&duplicate).is_err());
    }

    #[test]
    fn commits_are_deterministic() {
        let out = GitFixture::bare();
        let empty = Oid::from_str(EMPTY_TREE).unwrap();
        out.repo.treebuilder(None).unwrap().write().unwrap();

        let root = snapshot("s1", &[], "first", 1_700_000_000);
        let (c1, new1) = write_commit(&out.repo, &root, empty, &[]).unwrap();
        assert!(new1);
        let (again, new_again) = write_commit(&out.repo, &root, empty, &[]).unwrap();
        assert_eq!((again, new_again), (c1, false));

        // Same content in a different repository gives the same SHA.
        let other = GitFixture::bare();
        other.repo.treebuilder(None).unwrap().write().unwrap();
        assert_eq!(write_commit(&other.repo, &root, empty, &[]).unwrap().0, c1);

        let child = snapshot("s2", &["s1"], "second\n\nbody  \n", 1_700_000_100);
        let (c2, _) = write_commit(&out.repo, &child, empty, &[c1]).unwrap();
        let commit = out.repo.find_commit(c2).unwrap();
        assert_eq!(commit.parent_ids().collect::<Vec<_>>(), vec![c1]);
        assert_eq!(commit.message(), Some("second\n\nbody\n"));
        assert_eq!(commit.author().name(), Some("Grace Hopper"));
        assert_eq!(commit.author().email(), Some("grace@example.com"));
        assert_eq!(commit.committer().name(), Some("Grace Hopper"));
        assert_eq!(commit.time().seconds(), 1_700_000_100);
        assert_eq!(commit.time().offset_minutes(), 0);
    }

    #[test]
    fn merge_commits_keep_parent_order() {
        let out = GitFixture::bare();
        let empty = out.repo.treebuilder(None).unwrap().write().unwrap();
        let (a, _) = write_commit(&out.repo, &snapshot("a", &[], "a", 1), empty, &[]).unwrap();
        let (b, _) = write_commit(&out.repo, &snapshot("b", &[], "b", 2), empty, &[]).unwrap();
        let (m, _) = write_commit(
            &out.repo,
            &snapshot("m", &["b", "a"], "m", 3),
            empty,
            &[b, a],
        )
        .unwrap();
        let merge = out.repo.find_commit(m).unwrap();
        assert_eq!(merge.parent_ids().collect::<Vec<_>>(), vec![b, a]);
    }

    #[test]
    fn open_or_init_creates_with_requested_head() {
        let dir = tempfile::tempdir().unwrap();
        let bare_path = dir.path().join("nested").join("out.git");
        let (git, created) = open_or_init(&bare_path, true, "trunk").unwrap();
        assert!(created && git.is_bare());
        assert_eq!(
            git.find_reference("HEAD").unwrap().symbolic_target(),
            Some("refs/heads/trunk")
        );
        drop(git);
        let (git, created) = open_or_init(&bare_path, false, "other").unwrap();
        assert!(!created && git.is_bare());

        let work_path = dir.path().join("work");
        let (git, created) = open_or_init(&work_path, false, "main").unwrap();
        assert!(created && !git.is_bare());
        assert!(work_tree_is_clean(&git).unwrap());
        assert_eq!(head_commit(&git), None);
    }

    #[test]
    fn refresh_updates_clean_work_tree_only() {
        let fx = GitFixture::new();
        let c1 = fx.commit(None, &[Spec::File("a.txt", b"one\n")], &[], "c1", 1);
        let c2 = fx.commit(None, &[Spec::File("a.txt", b"two\n")], &[c1], "c2", 2);
        let c3 = fx.commit(None, &[Spec::File("b.txt", b"three\n")], &[c2], "c3", 3);
        let file_a = fx.path().join("a.txt");

        // Unborn HEAD -> c1 on a fresh repository: files appear.
        fx.repo
            .reference("refs/heads/main", c1, true, "test")
            .unwrap();
        refresh_work_tree(&fx.repo, None, true).unwrap();
        assert_eq!(fs::read_to_string(&file_a).unwrap(), "one\n");

        // Clean -> c2: updated.
        assert!(work_tree_is_clean(&fx.repo).unwrap());
        fx.repo
            .reference("refs/heads/main", c2, true, "test")
            .unwrap();
        refresh_work_tree(&fx.repo, Some(c1), true).unwrap();
        assert_eq!(fs::read_to_string(&file_a).unwrap(), "two\n");

        // Dirty -> c3: local edit preserved, nothing removed.
        fs::write(&file_a, "local edit\n").unwrap();
        let was_clean = work_tree_is_clean(&fx.repo).unwrap();
        assert!(!was_clean);
        fx.repo
            .reference("refs/heads/main", c3, true, "test")
            .unwrap();
        refresh_work_tree(&fx.repo, Some(c2), was_clean).unwrap();
        assert_eq!(fs::read_to_string(&file_a).unwrap(), "local edit\n");
        assert!(!fx.path().join("b.txt").exists());

        // Unchanged HEAD: no-op.
        refresh_work_tree(&fx.repo, Some(c3), true).unwrap();
        assert_eq!(fs::read_to_string(&file_a).unwrap(), "local edit\n");
    }

    #[test]
    fn tree_writer_handles_every_imported_commit() {
        // Convert a small history and rebuild each tree in a fresh repository.
        let src = GitFixture::new();
        let c1 = src.commit(None, &[Spec::File("a", b"1")], &[], "c1", 1);
        let c2 = src.commit(
            None,
            &[Spec::File("a", b"1"), Spec::Exec("d/x", b"x")],
            &[c1],
            "c2",
            2,
        );
        let store = MemStore::default();
        let dst = GitFixture::bare();
        let mut conv = TreeConverter::new(&src.repo, &store);
        let mut writer = TreeWriter::new(&dst.repo, &store);
        let mut seen = HashSet::new();
        for oid in [c1, c2] {
            let commit = src.repo.find_commit(oid).unwrap();
            let manifest = conv.manifest(&commit.tree().unwrap()).unwrap();
            let rebuilt = writer.write(&manifest).unwrap();
            assert_eq!(rebuilt, commit.tree_id());
            seen.insert(rebuilt);
        }
        assert_eq!(seen.len(), 2);
    }
}
