use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    /// Error from the embedded local engine.
    #[error(transparent)]
    Engine(#[from] worktree_engine::EngineError),

    /// The daemon was required for this operation but is not running.
    #[error("worktree daemon is not running (start it with `wt server start`)")]
    DaemonUnavailable,
}

pub type Result<T> = std::result::Result<T, SdkError>;
