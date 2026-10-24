use std::time::Instant;

/// Health status of the Worktree server daemon.
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Number of seconds the server has been running.
    pub uptime_secs: u64,
    /// Number of trees currently being watched.
    pub trees_watched: usize,
    /// Total number of snapshots created since the server started.
    pub snapshots_created: u64,
}

impl HealthStatus {
    /// Returns `true` if the server appears healthy (non-zero uptime).
    pub fn is_healthy(&self) -> bool {
        self.uptime_secs > 0
    }
}

/// Check the current health of the server.
///
/// In the future this will query the running daemon state; for now it
/// returns a placeholder status.
pub fn check_health() -> HealthStatus {
    HealthStatus {
        uptime_secs: 0,
        trees_watched: 0,
        snapshots_created: 0,
    }
}

/// A running health tracker that records the server start time and
/// accumulates runtime statistics.
pub struct HealthTracker {
    started_at: Instant,
    trees_watched: usize,
    snapshots_created: u64,
}

impl HealthTracker {
    /// Create a new tracker, recording the current instant as the start time.
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            trees_watched: 0,
            snapshots_created: 0,
        }
    }

    /// Update the number of trees currently being watched.
    pub fn set_trees_watched(&mut self, count: usize) {
        self.trees_watched = count;
    }

    /// Increment the snapshot counter by one.
    pub fn record_snapshot(&mut self) {
        self.snapshots_created += 1;
    }

    /// Produce a [`HealthStatus`] snapshot from the current tracker state.
    pub fn status(&self) -> HealthStatus {
        HealthStatus {
            uptime_secs: self.started_at.elapsed().as_secs(),
            trees_watched: self.trees_watched,
            snapshots_created: self.snapshots_created,
        }
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}
