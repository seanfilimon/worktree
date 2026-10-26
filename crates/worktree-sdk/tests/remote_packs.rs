//! pack-delta (ENGINEERING_PLAN §16.1): push, fetch and clone over packs,
//! resume of an interrupted upload, the `Unimplemented` fallback and
//! `WT_NO_PACK`, against the in-process fake server.

mod remote_fake_server;

use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote;
use worktree_sdk::{InitOptions, Repository, Snapshot, SnapshotOptions};

const USER: &str = "alice";

async fn setup(packs: bool) -> (TestHome, FakeServer) {
    let home = TestHome::new();
    let server = if packs {
        FakeServer::start_with_packs().await
    } else {
        FakeServer::start().await
    };
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

/// Pseudo-random, incompressible content.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed | 1;
    (0..len)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

fn edited(base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    let mid = v.len() / 2;
    v[mid..mid + 12].copy_from_slice(b"small edit!!");
    v
}

#[tokio::test]
async fn push_uses_packs_and_deltas_against_the_server_tip() {
    let (_home, server) = setup(true).await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    let files: Vec<(String, Vec<u8>)> = (0..6)
        .map(|i| (format!("data/f{i}.bin"), noise(2 * i + 1, 64 * 1024)))
        .collect();
    for (p, d) in &files {
        write(&a, p, d);
    }
    snap(&a, "initial");
    let (report, stats) = remote::push_with_stats(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(stats.pack, "{stats:?}");
    assert_eq!(stats.objects, report.objects_sent);
    assert_eq!(stats.objects, 7, "6 blobs + manifest: {stats:?}");
    {
        let st = server.state();
        assert_eq!(st.pack_upload_calls, 1);
        assert_eq!(st.upload_calls, 0, "no whole-object uploads");
    }

    // Small edits of two files: deltas against the pushed versions.
    write(&a, &files[0].0, &edited(&files[0].1));
    write(&a, &files[3].0, &edited(&files[3].1));
    snap(&a, "edit");
    let (report, stats) = remote::push_with_stats(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(!report.up_to_date);
    assert_eq!(stats.objects, 3, "{stats:?}");
    assert!(stats.deltas >= 2, "{stats:?}");
    assert!(
        stats.wire_bytes * 20 < stats.object_bytes,
        "a delta pack is a fraction of the objects: {stats:?}"
    );
    assert!(server.state().last_upload_deltas >= 2);
    let tip = a.branch_tip(&tree(&a), "main").unwrap();
    let tree_id = server.tree_id(USER, "proj").unwrap();
    assert_eq!(server.branch_tip(&tree_id, "main"), tip);
}

#[tokio::test]
async fn interrupted_pack_upload_is_resumed_once() {
    let (_home, server) = setup(true).await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    // 6 MiB of incompressible data: the pack spans two 4 MiB chunks and
    // the fake drops the connection after the first.
    for i in 0..3 {
        write(
            &a,
            &format!("big{i}.bin"),
            &noise(101 + 2 * i, 2 * 1024 * 1024),
        );
    }
    snap(&a, "big");
    server.state().fail_upload_after = Some(4 * 1024 * 1024);
    let (_, stats) = remote::push_with_stats(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(stats.pack && stats.resumed, "{stats:?}");
    let st = server.state();
    assert_eq!(st.pack_upload_calls, 2);
    assert_eq!(st.pack_resumes, 1);
    assert!(st.pack_spool.is_empty());
}

#[tokio::test]
async fn servers_without_packs_get_whole_objects() {
    let (_home, server) = setup(false).await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    write(&a, "a.txt", b"hello\n");
    snap(&a, "initial");
    let (report, stats) = remote::push_with_stats(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(!stats.pack);
    assert_eq!(stats.objects, report.objects_sent);
    assert_eq!(report.objects_sent, 2);
    assert!(server.state().upload_calls >= 1);

    // Clone and fetch fall back to DownloadObjects.
    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", &b_dir.path().join("b"))
        .await
        .unwrap();
    assert_eq!(std::fs::read(b.root().join("a.txt")).unwrap(), b"hello\n");
    assert!(server.state().download_calls >= 1);
    assert_eq!(server.state().pack_download_calls, 0);
}

#[tokio::test]
async fn clone_and_fetch_download_packs_with_deltas() {
    let (_home, server) = setup(true).await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let base = noise(7, 128 * 1024);
    write(&a, "doc.bin", &base);
    write(&a, "notes.txt", b"v1\n");
    snap(&a, "v1");
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();

    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", &b_dir.path().join("b"))
        .await
        .unwrap();
    assert_eq!(std::fs::read(b.root().join("doc.bin")).unwrap(), base);
    {
        let st = server.state();
        assert_eq!(st.pack_download_calls, 1);
        assert_eq!(st.download_calls, 0, "everything came in the pack");
    }

    write(&a, "doc.bin", &edited(&base));
    write(&a, "notes.txt", b"v2\n");
    snap(&a, "v2");
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    let report = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert_eq!(report.fetch.objects_received, 3, "{report:?}");
    assert_eq!(
        std::fs::read(b.root().join("doc.bin")).unwrap(),
        edited(&base)
    );
    let st = server.state();
    assert_eq!(st.pack_download_calls, 2);
    assert_eq!(st.download_calls, 0);
    assert!(st.last_download_deltas >= 1, "doc.bin travels as a delta");
}

#[tokio::test]
async fn wt_no_pack_disables_packs() {
    let (_home, server) = setup(true).await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    write(&a, "a.txt", b"hello\n");
    snap(&a, "initial");
    // TestHome serializes the tests of this binary, so the variable is
    // only seen here.
    std::env::set_var(remote::NO_PACK_ENV, "1");
    assert!(remote::packs_disabled_by_env());
    let pushed = remote::push_with_stats(&a, &tree(&a), "origin", None).await;
    let b_dir = tempfile::tempdir().unwrap();
    let cloned = remote::clone(&server.url, USER, "proj", &b_dir.path().join("b")).await;
    std::env::remove_var(remote::NO_PACK_ENV);
    let (_, stats) = pushed.unwrap();
    cloned.unwrap();
    assert!(!stats.pack);
    let st = server.state();
    assert_eq!(st.pack_upload_calls + st.pack_download_calls, 0);
    assert!(st.upload_calls >= 1 && st.download_calls >= 1);
}
