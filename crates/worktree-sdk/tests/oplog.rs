//! history-v2 (ENGINEERING_PLAN §16.10): the local operation log and
//! `Repository::undo` / `Repository::redo`.

mod common;

use common::Fixture;
use worktree_sdk::repo::{
    CherryPickOptions, OpKind, RebaseOptions, RefChange, StashOptions, UndoOptions,
};
use worktree_sdk::{MergeOptions, SdkError, SwitchOptions};

fn once() -> UndoOptions {
    UndoOptions::default()
}

#[test]
fn every_mutation_is_logged_and_undo_redo_round_trips() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    let s1 = f.snap("first");
    f.write("a.txt", "two\n");
    f.write("b.txt", "bee\n");
    let s2 = f.snap("second");

    let ops = f.repo.oplog(&f.tree).unwrap();
    assert_eq!(ops.len(), 2, "{ops:#?}");
    assert_eq!(ops[0].op, "snapshot");
    assert_eq!(ops[0].kind, OpKind::Op);
    assert!(matches!(
        &ops[0].changes[..],
        [RefChange::Branch { name, old: Some(o), new: Some(n) }]
            if name == "main" && o.tip.as_deref() == Some(s1.id.as_str()) && n.tip.as_deref() == Some(s2.id.as_str())
    ));
    assert!(f.repo.wt_dir().join("oplog.jsonl").is_file());

    // Undo the second snapshot: main and the working tree go back.
    let report = f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(report.action, "undo");
    assert_eq!(report.operations[0].id, ops[0].id);
    assert_eq!(report.head.as_deref(), Some(s1.id.as_str()));
    assert_eq!(
        f.repo.branch_tip(&f.tree, "main").unwrap(),
        Some(s1.id.clone())
    );
    assert_eq!(f.read("a.txt"), "one\n");
    assert!(!f.exists("b.txt"));
    assert!(f.repo.status(&f.tree).unwrap().is_clean());
    assert_eq!(f.repo.oplog(&f.tree).unwrap().len(), 1);
    assert_eq!(f.repo.redo_list(&f.tree).unwrap().len(), 1);

    // Redo brings it back.
    let report = f.repo.redo(&f.tree, once()).unwrap();
    assert_eq!(report.head.as_deref(), Some(s2.id.as_str()));
    assert_eq!(f.read("a.txt"), "two\n");
    assert_eq!(f.read("b.txt"), "bee\n");
    assert!(f.repo.redo_list(&f.tree).unwrap().is_empty());
    let err = f.repo.redo(&f.tree, once()).unwrap_err();
    assert!(err.to_string().contains("nothing to redo"), "{err}");

    // Undo two at once, then a new operation clears the redo stack.
    let report = f
        .repo
        .undo(
            &f.tree,
            UndoOptions {
                count: 2,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(report.operations.len(), 2);
    assert_eq!(report.head, None, "back before the first snapshot");
    assert!(!f.exists("a.txt"));
    f.write("c.txt", "see\n");
    f.snap("third");
    assert!(f.repo.redo_list(&f.tree).unwrap().is_empty());
    let entries = f.repo.oplog_entries(&f.tree).unwrap();
    assert_eq!(entries.iter().filter(|e| e.kind == OpKind::Undo).count(), 3);
    assert_eq!(entries.iter().filter(|e| e.kind == OpKind::Redo).count(), 1);
}

#[test]
fn undo_refuses_dirty_trees_unless_forced() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.snap("first");
    f.write("a.txt", "two\n");
    f.snap("second");
    f.write("a.txt", "dirty\n");
    let err = f.repo.undo(&f.tree, once()).unwrap_err();
    assert!(matches!(err, SdkError::DirtyWorkingTree(1)), "{err:?}");
    assert_eq!(f.read("a.txt"), "dirty\n");
    f.repo
        .undo(
            &f.tree,
            UndoOptions {
                count: 1,
                force: true,
            },
        )
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\n", "--force discards the changes");
    let err = f
        .repo
        .undo(
            &f.tree,
            UndoOptions {
                count: 5,
                force: false,
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("only 1 operation"), "{err}");
}

#[test]
fn branch_switch_create_and_archive_are_undone() {
    let f = Fixture::new();
    f.write("a.txt", "main\n");
    f.snap("base");
    f.repo
        .switch_branch(
            &f.tree,
            "feature",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    f.write("a.txt", "feature\n");
    f.snap("on feature");
    f.repo
        .switch_branch(&f.tree, "main", SwitchOptions::default())
        .unwrap();
    f.repo.archive_branch(&f.tree, "feature").unwrap();
    assert!(f
        .repo
        .branches(&f.tree, false)
        .unwrap()
        .iter()
        .all(|b| b.name != "feature"));

    // Undo the archive, then the switch back to main.
    f.repo.undo(&f.tree, once()).unwrap();
    assert!(f
        .repo
        .branches(&f.tree, false)
        .unwrap()
        .iter()
        .any(|b| b.name == "feature"));
    f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(f.repo.current_branch(&f.tree).unwrap(), "feature");
    assert_eq!(f.read("a.txt"), "feature\n");
    // Undo the snapshot on feature, then the switch that created it.
    f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(f.read("a.txt"), "main\n");
    let report = f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(report.branch, "main");
    assert!(
        f.repo
            .branches(&f.tree, true)
            .unwrap()
            .iter()
            .all(|b| b.name != "feature"),
        "the created branch is gone"
    );
}

#[test]
fn merge_and_tags_are_undone() {
    let f = Fixture::new();
    f.write("a.txt", "base\n");
    let base = f.snap("base");
    f.repo.create_branch(&f.tree, "side", None).unwrap();
    f.repo
        .switch_branch(&f.tree, "side", SwitchOptions::default())
        .unwrap();
    f.write("side.txt", "side\n");
    f.snap("side work");
    f.repo
        .switch_branch(&f.tree, "main", SwitchOptions::default())
        .unwrap();
    f.repo
        .merge(&f.tree, "side", MergeOptions::default())
        .unwrap();
    assert!(f.exists("side.txt"));
    assert_eq!(f.repo.oplog(&f.tree).unwrap()[0].op, "fast-forward");
    f.repo.undo(&f.tree, once()).unwrap();
    assert!(!f.exists("side.txt"));
    assert_eq!(
        f.repo.branch_tip(&f.tree, "main").unwrap(),
        Some(base.id.clone())
    );

    f.repo.create_tag(&f.tree, "v1", None, Some("one")).unwrap();
    f.repo.delete_tag(&f.tree, "v1").unwrap();
    f.repo.undo(&f.tree, once()).unwrap();
    let tags = f.repo.tags(&f.tree).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].message.as_deref(), Some("one"));
    f.repo.undo(&f.tree, once()).unwrap();
    assert!(f.repo.tags(&f.tree).unwrap().is_empty());
}

#[test]
fn stash_push_pop_and_drop_are_undone() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.snap("first");
    f.write("a.txt", "wip\n");
    f.repo
        .stash_push(&f.tree, &StashOptions::default())
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\n");
    assert_eq!(f.repo.oplog(&f.tree).unwrap()[0].op, "stash.push");

    // Undoing the push gives the changes back and empties the stash.
    let report = f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(report.restored_paths, ["a.txt"]);
    assert_eq!(f.read("a.txt"), "wip\n");
    assert!(f.repo.stash_list(&f.tree).unwrap().is_empty());
    // Redoing it stashes them again (the working tree holds exactly them).
    f.repo.redo(&f.tree, once()).unwrap();
    assert_eq!(f.read("a.txt"), "one\n");
    assert_eq!(f.repo.stash_list(&f.tree).unwrap().len(), 1);

    // Pop, then undo the pop: the entry is back, the tree clean.
    f.repo.stash_pop(&f.tree, 0).unwrap();
    assert_eq!(f.read("a.txt"), "wip\n");
    f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(f.read("a.txt"), "one\n");
    assert_eq!(f.repo.stash_list(&f.tree).unwrap().len(), 1);

    // Drop, then undo the drop: nothing is applied.
    f.repo.stash_drop(&f.tree, 0).unwrap();
    f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(f.repo.stash_list(&f.tree).unwrap().len(), 1);
    assert_eq!(f.read("a.txt"), "one\n");
}

#[test]
fn rebase_and_cherry_pick_are_undone() {
    let f = Fixture::new();
    f.write("a.txt", "base\n");
    f.snap("base");
    f.repo.create_branch(&f.tree, "topic", None).unwrap();
    f.write("main.txt", "main\n");
    let main_tip = f.snap("main work");
    f.repo
        .switch_branch(&f.tree, "topic", SwitchOptions::default())
        .unwrap();
    f.write("topic.txt", "topic\n");
    let topic_tip = f.snap("topic work");

    f.repo
        .rebase(&f.tree, "main", &RebaseOptions::default())
        .unwrap();
    assert!(f.exists("main.txt"));
    assert_ne!(
        f.repo.branch_tip(&f.tree, "topic").unwrap(),
        Some(topic_tip.id.clone())
    );
    f.repo.undo(&f.tree, once()).unwrap();
    assert_eq!(
        f.repo.branch_tip(&f.tree, "topic").unwrap(),
        Some(topic_tip.id.clone())
    );
    assert!(
        !f.exists("main.txt"),
        "the rebase is gone from the working tree"
    );

    f.repo
        .cherry_pick(
            &f.tree,
            std::slice::from_ref(&main_tip.id),
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert!(f.exists("main.txt"));
    f.repo.undo(&f.tree, once()).unwrap();
    assert!(!f.exists("main.txt"));
    assert_eq!(
        f.repo.branch_tip(&f.tree, "topic").unwrap(),
        Some(topic_tip.id)
    );
}

#[test]
fn refs_moved_outside_the_log_need_force() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    let s1 = f.snap("first");
    f.write("a.txt", "two\n");
    f.snap("second");
    // Move main behind the log's back (plumbing writes are logged too, so
    // write the ref file directly).
    let ref_path = f
        .repo
        .wt_dir()
        .join("trees")
        .join(&f.tree)
        .join("refs/branches/main");
    std::fs::write(&ref_path, &s1.id).unwrap();
    let err = f.repo.undo(&f.tree, once()).unwrap_err();
    assert!(err.to_string().contains("changed since"), "{err}");
}
