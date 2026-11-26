//! The local hook phases around merges and branch switches: `pre-merge`,
//! `post-merge` and `post-switch` (the push hooks and the merge hooks of
//! `pull` are in `remote_hooks.rs`).

mod common;
mod hook_util;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use common::Fixture;
use hook_util::{
    clear_records, hook_env, hook_file, hook_log, hook_stdin, remove_hook, script, write_hook,
};
use worktree_sdk::config::ConfigScope;
use worktree_sdk::{MergeOptions, MergeOutcome, SwitchOptions};

/// Every test in this binary shares one empty `WT_HOME` (no user config).
fn isolate_home() {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("home").keep();
        std::env::set_var("WT_HOME", &dir);
        dir
    });
}

/// `main` with `f.txt = "base"` (s1) and `feature` one snapshot ahead with
/// `f.txt = "feature"` (s2); `main` is checked out. Returns (s1, s2).
fn diverging_fixture() -> (Fixture, String, String) {
    isolate_home();
    let fx = Fixture::new();
    fx.write("f.txt", "base\n");
    let s1 = fx.snap("base").id;
    fx.repo.create_branch(&fx.tree, "feature", None).unwrap();
    switch(&fx, "feature");
    fx.write("f.txt", "feature\n");
    let s2 = fx.snap("feature work").id;
    switch(&fx, "main");
    (fx, s1, s2)
}

fn switch(fx: &Fixture, name: &str) {
    fx.repo
        .switch_branch(&fx.tree, name, SwitchOptions::default())
        .unwrap();
}

fn tip(fx: &Fixture, branch: &str) -> Option<String> {
    fx.repo.branch_tip(&fx.tree, branch).unwrap()
}

fn no_ff() -> MergeOptions {
    MergeOptions {
        no_fast_forward: true,
        ..MergeOptions::default()
    }
}

#[test]
fn merge_hooks_run_around_a_fast_forward_with_their_environment() {
    let (fx, s1, s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "pre-merge", script::RECORD);
    write_hook(&root, "post-merge", script::RECORD);

    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert_eq!(outcome, MergeOutcome::FastForward { to: s2.clone() });
    assert_eq!(hook_log(&root), ["pre-merge", "post-merge"]);

    let pre = hook_env(&root, "pre-merge");
    assert_eq!(pre["WT_HOOK"], "pre-merge");
    assert_eq!(pre["WT_HOOK_PHASE"], "pre-merge");
    assert_eq!(pre["WT_TREE"], fx.tree);
    assert_eq!(pre["WT_BRANCH"], "main");
    assert_eq!(pre["WT_MERGE_SOURCE"], "feature");
    assert_eq!(pre["WT_MERGE_SOURCE_TIP"], s2);
    assert_eq!(pre["WT_MERGE_OLD_TIP"], s1);
    assert_eq!(pre["WT_MERGE_KIND"], "fast-forward");
    assert!(!pre.contains_key("WT_REMOTE"), "{pre:?}");
    assert!(!pre.contains_key("WT_MERGE_RESULT"), "{pre:?}");
    assert_eq!(hook_file(&root, "pre-merge"), "base\n", "before the merge");

    let post = hook_env(&root, "post-merge");
    assert_eq!(post["WT_MERGE_KIND"], "fast-forward");
    assert_eq!(post["WT_MERGE_RESULT"], "fast-forward");
    assert_eq!(post["WT_MERGE_CONFLICTS"], "0");
    assert_eq!(post["WT_MERGE_NEW_TIP"], s2);
    assert_eq!(
        hook_file(&root, "post-merge"),
        "feature\n",
        "after the merge"
    );
    assert_eq!(hook_stdin(&root, "post-merge"), "");

    // Nothing to merge: no hooks.
    clear_records(&root);
    let again = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert_eq!(again, MergeOutcome::UpToDate);
    assert!(hook_log(&root).is_empty());
}

#[test]
fn post_merge_reports_a_merge_snapshot() {
    let (fx, s1, s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "post-merge", script::RECORD);
    let MergeOutcome::Merged { snapshot } = fx.repo.merge(&fx.tree, "feature", no_ff()).unwrap()
    else {
        panic!("expected a merge snapshot");
    };
    let post = hook_env(&root, "post-merge");
    assert_eq!(post["WT_MERGE_KIND"], "merge");
    assert_eq!(post["WT_MERGE_RESULT"], "merged");
    assert_eq!(post["WT_MERGE_OLD_TIP"], s1);
    assert_eq!(post["WT_MERGE_SOURCE_TIP"], s2);
    assert_eq!(post["WT_MERGE_NEW_TIP"], snapshot.id);
    assert_eq!(post["WT_MERGE_CONFLICTS"], "0");
}

#[test]
fn post_merge_knows_about_conflicts() {
    let (fx, _s1, s2) = diverging_fixture();
    let root = fx.root();
    fx.write("f.txt", "main\n");
    let main_tip = fx.snap("main work").id;
    write_hook(&root, "pre-merge", script::RECORD);
    write_hook(&root, "post-merge", script::RECORD);

    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert!(
        matches!(&outcome, MergeOutcome::Conflicts { conflicts } if conflicts.len() == 1),
        "{outcome:?}"
    );
    assert_eq!(hook_log(&root), ["pre-merge", "post-merge"]);
    assert_eq!(hook_env(&root, "pre-merge")["WT_MERGE_KIND"], "merge");
    let post = hook_env(&root, "post-merge");
    assert_eq!(post["WT_MERGE_RESULT"], "conflicts");
    assert_eq!(post["WT_MERGE_CONFLICTS"], "1");
    assert_eq!(post["WT_MERGE_SOURCE_TIP"], s2);
    assert_eq!(
        post["WT_MERGE_NEW_TIP"], main_tip,
        "the branch did not move"
    );
    assert_eq!(hook_stdin(&root, "post-merge"), "f.txt\n");
    assert!(
        hook_file(&root, "post-merge").contains("<<<<<<<"),
        "the hook sees the conflict markers"
    );
}

#[test]
fn failing_pre_merge_aborts_with_nothing_changed_and_skip_hooks_bypasses() {
    let (fx, s1, s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "pre-merge", script::FAIL);
    write_hook(&root, "post-merge", script::RECORD);
    let reflog_before = fx.repo.reflog(&fx.tree, None).unwrap().len();

    for opts in [MergeOptions::default(), no_ff()] {
        let msg = fx
            .repo
            .merge(&fx.tree, "feature", opts)
            .unwrap_err()
            .to_string();
        assert!(msg.starts_with("pre-merge hook failed:"), "{msg}");
        assert!(msg.contains("exit code 7"), "{msg}");
        assert!(msg.contains("blocked by hook"), "{msg}");
    }
    assert_eq!(tip(&fx, "main"), Some(s1.clone()), "branch unchanged");
    assert_eq!(fx.read("f.txt"), "base\n", "working directory unchanged");
    assert!(fx.repo.merge_state(&fx.tree).unwrap().is_none());
    assert_eq!(fx.repo.reflog(&fx.tree, None).unwrap().len(), reflog_before);
    assert_eq!(hook_log(&root), ["pre-merge", "pre-merge"], "no post-merge");

    clear_records(&root);
    let outcome = fx
        .repo
        .merge(
            &fx.tree,
            "feature",
            MergeOptions {
                skip_hooks: true,
                ..MergeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(outcome, MergeOutcome::FastForward { to: s2 });
    assert!(hook_log(&root).is_empty(), "skip_hooks skips both phases");
}

#[test]
fn failing_post_merge_keeps_the_merge() {
    let (fx, _s1, _s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "post-merge", script::FAIL);
    let MergeOutcome::Merged { snapshot } = fx.repo.merge(&fx.tree, "feature", no_ff()).unwrap()
    else {
        panic!("expected a merge snapshot");
    };
    assert_eq!(hook_log(&root), ["post-merge"]);
    assert_eq!(tip(&fx, "main"), Some(snapshot.id));
    assert_eq!(fx.read("f.txt"), "feature\n");
}

#[test]
fn pre_merge_timeout_aborts() {
    let (mut fx, s1, _s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "pre-merge", script::SLEEP);
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "hooks.timeout_secs", "1")
        .unwrap();
    let started = Instant::now();
    let msg = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap_err()
        .to_string();
    assert!(msg.starts_with("pre-merge hook failed:"), "{msg}");
    assert!(msg.contains("timed out after 1s"), "{msg}");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "hook was not killed: {:?}",
        started.elapsed()
    );
    assert_eq!(tip(&fx, "main"), Some(s1));
    assert_eq!(fx.read("f.txt"), "base\n");
}

#[test]
fn post_switch_runs_after_the_switch_with_both_branches() {
    let (fx, s1, s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "post-switch", script::RECORD);

    switch(&fx, "feature");
    assert_eq!(hook_log(&root), ["post-switch"]);
    let env = hook_env(&root, "post-switch");
    assert_eq!(env["WT_HOOK"], "post-switch");
    assert_eq!(env["WT_BRANCH"], "feature");
    assert_eq!(env["WT_SWITCH_FROM"], "main");
    assert_eq!(env["WT_SWITCH_TO"], "feature");
    assert_eq!(env["WT_SWITCH_OLD_TIP"], s1);
    assert_eq!(env["WT_SWITCH_NEW_TIP"], s2);
    assert_eq!(env["WT_SWITCH_CREATED"], "0");
    assert_eq!(
        hook_file(&root, "post-switch"),
        "feature\n",
        "after the switch"
    );

    // Staying on the current branch does nothing, so no hook runs.
    clear_records(&root);
    switch(&fx, "feature");
    assert!(hook_log(&root).is_empty());

    fx.repo
        .switch_branch(
            &fx.tree,
            "topic",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    let env = hook_env(&root, "post-switch");
    assert_eq!(env["WT_SWITCH_FROM"], "feature");
    assert_eq!(env["WT_SWITCH_TO"], "topic");
    assert_eq!(env["WT_SWITCH_CREATED"], "1");
    assert_eq!(env["WT_SWITCH_OLD_TIP"], s2);
    assert_eq!(env["WT_SWITCH_NEW_TIP"], s2);
}

#[test]
fn post_switch_failure_and_timeout_keep_the_switch() {
    let (mut fx, _s1, _s2) = diverging_fixture();
    let root = fx.root();
    write_hook(&root, "post-switch", script::FAIL);
    switch(&fx, "feature");
    assert_eq!(hook_log(&root), ["post-switch"]);
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "feature");
    assert_eq!(fx.read("f.txt"), "feature\n");

    write_hook(&root, "post-switch", script::SLEEP);
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "hooks.timeout_secs", "1")
        .unwrap();
    let started = Instant::now();
    switch(&fx, "main");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(fx.repo.current_branch(&fx.tree).unwrap(), "main");
    assert_eq!(fx.read("f.txt"), "base\n");

    remove_hook(&root, "post-switch");
    switch(&fx, "feature");
}

#[test]
fn post_switch_with_autostash_runs_after_the_stash_is_restored() {
    let (fx, _s1, _s2) = diverging_fixture();
    let root = fx.root();
    fx.write("f.txt", "uncommitted\n");
    let left = fx
        .repo
        .switch_branch_autostash(&fx.tree, "feature", SwitchOptions::default())
        .unwrap();
    assert!(left.stashed.is_some());

    write_hook(&root, "post-switch", script::RECORD);
    let back = fx
        .repo
        .switch_branch_autostash(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    assert!(back.restored.is_some());
    assert_eq!(hook_log(&root), ["post-switch"]);
    assert_eq!(
        hook_file(&root, "post-switch"),
        "uncommitted\n",
        "the hook sees the restored changes"
    );
}

#[test]
fn hooks_disabled_in_config_skip_the_new_phases() {
    let (mut fx, _s1, _s2) = diverging_fixture();
    let root = fx.root();
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "hooks.enabled", "false")
        .unwrap();
    write_hook(&root, "pre-merge", script::FAIL);
    write_hook(&root, "post-switch", script::FAIL);
    switch(&fx, "feature");
    switch(&fx, "main");
    fx.repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert!(hook_log(&root).is_empty());
}
