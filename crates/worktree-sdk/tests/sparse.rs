//! Sparse checkout working-tree semantics (ENGINEERING_PLAN §15.4).

mod common;

use common::Fixture;
use worktree_sdk::repo::storage::FsckOptions;
use worktree_sdk::{ChangeKind, MergeOptions, MergeOutcome, SdkError, SwitchOptions};

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn seeded() -> Fixture {
    let f = Fixture::new();
    f.write("src/a.rs", "fn a() {}\n");
    f.write("src/b.rs", "fn b() {}\n");
    f.write("docs/guide.md", "# guide\n");
    f.write("README.md", "# readme\n");
    f.snap("initial");
    f
}

#[test]
fn files_outside_the_set_are_not_checked_out_reported_or_dropped() {
    let f = seeded();
    let t = f.tree.clone();
    assert!(!f.repo.is_sparse(&t).unwrap());
    assert!(f.repo.sparse_status(&t).unwrap().is_none());

    let report = f.repo.sparse_set(&t, &strings(&["src"])).unwrap();
    assert!(report.enabled);
    assert_eq!(report.patterns, strings(&["/src"]));
    assert_eq!((report.written, report.removed), (0, 2));
    assert!(f.exists("src/a.rs") && f.exists("src/b.rs"));
    assert!(!f.exists("docs/guide.md") && !f.exists("README.md"));
    assert!(
        f.repo.status(&t).unwrap().is_clean(),
        "missing files outside the set are not deletions"
    );
    let status = f.repo.sparse_status(&t).unwrap().unwrap();
    assert_eq!((status.files_checked_out, status.files_total), (2, 4));
    assert_eq!(status.partial_remote, None);

    // A stray file outside the set is invisible.
    f.write("docs/stray.md", "not tracked\n");
    assert!(f.repo.status(&t).unwrap().is_clean());

    // Snapshots keep the head entries of paths outside the set.
    f.write("src/a.rs", "fn a() { 1 }\n");
    f.write("src/c.rs", "fn c() {}\n");
    let changes = f.repo.status(&t).unwrap().changes;
    assert_eq!(changes.len(), 2, "{changes:?}");
    let before = f
        .repo
        .manifest_of(&f.repo.resolve(&t, "HEAD").unwrap())
        .unwrap();
    let s2 = f.snap("edit inside the set");
    let after = f.repo.manifest_of(&s2).unwrap();
    for p in ["README.md", "docs/guide.md"] {
        assert_eq!(after.get(p), before.get(p), "{p} is kept unchanged");
    }
    assert!(after.get("docs/stray.md").is_none());
    assert!(after.get("src/c.rs").is_some());
    assert!(matches!(
        f.repo.snapshot(&t, common::opts("nothing")),
        Err(SdkError::NoChanges)
    ));

    // add brings files back; untracked strays stay untouched.
    f.remove("docs/stray.md");
    let report = f.repo.sparse_add(&t, &strings(&["docs", "src"])).unwrap();
    assert_eq!(report.patterns, strings(&["/src", "/docs"]));
    assert_eq!(report.written, 1);
    assert_eq!(f.read("docs/guide.md"), "# guide\n");
    assert!(!f.exists("README.md"));
    assert!(f.repo.status(&t).unwrap().is_clean());

    // remove: a modified file leaving the set is kept and reported.
    f.write("docs/guide.md", "# edited guide\n");
    let report = f.repo.sparse_remove(&t, &strings(&["docs"])).unwrap();
    assert_eq!(report.kept_modified, strings(&["docs/guide.md"]));
    assert_eq!(report.removed, 0);
    assert!(f.exists("docs/guide.md"));
    f.write("docs/guide.md", "# guide\n");
    let report = f.repo.sparse_reapply(&t).unwrap();
    assert_eq!((report.removed, report.kept_modified.len()), (1, 0));
    assert!(!f.exists("docs/guide.md"));

    // Pattern errors.
    assert!(matches!(
        f.repo.sparse_remove(&t, &strings(&["nope"])),
        Err(SdkError::InvalidArgument(_))
    ));
    assert!(matches!(
        f.repo.sparse_remove(&t, &strings(&["src"])),
        Err(SdkError::InvalidArgument(_))
    ));
    assert!(matches!(
        f.repo.sparse_set(&t, &strings(&["../x"])),
        Err(SdkError::InvalidArgument(_))
    ));

    // Checking out an older snapshot only writes inside the set.
    let s1 = f.repo.resolve(&t, "HEAD~1").unwrap();
    f.repo.checkout(&t, &s1.id, false).unwrap();
    assert_eq!(f.read("src/a.rs"), "fn a() {}\n");
    assert!(!f.exists("src/c.rs"));
    assert!(!f.exists("README.md"));
    f.repo.checkout(&t, &s2.id, true).unwrap();

    // disable writes everything back.
    let report = f.repo.sparse_disable(&t).unwrap();
    assert!(!report.enabled);
    assert_eq!(report.written, 2);
    assert_eq!(f.read("README.md"), "# readme\n");
    assert_eq!(f.read("docs/guide.md"), "# guide\n");
    assert!(f.repo.status(&t).unwrap().is_clean());
    assert!(!f.repo.is_sparse(&t).unwrap());
    assert!(matches!(
        f.repo.sparse_add(&t, &strings(&["docs"])),
        Err(SdkError::InvalidArgument(_))
    ));
    // A local sparse checkout is not a partial clone: fsck is clean.
    let fsck = f.repo.fsck(&FsckOptions::default()).unwrap();
    assert!(fsck.is_ok(), "{fsck:?}");
}

#[test]
fn deleting_inside_the_set_is_still_a_deletion() {
    let f = seeded();
    let t = f.tree.clone();
    f.repo.sparse_set(&t, &strings(&["src"])).unwrap();
    f.remove("src/b.rs");
    let changes = f.repo.status(&t).unwrap().changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "src/b.rs");
    assert_eq!(changes[0].kind, ChangeKind::Deleted);
    let s = f.snap("delete b");
    let m = f.repo.manifest_of(&s).unwrap();
    assert!(m.get("src/b.rs").is_none());
    assert!(m.get("README.md").is_some());
}

#[test]
fn merges_outside_the_set_update_history_and_conflicts_are_checked_out() {
    let f = seeded();
    let t = f.tree.clone();
    f.write("other/x.txt", "one\ntwo\nthree\n");
    f.write("other/y.txt", "base\n");
    f.snap("other files");
    let switch = SwitchOptions {
        create: true,
        force: false,
    };
    f.repo.switch_branch(&t, "feature", switch).unwrap();
    f.write("other/x.txt", "one\ntwo\nthree (feature)\n");
    f.write("other/y.txt", "feature\n");
    f.snap("feature edits");
    f.repo
        .switch_branch(&t, "main", SwitchOptions::default())
        .unwrap();
    f.write("other/x.txt", "one (main)\ntwo\nthree\n");
    f.snap("main edit");

    f.repo.sparse_set(&t, &strings(&["src"])).unwrap();
    assert!(!f.exists("other/x.txt"));
    let outcome = f
        .repo
        .merge(&t, "feature", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Merged { snapshot } = outcome else {
        panic!("expected a clean merge, got {outcome:?}");
    };
    let m = f.repo.manifest_of(&snapshot).unwrap();
    let x = f
        .repo
        .store()
        .read_entry(m.get("other/x.txt").unwrap())
        .unwrap();
    assert_eq!(x, b"one (main)\ntwo\nthree (feature)\n");
    assert_eq!(
        f.repo
            .store()
            .read_entry(m.get("other/y.txt").unwrap())
            .unwrap(),
        b"feature\n"
    );
    assert!(
        !f.exists("other/x.txt"),
        "merged files outside the set stay unwritten"
    );
    assert!(f.repo.status(&t).unwrap().is_clean());

    // A conflict outside the set is written and visible until resolved.
    f.repo
        .switch_branch(
            &t,
            "feature",
            SwitchOptions {
                create: false,
                force: false,
            },
        )
        .unwrap();
    assert!(!f.exists("other/y.txt"));
    // Edit y on feature outside the working tree: through a full checkout.
    f.repo.sparse_disable(&t).unwrap();
    f.write("other/y.txt", "feature again\n");
    f.snap("feature edits y again");
    f.repo
        .switch_branch(&t, "main", SwitchOptions::default())
        .unwrap();
    f.write("other/y.txt", "main\n");
    f.snap("main edits y");
    f.repo.sparse_set(&t, &strings(&["src"])).unwrap();
    let outcome = f
        .repo
        .merge(&t, "feature", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Conflicts { conflicts } = outcome else {
        panic!("expected conflicts, got {outcome:?}");
    };
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "other/y.txt");
    assert!(f.read("other/y.txt").contains("<<<<<<<"));
    let changes = f.repo.status(&t).unwrap().changes;
    assert!(
        changes.iter().any(|c| c.path == "other/y.txt"),
        "the conflicted file is visible: {changes:?}"
    );
    assert!(matches!(
        f.repo.sparse_add(&t, &strings(&["docs"])),
        Err(SdkError::MergeInProgress)
    ));
    f.write("other/y.txt", "resolved\n");
    let merged = f.repo.continue_merge(&t, None).unwrap();
    let m = f.repo.manifest_of(&merged).unwrap();
    assert_eq!(
        f.repo
            .store()
            .read_entry(m.get("other/y.txt").unwrap())
            .unwrap(),
        b"resolved\n"
    );
    // Afterwards the path is outside the set again; reapply removes it.
    assert!(f.repo.status(&t).unwrap().is_clean());
    let report = f.repo.sparse_reapply(&t).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!f.exists("other/y.txt"));
}
