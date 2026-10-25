//! History rewriting and stash (ENGINEERING_PLAN §14.5), local only:
//! `wt cherry-pick` across branches (and skipping an already-applied
//! change), `wt rebase` onto main with a conflict resolved through `wt merge
//! resolve --file` and `wt rebase --continue`, `wt stash push/list/show/pop`
//! (with untracked files), and the automatic stash of
//! `auto_snapshot.on_branch_switch = "stash"`.

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, ids, messages, pretty, str_field};
use worktree_e2e::{fsutil, require_e2e, Repo, TestEnv};

/// Stash entries (`{id, message, branch, ...}`) in `wt --json stash list`.
fn stash_entries(repo: &Repo) -> Vec<Value> {
    let v = repo.wt().args(["stash", "list"]).json();
    find_objects(&v, &["id", "message", "branch"])
        .into_iter()
        .cloned()
        .collect()
}

#[test]
fn rewrite() {
    require_e2e!();
    let env = TestEnv::new("rewrite");
    let alice = env.user("alice");
    let a = alice.init("proj");
    let base = a.commit(
        &[
            ("base.txt", "base\n"),
            ("conflict.txt", "alpha\nshared line\nomega\n"),
        ],
        "base",
    );

    // --- cherry-pick: one snapshot of `side` onto main
    a.wt().args(["switch", "-c", "side"]).ok();
    let p1 = a.commit(&[("pick.txt", "picked\n")], "side: add pick.txt");
    let p2 = a.commit(&[("base.txt", "base edited on side\n")], "side: edit base");
    a.wt().args(["switch", "main"]).ok();
    let main0 = a.commit(&[("main.txt", "main only\n")], "main: own work");
    a.wt().args(["cherry-pick", &p1]).ok();
    let head = a.head();
    let picked = str_field(&head, "id");
    assert_ne!(picked, p1, "cherry-pick creates a new snapshot");
    assert_eq!(str_field(&head, "message"), "side: add pick.txt");
    assert_eq!(json::parents(&head), vec![main0.clone()]);
    assert_eq!(a.read("pick.txt"), "picked\n");
    assert_eq!(a.read("base.txt"), "base\n", "only the picked change");
    a.assert_clean();
    // Picking the same change again is a no-op.
    a.wt().args(["cherry-pick", &p1]).ok();
    assert_eq!(a.head_id(), picked, "an already-applied change is skipped");
    // Several revisions, by branch name: `side` = p2.
    a.wt().args(["cherry-pick", "side"]).ok();
    assert_eq!(a.read("base.txt"), "base edited on side\n");
    assert_eq!(
        messages(&a.log(&["-n", "2"])),
        vec![
            "side: edit base".to_string(),
            "side: add pick.txt".to_string()
        ]
    );
    assert_ne!(a.head_id(), p2);
    // Dirty working trees are refused.
    a.write("main.txt", "dirty\n");
    a.wt().args(["cherry-pick", &p1]).fails();
    a.write("main.txt", "main only\n");
    a.assert_clean();

    // --- rebase topic onto main with a conflict
    a.wt()
        .args(["branch", "create", "topic", "--from", &base])
        .ok();
    a.wt().args(["switch", "topic"]).ok();
    assert_eq!(a.head_id(), base, "topic starts at base");
    let t1 = a.commit(
        &[("conflict.txt", "alpha\ntopic line\nomega\n")],
        "topic: change shared line",
    );
    let t2 = a.commit(&[("topic.txt", "topic file\n")], "topic: add file");
    a.wt().args(["switch", "main"]).ok();
    let main1 = a.commit(
        &[("conflict.txt", "alpha\nmain line\nomega\n")],
        "main: change shared line",
    );
    a.wt().args(["switch", "topic"]).ok();
    let out = a.wt().args(["rebase", "main"]).code(3);
    assert!(out.mentions("conflict"), "{out}");
    let status = a.wt().args(["merge", "status"]).json();
    assert!(
        status.to_string().contains("conflict.txt"),
        "merge status lists the conflict:\n{}",
        pretty(&status)
    );
    let text = a.read("conflict.txt");
    assert!(
        text.contains("<<<<<<<") && text.contains("main line") && text.contains("topic line"),
        "conflict markers:\n{text}"
    );
    // `--continue` with unresolved conflicts is refused.
    a.wt().args(["rebase", "--continue"]).fails();
    let resolution = env.path("resolution.txt");
    fsutil::write(&resolution, "alpha\nmain line + topic line\nomega\n");
    a.wt()
        .args(["merge", "resolve", "conflict.txt", "--file"])
        .arg(&resolution)
        .ok();
    a.wt().args(["rebase", "--continue"]).ok();
    assert_eq!(a.current_branch(), "topic");
    let log = a.log(&["-n", "3"]);
    assert_eq!(
        messages(&log),
        vec![
            "topic: add file".to_string(),
            "topic: change shared line".to_string(),
            "main: change shared line".to_string()
        ],
        "{}",
        pretty(&Value::Array(log.clone()))
    );
    let new_ids = ids(&log);
    assert_eq!(new_ids[2], main1, "topic now sits on main");
    assert!(
        new_ids[0] != t2 && new_ids[1] != t1,
        "rebased snapshots get new ids"
    );
    assert_eq!(json::parents(&log[1]), vec![main1.clone()]);
    assert_eq!(
        a.read("conflict.txt"),
        "alpha\nmain line + topic line\nomega\n"
    );
    assert_eq!(a.read("topic.txt"), "topic file\n");
    a.assert_clean();
    let reflog = a.wt().args(["reflog"]).ok();
    assert!(
        reflog.mentions("rebase"),
        "reflog records the rebase\n{reflog}"
    );
    // main can now fast-forward to topic.
    a.wt().args(["switch", "main"]).ok();
    a.wt().args(["merge", "topic"]).ok();
    assert_eq!(a.head_id(), new_ids[0]);

    // --- a conflicting cherry-pick: --abort, then resolve and --continue
    a.wt()
        .args(["branch", "create", "other", "--from", &base])
        .ok();
    a.wt().args(["switch", "other"]).ok();
    let o1 = a.commit(
        &[("conflict.txt", "alpha\nother line\nomega\n")],
        "other: change shared line",
    );
    a.wt().args(["switch", "main"]).ok();
    let main_head = a.head_id();
    a.wt().args(["cherry-pick", &o1]).code(3);
    assert!(a.read("conflict.txt").contains("<<<<<<<"));
    a.wt().args(["cherry-pick", "--abort"]).ok();
    assert_eq!(a.head_id(), main_head);
    assert_eq!(
        a.read("conflict.txt"),
        "alpha\nmain line + topic line\nomega\n"
    );
    a.assert_clean();
    a.wt().args(["cherry-pick", &o1]).code(3);
    a.wt()
        .args(["merge", "resolve", "conflict.txt", "--theirs"])
        .ok();
    a.wt().args(["cherry-pick", "--continue"]).ok();
    let head = a.head();
    assert_eq!(str_field(&head, "message"), "other: change shared line");
    assert_eq!(json::parents(&head), vec![main_head.clone()]);
    assert_eq!(a.read("conflict.txt"), "alpha\nother line\nomega\n");
    a.assert_clean();

    // --- stash push / list / show / pop
    a.write("base.txt", "work in progress\n");
    a.write("new.txt", "untracked\n");
    a.wt()
        .args(["stash", "push", "-m", "my wip", "--include-untracked"])
        .ok();
    a.assert_clean();
    assert!(!a.exists("new.txt"), "untracked file stashed and removed");
    assert_eq!(a.read("base.txt"), "base edited on side\n");
    let entries = stash_entries(&a);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(str_field(&entries[0], "message"), "my wip");
    assert_eq!(str_field(&entries[0], "branch"), "main");
    let show = a.wt().args(["stash", "show"]).ok();
    assert!(
        show.mentions("base.txt") && show.mentions("new.txt"),
        "{show}"
    );
    a.wt().args(["stash", "pop"]).ok();
    assert_eq!(a.read("base.txt"), "work in progress\n");
    assert_eq!(a.read("new.txt"), "untracked\n");
    assert!(stash_entries(&a).is_empty(), "pop drops the entry");
    // Without --include-untracked new files stay in place.
    a.wt().arg("stash").ok();
    assert_eq!(a.read("base.txt"), "base edited on side\n");
    assert_eq!(a.read("new.txt"), "untracked\n");
    a.wt().args(["stash", "apply", "0"]).ok();
    assert_eq!(a.read("base.txt"), "work in progress\n");
    assert_eq!(stash_entries(&a).len(), 1, "apply keeps the entry");
    a.wt().args(["stash", "drop", "0"]).ok();
    assert!(stash_entries(&a).is_empty());
    a.remove("new.txt");
    a.write("base.txt", "base edited on side\n");
    a.assert_clean();

    // --- auto-stash on branch switch
    a.wt()
        .args(["config", "set", "auto_snapshot.on_branch_switch", "stash"])
        .ok();
    a.write("main.txt", "dirty on main\n");
    a.wt().args(["switch", "side"]).ok();
    assert_eq!(a.current_branch(), "side");
    a.assert_clean();
    assert!(!a.exists("main.txt"), "side has no main.txt");
    let entries = stash_entries(&a);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(str_field(&entries[0], "message"), "auto-stash on main");
    assert_eq!(entries[0]["auto"], Value::Bool(true));
    a.wt().args(["switch", "main"]).ok();
    assert_eq!(
        a.read("main.txt"),
        "dirty on main\n",
        "switching back pops the auto-stash"
    );
    assert!(stash_entries(&a).is_empty());
    assert_eq!(
        a.changes(),
        vec![("main.txt".to_string(), "modified".to_string())]
    );
}
