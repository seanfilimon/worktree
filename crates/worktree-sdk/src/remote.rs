//! IPC client to a running `worktree-bg` daemon.
//!
//! WT-PHASE-3 implements this over `worktree-ipc`'s transports. Until
//! then, connection attempts always report the daemon as unavailable so
//! [`crate::Client`] falls back to the embedded engine.

use std::path::Path;

/// Handle to a daemon serving one worktree.
pub struct RemoteClient;

impl RemoteClient {
    /// Try to connect to the daemon for the worktree at `root`.
    ///
    /// Returns `None` when no daemon is listening (the caller falls back
    /// to embedded mode).
    pub fn try_connect(_root: &Path) -> Option<Self> {
        // WT-PHASE-3: dial worktree_ipc::endpoint::endpoint_for(root).
        None
    }
}
