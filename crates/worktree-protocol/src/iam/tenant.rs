//! # Tenant
//!
//! A Tenant is the top-level organizational unit in the Worktree system.
//! All accounts, teams, trees, and policies belong to a tenant.
//! Tenants provide multi-tenant isolation and resource limit enforcement.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::TenantId;

/// The operational status of a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenantStatus {
    /// Tenant is fully operational.
    Active,
    /// Tenant is suspended — all access is denied until reactivated.
    Suspended,
}

/// The billing/feature plan of a tenant, used for ABAC resource limit evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenantPlan {
    /// Free tier with basic limits.
    Free,
    /// Professional tier with expanded limits.
    Pro,
    /// Enterprise tier with maximum limits and premium features.
    Enterprise,
    /// A custom plan identified by name.
    Custom(String),
}

impl std::fmt::Display for TenantPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TenantPlan::Free => write!(f, "free"),
            TenantPlan::Pro => write!(f, "pro"),
            TenantPlan::Enterprise => write!(f, "enterprise"),
            TenantPlan::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

/// A Tenant is the top-level organizational boundary.
///
/// Every account, team, tree, role, and policy exists within the context of a tenant.
/// Tenants provide isolation: accounts in one tenant cannot access resources in another
/// unless explicitly granted cross-tenant access via GlobalAdmin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// Unique identifier for this tenant.
    pub id: TenantId,
    /// Human-readable name of the tenant (e.g., "Acme Corporation").
    pub name: String,
    /// URL-safe slug identifier (e.g., "acme-corp"). Must be unique across all tenants.
    pub slug: String,
    /// Current operational status.
    pub status: TenantStatus,
    /// The billing/feature plan for this tenant.
    pub plan: TenantPlan,
    /// Maximum number of accounts allowed in this tenant. `None` means unlimited.
    pub max_accounts: Option<u64>,
    /// Maximum number of trees allowed in this tenant. `None` means unlimited.
    pub max_trees: Option<u64>,
    /// When this tenant was created.
    pub created_at: DateTime<Utc>,
    /// Arbitrary key-value attributes for ABAC policy evaluation.
    pub attributes: HashMap<String, String>,
}

impl Tenant {
    /// Create a new tenant with the given name and slug on the Free plan.
    ///
    /// The tenant starts in `Active` status with no resource limits.
    pub fn new(name: impl Into<String>, slug: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: TenantId::new(),
            name: name.into(),
            slug: slug.into(),
            status: TenantStatus::Active,
            plan: TenantPlan::Free,
            max_accounts: None,
            max_trees: None,
            created_at: now,
            attributes: HashMap::new(),
        }
    }

    /// Create a new tenant with a specific plan.
    pub fn with_plan(
        name: impl Into<String>,
        slug: impl Into<String>,
        plan: TenantPlan,
    ) -> Self {
        let mut tenant = Self::new(name, slug);
        tenant.plan = plan;
        tenant
    }

    /// Returns `true` if the tenant is currently active.
    pub fn is_active(&self) -> bool {
        self.status == TenantStatus::Active
    }

    /// Returns `true` if the tenant is currently suspended.
    pub fn is_suspended(&self) -> bool {
        self.status == TenantStatus::Suspended
    }

    /// Suspend this tenant, blocking all access.
    pub fn suspend(&mut self) {
        self.status = TenantStatus::Suspended;
    }

    /// Reactivate a suspended tenant.
    pub fn activate(&mut self) {
        self.status = TenantStatus::Active;
    }

    /// Set the billing plan for this tenant.
    pub fn set_plan(&mut self, plan: TenantPlan) {
        self.plan = plan;
    }

    /// Set the maximum number of accounts allowed.
    pub fn set_max_accounts(&mut self, limit: Option<u64>) {
        self.max_accounts = limit;
    }

    /// Set the maximum number of trees allowed.
    pub fn set_max_trees(&mut self, limit: Option<u64>) {
        self.max_trees = limit;
    }

    /// Check whether adding one more account would exceed the tenant's limit.
    ///
    /// Returns `true` if the current count is within the allowed limit (or unlimited).
    pub fn can_add_account(&self, current_count: u64) -> bool {
        match self.max_accounts {
            Some(max) => current_count < max,
            None => true,
        }
    }

    /// Check whether adding one more tree would exceed the tenant's limit.
    ///
    /// Returns `true` if the current count is within the allowed limit (or unlimited).
    pub fn can_add_tree(&self, current_count: u64) -> bool {
        match self.max_trees {
            Some(max) => current_count < max,
            None => true,
        }
    }

    /// Set an arbitrary attribute on this tenant for ABAC evaluation.
    pub fn set_attribute(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.attributes.insert(key.into(), value.into());
    }

    /// Get an attribute value by key.
    pub fn get_attribute(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(|s| s.as_str())
    }

    /// Remove an attribute by key.
    pub fn remove_attribute(&mut self, key: &str) -> Option<String> {
        self.attributes.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tenant_defaults() {
        let tenant = Tenant::new("Acme Corp", "acme-corp");
        assert_eq!(tenant.name, "Acme Corp");
        assert_eq!(tenant.slug, "acme-corp");
        assert!(tenant.is_active());
        assert!(!tenant.is_suspended());
        assert_eq!(tenant.plan, TenantPlan::Free);
        assert_eq!(tenant.max_accounts, None);
        assert_eq!(tenant.max_trees, None);
        assert!(tenant.attributes.is_empty());
    }

    #[test]
    fn test_tenant_with_plan() {
        let tenant = Tenant::with_plan("Big Co", "big-co", TenantPlan::Enterprise);
        assert_eq!(tenant.plan, TenantPlan::Enterprise);
        assert!(tenant.is_active());
    }

    #[test]
    fn test_tenant_suspend_and_activate() {
        let mut tenant = Tenant::new("Test", "test");
        assert!(tenant.is_active());

        tenant.suspend();
        assert!(tenant.is_suspended());
        assert!(!tenant.is_active());

        tenant.activate();
        assert!(tenant.is_active());
        assert!(!tenant.is_suspended());
    }

    #[test]
    fn test_tenant_set_plan() {
        let mut tenant = Tenant::new("Test", "test");
        assert_eq!(tenant.plan, TenantPlan::Free);

        tenant.set_plan(TenantPlan::Pro);
        assert_eq!(tenant.plan, TenantPlan::Pro);

        tenant.set_plan(TenantPlan::Custom("special".into()));
        assert_eq!(tenant.plan, TenantPlan::Custom("special".into()));
    }

    #[test]
    fn test_tenant_account_limits() {
        let mut tenant = Tenant::new("Test", "test");

        // No limit — can always add
        assert!(tenant.can_add_account(0));
        assert!(tenant.can_add_account(999_999));

        // Set a limit
        tenant.set_max_accounts(Some(5));
        assert!(tenant.can_add_account(0));
        assert!(tenant.can_add_account(4));
        assert!(!tenant.can_add_account(5));
        assert!(!tenant.can_add_account(10));

        // Remove the limit
        tenant.set_max_accounts(None);
        assert!(tenant.can_add_account(999_999));
    }

    #[test]
    fn test_tenant_tree_limits() {
        let mut tenant = Tenant::new("Test", "test");

        assert!(tenant.can_add_tree(100));

        tenant.set_max_trees(Some(10));
        assert!(tenant.can_add_tree(9));
        assert!(!tenant.can_add_tree(10));
        assert!(!tenant.can_add_tree(20));
    }

    #[test]
    fn test_tenant_attributes() {
        let mut tenant = Tenant::new("Test", "test");

        assert_eq!(tenant.get_attribute("region"), None);

        tenant.set_attribute("region", "us-east-1");
        assert_eq!(tenant.get_attribute("region"), Some("us-east-1"));

        tenant.set_attribute("region", "eu-west-1");
        assert_eq!(tenant.get_attribute("region"), Some("eu-west-1"));

        let removed = tenant.remove_attribute("region");
        assert_eq!(removed, Some("eu-west-1".to_string()));
        assert_eq!(tenant.get_attribute("region"), None);
    }

    #[test]
    fn test_tenant_unique_ids() {
        let t1 = Tenant::new("A", "a");
        let t2 = Tenant::new("B", "b");
        assert_ne!(t1.id, t2.id);
    }

    #[test]
    fn test_tenant_plan_display() {
        assert_eq!(TenantPlan::Free.to_string(), "free");
        assert_eq!(TenantPlan::Pro.to_string(), "pro");
        assert_eq!(TenantPlan::Enterprise.to_string(), "enterprise");
        assert_eq!(
            TenantPlan::Custom("startup".into()).to_string(),
            "custom:startup"
        );
    }

    #[test]
    fn test_tenant_serde_roundtrip() {
        let mut tenant = Tenant::with_plan("Serde Co", "serde-co", TenantPlan::Pro);
        tenant.set_max_accounts(Some(50));
        tenant.set_max_trees(Some(100));
        tenant.set_attribute("industry", "tech");

        let json = serde_json::to_string(&tenant).expect("serialize failed");
        let deserialized: Tenant =
            serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(deserialized.id, tenant.id);
        assert_eq!(deserialized.name, "Serde Co");
        assert_eq!(deserialized.slug, "serde-co");
        assert_eq!(deserialized.plan, TenantPlan::Pro);
        assert_eq!(deserialized.max_accounts, Some(50));
        assert_eq!(deserialized.max_trees, Some(100));
        assert_eq!(deserialized.get_attribute("industry"), Some("tech"));
    }
}
