//! Error types for the worktree-bg local daemon.
//!
//! Variants are pre-declared so the subsequent WT-EXTRACT-2 module moves
//! can reference `BgError::*` without back-and-forth churn across commits.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BgError {
    #[error("watcher error: {0}")]
    Watcher(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("sync error: {0}")]
    Sync(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("ipc error: {0}")]
    Ipc(#[from] worktree_ipc::IpcError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
