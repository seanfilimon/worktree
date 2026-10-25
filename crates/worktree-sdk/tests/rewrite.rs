//! History rewriting (ENGINEERING_PLAN §14.5): cherry-pick and rebase,
//! including conflicts resolved with the merge-resolution API, `--skip`,
//! and `--abort` restoring the exact previous state.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use common::Fixture;
use worktree_sdk::repo::{CherryPickOptions, RebaseOptions, RewriteOp, RewriteStatus};
use worktree_sdk::{ConflictKind, ConflictResolution, SdkError, Snapshot, SwitchOptions};

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

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.repo.branch_tip(&fx.tree, branch).unwrap().unwrap()
}

/// HEAD, HEAD~1, ... HEAD~(n-1).
fn chain(fx: &Fixture, n: usize) -> Vec<Snapshot> {
    (0..n)
        .map(|i| fx.repo.resolve(&fx.tree, &format!("HEAD~{i}")).unwrap())
        .collect()
}

/// Every file under the worktree root except `.wt/`, with its content.
fn files(root: &Path) -> BTreeMap<String, String> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if prefix.is_empty() && name == ".wt" {
                continue;
            }
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &rel, out);
            } else {
                out.insert(rel, fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, "", &mut out);
    out
}

fn no_operation_in_progress(fx: &Fixture) {
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
    assert!(fx.repo.rewrite_state(&fx.tree).unwrap().is_none());
    assert!(!fx
        .repo
        .wt_dir()
        .join("conflicts")
        .join("demo.json")
        .exists());
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

/// main: base(a.txt = 1..5); feature: f1 edits line 1 + adds f.txt, f2
/// edits line 5; main then edits line 3.
fn fork(fx: &Fixture) -> (Snapshot, Snapshot) {
    fx.write("a.txt", "1\n2\n3\n4\n5\n");
    fx.snap("base");
    switch(fx, "feature", true);
    fx.write("a.txt", "one\n2\n3\n4\n5\n");
    fx.write("f.txt", "f\n");
    let f1 = fx.snap("f1: line one");
    fx.write("a.txt", "one\n2\n3\n4\nfive\n");
    let f2 = fx.snap("f2: line five");
    switch(fx, "main", false);
    fx.write("a.txt", "1\n2\nthree\n4\n5\n");
    fx.snap("main: line three");
    (f1, f2)
}

// --------------------------------------------------------------------
// Cherry-pick
// --------------------------------------------------------------------

#[test]
fn cherry_pick_applies_changes_in_order_as_new_snapshots() {
    let fx = Fixture::new();
    let (f1, f2) = fork(&fx);
    let main_before = tip(&fx, "main");
    let out = fx
        .repo
        .cherry_pick(
            &fx.tree,
            &[f1.id.clone(), f2.id.clone()],
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_eq!(out.op, RewriteOp::CherryPick);
    assert_eq!(out.picked.len(), 2);
    assert_eq!(out.old_tip.as_deref(), Some(main_before.as_str()));
    assert_eq!(fx.read("a.txt"), "one\n2\nthree\n4\nfive\n");
    assert_eq!(fx.read("f.txt"), "f\n");

    let p2 = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    let p1 = fx.repo.resolve(&fx.tree, "HEAD~1").unwrap();
    assert_eq!(out.new_tip.as_deref(), Some(p2.id.as_str()));
    assert_eq!(p1.parents, vec![main_before.clone()]);
    assert_eq!(p2.parents, vec![p1.id.clone()]);
    assert_ne!(p1.id, f1.id);
    assert_eq!(p1.message, f1.message);
    assert_eq!(p1.author, f1.author);
    assert_eq!(p1.branch, "main");
    assert_eq!(p2.message, f2.message);
    // The originals are untouched.
    assert_eq!(tip(&fx, "feature"), f2.id);
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    let reflog = fx
        .repo
        .reflog_entries(&fx.tree, Some("main"), Some(2))
        .unwrap();
    assert!(reflog.iter().all(|e| e.action == "cherry-pick"));
    assert_eq!(reflog[0].new.as_deref(), Some(p2.id.as_str()));
    assert!(reflog[1].message.contains(f1.short_id()), "{reflog:?}");
}

#[test]
fn cherry_pick_skips_changes_already_present() {
    let fx = Fixture::new();
    let (f1, _) = fork(&fx);
    fx.repo
        .cherry_pick(
            &fx.tree,
            std::slice::from_ref(&f1.id),
            &CherryPickOptions::default(),
        )
        .unwrap();
    let head = tip(&fx, "main");
    let again = fx
        .repo
        .cherry_pick(
            &fx.tree,
            std::slice::from_ref(&f1.id),
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert_eq!(again.status, RewriteStatus::Completed);
    assert!(again.picked.is_empty());
    assert_eq!(again.skipped, vec![f1.id]);
    assert_eq!(tip(&fx, "main"), head);
}

#[test]
fn cherry_pick_needs_a_clean_tree_and_revisions() {
    let fx = Fixture::new();
    let (f1, _) = fork(&fx);
    fx.write("dirty.txt", "x");
    let err = fx
        .repo
        .cherry_pick(
            &fx.tree,
            std::slice::from_ref(&f1.id),
            &CherryPickOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(err, SdkError::DirtyWorkingTree(1)), "{err:?}");
    fx.remove("dirty.txt");
    let err = fx
        .repo
        .cherry_pick(&fx.tree, &[], &CherryPickOptions::default())
        .unwrap_err();
    assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
    let err = fx
        .repo
        .cherry_pick(&fx.tree, &["nope".into()], &CherryPickOptions::default())
        .unwrap_err();
    assert!(matches!(err, SdkError::InvalidRevision(_)), "{err:?}");
}

/// feature: c1 edits line 3 (conflicts with main), c2 adds g.txt.
fn conflicting_fork(fx: &Fixture) -> (Snapshot, Snapshot) {
    fx.write("a.txt", "1\n2\n3\n");
    fx.snap("base");
    switch(fx, "feature", true);
    fx.write("a.txt", "1\n2\nfeature\n");
    let c1 = fx.snap("c1: feature line");
    fx.write("g.txt", "g\n");
    let c2 = fx.snap("c2: add g");
    switch(fx, "main", false);
    fx.write("a.txt", "1\n2\nmain\n");
    fx.write("m.txt", "m\n");
    fx.snap("main line");
    (c1, c2)
}

#[test]
fn cherry_pick_conflict_resolves_with_merge_resolve_and_continues() {
    let fx = Fixture::new();
    let (c1, c2) = conflicting_fork(&fx);
    let main_before = tip(&fx, "main");
    let out = fx
        .repo
        .cherry_pick(
            &fx.tree,
            &[c1.id.clone(), c2.id.clone()],
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Conflicts);
    assert_eq!(out.stopped_at.as_deref(), Some(c1.id.as_str()));
    assert_eq!(out.remaining, 1);
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].kind, ConflictKind::Content);
    let text = fx.read("a.txt");
    assert!(text.contains("<<<<<<< ours (main "), "{text}");
    assert!(
        text.contains(&format!(">>>>>>> theirs ({})", c1.short_id())),
        "{text}"
    );

    // The same conflict files as a merge: status works.
    let status = fx.repo.merge_status(&fx.tree).unwrap().unwrap();
    assert_eq!(status.source_tip, c1.id);
    assert_eq!(status.target_tip.as_deref(), Some(main_before.as_str()));
    assert_eq!(status.unresolved().count(), 1);
    let state = fx.repo.rewrite_state(&fx.tree).unwrap().unwrap();
    assert_eq!(state.op, RewriteOp::CherryPick);
    assert_eq!(state.todo, vec![c2.id.clone()]);

    // Merge completion paths refuse while a cherry-pick is stopped.
    let err = fx.repo.continue_merge(&fx.tree, None).unwrap_err();
    assert!(
        err.to_string().contains("wt cherry-pick --continue"),
        "{err}"
    );
    assert!(fx.repo.abort_merge(&fx.tree).is_err());
    assert!(matches!(
        fx.repo.cherry_pick_continue(&fx.tree).unwrap_err(),
        SdkError::MergeConflict(_)
    ));
    assert!(fx.repo.rebase_continue(&fx.tree).is_err());

    fx.repo
        .resolve_conflict(&fx.tree, "a.txt", &ConflictResolution::Theirs)
        .unwrap();
    // A snapshot would be a merge: refused, too.
    fx.write("a.txt", "1\n2\nresolved\n");
    let err = fx.repo.snapshot(&fx.tree, common::opts("x")).unwrap_err();
    assert!(err.to_string().contains("cherry-pick"), "{err}");

    let done = fx.repo.cherry_pick_continue(&fx.tree).unwrap();
    assert_eq!(done.status, RewriteStatus::Completed);
    assert_eq!(done.picked.len(), 2);
    let head = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    let first = fx.repo.resolve(&fx.tree, "HEAD~1").unwrap();
    assert_eq!(first.parents, vec![main_before]);
    assert_eq!(first.message, c1.message);
    assert_eq!(head.message, c2.message);
    assert_eq!(fx.read("a.txt"), "1\n2\nresolved\n");
    assert_eq!(fx.read("g.txt"), "g\n");
    no_operation_in_progress(&fx);
}

#[test]
fn cherry_pick_abort_restores_branch_and_files_exactly() {
    let fx = Fixture::new();
    let (c1, c2) = conflicting_fork(&fx);
    let main_before = tip(&fx, "main");
    let files_before = files(&fx.root());
    // c2 applies cleanly first (a snapshot is created), then c1 conflicts.
    let out = fx
        .repo
        .cherry_pick(
            &fx.tree,
            &[c2.id.clone(), c1.id.clone()],
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Conflicts);
    assert_eq!(out.picked.len(), 1);
    assert_ne!(tip(&fx, "main"), main_before);
    assert!(fx.exists("g.txt"));

    fx.repo.cherry_pick_abort(&fx.tree).unwrap();
    assert_eq!(tip(&fx, "main"), main_before);
    assert_eq!(files(&fx.root()), files_before);
    no_operation_in_progress(&fx);
    let last = &fx
        .repo
        .reflog_entries(&fx.tree, Some("main"), Some(1))
        .unwrap()[0];
    assert_eq!(last.action, "cherry-pick");
    assert_eq!(last.new.as_deref(), Some(main_before.as_str()));
    // Nothing left to abort.
    assert!(fx.repo.cherry_pick_abort(&fx.tree).is_err());
}

#[test]
fn cherry_pick_skip_drops_the_conflicting_step() {
    let fx = Fixture::new();
    let (c1, c2) = conflicting_fork(&fx);
    fx.repo
        .cherry_pick(
            &fx.tree,
            &[c1.id.clone(), c2.id.clone()],
            &CherryPickOptions::default(),
        )
        .unwrap();
    let out = fx.repo.cherry_pick_skip(&fx.tree).unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_eq!(out.skipped, vec![c1.id]);
    assert_eq!(out.picked.len(), 1);
    assert_eq!(fx.read("a.txt"), "1\n2\nmain\n");
    assert_eq!(fx.read("g.txt"), "g\n");
    no_operation_in_progress(&fx);
}

#[test]
fn cherry_pick_no_snapshot_changes_only_the_working_tree() {
    let fx = Fixture::new();
    let (f1, f2) = fork(&fx);
    let main_before = tip(&fx, "main");
    let out = fx
        .repo
        .cherry_pick(
            &fx.tree,
            &[f1.id.clone(), f2.id.clone()],
            &CherryPickOptions { no_snapshot: true },
        )
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert!(out.picked.iter().all(|p| p.new.is_none()));
    assert_eq!(tip(&fx, "main"), main_before);
    assert_eq!(fx.read("a.txt"), "one\n2\nthree\n4\nfive\n");
    let status = fx.repo.status(&fx.tree).unwrap();
    assert_eq!(status.changes.len(), 2, "{status:?}");
    assert!(fx.repo.rewrite_state(&fx.tree).unwrap().is_none());
}

#[test]
fn cherry_pick_no_snapshot_conflict_keeps_earlier_steps_on_the_ours_side() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n2\n3\n");
    fx.snap("base");
    switch(&fx, "feature", true);
    fx.write("a.txt", "one\n2\n3\n");
    let s1 = fx.snap("s1");
    fx.write("a.txt", "one\n2\nfeature\n");
    let s2 = fx.snap("s2");
    switch(&fx, "main", false);
    fx.write("a.txt", "1\n2\nmain\n");
    fx.snap("main");
    let main_before = tip(&fx, "main");
    let files_before = files(&fx.root());

    let out = fx
        .repo
        .cherry_pick(
            &fx.tree,
            &[s1.id.clone(), s2.id.clone()],
            &CherryPickOptions { no_snapshot: true },
        )
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Conflicts);
    assert_eq!(out.stopped_at.as_deref(), Some(s2.id.as_str()));
    // "ours" is the working state with s1 applied.
    fx.repo
        .resolve_conflict(&fx.tree, "a.txt", &ConflictResolution::Ours)
        .unwrap();
    assert_eq!(fx.read("a.txt"), "one\n2\nmain\n");
    let done = fx.repo.cherry_pick_continue(&fx.tree).unwrap();
    assert_eq!(done.status, RewriteStatus::Completed);
    assert_eq!(tip(&fx, "main"), main_before);
    assert_eq!(fx.read("a.txt"), "one\n2\nmain\n");
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());

    // And --abort brings back HEAD's content.
    fx.repo
        .restore(&fx.tree, &[], None)
        .expect("reset working tree");
    fx.repo
        .cherry_pick(
            &fx.tree,
            &[s1.id.clone(), s2.id.clone()],
            &CherryPickOptions { no_snapshot: true },
        )
        .unwrap();
    fx.repo.cherry_pick_abort(&fx.tree).unwrap();
    assert_eq!(files(&fx.root()), files_before);
    no_operation_in_progress(&fx);
}

// --------------------------------------------------------------------
// Rebase
// --------------------------------------------------------------------

#[test]
fn rebase_replays_first_parent_snapshots_onto_the_target() {
    let fx = Fixture::new();
    let (f1, f2) = fork(&fx);
    let main_tip = tip(&fx, "main");
    // A revert on the branch: its `reverts` must follow the rewrite.
    switch(&fx, "feature", false);
    fx.write("r.txt", "r\n");
    let added = fx.snap("add r");
    let revert = fx.repo.revert(&fx.tree, &added.id).unwrap();

    let out = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_eq!(out.op, RewriteOp::Rebase);
    assert_eq!(out.onto.as_deref(), Some(main_tip.as_str()));
    assert_eq!(out.picked.len(), 4);
    assert_eq!(
        out.picked
            .iter()
            .map(|p| p.original.clone())
            .collect::<Vec<_>>(),
        vec![
            f1.id.clone(),
            f2.id.clone(),
            added.id.clone(),
            revert.id.clone()
        ]
    );
    let log = chain(&fx, 5);
    assert_eq!(log[4].id, main_tip);
    for pair in log.windows(2).take(4) {
        assert_eq!(pair[0].parents, vec![pair[1].id.clone()]);
    }
    assert_eq!(log[3].message, f1.message);
    assert_eq!(log[3].author, f1.author);
    assert_ne!(log[3].id, f1.id);
    assert_eq!(log[0].reverts.as_deref(), Some(log[1].id.as_str()));
    assert_eq!(fx.read("a.txt"), "one\n2\nthree\n4\nfive\n");
    assert!(!fx.exists("r.txt"));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    assert!(fx
        .repo
        .is_ancestor(&main_tip, &tip(&fx, "feature"))
        .unwrap());

    let entry = &fx
        .repo
        .reflog_entries(&fx.tree, Some("feature"), Some(1))
        .unwrap()[0];
    assert_eq!(entry.action, "rebase");
    assert_eq!(entry.old.as_deref(), Some(revert.id.as_str()));
    assert_eq!(entry.new.as_deref(), Some(log[0].id.as_str()));
    // feature@{1} is the pre-rebase tip.
    assert_eq!(
        fx.repo.resolve(&fx.tree, "feature@{1}").unwrap().id,
        revert.id
    );
}

#[test]
fn rebase_up_to_date_and_fast_forward() {
    let fx = Fixture::new();
    fork(&fx);
    // main already contains its own history.
    let out = fx
        .repo
        .rebase(&fx.tree, "main~1", &RebaseOptions::default())
        .unwrap();
    assert_eq!(out.status, RewriteStatus::UpToDate);

    switch(&fx, "behind", true);
    switch(&fx, "main", false);
    fx.write("later.txt", "l\n");
    let later = fx.snap("later");
    switch(&fx, "behind", false);
    let out = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    assert_eq!(out.status, RewriteStatus::FastForward);
    assert_eq!(tip(&fx, "behind"), later.id);
    assert_eq!(fx.read("later.txt"), "l\n");
}

#[test]
fn rebase_refuses_pushed_history_unless_forced() {
    let fx = Fixture::new();
    let (_, f2) = fork(&fx);
    switch(&fx, "feature", false);
    fx.repo
        .set_remote_tip(&fx.tree, "origin", "feature", Some(&f2.id))
        .unwrap();
    let err = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("origin/feature"), "{msg}");
    assert!(msg.contains("--force"), "{msg}");
    assert_eq!(tip(&fx, "feature"), f2.id);
    let out = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions { force: true })
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_ne!(tip(&fx, "feature"), f2.id);
}

#[test]
fn rebase_conflict_continue_skip_and_abort() {
    let fx = Fixture::new();
    let (c1, c2) = conflicting_fork(&fx);
    let main_tip = tip(&fx, "main");
    switch(&fx, "feature", false);
    let files_before = files(&fx.root());

    // Abort restores exactly.
    let out = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Conflicts);
    assert_eq!(out.stopped_at.as_deref(), Some(c1.id.as_str()));
    assert_eq!(tip(&fx, "feature"), main_tip, "branch at the progress tip");
    assert!(fx.exists("m.txt"));
    assert!(fx.repo.cherry_pick_continue(&fx.tree).is_err());
    fx.repo.rebase_abort(&fx.tree).unwrap();
    assert_eq!(tip(&fx, "feature"), c2.id);
    assert_eq!(files(&fx.root()), files_before);
    no_operation_in_progress(&fx);

    // Skip drops the conflicting snapshot.
    fx.repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    let out = fx.repo.rebase_skip(&fx.tree).unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_eq!(out.skipped, vec![c1.id.clone()]);
    assert_eq!(fx.read("a.txt"), "1\n2\nmain\n");
    assert_eq!(fx.read("g.txt"), "g\n");
    let head = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    assert_eq!(head.parents, vec![main_tip.clone()]);
    no_operation_in_progress(&fx);

    // Continue after resolving with the merge API.
    fx.repo.rebase_abort(&fx.tree).unwrap_err();
    fx.repo
        .set_branch_tip(&fx.tree, "feature", Some(&c2.id))
        .unwrap();
    fx.repo.restore(&fx.tree, &[], None).unwrap();
    fx.repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    let resolved = fx
        .repo
        .resolve_all_conflicts(&fx.tree, &ConflictResolution::Theirs)
        .unwrap();
    assert_eq!(resolved, vec!["a.txt".to_string()]);
    let out = fx.repo.rebase_continue(&fx.tree).unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    assert_eq!(out.picked.len(), 2);
    assert_eq!(fx.read("a.txt"), "1\n2\nfeature\n");
    let log = chain(&fx, 3);
    assert_eq!(log[2].id, main_tip);
    assert_eq!(log[1].message, c1.message);
    assert_eq!(log[0].message, c2.message);
    no_operation_in_progress(&fx);
}

#[test]
fn rewrite_roots_cover_a_stopped_rebase() {
    let fx = Fixture::new();
    let (c1, c2) = conflicting_fork(&fx);
    switch(&fx, "feature", false);
    assert!(fx.repo.rewrite_roots().unwrap().is_empty());
    fx.repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    let roots = fx.repo.rewrite_roots().unwrap();
    assert!(
        roots.contains(&c1.id) && roots.contains(&c2.id),
        "{roots:?}"
    );
    // Other operations are refused meanwhile.
    assert!(matches!(
        fx.repo
            .cherry_pick(
                &fx.tree,
                std::slice::from_ref(&c1.id),
                &CherryPickOptions::default()
            )
            .unwrap_err(),
        SdkError::Other(_)
    ));
}
