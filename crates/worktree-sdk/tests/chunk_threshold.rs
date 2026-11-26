//! `[storage] chunk_threshold` (specs/storage/Storage.md): new and modified
//! files use the configured large-file threshold, unchanged files keep the
//! form (chunked or one blob) of their head entry, so changing the setting
//! never shows spurious modifications in status or diff.

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use common::Fixture;
use worktree_sdk::config::ConfigScope;
use worktree_sdk::store::{file_hash_as, hash_bytes, CHUNK_THRESHOLD};
use worktree_sdk::{DiffOptions, ManifestEntry, Repository, SdkError, Snapshot};

const MIB: u64 = 1024 * 1024;

/// Every test in this binary shares one empty `WT_HOME` (no user config).
fn isolate_home() {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("home").keep();
        std::env::set_var("WT_HOME", &dir);
        dir
    });
}

fn fixture() -> Fixture {
    isolate_home();
    Fixture::new()
}

/// `len` pseudo-random bytes (incompressible, several FastCDC chunks).
fn noise(seed: u64, len: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

fn set_threshold(fx: &mut Fixture, value: &str) {
    fx.repo
        .set_config_value(&ConfigScope::Worktree, "storage.chunk_threshold", value)
        .unwrap();
}

fn write(fx: &Fixture, rel: &str, data: &[u8]) {
    fs::write(fx.path(rel), data).unwrap();
}

fn entry(fx: &Fixture, snap: &Snapshot, path: &str) -> ManifestEntry {
    fx.repo
        .manifest_of(snap)
        .unwrap()
        .get(path)
        .unwrap_or_else(|| panic!("{path} not in {}", snap.id))
        .clone()
}

/// Status and diff see no change, and the working manifest holds `expected`.
fn assert_unchanged(fx: &Fixture, expected: &ManifestEntry) {
    let status = fx.repo.status(&fx.tree).unwrap();
    assert!(status.is_clean(), "spurious changes: {:?}", status.changes);
    let diff = fx
        .repo
        .diff_working(&fx.tree, None, &DiffOptions::default())
        .unwrap();
    assert!(diff.is_empty(), "spurious diff: {diff:?}");
    let working = fx.repo.manifest_from_working(&fx.tree).unwrap();
    assert_eq!(working.get(&expected.path), Some(expected));
}

#[test]
fn the_default_threshold_is_8_mib() {
    let fx = fixture();
    assert_eq!(CHUNK_THRESHOLD, 8 * MIB);
    assert_eq!(fx.repo.config().storage.chunk_threshold, 8 * MIB);
    assert_eq!(fx.repo.store().chunk_threshold(), 8 * MIB);
    let text = fs::read_to_string(fx.path(".wt/config.toml")).unwrap();
    assert!(!text.contains("[storage]"), "{text}");
    // A 3 MiB file is one blob by default.
    let data = noise(1, 3 * MIB);
    write(&fx, "big.bin", &data);
    let e = entry(&fx, &fx.snap("big"), "big.bin");
    assert!(!e.chunked);
    assert_eq!(e.hash, hash_bytes(&data));
}

#[test]
fn new_files_use_the_configured_threshold() {
    let mut fx = fixture();
    set_threshold(&mut fx, "2MiB");
    assert_eq!(fx.repo.store().chunk_threshold(), 2 * MIB);
    assert_eq!(
        Repository::open(&fx.root())
            .unwrap()
            .store()
            .chunk_threshold(),
        2 * MIB,
        "loaded on open"
    );
    let above = noise(2, 3 * MIB);
    let at = noise(3, 2 * MIB);
    write(&fx, "above.bin", &above);
    write(&fx, "at.bin", &at);
    let snap = fx.snap("files");
    let e = entry(&fx, &snap, "above.bin");
    assert!(e.chunked, "3 MiB > 2 MiB");
    assert_eq!(e.hash, file_hash_as(&above, true));
    assert!(
        !entry(&fx, &snap, "at.bin").chunked,
        "exactly the threshold"
    );
    assert_eq!(fx.repo.read_file_at(&snap, "above.bin").unwrap(), above);
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

#[test]
fn raising_the_threshold_keeps_unchanged_chunked_files_chunked() {
    let mut fx = fixture();
    set_threshold(&mut fx, "1MiB");
    let data = noise(4, 3 * MIB);
    write(&fx, "big.bin", &data);
    let chunked = entry(&fx, &fx.snap("chunked"), "big.bin");
    assert!(chunked.chunked);

    set_threshold(&mut fx, "8MiB");
    assert_unchanged(&fx, &chunked);
    // Re-written with the same bytes: re-hashed, still unchanged.
    write(&fx, "big.bin", &data);
    assert_unchanged(&fx, &chunked);
    // A fresh scan without the stat cache agrees.
    let reopened = Repository::open(&fx.root()).unwrap();
    assert!(reopened.status(&fx.tree).unwrap().is_clean());

    // New and modified files follow the new threshold.
    let other = noise(5, 3 * MIB);
    write(&fx, "new.bin", &other);
    let mut modified = data.clone();
    modified[42] ^= 0xff;
    write(&fx, "big.bin", &modified);
    let snap = fx.snap("edit");
    let new = entry(&fx, &snap, "new.bin");
    assert!(!new.chunked);
    let big = entry(&fx, &snap, "big.bin");
    assert!(!big.chunked, "a modified file uses the new threshold");
    assert_eq!(big.hash, hash_bytes(&modified));
    assert!(fx.repo.status(&fx.tree).unwrap().is_clean());
}

#[test]
fn lowering_the_threshold_keeps_unchanged_single_blob_files_single() {
    let mut fx = fixture();
    let data = noise(6, 3 * MIB);
    write(&fx, "big.bin", &data);
    let blob = entry(&fx, &fx.snap("blob"), "big.bin");
    assert!(!blob.chunked);

    set_threshold(&mut fx, "1MiB");
    assert_unchanged(&fx, &blob);
    write(&fx, "big.bin", &data);
    assert_unchanged(&fx, &blob);
    // The next snapshot (of another change) records it unchanged.
    fx.write("note.txt", "x\n");
    assert_eq!(entry(&fx, &fx.snap("note"), "big.bin"), blob);

    let mut modified = data.clone();
    modified[7] ^= 1;
    write(&fx, "big.bin", &modified);
    let big = entry(&fx, &fx.snap("edit"), "big.bin");
    assert!(big.chunked, "a modified file uses the new threshold");
    assert_eq!(big.hash, file_hash_as(&modified, true));
}

#[test]
fn a_file_modified_before_the_change_is_rehashed_with_the_new_threshold() {
    let mut fx = fixture();
    let data = noise(7, 3 * MIB);
    write(&fx, "big.bin", &data);
    fx.snap("blob");
    let mut modified = data.clone();
    modified[0] ^= 1;
    write(&fx, "big.bin", &modified);
    // Old enough for the stat cache to trust it (outside the racy window).
    fs::File::options()
        .write(true)
        .open(fx.path("big.bin"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        .unwrap();
    // Stored and cached in the stat index as a modified one-blob file (as
    // staging the working state does)...
    let staged = fx.repo.manifest_from_working(&fx.tree).unwrap();
    assert!(!staged.get("big.bin").unwrap().chunked);
    set_threshold(&mut fx, "1MiB");
    // ...but snapshotted in the form the threshold now asks for.
    let e = entry(&fx, &fx.snap("edit"), "big.bin");
    assert!(e.chunked);
    assert_eq!(e.hash, file_hash_as(&modified, true));
}

#[test]
fn the_threshold_is_validated_and_worktree_only() {
    let mut fx = fixture();
    for bad in ["512KiB", "64MiB", "huge"] {
        let err = fx
            .repo
            .set_config_value(&ConfigScope::Worktree, "storage.chunk_threshold", bad)
            .unwrap_err();
        assert!(matches!(err, SdkError::InvalidConfig(_)), "{bad}: {err}");
    }
    assert_eq!(fx.repo.store().chunk_threshold(), CHUNK_THRESHOLD);
    fx.repo
        .add_tree("svc", std::path::Path::new("services/svc"))
        .unwrap();
    for scope in [ConfigScope::User, ConfigScope::Tree("svc".into())] {
        let err = fx
            .repo
            .set_config_value(&scope, "storage.chunk_threshold", "4MiB")
            .unwrap_err();
        assert!(
            err.to_string().contains("cannot be set"),
            "{scope:?}: {err}"
        );
    }
    set_threshold(&mut fx, "4MiB");
    assert_eq!(
        fx.repo.config().get("storage.chunk_threshold").as_deref(),
        Some("4194304")
    );
    assert!(fx
        .repo
        .unset_config_value(&ConfigScope::Worktree, "storage.chunk_threshold")
        .unwrap());
    assert_eq!(fx.repo.store().chunk_threshold(), CHUNK_THRESHOLD);
    // A hand-edited out-of-range value makes the worktree config invalid.
    let path = fx.path(".wt/config.toml");
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("chunk_threshold"),
        "unset removes the key: {text}"
    );
    let text = if text.contains("[storage]") {
        text.replace("[storage]", "[storage]\nchunk_threshold = \"1GiB\"")
    } else {
        format!("{text}\n[storage]\nchunk_threshold = \"1GiB\"\n")
    };
    fs::write(&path, text).unwrap();
    let err = Repository::open(&fx.root()).unwrap_err();
    assert!(
        matches!(&err, SdkError::InvalidConfig(m) if m.contains("storage.chunk_threshold")),
        "{err}"
    );
}
