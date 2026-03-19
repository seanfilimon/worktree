use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::ServerError;

/// The background daemon that manages the Worktree server lifecycle.
///
/// `Daemon` owns the main event loop and coordinates starting and stopping
/// all server subsystems (watcher, engine, API, sync, etc.).
pub struct Daemon {
    /// Whether the daemon is currently running.
    running: Arc<AtomicBool>,
}

impl Daemon {
    /// Create a new `Daemon` instance in the stopped state.
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the daemon, launching all subsystems.
    ///
    /// Returns an error if the daemon is already running or if any subsystem
    /// fails to initialize.
    pub async fn start(&self) -> Result<(), ServerError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(ServerError::Engine("daemon is already running".into()));
        }
        self.running.store(true, Ordering::SeqCst);
        tracing::info!("Daemon started");
        todo!("launch subsystems: watcher, engine, API server, sync")
    }

    /// Gracefully stop the daemon, shutting down all subsystems.
    ///
    /// Returns an error if the daemon is not currently running or if shutdown
    /// fails to complete cleanly.
    pub async fn stop(&self) -> Result<(), ServerError> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(ServerError::Engine("daemon is not running".into()));
        }
        self.running.store(false, Ordering::SeqCst);
        tracing::info!("Daemon stopped");
        todo!("gracefully shut down all subsystems")
    }

    /// Returns `true` if the daemon is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new()
    }
}
