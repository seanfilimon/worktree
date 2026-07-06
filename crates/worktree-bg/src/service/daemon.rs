//! Daemon lifecycle (BgProcess.md §5): startup, runtime loop, graceful
//! shutdown.
//!
//! Subsystems:
//! - **IPC server** — serves `wt` CLI requests (accept loop, one task per
//!   connection, mutating commands serialized through a single-writer lock).
//! - **Watcher** — platform file notifications on a dedicated thread,
//!   filtered and forwarded into the async world.
//! - **Auto-snapshot loop** — debounces changes and snapshots after the
//!   configured inactivity window, or immediately once `max_changed_files`
//!   accumulate (§6 triggers).

use crate::config::AutoSnapshotConfig;
use crate::engine::auto_snapshot::AutoSnapshotEngine;
use crate::engine::event::classify_event;
use crate::error::BgError;
use crate::service::health::HealthTracker;
use crate::watcher::debounce::{Debouncer, EventKind};
use crate::watcher::fs::FileSystemWatcher;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use worktree_engine::WorktreeEngine;
use worktree_ipc::IpcListener;

/// Shared state for all daemon subsystems and IPC handlers.
pub struct DaemonContext {
    /// Worktree root this daemon serves.
    pub root: PathBuf,
    /// Auto-snapshot configuration (from `.wt/config.toml`).
    pub auto_snapshot: AutoSnapshotConfig,
    /// Runtime statistics.
    pub health: Mutex<HealthTracker>,
    /// Single-writer serialization for mutating IPC commands (§14.4).
    pub writer_lock: tokio::sync::Mutex<()>,
    /// True while the watcher thread is delivering events.
    pub watcher_active: AtomicBool,
    /// Signals every subsystem to stop.
    shutdown: Notify,
    stopping: AtomicBool,
}

impl DaemonContext {
    pub fn request_shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.shutdown.notify_waiters();
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    async fn wait_shutdown(&self) {
        if self.is_stopping() {
            return;
        }
        self.shutdown.notified().await;
    }
}

/// Load the `[auto_snapshot]` section from `.wt/config.toml`.
fn load_auto_snapshot_config(root: &Path) -> AutoSnapshotConfig {
    let path = root.join(".wt").join("config.toml");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return AutoSnapshotConfig::default();
    };
    let Ok(table) = contents.parse::<toml::Table>() else {
        return AutoSnapshotConfig::default();
    };
    match table.get("auto_snapshot") {
        Some(section) => toml::Value::try_into(section.clone()).unwrap_or_default(),
        None => AutoSnapshotConfig::default(),
    }
}

/// Run the daemon for the worktree at `root` until shutdown is requested.
pub async fn run(root: PathBuf) -> Result<(), BgError> {
    // Startup scan: verify the worktree and warm the engine.
    let engine =
        WorktreeEngine::open(&root).map_err(|e| BgError::Engine(format!("open worktree: {e}")))?;
    let root = engine.root().to_path_buf();
    let auto_snapshot = load_auto_snapshot_config(&root);

    let ctx = Arc::new(DaemonContext {
        root: root.clone(),
        auto_snapshot,
        health: Mutex::new(HealthTracker::new()),
        writer_lock: tokio::sync::Mutex::new(()),
        watcher_active: AtomicBool::new(false),
        shutdown: Notify::new(),
        stopping: AtomicBool::new(false),
    });

    // IPC endpoint first — fails fast if another daemon already serves this
    // worktree (Windows named pipes error on double-bind; Unix socket files
    // are cleaned and rebound, so also check the pid file).
    let endpoint = worktree_ipc::endpoint::endpoint_for(&root);
    let listener = IpcListener::bind(&endpoint)?;
    tracing::info!(%endpoint, root = %root.display(), "worktree-bg listening");

    // Pid file for `wt server status` and stale-daemon detection.
    let pid_file = root.join(".wt").join("cache").join("bgprocess.pid");
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_file, std::process::id().to_string())?;

    // Watcher thread → async channel.
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_watcher_thread(ctx.clone(), event_tx)?;

    // Auto-snapshot loop.
    let snapshot_task = tokio::spawn(auto_snapshot_loop(ctx.clone(), event_rx));

    // IPC accept loop.
    let accept_ctx = ctx.clone();
    let accept_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                conn = listener.accept() => match conn {
                    Ok(stream) => {
                        tokio::spawn(crate::ipc::handle_connection(accept_ctx.clone(), stream));
                    }
                    Err(e) => {
                        tracing::warn!("IPC accept failed: {e}");
                    }
                },
                _ = accept_ctx.wait_shutdown() => break,
            }
        }
    });

    // Also stop on Ctrl-C when running in the foreground.
    let signal_ctx = ctx.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("ctrl-c received, shutting down");
            signal_ctx.request_shutdown();
        }
    });

    ctx.wait_shutdown().await;
    tracing::info!("daemon shutting down");

    // Give in-flight connection handlers a moment to flush their final
    // response (e.g. the `daemon.shutdown` ack) before the runtime drops.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Graceful shutdown: stop accepting, let the snapshot loop drain.
    accept_task.abort();
    let _ = snapshot_task.await;
    let _ = std::fs::remove_file(&pid_file);
    Ok(())
}

/// Raw watcher events, filtered and normalized, on a dedicated thread
/// (the `notify` receiver is blocking).
fn spawn_watcher_thread(
    ctx: Arc<DaemonContext>,
    tx: tokio::sync::mpsc::UnboundedSender<crate::watcher::debounce::DebouncedEvent>,
) -> Result<(), BgError> {
    let mut watcher = FileSystemWatcher::new()?;
    watcher.watch(&ctx.root)?;
    ctx.watcher_active.store(true, Ordering::SeqCst);

    let root = ctx.root.clone();
    std::thread::Builder::new()
        .name("wt-watcher".into())
        .spawn(move || {
            // Rebind the WHOLE struct: edition-2021 closures capture disjoint
            // fields, and capturing only `watcher.receiver` would drop the OS
            // watcher (closing the channel and killing this thread instantly).
            let watcher = watcher;
            while let Ok(event) = watcher.receiver.recv() {
                if ctx.is_stopping() {
                    break;
                }
                let Ok(event) = event else { continue };
                let Some(kind) = map_kind(&event.kind) else {
                    continue;
                };
                for path in event.paths {
                    let rel = path.strip_prefix(&root).unwrap_or(&path);
                    if is_internal_path(rel) {
                        continue;
                    }
                    let _ = tx.send(crate::watcher::debounce::DebouncedEvent::now(
                        rel.to_path_buf(),
                        kind,
                    ));
                }
            }
            ctx.watcher_active.store(false, Ordering::SeqCst);
        })
        .map_err(|e| BgError::Watcher(format!("spawn watcher thread: {e}")))?;
    Ok(())
}

fn map_kind(kind: &notify::EventKind) -> Option<EventKind> {
    use notify::EventKind as NK;
    Some(match kind {
        NK::Create(_) => EventKind::Created,
        NK::Modify(notify::event::ModifyKind::Name(_)) => EventKind::Renamed,
        NK::Modify(_) => EventKind::Modified,
        NK::Remove(_) => EventKind::Deleted,
        _ => return None,
    })
}

/// Paths the daemon must never react to (its own metadata and VCS dirs).
fn is_internal_path(rel: &Path) -> bool {
    rel.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".wt" | ".wt-tree" | ".git" | "node_modules" | "target")
        )
    })
}

/// Debounce changes and snapshot on inactivity or file-count triggers (§6).
async fn auto_snapshot_loop(
    ctx: Arc<DaemonContext>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::watcher::debounce::DebouncedEvent>,
) {
    let mut debouncer = Debouncer::new(200);
    let mut batch: Vec<crate::watcher::debounce::DebouncedEvent> = Vec::new();
    let mut last_change = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let inactivity = Duration::from_secs(ctx.auto_snapshot.inactivity_timeout_secs.max(1));

    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                last_change = Instant::now();
                debouncer.push(event);
            }
            _ = tick.tick() => {
                batch.extend(debouncer.flush());
                if batch.is_empty() {
                    continue;
                }
                let count_trigger = batch.len() >= ctx.auto_snapshot.max_changed_files.max(1);
                let inactivity_trigger = last_change.elapsed() >= inactivity;
                if ctx.auto_snapshot.enabled && (count_trigger || inactivity_trigger) {
                    let events = std::mem::take(&mut batch);
                    take_auto_snapshot(&ctx, events).await;
                }
            }
            _ = ctx.wait_shutdown() => break,
        }
    }
}

async fn take_auto_snapshot(
    ctx: &Arc<DaemonContext>,
    events: Vec<crate::watcher::debounce::DebouncedEvent>,
) {
    let _writer = ctx.writer_lock.lock().await;
    let root = ctx.root.clone();

    let result = tokio::task::spawn_blocking(move || {
        let engine = WorktreeEngine::open(&root)
            .map_err(|e| BgError::Engine(format!("open worktree: {e}")))?;

        // Resolve the current tree once; classify events against it.
        let state = worktree_engine::persist::load_state(&engine)
            .map_err(|e| BgError::Engine(e.to_string()))?;
        let tree_name = state.current_tree.as_deref().unwrap_or("root").to_string();

        let semantic: Vec<_> = events
            .iter()
            .map(|e| classify_event(e, &tree_name))
            .collect();
        let Some(message) = AutoSnapshotEngine::new().evaluate(&semantic) else {
            return Ok(None);
        };

        match worktree_engine::ops::snapshot::create_auto_snapshot(&engine, None, &message) {
            Ok(snapshot) => Ok(Some(snapshot)),
            // The working tree settled back to the last snapshot's state.
            Err(worktree_engine::EngineError::NoChanges) => Ok(None),
            Err(e) => Err(BgError::Engine(e.to_string())),
        }
    })
    .await;

    match result {
        Ok(Ok(Some(snapshot))) => {
            tracing::info!(id = %snapshot.id, "auto-snapshot created");
            if let Ok(mut health) = ctx.health.lock() {
                health.record_snapshot();
            }
        }
        Ok(Ok(None)) => {}
        Ok(Err(e)) => tracing::warn!("auto-snapshot failed: {e}"),
        Err(e) => tracing::warn!("auto-snapshot task panicked: {e}"),
    }
}
