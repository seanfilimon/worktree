//! Git commits -> snapshots in the cache worktree: the first half of a
//! `git push`.
//!
//! Each commit that has no snapshot yet becomes one, with the id
//! `wt git import` would give it (UUID v5 of the commit SHA), and is
//! recorded in the cache's `.wt/git-map.json`. Nothing is sent anywhere
//! here; the SDK pushes the branch afterwards.
//!
//! Content W0rkTree cannot store is refused, never dropped: a push either
//! carries every file of every commit or fails before reaching the server.

use std::collections::{BTreeMap, HashMap};

use git2::{FileMode, ObjectType, Oid, Sort};
use worktree_sdk::{Manifest, ObjectHash, ObjectStore, Repository, Snapshot, SnapshotId};

use crate::error::{GitCompatError, Result};
use crate::ident;
use crate::import::TreeConverter;
use crate::map::GitMap;

/// At most this many offending paths are named in a refusal.
const MAX_REPORTED_PATHS: usize = 5;

fn refused(message: String) -> GitCompatError {
    GitCompatError::ImportError(message)
}

fn short(oid: Oid) -> String {
    oid.to_string()[..10].to_owned()
}

/// Converts commits of `git` into snapshots of one tree of `repo`.
pub(crate) struct CommitConverter<'a> {
    repo: &'a Repository,
    git: &'a git2::Repository,
    tree_id: String,
    trees: TreeConverter<'a, ObjectStore>,
    /// Git tree -> manifest hash.
    manifests: HashMap<Oid, ObjectHash>,
    /// Git tree -> number of non-directory entries below it.
    leaves: HashMap<Oid, usize>,
}

impl<'a> CommitConverter<'a> {
    /// A converter writing snapshots of the tree with id `tree_id`.
    pub fn new(repo: &'a Repository, git: &'a git2::Repository, tree_id: &str) -> Self {
        Self {
            repo,
            git,
            tree_id: tree_id.to_owned(),
            trees: TreeConverter::new(git, repo.store()),
            manifests: HashMap::new(),
            leaves: HashMap::new(),
        }
    }

    /// Make sure `tip` and all its ancestors have snapshots, attributing
    /// new ones to `branch`; returns the tip's snapshot id and how many
    /// snapshots were written.
    ///
    /// `known` are commits whose whole history already has snapshots (the
    /// server's tips); the walk stops there. `map` gains every converted
    /// commit; the caller saves it.
    ///
    /// Errors with `ImportError` when a commit holds content W0rkTree
    /// cannot store (symbolic links, submodules, non-UTF-8 names, a `.wt`
    /// path) or when history is missing (shallow clone).
    pub fn convert(
        &mut self,
        tip: Oid,
        branch: &str,
        known: &[Oid],
        map: &mut GitMap,
    ) -> Result<(SnapshotId, usize)> {
        let mut walk = self.git.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
        walk.push(tip)?;
        for oid in known {
            walk.hide(*oid)?;
        }
        let mut written = 0;
        for oid in walk {
            let oid = oid?;
            if self.snapshot_of(oid, map).is_some() {
                continue;
            }
            let snapshot = self.snapshot(oid, branch, map)?;
            self.repo.write_snapshot(&snapshot)?;
            map.insert(&snapshot.id, &oid.to_string());
            written += 1;
        }
        let id = self
            .snapshot_of(tip, map)
            .ok_or_else(|| refused(format!("commit {} was not converted", short(tip))))?;
        Ok((id, written))
    }

    /// Snapshot standing for `commit`, if it is stored in the cache.
    fn snapshot_of(&self, commit: Oid, map: &GitMap) -> Option<SnapshotId> {
        let id = map.snapshot_for(&commit.to_string())?;
        self.repo.has_snapshot(id).then(|| id.to_owned())
    }

    fn snapshot(&mut self, oid: Oid, branch: &str, map: &GitMap) -> Result<Snapshot> {
        let commit = self.git.find_commit(oid)?;
        let mut parents = Vec::with_capacity(commit.parent_count());
        for parent in commit.parent_ids() {
            parents.push(self.snapshot_of(parent, map).ok_or_else(|| {
                refused(format!(
                    "parent {} of commit {} is not available (shallow clone?); \
                     run `git fetch --unshallow` and push again",
                    short(parent),
                    short(oid)
                ))
            })?);
        }
        let manifest = match self.manifests.get(&commit.tree_id()) {
            Some(hash) => *hash,
            None => {
                let tree = commit.tree()?;
                let manifest = self.trees.manifest(&tree)?;
                self.check_complete(oid, &tree, &manifest)?;
                let hash = self.repo.write_manifest(&manifest)?;
                self.manifests.insert(commit.tree_id(), hash);
                hash
            }
        };
        let author = ident::author_of(&commit.author());
        Ok(Snapshot {
            id: ident::snapshot_id_for_commit(oid),
            tree_id: self.tree_id.clone(),
            branch: branch.to_owned(),
            parents,
            manifest,
            author,
            message: ident::message_from_git(commit.message_bytes()),
            created_at: ident::git_time_to_utc(commit.time()),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        })
    }

    /// Refuse `commit` unless `manifest` holds every entry of `tree` and
    /// no path W0rkTree reserves.
    fn check_complete(
        &mut self,
        commit: Oid,
        tree: &git2::Tree<'_>,
        manifest: &Manifest,
    ) -> Result<()> {
        if let Some(entry) = manifest
            .entries
            .iter()
            .find(|e| e.path.split('/').any(|c| c.eq_ignore_ascii_case(".wt")))
        {
            return Err(refused(format!(
                "commit {} contains {:?}: `.wt` is reserved for W0rkTree metadata; \
                 remove it from the history you push",
                short(commit),
                entry.path
            )));
        }
        // The tree converter skips what W0rkTree cannot represent. Counting
        // is cheap (memoized per subtree); only a mismatch pays for a walk
        // that names the entries.
        if self.leaf_count(tree)? == manifest.entries.len() {
            return Ok(());
        }
        let mut all = BTreeMap::new();
        collect_leaves(self.git, tree, "", &mut all)?;
        for entry in &manifest.entries {
            all.remove(&entry.path);
        }
        let mut described: Vec<String> = all
            .iter()
            .take(MAX_REPORTED_PATHS)
            .map(|(path, mode)| format!("{path} ({})", describe_mode(*mode)))
            .collect();
        if all.len() > MAX_REPORTED_PATHS {
            described.push(format!("and {} more", all.len() - MAX_REPORTED_PATHS));
        }
        Err(refused(format!(
            "commit {} contains entries W0rkTree cannot store: {}; \
             nothing was pushed — replace them with regular files in the history you push",
            short(commit),
            described.join(", ")
        )))
    }

    fn leaf_count(&mut self, tree: &git2::Tree<'_>) -> Result<usize> {
        if let Some(count) = self.leaves.get(&tree.id()) {
            return Ok(*count);
        }
        let mut count = 0;
        for entry in tree.iter() {
            if entry.kind() == Some(ObjectType::Tree) {
                let subtree = self.git.find_tree(entry.id())?;
                count += self.leaf_count(&subtree)?;
            } else {
                count += 1;
            }
        }
        self.leaves.insert(tree.id(), count);
        Ok(count)
    }
}

/// Every non-directory entry below `tree` as path -> file mode.
fn collect_leaves(
    git: &git2::Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
    out: &mut BTreeMap<String, i32>,
) -> Result<()> {
    for entry in tree.iter() {
        let name = String::from_utf8_lossy(entry.name_bytes());
        let path = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.kind() == Some(ObjectType::Tree) {
            collect_leaves(git, &git.find_tree(entry.id())?, &path, out)?;
        } else {
            out.insert(path, entry.filemode());
        }
    }
    Ok(())
}

fn describe_mode(mode: i32) -> String {
    if mode == i32::from(FileMode::Link) {
        "symbolic link".to_owned()
    } else if mode == i32::from(FileMode::Commit) {
        "submodule".to_owned()
    } else if mode == i32::from(FileMode::Blob) || mode == i32::from(FileMode::BlobExecutable) {
        "file name is not valid UTF-8".to_owned()
    } else {
        format!("mode {mode:o}")
    }
}

#[cfg(test)]
mod tests {
    use worktree_sdk::InitOptions;

    use super::*;
    use crate::test_support::{GitFixture, Spec};

    fn repo(dir: &std::path::Path) -> Repository {
        Repository::init(
            dir,
            InitOptions {
                name: Some("proj".into()),
                default_branch: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn converts_new_commits_and_reuses_known_ones() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let fx = GitFixture::new();
        let c1 = fx.commit(None, &[Spec::File("a.txt", b"one\n")], &[], "first\n", 10);
        let c2 = fx.commit(
            None,
            &[
                Spec::File("a.txt", b"two\n"),
                Spec::Exec("bin/run", b"#!\n"),
            ],
            &[c1],
            "second\n\nbody\n",
            20,
        );
        let side = fx.commit(None, &[Spec::File("b.txt", b"b\n")], &[c1], "side", 15);
        let merge = fx.commit(
            None,
            &[Spec::File("a.txt", b"two\n"), Spec::File("b.txt", b"b\n")],
            &[c2, side],
            "merge",
            30,
        );

        let mut map = GitMap::default();
        let mut converter = CommitConverter::new(&repo, &fx.repo, "tree-1");
        let (tip, written) = converter.convert(c2, "main", &[], &mut map).unwrap();
        assert_eq!(written, 2);
        assert_eq!(tip, ident::snapshot_id_for_commit(c2));
        let snap = repo.get_snapshot(&tip).unwrap();
        assert_eq!(snap.parents, vec![ident::snapshot_id_for_commit(c1)]);
        assert_eq!(snap.branch, "main");
        assert_eq!(snap.tree_id, "tree-1");
        assert_eq!(snap.message, "second\n\nbody");
        assert_eq!(snap.author, "Ada Lovelace <ada@example.com>");
        assert_eq!(snap.created_at.timestamp(), 20);
        let manifest = repo.read_manifest(&snap.manifest).unwrap();
        let files: Vec<_> = manifest
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e.executable))
            .collect();
        assert_eq!(files, vec![("a.txt", false), ("bin/run", true)]);
        assert_eq!(map.snapshot_for(&c2.to_string()), Some(tip.as_str()));

        // Pushing the merge later converts only the two new commits, on the
        // branch they are pushed to, and keeps the parent order.
        let (merged, written) = converter.convert(merge, "topic", &[c2], &mut map).unwrap();
        assert_eq!(written, 2);
        let snap = repo.get_snapshot(&merged).unwrap();
        assert_eq!(
            snap.parents,
            vec![tip.clone(), ident::snapshot_id_for_commit(side)]
        );
        assert_eq!(snap.branch, "topic");
        assert_eq!(repo.get_snapshot(&tip).unwrap().branch, "main");

        // Nothing left to do.
        let (again, written) = converter.convert(merge, "topic", &[], &mut map).unwrap();
        assert_eq!((again, written), (merged, 0));
    }

    #[test]
    fn parents_resolve_through_the_map_not_the_commit_sha() {
        // A commit exported from a native snapshot: its snapshot id is
        // whatever the map says, not the UUID v5 of the commit.
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let fx = GitFixture::new();
        let exported = fx.commit(None, &[Spec::File("a", b"1")], &[], "native", 10);
        let child = fx.commit(None, &[Spec::File("a", b"2")], &[exported], "git", 20);

        let native = uuid::Uuid::from_u128(7).to_string();
        let mut seed = CommitConverter::new(&repo, &fx.repo, "t");
        let mut scratch = GitMap::default();
        let (seeded, _) = seed.convert(exported, "main", &[], &mut scratch).unwrap();
        let mut record = repo.get_snapshot(&seeded).unwrap();
        record.id = native.clone();
        repo.write_snapshot(&record).unwrap();

        let mut map = GitMap::default();
        map.insert(&native, &exported.to_string());
        let mut converter = CommitConverter::new(&repo, &fx.repo, "t");
        let (tip, written) = converter
            .convert(child, "main", &[exported], &mut map)
            .unwrap();
        assert_eq!(written, 1);
        assert_eq!(repo.get_snapshot(&tip).unwrap().parents, vec![native]);
    }

    #[test]
    fn refuses_symlinks_and_submodules_without_writing_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let fx = GitFixture::new();
        let base = fx.commit(None, &[Spec::File("a", b"1")], &[], "base", 10);
        let bad = fx.commit(
            None,
            &[
                Spec::File("a", b"1"),
                Spec::Link("docs/link", "../a"),
                Spec::Gitlink("vendor/sub", base),
            ],
            &[base],
            "links",
            20,
        );
        let mut map = GitMap::default();
        let mut converter = CommitConverter::new(&repo, &fx.repo, "t");
        let err = converter
            .convert(bad, "main", &[], &mut map)
            .expect_err("must refuse");
        let message = err.to_string();
        assert!(matches!(err, GitCompatError::ImportError(_)), "{message}");
        assert!(message.contains("docs/link (symbolic link)"), "{message}");
        assert!(message.contains("vendor/sub (submodule)"), "{message}");
        assert!(message.contains(&bad.to_string()[..10]), "{message}");
        // The clean parent was converted; the refused commit was not.
        assert!(repo.has_snapshot(&ident::snapshot_id_for_commit(base)));
        assert!(!repo.has_snapshot(&ident::snapshot_id_for_commit(bad)));
        assert_eq!(map.snapshot_for(&bad.to_string()), None);
    }

    #[test]
    fn refuses_reserved_metadata_paths() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let fx = GitFixture::new();
        let bad = fx.commit(
            None,
            &[Spec::File("sub/.WT/config.toml", b"x")],
            &[],
            "sneaky",
            10,
        );
        let mut map = GitMap::default();
        let err = CommitConverter::new(&repo, &fx.repo, "t")
            .convert(bad, "main", &[], &mut map)
            .expect_err("must refuse");
        assert!(err.to_string().contains("reserved for W0rkTree"), "{err}");
    }

    #[test]
    fn many_unsupported_entries_are_summarised() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let fx = GitFixture::new();
        let names: Vec<String> = (0..8).map(|i| format!("l{i}")).collect();
        let specs: Vec<Spec<'_>> = names.iter().map(|n| Spec::Link(n, "x")).collect();
        let bad = fx.commit(None, &specs, &[], "links", 10);
        let mut map = GitMap::default();
        let err = CommitConverter::new(&repo, &fx.repo, "t")
            .convert(bad, "main", &[], &mut map)
            .expect_err("must refuse");
        let message = err.to_string();
        assert!(
            message.contains("l0 (symbolic link)") && message.contains("and 3 more"),
            "{message}"
        );
    }
}
