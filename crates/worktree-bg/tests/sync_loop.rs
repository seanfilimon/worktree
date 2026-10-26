//! The daemon's sync loop (ENGINEERING_PLAN §13.11) against the SDK's
//! in-process fake server: WatchBranches-driven fetches, offline backoff and
//! reconnection, and the polling fallback.

#[path = "../../worktree-sdk/tests/remote_fake_server/mod.rs"]
mod remote_fake_server;

use std::path::Path;
use std::time::Duration;

use remote_fake_server::{FakeServer, TestHome};
use tokio::sync::oneshot;
use worktree_bg::sync::{run_sync, SyncSettings};
use worktree_bg::worker::SharedStats;
use worktree_bg::{run_with_options, DaemonOptions, RunOutcome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::ipc::{read_daemon_info_at, send_to, IpcRequest, IpcResponse};
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

/// The "teammate" worktree that pushes; returns it after the first push.
async fn pusher(dir: &Path, server: &FakeServer) -> Repository {
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
    push_change(&repo, "a.txt", "1").await;
    repo
}

async fn push_change(repo: &Repository, file: &str, content: &str) -> String {
    std::fs::write(repo.root().join(file), content).unwrap();
    let tree = repo.root_tree().unwrap().name;
    let snap = repo
        .snapshot(
            &tree,
            SnapshotOptions {
                message: format!("edit {file}"),
                skip_secret_scan: true,
                ..Default::default()
            },
        )
        .unwrap();
    remote::push(repo, &tree, "origin", None).await.unwrap();
    snap.id
}

/// A clone of the pushed tree whose daemon-side sync we observe.
async fn watcher(dir: &Path, server: &FakeServer) -> Repository {
    let dest = dir.join("clone");
    let mut repo = remote::clone(&server.url, USER, "proj", &dest)
        .await
        .unwrap();
    let mut cfg = repo.config().clone();
    cfg.sync.auto_stage = false;
    repo.set_config(cfg).unwrap();
    repo
}

fn remote_main(root: &Path) -> Option<String> {
    let repo = Repository::open(root).unwrap();
    let tree = repo.root_tree().unwrap().name;
    repo.remote_tip(&tree, "origin", "main").unwrap()
}

fn fast(interval: Duration) -> SyncSettings {
    SyncSettings {
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(400),
        interval: Some(interval),
        recheck: Duration::from_millis(200),
        healthy_stream: Duration::from_secs(30),
        ..SyncSettings::default() // sync workstream (§15.5): outbox settings
    }
}

async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn state(stats: &SharedStats) -> Option<String> {
    stats.snapshot().sync_state
}

#[tokio::test]
async fn pushes_are_fetched_through_watch_branches_and_offline_reconnects() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    let alice = pusher(&work.path().join("a"), &server).await;
    let bob = watcher(work.path(), &server).await;
    let root = bob.root().to_path_buf();

    let stats = SharedStats::default();
    let (stop, stop_rx) = oneshot::channel();
    // Polling every minute: anything faster than that came from an event.
    let task = tokio::spawn(run_sync(
        root.clone(),
        stats.clone(),
        fast(Duration::from_secs(60)),
        stop_rx,
    ));
    eventually("watching", || state(&stats).as_deref() == Some("watching")).await;
    assert!(stats.snapshot().last_fetch_at.is_some());
    assert_eq!(server.watch_calls(), 1);

    let tip = push_change(&alice, "a.txt", "2").await;
    eventually("event-driven fetch", || {
        remote_main(&root).as_deref() == Some(tip.as_str())
    })
    .await;
    assert!(Repository::open(&root).unwrap().has_snapshot(&tip));

    // The server goes away: the loop backs off, then reconnects.
    server.state().offline = true;
    server.disconnect_watchers();
    eventually("offline", || state(&stats).as_deref() == Some("offline")).await;
    assert!(stats.snapshot().sync_error.is_some());
    tokio::time::sleep(Duration::from_millis(500)).await;
    let calls_offline = server.watch_calls();
    server.state().offline = false;
    eventually("reconnected", || {
        state(&stats).as_deref() == Some("watching") && server.watch_calls() > calls_offline
    })
    .await;
    assert_eq!(
        stats.snapshot().sync_error,
        None,
        "cleared by the next fetch"
    );

    let tip = push_change(&alice, "b.txt", "3").await;
    eventually("fetch after reconnect", || {
        remote_main(&root).as_deref() == Some(tip.as_str())
    })
    .await;

    // Local branches are never moved by the sync loop.
    let tree = bob.root_tree().unwrap().name;
    assert_ne!(
        Repository::open(&root)
            .unwrap()
            .branch_tip(&tree, "main")
            .unwrap()
            .as_deref(),
        Some(tip.as_str())
    );
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("sync loop stops")
        .unwrap();
}

#[tokio::test]
async fn servers_without_watch_are_polled() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    let alice = pusher(&work.path().join("a"), &server).await;
    let bob = watcher(work.path(), &server).await;
    let root = bob.root().to_path_buf();
    server.state().watch_unavailable = true;

    let stats = SharedStats::default();
    let (stop, stop_rx) = oneshot::channel();
    let task = tokio::spawn(run_sync(
        root.clone(),
        stats.clone(),
        fast(Duration::from_millis(200)),
        stop_rx,
    ));
    // Unavailable is treated as offline (backoff), and fetches keep working.
    eventually("fetched", || stats.snapshot().last_fetch_at.is_some()).await;
    let tip = push_change(&alice, "a.txt", "polled").await;
    eventually("polled fetch", || {
        remote_main(&root).as_deref() == Some(tip.as_str())
    })
    .await;
    drop(stop);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("sync loop stops when the sender is dropped")
        .unwrap();
}

#[tokio::test]
async fn disabled_auto_fetch_does_nothing() {
    let (_home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    pusher(&work.path().join("a"), &server).await;
    let mut bob = watcher(work.path(), &server).await;
    let mut cfg = bob.config().clone();
    cfg.sync.auto_fetch = false;
    bob.set_config(cfg).unwrap();

    let stats = SharedStats::default();
    let (stop, stop_rx) = oneshot::channel();
    let task = tokio::spawn(run_sync(
        bob.root().to_path_buf(),
        stats.clone(),
        fast(Duration::from_millis(100)),
        stop_rx,
    ));
    eventually("disabled", || state(&stats).as_deref() == Some("disabled")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.watch_calls(), 0);
    assert!(stats.snapshot().last_fetch_at.is_none());
    stop.send(()).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn daemon_status_reports_sync_state() {
    let (home, server) = setup().await;
    let work = tempfile::tempdir().unwrap();
    pusher(&work.path().join("a"), &server).await;
    let bob = watcher(work.path(), &server).await;
    let root = worktree_bg::paths::canonical(bob.root()).unwrap();

    let daemon_home = home.path().join("daemon");
    let options = DaemonOptions {
        sync: fast(Duration::from_secs(60)),
        ..DaemonOptions::default()
    };
    let (stop, stop_rx) = oneshot::channel::<()>();
    let task = tokio::spawn({
        let daemon_home = daemon_home.clone();
        async move {
            run_with_options(
                &daemon_home,
                async {
                    let _ = stop_rx.await;
                },
                options,
            )
            .await
        }
    });
    let info_path = daemon_home.join("bg.json");
    let mut info = None;
    eventually("daemon started", || {
        info = read_daemon_info_at(&info_path).ok().flatten();
        info.is_some()
    })
    .await;
    let info = info.unwrap();
    let resp = send_to(info.port, &info.token, &IpcRequest::Watch { path: root })
        .await
        .unwrap();
    assert!(matches!(resp, IpcResponse::Ok { .. }), "{resp:?}");

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let IpcResponse::Status(s) = send_to(info.port, &info.token, &IpcRequest::Status)
            .await
            .unwrap()
        else {
            panic!("status")
        };
        let w = &s.watched[0];
        if w.sync_state.as_deref() == Some("watching") {
            assert!(w.last_fetch_at.is_some());
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{w:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stop.send(()).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome, RunOutcome::Stopped);
}
