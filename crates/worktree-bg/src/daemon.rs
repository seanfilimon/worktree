//! The daemon: watch manager, IPC request handling and process lifecycle.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use notify::RecommendedWatcher;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;
use worktree_sdk::ipc::{
    self, read_daemon_info_at, DaemonInfo, DaemonStatus, IpcRequest, IpcResponse,
};
use worktree_sdk::Repository;

use crate::config::{BgConfig, CONFIG_FILE};
use crate::error::{BgError, Result};
use crate::info::{DaemonInfoFile, INFO_FILE};
use crate::paths;
use crate::snapshot::RepoSnapshotter;
use crate::watcher::{self, RepoView};
use crate::worker::{run_worker, SharedStats, TriggerReport, WorkerCommand};

/// Version reported in `bg.json` and `Status`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long shutdown waits for workers to finish an in-flight snapshot.
const WORKER_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// A running watch: the file watcher, its worker task, and the control channel.
struct WorkerHandle {
    commands: UnboundedSender<WorkerCommand>,
    watcher: RecommendedWatcher,
    task: JoinHandle<()>,
}

impl WorkerHandle {
    /// Open the worktree at `root`, start watching it and spawn its worker.
    async fn start(root: PathBuf, stats: SharedStats) -> Result<Self> {
        let (events_tx, events_rx) = unbounded_channel();
        let (commands, commands_rx) = unbounded_channel();
        let watch_root = root.clone();
        let (watcher, settings) = tokio::task::spawn_blocking(move || -> Result<_> {
            let view = RepoView::open(&watch_root)?;
            let settings = view.settings();
            Ok((watcher::watch(&watch_root, view, events_tx)?, settings))
        })
        .await??;
        let action = RepoSnapshotter::new(root);
        let task = tokio::spawn(run_worker(action, settings, events_rx, commands_rx, stats));
        Ok(Self {
            commands,
            watcher,
            task,
        })
    }

    /// Stop watching and signal the worker to exit; returns its task.
    fn shut(self) -> JoinHandle<()> {
        drop(self.watcher);
        drop(self.commands);
        self.task
    }
}

struct WatchEntry {
    stats: SharedStats,
    /// `None` when the worktree could not be watched (see `stats.last_error`).
    worker: Option<WorkerHandle>,
}

/// Daemon state shared by all IPC connections.
pub struct Daemon {
    config_path: PathBuf,
    started_at: DateTime<Utc>,
    config: Mutex<BgConfig>,
    watches: tokio::sync::Mutex<BTreeMap<PathBuf, WatchEntry>>,
    shutdown: Notify,
}

impl Daemon {
    /// Create a daemon whose per-user state lives in `home` (`$WT_HOME`).
    pub fn new(home: &Path) -> Result<Self> {
        let config_path = home.join(CONFIG_FILE);
        let config = BgConfig::load(&config_path)?;
        Ok(Self {
            config_path,
            started_at: Utc::now(),
            config: Mutex::new(config),
            watches: tokio::sync::Mutex::new(BTreeMap::new()),
            shutdown: Notify::new(),
        })
    }

    /// Start watching every worktree listed in `bg.toml`. Worktrees that
    /// cannot be opened stay listed (reported with `last_error`).
    pub async fn restore(&self) {
        let roots = self.lock_config().watched.clone();
        for root in roots {
            let stats = SharedStats::default();
            let (key, worker) = match self.start_watch(&root, stats.clone()).await {
                Ok((resolved, worker)) => (resolved, Some(worker)),
                Err(e) => {
                    tracing::warn!(root = %root.display(), error = %e, "cannot watch worktree");
                    stats.lock().last_error = Some(format!("not watching: {e}"));
                    (root, None)
                }
            };
            let mut watches = self.watches.lock().await;
            if watches.contains_key(&key) {
                if let Some(worker) = worker {
                    drop(worker.shut());
                }
                continue;
            }
            if worker.is_some() {
                tracing::info!(root = %key.display(), "watching");
            }
            watches.insert(key, WatchEntry { stats, worker });
        }
    }

    /// Handle one IPC request.
    pub async fn handle(&self, req: IpcRequest) -> IpcResponse {
        tracing::debug!(?req, "ipc request");
        let result = match req {
            IpcRequest::Status => return IpcResponse::Status(self.status().await),
            IpcRequest::Watch { path } => self.watch(&path).await,
            IpcRequest::Unwatch { path } => self.unwatch(&path).await,
            IpcRequest::Pause { path } => self.set_paused(path.as_deref(), true).await,
            IpcRequest::Resume { path } => self.set_paused(path.as_deref(), false).await,
            IpcRequest::SnapshotNow { path } => match self.snapshot_now(&path).await {
                Ok(report) if report.errors.is_empty() => Ok(report.summary()),
                Ok(report) => Err(BgError::InvalidRequest(report.summary())),
                Err(e) => Err(e),
            },
            IpcRequest::Shutdown => {
                self.shutdown.notify_one();
                Ok("shutting down".to_string())
            }
        };
        match result {
            Ok(message) => IpcResponse::Ok { message },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        }
    }

    /// Completes once a `Shutdown` request has been handled.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await;
    }

    /// Stop all watches, waiting (bounded) for in-flight snapshots.
    pub async fn stop_all(&self) {
        let entries = std::mem::take(&mut *self.watches.lock().await);
        let tasks: Vec<_> = entries
            .into_values()
            .filter_map(|e| e.worker)
            .map(WorkerHandle::shut)
            .collect();
        let deadline = tokio::time::Instant::now() + WORKER_STOP_TIMEOUT;
        for task in tasks {
            match tokio::time::timeout_at(deadline, task).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "worker task failed"),
                Err(_) => tracing::warn!("worker did not stop in time"),
            }
        }
    }

    pub async fn status(&self) -> DaemonStatus {
        let watches = self.watches.lock().await;
        DaemonStatus {
            pid: std::process::id(),
            version: VERSION.to_string(),
            started_at: self.started_at,
            watched: watches
                .iter()
                .map(|(root, e)| e.stats.snapshot().to_watched(root))
                .collect(),
        }
    }

    async fn watch(&self, path: &Path) -> Result<String> {
        require_absolute(path)?;
        let stats = SharedStats::default();
        let (root, worker) = self.start_watch(path, stats.clone()).await?;
        {
            let mut watches = self.watches.lock().await;
            if watches.get(&root).is_some_and(|e| e.worker.is_some()) {
                drop(watches);
                drop(worker.shut());
                return Ok(format!("already watching {}", root.display()));
            }
            watches.insert(
                root.clone(),
                WatchEntry {
                    stats,
                    worker: Some(worker),
                },
            );
        }
        self.update_config(|c| c.add(&root))?;
        tracing::info!(root = %root.display(), "watching");
        Ok(format!("watching {}", root.display()))
    }

    /// Resolve `path` to its worktree root and start a worker for it.
    async fn start_watch(
        &self,
        path: &Path,
        stats: SharedStats,
    ) -> Result<(PathBuf, WorkerHandle)> {
        let path = path.to_path_buf();
        let root = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
            let dir = paths::canonical(&path).map_err(|e| {
                BgError::InvalidRequest(format!("cannot watch {}: {e}", path.display()))
            })?;
            let repo = Repository::open(&dir)?;
            Ok(paths::canonical(repo.root())?)
        })
        .await??;
        let worker = WorkerHandle::start(root.clone(), stats).await?;
        Ok((root, worker))
    }

    async fn unwatch(&self, path: &Path) -> Result<String> {
        require_absolute(path)?;
        let key = paths::lookup_key(path);
        let removed = {
            let mut watches = self.watches.lock().await;
            let root = find_root(&watches, &key).unwrap_or_else(|| key.clone());
            let entry = watches.remove(&root);
            (root, entry)
        };
        let (root, entry) = removed;
        let mut was_configured = false;
        self.update_config(|c| {
            was_configured = c.remove(&root);
            was_configured
        })?;
        match entry {
            Some(entry) => {
                if let Some(worker) = entry.worker {
                    let task = worker.shut();
                    if tokio::time::timeout(WORKER_STOP_TIMEOUT, task)
                        .await
                        .is_err()
                    {
                        tracing::warn!(root = %root.display(), "worker did not stop in time");
                    }
                }
                tracing::info!(root = %root.display(), "stopped watching");
                Ok(format!("stopped watching {}", root.display()))
            }
            None if was_configured => Ok(format!("stopped watching {}", root.display())),
            None => Err(BgError::NotWatched(root)),
        }
    }

    async fn set_paused(&self, path: Option<&Path>, paused: bool) -> Result<String> {
        let watches = self.watches.lock().await;
        let roots: Vec<PathBuf> = match path {
            None => watches.keys().cloned().collect(),
            Some(path) => {
                require_absolute(path)?;
                let key = paths::lookup_key(path);
                vec![find_root(&watches, &key).ok_or(BgError::NotWatched(key))?]
            }
        };
        for root in &roots {
            let entry = &watches[root];
            entry.stats.lock().paused = paused;
            if let Some(worker) = &entry.worker {
                // A closed channel means the worker is exiting; nothing to pause.
                let _ = worker.commands.send(WorkerCommand::SetPaused(paused));
            }
        }
        let verb = if paused { "paused" } else { "resumed" };
        Ok(match roots.as_slice() {
            [one] => format!("{verb} {}", one.display()),
            many => format!("{verb} {} worktree(s)", many.len()),
        })
    }

    async fn snapshot_now(&self, path: &Path) -> Result<TriggerReport> {
        require_absolute(path)?;
        let key = paths::lookup_key(path);
        let commands = {
            let watches = self.watches.lock().await;
            let root = find_root(&watches, &key).ok_or_else(|| BgError::NotWatched(key.clone()))?;
            let entry = &watches[&root];
            match &entry.worker {
                Some(worker) => worker.commands.clone(),
                None => {
                    let reason = entry.stats.lock().last_error.clone().unwrap_or_default();
                    return Err(BgError::InvalidRequest(format!(
                        "watch of {} is inactive: {reason}",
                        root.display()
                    )));
                }
            }
        };
        let (tx, rx) = oneshot::channel();
        let stopped = || BgError::InvalidRequest("worktree worker stopped".into());
        commands
            .send(WorkerCommand::SnapshotNow(tx))
            .map_err(|_| stopped())?;
        rx.await.map_err(|_| stopped())
    }

    fn lock_config(&self) -> std::sync::MutexGuard<'_, BgConfig> {
        self.config.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Apply `change` to the config and persist it if it reports a change.
    fn update_config(&self, change: impl FnOnce(&mut BgConfig) -> bool) -> Result<()> {
        let mut config = self.lock_config();
        if change(&mut config) {
            config.save(&self.config_path)?;
        }
        Ok(())
    }
}

fn require_absolute(path: &Path) -> Result<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(BgError::InvalidRequest(format!(
            "path must be absolute: {}",
            path.display()
        )))
    }
}

/// The watched root containing `path` (the deepest one if nested).
fn find_root<V>(watches: &BTreeMap<PathBuf, V>, path: &Path) -> Option<PathBuf> {
    watches
        .keys()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .cloned()
}

/// How [`run`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// Served until shutdown; `bg.json` has been removed.
    Stopped,
    /// Another live daemon owns this `$WT_HOME`.
    AlreadyRunning(DaemonInfo),
}

/// Run the daemon with per-user state in `home` until `shutdown` completes
/// or a `Shutdown` request arrives.
pub async fn run<S: Future<Output = ()>>(home: &Path, shutdown: S) -> Result<RunOutcome> {
    std::fs::create_dir_all(home)?;
    let info_path = home.join(INFO_FILE);
    match read_daemon_info_at(&info_path) {
        Ok(Some(existing)) => match ipc::send_to(existing.port, &IpcRequest::Status).await {
            Ok(IpcResponse::Status(_)) => return Ok(RunOutcome::AlreadyRunning(existing)),
            _ => tracing::warn!(
                pid = existing.pid,
                port = existing.port,
                "replacing stale daemon info"
            ),
        },
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "replacing unreadable daemon info"),
    }

    let daemon = Arc::new(Daemon::new(home)?);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let info = DaemonInfo {
        pid: std::process::id(),
        port,
        version: VERSION.to_string(),
    };
    let info_file = DaemonInfoFile::create(&info_path, info)?;
    tracing::info!(
        port,
        pid = std::process::id(),
        version = VERSION,
        "worktree-bg listening"
    );

    let restore = tokio::spawn({
        let daemon = daemon.clone();
        async move { daemon.restore().await }
    });
    let handler = {
        let daemon = daemon.clone();
        move |req| {
            let daemon = daemon.clone();
            async move { daemon.handle(req).await }
        }
    };
    let stop = {
        let daemon = daemon.clone();
        async move {
            tokio::select! {
                () = shutdown => tracing::info!("shutdown signal received"),
                () = daemon.shutdown_requested() => tracing::info!("shutdown requested over ipc"),
            }
        }
    };
    ipc::serve(listener, handler, stop).await;

    restore.abort();
    if let Err(e) = restore.await {
        if !e.is_cancelled() {
            tracing::warn!(error = %e, "restoring watches failed");
        }
    }
    daemon.stop_all().await;
    drop(info_file);
    tracing::info!("worktree-bg stopped");
    Ok(RunOutcome::Stopped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(p: &str) -> PathBuf {
        std::env::temp_dir().join(p)
    }

    #[test]
    fn find_root_prefers_deepest_containing_root() {
        let mut map = BTreeMap::new();
        map.insert(abs("w"), ());
        map.insert(abs("w").join("nested"), ());
        map.insert(abs("other"), ());
        assert_eq!(find_root(&map, &abs("w").join("src")), Some(abs("w")));
        assert_eq!(
            find_root(&map, &abs("w").join("nested").join("x")),
            Some(abs("w").join("nested"))
        );
        assert_eq!(find_root(&map, &abs("w")), Some(abs("w")));
        assert_eq!(
            find_root(&map, &abs("wx")),
            None,
            "component-wise prefix only"
        );
        assert_eq!(find_root(&map, &abs("none")), None);
    }

    #[test]
    fn relative_paths_are_rejected() {
        assert!(require_absolute(Path::new("relative/dir")).is_err());
        assert!(require_absolute(&abs("x")).is_ok());
    }

    #[tokio::test]
    async fn status_of_fresh_daemon_is_empty() {
        let home = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(home.path()).unwrap();
        match daemon.handle(IpcRequest::Status).await {
            IpcResponse::Status(s) => {
                assert_eq!(s.pid, std::process::id());
                assert_eq!(s.version, VERSION);
                assert!(s.watched.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn requests_for_unknown_worktrees_fail() {
        let home = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(home.path()).unwrap();
        let missing = home.path().join("nope");
        for req in [
            IpcRequest::Watch {
                path: missing.clone(),
            },
            IpcRequest::Unwatch {
                path: missing.clone(),
            },
            IpcRequest::Pause {
                path: Some(missing.clone()),
            },
            IpcRequest::Resume {
                path: Some(missing.clone()),
            },
            IpcRequest::SnapshotNow {
                path: missing.clone(),
            },
            IpcRequest::Watch {
                path: "relative".into(),
            },
        ] {
            let resp = daemon.handle(req.clone()).await;
            assert!(
                matches!(resp, IpcResponse::Error { .. }),
                "{req:?} -> {resp:?}"
            );
        }
        assert!(
            !home.path().join(CONFIG_FILE).exists(),
            "failed requests must not persist"
        );
    }

    #[tokio::test]
    async fn pause_and_resume_all_with_nothing_watched() {
        let home = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(home.path()).unwrap();
        let resp = daemon.handle(IpcRequest::Pause { path: None }).await;
        assert_eq!(
            resp,
            IpcResponse::Ok {
                message: "paused 0 worktree(s)".into()
            }
        );
        let resp = daemon.handle(IpcRequest::Resume { path: None }).await;
        assert_eq!(
            resp,
            IpcResponse::Ok {
                message: "resumed 0 worktree(s)".into()
            }
        );
    }

    #[tokio::test]
    async fn shutdown_request_notifies() {
        let home = tempfile::tempdir().unwrap();
        let daemon = Daemon::new(home.path()).unwrap();
        let resp = daemon.handle(IpcRequest::Shutdown).await;
        assert_eq!(
            resp,
            IpcResponse::Ok {
                message: "shutting down".into()
            }
        );
        tokio::time::timeout(Duration::from_secs(1), daemon.shutdown_requested())
            .await
            .expect("shutdown permit stored");
    }

    #[tokio::test]
    async fn restore_reports_missing_worktrees_and_unwatch_forgets_them() {
        let home = tempfile::tempdir().unwrap();
        let gone = home.path().join("deleted-worktree");
        let mut cfg = BgConfig::default();
        cfg.add(&gone);
        cfg.save(&home.path().join(CONFIG_FILE)).unwrap();

        let daemon = Daemon::new(home.path()).unwrap();
        daemon.restore().await;
        let status = daemon.status().await;
        assert_eq!(status.watched.len(), 1);
        let w = &status.watched[0];
        assert_eq!(w.root, gone);
        assert!(
            w.last_error
                .as_deref()
                .is_some_and(|e| e.starts_with("not watching")),
            "{w:?}"
        );

        // Pausing an inactive entry is allowed; snapshotting it is not.
        let resp = daemon
            .handle(IpcRequest::Pause {
                path: Some(gone.clone()),
            })
            .await;
        assert!(matches!(resp, IpcResponse::Ok { .. }), "{resp:?}");
        assert!(daemon.status().await.watched[0].paused);
        let resp = daemon
            .handle(IpcRequest::SnapshotNow { path: gone.clone() })
            .await;
        assert!(
            matches!(&resp, IpcResponse::Error { message } if message.contains("inactive")),
            "{resp:?}"
        );

        let resp = daemon
            .handle(IpcRequest::Unwatch { path: gone.clone() })
            .await;
        assert!(matches!(resp, IpcResponse::Ok { .. }), "{resp:?}");
        assert!(daemon.status().await.watched.is_empty());
        assert!(BgConfig::load(&home.path().join(CONFIG_FILE))
            .unwrap()
            .watched
            .is_empty());
    }

    #[tokio::test]
    async fn corrupt_config_refuses_to_start() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(CONFIG_FILE), "watched = \"nope\"").unwrap();
        assert!(matches!(
            Daemon::new(home.path()),
            Err(BgError::Config { .. })
        ));
    }
}
