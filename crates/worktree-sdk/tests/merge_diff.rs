//! Three-way merges, conflict handling and diffs. Most of these exercise
//! `crate::merge` / `crate::diff` and therefore need the sdk-merge-diff
//! workstream; name-only diffs run standalone.

mod common;

use common::Fixture;
use worktree_sdk::{
    ChangeKind, ConflictKind, DiffOptions, MergeOptions, MergeOutcome, SdkError, SwitchOptions,
};

/// main and feature both change `shared.txt` (or not) from a common base.
fn diverge(fx: &Fixture, main_edit: &str, feature_edit: &str) {
    fx.write("shared.txt", "1\n2\n3\n4\n5\n");
    fx.write("main-only.txt", "m\n");
    fx.snap("base");
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
    fx.write("shared.txt", feature_edit);
    fx.write("feature-only.txt", "f\n");
    fx.snap("feature edit");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    fx.write("shared.txt", main_edit);
    fx.snap("main edit");
}

#[test]
fn name_only_diffs_need_no_content() {
    let fx = Fixture::new();
    fx.write("a.txt", "1");
    fx.write("b.txt", "1");
    let s1 = fx.snap("one");
    fx.write("a.txt", "2");
    fx.remove("b.txt");
    fx.write("c/c.txt", "3");
    let name_only = DiffOptions {
        name_only: true,
        ..DiffOptions::default()
    };
    let diffs = fx.repo.diff_working(&fx.tree, None, &name_only).unwrap();
    let summary: Vec<_> = diffs.iter().map(|d| (d.path.as_str(), d.kind)).collect();
    assert_eq!(
        summary,
        [
            ("a.txt", ChangeKind::Modified),
            ("b.txt", ChangeKind::Deleted),
            ("c/c.txt", ChangeKind::Added)
        ]
    );
    assert!(diffs.iter().all(|d| d.patch.is_none()));
    assert!(diffs[1].new_hash.is_none() && diffs[1].old_hash.is_some());

    let s2 = fx.snap("two");
    let between = fx
        .repo
        .diff_revs(&fx.tree, &s1.id, &s2.id, &name_only)
        .unwrap();
    assert_eq!(between.len(), 3);
    let scoped = DiffOptions {
        paths: vec!["c".into()],
        ..name_only.clone()
    };
    let only_c = fx
        .repo
        .diff_revs(&fx.tree, &s1.id, "HEAD", &scoped)
        .unwrap();
    assert_eq!(only_c.len(), 1);
    assert_eq!(only_c[0].path, "c/c.txt");
    assert!(fx
        .repo
        .diff_working(&fx.tree, None, &name_only)
        .unwrap()
        .is_empty());
}

#[test]
fn diffs_include_patches_and_stats() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\n");
    fx.snap("one");
    fx.write("a.txt", "one\nTWO\nthree\n");
    fx.write("bin.dat", "\0\0binary");
    let diffs = fx
        .repo
        .diff_working(&fx.tree, None, &DiffOptions::default())
        .unwrap();
    let a = diffs.iter().find(|d| d.path == "a.txt").unwrap();
    assert_eq!((a.added, a.removed), (2, 1));
    let patch = a.patch.as_deref().unwrap();
    assert!(patch.contains("+TWO") && patch.contains("-two"), "{patch}");
    let bin = diffs.iter().find(|d| d.path == "bin.dat").unwrap();
    assert!(bin.binary && bin.patch.is_none());
}

#[test]
fn clean_three_way_merge_creates_two_parent_snapshot() {
    let fx = Fixture::new();
    diverge(&fx, "ONE\n2\n3\n4\n5\n", "1\n2\n3\n4\nFIVE\n");
    let main_tip = fx.repo.branch_tip(&fx.tree, "main").unwrap().unwrap();
    let feature_tip = fx.repo.branch_tip(&fx.tree, "feature").unwrap().unwrap();
    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Merged { snapshot } = outcome else {
        panic!("expected merge, got {outcome:?}")
    };
    assert_eq!(snapshot.parents, vec![main_tip, feature_tip]);
    assert_eq!(fx.read("shared.txt"), "ONE\n2\n3\n4\nFIVE\n");
    assert_eq!(fx.read("feature-only.txt"), "f\n");
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert_eq!(
        fx.repo.reflog(&fx.tree, Some(1)).unwrap()[0].action,
        "merge"
    );
}

#[test]
fn no_ff_merge_records_merge_snapshot() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n");
    fx.snap("base");
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
    fx.write("b.txt", "2\n");
    fx.snap("feature");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    let opts = MergeOptions {
        no_fast_forward: true,
        message: Some("merge it".into()),
        ..MergeOptions::default()
    };
    let MergeOutcome::Merged { snapshot } = fx.repo.merge(&fx.tree, "feature", opts).unwrap()
    else {
        panic!("expected a merge snapshot")
    };
    assert!(snapshot.is_merge());
    assert_eq!(snapshot.message, "merge it");
    assert_eq!(fx.read("b.txt"), "2\n");
}

#[test]
fn conflicted_merge_continue_flow() {
    let fx = Fixture::new();
    diverge(&fx, "1\nMAIN\n3\n4\n5\n", "1\nFEATURE\n3\n4\n5\n");
    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Conflicts { conflicts } = outcome else {
        panic!("expected conflicts, got {outcome:?}")
    };
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "shared.txt");
    assert_eq!(conflicts[0].kind, ConflictKind::Content);
    let text = fx.read("shared.txt");
    assert!(
        text.contains("<<<<<<<") && text.contains(">>>>>>>"),
        "{text}"
    );
    assert_eq!(fx.read("feature-only.txt"), "f\n");
    assert!(fx
        .repo
        .wt_dir()
        .join("conflicts")
        .join("demo.json")
        .is_file());

    let state = fx.repo.merge_state(&fx.tree).unwrap().unwrap();
    assert_eq!(state.source_branch, "feature");
    assert!(fx.repo.status(&fx.tree).unwrap().merge.is_some());
    assert!(matches!(
        fx.repo.merge(&fx.tree, "feature", MergeOptions::default()),
        Err(SdkError::MergeInProgress)
    ));
    assert!(matches!(
        fx.repo.continue_merge(&fx.tree, None),
        Err(SdkError::MergeConflict(_))
    ));
    assert!(matches!(
        fx.repo.snapshot(&fx.tree, common::opts("x")),
        Err(SdkError::MergeInProgress)
    ));
    assert!(fx
        .repo
        .switch_branch(&fx.tree, "feature", SwitchOptions::default())
        .is_err());

    fx.write("shared.txt", "1\nBOTH\n3\n4\n5\n");
    let merged = fx.repo.continue_merge(&fx.tree, Some("resolved")).unwrap();
    assert!(merged.is_merge());
    assert_eq!(merged.parents[1], state.source_tip);
    assert_eq!(merged.message, "resolved");
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
    assert!(!fx
        .repo
        .wt_dir()
        .join("conflicts")
        .join("demo.json")
        .exists());
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

#[test]
fn snapshot_completes_resolved_merge() {
    let fx = Fixture::new();
    diverge(&fx, "1\nMAIN\n3\n4\n5\n", "1\nFEATURE\n3\n4\n5\n");
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    fx.write("shared.txt", "resolved\n");
    let snap = fx.snap("");
    assert_eq!(snap.parents.len(), 2);
    assert!(snap.message.starts_with("Merge feature into main"));
}

#[test]
fn abort_merge_restores_pre_merge_state() {
    let fx = Fixture::new();
    diverge(&fx, "1\nMAIN\n3\n4\n5\n", "1\nFEATURE\n3\n4\n5\n");
    let before = fx.read("shared.txt");
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    fx.write("user-notes.txt", "keep me");
    fx.repo.abort_merge(&fx.tree).unwrap();
    assert_eq!(fx.read("shared.txt"), before);
    assert!(
        !fx.exists("feature-only.txt"),
        "files brought in by the merge are removed"
    );
    assert_eq!(fx.read("user-notes.txt"), "keep me");
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
    assert!(matches!(
        fx.repo.abort_merge(&fx.tree),
        Err(SdkError::NoMergeInProgress)
    ));
}

#[test]
fn merge_base_of_diverged_branches() {
    let fx = Fixture::new();
    diverge(&fx, "a\n", "b\n");
    let base = fx.repo.resolve(&fx.tree, "main~1").unwrap();
    let main = fx.repo.branch_tip(&fx.tree, "main").unwrap().unwrap();
    let feature = fx.repo.branch_tip(&fx.tree, "feature").unwrap().unwrap();
    assert_eq!(fx.repo.merge_base(&main, &feature).unwrap(), Some(base.id));
}

#[test]
fn merge_state_helpers_without_merge() {
    let fx = Fixture::new();
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
    assert!(matches!(
        fx.repo.abort_merge(&fx.tree),
        Err(SdkError::NoMergeInProgress)
    ));
    assert!(matches!(
        fx.repo.continue_merge(&fx.tree, None),
        Err(SdkError::NoMergeInProgress)
    ));
}
