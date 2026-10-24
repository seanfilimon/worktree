//! High-level Git <-> W0rkTree bridge used by `wt git import|export`.
//!
//! Contract (docs/ENGINEERING_PLAN.md): import converts every Git branch's
//! commit DAG into snapshots on the root tree (preserving parents, author,
//! timestamp, message), converts lightweight/annotated tags, and converts
//! `.gitignore` into `.wt/ignore`. Export writes snapshots as Git commits;
//! the snapshot-id <-> commit-sha mapping is kept in `.wt/git-map.json` so
//! repeated exports are incremental and deterministic.
//!
//! Conversion rules:
//!
//! * Snapshot ids of imported commits are UUID v5 of the commit SHA
//!   ([`crate::ident::snapshot_id_for_commit`]), so re-imports are stable.
//! * Regular files keep their executable bit (`100755`); symlinks and
//!   submodule gitlinks are skipped with a warning.
//! * Each commit is attributed to the first branch containing it, the
//!   checked-out branch first, then the others by name.
//! * Exported commits use the snapshot author as author and committer, the
//!   snapshot time in UTC and the message terminated by a newline. Branch
//!   and tag refs in the target are overwritten (W0rkTree is the source of
//!   truth); a non-bare target's files follow HEAD only if it had no local
//!   modifications.

use std::path::{Path, PathBuf};

use worktree_sdk::Repository;

use crate::error::Result;

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Only import these branches (default: all local branches).
    pub branches: Vec<String>,
    /// Name for the new worktree (default: destination dir name).
    pub name: Option<String>,
    /// Import tags.
    pub include_tags: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub dest: PathBuf,
    pub branches: Vec<String>,
    pub snapshots: usize,
    pub blobs: usize,
    pub tags: usize,
    /// Branch checked out in the working directory.
    pub checked_out: Option<String>,
}

/// Create a new worktree at `dest` from the Git repository at `git_path`.
pub fn import_repository(
    git_path: &Path,
    dest: &Path,
    opts: &ImportOptions,
) -> Result<ImportReport> {
    crate::import::import_repository(git_path, dest, opts)
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Tree to export (default: root tree).
    pub tree: Option<String>,
    /// Only export these branches (default: all).
    pub branches: Vec<String>,
    pub include_tags: bool,
    /// Create the target as a bare repository if it does not exist.
    pub bare: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub git_path: PathBuf,
    pub branches: Vec<String>,
    /// Newly written commits (already-mapped snapshots are skipped).
    pub commits: usize,
    pub tags: usize,
}

/// Export `repo` into the Git repository at `git_path` (created if missing).
pub fn export_repository(
    repo: &Repository,
    git_path: &Path,
    opts: &ExportOptions,
) -> Result<ExportReport> {
    crate::export::export_repository(repo, git_path, opts)
}

/// End-to-end tests over the real SDK repository engine.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use git2::Oid;
    use worktree_sdk::{SnapshotOptions, SwitchOptions};

    use super::*;
    use crate::ident::snapshot_id_for_commit;
    use crate::map::GitMap;
    use crate::test_support::{isolate_wt_home, GitFixture, Spec};

    /// History used by most tests:
    ///
    /// ```text
    /// c1 -- c2 ------- m   (main)
    ///   \             /
    ///    f1 -------- f2    (feature)
    /// ```
    struct Sample {
        fx: GitFixture,
        c1: Oid,
        c2: Oid,
        f1: Oid,
        f2: Oid,
        m: Oid,
    }

    fn sample() -> Sample {
        let fx = GitFixture::new();
        let base: &[Spec<'_>] = &[
            Spec::File(".gitignore", b"target/\n*.log\n"),
            Spec::File("README.md", b"# Sample\n"),
            Spec::File("src/main.rs", b"fn main() {}\n"),
        ];
        let c1 = fx.commit(
            Some("refs/heads/main"),
            base,
            &[],
            "Initial commit\n",
            1_600_000_000,
        );
        let c2 = fx.commit(
            Some("refs/heads/main"),
            &[
                base[0],
                base[1],
                base[2],
                Spec::Exec("scripts/build.sh", b"#!/bin/sh\ncargo build\n"),
            ],
            &[c1],
            "Add build script\n",
            1_600_000_100,
        );
        let f1 = fx.commit(
            Some("refs/heads/feature"),
            &[
                base[0],
                base[1],
                base[2],
                Spec::File("src/lib/util/mod.rs", b"pub fn u() {}\n"),
            ],
            &[c1],
            "Add util\n\nWith a body.\n",
            1_600_000_050,
        );
        let f2 = fx.commit(
            Some("refs/heads/feature"),
            &[
                base[0],
                base[1],
                base[2],
                Spec::File("src/lib/util/mod.rs", b"pub fn u() -> u8 { 1 }\n"),
                Spec::Link("link-to-readme", "README.md"),
            ],
            &[f1],
            "Improve util\n",
            1_600_000_150,
        );
        let m = fx.commit(
            Some("refs/heads/main"),
            &[
                base[0],
                base[1],
                base[2],
                Spec::Exec("scripts/build.sh", b"#!/bin/sh\ncargo build\n"),
                Spec::File("src/lib/util/mod.rs", b"pub fn u() -> u8 { 1 }\n"),
            ],
            &[c2, f2],
            "Merge feature\n",
            1_600_000_200,
        );
        fx.tag_lightweight("v0.1", c2);
        fx.tag_annotated("v1.0", m, "Release 1.0\n", 1_600_000_300);
        Sample {
            fx,
            c1,
            c2,
            f1,
            f2,
            m,
        }
    }

    fn import_all(git_path: &Path, dest: &Path) -> ImportReport {
        import_repository(
            git_path,
            dest,
            &ImportOptions {
                include_tags: true,
                ..ImportOptions::default()
            },
        )
        .expect("import")
    }

    /// Every commit reachable from `tip`, keyed by its (unique in the
    /// sample) message: (commit id, parent ids, tree id).
    fn history(git: &git2::Repository, tip: Oid) -> HashMap<String, (Oid, Vec<Oid>, Oid)> {
        let mut walk = git.revwalk().unwrap();
        walk.push(tip).unwrap();
        walk.map(|oid| {
            let commit = git.find_commit(oid.unwrap()).unwrap();
            let key = commit.message().unwrap().trim_end().to_owned();
            (
                key,
                (commit.id(), commit.parent_ids().collect(), commit.tree_id()),
            )
        })
        .collect()
    }

    /// Match source and exported commits by message and check that trees
    /// (except where the source has a skipped symlink) and parent
    /// structure are identical.
    fn assert_same_shape(
        src: &git2::Repository,
        src_tip: Oid,
        dst: &git2::Repository,
        dst_tip: Oid,
    ) {
        let a = history(src, src_tip);
        let b = history(dst, dst_tip);
        assert_eq!(a.len(), b.len(), "commit count");
        let translate: HashMap<Oid, Oid> = a
            .iter()
            .map(|(message, (oid, _, _))| (*oid, b[message].0))
            .collect();
        for (message, (oid, parents, tree)) in &a {
            let (_, dst_parents, dst_tree) = &b[message];
            let has_link = src
                .find_tree(*tree)
                .unwrap()
                .iter()
                .any(|e| e.filemode() == i32::from(git2::FileMode::Link));
            if !has_link {
                assert_eq!(tree, dst_tree, "tree of {oid}");
            }
            let expected: Vec<Oid> = parents.iter().map(|p| translate[p]).collect();
            assert_eq!(&expected, dst_parents, "parents of {oid}");
        }
    }

    #[test]
    fn import_builds_snapshot_graph() {
        isolate_wt_home();
        let s = sample();
        let dest = tempfile::tempdir().unwrap();
        let report = import_all(s.fx.path(), dest.path());

        assert_eq!(
            report.branches,
            vec!["feature".to_owned(), "main".to_owned()]
        );
        assert_eq!(report.snapshots, 5);
        assert_eq!(report.tags, 2);
        assert_eq!(report.checked_out.as_deref(), Some("main"));

        let repo = Repository::open(dest.path()).unwrap();
        let tree = repo.root_tree().unwrap();
        assert_eq!(repo.current_branch(&tree.name).unwrap(), "main");
        assert_eq!(
            repo.branch_tip(&tree.name, "main").unwrap(),
            Some(snapshot_id_for_commit(s.m))
        );
        assert_eq!(
            repo.branch_tip(&tree.name, "feature").unwrap(),
            Some(snapshot_id_for_commit(s.f2))
        );

        let merge = repo.get_snapshot(&snapshot_id_for_commit(s.m)).unwrap();
        assert_eq!(
            merge.parents,
            vec![snapshot_id_for_commit(s.c2), snapshot_id_for_commit(s.f2)]
        );
        assert_eq!(merge.author, "Ada Lovelace <ada@example.com>");
        assert_eq!(merge.message, "Merge feature");
        assert_eq!(merge.created_at.timestamp(), 1_600_000_200);
        assert_eq!(merge.branch, "main");
        // f1 is only on feature.
        let f1 = repo.get_snapshot(&snapshot_id_for_commit(s.f1)).unwrap();
        assert_eq!(f1.branch, "feature");
        assert_eq!(f1.message, "Add util\n\nWith a body.");
        let c1 = repo.get_snapshot(&snapshot_id_for_commit(s.c1)).unwrap();
        assert!(c1.parents.is_empty());
        assert_eq!(c1.branch, "main");

        let manifest = repo.manifest_of(&merge).unwrap();
        let paths: Vec<_> = manifest.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".gitignore",
                "README.md",
                "scripts/build.sh",
                "src/lib/util/mod.rs",
                "src/main.rs"
            ]
        );
        assert!(manifest.get("scripts/build.sh").unwrap().executable);
        assert!(!manifest.get("README.md").unwrap().executable);
        // The symlink on feature is skipped.
        let f2 = repo.get_snapshot(&snapshot_id_for_commit(s.f2)).unwrap();
        assert!(repo
            .manifest_of(&f2)
            .unwrap()
            .get("link-to-readme")
            .is_none());

        // Working directory holds main's files.
        assert_eq!(
            fs::read_to_string(dest.path().join("src/lib/util/mod.rs")).unwrap(),
            "pub fn u() -> u8 { 1 }\n"
        );
        assert!(repo.status(&tree.name).unwrap().is_clean());

        // Tags.
        let tags = repo.tags(&tree.name).unwrap();
        let v01 = tags.iter().find(|t| t.name == "v0.1").unwrap();
        assert_eq!(v01.target, snapshot_id_for_commit(s.c2));
        assert_eq!(v01.message, None);
        let v10 = tags.iter().find(|t| t.name == "v1.0").unwrap();
        assert_eq!(v10.target, snapshot_id_for_commit(s.m));
        assert_eq!(v10.message.as_deref(), Some("Release 1.0"));
        assert_eq!(v10.tagger, "Tagger <tagger@example.com>");
        assert_eq!(v10.created_at.timestamp(), 1_600_000_300);

        // .gitignore -> .wt/ignore
        assert!(repo.is_ignored("build.log").unwrap());
        assert!(repo.is_ignored("target/debug/app").unwrap());

        // Mapping file.
        let map = GitMap::load(&repo.wt_dir()).unwrap();
        assert_eq!(map.len(), 5);
        assert_eq!(
            map.commit_for(&snapshot_id_for_commit(s.m)),
            Some(s.m.to_string().as_str())
        );
        assert_eq!(
            map.snapshot_for(&s.c1.to_string()),
            Some(snapshot_id_for_commit(s.c1).as_str())
        );
    }

    #[test]
    fn reimport_yields_identical_snapshot_ids() {
        isolate_wt_home();
        let s = sample();
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        import_all(s.fx.path(), a.path());
        import_all(s.fx.path(), b.path());
        let ra = Repository::open(a.path()).unwrap();
        let rb = Repository::open(b.path()).unwrap();
        let ta = ra.root_tree().unwrap();
        let tb = rb.root_tree().unwrap();
        let ids = |r: &Repository, t: &str| -> Vec<String> {
            r.log(t, Some("main"), None)
                .unwrap()
                .into_iter()
                .map(|s| s.id)
                .collect()
        };
        assert_eq!(ids(&ra, &ta.name), ids(&rb, &tb.name));
        assert_eq!(
            GitMap::load(&ra.wt_dir())
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            GitMap::load(&rb.wt_dir())
                .unwrap()
                .iter()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn import_selected_branch_only() {
        isolate_wt_home();
        let s = sample();
        let dest = tempfile::tempdir().unwrap();
        let report = import_repository(
            s.fx.path(),
            dest.path(),
            &ImportOptions {
                branches: vec!["feature".to_owned()],
                name: Some("only-feature".to_owned()),
                include_tags: true,
            },
        )
        .unwrap();
        assert_eq!(report.branches, vec!["feature".to_owned()]);
        assert_eq!(report.snapshots, 3);
        assert_eq!(report.checked_out.as_deref(), Some("feature"));
        // Both tags point outside feature's history.
        assert_eq!(report.tags, 0);
        let repo = Repository::open(dest.path()).unwrap();
        assert_eq!(repo.root_tree().unwrap().name, "only-feature");
    }

    #[test]
    fn import_rejects_unknown_branch_before_creating_dest() {
        isolate_wt_home();
        let s = sample();
        let parent = tempfile::tempdir().unwrap();
        let dest = parent.path().join("wt");
        let err = import_repository(
            s.fx.path(),
            &dest,
            &ImportOptions {
                branches: vec!["missing".to_owned()],
                ..ImportOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert!(!dest.exists());
    }

    #[test]
    fn import_rejects_non_repository() {
        let dir = tempfile::tempdir().unwrap();
        let err = import_repository(
            dir.path(),
            &dir.path().join("dest"),
            &ImportOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(err, crate::GitCompatError::GitError(_)));
    }

    #[test]
    fn round_trip_reproduces_trees_and_topology() {
        isolate_wt_home();
        let s = sample();
        let dest = tempfile::tempdir().unwrap();
        import_all(s.fx.path(), dest.path());
        let repo = Repository::open(dest.path()).unwrap();

        let out_dir = tempfile::tempdir().unwrap();
        let out_path = out_dir.path().join("out.git");
        let report = export_repository(
            &repo,
            &out_path,
            &ExportOptions {
                include_tags: true,
                bare: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            report.branches,
            vec!["feature".to_owned(), "main".to_owned()]
        );
        assert_eq!(report.commits, 5);
        assert_eq!(report.tags, 2);

        let out = git2::Repository::open(&out_path).unwrap();
        assert!(out.is_bare());
        let main = out.refname_to_id("refs/heads/main").unwrap();
        let feature = out.refname_to_id("refs/heads/feature").unwrap();
        assert_same_shape(&s.fx.repo, s.m, &out, main);
        assert_same_shape(&s.fx.repo, s.f2, &out, feature);

        // Executable bit preserved.
        let tree = out.find_commit(main).unwrap().tree().unwrap();
        let script = tree.get_path(Path::new("scripts/build.sh")).unwrap();
        assert_eq!(script.filemode(), i32::from(git2::FileMode::BlobExecutable));

        // Tags: lightweight stays lightweight, annotated keeps its message.
        let v01 = out.find_reference("refs/tags/v0.1").unwrap();
        assert!(v01.peel_to_tag().is_err());
        assert_eq!(
            v01.peel_to_commit().unwrap().tree_id(),
            s.fx.repo.find_commit(s.c2).unwrap().tree_id()
        );
        let v10 = out
            .find_reference("refs/tags/v1.0")
            .unwrap()
            .peel_to_tag()
            .unwrap();
        assert_eq!(v10.message(), Some("Release 1.0\n"));
        assert_eq!(v10.target_id(), main);

        // Exporting the same worktree elsewhere gives identical SHAs.
        let again_path = out_dir.path().join("again.git");
        export_repository(
            &repo,
            &again_path,
            &ExportOptions {
                include_tags: true,
                bare: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
        let again = git2::Repository::open(&again_path).unwrap();
        assert_eq!(again.refname_to_id("refs/heads/main").unwrap(), main);
        assert_eq!(again.refname_to_id("refs/tags/v1.0").unwrap(), v10.id());
    }

    #[test]
    fn export_into_source_reuses_original_commits_and_is_incremental() {
        isolate_wt_home();
        let s = sample();
        let dest = tempfile::tempdir().unwrap();
        import_all(s.fx.path(), dest.path());
        let repo = Repository::open(dest.path()).unwrap();
        let tree = repo.root_tree().unwrap();
        // Give the source a clean, checked-out working tree.
        s.fx.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();

        // Nothing new: the imported commits already exist in the source repo.
        let report = export_repository(&repo, s.fx.path(), &ExportOptions::default()).unwrap();
        assert_eq!(report.commits, 0);
        assert_eq!(s.fx.repo.refname_to_id("refs/heads/main").unwrap(), s.m);

        // A new snapshot becomes exactly one new commit on top of the original.
        fs::write(dest.path().join("NEW.md"), "new file\n").unwrap();
        let snap = repo
            .snapshot(
                &tree.name,
                SnapshotOptions {
                    message: "Add NEW.md".to_owned(),
                    author: Some("Grace Hopper <grace@example.com>".to_owned()),
                    ..SnapshotOptions::default()
                },
            )
            .unwrap();
        let report = export_repository(&repo, s.fx.path(), &ExportOptions::default()).unwrap();
        assert_eq!(report.commits, 1);
        let tip = s.fx.repo.refname_to_id("refs/heads/main").unwrap();
        let commit = s.fx.repo.find_commit(tip).unwrap();
        assert_eq!(commit.parent_ids().collect::<Vec<_>>(), vec![s.m]);
        assert_eq!(commit.message(), Some("Add NEW.md\n"));
        assert_eq!(commit.author().email(), Some("grace@example.com"));
        assert!(commit.tree().unwrap().get_name("NEW.md").is_some());
        let map = GitMap::load(&repo.wt_dir()).unwrap();
        assert_eq!(map.commit_for(&snap.id), Some(tip.to_string().as_str()));

        // The source's (clean) working tree followed HEAD.
        assert_eq!(
            fs::read_to_string(s.fx.path().join("NEW.md")).unwrap(),
            "new file\n"
        );

        // Re-running is a no-op.
        let report = export_repository(&repo, s.fx.path(), &ExportOptions::default()).unwrap();
        assert_eq!(report.commits, 0);
        assert_eq!(s.fx.repo.refname_to_id("refs/heads/main").unwrap(), tip);
    }

    #[test]
    fn export_native_worktree_is_deterministic_and_incremental() {
        isolate_wt_home();
        let wt = tempfile::tempdir().unwrap();
        let repo = Repository::init(wt.path(), worktree_sdk::InitOptions::default()).unwrap();
        let tree = repo.root_tree().unwrap();
        let opts = |message: &str| SnapshotOptions {
            message: message.to_owned(),
            author: Some("Linus <linus@example.com>".to_owned()),
            ..SnapshotOptions::default()
        };
        fs::create_dir_all(wt.path().join("src/nested")).unwrap();
        fs::write(wt.path().join("src/nested/a.txt"), "a\n").unwrap();
        repo.snapshot(&tree.name, opts("first")).unwrap();
        repo.switch_branch(
            &tree.name,
            "topic",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
        fs::write(wt.path().join("b.txt"), "b\n").unwrap();
        repo.snapshot(&tree.name, opts("topic work")).unwrap();
        repo.create_tag(&tree.name, "t1", None, Some("annotated"))
            .unwrap();

        let out_dir = tempfile::tempdir().unwrap();
        let first = out_dir.path().join("first");
        let report = export_repository(
            &repo,
            &first,
            &ExportOptions {
                include_tags: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.commits, 2);
        assert_eq!(report.tags, 1);
        let git = git2::Repository::open(&first).unwrap();
        assert!(!git.is_bare());
        let topic = git.refname_to_id("refs/heads/topic").unwrap();
        let commit = git.find_commit(topic).unwrap();
        assert_eq!(commit.parent_count(), 1);
        assert!(commit
            .tree()
            .unwrap()
            .get_path(Path::new("src/nested/a.txt"))
            .is_ok());
        // HEAD follows the worktree's current branch and files are checked out.
        assert_eq!(
            git.find_reference("HEAD").unwrap().symbolic_target(),
            Some("refs/heads/topic")
        );
        // (Line endings follow the developer's core.autocrlf in a new repo.)
        assert_eq!(
            fs::read_to_string(first.join("b.txt")).unwrap().trim_end(),
            "b"
        );

        // Incremental: nothing new to write.
        let report = export_repository(&repo, &first, &ExportOptions::default()).unwrap();
        assert_eq!(report.commits, 0);

        // Deterministic: a second target receives identical SHAs.
        let second = out_dir.path().join("second.git");
        export_repository(
            &repo,
            &second,
            &ExportOptions {
                bare: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
        let git2nd = git2::Repository::open(&second).unwrap();
        assert_eq!(git2nd.refname_to_id("refs/heads/topic").unwrap(), topic);

        // Only the requested branch is exported.
        let third = out_dir.path().join("third.git");
        let report = export_repository(
            &repo,
            &third,
            &ExportOptions {
                branches: vec!["main".to_owned()],
                bare: true,
                ..ExportOptions::default()
            },
        )
        .unwrap();
        assert_eq!(report.branches, vec!["main".to_owned()]);
        let git3 = git2::Repository::open(&third).unwrap();
        assert!(git3.find_reference("refs/heads/topic").is_err());

        let err = export_repository(
            &repo,
            &third,
            &ExportOptions {
                branches: vec!["nope".to_owned()],
                ..ExportOptions::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("nope"));
    }
}
