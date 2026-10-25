//! Test fixtures: programmatically built Git repositories and in-memory
//! object store fakes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use git2::{
    FileMode, IndexEntry, IndexTime, Oid, Repository, RepositoryInitOptions, Signature, Time,
};
use tempfile::TempDir;
use worktree_sdk::{store::hash_bytes, ObjectHash, SdkError};

use crate::error::Result;
use crate::export::BlobSource;
use crate::import::BlobSink;

/// Point `WT_HOME` at a process-wide temporary directory so tests never
/// touch the developer's real per-user state.
pub(crate) fn isolate_wt_home() {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    let home = HOME.get_or_init(|| tempfile::tempdir().expect("WT_HOME tempdir"));
    std::env::set_var("WT_HOME", home.path());
}

/// One entry of a fixture tree.
#[derive(Clone, Copy)]
pub(crate) enum Spec<'a> {
    File(&'a str, &'a [u8]),
    Exec(&'a str, &'a [u8]),
    Link(&'a str, &'a str),
    Gitlink(&'a str, Oid),
}

/// A Git repository in a temp dir with helpers to build history directly
/// in the object database (no working-tree files involved).
pub(crate) struct GitFixture {
    pub dir: TempDir,
    pub repo: Repository,
}

impl GitFixture {
    pub fn new() -> Self {
        Self::with_options(false)
    }

    pub fn bare() -> Self {
        Self::with_options(true)
    }

    fn with_options(bare: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut opts = RepositoryInitOptions::new();
        opts.bare(bare).initial_head("main");
        let repo = Repository::init_opts(dir.path(), &opts).expect("git init");
        // Keep checkouts byte-exact regardless of the developer's global config.
        repo.config()
            .and_then(|mut config| config.set_bool("core.autocrlf", false))
            .expect("disable autocrlf");
        Self { dir, repo }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Build a tree from `specs` through an in-memory index.
    pub fn tree(&self, specs: &[Spec<'_>]) -> Oid {
        let mut index = git2::Index::new().expect("index");
        for spec in specs {
            let (path, id, mode) = match *spec {
                Spec::File(p, data) => (p, self.repo.blob(data).expect("blob"), FileMode::Blob),
                Spec::Exec(p, data) => (
                    p,
                    self.repo.blob(data).expect("blob"),
                    FileMode::BlobExecutable,
                ),
                Spec::Link(p, target) => (
                    p,
                    self.repo.blob(target.as_bytes()).expect("blob"),
                    FileMode::Link,
                ),
                Spec::Gitlink(p, commit) => (p, commit, FileMode::Commit),
            };
            index
                .add(&IndexEntry {
                    ctime: IndexTime::new(0, 0),
                    mtime: IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: u32::from(mode),
                    uid: 0,
                    gid: 0,
                    file_size: 0,
                    id,
                    flags: path.len().min(0xfff) as u16,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                })
                .expect("index add");
        }
        index.write_tree_to(&self.repo).expect("write tree")
    }

    /// Create a commit (optionally updating `refname`) at a fixed time.
    pub fn commit(
        &self,
        refname: Option<&str>,
        specs: &[Spec<'_>],
        parents: &[Oid],
        message: &str,
        time: i64,
    ) -> Oid {
        let tree = self.repo.find_tree(self.tree(specs)).expect("tree");
        let sig = Signature::new("Ada Lovelace", "ada@example.com", &Time::new(time, 60))
            .expect("signature");
        let parents: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(*p).expect("parent"))
            .collect();
        let parent_refs: Vec<_> = parents.iter().collect();
        self.repo
            .commit(refname, &sig, &sig, message, &tree, &parent_refs)
            .expect("commit")
    }

    pub fn tag_annotated(&self, name: &str, target: Oid, message: &str, time: i64) -> Oid {
        let obj = self.repo.find_object(target, None).expect("object");
        let sig =
            Signature::new("Tagger", "tagger@example.com", &Time::new(time, 0)).expect("signature");
        self.repo
            .tag(name, &obj, &sig, message, false)
            .expect("annotated tag")
    }

    pub fn tag_lightweight(&self, name: &str, target: Oid) {
        let obj = self.repo.find_object(target, None).expect("object");
        self.repo
            .tag_lightweight(name, &obj, false)
            .expect("lightweight tag");
    }
}

/// In-memory blob store implementing both import and export blob traits.
#[derive(Default)]
pub(crate) struct MemStore {
    blobs: RefCell<HashMap<ObjectHash, Vec<u8>>>,
    reads: Cell<usize>,
}

impl MemStore {
    pub fn len(&self) -> usize {
        self.blobs.borrow().len()
    }

    pub fn reads(&self) -> usize {
        self.reads.get()
    }

    pub fn get(&self, hash: &ObjectHash) -> Option<Vec<u8>> {
        self.blobs.borrow().get(hash).cloned()
    }
}

impl BlobSink for MemStore {
    fn put_blob(&self, data: &[u8]) -> Result<ObjectHash> {
        let hash = hash_bytes(data);
        self.blobs.borrow_mut().insert(hash, data.to_vec());
        Ok(hash)
    }
}

impl BlobSource for MemStore {
    fn read_blob(&self, entry: &worktree_sdk::ManifestEntry) -> Result<Vec<u8>> {
        self.reads.set(self.reads.get() + 1);
        self.get(&entry.hash)
            .ok_or_else(|| SdkError::ObjectNotFound(entry.hash.to_hex()).into())
    }
}
