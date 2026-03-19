use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Configuration for continuous mirroring between a Worktree repository
/// and a Git remote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorConfig {
    /// The URL of the Git remote to mirror to/from.
    pub remote_url: String,
    /// The branch to mirror.
    pub branch: String,
    /// How often (in seconds) to sync with the remote.
    pub interval_secs: u64,
}

impl MirrorConfig {
    /// Create a new `MirrorConfig`.
    pub fn new(remote_url: impl Into<String>, branch: impl Into<String>, interval_secs: u64) -> Self {
        Self {
            remote_url: remote_url.into(),
            branch: branch.into(),
            interval_secs,
        }
    }
}

/// A mirror that continuously synchronizes a Worktree repository with a
/// Git remote according to the provided [`MirrorConfig`].
#[derive(Debug)]
pub struct Mirror {
    config: MirrorConfig,
}

impl Mirror {
    /// Create a new `Mirror` from the given configuration.
    pub fn new(config: MirrorConfig) -> Self {
        Self { config }
    }

    /// Return a reference to the mirror configuration.
    pub fn config(&self) -> &MirrorConfig {
        &self.config
    }

    /// Start the mirror loop, periodically pushing and pulling changes
    /// to keep the Worktree repository and the Git remote in sync.
    ///
    /// This method blocks until an error occurs or the mirror is stopped.
    pub fn start(config: MirrorConfig) -> Result<()> {
        let _mirror = Self::new(config);
        todo!("implement mirror sync loop")
    }
}
