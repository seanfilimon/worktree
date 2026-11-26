//! Server state -> Git objects: what `list` advertises and `fetch` delivers.
//!
//! Snapshots become commits with the same deterministic conversion as
//! `wt git export` (`TreeWriter` + `write_commit`), so a snapshot has the
//! same commit SHA in every clone and in every export. Unlike the export,
//! nothing here touches a ref, `HEAD` or the work tree of the user's
//! repository: only objects are written, and Git moves its own refs.
//!
//! A snapshot that was made from a Git commit (`git push` through the
//! helper, `wt git import`) carries an id derived from that commit's SHA.
//! When the repository already holds that very commit, it is used instead
//! of a rebuilt one — after checking that it converts to exactly this
//! snapshot — so people who have the original history keep their ids.

use std::collections::{HashMap, HashSet};

use git2::{ObjectType, Oid};
use tracing::warn;
use uuid::Uuid;
use worktree_sdk::{ObjectHash, ObjectStore, Repository, Snapshot, SnapshotId, TagInfo};

use super::cache::{Cache, TagObjects, ORIGIN};
use crate::error::{GitCompatError, Result};
use crate::export::{write_commit, TreeWriter};
use crate::ident;
use crate::map::GitMap;

/// The server's refs as Git objects in the local object database.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Exported {
    /// `(branch name, commit)`, sorted by name.
    pub branches: Vec<(String, Oid)>,
    /// `(tag name, commit or tag object)`, sorted by name.
    pub tags: Vec<(String, Oid)>,
    /// Commits of the branch tips and tag targets (history the server has).
    pub tips: Vec<Oid>,
    /// Commit objects newly written by this export.
    pub commits: usize,
}

struct Exporter<'a> {
    repo: &'a Repository,
    git: &'a git2::Repository,
    map: GitMap,
    map_changed: bool,
    resolved: HashMap<SnapshotId, Oid>,
    trees: HashMap<ObjectHash, Oid>,
    writer: TreeWriter<'a, ObjectStore>,
    commits: usize,
    /// Commits reachable from the repository's refs, keyed by the snapshot
    /// id an import gives them; built on first use.
    originals: Option<HashMap<SnapshotId, Oid>>,
}

/// Whether `id` can be the id of a snapshot made from a Git commit
/// (a version 5 UUID; ids of native snapshots are random).
fn is_import_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|uuid| uuid.get_version_num() == 5)
}

impl Exporter<'_> {
    /// The local commit `snapshot` was made from, if the repository has it
    /// and it still converts to exactly this snapshot (same files, parents,
    /// author, message and time). The id alone is not trusted: whoever
    /// pushed the snapshot chose it.
    fn original_commit(
        &mut self,
        snapshot: &Snapshot,
        tree: Oid,
        parents: &[Oid],
    ) -> Result<Option<Oid>> {
        if !is_import_id(&snapshot.id) {
            return Ok(None);
        }
        if self.originals.is_none() {
            let mut walk = self.git.revwalk()?;
            walk.push_glob("refs/*")?;
            let mut originals = HashMap::new();
            for oid in walk {
                let oid = oid?;
                originals.insert(ident::snapshot_id_for_commit(oid), oid);
            }
            self.originals = Some(originals);
        }
        let Some(oid) = self
            .originals
            .as_ref()
            .and_then(|originals| originals.get(&snapshot.id))
            .copied()
        else {
            return Ok(None);
        };
        let commit = self.git.find_commit(oid)?;
        let same = commit.tree_id() == tree
            && commit.parent_ids().eq(parents.iter().copied())
            && ident::author_of(&commit.author()) == snapshot.author
            && ident::message_from_git(commit.message_bytes()) == snapshot.message
            && ident::git_time_to_utc(commit.time()) == snapshot.created_at;
        Ok(same.then_some(oid))
    }

    /// Commit already standing for snapshot `id` in this Git repository.
    fn existing(&mut self, id: &str) -> Option<Oid> {
        if let Some(oid) = self.resolved.get(id) {
            return Some(*oid);
        }
        let oid = Oid::from_str(self.map.commit_for(id)?).ok()?;
        // Unreferenced objects can be pruned by `git gc`; rebuild them then.
        self.git.find_commit(oid).ok()?;
        self.resolved.insert(id.to_owned(), oid);
        Some(oid)
    }

    /// Write the commits of `tip` and of every ancestor that has none yet
    /// (parents first); returns the tip's commit. Only the new part of the
    /// history is read, so repeated fetches stay proportional to what
    /// changed.
    fn export_history(&mut self, tip: &str) -> Result<Oid> {
        if let Some(oid) = self.existing(tip) {
            return Ok(oid);
        }
        let mut entered: HashSet<SnapshotId> = HashSet::from([tip.to_owned()]);
        // (snapshot, index of its next parent to visit)
        let mut stack: Vec<(Snapshot, usize)> = vec![(self.repo.get_snapshot(tip)?, 0)];
        while let Some((snapshot, next)) = stack.last_mut() {
            match snapshot.parents.get(*next).cloned() {
                Some(parent) => {
                    *next += 1;
                    if self.existing(&parent).is_none() && entered.insert(parent.clone()) {
                        stack.push((self.repo.get_snapshot(&parent)?, 0));
                    }
                }
                None => {
                    if let Some((snapshot, _)) = stack.pop() {
                        self.export_snapshot(&snapshot)?;
                    }
                }
            }
        }
        self.resolved.get(tip).copied().ok_or_else(|| {
            GitCompatError::ExportError(format!("snapshot {tip} is not in its own ancestry"))
        })
    }

    fn export_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        let mut parents = Vec::with_capacity(snapshot.parents.len());
        for parent in &snapshot.parents {
            // Dropping a parent would change the commit SHA, so a history
            // with holes is an error here (the cache is never shallow).
            parents.push(self.resolved.get(parent).copied().ok_or_else(|| {
                GitCompatError::ExportError(format!(
                    "parent {parent} of snapshot {} has no commit",
                    snapshot.id
                ))
            })?);
        }
        let tree = match self.trees.get(&snapshot.manifest) {
            Some(tree) => *tree,
            None => {
                let manifest = self.repo.read_manifest(&snapshot.manifest)?;
                let tree = self.writer.write(&manifest)?;
                self.trees.insert(snapshot.manifest, tree);
                tree
            }
        };
        let (oid, new) = match self.original_commit(snapshot, tree, &parents)? {
            Some(original) => (original, false),
            None => write_commit(self.git, snapshot, tree, &parents)?,
        };
        if new {
            self.commits += 1;
        }
        self.resolved.insert(snapshot.id.clone(), oid);
        // Also replaces an entry whose commit is gone from this repository.
        let sha = oid.to_string();
        if self.map.commit_for(&snapshot.id) != Some(sha.as_str()) {
            self.map.insert(&snapshot.id, &sha);
            self.map_changed = true;
        }
        Ok(())
    }

    /// The Git object for an annotated tag: the object this repository
    /// pushed, if it is still here, else one rebuilt from the tag record.
    fn tag_object(
        &self,
        tag: &TagInfo,
        message: &str,
        commit: Oid,
        pushed: &TagObjects,
    ) -> Result<Oid> {
        if let Some(object) = pushed
            .object_for(&tag.name, &tag.target)
            .and_then(|sha| Oid::from_str(sha).ok())
        {
            let intact = self
                .git
                .find_tag(object)
                .is_ok_and(|found| found.target_id() == commit);
            if intact {
                return Ok(object);
            }
        }
        let target = self.git.find_object(commit, Some(ObjectType::Commit))?;
        let tagger = ident::signature(&tag.tagger, tag.created_at)?;
        Ok(self.git.tag_annotation_create(
            &tag.name,
            &target,
            &tagger,
            &ident::message_to_git(message),
        )?)
    }
}

/// Write the Git objects for everything the cache knows the server has —
/// the remote-tracking branches and the tags of `cache` — into `git`'s
/// object database, and record new commits in the cache's
/// `.wt/git-map.json`.
///
/// Branches and tags whose names are not valid Git ref names, and tags
/// whose target snapshot was not fetched, are skipped with a warning.
pub(crate) fn export_remote(cache: &Cache, git: &git2::Repository) -> Result<Exported> {
    let repo = &cache.repo;
    let wt_dir = repo.wt_dir();
    let mut exporter = Exporter {
        repo,
        git,
        map: GitMap::load(&wt_dir)?,
        map_changed: false,
        resolved: HashMap::new(),
        trees: HashMap::new(),
        writer: TreeWriter::new(git, repo.store()),
        commits: 0,
        originals: None,
    };
    let mut exported = Exported::default();

    for (name, tip) in repo.remote_branches(&cache.tree, ORIGIN)? {
        if !git2::Reference::is_valid_name(&format!("refs/heads/{name}")) {
            warn!(branch = %name, "skipping branch whose name is not a valid Git ref");
            continue;
        }
        let commit = exporter.export_history(&tip)?;
        exported.tips.push(commit);
        exported.branches.push((name, commit));
    }

    let pushed = TagObjects::load(&wt_dir)?;
    for tag in repo.tags(&cache.tree)? {
        if !git2::Reference::is_valid_name(&format!("refs/tags/{}", tag.name)) {
            warn!(tag = %tag.name, "skipping tag whose name is not a valid Git ref");
            continue;
        }
        if !repo.has_snapshot(&tag.target) {
            warn!(tag = %tag.name, target = %tag.target, "skipping tag with a missing target");
            continue;
        }
        let commit = exporter.export_history(&tag.target)?;
        let object = match &tag.message {
            Some(message) => exporter.tag_object(&tag, message, commit, &pushed)?,
            None => commit,
        };
        exported.tips.push(commit);
        exported.tags.push((tag.name, object));
    }

    if exporter.map_changed {
        exporter.map.save(&wt_dir)?;
    }
    exported.commits = exporter.commits;
    exported.branches.sort();
    exported.tags.sort();
    Ok(exported)
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use worktree_sdk::{InitOptions, Manifest, ManifestEntry};

    use super::*;
    use crate::test_support::GitFixture;

    fn cache(dir: &std::path::Path) -> Cache {
        let repo = Repository::init(
            dir,
            InitOptions {
                name: Some("proj".into()),
                default_branch: None,
            },
        )
        .unwrap();
        let tree = repo.root_tree().unwrap().name;
        Cache { repo, tree }
    }

    /// Store a snapshot with one file; ids must be valid snapshot ids.
    fn snapshot(cache: &Cache, n: u128, parents: &[&str], content: &str) -> String {
        let repo = &cache.repo;
        let hash = repo.store().put(content.as_bytes()).unwrap();
        let manifest = Manifest {
            entries: vec![ManifestEntry {
                path: "dir/file.txt".into(),
                hash,
                size: content.len() as u64,
                executable: false,
                chunked: false,
                symlink: false,
            }],
        };
        let id = uuid::Uuid::from_u128(n).to_string();
        repo.write_snapshot(&Snapshot {
            id: id.clone(),
            tree_id: repo.tree(&cache.tree).unwrap().id,
            branch: "main".into(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            manifest: repo.write_manifest(&manifest).unwrap(),
            author: "Grace Hopper <grace@example.com>".into(),
            message: format!("snapshot {n}"),
            created_at: DateTime::from_timestamp(1_700_000_000 + n as i64, 0).unwrap(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        })
        .unwrap();
        id
    }

    fn set_remote(cache: &Cache, branch: &str, tip: &str) {
        cache
            .repo
            .set_remote_tip(&cache.tree, ORIGIN, branch, Some(tip))
            .unwrap();
    }

    fn tag(cache: &Cache, name: &str, target: &str, message: Option<&str>) {
        cache
            .repo
            .put_tag(
                &cache.tree,
                &TagInfo {
                    name: name.into(),
                    target: target.into(),
                    message: message.map(str::to_owned),
                    tagger: "Tagger <tagger@example.com>".into(),
                    created_at: DateTime::from_timestamp(1_700_001_000, 0).unwrap(),
                    signature: None,
                },
            )
            .unwrap();
    }

    #[test]
    fn exports_branches_and_tags_as_objects_only() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let s1 = snapshot(&cache, 1, &[], "one\n");
        let s2 = snapshot(&cache, 2, &[&s1], "two\n");
        let side = snapshot(&cache, 3, &[&s1], "side\n");
        let merge = snapshot(&cache, 4, &[&s2, &side], "merged\n");
        set_remote(&cache, "main", &merge);
        set_remote(&cache, "topic", &side);
        tag(&cache, "v1", &s1, None);
        tag(&cache, "v2", &s2, Some("release two"));

        let git = GitFixture::bare();
        let exported = export_remote(&cache, &git.repo).unwrap();
        assert_eq!(exported.commits, 4);
        let names: Vec<_> = exported.branches.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["main", "topic"]);

        let main = git.repo.find_commit(exported.branches[0].1).unwrap();
        assert_eq!(main.message(), Some("snapshot 4\n"));
        assert_eq!(main.parent_count(), 2);
        assert_eq!(main.parent_id(1).unwrap(), exported.branches[1].1);
        let blob = main
            .tree()
            .unwrap()
            .get_path(std::path::Path::new("dir/file.txt"))
            .unwrap()
            .to_object(&git.repo)
            .unwrap();
        assert_eq!(blob.as_blob().unwrap().content(), b"merged\n");

        // v1 is lightweight (the commit itself), v2 an annotated tag object.
        let (v1, v2) = (exported.tags[0].1, exported.tags[1].1);
        assert_eq!(
            git.repo.find_commit(v1).unwrap().message(),
            Some("snapshot 1\n")
        );
        let annotated = git.repo.find_tag(v2).unwrap();
        assert_eq!(annotated.name(), Some("v2"));
        assert_eq!(annotated.message(), Some("release two\n"));
        assert_eq!(annotated.target_id(), main.parent_id(0).unwrap());

        // Nothing but objects was written to the user's repository.
        assert_eq!(git.repo.references().unwrap().count(), 0);

        // The mapping is recorded in the cache.
        let map = GitMap::load(&cache.repo.wt_dir()).unwrap();
        assert_eq!(map.commit_for(&merge), Some(main.id().to_string().as_str()));
    }

    #[test]
    fn commit_ids_do_not_depend_on_the_clone() {
        let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut tips = Vec::new();
        for dir in [&dir_a, &dir_b] {
            let cache = cache(dir.path());
            let s1 = snapshot(&cache, 1, &[], "one\n");
            let s2 = snapshot(&cache, 2, &[&s1], "two\n");
            set_remote(&cache, "main", &s2);
            tag(&cache, "v1", &s2, Some("annotated"));
            let git = GitFixture::bare();
            let exported = export_remote(&cache, &git.repo).unwrap();
            tips.push((exported.branches[0].1, exported.tags[0].1));
        }
        assert_eq!(tips[0], tips[1]);
    }

    #[test]
    fn repeated_exports_are_incremental_and_survive_pruned_objects() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let s1 = snapshot(&cache, 1, &[], "one\n");
        set_remote(&cache, "main", &s1);
        let git = GitFixture::bare();
        let first = export_remote(&cache, &git.repo).unwrap();
        assert_eq!(first.commits, 1);

        let again = export_remote(&cache, &git.repo).unwrap();
        assert_eq!(again.commits, 0);
        assert_eq!(again.branches, first.branches);

        let s2 = snapshot(&cache, 2, &[&s1], "two\n");
        set_remote(&cache, "main", &s2);
        let next = export_remote(&cache, &git.repo).unwrap();
        assert_eq!(next.commits, 1);
        let tip = git.repo.find_commit(next.branches[0].1).unwrap();
        assert_eq!(tip.parent_id(0).unwrap(), first.branches[0].1);

        // A fresh Git repository (objects gone): everything is rebuilt with
        // the same ids although the map still lists the old commits.
        let fresh = GitFixture::bare();
        let rebuilt = export_remote(&cache, &fresh.repo).unwrap();
        assert_eq!(rebuilt.commits, 2);
        assert_eq!(rebuilt.branches, next.branches);
    }

    #[test]
    fn original_commits_are_recognised_when_the_repository_has_them() {
        use crate::remote_helper::to_wt::CommitConverter;
        use crate::test_support::Spec;

        // Commits with a +01:00 time zone: rebuilding them from snapshots
        // gives other ids.
        let git = GitFixture::bare();
        let c1 = git.commit(
            Some("refs/heads/main"),
            &[Spec::File("a.txt", b"one\n")],
            &[],
            "first",
            1_700_000_000,
        );
        let c2 = git.commit(
            Some("refs/heads/main"),
            &[Spec::File("a.txt", b"two\n")],
            &[c1],
            "second",
            1_700_000_100,
        );

        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let tree_id = cache.repo.tree(&cache.tree).unwrap().id;
        let mut scratch = GitMap::default();
        let (tip, _) = CommitConverter::new(&cache.repo, &git.repo, &tree_id)
            .convert(c2, "main", &[], &mut scratch)
            .unwrap();
        set_remote(&cache, "main", &tip);

        // No map (a fresh cache): the repository's own commits are found.
        let exported = export_remote(&cache, &git.repo).unwrap();
        assert_eq!(exported.branches, vec![("main".to_owned(), c2)]);
        assert_eq!(exported.commits, 0);
        let map = GitMap::load(&cache.repo.wt_dir()).unwrap();
        assert_eq!(
            map.snapshot_for(&c1.to_string()),
            scratch.snapshot_for(&c1.to_string())
        );

        // A repository without them rebuilds the commits (other ids).
        let elsewhere = GitFixture::bare();
        let rebuilt = export_remote(&cache, &elsewhere.repo).unwrap();
        assert_eq!(rebuilt.commits, 2);
        assert_ne!(rebuilt.branches[0].1, c2);

        // A snapshot that merely claims a commit's id is not believed.
        let forged_dir = tempfile::tempdir().unwrap();
        let forged = self::cache(forged_dir.path());
        let parent = cache.repo.get_snapshot(&tip).unwrap().parents[0].clone();
        for id in [&parent, &tip] {
            let mut record = cache.repo.get_snapshot(id).unwrap();
            let manifest = cache.repo.read_manifest(&record.manifest).unwrap();
            for entry in &manifest.entries {
                let data = cache.repo.store().get(&entry.hash).unwrap();
                forged.repo.store().put(&data).unwrap();
            }
            record.manifest = forged.repo.write_manifest(&manifest).unwrap();
            if *id == tip {
                record.message = "something else".into();
            }
            forged.repo.write_snapshot(&record).unwrap();
        }
        set_remote(&forged, "main", &tip);
        let exported = export_remote(&forged, &git.repo).unwrap();
        let commit = git.repo.find_commit(exported.branches[0].1).unwrap();
        assert_ne!(commit.id(), c2);
        assert_eq!(commit.message(), Some("something else\n"));
        // Its honest parent is still the original commit.
        assert_eq!(commit.parent_id(0).unwrap(), c1);
    }

    #[test]
    fn pushed_tag_objects_are_reused_while_they_match() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache(dir.path());
        let s1 = snapshot(&cache, 1, &[], "one\n");
        set_remote(&cache, "main", &s1);
        tag(&cache, "v1", &s1, Some("release"));
        let git = GitFixture::bare();
        let rebuilt = export_remote(&cache, &git.repo).unwrap();
        let commit = rebuilt.branches[0].1;

        // The user's own tag object: other tagger time zone, same meaning.
        let tagger = git2::Signature::new(
            "Tagger",
            "tagger@example.com",
            &git2::Time::new(1_700_001_000, 120),
        )
        .unwrap();
        let object = git.repo.find_object(commit, None).unwrap();
        let own = git
            .repo
            .tag_annotation_create("v1", &object, &tagger, "release\n")
            .unwrap();
        assert_ne!(own, rebuilt.tags[0].1);
        let mut pushed = TagObjects::default();
        pushed.record("v1", &own.to_string(), &s1);
        pushed.save(&cache.repo.wt_dir()).unwrap();
        assert_eq!(export_remote(&cache, &git.repo).unwrap().tags[0].1, own);

        // Recorded for another snapshot: not reused.
        pushed.record("v1", &own.to_string(), "other-snapshot");
        pushed.save(&cache.repo.wt_dir()).unwrap();
        assert_eq!(
            export_remote(&cache, &git.repo).unwrap().tags[0].1,
            rebuilt.tags[0].1
        );
    }
}
