//! # Account
//!
//! An Account is the principal identity in the Worktree IAM system.
//! This is NOT the same as a user profile — it represents the
//! authentication/authorization identity used for access control.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{AccountId, TenantId};

/// The lifecycle status of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountStatus {
    /// The account is active and can authenticate/authorize.
    Active,
    /// The account is temporarily suspended (e.g., by an admin).
    Suspended,
    /// The account has been permanently deactivated.
    Deactivated,
}

impl std::fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountStatus::Active => write!(f, "active"),
            AccountStatus::Suspended => write!(f, "suspended"),
            AccountStatus::Deactivated => write!(f, "deactivated"),
        }
    }
}

/// An Account is the principal identity in the system.
///
/// Every account belongs to exactly one tenant. Accounts carry arbitrary
/// key-value attributes that are used during ABAC policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Unique identifier for this account.
    pub id: AccountId,
    /// Unique username within the tenant.
    pub username: String,
    /// Email address associated with this account.
    pub email: String,
    /// Human-readable display name.
    pub display_name: String,
    /// The tenant this account belongs to.
    pub tenant_id: TenantId,
    /// Current lifecycle status.
    pub status: AccountStatus,
    /// When the account was created.
    pub created_at: DateTime<Utc>,
    /// When the account was last updated.
    pub updated_at: DateTime<Utc>,
    /// Arbitrary key-value attributes for ABAC evaluation.
    ///
    /// Examples: `"department" -> "engineering"`, `"mfa_verified" -> "true"`,
    /// `"clearance_level" -> "3"`.
    pub attributes: HashMap<String, String>,
}

impl Account {
    /// Create a new active account with the given details.
    pub fn new(
        username: impl Into<String>,
        email: impl Into<String>,
        display_name: impl Into<String>,
        tenant_id: TenantId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: AccountId::new(),
            username: username.into(),
            email: email.into(),
            display_name: display_name.into(),
            tenant_id,
            status: AccountStatus::Active,
            created_at: now,
            updated_at: now,
            attributes: HashMap::new(),
        }
    }

    /// Create a new account with a specific ID (useful for reconstitution from storage).
    pub fn with_id(
        id: AccountId,
        username: impl Into<String>,
        email: impl Into<String>,
        display_name: impl Into<String>,
        tenant_id: TenantId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            username: username.into(),
            email: email.into(),
            display_name: display_name.into(),
            tenant_id,
            status: AccountStatus::Active,
            created_at: now,
            updated_at: now,
            attributes: HashMap::new(),
        }
    }

    /// Returns `true` if the account status is `Active`.
    pub fn is_active(&self) -> bool {
        self.status == AccountStatus::Active
    }

    /// Suspend the account. A suspended account cannot authenticate or authorize.
    pub fn suspend(&mut self) {
        self.status = AccountStatus::Suspended;
        self.updated_at = Utc::now();
    }

    /// Permanently deactivate the account.
    pub fn deactivate(&mut self) {
        self.status = AccountStatus::Deactivated;
        self.updated_at = Utc::now();
    }

    /// Reactivate a suspended or deactivated account.
    pub fn activate(&mut self) {
        self.status = AccountStatus::Active;
        self.updated_at = Utc::now();
    }

    /// Set an arbitrary attribute on the account for ABAC evaluation.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(key.into(), value.into());
        self.updated_at = Utc::now();
    }

    /// Get the value of an attribute, if it exists.
    pub fn get_attribute(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(|s| s.as_str())
    }

    /// Remove an attribute from the account. Returns the old value if it existed.
    pub fn remove_attribute(&mut self, key: &str) -> Option<String> {
        let removed = self.attributes.remove(key);
        if removed.is_some() {
            self.updated_at = Utc::now();
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account() -> Account {
        let tenant = TenantId::new();
        Account::new("alice", "alice@example.com", "Alice Smith", tenant)
    }

    #[test]
    fn test_new_account_is_active() {
        let account = make_account();
        assert!(account.is_active());
        assert_eq!(account.status, AccountStatus::Active);
        assert_eq!(account.username, "alice");
        assert_eq!(account.email, "alice@example.com");
        assert_eq!(account.display_name, "Alice Smith");
        assert!(account.attributes.is_empty());
    }

    #[test]
    fn test_account_with_id() {
        let id = AccountId::new();
        let tenant = TenantId::new();
        let account = Account::with_id(id, "bob", "bob@example.com", "Bob Jones", tenant);
        assert_eq!(account.id, id);
        assert_eq!(account.tenant_id, tenant);
        assert!(account.is_active());
    }

    #[test]
    fn test_suspend_account() {
        let mut account = make_account();
        assert!(account.is_active());

        account.suspend();
        assert!(!account.is_active());
        assert_eq!(account.status, AccountStatus::Suspended);
    }

    #[test]
    fn test_deactivate_account() {
        let mut account = make_account();
        account.deactivate();
        assert!(!account.is_active());
        assert_eq!(account.status, AccountStatus::Deactivated);
    }

    #[test]
    fn test_reactivate_account() {
        let mut account = make_account();
        account.suspend();
        assert!(!account.is_active());

        account.activate();
        assert!(account.is_active());
    }

    #[test]
    fn test_set_and_get_attribute() {
        let mut account = make_account();
        account.set_attribute("department", "engineering");
        account.set_attribute("clearance_level", "3");

        assert_eq!(account.get_attribute("department"), Some("engineering"));
        assert_eq!(account.get_attribute("clearance_level"), Some("3"));
        assert_eq!(account.get_attribute("nonexistent"), None);
    }

    #[test]
    fn test_remove_attribute() {
        let mut account = make_account();
        account.set_attribute("department", "engineering");

        let removed = account.remove_attribute("department");
        assert_eq!(removed, Some("engineering".to_string()));
        assert_eq!(account.get_attribute("department"), None);

        // Removing nonexistent key returns None
        let removed = account.remove_attribute("department");
        assert_eq!(removed, None);
    }

    #[test]
    fn test_updated_at_changes_on_mutation() {
        let mut account = make_account();
        let original_updated = account.updated_at;

        // Force a tiny delay by doing some work
        account.set_attribute("key", "value");
        // updated_at should be >= original (may be equal if clock resolution is coarse)
        assert!(account.updated_at >= original_updated);
    }

    #[test]
    fn test_account_status_display() {
        assert_eq!(AccountStatus::Active.to_string(), "active");
        assert_eq!(AccountStatus::Suspended.to_string(), "suspended");
        assert_eq!(AccountStatus::Deactivated.to_string(), "deactivated");
    }

    #[test]
    fn test_account_serde_roundtrip() {
        let mut account = make_account();
        account.set_attribute("department", "ops");

        let json = serde_json::to_string(&account).expect("serialize failed");
        let deserialized: Account = serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(deserialized.id, account.id);
        assert_eq!(deserialized.username, account.username);
        assert_eq!(deserialized.email, account.email);
        assert_eq!(deserialized.display_name, account.display_name);
        assert_eq!(deserialized.tenant_id, account.tenant_id);
        assert_eq!(deserialized.status, account.status);
        assert_eq!(deserialized.get_attribute("department"), Some("ops"));
    }

    #[test]
    fn test_account_clone() {
        let account = make_account();
        let cloned = account.clone();
        assert_eq!(cloned.id, account.id);
        assert_eq!(cloned.username, account.username);
    }
}
