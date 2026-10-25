//! Sparse and partial clones (ENGINEERING_PLAN §15.4) through
//! `worktree_sdk::remote` against the in-process fake server.
//!
//! The tests use a multi-threaded runtime: lazy downloads block the calling
//! thread while the fake server answers on another worker.

mod remote_fake_server;

use std::collections::HashSet;
use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, FetchOptions};
use worktree_sdk::repo::storage::{FsckOptions, GcOptions};
use worktree_sdk::{
    DiffOptions, InitOptions, MergeOptions, MergeOutcome, ObjectHash, Repository, SdkError,
    Snapshot, SnapshotOptions, SwitchOptions,
};

const USER: &str = "alice";

async fn setup() -> (TestHome, FakeServer) {
    let home = TestHome::new();
    let server = FakeServer::start().await;
    remote::register(&server.url, USER, "alice@example.com", "pw")
        .await
        .unwrap();
    (home, server)
}

fn init_repo(dir: &Path, server: &FakeServer) -> Repository {
    let mut repo = Repository::init(
        dir,
        InitOptions {
            name: Some("proj".into()),
            default_branch: None,
        },
    )
    .unwrap();
    let mut config = repo.config().clone();
    config.remote.insert(
        "origin".into(),
        RemoteSection {
            url: server.url.clone(),
            tenant: USER.into(),
        },
    );
    repo.set_config(config).unwrap();
    repo
}

fn tree(repo: &Repository) -> String {
    repo.root_tree().unwrap().name
}

fn write(repo: &Repository, path: &str, content: &[u8]) {
    let full = repo.root().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn snap(repo: &Repository, message: &str) -> Snapshot {
    let opts = SnapshotOptions {
        message: message.into(),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap()
}

/// ~12 MiB of pseudo-random data (chunked).
fn big_content() -> Vec<u8> {
    let mut state = 0x5eed_u64;
    let mut out = Vec::with_capacity(12 * 1024 * 1024);
    while out.len() < 12 * 1024 * 1024 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(&state.to_le_bytes());
    }
    out
}

fn objects(repo: &Repository) -> HashSet<ObjectHash> {
    repo.store().list().unwrap().into_iter().collect()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sparse_clone_downloads_only_sparse_content_and_fetches_the_rest_on_demand() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let at = tree(&a);
    let big = big_content();
    write(&a, "src/main.rs", b"fn main() {}\n");
    write(&a, "docs/guide.md", b"line 1\nline 2\nline 3\nline 4\n");
    write(&a, "docs/notes.md", b"notes v1\n");
    write(&a, "docs/untouched.md", b"never read by the sparse clone\n");
    write(&a, "assets/big.bin", &big);
    let s1 = snap(&a, "initial");
    write(&a, "docs/notes.md", b"notes v2\n");
    let s2 = snap(&a, "notes v2");
    // A feature branch editing the guide's last line.
    a.switch_branch(
        &at,
        "feature",
        SwitchOptions {
            create: true,
            force: false,
        },
    )
    .unwrap();
    write(
        &a,
        "docs/guide.md",
        b"line 1\nline 2\nline 3\nline 4 (feature)\n",
    );
    snap(&a, "feature: guide");
    remote::push(&a, &at, "origin", None).await.unwrap();
    a.switch_branch(&at, "main", SwitchOptions::default())
        .unwrap();
    remote::push(&a, &at, "origin", None).await.unwrap();
    let full = objects(&a);

    // --- sparse clone: only src/ content.
    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone_sparse(
        &server.url,
        USER,
        "proj",
        b_dir.path(),
        FetchOptions::default(),
        &strings(&["src"]),
    )
    .await
    .unwrap();
    let bt = tree(&b);
    assert!(
        b.store().has_fetcher(),
        "a partial clone downloads on demand"
    );
    assert_eq!(b.partial_remote(&bt).unwrap().as_deref(), Some("origin"));
    assert_eq!(b.sparse_patterns(&bt).unwrap(), Some(strings(&["/src"])));
    assert!(b.root().join("src/main.rs").is_file());
    for p in ["docs/guide.md", "docs/notes.md", "assets/big.bin"] {
        assert!(!b.root().join(p).exists(), "{p} is not checked out");
    }
    let cloned = objects(&b);
    assert!(
        cloned.len() * 3 < full.len(),
        "a sparse clone stores a fraction of the objects ({} of {})",
        cloned.len(),
        full.len()
    );
    let head = b.manifest_of(&b.resolve(&bt, "HEAD").unwrap()).unwrap();
    let big_entry = head.get("assets/big.bin").unwrap().clone();
    assert!(big_entry.chunked);
    assert!(!b.store().has(&big_entry.hash), "chunk list not downloaded");
    assert!(b.store().has(&head.get("src/main.rs").unwrap().hash));
    assert!(b.status(&bt).unwrap().is_clean());

    // fsck: content a partial clone lacks is expected; gc neither deletes
    // reachable objects nor downloads anything.
    let fsck = b.fsck(&FsckOptions::default()).unwrap();
    assert!(fsck.is_ok(), "{:?}", fsck.errors);
    assert!(fsck.restricted_missing >= 3, "{fsck:?}");
    let calls = server.state().download_calls;
    let gc = b
        .gc(&GcOptions {
            prune_days: 0,
            ..GcOptions::default()
        })
        .unwrap();
    assert_eq!(gc.objects_deleted, 0, "{gc:?}");
    assert_eq!(objects(&b), cloned);
    assert_eq!(server.state().download_calls, calls, "gc never downloads");

    // --- diff outside the set downloads lazily (reopened: install on open).
    let b = Repository::open(b.root()).unwrap();
    assert!(b.store().has_fetcher());
    let opts = DiffOptions {
        paths: strings(&["docs"]),
        ..DiffOptions::default()
    };
    let diffs = b.diff_revs(&bt, &s1.id, &s2.id, &opts).unwrap();
    assert_eq!(diffs.len(), 1);
    let patch = diffs[0].patch.as_deref().unwrap();
    assert!(
        patch.contains("-notes v1") && patch.contains("+notes v2"),
        "{patch}"
    );
    assert!(server.state().download_calls > calls);

    // --- sparse add hydrates (chunk list + chunks of the big file).
    let report = b.sparse_add(&bt, &strings(&["assets"])).unwrap();
    assert_eq!(report.written, 1);
    assert!(report.fetched > 2, "{report:?}");
    assert_eq!(std::fs::read(b.root().join("assets/big.bin")).unwrap(), big);
    assert!(b.status(&bt).unwrap().is_clean());

    // --- merge touching paths outside the set fetches lazily.
    write(
        &a,
        "docs/guide.md",
        b"line 1 (main)\nline 2\nline 3\nline 4\n",
    );
    snap(&a, "main: guide");
    remote::push(&a, &at, "origin", None).await.unwrap();
    let pulled = remote::pull(&b, &bt, "origin", None).await.unwrap();
    assert!(matches!(pulled.outcome, MergeOutcome::FastForward { .. }));
    let main_guide = b
        .manifest_of(&b.resolve(&bt, "HEAD").unwrap())
        .unwrap()
        .get("docs/guide.md")
        .unwrap()
        .clone();
    assert!(
        !b.store().has(&main_guide.hash),
        "pull of a sparse tree downloads no content outside the set"
    );
    let outcome = b.merge(&bt, "feature", MergeOptions::default()).unwrap();
    let MergeOutcome::Merged { snapshot: merged } = outcome else {
        panic!("expected a merge, got {outcome:?}");
    };
    let guide = b.read_file_at(&merged, "docs/guide.md").unwrap();
    assert_eq!(guide, b"line 1 (main)\nline 2\nline 3\nline 4 (feature)\n");
    assert!(!b.root().join("docs/guide.md").exists());

    // --- snapshot + push from the sparse clone keeps other paths intact.
    write(&b, "src/main.rs", b"fn main() { println!(\"sparse\"); }\n");
    let tip = snap(&b, "edit from the sparse clone");
    remote::push(&b, &bt, "origin", None).await.unwrap();
    remote::pull(&a, &at, "origin", None).await.unwrap();
    assert_eq!(a.resolve(&at, "HEAD").unwrap().id, tip.id);
    assert_eq!(
        std::fs::read(a.root().join("docs/guide.md")).unwrap(),
        b"line 1 (main)\nline 2\nline 3\nline 4 (feature)\n"
    );
    assert_eq!(
        std::fs::read(a.root().join("docs/notes.md")).unwrap(),
        b"notes v2\n"
    );
    assert_eq!(std::fs::read(a.root().join("assets/big.bin")).unwrap(), big);
    assert!(a.status(&at).unwrap().is_clean());

    // --- offline: a clear error instead of a bare "object not found".
    let mut config = b.config().clone();
    config.remote.get_mut("origin").unwrap().url = "http://127.0.0.1:1".into();
    let mut b = Repository::open(b.root()).unwrap();
    b.set_config(config).unwrap();
    let offline = Repository::open(b.root()).unwrap();
    let err = offline.read_file_at(&s1, "docs/untouched.md").unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, SdkError::Remote(_)), "{err:?}");
    assert!(msg.contains("partial clone"), "{msg}");
    // Local content still works offline.
    assert_eq!(
        offline.read_file_at(&s1, "src/main.rs").unwrap(),
        b"fn main() {}\n"
    );
    assert!(offline.status(&bt).unwrap().is_clean());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sparse_fetch_in_an_existing_clone_makes_it_partial() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let at = tree(&a);
    write(&a, "src/lib.rs", b"pub fn x() {}\n");
    write(&a, "docs/a.md", b"a\n");
    snap(&a, "initial");
    remote::push(&a, &at, "origin", None).await.unwrap();

    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", b_dir.path())
        .await
        .unwrap();
    let bt = tree(&b);
    assert!(!b.is_partial(&bt).unwrap(), "a full clone is not partial");
    assert!(!b.store().has_fetcher());
    b.sparse_set(&bt, &strings(&["src"])).unwrap();
    assert!(!b.root().join("docs/a.md").exists());

    write(&a, "docs/a.md", b"a v2\n");
    write(&a, "src/lib.rs", b"pub fn x() { 2 }\n");
    snap(&a, "both");
    remote::push(&a, &at, "origin", None).await.unwrap();
    remote::pull(&b, &bt, "origin", None).await.unwrap();
    assert_eq!(
        std::fs::read(b.root().join("src/lib.rs")).unwrap(),
        b"pub fn x() { 2 }\n"
    );
    assert!(b.is_partial(&bt).unwrap());
    let head = b.manifest_of(&b.resolve(&bt, "HEAD").unwrap()).unwrap();
    assert!(!b.store().has(&head.get("docs/a.md").unwrap().hash));

    // disable hydrates everything through the lazy fetcher.
    let b = Repository::open(b.root()).unwrap();
    let report = b.sparse_disable(&bt).unwrap();
    assert_eq!(report.written, 1);
    assert!(report.fetched >= 1);
    assert_eq!(
        std::fs::read(b.root().join("docs/a.md")).unwrap(),
        b"a v2\n"
    );
    assert!(b.status(&bt).unwrap().is_clean());
}

#[tokio::test]
async fn invalid_sparse_patterns_fail_before_cloning() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("x");
    let err = remote::clone_sparse(
        &server.url,
        USER,
        "proj",
        &dest,
        FetchOptions::default(),
        &strings(&["../outside"]),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SdkError::InvalidArgument(_)), "{err:?}");
    assert!(!dest.exists());
}
