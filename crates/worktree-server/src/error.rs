use thiserror::Error;

/// Top-level error type for the Worktree server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Configuration error.
    #[error("config error: {0}")]
    Config(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// File-system watcher error.
    #[error("watcher error: {0}")]
    Watcher(String),

    /// Storage backend error.
    #[error("storage error: {0}")]
    Storage(String),

    /// Engine / rule-evaluation error.
    #[error("engine error: {0}")]
    Engine(String),

    /// Authentication / authorization error.
    #[error("auth error: {0}")]
    Auth(String),

    /// API layer error.
    #[error("api error: {0}")]
    Api(String),

    /// Git interoperability error.
    #[error("git error: {0}")]
    Git(String),
}
