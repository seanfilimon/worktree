//! v0.5 merge, diff and history features (ENGINEERING_PLAN §13.10):
//! merge strategies, conflict hunks and resolution, revert options, rename
//! detection, word diffs, reflog revisions and expiry, signature status.

mod common;

use common::Fixture;
use worktree_sdk::signing::{verify_local, SignatureStatus};
use worktree_sdk::{
    ChangeKind, ConflictKind, ConflictResolution, DiffOptions, MergeOptions, MergeOutcome,
    MergeStrategy, RevertOptions, SdkError, SwitchOptions,
};

fn switch(fx: &Fixture, name: &str, create: bool) {
    fx.repo
        .switch_branch(
            &fx.tree,
            name,
            SwitchOptions {
                create,
                force: false,
            },
        )
        .unwrap();
}

/// main and feature both edit line 2 of `shared.txt`; feature also edits
/// line 5 and adds a file; main deletes `gone.txt` which feature modifies.
fn diverge(fx: &Fixture) {
    fx.write("shared.txt", "1\n2\n3\n4\n5\n");
    fx.write("gone.txt", "g\n");
    fx.snap("base");
    switch(fx, "feature", true);
    fx.write("shared.txt", "1\nFEATURE\n3\n4\nfive\n");
    fx.write("gone.txt", "g changed\n");
    fx.write("feature-only.txt", "f\n");
    fx.snap("feature edit");
    switch(fx, "main", false);
    fx.write("shared.txt", "1\nMAIN\n3\n4\n5\n");
    fx.remove("gone.txt");
    fx.snap("main edit");
}

fn strategy(s: MergeStrategy) -> MergeOptions {
    MergeOptions {
        strategy: s,
        ..MergeOptions::default()
    }
}

#[test]
fn manual_merge_records_hunks_and_short_id_labels() {
    let fx = Fixture::new();
    diverge(&fx);
    let main_tip = fx.repo.resolve(&fx.tree, "main").unwrap();
    let feature_tip = fx.repo.resolve(&fx.tree, "feature").unwrap();
    let MergeOutcome::Conflicts { conflicts } = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap()
    else {
        panic!("expected conflicts")
    };
    let shared = conflicts.iter().find(|c| c.path == "shared.txt").unwrap();
    assert_eq!(shared.kind, ConflictKind::Content);
    assert_eq!(shared.hunks.len(), 1);
    let hunk = shared.hunks[0];
    assert_eq!(
        (
            hunk.ours_start,
            hunk.ours_len,
            hunk.theirs_start,
            hunk.theirs_len
        ),
        (2, 1, 2, 1)
    );
    let gone = conflicts.iter().find(|c| c.path == "gone.txt").unwrap();
    assert_eq!(gone.kind, ConflictKind::ModifyDelete);

    let text = fx.read("shared.txt");
    assert!(
        text.contains(&format!("<<<<<<< ours (main {})", main_tip.short_id())),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            ">>>>>>> theirs (feature {})",
            feature_tip.short_id()
        )),
        "{text}"
    );
    // Non-conflicting change from feature is merged.
    assert!(text.ends_with("4\nfive\n"), "{text}");

    // Hunks are persisted in .wt/conflicts/ and MERGE_STATE.
    let report =
        std::fs::read_to_string(fx.repo.wt_dir().join("conflicts").join("demo.json")).unwrap();
    assert!(report.contains("\"ours_start\": 2"), "{report}");
    let state = fx.repo.merge_state(&fx.tree).unwrap().unwrap();
    assert_eq!(
        state.conflicts.iter().map(|c| c.hunks.len()).sum::<usize>(),
        1
    );
}

#[test]
fn ours_strategy_completes_and_keeps_clean_changes() {
    let fx = Fixture::new();
    diverge(&fx);
    let MergeOutcome::Merged { snapshot } = fx
        .repo
        .merge(&fx.tree, "feature", strategy(MergeStrategy::Ours))
        .unwrap()
    else {
        panic!("expected a merge")
    };
    assert!(snapshot.is_merge());
    assert_eq!(fx.read("shared.txt"), "1\nMAIN\n3\n4\nfive\n");
    assert!(!fx.exists("gone.txt"), "ours deleted gone.txt");
    assert_eq!(fx.read("feature-only.txt"), "f\n");
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
}

#[test]
fn theirs_strategy_takes_source_side() {
    let fx = Fixture::new();
    diverge(&fx);
    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", strategy(MergeStrategy::Theirs))
        .unwrap();
    assert!(
        matches!(outcome, MergeOutcome::Merged { .. }),
        "{outcome:?}"
    );
    assert_eq!(fx.read("shared.txt"), "1\nFEATURE\n3\n4\nfive\n");
    assert_eq!(fx.read("gone.txt"), "g changed\n");
}

#[test]
fn resolve_paths_then_continue() {
    let fx = Fixture::new();
    diverge(&fx);
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    let status = fx.repo.merge_status(&fx.tree).unwrap().unwrap();
    assert_eq!(status.branch, "main");
    assert_eq!(status.unresolved().count(), 2);

    assert!(matches!(
        fx.repo
            .resolve_conflict(&fx.tree, "feature-only.txt", &ConflictResolution::Ours),
        Err(SdkError::InvalidArgument(_))
    ));

    // --theirs on a text conflict keeps main's clean changes (none here)
    // and feature's line 5.
    fx.repo
        .resolve_conflict(&fx.tree, "shared.txt", &ConflictResolution::Theirs)
        .unwrap();
    assert_eq!(fx.read("shared.txt"), "1\nFEATURE\n3\n4\nfive\n");
    // --ours on modify/delete deletes the file again.
    fx.repo
        .resolve_conflict(&fx.tree, "gone.txt", &ConflictResolution::Ours)
        .unwrap();
    assert!(!fx.exists("gone.txt"));

    let status = fx.repo.merge_status(&fx.tree).unwrap().unwrap();
    assert_eq!(status.unresolved().count(), 0, "{status:?}");
    let state = fx.repo.merge_state(&fx.tree).unwrap().unwrap();
    assert_eq!(state.resolved, ["shared.txt", "gone.txt"]);

    let merged = fx.repo.continue_merge(&fx.tree, Some("resolved")).unwrap();
    assert_eq!(merged.parents.len(), 2);
    assert!(fx.repo.merge_status(&fx.tree).unwrap().is_none());
}

#[test]
fn resolve_with_content_accepts_marker_like_text() {
    let fx = Fixture::new();
    diverge(&fx);
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    let custom = b"<<<<<<< this line is intentional\n".to_vec();
    fx.repo
        .resolve_conflict(
            &fx.tree,
            "shared.txt",
            &ConflictResolution::Content(custom.clone()),
        )
        .unwrap();
    assert_eq!(fx.read("shared.txt").as_bytes(), custom.as_slice());
    // Explicitly resolved paths are not re-checked for markers.
    fx.repo.continue_merge(&fx.tree, None).unwrap();
}

#[test]
fn resolve_all_and_abort() {
    let fx = Fixture::new();
    diverge(&fx);
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert!(fx
        .repo
        .resolve_all_conflicts(&fx.tree, &ConflictResolution::Content(Vec::new()))
        .is_err());
    let done = fx
        .repo
        .resolve_all_conflicts(&fx.tree, &ConflictResolution::Ours)
        .unwrap();
    assert_eq!(done, ["gone.txt", "shared.txt"]);
    assert_eq!(fx.read("shared.txt"), "1\nMAIN\n3\n4\nfive\n");
    // Nothing left: a second --all resolves nothing.
    assert!(fx
        .repo
        .resolve_all_conflicts(&fx.tree, &ConflictResolution::Theirs)
        .unwrap()
        .is_empty());
    fx.repo.abort_merge(&fx.tree).unwrap();
    assert_eq!(fx.read("shared.txt"), "1\nMAIN\n3\n4\n5\n");
    assert!(matches!(
        fx.repo
            .resolve_conflict(&fx.tree, "shared.txt", &ConflictResolution::Ours),
        Err(SdkError::NoMergeInProgress)
    ));
}

#[test]
fn revert_options_and_merge_parents() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.snap("base");
    switch(&fx, "feature", true);
    fx.write("b.txt", "b\n");
    fx.snap("add b");
    switch(&fx, "main", false);
    fx.write("c.txt", "c\n");
    fx.snap("add c");
    let MergeOutcome::Merged { snapshot: merge } = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap()
    else {
        panic!("merge")
    };

    let err = fx.repo.revert(&fx.tree, "HEAD").unwrap_err();
    assert!(matches!(err, SdkError::InvalidArgument(_)), "{err}");
    assert!(err.to_string().contains("--parent"), "{err}");
    assert!(fx
        .repo
        .revert_with(
            &fx.tree,
            "HEAD",
            &RevertOptions {
                parent: Some(3),
                ..RevertOptions::default()
            }
        )
        .is_err());

    // Mainline 1 (main): undo what the merge brought in from feature.
    let revert = fx
        .repo
        .revert_with(
            &fx.tree,
            "HEAD",
            &RevertOptions {
                message: None,
                parent: Some(1),
            },
        )
        .unwrap();
    assert_eq!(revert.reverts.as_deref(), Some(merge.id.as_str()));
    assert!(revert.message.contains("parent 1"), "{}", revert.message);
    assert!(!fx.exists("b.txt"));
    assert!(fx.exists("c.txt"));

    // Plain revert with a custom message; --parent is rejected on it.
    assert!(fx
        .repo
        .revert_with(
            &fx.tree,
            "HEAD",
            &RevertOptions {
                parent: Some(1),
                ..RevertOptions::default()
            }
        )
        .is_err());
    let again = fx
        .repo
        .revert_with(
            &fx.tree,
            "HEAD",
            &RevertOptions {
                message: Some("bring b back".into()),
                parent: None,
            },
        )
        .unwrap();
    assert_eq!(again.message, "bring b back");
    assert_eq!(again.reverts.as_deref(), Some(revert.id.as_str()));
    assert!(fx.exists("b.txt"));
}

#[test]
fn rename_detection_exact_and_similar() {
    let fx = Fixture::new();
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    fx.write("src/old.rs", &body);
    fx.write("docs/a.md", "same content\n");
    fx.write("other.txt", "unrelated\n");
    let s1 = fx.snap("one");
    fx.remove("src/old.rs");
    fx.remove("docs/a.md");
    fx.remove("other.txt");
    fx.write("src/new.rs", &body.replace("line 7\n", "line seven\n"));
    fx.write("docs/b.md", "same content\n");
    fx.write("fresh.txt", "brand new\n");

    let plain = fx
        .repo
        .diff_working(&fx.tree, None, &DiffOptions::default())
        .unwrap();
    assert!(plain.iter().all(|d| d.old_path.is_none()));
    assert_eq!(plain.len(), 6);

    let opts = DiffOptions {
        find_renames: Some(50),
        ..DiffOptions::default()
    };
    let diffs = fx.repo.diff_working(&fx.tree, None, &opts).unwrap();
    let summary: Vec<_> = diffs
        .iter()
        .map(|d| (d.path.as_str(), d.kind, d.old_path.as_deref(), d.similarity))
        .collect();
    assert_eq!(
        summary,
        [
            (
                "docs/b.md",
                ChangeKind::Modified,
                Some("docs/a.md"),
                Some(100)
            ),
            ("fresh.txt", ChangeKind::Added, None, None),
            ("other.txt", ChangeKind::Deleted, None, None),
            (
                "src/new.rs",
                ChangeKind::Modified,
                Some("src/old.rs"),
                Some(95)
            ),
        ]
    );
    let exact = &diffs[0];
    assert!(exact.patch.is_none());
    let similar = &diffs[3];
    let patch = similar.patch.as_deref().unwrap();
    assert!(
        patch.starts_with("--- a/src/old.rs\n+++ b/src/new.rs\n"),
        "{patch}"
    );
    assert_eq!((similar.added, similar.removed), (1, 1));

    // A high threshold keeps only exact renames; revision diffs work too.
    let s2 = fx.snap("two");
    let strict = DiffOptions {
        find_renames: Some(100),
        name_only: true,
        ..DiffOptions::default()
    };
    let diffs = fx
        .repo
        .diff_revs(&fx.tree, &s1.id, &s2.id, &strict)
        .unwrap();
    let renamed: Vec<_> = diffs.iter().filter_map(|d| d.old_path.as_deref()).collect();
    assert_eq!(renamed, ["docs/a.md"]);

    // Path filters match either side of a rename.
    let scoped = DiffOptions {
        paths: vec!["src/old.rs".into()],
        ..opts.clone()
    };
    let diffs = fx
        .repo
        .diff_revs(&fx.tree, &s1.id, &s2.id, &scoped)
        .unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "src/new.rs");
}

#[test]
fn word_diff_patches() {
    let fx = Fixture::new();
    fx.write("a.txt", "keep\nthe quick brown fox\n");
    fx.snap("one");
    fx.write("a.txt", "keep\nthe slow brown fox\n");
    let opts = DiffOptions {
        word_diff: true,
        ..DiffOptions::default()
    };
    let diffs = fx.repo.diff_working(&fx.tree, None, &opts).unwrap();
    let patch = diffs[0].patch.as_deref().unwrap();
    assert!(
        patch.contains("the [-quick-]{+slow+} brown fox\n"),
        "{patch}"
    );
    assert_eq!((diffs[0].added, diffs[0].removed), (1, 1));
}

#[test]
fn reflog_revisions_and_filters() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n");
    let s1 = fx.snap("one");
    fx.write("a.txt", "2\n");
    let s2 = fx.snap("two");
    switch(&fx, "feature", true);
    fx.write("a.txt", "3\n");
    let s3 = fx.snap("three");

    // Current branch (feature): newest entry is the snapshot.
    assert_eq!(fx.repo.resolve(&fx.tree, "@{0}").unwrap().id, s3.id);
    assert_eq!(fx.repo.resolve(&fx.tree, "HEAD@{0}").unwrap().id, s3.id);
    assert_eq!(fx.repo.resolve(&fx.tree, "main@{0}").unwrap().id, s2.id);
    assert_eq!(fx.repo.resolve(&fx.tree, "main@{1}").unwrap().id, s1.id);
    assert_eq!(fx.repo.resolve(&fx.tree, "main@{0}~1").unwrap().id, s1.id);
    // main@{2} is the "init" entry, which records no snapshot.
    assert!(matches!(
        fx.repo.resolve(&fx.tree, "main@{2}"),
        Err(SdkError::InvalidRevision(_))
    ));
    assert!(matches!(
        fx.repo.resolve(&fx.tree, "main@{99}"),
        Err(SdkError::InvalidRevision(_))
    ));
    assert!(matches!(
        fx.repo.resolve(&fx.tree, "main@{yesterday}"),
        Err(SdkError::InvalidRevision(_))
    ));

    let main_entries = fx
        .repo
        .reflog_entries(&fx.tree, Some("main"), None)
        .unwrap();
    assert!(main_entries.iter().all(|e| e.branch == "main"));
    assert_eq!(main_entries.len(), 3);
    assert_eq!(
        fx.repo
            .reflog_entries(&fx.tree, Some("main"), Some(1))
            .unwrap()
            .len(),
        1
    );
    // init, one, two, branch.create, switch, three.
    assert_eq!(fx.repo.reflog(&fx.tree, None).unwrap().len(), 6);
}

#[test]
fn reflog_expiry() {
    let fx = Fixture::new();
    for i in 0..5 {
        fx.write("a.txt", &format!("{i}\n"));
        fx.snap(&format!("s{i}"));
    }
    let total = fx.repo.reflog(&fx.tree, None).unwrap().len();
    assert_eq!(total, 6);
    let report = fx
        .repo
        .expire_reflog(&fx.tree, chrono::Duration::days(90), 3)
        .unwrap();
    assert_eq!((report.removed, report.kept), (3, 3));
    let left = fx.repo.reflog(&fx.tree, None).unwrap();
    assert_eq!(left.len(), 3);
    assert_eq!(left[0].message, "s4");
    // Nothing more to do with the same retention.
    let again = fx
        .repo
        .expire_reflog(&fx.tree, chrono::Duration::days(90), 3)
        .unwrap();
    assert_eq!(again.removed, 0);
    // Age-based: everything is older than "now".
    let all = fx
        .repo
        .expire_all_reflogs(chrono::Duration::zero(), 1000)
        .unwrap();
    assert_eq!(all.kept, 0);
    assert!(fx.repo.reflog(&fx.tree, None).unwrap().is_empty());
    // Defaults are callable (daemon maintenance) and appends keep working.
    fx.repo.expire_reflogs_default().unwrap();
    fx.write("a.txt", "again\n");
    fx.snap("after expiry");
    assert_eq!(fx.repo.reflog(&fx.tree, None).unwrap().len(), 1);
    assert_eq!(Repository::REFLOG_DEFAULT_KEEP, 1000);
    assert_eq!(Repository::REFLOG_DEFAULT_MAX_AGE_DAYS, 90);
}

use worktree_sdk::Repository;

#[test]
fn verify_local_reports_unsigned() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n");
    let snap = fx.snap("one");
    assert_eq!(
        verify_local(&fx.repo, &snap).unwrap(),
        SignatureStatus::Unsigned
    );
}
