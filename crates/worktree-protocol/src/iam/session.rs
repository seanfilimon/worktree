//! # Authentication Sessions
//!
//! Sessions represent authenticated periods for an account. Each session
//! has a token, expiry time, and can be revoked or refreshed.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{AccountId, SessionId, TenantId};

/// The current status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionStatus {
    /// The session is active and valid.
    Active,
    /// The session has expired (past its `expires_at` time).
    Expired,
    /// The session was explicitly revoked.
    Revoked,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Active => write!(f, "active"),
            SessionStatus::Expired => write!(f, "expired"),
            SessionStatus::Revoked => write!(f, "revoked"),
        }
    }
}

/// An authenticated session for an account.
///
/// Sessions are created when an account authenticates and are used to
/// validate subsequent requests. They carry metadata about the client
/// (IP address, user agent) and can be refreshed to extend their lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// The account this session belongs to.
    pub account_id: AccountId,
    /// The tenant the account belongs to.
    pub tenant_id: TenantId,
    /// The opaque session token (e.g., a JWT or random bearer token).
    pub token: String,
    /// Current status of the session.
    pub status: SessionStatus,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
    /// When the session expires.
    pub expires_at: DateTime<Utc>,
    /// When the session was last used.
    pub last_active_at: DateTime<Utc>,
    /// The IP address of the client that created the session.
    pub ip_address: Option<String>,
    /// The user agent string of the client that created the session.
    pub user_agent: Option<String>,
}

impl Session {
    /// Create a new active session with the given duration.
    ///
    /// The session starts now and expires after `duration`.
    pub fn new(
        account_id: AccountId,
        tenant_id: TenantId,
        token: String,
        duration: Duration,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            account_id,
            tenant_id,
            token,
            status: SessionStatus::Active,
            created_at: now,
            expires_at: now + duration,
            last_active_at: now,
            ip_address: None,
            user_agent: None,
        }
    }

    /// Create a new session with client metadata.
    pub fn with_metadata(
        account_id: AccountId,
        tenant_id: TenantId,
        token: String,
        duration: Duration,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        let mut session = Self::new(account_id, tenant_id, token, duration);
        session.ip_address = ip_address;
        session.user_agent = user_agent;
        session
    }

    /// Returns `true` if the session is both marked Active and has not
    /// passed its expiry time.
    pub fn is_active(&self) -> bool {
        self.status == SessionStatus::Active && Utc::now() < self.expires_at
    }

    /// Returns `true` if the session has passed its expiry time,
    /// regardless of the stored status field.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    /// Returns `true` if the session was explicitly revoked.
    pub fn is_revoked(&self) -> bool {
        self.status == SessionStatus::Revoked
    }

    /// Revoke the session immediately. Once revoked, it cannot be refreshed.
    pub fn revoke(&mut self) {
        self.status = SessionStatus::Revoked;
    }

    /// Mark the session as expired. Typically called by a cleanup process
    /// when `is_expired()` returns true.
    pub fn mark_expired(&mut self) {
        self.status = SessionStatus::Expired;
    }

    /// Refresh the session, extending its expiry by `duration` from now.
    ///
    /// This only works if the session is currently active (not revoked or
    /// expired by status). Returns `true` if the refresh succeeded.
    pub fn refresh(&mut self, duration: Duration) -> bool {
        if self.status != SessionStatus::Active {
            return false;
        }
        let now = Utc::now();
        self.expires_at = now + duration;
        self.last_active_at = now;
        true
    }

    /// Record activity on the session, updating `last_active_at`.
    ///
    /// Also checks if the session has expired by time and updates the
    /// status accordingly. Returns `true` if the session is still active.
    pub fn touch(&mut self) -> bool {
        if self.status == SessionStatus::Revoked {
            return false;
        }
        let now = Utc::now();
        if now >= self.expires_at {
            self.status = SessionStatus::Expired;
            return false;
        }
        self.last_active_at = now;
        true
    }

    /// Returns the remaining duration before the session expires.
    /// Returns zero if already expired.
    pub fn remaining(&self) -> Duration {
        let now = Utc::now();
        if now >= self.expires_at {
            Duration::zero()
        } else {
            self.expires_at - now
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(duration: Duration) -> Session {
        Session::new(
            AccountId::new(),
            TenantId::new(),
            "test-token-abc123".to_string(),
            duration,
        )
    }

    #[test]
    fn test_new_session_is_active() {
        let session = make_session(Duration::hours(1));
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.is_active());
        assert!(!session.is_expired());
        assert!(!session.is_revoked());
    }

    #[test]
    fn test_session_with_metadata() {
        let session = Session::with_metadata(
            AccountId::new(),
            TenantId::new(),
            "tok".to_string(),
            Duration::hours(1),
            Some("192.168.1.1".to_string()),
            Some("Mozilla/5.0".to_string()),
        );
        assert_eq!(session.ip_address.as_deref(), Some("192.168.1.1"));
        assert_eq!(session.user_agent.as_deref(), Some("Mozilla/5.0"));
        assert!(session.is_active());
    }

    #[test]
    fn test_revoke_session() {
        let mut session = make_session(Duration::hours(1));
        session.revoke();
        assert!(session.is_revoked());
        assert!(!session.is_active());
        assert_eq!(session.status, SessionStatus::Revoked);
    }

    #[test]
    fn test_expired_session() {
        // Create a session that already expired (negative duration)
        let mut session = make_session(Duration::hours(1));
        session.expires_at = Utc::now() - Duration::seconds(10);
        assert!(session.is_expired());
        assert!(!session.is_active());
    }

    #[test]
    fn test_mark_expired() {
        let mut session = make_session(Duration::hours(1));
        session.mark_expired();
        assert_eq!(session.status, SessionStatus::Expired);
        assert!(!session.is_active());
    }

    #[test]
    fn test_refresh_active_session() {
        let mut session = make_session(Duration::seconds(10));
        let old_expires = session.expires_at;

        // Wait a tiny bit (or just refresh with a longer duration)
        let result = session.refresh(Duration::hours(2));
        assert!(result);
        assert!(session.expires_at > old_expires);
        assert!(session.is_active());
    }

    #[test]
    fn test_refresh_revoked_session_fails() {
        let mut session = make_session(Duration::hours(1));
        session.revoke();
        let result = session.refresh(Duration::hours(2));
        assert!(!result);
        assert!(session.is_revoked());
    }

    #[test]
    fn test_refresh_expired_status_session_fails() {
        let mut session = make_session(Duration::hours(1));
        session.mark_expired();
        let result = session.refresh(Duration::hours(2));
        assert!(!result);
    }

    #[test]
    fn test_touch_active_session() {
        let mut session = make_session(Duration::hours(1));
        let result = session.touch();
        assert!(result);
    }

    #[test]
    fn test_touch_revoked_session() {
        let mut session = make_session(Duration::hours(1));
        session.revoke();
        let result = session.touch();
        assert!(!result);
    }

    #[test]
    fn test_touch_time_expired_session() {
        let mut session = make_session(Duration::hours(1));
        session.expires_at = Utc::now() - Duration::seconds(1);
        let result = session.touch();
        assert!(!result);
        assert_eq!(session.status, SessionStatus::Expired);
    }

    #[test]
    fn test_remaining_positive() {
        let session = make_session(Duration::hours(1));
        let remaining = session.remaining();
        // Should be close to 1 hour
        assert!(remaining > Duration::minutes(59));
    }

    #[test]
    fn test_remaining_expired() {
        let mut session = make_session(Duration::hours(1));
        session.expires_at = Utc::now() - Duration::seconds(10);
        let remaining = session.remaining();
        assert_eq!(remaining, Duration::zero());
    }

    #[test]
    fn test_session_status_display() {
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Expired.to_string(), "expired");
        assert_eq!(SessionStatus::Revoked.to_string(), "revoked");
    }

    #[test]
    fn test_session_token_preserved() {
        let session = make_session(Duration::hours(1));
        assert_eq!(session.token, "test-token-abc123");
    }

    #[test]
    fn test_session_ids_linked() {
        let account_id = AccountId::new();
        let tenant_id = TenantId::new();
        let session = Session::new(
            account_id,
            tenant_id,
            "tok".to_string(),
            Duration::hours(1),
        );
        assert_eq!(session.account_id, account_id);
        assert_eq!(session.tenant_id, tenant_id);
    }

    #[test]
    fn test_session_serde_roundtrip() {
        let session = make_session(Duration::hours(1));
        let json = serde_json::to_string(&session).expect("serialize");
        let deserialized: Session = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.id, session.id);
        assert_eq!(deserialized.account_id, session.account_id);
        assert_eq!(deserialized.tenant_id, session.tenant_id);
        assert_eq!(deserialized.token, session.token);
        assert_eq!(deserialized.status, session.status);
    }
}
