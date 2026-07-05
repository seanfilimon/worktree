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

    /// Storage backend error.
    #[error("storage error: {0}")]
    Storage(String),

    /// Authentication / authorization error.
    ///
    /// Forward-declared for the IAM enforcement layer per Server.md §7.
    /// Currently has no callers (auth/enforcer.rs is mostly stub); kept as
    /// scaffolding because the server-side auth domain stays in this crate
    /// even after the WT-EXTRACT-* trilogy.
    #[error("auth error: {0}")]
    Auth(String),

    /// API layer error.
    #[error("api error: {0}")]
    Api(String),
}
