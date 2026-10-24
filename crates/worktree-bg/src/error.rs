//! Error type for the background daemon.

use std::path::PathBuf;

use thiserror::Error;
use worktree_sdk::SdkError;

#[derive(Debug, Error)]
pub enum BgError {
    #[error(transparent)]
    Sdk(#[from] SdkError),

    #[error("file watcher error: {0}")]
    Watcher(#[from] notify::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid daemon config {path}: {message}")]
    Config { path: PathBuf, message: String },

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("{0}")]
    InvalidRequest(String),

    #[error("not watching {0}")]
    NotWatched(PathBuf),

    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl From<serde_json::Error> for BgError {
    fn from(e: serde_json::Error) -> Self {
        BgError::Serialization(e.to_string())
    }
}

impl From<toml::ser::Error> for BgError {
    fn from(e: toml::ser::Error) -> Self {
        BgError::Serialization(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, BgError>;
