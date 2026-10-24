//! Branches, switching, checkout, revert and restore.

mod common;

use common::{opts, Fixture};
use worktree_sdk::{SdkError, SwitchOptions};

#[test]
fn create_and_list_branches() {
    let fx = Fixture::new();
    // Branching off an empty branch yields an empty branch.
    let empty = fx.repo.create_branch(&fx.tree, "early", None).unwrap();
    assert_eq!(empty.tip, None);
    fx.write("a.txt", "1");
    let s1 = fx.snap("one");
    fx.write("a.txt", "2");
    let s2 = fx.snap("two");

    let dev = fx.repo.create_branch(&fx.tree, "dev", None).unwrap();
    assert_eq!(dev.tip, Some(s2.id.clone()));
    assert!(!dev.is_current);
    let old = fx
        .repo
        .create_branch(&fx.tree, "feature/old", Some("HEAD~1"))
        .unwrap();
    assert_eq!(old.tip, Some(s1.id.clone()));

    assert!(matches!(
        fx.repo.create_branch(&fx.tree, "dev", None),
        Err(SdkError::BranchExists(_))
    ));
    assert!(fx.repo.create_branch(&fx.tree, "bad name", None).is_err());
    assert!(fx.repo.create_branch(&fx.tree, "HEAD", None).is_err());
    // Layout conflicts with existing refs are refused.
    assert!(fx.repo.create_branch(&fx.tree, "dev/sub", None).is_err());
    assert!(fx.repo.create_branch(&fx.tree, "feature", None).is_err());
    assert!(fx
        .repo
        .create_branch(&fx.tree, "x", Some("no-such-rev"))
        .is_err());

    let names: Vec<_> = fx
        .repo
        .branches(&fx.tree, false)
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["dev", "early", "feature/old", "main"]);
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "main");
    assert!(matches!(
        fx.repo.branch_tip(&fx.tree, "nope"),
        Err(SdkError::BranchNotFound(_))
    ));
}

#[test]
fn switch_rewrites_working_directory() {
    let fx = Fixture::new();
    fx.write("shared.txt", "base\n");
    fx.write("dir/only-main.txt", "main\n");
    let base = fx.snap("base");

    fx.repo
        .switch_branch(
            &fx.tree,
            "feature",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "feature");
    fx.write("shared.txt", "feature\n");
    fx.remove("dir/only-main.txt");
    fx.write("new/feature.txt", "f\n");
    let feat = fx.snap("feature work");
    assert_eq!(feat.branch, "feature");
    assert_eq!(feat.parents, vec![base.id.clone()]);

    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    assert_eq!(fx.read("shared.txt"), "base\n");
    assert_eq!(fx.read("dir/only-main.txt"), "main\n");
    assert!(!fx.exists("new/feature.txt"));
    assert!(!fx.exists("new"), "emptied directories are pruned");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    fx.repo
        .switch_branch(&fx.tree, "feature", SwitchOptions::default())
        .unwrap();
    assert_eq!(fx.read("shared.txt"), "feature\n");
    assert!(!fx.exists("dir/only-main.txt"));
    assert_eq!(fx.read("new/feature.txt"), "f\n");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    let actions: Vec<_> = fx
        .repo
        .reflog(&fx.tree, Some(3))
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["switch", "switch", "snapshot"]);
    assert!(matches!(
        fx.repo
            .switch_branch(&fx.tree, "missing", SwitchOptions::default()),
        Err(SdkError::BranchNotFound(_))
    ));
}

#[test]
fn switch_refuses_dirty_tree_unless_forced() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    fx.snap("one");
    fx.repo.create_branch(&fx.tree, "other", None).unwrap();
    fx.repo
        .switch_branch(&fx.tree, "other", SwitchOptions::default())
        .unwrap();
    fx.write("a.txt", "2");
    fx.snap("two on other");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();

    fx.write("a.txt", "dirty");
    fx.write("untracked.txt", "u");
    let err = fx
        .repo
        .switch_branch(&fx.tree, "other", SwitchOptions::default())
        .unwrap_err();
    assert!(matches!(err, SdkError::DirtyWorkingTree(2)), "{err}");
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "main");

    fx.repo
        .switch_branch(
            &fx.tree,
            "other",
            SwitchOptions {
                create: false,
                force: true,
            },
        )
        .unwrap();
    assert_eq!(fx.read("a.txt"), "2");
    assert_eq!(
        fx.read("untracked.txt"),
        "u",
        "untracked files survive a forced switch"
    );
}

#[test]
fn switch_between_same_tip_carries_changes() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    fx.snap("one");
    fx.write("a.txt", "work in progress");
    fx.repo
        .switch_branch(
            &fx.tree,
            "wip",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(fx.read("a.txt"), "work in progress");
    let s = fx.snap("wip");
    assert_eq!(s.branch, "wip");
    assert_eq!(
        fx.repo.branch_tip(&fx.tree, "main").unwrap().as_deref(),
        s.parents.first().map(String::as_str)
    );
}

#[test]
fn archive_and_restore_branches() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    let s = fx.snap("one");
    fx.repo.create_branch(&fx.tree, "old", None).unwrap();

    assert!(
        fx.repo.archive_branch(&fx.tree, "main").is_err(),
        "current/default branch"
    );
    fx.repo.archive_branch(&fx.tree, "old").unwrap();
    assert!(
        fx.repo.archive_branch(&fx.tree, "old").is_err(),
        "already archived"
    );
    let visible: Vec<_> = fx
        .repo
        .branches(&fx.tree, false)
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(visible, ["main"]);
    let all = fx.repo.branches(&fx.tree, true).unwrap();
    assert!(all.iter().any(|b| b.name == "old" && b.archived));
    // History remains reachable and the name stays reserved.
    assert_eq!(fx.repo.resolve(&fx.tree, "old").unwrap().id, s.id);
    assert!(matches!(
        fx.repo.create_branch(&fx.tree, "old", None),
        Err(SdkError::BranchExists(_))
    ));
    assert!(fx
        .repo
        .switch_branch(&fx.tree, "old", SwitchOptions::default())
        .is_err());

    fx.repo.restore_branch(&fx.tree, "old").unwrap();
    assert!(
        fx.repo.restore_branch(&fx.tree, "old").is_err(),
        "not archived"
    );
    assert!(matches!(
        fx.repo.restore_branch(&fx.tree, "ghost"),
        Err(SdkError::BranchNotFound(_))
    ));
    fx.repo
        .switch_branch(&fx.tree, "old", SwitchOptions::default())
        .unwrap();

    let actions: Vec<_> = fx
        .repo
        .reflog(&fx.tree, Some(3))
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["switch", "branch.restore", "branch.archive"]);

    // The default branch can't be archived even when not current.
    assert!(fx.repo.archive_branch(&fx.tree, "main").is_err());
}

#[test]
fn checkout_cleans_tracked_files_only() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    let s1 = fx.snap("one");
    fx.write("b/b.txt", "2");
    fx.write("a.txt", "1b");
    fx.snap("two");
    fx.write("build.log", "ignored");
    fx.repo.add_ignore_pattern("*.log").unwrap();

    fx.repo.checkout(&fx.tree, &s1.id, false).unwrap();
    assert_eq!(fx.read("a.txt"), "1");
    assert!(!fx.exists("b"), "tracked file removed, empty dir pruned");
    assert_eq!(fx.read("build.log"), "ignored", "ignored files untouched");
    // The branch did not move.
    assert_ne!(
        fx.repo.branch_tip(&fx.tree, "main").unwrap(),
        Some(s1.id.clone())
    );

    // Back to HEAD; then dirty checkouts are refused.
    fx.repo.checkout(&fx.tree, "HEAD", true).unwrap();
    assert_eq!(fx.read("b/b.txt"), "2");
    fx.write("a.txt", "dirty");
    assert!(matches!(
        fx.repo.checkout(&fx.tree, &s1.id, false),
        Err(SdkError::DirtyWorkingTree(1))
    ));
    fx.write("extra.txt", "untracked");
    fx.repo.checkout(&fx.tree, &s1.id, true).unwrap();
    assert_eq!(fx.read("a.txt"), "1");
    assert_eq!(fx.read("extra.txt"), "untracked");
}

#[test]
fn revert_creates_inverse_snapshot() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.write("b.txt", "keep\n");
    fx.snap("base");
    fx.write("a.txt", "two\n");
    fx.write("c.txt", "added\n");
    fx.remove("b.txt");
    let change = fx.snap("change");
    fx.write("d.txt", "later\n");
    fx.snap("later");

    let revert = fx.repo.revert(&fx.tree, &change.id[..8]).unwrap();
    assert!(revert.message.starts_with("Revert \"change\""));
    assert!(revert.message.contains(&change.id));
    assert_eq!(revert.parents.len(), 1);
    assert_eq!(fx.read("a.txt"), "one\n");
    assert_eq!(fx.read("b.txt"), "keep\n");
    assert!(!fx.exists("c.txt"));
    assert_eq!(fx.read("d.txt"), "later\n", "later changes are kept");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert_eq!(
        common::head_paths(&fx.repo, &fx.tree),
        ["a.txt", "b.txt", "d.txt"]
    );
    assert_eq!(
        fx.repo.reflog(&fx.tree, Some(1)).unwrap()[0].action,
        "revert"
    );

    // Reverting again is a no-op.
    assert!(matches!(
        fx.repo.revert(&fx.tree, &change.id),
        Err(SdkError::NoChanges)
    ));
    // Dirty trees are refused.
    fx.write("a.txt", "dirty");
    assert!(matches!(
        fx.repo.revert(&fx.tree, "HEAD"),
        Err(SdkError::DirtyWorkingTree(_))
    ));
}

#[test]
fn revert_of_root_snapshot_empties_its_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.snap("root");
    fx.repo.revert(&fx.tree, "HEAD").unwrap();
    assert!(!fx.exists("a.txt"));
    assert!(common::head_paths(&fx.repo, &fx.tree).is_empty());
}

#[test]
#[ignore = "integration: needs sdk-merge-diff"]
fn revert_with_later_edits_merges_text() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n2\n3\n4\n5\n");
    fx.snap("base");
    fx.write("a.txt", "1\nTWO\n3\n4\n5\n");
    let change = fx.snap("change line 2");
    fx.write("a.txt", "1\nTWO\n3\n4\nFIVE\n");
    fx.snap("change line 5");
    fx.repo.revert(&fx.tree, &change.id).unwrap();
    assert_eq!(fx.read("a.txt"), "1\n2\n3\n4\nFIVE\n");
}

#[test]
fn restore_paths_and_revisions() {
    let fx = Fixture::new();
    fx.write("a.txt", "a1");
    fx.write("dir/b.txt", "b1");
    let s1 = fx.snap("one");
    fx.write("a.txt", "a2");
    fx.write("dir/b.txt", "b2");
    fx.snap("two");

    fx.write("a.txt", "dirty");
    fx.write("dir/b.txt", "dirty");
    fx.write("dir/new.txt", "new");
    let restored = fx
        .repo
        .restore(&fx.tree, &["a.txt".to_string()], None)
        .unwrap();
    assert_eq!(restored, ["a.txt"]);
    assert_eq!(fx.read("a.txt"), "a2");
    assert_eq!(fx.read("dir/b.txt"), "dirty");

    let restored = fx
        .repo
        .restore(&fx.tree, &["dir/".to_string()], None)
        .unwrap();
    assert_eq!(restored, ["dir/b.txt", "dir/new.txt"]);
    assert_eq!(fx.read("dir/b.txt"), "b2");
    assert!(!fx.exists("dir/new.txt"));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    let restored = fx.repo.restore(&fx.tree, &[], Some(&s1.id)).unwrap();
    assert_eq!(restored, ["a.txt", "dir/b.txt"]);
    assert_eq!(fx.read("a.txt"), "a1");
    assert_eq!(
        fx.repo.restore(&fx.tree, &[], Some(&s1.id)).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        fx.repo.reflog(&fx.tree, Some(1)).unwrap()[0].action,
        "restore"
    );
}

#[test]
fn fast_forward_and_up_to_date_merges() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    let s1 = fx.snap("one");
    fx.repo
        .switch_branch(
            &fx.tree,
            "feature",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    fx.write("b.txt", "2");
    let s2 = fx.snap("two");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    assert!(!fx.exists("b.txt"));

    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", Default::default())
        .unwrap();
    assert_eq!(
        outcome,
        worktree_sdk::MergeOutcome::FastForward { to: s2.id.clone() }
    );
    assert_eq!(fx.read("b.txt"), "2");
    assert_eq!(
        fx.repo.branch_tip(&fx.tree, "main").unwrap(),
        Some(s2.id.clone())
    );
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert_eq!(
        fx.repo.reflog(&fx.tree, Some(1)).unwrap()[0].action,
        "fast-forward"
    );

    assert_eq!(
        fx.repo
            .merge(&fx.tree, "feature", Default::default())
            .unwrap(),
        worktree_sdk::MergeOutcome::UpToDate
    );
    assert_eq!(
        fx.repo.merge(&fx.tree, &s1.id, Default::default()).unwrap(),
        worktree_sdk::MergeOutcome::UpToDate
    );

    fx.write("a.txt", "dirty");
    assert!(matches!(
        fx.repo.merge(&fx.tree, "feature", Default::default()),
        Err(SdkError::DirtyWorkingTree(_))
    ));
}

#[test]
fn merge_into_empty_branch_fast_forwards() {
    let fx = Fixture::new();
    fx.repo.create_branch(&fx.tree, "empty", None).unwrap();
    fx.write("a.txt", "1");
    let s1 = fx.snap("one");
    fx.repo
        .switch_branch(&fx.tree, "empty", SwitchOptions::default())
        .unwrap();
    assert!(!fx.exists("a.txt"));
    let outcome = fx.repo.merge(&fx.tree, "main", Default::default()).unwrap();
    assert_eq!(
        outcome,
        worktree_sdk::MergeOutcome::FastForward { to: s1.id }
    );
    assert_eq!(fx.read("a.txt"), "1");
}

#[test]
fn concurrent_snapshots_serialize_on_the_tree_lock() {
    let fx = Fixture::new();
    fx.write("seed.txt", "0");
    fx.snap("seed");
    let root = fx.root();
    let handles: Vec<_> = (0..6)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                let repo = worktree_sdk::Repository::open(&root).unwrap();
                common::write_file(&root.join(format!("t{i}.txt")), "x");
                match repo.snapshot("demo", opts(&format!("thread {i}"))) {
                    Ok(_) | Err(SdkError::NoChanges) => {}
                    Err(e) => panic!("thread {i}: {e}"),
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    // History is linear: every snapshot's parent is the previous tip.
    let log = fx.repo.log(&fx.tree, None, None).unwrap();
    for pair in log.windows(2) {
        assert_eq!(pair[0].parents, vec![pair[1].id.clone()]);
    }
    let _ = fx.repo.snapshot(&fx.tree, opts("final"));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert_eq!(common::head_paths(&fx.repo, &fx.tree).len(), 7);
}
