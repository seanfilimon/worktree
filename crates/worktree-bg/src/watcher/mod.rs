//! File-system watching for one worktree.
//!
//! A recursive `notify` watcher feeds an [`EventForwarder`], which runs on the
//! watcher's own thread: it drops metadata paths (`.wt/`), reads and ignored
//! paths, reloads ignore rules / auto-snapshot settings when their files
//! change, and forwards the rest as [`WatchEvent`]s to the worktree's worker.

pub mod debounce;
pub mod filter;

use std::path::{Path, PathBuf};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;
use worktree_sdk::Repository;

use crate::error::Result;
use debounce::DebounceSettings;
use filter::{classify, is_change, PathClass};

/// What the watcher tells a worktree worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// A non-ignored working path changed ('/'-separated, root-relative).
    Changed(String),
    /// The worktree's `[auto_snapshot]` settings were (re)loaded.
    Settings(DebounceSettings),
    /// The watcher or a config reload failed.
    Error(String),
}

/// The repository facts the forwarder needs. Implemented by [`RepoView`];
/// tests substitute fakes.
pub trait WorktreeView: Send + 'static {
    /// Whether `rel` ('/'-separated, root-relative) is ignored.
    fn is_ignored(&self, rel: &str) -> worktree_sdk::Result<bool>;
    /// Re-read config and ignore rules, returning the new settings.
    fn reload(&mut self) -> worktree_sdk::Result<DebounceSettings>;
}

/// [`WorktreeView`] backed by an open [`Repository`].
pub struct RepoView {
    root: PathBuf,
    repo: Repository,
}

impl RepoView {
    pub fn open(root: &Path) -> worktree_sdk::Result<Self> {
        Ok(Self {
            root: root.to_path_buf(),
            repo: Repository::open(root)?,
        })
    }

    pub fn settings(&self) -> DebounceSettings {
        DebounceSettings::from(&self.repo.config().auto_snapshot)
    }
}

impl WorktreeView for RepoView {
    fn is_ignored(&self, rel: &str) -> worktree_sdk::Result<bool> {
        self.repo.is_ignored(rel)
    }

    fn reload(&mut self) -> worktree_sdk::Result<DebounceSettings> {
        self.repo = Repository::open(&self.root)?;
        Ok(self.settings())
    }
}

/// `notify` event handler that filters and forwards events for one worktree.
pub struct EventForwarder<V> {
    root: PathBuf,
    view: V,
    tx: UnboundedSender<WatchEvent>,
}

impl<V: WorktreeView> EventForwarder<V> {
    pub fn new(root: PathBuf, view: V, tx: UnboundedSender<WatchEvent>) -> Self {
        Self { root, view, tx }
    }

    fn send(&self, event: WatchEvent) {
        // The worker is gone only while the watch is being torn down.
        let _ = self.tx.send(event);
    }

    fn forward(&mut self, event: notify::Event) {
        if !is_change(&event.kind) {
            return;
        }
        let mut reload = false;
        let mut changed = Vec::new();
        for path in &event.paths {
            match classify(&self.root, path) {
                None => {}
                Some(PathClass::Internal { reload: r }) => reload |= r,
                Some(PathClass::Change { rel, reload: r }) => {
                    reload |= r;
                    changed.push(rel);
                }
            }
        }
        if reload {
            match self.view.reload() {
                Ok(settings) => self.send(WatchEvent::Settings(settings)),
                Err(e) => self.send(WatchEvent::Error(format!("reloading worktree config: {e}"))),
            }
        }
        for rel in changed {
            match self.view.is_ignored(&rel) {
                Ok(true) => {}
                Ok(false) => self.send(WatchEvent::Changed(rel)),
                Err(e) => {
                    // Fail open: a spurious snapshot attempt is harmless,
                    // a silently dropped change is not.
                    tracing::warn!(path = %rel, error = %e, "ignore check failed");
                    self.send(WatchEvent::Changed(rel));
                }
            }
        }
    }
}

impl<V: WorktreeView> notify::EventHandler for EventForwarder<V> {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        match event {
            Ok(event) => self.forward(event),
            Err(e) => self.send(WatchEvent::Error(format!("file watcher: {e}"))),
        }
    }
}

/// Start a recursive watch of `root`. Events flow until the returned
/// watcher is dropped. Blocking: on some platforms this walks the tree.
pub fn watch<V: WorktreeView>(
    root: &Path,
    view: V,
    tx: UnboundedSender<WatchEvent>,
) -> Result<RecommendedWatcher> {
    let forwarder = EventForwarder::new(root.to_path_buf(), view, tx);
    let mut watcher = RecommendedWatcher::new(forwarder, Config::default())?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind, RenameMode};
    use notify::{Event, EventHandler, EventKind};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
    use worktree_sdk::SdkError;

    #[derive(Clone, Default)]
    struct FakeView {
        ignored: BTreeSet<String>,
        reloads: Arc<AtomicUsize>,
        fail_reload: bool,
    }

    impl WorktreeView for FakeView {
        fn is_ignored(&self, rel: &str) -> worktree_sdk::Result<bool> {
            if rel == "broken" {
                return Err(SdkError::Other("ignore rules unreadable".into()));
            }
            Ok(self
                .ignored
                .iter()
                .any(|p| rel == p || rel.starts_with(&format!("{p}/"))))
        }

        fn reload(&mut self) -> worktree_sdk::Result<DebounceSettings> {
            self.reloads.fetch_add(1, Ordering::SeqCst);
            if self.fail_reload {
                return Err(SdkError::InvalidConfig("bad toml".into()));
            }
            Ok(DebounceSettings {
                inactivity: Duration::from_secs(1),
                ..Default::default()
            })
        }
    }

    fn view_ignoring(paths: &[&str]) -> FakeView {
        FakeView {
            ignored: paths.iter().map(|p| p.to_string()).collect(),
            ..Default::default()
        }
    }

    fn forwarder(
        root: &Path,
        view: FakeView,
    ) -> (EventForwarder<FakeView>, UnboundedReceiver<WatchEvent>) {
        let (tx, rx) = unbounded_channel();
        (EventForwarder::new(root.to_path_buf(), view, tx), rx)
    }

    fn drain(rx: &mut UnboundedReceiver<WatchEvent>) -> Vec<WatchEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn modify(path: PathBuf) -> notify::Result<Event> {
        Ok(Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(path))
    }

    #[test]
    fn forwards_working_changes() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(modify(root.join("src").join("main.rs")));
        assert_eq!(
            drain(&mut rx),
            vec![WatchEvent::Changed("src/main.rs".into())]
        );
    }

    #[test]
    fn drops_wt_metadata_events() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(modify(
            root.join(".wt").join("objects").join("aa").join("bb"),
        ));
        fwd.handle_event(modify(root.join(".wt").join("snapshots").join("x.json")));
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn drops_ignored_paths() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, view_ignoring(&["target", "node_modules"]));
        fwd.handle_event(modify(root.join("target").join("debug").join("app")));
        fwd.handle_event(modify(root.join("node_modules").join("x.js")));
        fwd.handle_event(modify(root.join("README.md")));
        assert_eq!(
            drain(&mut rx),
            vec![WatchEvent::Changed("README.md".into())]
        );
    }

    #[test]
    fn drops_reads_and_outside_paths() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(Ok(
            Event::new(EventKind::Access(AccessKind::Read)).add_path(root.join("a"))
        ));
        fwd.handle_event(modify(std::env::temp_dir().join("other").join("a")));
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn rename_reports_both_paths() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        let ev = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(root.join("old.txt"))
            .add_path(root.join("new.txt"));
        fwd.handle_event(Ok(ev));
        assert_eq!(
            drain(&mut rx),
            vec![
                WatchEvent::Changed("old.txt".into()),
                WatchEvent::Changed("new.txt".into())
            ]
        );
    }

    #[test]
    fn config_change_reloads_settings() {
        let root = std::env::temp_dir().join("wt-fwd");
        let view = FakeView::default();
        let reloads = view.reloads.clone();
        let (mut fwd, mut rx) = forwarder(&root, view);
        fwd.handle_event(modify(root.join(".wt").join("config.toml")));
        assert_eq!(reloads.load(Ordering::SeqCst), 1);
        match drain(&mut rx).as_slice() {
            [WatchEvent::Settings(s)] => assert_eq!(s.inactivity, Duration::from_secs(1)),
            other => panic!("unexpected events {other:?}"),
        }
    }

    #[test]
    fn wtignore_change_reloads_and_counts_as_change() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(Ok(
            Event::new(EventKind::Create(CreateKind::File)).add_path(root.join(".wtignore"))
        ));
        let events = drain(&mut rx);
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(events[0], WatchEvent::Settings(_)));
        assert_eq!(events[1], WatchEvent::Changed(".wtignore".into()));
    }

    #[test]
    fn failed_reload_is_reported() {
        let root = std::env::temp_dir().join("wt-fwd");
        let view = FakeView {
            fail_reload: true,
            ..Default::default()
        };
        let (mut fwd, mut rx) = forwarder(&root, view);
        fwd.handle_event(modify(root.join(".wt").join("ignore")));
        match drain(&mut rx).as_slice() {
            [WatchEvent::Error(msg)] => assert!(msg.contains("bad toml"), "{msg}"),
            other => panic!("unexpected events {other:?}"),
        }
    }

    #[test]
    fn failed_ignore_check_fails_open() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(modify(root.join("broken")));
        assert_eq!(drain(&mut rx), vec![WatchEvent::Changed("broken".into())]);
    }

    #[test]
    fn watcher_errors_are_forwarded() {
        let root = std::env::temp_dir().join("wt-fwd");
        let (mut fwd, mut rx) = forwarder(&root, FakeView::default());
        fwd.handle_event(Err(notify::Error::generic("queue overflow")));
        match drain(&mut rx).as_slice() {
            [WatchEvent::Error(msg)] => assert!(msg.contains("queue overflow")),
            other => panic!("unexpected events {other:?}"),
        }
    }

    /// Wait until `want` arrives as a `Changed` event, collecting everything seen.
    async fn wait_for_change(
        rx: &mut UnboundedReceiver<WatchEvent>,
        want: &str,
    ) -> Vec<WatchEvent> {
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(ev)) => {
                    let done = ev == WatchEvent::Changed(want.into());
                    seen.push(ev);
                    if done {
                        return seen;
                    }
                }
                Ok(None) => panic!("watcher channel closed; saw {seen:?}"),
                Err(_) => panic!("timed out waiting for {want}; saw {seen:?}"),
            }
        }
    }

    #[tokio::test]
    async fn real_watcher_reports_working_changes_but_not_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let root = crate::paths::strip_verbatim(&root);
        std::fs::create_dir_all(root.join(".wt").join("objects")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();

        let (tx, mut rx) = unbounded_channel();
        let watcher = watch(&root, view_ignoring(&["target"]), tx).unwrap();

        std::fs::write(root.join(".wt").join("objects").join("blob"), b"meta").unwrap();
        std::fs::write(root.join("target").join("out.o"), b"build").unwrap();
        std::fs::write(root.join("hello.txt"), b"hi").unwrap();

        let seen = wait_for_change(&mut rx, "hello.txt").await;
        for ev in &seen {
            if let WatchEvent::Changed(rel) = ev {
                assert!(!rel.starts_with(".wt"), "metadata leaked: {seen:?}");
                assert!(!rel.starts_with("target"), "ignored path leaked: {seen:?}");
            }
        }
        drop(watcher);
    }
}
