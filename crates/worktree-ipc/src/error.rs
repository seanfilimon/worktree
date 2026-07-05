use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("encoding error: {0}")]
    Encode(#[source] serde_json::Error),

    #[error("decoding error: {0}")]
    Decode(#[source] serde_json::Error),

    #[error("frame too large: {0} bytes (max {max})", max = crate::frame::MAX_FRAME_LEN)]
    FrameTooLarge(usize),

    #[error("connection closed by peer")]
    ConnectionClosed,

    #[error("daemon not running for this worktree")]
    DaemonUnavailable,
}

pub type Result<T> = std::result::Result<T, IpcError>;
