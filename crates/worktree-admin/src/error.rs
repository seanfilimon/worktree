//! Error types for the Worktree admin panel

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fmt;

/// Result type for admin operations
pub type Result<T> = std::result::Result<T, AdminError>;

/// Admin panel error types
#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    /// Server connection error
    #[error("Failed to connect to server: {0}")]
    ServerConnection(String),

    /// Server operation error
    #[error("Server operation failed: {0}")]
    ServerOperation(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization error
    #[error("Not authorized: {0}")]
    Authorization(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Invalid request
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(String),
}

impl AdminError {
    /// Get the HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::ServerConnection(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::ServerOperation(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Authentication(_) => StatusCode::UNAUTHORIZED,
            Self::Authorization(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Serialization(_) => StatusCode::BAD_REQUEST,
            Self::Protocol(_) => StatusCode::BAD_GATEWAY,
        }
    }

    /// Get the error code string
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::ServerConnection(_) => "SERVER_CONNECTION",
            Self::ServerOperation(_) => "SERVER_OPERATION",
            Self::Config(_) => "CONFIG_ERROR",
            Self::Authentication(_) => "AUTH_FAILED",
            Self::Authorization(_) => "NOT_AUTHORIZED",
            Self::NotFound(_) => "NOT_FOUND",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Protocol(_) => "PROTOCOL_ERROR",
        }
    }
}

/// Implement IntoResponse for AdminError to allow it to be returned from handlers
impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_code = self.error_code();
        let message = self.to_string();

        tracing::error!(
            error_code = error_code,
            status = %status,
            message = %message,
            "Admin panel error"
        );

        let body = Json(json!({
            "status": "error",
            "code": error_code,
            "message": message,
        }));

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_codes() {
        assert_eq!(
            AdminError::NotFound("test".to_string()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AdminError::Authentication("test".to_string()).status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            AdminError::InvalidRequest("test".to_string()).status_code(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            AdminError::NotFound("test".to_string()).error_code(),
            "NOT_FOUND"
        );
        assert_eq!(
            AdminError::Authentication("test".to_string()).error_code(),
            "AUTH_FAILED"
        );
    }
}
