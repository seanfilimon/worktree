//! The stash (ENGINEERING_PLAN §14.5): push/list/show/apply/pop/drop/clear,
//! untracked files, three-way apply, GC roots and the `on_branch_switch =
//! "stash"` branch switch.

mod common;

use common::Fixture;
use worktree_sdk::repo::StashOptions;
use worktree_sdk::{ChangeKind, ConflictKind, DiffOptions, SdkError, SwitchOptions};

fn push(fx: &Fixture, message: Option<&str>, untracked: bool) -> worktree_sdk::repo::StashEntry {
    fx.repo
        .stash_push(
            &fx.tree,
            &StashOptions {
                message: message.map(str::to_string),
                include_untracked: untracked,
                auto: false,
            },
        )
        .unwrap()
}

fn base(fx: &Fixture) {
    fx.write("a.txt", "1\n2\n3\n");
    fx.write("b.txt", "b\n");
    fx.snap("base");
}

#[test]
fn push_resets_tracked_changes_and_pop_restores_them() {
    let fx = Fixture::new();
    base(&fx);
    fx.write("a.txt", "1\nTWO\n3\n");
    fx.remove("b.txt");
    fx.write("new.txt", "new\n");

    let entry = push(&fx, None, false);
    assert!(
        entry.message.starts_with("WIP on main: "),
        "{}",
        entry.message
    );
    assert_eq!(entry.branch, "main");
    assert!(!entry.include_untracked);
    let kinds: Vec<_> = entry
        .changes
        .iter()
        .map(|c| (c.path.as_str(), c.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("a.txt", ChangeKind::Modified),
            ("b.txt", ChangeKind::Deleted)
        ]
    );
    // Tracked files are back to HEAD; the new file stays.
    assert_eq!(fx.read("a.txt"), "1\n2\n3\n");
    assert_eq!(fx.read("b.txt"), "b\n");
    assert_eq!(fx.read("new.txt"), "new\n");
    let status = fx.repo.status(&fx.tree).unwrap();
    assert_eq!(status.changes.len(), 1);

    let list = fx.repo.stash_list(&fx.tree).unwrap();
    assert_eq!(list, vec![entry.clone()]);
    let (shown, diffs) = fx
        .repo
        .stash_show(&fx.tree, 0, &DiffOptions::default())
        .unwrap();
    assert_eq!(shown.id, entry.id);
    assert_eq!(diffs.len(), 2);
    assert!(diffs[0].patch.as_deref().unwrap().contains("+TWO"));

    let out = fx.repo.stash_pop(&fx.tree, 0).unwrap();
    assert!(out.dropped);
    assert!(out.conflicts.is_empty());
    assert_eq!(out.changed, vec!["a.txt".to_string(), "b.txt".to_string()]);
    assert_eq!(fx.read("a.txt"), "1\nTWO\n3\n");
    assert!(!fx.exists("b.txt"));
    assert_eq!(fx.read("new.txt"), "new\n");
    assert!(fx.repo.stash_list(&fx.tree).unwrap().is_empty());
    assert!(!fx.repo.wt_dir().join("trees/demo/stash.jsonl").exists());
}

#[test]
fn include_untracked_stashes_new_files() {
    let fx = Fixture::new();
    base(&fx);
    fx.write("new.txt", "new\n");
    let err = fx
        .repo
        .stash_push(&fx.tree, &StashOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("--include-untracked"), "{err}");

    let entry = push(&fx, Some("with new"), true);
    assert_eq!(entry.message, "with new");
    assert!(!fx.exists("new.txt"));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
    fx.repo.stash_apply(&fx.tree, 0).unwrap();
    assert_eq!(fx.read("new.txt"), "new\n");
    // apply keeps the entry.
    assert_eq!(fx.repo.stash_list(&fx.tree).unwrap().len(), 1);

    // Nothing to stash on a clean tree.
    fx.remove("new.txt");
    let err = fx
        .repo
        .stash_push(&fx.tree, &StashOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("no local changes"), "{err}");
}

#[test]
fn entries_are_numbered_newest_first_and_drop_clear() {
    let fx = Fixture::new();
    base(&fx);
    fx.write("a.txt", "first\n");
    let first = push(&fx, Some("first"), false);
    fx.write("b.txt", "second\n");
    let second = push(&fx, Some("second"), false);
    let list = fx.repo.stash_list(&fx.tree).unwrap();
    assert_eq!(list[0].id, second.id);
    assert_eq!(list[1].id, first.id);
    assert_eq!(fx.repo.stash_get(&fx.tree, 1).unwrap().id, first.id);
    assert!(fx.repo.stash_get(&fx.tree, 2).is_err());

    // Applying an older entry.
    let out = fx.repo.stash_pop(&fx.tree, 1).unwrap();
    assert_eq!(out.entry.id, first.id);
    assert_eq!(fx.read("a.txt"), "first\n");
    assert_eq!(fx.repo.stash_list(&fx.tree).unwrap()[0].id, second.id);

    let dropped = fx.repo.stash_drop(&fx.tree, 0).unwrap();
    assert_eq!(dropped.id, second.id);
    assert!(fx.repo.stash_list(&fx.tree).unwrap().is_empty());
    assert!(matches!(
        fx.repo.stash_drop(&fx.tree, 0).unwrap_err(),
        SdkError::InvalidArgument(_)
    ));

    fx.repo.restore(&fx.tree, &[], None).unwrap();
    fx.write("a.txt", "x\n");
    push(&fx, None, false);
    fx.write("a.txt", "y\n");
    push(&fx, None, false);
    assert_eq!(fx.repo.stash_clear(&fx.tree).unwrap(), 2);
    assert!(fx.repo.stash_list(&fx.tree).unwrap().is_empty());
}

#[test]
fn apply_merges_three_way_onto_a_moved_head() {
    let fx = Fixture::new();
    base(&fx);
    fx.write("a.txt", "ONE\n2\n3\n");
    push(&fx, None, false);
    // HEAD moves on and edits another line of the same file.
    fx.write("a.txt", "1\n2\nTHREE\n");
    fx.snap("edit line three");
    let out = fx.repo.stash_pop(&fx.tree, 0).unwrap();
    assert!(out.conflicts.is_empty());
    assert!(out.dropped);
    assert_eq!(fx.read("a.txt"), "ONE\n2\nTHREE\n");
}

#[test]
fn conflicting_apply_writes_markers_and_pop_keeps_the_entry() {
    let fx = Fixture::new();
    base(&fx);
    fx.write("a.txt", "1\nstash\n3\n");
    push(&fx, Some("mine"), false);
    fx.write("a.txt", "1\nhead\n3\n");
    fx.snap("conflicting");
    let out = fx.repo.stash_pop(&fx.tree, 0).unwrap();
    assert!(!out.dropped);
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].kind, ConflictKind::Content);
    let text = fx.read("a.txt");
    assert!(text.contains("<<<<<<< ours (working tree)"), "{text}");
    assert!(text.contains(">>>>>>> theirs (stash@{0}: mine)"), "{text}");
    assert_eq!(fx.repo.stash_list(&fx.tree).unwrap().len(), 1);
    // No merge state: the user fixes the file and snapshots normally.
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
}

#[test]
fn stash_roots_keep_both_manifests() {
    let fx = Fixture::new();
    base(&fx);
    assert!(fx.repo.stash_roots().unwrap().is_empty());
    fx.write("a.txt", "changed\n");
    let entry = push(&fx, None, false);
    let roots = fx.repo.stash_roots().unwrap();
    assert!(roots.contains(&entry.manifest));
    assert!(roots.contains(&entry.base_manifest));
    let head = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    assert_eq!(entry.base_manifest, head.manifest);
    // The stashed manifest is readable from the store.
    let stashed = fx.repo.read_manifest(&entry.manifest).unwrap();
    assert!(stashed.get("a.txt").is_some());
}

#[test]
fn stash_is_refused_during_a_merge() {
    let fx = Fixture::new();
    base(&fx);
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
    fx.write("a.txt", "1\nfeature\n3\n");
    fx.snap("feature");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    fx.write("a.txt", "1\nmain\n3\n");
    fx.snap("main");
    fx.repo
        .merge(&fx.tree, "feature", Default::default())
        .unwrap();
    assert!(matches!(
        fx.repo
            .stash_push(&fx.tree, &StashOptions::default())
            .unwrap_err(),
        SdkError::MergeInProgress
    ));
}

#[test]
fn switch_with_autostash_stashes_on_leave_and_pops_on_return() {
    let fx = Fixture::new();
    base(&fx);
    fx.repo.create_branch(&fx.tree, "feature", None).unwrap();
    fx.write("feature-base.txt", "x\n");
    fx.snap("main moves on");

    // Dirty main (a modification and a new file), switch away.
    fx.write("a.txt", "1\nwip\n3\n");
    fx.write("scratch.txt", "scratch\n");
    let report = fx
        .repo
        .switch_branch_autostash(&fx.tree, "feature", SwitchOptions::default())
        .unwrap();
    let stashed = report.stashed.expect("stashed");
    assert_eq!(stashed.message, "auto-stash on main");
    assert!(stashed.auto && stashed.include_untracked);
    assert!(report.restored.is_none());
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "feature");
    assert_eq!(fx.read("a.txt"), "1\n2\n3\n");
    assert!(!fx.exists("scratch.txt"));
    assert!(!fx.exists("feature-base.txt"));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());

    // Work on feature, dirty it too, and go back.
    fx.write("b.txt", "feature wip\n");
    let report = fx
        .repo
        .switch_branch_autostash(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    assert_eq!(
        report.stashed.as_ref().map(|e| e.message.as_str()),
        Some("auto-stash on feature")
    );
    let restored = report.restored.expect("restored main's auto-stash");
    assert!(restored.dropped);
    assert_eq!(restored.entry.id, stashed.id);
    assert_eq!(fx.read("a.txt"), "1\nwip\n3\n");
    assert_eq!(fx.read("scratch.txt"), "scratch\n");
    assert_eq!(fx.read("b.txt"), "b\n");
    assert_eq!(fx.read("feature-base.txt"), "x\n");

    // Back to feature: its auto-stash comes back.
    fx.repo
        .stash_push(
            &fx.tree,
            &StashOptions {
                message: Some("manual".into()),
                include_untracked: true,
                auto: false,
            },
        )
        .unwrap();
    let report = fx
        .repo
        .switch_branch_autostash(&fx.tree, "feature", SwitchOptions::default())
        .unwrap();
    assert!(report.stashed.is_none(), "clean tree: nothing stashed");
    assert_eq!(fx.read("b.txt"), "feature wip\n");
    let left: Vec<String> = fx
        .repo
        .stash_list(&fx.tree)
        .unwrap()
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert_eq!(left, vec!["manual".to_string()]);

    // A failing switch puts the changes back.
    fx.write("b.txt", "again\n");
    assert!(fx
        .repo
        .switch_branch_autostash(&fx.tree, "missing", SwitchOptions::default())
        .is_err());
    assert_eq!(fx.read("b.txt"), "again\n");
    assert_eq!(fx.repo.stash_list(&fx.tree).unwrap().len(), 1);
}
