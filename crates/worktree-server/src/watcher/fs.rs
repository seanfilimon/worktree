use std::path::Path;
use std::sync::mpsc;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::ServerError;

/// Watches a directory tree for file-system changes using the platform's
/// native file-notification API (via the `notify` crate).
pub struct FileSystemWatcher {
    /// The underlying recommended watcher handle. Dropping this stops watching.
    watcher: RecommendedWatcher,
    /// Receiver for raw file-system events produced by the watcher.
    pub receiver: mpsc::Receiver<Result<Event, notify::Error>>,
}

impl FileSystemWatcher {
    /// Create a new `FileSystemWatcher`.
    ///
    /// Events will be delivered to the internal receiver which can be
    /// consumed via [`Self::receiver`].
    pub fn new() -> Result<Self, ServerError> {
        let (tx, rx) = mpsc::channel();

        let watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                // Best-effort send — if the receiver is dropped we silently ignore.
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|e| ServerError::Watcher(format!("failed to create fs watcher: {}", e)))?;

        Ok(Self {
            watcher,
            receiver: rx,
        })
    }

    /// Start watching the given path recursively.
    ///
    /// All file-system events under `path` will be forwarded to the
    /// internal receiver channel.
    pub fn watch(&mut self, path: &Path) -> Result<(), ServerError> {
        self.watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| {
                ServerError::Watcher(format!(
                    "failed to watch path {}: {}",
                    path.display(),
                    e
                ))
            })
    }

    /// Stop watching the given path.
    ///
    /// If the path was not previously watched this is a no-op.
    pub fn unwatch(&mut self, path: &Path) -> Result<(), ServerError> {
        self.watcher.unwatch(path).map_err(|e| {
            ServerError::Watcher(format!(
                "failed to unwatch path {}: {}",
                path.display(),
                e
            ))
        })
    }

    /// Stop watching all paths by dropping and re-creating the inner watcher.
    pub fn stop(&mut self) -> Result<(), ServerError> {
        let (tx, rx) = mpsc::channel();

        let watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|e| {
            ServerError::Watcher(format!("failed to recreate fs watcher on stop: {}", e))
        })?;

        self.watcher = watcher;
        self.receiver = rx;
        Ok(())
    }
}
