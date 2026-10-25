//! Rebased and cherry-picked snapshots are signed again when signing is
//! configured (ENGINEERING_PLAN §14.5).
//!
//! This binary sets `WT_HOME` for its own process, so it holds a single
//! test.

mod common;

use common::Fixture;
use worktree_sdk::config::ConfigScope;
use worktree_sdk::repo::{CherryPickOptions, RebaseOptions, RewriteStatus};
use worktree_sdk::signing::{self, KeyStore, Verification};
use worktree_sdk::SwitchOptions;

#[test]
fn rewritten_snapshots_are_resigned() {
    let mut fx = Fixture::new();
    let home = fx.tmp.path().join("home");
    std::env::set_var("WT_HOME", &home);
    KeyStore::default_location()
        .generate(signing::DEFAULT_KEY, false)
        .unwrap();

    fx.write("a.txt", "1\n2\n");
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
    fx.write("f.txt", "f\n");
    let unsigned = fx.snap("feature work");
    assert!(unsigned.signature.is_none());
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    fx.write("m.txt", "m\n");
    fx.snap("main work");

    for (key, value) in [
        ("signing.key", signing::DEFAULT_KEY),
        ("signing.auto", "true"),
    ] {
        fx.repo
            .set_config_value(&ConfigScope::Worktree, key, value)
            .unwrap();
    }

    let picked = fx
        .repo
        .cherry_pick(
            &fx.tree,
            std::slice::from_ref(&unsigned.id),
            &CherryPickOptions::default(),
        )
        .unwrap();
    assert_eq!(picked.status, RewriteStatus::Completed);
    let copy = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    assert!(copy.signature.is_some());
    assert!(matches!(
        signing::verify_with_local_keys(&copy).unwrap(),
        Verification::Valid { .. }
    ));

    fx.repo
        .switch_branch(&fx.tree, "feature", SwitchOptions::default())
        .unwrap();
    fx.repo
        .set_branch_tip(&fx.tree, "main", Some(&copy.parents[0]))
        .unwrap();
    let out = fx
        .repo
        .rebase(&fx.tree, "main", &RebaseOptions::default())
        .unwrap();
    assert_eq!(out.status, RewriteStatus::Completed);
    let rebased = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    assert_ne!(rebased.id, unsigned.id);
    assert!(matches!(
        signing::verify_with_local_keys(&rebased).unwrap(),
        Verification::Valid { .. }
    ));
}
