//! Per-worktree sync loop (ENGINEERING_PLAN §13.11).
//!
//! When `[sync] auto_fetch` is on and the sync remote is configured, the
//! loop fetches every tree's remote branches (`remote::fetch`, which only
//! updates `refs/remotes/<remote>/*`), then opens a `WatchBranches` stream
//! for the trees that exist on the server and fetches a tree whenever one of
//! its branches changes. Every `sync.interval_secs` it also fetches
//! everything as a fallback for missed events.
//!
//! Failures back off exponentially (1 s doubling to 5 min, [`Backoff`]).
//! A server that cannot watch (older server, `Unimplemented`) is polled
//! every interval instead. Config is re-read on every (re)connect.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use tokio::sync::oneshot;
use tokio::time::Instant;
use worktree_sdk::{remote, Repository, SdkError};

use crate::worker::SharedStats;

/// Exponential backoff: `min`, `2·min`, `4·min`, ... capped at `max`.
#[derive(Debug, Clone)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        let min = min.max(Duration::from_millis(1));
        Self {
            min,
            max: max.max(min),
            next: min,
        }
    }

    /// The delay to wait now; doubles the following one.
    pub fn next_delay(&mut self) -> Duration {
        let current = self.next;
        self.next = (current * 2).min(self.max);
        current
    }

    /// Back to `min` (the remote was reachable).
    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

/// Tunables of the sync loop; [`Default`] is the production schedule.
#[derive(Debug, Clone)]
pub struct SyncSettings {
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// Overrides `sync.interval_secs` (tests).
    pub interval: Option<Duration>,
    /// How often a worktree with sync disabled re-reads its config.
    pub recheck: Duration,
    /// A watch stream that lived this long resets the backoff when it ends.
    pub healthy_stream: Duration,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            interval: None,
            recheck: Duration::from_secs(60),
            healthy_stream: Duration::from_secs(30),
        }
    }
}

/// Lower bound for `sync.interval_secs` (avoids hammering the server).
const MIN_INTERVAL: Duration = Duration::from_secs(5);

/// What to sync for one worktree, from its config.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    remote: String,
    interval: Duration,
    trees: Vec<String>,
}

/// The worktree's sync target; `Err(state)` when sync is disabled or the
/// worktree cannot be opened.
fn load_target(root: &Path, settings: &SyncSettings) -> Result<Target, String> {
    let repo = Repository::open(root).map_err(|e| e.to_string())?;
    let sync = &repo.config().sync;
    if !sync.auto_fetch {
        return Err("disabled".into());
    }
    if !repo.config().remote.contains_key(&sync.remote) {
        return Err(format!("disabled (no remote '{}')", sync.remote));
    }
    let interval = settings
        .interval
        .unwrap_or_else(|| Duration::from_secs(sync.interval_secs).max(MIN_INTERVAL));
    let trees = repo
        .trees()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.name)
        .collect();
    Ok(Target {
        remote: sync.remote.clone(),
        interval,
        trees,
    })
}

fn set_state(stats: &SharedStats, state: &str, error: Option<String>) {
    let mut s = stats.lock();
    s.sync_state = Some(state.to_string());
    if error.is_some() {
        s.sync_error = error;
    }
}

/// Fetch `trees` (all trees when `None`). Trees not on the server are
/// skipped. Returns the number of branches whose remote tip changed.
async fn fetch(root: &Path, target: &Target, trees: Option<&[String]>) -> Result<usize, SdkError> {
    let repo = Repository::open(root)?;
    let mut changed = 0;
    for tree in trees.unwrap_or(&target.trees) {
        match remote::fetch(&repo, tree, &target.remote).await {
            Ok(report) => {
                if !report.updated.is_empty() {
                    tracing::info!(root = %root.display(), %tree, branches = report.updated.len(), snapshots = report.snapshots_received, "fetched remote branches");
                }
                changed += report.updated.len();
            }
            Err(SdkError::TreeNotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(changed)
}

fn record_fetch(stats: &SharedStats) {
    let mut s = stats.lock();
    s.last_fetch_at = Some(Utc::now());
    s.sync_error = None;
}

/// Wait `delay` unless stopped first; returns whether to stop.
async fn wait(stop: &mut oneshot::Receiver<()>, delay: Duration) -> bool {
    tokio::select! {
        _ = stop => true,
        () = tokio::time::sleep(delay) => false,
    }
}

/// Open the watch: `Ok(None)` when the server cannot watch (poll instead).
async fn open_watch(root: &Path, target: &Target) -> Result<Option<remote::TreeWatch>, SdkError> {
    let repo = Repository::open(root)?;
    match remote::watch_trees(&repo, &target.trees, &target.remote).await {
        Ok(w) => Ok(Some(w)),
        // Nothing pushed yet, or a server without WatchBranches: poll.
        Err(SdkError::TreeNotFound(_)) => Ok(None),
        Err(SdkError::Remote(m)) if m.contains("does not support") => Ok(None),
        Err(e) => Err(e),
    }
}

/// Run the sync loop for the worktree at `root` until `stop` fires (or its
/// sender is dropped).
pub async fn run_sync(
    root: PathBuf,
    stats: SharedStats,
    settings: SyncSettings,
    mut stop: oneshot::Receiver<()>,
) {
    let mut backoff = Backoff::new(settings.backoff_min, settings.backoff_max);
    loop {
        let target = match tokio::task::spawn_blocking({
            let root = root.clone();
            let settings = settings.clone();
            move || load_target(&root, &settings)
        })
        .await
        {
            Ok(Ok(t)) => t,
            Ok(Err(reason)) => {
                set_state(&stats, &reason, None);
                if wait(&mut stop, settings.recheck).await {
                    return;
                }
                continue;
            }
            Err(e) => {
                tracing::warn!(error = %e, "sync config task failed");
                if wait(&mut stop, settings.recheck).await {
                    return;
                }
                continue;
            }
        };

        // Catch up on everything first.
        if let Err(e) = fetch(&root, &target, None).await {
            let delay = backoff.next_delay();
            tracing::debug!(root = %root.display(), error = %e, ?delay, "fetch failed; backing off");
            set_state(&stats, "offline", Some(e.to_string()));
            if wait(&mut stop, delay).await {
                return;
            }
            continue;
        }
        record_fetch(&stats);

        let watch = match open_watch(&root, &target).await {
            Ok(Some(w)) => w,
            Ok(None) => {
                backoff.reset();
                set_state(&stats, "polling", None);
                if wait(&mut stop, target.interval).await {
                    return;
                }
                continue;
            }
            Err(e) => {
                let delay = backoff.next_delay();
                set_state(&stats, "offline", Some(e.to_string()));
                if wait(&mut stop, delay).await {
                    return;
                }
                continue;
            }
        };
        set_state(&stats, "watching", None);
        tracing::info!(root = %root.display(), trees = watch.trees.len(), "watching remote branches");
        let healthy = match watch_loop(&root, &target, watch, &stats, &settings, &mut stop).await {
            WatchEnd::Stopped => return,
            WatchEnd::Broken { healthy, error } => {
                tracing::info!(root = %root.display(), %error, "remote watch ended; reconnecting");
                set_state(&stats, "offline", Some(error));
                healthy
            }
        };
        if healthy {
            backoff.reset();
        }
        let delay = backoff.next_delay();
        if wait(&mut stop, delay).await {
            return;
        }
    }
}

enum WatchEnd {
    Stopped,
    /// The stream (or a fetch) failed; `healthy` when it had been working.
    Broken {
        healthy: bool,
        error: String,
    },
}

async fn watch_loop(
    root: &Path,
    target: &Target,
    mut watch: remote::TreeWatch,
    stats: &SharedStats,
    settings: &SyncSettings,
    stop: &mut oneshot::Receiver<()>,
) -> WatchEnd {
    let opened = Instant::now();
    let trees: BTreeMap<String, String> = watch.trees.clone();
    let mut got_event = false;
    let mut next_poll = Instant::now() + target.interval;
    loop {
        let fetched = tokio::select! {
            _ = &mut *stop => return WatchEnd::Stopped,
            ev = watch.stream.next() => match ev {
                Ok(Some(ev)) => {
                    got_event = true;
                    let Some(tree) = trees.get(&ev.tree_id) else { continue };
                    tracing::debug!(%tree, branch = %ev.branch, "remote branch changed");
                    fetch(root, target, Some(std::slice::from_ref(tree))).await
                }
                Ok(None) => Err(SdkError::Remote("server ended the watch".into())),
                Err(e) => Err(e),
            },
            () = tokio::time::sleep_until(next_poll) => {
                next_poll = Instant::now() + target.interval;
                fetch(root, target, None).await
            }
        };
        match fetched {
            Ok(_) => record_fetch(stats),
            Err(e) => {
                return WatchEnd::Broken {
                    healthy: got_event || opened.elapsed() >= settings.healthy_stream,
                    error: e.to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_doubles_to_five_minutes() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(300));
        let secs: Vec<u64> = (0..12).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300, 300]);
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(2));
    }

    #[test]
    fn backoff_bounds_are_sane() {
        let mut b = Backoff::new(Duration::ZERO, Duration::ZERO);
        assert!(b.next_delay() > Duration::ZERO);
        let mut b = Backoff::new(Duration::from_secs(10), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(10));
        assert_eq!(b.next_delay(), Duration::from_secs(10));
    }

    #[test]
    fn default_settings_match_the_contract() {
        let s = SyncSettings::default();
        assert_eq!(s.backoff_min, Duration::from_secs(1));
        assert_eq!(s.backoff_max, Duration::from_secs(300));
        assert_eq!(s.interval, None);
    }

    #[test]
    fn target_follows_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = Repository::init(dir.path(), Default::default()).unwrap();
        let settings = SyncSettings::default();
        let err = load_target(dir.path(), &settings).unwrap_err();
        assert!(err.starts_with("disabled (no remote"), "{err}");

        let mut cfg = repo.config().clone();
        cfg.set("remote.origin.url", "http://127.0.0.1:1").unwrap();
        cfg.set("remote.origin.tenant", "acme").unwrap();
        cfg.sync.interval_secs = 1;
        repo.set_config(cfg.clone()).unwrap();
        let t = load_target(dir.path(), &settings).unwrap();
        assert_eq!(t.remote, "origin");
        assert_eq!(t.interval, MIN_INTERVAL, "interval is clamped");
        assert_eq!(t.trees, vec![repo.root_tree().unwrap().name]);

        cfg.sync.auto_fetch = false;
        repo.set_config(cfg).unwrap();
        assert_eq!(load_target(dir.path(), &settings).unwrap_err(), "disabled");
    }

    #[tokio::test]
    async fn offline_remote_backs_off_and_stops_promptly() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = Repository::init(dir.path(), Default::default()).unwrap();
        let mut cfg = repo.config().clone();
        // Nothing listens on port 1: every connect fails.
        cfg.set("remote.origin.url", "http://127.0.0.1:1").unwrap();
        cfg.set("remote.origin.tenant", "acme").unwrap();
        repo.set_config(cfg).unwrap();
        let stats = SharedStats::default();
        let (stop_tx, stop_rx) = oneshot::channel();
        let task = tokio::spawn(run_sync(
            dir.path().to_path_buf(),
            stats.clone(),
            SyncSettings::default(),
            stop_rx,
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while stats.snapshot().sync_state.as_deref() != Some("offline") {
            assert!(
                std::time::Instant::now() < deadline,
                "{:?}",
                stats.snapshot()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(stats.snapshot().sync_error.is_some());
        assert!(stats.snapshot().last_fetch_at.is_none());
        drop(stop_tx);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("stops")
            .unwrap();
    }
}
