use thiserror::Error;

/// Errors that can occur in the Git compatibility layer.
#[derive(Debug, Error)]
pub enum GitCompatError {
    /// An error originating from the `git2` library.
    #[error("git error: {0}")]
    GitError(#[from] git2::Error),

    /// An error related to the Worktree protocol layer.
    #[error("protocol error: {0}")]
    ProtocolError(String),

    /// A standard I/O error.
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    /// An error in the hash index (Git SHA-1 ↔ BLAKE3 mapping).
    #[error("hash index error: {0}")]
    HashIndexError(String),

    /// An error during Git-to-Worktree import.
    #[error("import error: {0}")]
    ImportError(String),

    /// An error during Worktree-to-Git export.
    #[error("export error: {0}")]
    ExportError(String),

    /// An error related to Git remote operations.
    #[error("remote error: {0}")]
    RemoteError(String),

    /// An error related to configuration parsing or conversion.
    #[error("config error: {0}")]
    ConfigError(String),
}

/// Convenience alias used throughout this crate.
pub type Result<T> = std::result::Result<T, GitCompatError>;
