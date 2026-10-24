use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("not a worktree repository (no .wt directory found)")]
    NotAWorktree,

    #[error("worktree already initialized at this path")]
    AlreadyInitialized,

    #[error("tree not found: {0}")]
    TreeNotFound(String),

    #[error("tree already exists: {0}")]
    TreeExists(String),

    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error("branch already exists: {0}")]
    BranchExists(String),

    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("ambiguous snapshot id prefix: {0}")]
    AmbiguousId(String),

    #[error("invalid revision: {0}")]
    InvalidRevision(String),

    #[error("object not found: {0}")]
    ObjectNotFound(String),

    #[error("corrupt object {0}: {1}")]
    CorruptObject(String, String),

    #[error("no changes to snapshot")]
    NoChanges,

    #[error("working tree has uncommitted changes ({0} files); snapshot them or use --force")]
    DirtyWorkingTree(usize),

    #[error("merge conflict: {0}")]
    MergeConflict(String),

    #[error("a merge is in progress; resolve conflicts and run `wt merge --continue` or `wt merge --abort`")]
    MergeInProgress,

    #[error("no merge in progress")]
    NoMergeInProgress,

    #[error("branch protection violation: {0}")]
    BranchProtection(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("possible secrets detected: {0}")]
    SecretsDetected(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("tag already exists: {0}")]
    TagExists(String),

    #[error("tag not found: {0}")]
    TagNotFound(String),

    #[error("remote not configured: {0}")]
    RemoteNotConfigured(String),

    #[error("not logged in to {0}; run `wt login`")]
    NotLoggedIn(String),

    #[error("remote error: {0}")]
    Remote(String),

    #[error("push rejected: {0}")]
    PushRejected(String),

    #[error("background daemon is not running")]
    DaemonNotRunning,

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for SdkError {
    fn from(e: serde_json::Error) -> Self {
        SdkError::Serialization(e.to_string())
    }
}

impl From<toml::de::Error> for SdkError {
    fn from(e: toml::de::Error) -> Self {
        SdkError::Serialization(e.to_string())
    }
}

impl From<toml::ser::Error> for SdkError {
    fn from(e: toml::ser::Error) -> Self {
        SdkError::Serialization(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SdkError>;
