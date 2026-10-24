use chrono::{DateTime, Utc};
use worktree_protocol::core::id::AccountId;

/// Represents an authenticated user session.
///
/// Sessions are time-limited tokens that authorize a user to perform
/// operations against the Worktree server. Each session has an expiration
/// time after which it is no longer valid and the user must re-authenticate.
#[derive(Debug, Clone)]
pub struct Session {
    /// The user this session belongs to.
    pub user_id: AccountId,

    /// Opaque bearer token string used to authenticate API requests.
    pub token: String,

    /// When this session expires (UTC). After this time, [`is_expired`](Self::is_expired)
    /// returns `true` and the session should be rejected.
    pub expires_at: DateTime<Utc>,
}

impl Session {
    /// Create a new session for the given user with the specified token and expiration.
    pub fn new(user_id: AccountId, token: impl Into<String>, expires_at: DateTime<Utc>) -> Self {
        Self {
            user_id,
            token: token.into(),
            expires_at,
        }
    }

    /// Returns `true` if the session has expired (i.e. the current time is
    /// at or past `expires_at`).
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Returns the remaining lifetime of this session, or `None` if it has
    /// already expired.
    pub fn remaining(&self) -> Option<chrono::Duration> {
        let remaining = self.expires_at.signed_duration_since(Utc::now());
        if remaining > chrono::Duration::zero() {
            Some(remaining)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn session_in_the_future_is_not_expired() {
        let session = Session::new(
            AccountId::new(),
            "test-token-abc",
            Utc::now() + Duration::hours(1),
        );
        assert!(!session.is_expired());
        assert!(session.remaining().is_some());
    }

    #[test]
    fn session_in_the_past_is_expired() {
        let session = Session::new(
            AccountId::new(),
            "test-token-expired",
            Utc::now() - Duration::hours(1),
        );
        assert!(session.is_expired());
        assert!(session.remaining().is_none());
    }
}
