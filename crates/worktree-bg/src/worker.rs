//! Per-worktree worker: debounces watcher events and runs auto-snapshots.

use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;
use tokio::time::Instant;
use worktree_sdk::ipc::WatchedWorktree;

use crate::watcher::debounce::{DebounceSettings, Debouncer};
use crate::watcher::WatchEvent;

/// Live per-worktree counters reported by `Status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchStats {
    pub paused: bool,
    pub pending_changes: usize,
    pub snapshots_created: u64,
    pub last_snapshot: Option<String>,
    pub last_snapshot_at: Option<DateTime<Utc>>,
    pub last_stage_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl WatchStats {
    pub fn to_watched(&self, root: &Path) -> WatchedWorktree {
        WatchedWorktree {
            root: root.to_path_buf(),
            paused: self.paused,
            pending_changes: self.pending_changes,
            snapshots_created: self.snapshots_created,
            last_snapshot: self.last_snapshot.clone(),
            last_snapshot_at: self.last_snapshot_at,
            last_stage_at: self.last_stage_at,
            last_error: self.last_error.clone(),
        }
    }

    /// Fold the outcome of one snapshot pass into the counters.
    pub fn apply(&mut self, report: &TriggerReport, at: DateTime<Utc>) {
        self.snapshots_created += report.snapshots.len() as u64;
        if let Some(last) = report.snapshots.last() {
            self.last_snapshot = Some(last.clone());
            self.last_snapshot_at = Some(at);
        }
        if report.staged > 0 {
            self.last_stage_at = Some(at);
        }
        self.last_error = (!report.errors.is_empty()).then(|| report.errors.join("; "));
    }
}

/// Stats shared between a worker and the daemon's `Status` handler.
#[derive(Debug, Clone, Default)]
pub struct SharedStats(Arc<Mutex<WatchStats>>);

impl SharedStats {
    pub fn lock(&self) -> MutexGuard<'_, WatchStats> {
        // Stats are plain counters; a panic mid-update cannot leave them
        // in a state worth refusing to read.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> WatchStats {
        self.lock().clone()
    }
}

/// Outcome of one auto-snapshot pass over a worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerReport {
    /// Ids of the snapshots created, in tree order.
    pub snapshots: Vec<String>,
    /// Number of trees successfully staged on the remote.
    pub staged: usize,
    /// Human-readable failures (per tree or global).
    pub errors: Vec<String>,
}

impl TriggerReport {
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            errors: vec![message.into()],
            ..Self::default()
        }
    }

    /// One-line description for IPC replies and logs.
    pub fn summary(&self) -> String {
        let mut text = match self.snapshots.len() {
            0 => "no changes to snapshot".to_string(),
            1 => "created 1 snapshot".to_string(),
            n => format!("created {n} snapshots"),
        };
        if self.staged > 0 {
            text.push_str(&format!(", staged {} tree(s)", self.staged));
        }
        if !self.errors.is_empty() {
            text.push_str(&format!("; errors: {}", self.errors.join("; ")));
        }
        text
    }
}

/// The work done when a snapshot is due. Implemented by
/// [`crate::snapshot::RepoSnapshotter`]; tests substitute fakes.
pub trait SnapshotAction: Send + Sync + 'static {
    fn run(&self) -> impl Future<Output = TriggerReport> + Send;

    // --- local-config (§13.9): `auto_snapshot.max_changed_bytes` ----------
    /// Current size in bytes of the changed root-relative path (0 if gone
    /// or unknown). Only consulted when `max_changed_bytes` is set.
    fn changed_bytes(&self, _rel: &str) -> u64 {
        0
    }
}

/// Control messages from the daemon to a worker.
#[derive(Debug)]
pub enum WorkerCommand {
    /// Snapshot immediately (ignores pause and `enabled`), replying with the report.
    SnapshotNow(oneshot::Sender<TriggerReport>),
    SetPaused(bool),
}

/// Run the worker until either channel closes (the watch was removed).
pub async fn run_worker<A: SnapshotAction>(
    action: A,
    settings: DebounceSettings,
    mut events: UnboundedReceiver<WatchEvent>,
    mut commands: UnboundedReceiver<WorkerCommand>,
    stats: SharedStats,
) {
    let mut debouncer = Debouncer::new(settings);
    debouncer.set_paused(stats.lock().paused);
    loop {
        let deadline = debouncer.deadline();
        tokio::select! {
            event = events.recv() => match event {
                Some(WatchEvent::Changed(rel)) => {
                    // local-config (§13.9): track sizes for max_changed_bytes.
                    if debouncer.settings().max_changed_bytes > 0 {
                        let bytes = action.changed_bytes(&rel);
                        debouncer.record_bytes(&rel, bytes);
                    }
                    debouncer.record(rel, Instant::now());
                    stats.lock().pending_changes = debouncer.pending_count();
                }
                Some(WatchEvent::Settings(settings)) => debouncer.set_settings(settings),
                Some(WatchEvent::Error(message)) => {
                    tracing::warn!(error = %message, "watch error");
                    stats.lock().last_error = Some(message);
                }
                None => break,
            },
            command = commands.recv() => match command {
                Some(WorkerCommand::SnapshotNow(reply)) => {
                    let report = fire(&action, &mut debouncer, &stats).await;
                    // The requester may have given up waiting; nothing to do then.
                    let _ = reply.send(report);
                }
                Some(WorkerCommand::SetPaused(paused)) => {
                    debouncer.set_paused(paused);
                    stats.lock().paused = paused;
                }
                None => break,
            },
            () = sleep_until(deadline) => {}
        }
        if debouncer.is_ready(Instant::now()) {
            fire(&action, &mut debouncer, &stats).await;
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn fire<A: SnapshotAction>(
    action: &A,
    debouncer: &mut Debouncer,
    stats: &SharedStats,
) -> TriggerReport {
    let changed = debouncer.take();
    stats.lock().pending_changes = 0;
    let report = action.run().await;
    if report.errors.is_empty() {
        tracing::info!(paths = changed.len(), summary = %report.summary(), "auto-snapshot pass");
    } else {
        tracing::warn!(paths = changed.len(), summary = %report.summary(), "auto-snapshot pass failed");
    }
    stats.lock().apply(&report, Utc::now());
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
    use tokio::task::JoinHandle;

    #[derive(Clone)]
    struct FakeAction {
        calls: UnboundedSender<Instant>,
        report: TriggerReport,
    }

    impl SnapshotAction for FakeAction {
        fn run(&self) -> impl Future<Output = TriggerReport> + Send {
            let _ = self.calls.send(Instant::now());
            let report = self.report.clone();
            async move { report }
        }
    }

    struct Harness {
        events: UnboundedSender<WatchEvent>,
        commands: UnboundedSender<WorkerCommand>,
        calls: UnboundedReceiver<Instant>,
        stats: SharedStats,
        task: JoinHandle<()>,
    }

    fn settings(inactivity_secs: u64, max: usize) -> DebounceSettings {
        DebounceSettings {
            enabled: true,
            inactivity: Duration::from_secs(inactivity_secs),
            max_changed_files: max,
            max_changed_bytes: 0,
        }
    }

    fn ok_report(id: &str) -> TriggerReport {
        TriggerReport {
            snapshots: vec![id.to_string()],
            staged: 0,
            errors: Vec::new(),
        }
    }

    fn start(settings: DebounceSettings, report: TriggerReport) -> Harness {
        let (events, events_rx) = unbounded_channel();
        let (commands, commands_rx) = unbounded_channel();
        let (calls_tx, calls) = unbounded_channel();
        let stats = SharedStats::default();
        let action = FakeAction {
            calls: calls_tx,
            report,
        };
        let task = tokio::spawn(run_worker(
            action,
            settings,
            events_rx,
            commands_rx,
            stats.clone(),
        ));
        Harness {
            events,
            commands,
            calls,
            stats,
            task,
        }
    }

    impl Harness {
        fn change(&self, rel: &str) {
            self.events.send(WatchEvent::Changed(rel.into())).unwrap();
        }

        /// Next snapshot call, if one happens within `within` of virtual time.
        async fn next_call(&mut self, within: Duration) -> Option<Instant> {
            tokio::time::timeout(within, self.calls.recv())
                .await
                .ok()
                .flatten()
        }

        async fn settle(&self) {
            // Let the worker drain its channels without advancing the clock.
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn snapshots_after_inactivity() {
        let mut h = start(settings(30, 50), ok_report("s1"));
        let t0 = Instant::now();
        h.change("a.txt");
        let at = h
            .next_call(Duration::from_secs(60))
            .await
            .expect("snapshot");
        assert_eq!(at - t0, Duration::from_secs(30));
        assert!(
            h.next_call(Duration::from_secs(600)).await.is_none(),
            "only one snapshot"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn new_events_restart_the_quiet_period() {
        let mut h = start(settings(30, 50), ok_report("s1"));
        let t0 = Instant::now();
        h.change("a");
        tokio::time::sleep(Duration::from_secs(20)).await;
        h.change("b");
        tokio::time::sleep(Duration::from_secs(20)).await;
        h.change("c");
        h.settle().await;
        assert_eq!(h.stats.snapshot().pending_changes, 3);
        let at = h
            .next_call(Duration::from_secs(60))
            .await
            .expect("snapshot");
        assert_eq!(at - t0, Duration::from_secs(70));
        assert_eq!(h.stats.snapshot().pending_changes, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn max_changed_files_fires_immediately() {
        let mut h = start(settings(30, 3), ok_report("s1"));
        let t0 = Instant::now();
        h.change("a");
        h.change("b");
        h.change("a");
        h.settle().await;
        assert!(
            h.calls.try_recv().is_err(),
            "two distinct paths are below the limit"
        );
        h.change("c");
        let at = h
            .next_call(Duration::from_secs(1))
            .await
            .expect("immediate snapshot");
        assert_eq!(at, t0);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_auto_snapshot_only_counts_changes() {
        let mut h = start(
            DebounceSettings {
                enabled: false,
                ..settings(1, 1)
            },
            ok_report("s1"),
        );
        h.change("a");
        assert!(h.next_call(Duration::from_secs(3600)).await.is_none());
        assert_eq!(h.stats.snapshot().pending_changes, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn snapshot_now_ignores_pause_and_replies() {
        let mut h = start(settings(30, 50), ok_report("s9"));
        h.commands.send(WorkerCommand::SetPaused(true)).unwrap();
        h.change("a");
        h.settle().await;
        assert_eq!(h.stats.snapshot().pending_changes, 1);
        let (tx, rx) = oneshot::channel();
        h.commands.send(WorkerCommand::SnapshotNow(tx)).unwrap();
        let report = rx.await.unwrap();
        assert_eq!(report.snapshots, vec!["s9".to_string()]);
        assert!(h.calls.recv().await.is_some());
        let stats = h.stats.snapshot();
        assert_eq!(stats.snapshots_created, 1);
        assert_eq!(stats.pending_changes, 0);
        assert!(stats.paused);
    }

    #[tokio::test(start_paused = true)]
    async fn pause_defers_until_resume() {
        let mut h = start(settings(10, 50), ok_report("s1"));
        h.commands.send(WorkerCommand::SetPaused(true)).unwrap();
        h.change("a");
        assert!(h.next_call(Duration::from_secs(120)).await.is_none());
        assert!(h.stats.snapshot().paused);
        let resumed_at = Instant::now();
        h.commands.send(WorkerCommand::SetPaused(false)).unwrap();
        // The quiet period has long passed, so resuming snapshots right away.
        let at = h
            .next_call(Duration::from_secs(1))
            .await
            .expect("snapshot after resume");
        assert_eq!(at, resumed_at);
        assert!(!h.stats.snapshot().paused);
    }

    #[tokio::test(start_paused = true)]
    async fn initial_paused_flag_is_honoured() {
        let (events, events_rx) = unbounded_channel();
        let (_commands, commands_rx) = unbounded_channel();
        let (calls_tx, mut calls) = unbounded_channel();
        let stats = SharedStats::default();
        stats.lock().paused = true;
        let action = FakeAction {
            calls: calls_tx,
            report: ok_report("s"),
        };
        let _task = tokio::spawn(run_worker(
            action,
            settings(1, 50),
            events_rx,
            commands_rx,
            stats,
        ));
        events.send(WatchEvent::Changed("a".into())).unwrap();
        let call = tokio::time::timeout(Duration::from_secs(60), calls.recv()).await;
        assert!(call.is_err(), "paused worker must not snapshot");
    }

    #[tokio::test(start_paused = true)]
    async fn settings_events_take_effect() {
        let mut h = start(settings(300, 50), ok_report("s1"));
        let t0 = Instant::now();
        h.events
            .send(WatchEvent::Settings(settings(5, 50)))
            .unwrap();
        h.change("a");
        let at = h
            .next_call(Duration::from_secs(60))
            .await
            .expect("snapshot");
        assert_eq!(at - t0, Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn stats_track_successes_and_errors() {
        let report = TriggerReport {
            snapshots: vec!["a1".into(), "b2".into()],
            staged: 2,
            errors: Vec::new(),
        };
        let mut h = start(settings(1, 50), report);
        h.change("x");
        h.next_call(Duration::from_secs(5)).await.expect("snapshot");
        h.settle().await;
        let stats = h.stats.snapshot();
        assert_eq!(stats.snapshots_created, 2);
        assert_eq!(stats.last_snapshot.as_deref(), Some("b2"));
        assert!(stats.last_snapshot_at.is_some());
        assert!(stats.last_stage_at.is_some());
        assert_eq!(stats.last_error, None);

        h.events
            .send(WatchEvent::Error("watch overflow".into()))
            .unwrap();
        h.settle().await;
        assert_eq!(
            h.stats.snapshot().last_error.as_deref(),
            Some("watch overflow")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failed_pass_records_error_and_rearms_on_next_change() {
        let mut h = start(settings(1, 50), TriggerReport::failed("secrets detected"));
        h.change("x");
        h.next_call(Duration::from_secs(5))
            .await
            .expect("first attempt");
        h.settle().await;
        let stats = h.stats.snapshot();
        assert_eq!(stats.snapshots_created, 0);
        assert_eq!(stats.last_error.as_deref(), Some("secrets detected"));
        // No retry loop without new changes...
        assert!(h.next_call(Duration::from_secs(60)).await.is_none());
        // ...but the next change triggers another attempt.
        h.change("y");
        assert!(h.next_call(Duration::from_secs(5)).await.is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn worker_exits_when_commands_close() {
        let h = start(settings(1, 50), ok_report("s"));
        drop(h.commands);
        tokio::time::timeout(Duration::from_secs(1), h.task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn worker_exits_when_events_close() {
        let h = start(settings(1, 50), ok_report("s"));
        drop(h.events);
        tokio::time::timeout(Duration::from_secs(1), h.task)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn summary_describes_outcome() {
        assert_eq!(TriggerReport::default().summary(), "no changes to snapshot");
        assert_eq!(ok_report("a").summary(), "created 1 snapshot");
        let r = TriggerReport {
            snapshots: vec!["a".into(), "b".into()],
            staged: 1,
            errors: vec!["x".into()],
        };
        assert_eq!(
            r.summary(),
            "created 2 snapshots, staged 1 tree(s); errors: x"
        );
    }

    #[test]
    fn to_watched_copies_every_field() {
        let stats = WatchStats {
            paused: true,
            pending_changes: 4,
            snapshots_created: 9,
            last_snapshot: Some("id".into()),
            last_snapshot_at: Some(Utc::now()),
            last_stage_at: Some(Utc::now()),
            last_error: Some("e".into()),
        };
        let w = stats.to_watched(Path::new("/r"));
        assert_eq!(w.root, Path::new("/r"));
        assert!(w.paused);
        assert_eq!(w.pending_changes, 4);
        assert_eq!(w.snapshots_created, 9);
        assert_eq!(w.last_snapshot.as_deref(), Some("id"));
        assert_eq!(w.last_snapshot_at, stats.last_snapshot_at);
        assert_eq!(w.last_stage_at, stats.last_stage_at);
        assert_eq!(w.last_error.as_deref(), Some("e"));
    }
}
