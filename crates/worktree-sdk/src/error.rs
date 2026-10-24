use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("not a worktree repository (no .wt directory found)")]
    NotAWorktree,

    #[error("worktree already initialized at this path")]
    AlreadyInitialized,

    #[error("tree not found: {0}")]
    TreeNotFound(String),

    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("no changes to snapshot")]
    NoChanges,

    #[error("merge conflict: {0}")]
    MergeConflict(String),

    #[error("branch protection violation: {0}")]
    BranchProtection(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("tag already exists: {0}")]
    TagExists(String),

    #[error("tag not found: {0}")]
    TagNotFound(String),
}

pub type Result<T> = std::result::Result<T, SdkError>;
