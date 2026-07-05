use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("object not found: {0}")]
    ObjectNotFound(String),

    #[error("object corrupt: {0}")]
    Corrupt(String),

    #[error("ref not found: {0}")]
    RefNotFound(String),

    #[error("ref update conflict: expected {expected}, found {actual}")]
    RefConflict { expected: String, actual: String },

    #[error("store is locked by another process")]
    Locked,

    #[error("no platform data directory available")]
    NoDataDir,
}

pub type Result<T> = std::result::Result<T, StoreError>;
