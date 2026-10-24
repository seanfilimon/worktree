use thiserror::Error;

/// Top-level error type for the worktree protocol crate.
#[derive(Debug, Clone, Error)]
pub enum ProtocolError {
    /// A serialization operation failed.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// A deserialization operation failed.
    #[error("deserialization error: {0}")]
    Deserialization(String),

    /// A content hash did not match the expected value.
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        expected: String,
        actual: String,
    },

    /// An identifier was invalid or could not be parsed.
    #[error("invalid id: {0}")]
    InvalidId(String),

    /// The caller does not have permission for the requested operation.
    #[error("access denied: {0}")]
    AccessDenied(String),

    /// The operation violates a configured policy.
    #[error("policy violation: {0}")]
    PolicyViolation(String),

    /// A scope specification was invalid.
    #[error("invalid scope: {0}")]
    InvalidScope(String),

    /// The requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_serialization() {
        let err = ProtocolError::Serialization("bad payload".into());
        assert_eq!(err.to_string(), "serialization error: bad payload");
    }

    #[test]
    fn display_hash_mismatch() {
        let err = ProtocolError::HashMismatch {
            expected: "aabb".into(),
            actual: "ccdd".into(),
        };
        assert_eq!(err.to_string(), "hash mismatch: expected aabb, got ccdd");
    }

    #[test]
    fn display_not_found() {
        let err = ProtocolError::NotFound("tree/abc".into());
        assert_eq!(err.to_string(), "not found: tree/abc");
    }

    #[test]
    fn display_access_denied() {
        let err = ProtocolError::AccessDenied("insufficient privileges".into());
        assert_eq!(err.to_string(), "access denied: insufficient privileges");
    }

    #[test]
    fn display_policy_violation() {
        let err = ProtocolError::PolicyViolation("max size exceeded".into());
        assert_eq!(err.to_string(), "policy violation: max size exceeded");
    }

    #[test]
    fn display_invalid_scope() {
        let err = ProtocolError::InvalidScope("unknown scope token".into());
        assert_eq!(err.to_string(), "invalid scope: unknown scope token");
    }

    #[test]
    fn display_invalid_id() {
        let err = ProtocolError::InvalidId("not-a-uuid".into());
        assert_eq!(err.to_string(), "invalid id: not-a-uuid");
    }

    #[test]
    fn display_deserialization() {
        let err = ProtocolError::Deserialization("unexpected EOF".into());
        assert_eq!(err.to_string(), "deserialization error: unexpected EOF");
    }

    #[test]
    fn error_is_clone() {
        let err = ProtocolError::NotFound("x".into());
        let err2 = err.clone();
        assert_eq!(err.to_string(), err2.to_string());
    }
}
