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
//!
//! Next to it runs the outbox loop (ENGINEERING_PLAN §15.5,
//! [`run_outbox`]): with `sync.auto_flush` (default on) pushes queued in
//! `.wt/trees/<tree>/outbox.jsonl` while the server was unreachable are
//! retried — immediately when the queue changes, then with the same
//! exponential backoff — until the server accepts them; with
//! `sync.auto_sync_secs = N` (default 0 = off) every clean tree gets a full
//! `wt sync` every N seconds, conflicting integrations being undone.

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
    /// sync workstream (§15.5): how often the outbox loop re-reads the
    /// config and looks for queued pushes.
    pub outbox_poll: Duration,
    /// sync workstream (§15.5): overrides `sync.auto_sync_secs` (tests).
    pub auto_sync: Option<Duration>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            interval: None,
            recheck: Duration::from_secs(60),
            healthy_stream: Duration::from_secs(30),
            outbox_poll: Duration::from_secs(10),
            auto_sync: None,
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

/// Run the sync loops for the worktree at `root` until `stop` fires (or its
/// sender is dropped): the fetch loop and (sync workstream, §15.5) the
/// outbox / auto-sync loop ([`run_outbox`]).
pub async fn run_sync(
    root: PathBuf,
    stats: SharedStats,
    settings: SyncSettings,
    stop: oneshot::Receiver<()>,
) {
    // The outbox loop never ends on its own; it is dropped (cancelled at
    // its next await point) once the fetch loop sees `stop`.
    tokio::select! {
        () = run_fetch(root.clone(), stats, settings.clone(), stop) => {}
        () = run_outbox(root, settings) => {}
    }
}

/// The fetch loop (§13.11).
async fn run_fetch(
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

// --------------------------------------------------------------------
// sync workstream (§15.5): outbox flushing and periodic auto-sync
// --------------------------------------------------------------------

/// What the outbox loop should do for one worktree, from its config.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OutboxPlan {
    /// `sync.auto_flush`.
    auto_flush: bool,
    /// Trees with queued pushes.
    queued: Vec<String>,
    /// `(entry id, tip)` of every queued push: a change (new entry, newer
    /// tip) is retried right away instead of waiting out the backoff.
    fingerprint: Vec<(String, String)>,
    /// `sync.auto_sync_secs` (or the test override); `None` = off or no
    /// sync remote configured.
    auto_sync: Option<Duration>,
    remote: String,
    trees: Vec<String>,
}

fn load_outbox_plan(root: &Path, settings: &SyncSettings) -> Result<OutboxPlan, SdkError> {
    let repo = Repository::open(root)?;
    let sync = &repo.config().sync;
    let trees: Vec<String> = repo.trees()?.into_iter().map(|t| t.name).collect();
    let mut queued = Vec::new();
    let mut fingerprint = Vec::new();
    for tree in &trees {
        let entries = repo.outbox(tree)?;
        if !entries.is_empty() {
            queued.push(tree.clone());
        }
        fingerprint.extend(entries.into_iter().map(|e| (e.id, e.tip)));
    }
    let configured = repo.config().remote.contains_key(&sync.remote);
    let auto_sync = settings
        .auto_sync
        .or_else(|| (sync.auto_sync_secs > 0).then(|| Duration::from_secs(sync.auto_sync_secs)))
        .filter(|_| configured)
        .map(|d| d.max(Duration::from_millis(100)));
    Ok(OutboxPlan {
        auto_flush: sync.auto_flush,
        queued,
        fingerprint,
        auto_sync,
        remote: sync.remote.clone(),
        trees,
    })
}

/// Deliver the queued pushes of `trees`. Returns `Ok(true)` when every
/// entry went out, `Ok(false)` when some are still queued.
async fn flush_trees(root: &Path, trees: &[String]) -> Result<bool, SdkError> {
    let repo = Repository::open(root)?;
    let mut all = true;
    for tree in trees {
        let report = remote::flush_outbox(&repo, tree).await?;
        for d in &report.delivered {
            tracing::info!(root = %root.display(), %tree, branch = %d.entry.branch, remote = %d.entry.remote, tip = %d.push.new_tip, "delivered queued push");
        }
        for f in &report.failed {
            tracing::debug!(root = %root.display(), %tree, branch = %f.entry.branch, error = %f.error, "queued push not delivered");
        }
        all &= report.remaining == 0;
    }
    Ok(all)
}

/// A full `wt sync` of every clean tree (conflicting integrations are
/// undone, never left in the working tree). Returns whether every sync
/// reached the server.
async fn auto_sync(root: &Path, plan: &OutboxPlan) -> Result<bool, SdkError> {
    let repo = Repository::open(root)?;
    let opts = remote::SyncOptions {
        abort_on_conflict: true,
        ..Default::default()
    };
    let mut online = true;
    for tree in &plan.trees {
        let idle = repo.merge_state(tree)?.is_none() && repo.rewrite_state(tree)?.is_none();
        if !idle || !repo.status(tree)?.is_clean() {
            tracing::debug!(root = %root.display(), %tree, "auto-sync skipped: tree is not clean");
            continue;
        }
        match remote::sync(&repo, tree, &plan.remote, opts).await {
            Ok(r) => {
                use remote::SyncStatus;
                match r.status {
                    SyncStatus::UpToDate => {}
                    SyncStatus::Queued | SyncStatus::Offline => online = false,
                    SyncStatus::Aborted => {
                        tracing::warn!(root = %root.display(), %tree, branch = %r.branch, "auto-sync: integrating the remote branch conflicts; run `wt sync` to resolve")
                    }
                    status => {
                        tracing::info!(root = %root.display(), %tree, branch = %r.branch, ?status, integration = ?r.integration, "auto-synced")
                    }
                }
            }
            // Not on the server, not logged in, ...: nothing to do here.
            Err(e) => {
                tracing::debug!(root = %root.display(), %tree, error = %e, "auto-sync failed")
            }
        }
    }
    Ok(online)
}

/// The outbox loop of one worktree: with `sync.auto_flush`, deliver queued
/// pushes (retrying with exponential backoff while the server is
/// unreachable or a push is refused); with `sync.auto_sync_secs`, run a full
/// sync of every clean tree on that schedule. Config is re-read every
/// `outbox_poll`.
pub async fn run_outbox(root: PathBuf, settings: SyncSettings) {
    let mut backoff = Backoff::new(settings.backoff_min, settings.backoff_max);
    // Flush retries wait until this instant (backoff).
    let mut retry_at = Instant::now();
    let mut next_auto_sync: Option<Instant> = None;
    let mut seen: Vec<(String, String)> = Vec::new();
    loop {
        let plan = tokio::task::spawn_blocking({
            let root = root.clone();
            let settings = settings.clone();
            move || load_outbox_plan(&root, &settings)
        })
        .await;
        let mut wake = Instant::now() + settings.outbox_poll;
        if let Ok(Ok(plan)) = plan {
            if plan.fingerprint != seen {
                // New or updated entries: try them now.
                backoff.reset();
                retry_at = Instant::now();
                seen = plan.fingerprint.clone();
            }
            if plan.auto_flush && !plan.queued.is_empty() {
                if Instant::now() >= retry_at {
                    match flush_trees(&root, &plan.queued).await {
                        Ok(true) => backoff.reset(),
                        Ok(false) => retry_at = Instant::now() + backoff.next_delay(),
                        Err(e) => {
                            tracing::debug!(root = %root.display(), error = %e, "outbox flush failed");
                            retry_at = Instant::now() + backoff.next_delay();
                        }
                    }
                }
                if retry_at > Instant::now() {
                    wake = wake.min(retry_at);
                }
            } else {
                backoff.reset();
                retry_at = Instant::now();
            }
            match plan.auto_sync {
                Some(every) => {
                    let due = *next_auto_sync.get_or_insert_with(|| Instant::now() + every);
                    if Instant::now() >= due {
                        let online = auto_sync(&root, &plan).await.unwrap_or(false);
                        next_auto_sync = Some(Instant::now() + every);
                        if !online {
                            // Queued while offline: let the flush back off.
                            retry_at = retry_at.max(Instant::now() + settings.backoff_min);
                        }
                    }
                    if let Some(at) = next_auto_sync {
                        wake = wake.min(at);
                    }
                }
                None => next_auto_sync = None,
            }
        }
        tokio::time::sleep_until(wake).await;
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
