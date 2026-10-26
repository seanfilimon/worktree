//! The daemon's outbox loop (ENGINEERING_PLAN §15.5) against the SDK's
//! in-process fake server: queued pushes are delivered once the server is
//! reachable again (with backoff while it is not), `sync.auto_flush = false`
//! leaves them alone, and `sync.auto_sync_secs` syncs clean trees only.

#[path = "../../worktree-sdk/tests/remote_fake_server/mod.rs"]
mod remote_fake_server;

use std::path::Path;
use std::time::Duration;

use remote_fake_server::{FakeServer, TestHome};
use worktree_bg::sync::{run_outbox, SyncSettings};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::{remote, InitOptions, Repository, SnapshotOptions};

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

fn snap(repo: &Repository, file: &str, content: &str) -> String {
    std::fs::write(repo.root().join(file), content).unwrap();
    repo.snapshot(
        &tree(repo),
        SnapshotOptions {
            message: format!("edit {file}"),
            skip_secret_scan: true,
            ..Default::default()
        },
    )
    .unwrap()
    .id
}

fn server_tip(server: &FakeServer) -> Option<String> {
    server.branch_tip(&server.tree_id(USER, "proj")?, "main")
}

fn fast() -> SyncSettings {
    SyncSettings {
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(400),
        outbox_poll: Duration::from_millis(100),
        ..SyncSettings::default()
    }
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn queued(root: &Path) -> Vec<worktree_sdk::repo::OutboxEntry> {
    let repo = Repository::open(root).unwrap();
    repo.outbox(&tree(&repo)).unwrap()
}

#[tokio::test]
async fn queued_pushes_are_delivered_when_the_server_returns() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    let repo = init_repo(&work.path().join("a"), &server);
    snap(&repo, "a.txt", "1");
    remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    let root = repo.root().to_path_buf();

    server.state().offline = true;
    let local = snap(&repo, "a.txt", "2 (offline)");
    let out = remote::push_or_queue(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    assert!(out.queued.is_some());

    let task = tokio::spawn(run_outbox(root.clone(), fast()));
    // While offline the loop keeps trying (with backoff).
    eventually("a failed delivery attempt", || {
        queued(&root).first().is_some_and(|e| e.attempts >= 2)
    })
    .await;
    assert_ne!(server_tip(&server).as_deref(), Some(local.as_str()));

    server.state().offline = false;
    eventually("delivery", || queued(&root).is_empty()).await;
    assert_eq!(server_tip(&server).as_deref(), Some(local.as_str()));
    assert_eq!(
        repo.remote_tip(&tree(&repo), "origin", "main")
            .unwrap()
            .as_deref(),
        Some(local.as_str())
    );
    task.abort();
}

#[tokio::test]
async fn auto_flush_off_leaves_the_queue_alone() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    let mut repo = init_repo(&work.path().join("a"), &server);
    let mut cfg = repo.config().clone();
    cfg.set("sync.auto_flush", "false").unwrap();
    repo.set_config(cfg).unwrap();
    snap(&repo, "a.txt", "1");
    server.state().offline = true;
    remote::push_or_queue(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    server.state().offline = false;

    let task = tokio::spawn(run_outbox(repo.root().to_path_buf(), fast()));
    tokio::time::sleep(Duration::from_millis(600)).await;
    let entries = queued(repo.root());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].attempts, 0, "never tried");
    assert_eq!(server_tip(&server), None);
    task.abort();
}

#[tokio::test]
async fn auto_sync_integrates_clean_trees_only() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    let alice = init_repo(&work.path().join("a"), &server);
    snap(&alice, "a.txt", "1");
    remote::push(&alice, &tree(&alice), "origin", None)
        .await
        .unwrap();
    let bob = remote::clone(&server.url, USER, "proj", &work.path().join("b"))
        .await
        .unwrap();
    let bob_root = bob.root().to_path_buf();

    let settings = SyncSettings {
        auto_sync: Some(Duration::from_millis(150)),
        ..fast()
    };
    let task = tokio::spawn(run_outbox(bob_root.clone(), settings));

    // A teammate pushes: bob's clean tree follows (fetch + fast-forward).
    let tip = snap(&alice, "a.txt", "2");
    remote::push(&alice, &tree(&alice), "origin", None)
        .await
        .unwrap();
    eventually("bob fast-forwarded", || {
        Repository::open(&bob_root)
            .unwrap()
            .branch_tip("proj", "main")
            .unwrap()
            .as_deref()
            == Some(tip.as_str())
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(bob_root.join("a.txt")).unwrap(),
        "2"
    );

    // Bob's own snapshot is pushed by the next auto-sync.
    let mine = snap(&bob, "b.txt", "bob");
    eventually("bob's snapshot pushed", || {
        server_tip(&server).as_deref() == Some(mine.as_str())
    })
    .await;

    // A dirty tree is left alone.
    std::fs::write(bob_root.join("b.txt"), "uncommitted").unwrap();
    snap(&alice, "c.txt", "3");
    let c = remote::sync(
        &alice,
        &tree(&alice),
        "origin",
        remote::SyncOptions::default(),
    )
    .await
    .unwrap();
    assert!(c.push.is_some());
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        Repository::open(&bob_root)
            .unwrap()
            .branch_tip("proj", "main")
            .unwrap()
            .as_deref(),
        Some(mine.as_str()),
        "dirty tree not synced"
    );
    assert!(!bob_root.join("c.txt").exists());
    task.abort();
}
