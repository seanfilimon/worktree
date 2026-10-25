//! Snapshot signing through the repository (ENGINEERING_PLAN §13.6).
//!
//! This binary sets `WT_HOME` for its own process, so it holds a single
//! test.

mod common;

use common::{opts, Fixture};
use worktree_sdk::config::SigningSection;
use worktree_sdk::signing::{self, KeyStore, Verification};
use worktree_sdk::{SdkError, SnapshotOptions};

#[test]
fn snapshots_are_signed_per_config_and_flag() {
    let fx = Fixture::new();
    let home = fx.tmp.path().join("home");
    std::env::set_var("WT_HOME", &home);
    let keys = KeyStore::default_location();
    assert_eq!(keys.dir(), home.join("keys"));

    // Unsigned by default.
    fx.write("a.txt", "a\n");
    let plain = fx.snap("plain");
    assert!(plain.signature.is_none());
    assert_eq!(
        signing::verify_with_local_keys(&plain).unwrap(),
        Verification::Unsigned
    );

    // --sign without any key fails before anything is written.
    fx.write("a.txt", "b\n");
    let err = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                sign: true,
                ..opts("signed")
            },
        )
        .unwrap_err();
    assert!(matches!(err, SdkError::InvalidConfig(_)), "{err:?}");
    assert_eq!(fx.repo.resolve(&fx.tree, "HEAD").unwrap().id, plain.id);

    // --sign with the default key.
    keys.generate(signing::DEFAULT_KEY, false).unwrap();
    let signed = fx
        .repo
        .snapshot(
            &fx.tree,
            SnapshotOptions {
                sign: true,
                ..opts("signed")
            },
        )
        .unwrap();
    assert!(signed.signature.is_some());
    // The stored snapshot verifies.
    let stored = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    assert_eq!(stored, signed);
    assert!(signing::verify_with_local_keys(&stored).unwrap().is_valid());

    // [signing] key + auto signs every snapshot, including reverts.
    keys.generate("work", false).unwrap();
    let mut repo = fx.repo;
    let mut cfg = repo.config().clone();
    cfg.signing = SigningSection {
        key: "work".into(),
        auto: true,
    };
    repo.set_config(cfg).unwrap();
    std::fs::write(repo.root().join("a.txt"), "c\n").unwrap();
    let auto = repo.snapshot(&fx.tree, opts("auto")).unwrap();
    let work = keys.info("work").unwrap();
    assert_eq!(auto.signature.as_ref().unwrap().key_id, work.fingerprint);
    match signing::verify_with_local_keys(&auto).unwrap() {
        Verification::Valid { key, .. } => assert_eq!(key, "work"),
        other => panic!("{other:?}"),
    }
    let revert = repo.revert(&fx.tree, &auto.id).unwrap();
    assert!(signing::verify_with_local_keys(&revert).unwrap().is_valid());

    // Tampering with the stored record is detected.
    let mut tampered = revert.clone();
    tampered.message = "rewritten".into();
    assert!(matches!(
        signing::verify_with_local_keys(&tampered).unwrap(),
        Verification::Invalid { .. }
    ));
    std::env::remove_var("WT_HOME");
}
