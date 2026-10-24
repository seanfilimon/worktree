//! In-process daemon tests over real TCP IPC.
//!
//! Lifecycle tests run standalone. Auto-snapshot end-to-end tests need the
//! real `Repository` implementation and are ignored until sdk-core lands.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use worktree_bg::config::{BgConfig, CONFIG_FILE};
use worktree_bg::info::INFO_FILE;
use worktree_bg::{run, BgError, RunOutcome, VERSION};
use worktree_sdk::ipc::{
    read_daemon_info_at, send_to, DaemonInfo, DaemonStatus, IpcRequest, IpcResponse,
};
use worktree_sdk::{
    InitOptions, MergeOptions, MergeOutcome, Repository, SdkError, SnapshotOptions, SwitchOptions,
};

type RunTask = JoinHandle<Result<RunOutcome, BgError>>;

/// Spawn the daemon on `home` and wait for it to advertise itself.
async fn start<S>(home: &Path, shutdown: S) -> (RunTask, DaemonInfo)
where
    S: Future<Output = ()> + Send + 'static,
{
    let info_path = home.join(INFO_FILE);
    let task = tokio::spawn({
        let home = home.to_path_buf();
        async move { run(&home, shutdown).await }
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(info) = read_daemon_info_at(&info_path).unwrap() {
            if info.pid == std::process::id()
                && send_to(info.port, &IpcRequest::Status).await.is_ok()
            {
                return (task, info);
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "daemon did not start"
        );
        assert!(!task.is_finished(), "daemon exited early: {:?}", task.await);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn status(port: u16) -> DaemonStatus {
    match send_to(port, &IpcRequest::Status).await.unwrap() {
        IpcResponse::Status(s) => s,
        other => panic!("unexpected response {other:?}"),
    }
}

async fn request_ok(port: u16, req: IpcRequest) -> String {
    match send_to(port, &req).await.unwrap() {
        IpcResponse::Ok { message } => message,
        other => panic!("{req:?} -> {other:?}"),
    }
}

async fn shutdown(port: u16, task: RunTask) {
    request_ok(port, IpcRequest::Shutdown).await;
    let outcome = tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .expect("daemon stops")
        .unwrap()
        .unwrap();
    assert_eq!(outcome, RunOutcome::Stopped);
}

#[tokio::test]
async fn lifecycle_status_single_instance_and_shutdown() {
    let home = tempfile::tempdir().unwrap();
    let (task, info) = start(home.path(), std::future::pending()).await;
    assert_eq!(info.version, VERSION);

    let s = status(info.port).await;
    assert_eq!(s.pid, std::process::id());
    assert_eq!(s.version, VERSION);
    assert!(s.watched.is_empty());
    assert!(s.started_at <= chrono::Utc::now());

    // A second instance on the same home defers to the live one.
    let second = run(home.path(), std::future::pending()).await.unwrap();
    assert_eq!(second, RunOutcome::AlreadyRunning(info.clone()));
    assert_eq!(
        read_daemon_info_at(&home.path().join(INFO_FILE)).unwrap(),
        Some(info.clone())
    );

    shutdown(info.port, task).await;
    assert!(
        !home.path().join(INFO_FILE).exists(),
        "bg.json removed on shutdown"
    );
    assert!(matches!(
        send_to(info.port, &IpcRequest::Status).await,
        Err(SdkError::DaemonNotRunning)
    ));
}

#[tokio::test]
async fn external_signal_stops_daemon() {
    let home = tempfile::tempdir().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (task, _info) = start(home.path(), async {
        let _ = stop_rx.await;
    })
    .await;
    stop_tx.send(()).unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(30), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(outcome, RunOutcome::Stopped);
    assert!(!home.path().join(INFO_FILE).exists());
}

#[tokio::test]
async fn stale_info_file_is_replaced() {
    let home = tempfile::tempdir().unwrap();
    let dead_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let stale = DaemonInfo {
        pid: 1,
        port: dead_port,
        version: "old".into(),
    };
    std::fs::write(
        home.path().join(INFO_FILE),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();

    let (task, info) = start(home.path(), std::future::pending()).await;
    assert_ne!(info, stale);
    shutdown(info.port, task).await;
}

#[tokio::test]
async fn invalid_requests_get_errors_over_ipc() {
    let home = tempfile::tempdir().unwrap();
    let (task, info) = start(home.path(), std::future::pending()).await;
    let missing = home.path().join("missing");
    for req in [
        IpcRequest::Watch {
            path: missing.clone(),
        },
        IpcRequest::Watch {
            path: PathBuf::from("relative"),
        },
        IpcRequest::Unwatch {
            path: missing.clone(),
        },
        IpcRequest::SnapshotNow {
            path: missing.clone(),
        },
    ] {
        let resp = send_to(info.port, &req).await.unwrap();
        assert!(
            matches!(resp, IpcResponse::Error { .. }),
            "{req:?} -> {resp:?}"
        );
    }
    shutdown(info.port, task).await;
}

// ---------------------------------------------------------------------------
// End-to-end auto-snapshot tests (need sdk-core)
// ---------------------------------------------------------------------------

/// Point `$WT_HOME` at a private directory for the whole test process, so
/// SDK code that consults per-user state never sees the real one.
fn isolate_wt_home() {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("WT_HOME", dir.path());
        dir
    });
}

/// A fresh worktree with test-friendly auto-snapshot settings.
fn init_worktree(dir: &Path, inactivity_secs: u64) -> PathBuf {
    let repo = Repository::init(dir, InitOptions::default()).unwrap();
    let mut cfg = repo.config().clone();
    cfg.user.name = "Daemon Test".into();
    cfg.user.email = "daemon@test.invalid".into();
    cfg.auto_snapshot.enabled = true;
    cfg.auto_snapshot.inactivity_secs = inactivity_secs;
    cfg.auto_snapshot.max_changed_files = 1000;
    cfg.sync.auto_stage = false;
    let mut repo = repo;
    repo.set_config(cfg).unwrap();
    worktree_bg::paths::canonical(repo.root()).unwrap()
}

fn root_tree_log(root: &Path) -> Vec<worktree_sdk::Snapshot> {
    let repo = Repository::open(root).unwrap();
    let tree = repo.root_tree().unwrap();
    repo.log(&tree.name, None, None).unwrap()
}

/// Poll `Status` until `pred` holds for the single watched worktree.
async fn wait_for(
    port: u16,
    what: &str,
    pred: impl Fn(&worktree_sdk::ipc::WatchedWorktree) -> bool,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let s = status(port).await;
        if s.watched.first().is_some_and(&pred) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}: {s:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn auto_snapshot_after_inactivity() {
    isolate_wt_home();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = init_worktree(work.path(), 1);
    let (task, info) = start(home.path(), std::future::pending()).await;

    let msg = request_ok(
        info.port,
        IpcRequest::Watch {
            path: root.join("."),
        },
    )
    .await;
    assert!(msg.contains("watching"), "{msg}");
    assert_eq!(
        BgConfig::load(&home.path().join(CONFIG_FILE))
            .unwrap()
            .watched,
        vec![root.clone()]
    );

    std::fs::write(root.join("notes.txt"), "hello").unwrap();
    wait_for(info.port, "auto-snapshot", |w| w.snapshots_created >= 1).await;

    let s = status(info.port).await;
    let w = &s.watched[0];
    assert_eq!(w.root, root);
    assert_eq!(w.last_error, None);
    assert!(w.last_snapshot_at.is_some());
    let log = root_tree_log(&root);
    let head = &log[0];
    assert!(head.auto_generated);
    assert_eq!(head.message, "auto: 1 files changed");
    assert_eq!(w.last_snapshot.as_deref(), Some(head.id.as_str()));

    shutdown(info.port, task).await;
}

#[tokio::test]
async fn snapshot_now_and_pause() {
    isolate_wt_home();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = init_worktree(work.path(), 1);
    let (task, info) = start(home.path(), std::future::pending()).await;
    request_ok(info.port, IpcRequest::Watch { path: root.clone() }).await;

    let msg = request_ok(
        info.port,
        IpcRequest::Pause {
            path: Some(root.clone()),
        },
    )
    .await;
    assert!(msg.starts_with("paused"), "{msg}");
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::write(root.join("b.txt"), "b").unwrap();
    wait_for(info.port, "pending changes", |w| w.pending_changes >= 2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let s = status(info.port).await;
    assert!(s.watched[0].paused);
    assert_eq!(
        s.watched[0].snapshots_created, 0,
        "paused worktrees are not auto-snapshotted"
    );

    // SnapshotNow works while paused.
    let msg = request_ok(info.port, IpcRequest::SnapshotNow { path: root.clone() }).await;
    assert_eq!(msg, "created 1 snapshot");
    assert_eq!(root_tree_log(&root)[0].message, "auto: 2 files changed");

    // Nothing left to snapshot.
    let msg = request_ok(info.port, IpcRequest::SnapshotNow { path: root.clone() }).await;
    assert_eq!(msg, "no changes to snapshot");

    request_ok(info.port, IpcRequest::Resume { path: None }).await;
    std::fs::write(root.join("c.txt"), "c").unwrap();
    wait_for(info.port, "auto-snapshot after resume", |w| {
        w.snapshots_created >= 2
    })
    .await;

    shutdown(info.port, task).await;
}

#[tokio::test]
async fn metadata_and_ignored_paths_do_not_trigger() {
    isolate_wt_home();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = init_worktree(work.path(), 1);
    std::fs::create_dir_all(root.join("target")).unwrap();
    let (task, info) = start(home.path(), std::future::pending()).await;
    request_ok(info.port, IpcRequest::Watch { path: root.clone() }).await;

    std::fs::write(root.join("target").join("out.bin"), "built").unwrap();
    std::fs::write(root.join(".wt").join("scratch"), "internal").unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let s = status(info.port).await;
    assert_eq!(s.watched[0].pending_changes, 0, "{s:?}");
    assert_eq!(s.watched[0].snapshots_created, 0, "{s:?}");

    shutdown(info.port, task).await;
}

#[tokio::test]
async fn watch_list_survives_restart_and_unwatch_stops_snapshots() {
    isolate_wt_home();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = init_worktree(work.path(), 1);

    let (task, info) = start(home.path(), std::future::pending()).await;
    request_ok(info.port, IpcRequest::Watch { path: root.clone() }).await;
    let again = request_ok(info.port, IpcRequest::Watch { path: root.clone() }).await;
    assert!(again.starts_with("already watching"), "{again}");
    shutdown(info.port, task).await;

    let (task, info) = start(home.path(), std::future::pending()).await;
    wait_for(info.port, "restored watch", |w| {
        w.root == root && w.last_error.is_none()
    })
    .await;
    std::fs::write(root.join("after-restart.txt"), "x").unwrap();
    wait_for(info.port, "auto-snapshot after restart", |w| {
        w.snapshots_created >= 1
    })
    .await;

    request_ok(
        info.port,
        IpcRequest::Unwatch {
            path: root.join("sub-path-is-fine"),
        },
    )
    .await;
    assert!(status(info.port).await.watched.is_empty());
    assert!(BgConfig::load(&home.path().join(CONFIG_FILE))
        .unwrap()
        .watched
        .is_empty());
    let before = root_tree_log(&root).len();
    std::fs::write(root.join("unwatched.txt"), "y").unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        root_tree_log(&root).len(),
        before,
        "unwatched worktrees are left alone"
    );

    shutdown(info.port, task).await;
}

#[tokio::test]
async fn no_auto_snapshot_during_merge() {
    isolate_wt_home();
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = init_worktree(work.path(), 3600);
    let repo = Repository::open(&root).unwrap();
    let tree = repo.root_tree().unwrap().name;
    let snap = |msg: &str| {
        repo.snapshot(
            &tree,
            SnapshotOptions {
                message: msg.into(),
                ..Default::default()
            },
        )
        .unwrap();
    };
    std::fs::write(root.join("f.txt"), "base\n").unwrap();
    snap("base");
    repo.switch_branch(
        &tree,
        "other",
        SwitchOptions {
            create: true,
            force: false,
        },
    )
    .unwrap();
    std::fs::write(root.join("f.txt"), "other\n").unwrap();
    snap("other");
    repo.switch_branch(&tree, "main", SwitchOptions::default())
        .unwrap();
    std::fs::write(root.join("f.txt"), "main\n").unwrap();
    snap("main");
    let outcome = repo.merge(&tree, "other", MergeOptions::default()).unwrap();
    assert!(
        matches!(outcome, MergeOutcome::Conflicts { .. }),
        "{outcome:?}"
    );
    let head_before = root_tree_log(&root)[0].id.clone();

    let (task, info) = start(home.path(), std::future::pending()).await;
    request_ok(info.port, IpcRequest::Watch { path: root.clone() }).await;
    let msg = request_ok(info.port, IpcRequest::SnapshotNow { path: root.clone() }).await;
    assert_eq!(msg, "no changes to snapshot");
    assert_eq!(root_tree_log(&root)[0].id, head_before);

    shutdown(info.port, task).await;
}
