//! # ABAC Policy Definitions
//!
//! Attribute-Based Access Control policies combine subjects, scopes, permissions,
//! and attribute conditions to make fine-grained access decisions.

use std::collections::HashMap;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{AccountId, PolicyId, RoleId, TeamId, TenantId};
use crate::iam::permission::Permission;
use crate::iam::scope::Scope;

// ---------------------------------------------------------------------------
// PolicyEffect
// ---------------------------------------------------------------------------

/// Whether a policy grants or denies the matched permissions.
/// Deny always takes precedence over Allow at the same priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyEffect {
    /// The policy grants the specified permissions.
    Allow,
    /// The policy explicitly denies the specified permissions.
    Deny,
}

impl std::fmt::Display for PolicyEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyEffect::Allow => write!(f, "allow"),
            PolicyEffect::Deny => write!(f, "deny"),
        }
    }
}

// ---------------------------------------------------------------------------
// PolicySubject
// ---------------------------------------------------------------------------

/// Identifies *who* a policy applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicySubject {
    /// A specific account.
    Account(AccountId),
    /// All members of a specific team.
    Team(TeamId),
    /// All accounts that hold a specific role.
    Role(RoleId),
    /// Any authenticated account within the tenant.
    AllAuthenticated,
    /// Everyone, including unauthenticated principals (public access).
    Everyone,
}

impl std::fmt::Display for PolicySubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicySubject::Account(id) => write!(f, "account:{}", id),
            PolicySubject::Team(id) => write!(f, "team:{}", id),
            PolicySubject::Role(id) => write!(f, "role:{}", id),
            PolicySubject::AllAuthenticated => write!(f, "all_authenticated"),
            PolicySubject::Everyone => write!(f, "everyone"),
        }
    }
}

// ---------------------------------------------------------------------------
// ConditionOperator
// ---------------------------------------------------------------------------

/// Comparison operator used in attribute conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConditionOperator {
    /// Exact string equality.
    Equals,
    /// String inequality.
    NotEquals,
    /// The attribute value contains the condition value as a substring.
    Contains,
    /// The attribute value starts with the condition value.
    StartsWith,
    /// The attribute value ends with the condition value.
    EndsWith,
    /// Numeric greater-than (values are parsed as f64).
    GreaterThan,
    /// Numeric less-than (values are parsed as f64).
    LessThan,
    /// The attribute value is one of a comma-separated list of allowed values.
    In,
}

impl std::fmt::Display for ConditionOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            ConditionOperator::Equals => "eq",
            ConditionOperator::NotEquals => "neq",
            ConditionOperator::Contains => "contains",
            ConditionOperator::StartsWith => "starts_with",
            ConditionOperator::EndsWith => "ends_with",
            ConditionOperator::GreaterThan => "gt",
            ConditionOperator::LessThan => "lt",
            ConditionOperator::In => "in",
        };
        write!(f, "{}", label)
    }
}

// ---------------------------------------------------------------------------
// AttributeCondition
// ---------------------------------------------------------------------------

/// A single ABAC condition that compares an attribute against an expected value.
///
/// All conditions on a policy must evaluate to `true` for the policy to match
/// (logical AND).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeCondition {
    /// The attribute key to look up (e.g. `"department"`, `"ip_range"`,
    /// `"time_of_day"`, `"mfa_verified"`).
    pub key: String,
    /// The comparison operator.
    pub operator: ConditionOperator,
    /// The expected value to compare against.
    pub value: String,
}

impl AttributeCondition {
    /// Create a new condition.
    pub fn new(key: impl Into<String>, operator: ConditionOperator, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            operator,
            value: value.into(),
        }
    }

    /// Evaluate this condition against the provided attribute map.
    ///
    /// Returns `true` if the condition is satisfied, `false` otherwise.
    /// If the attribute key is missing from the map the condition fails.
    pub fn evaluate(&self, attributes: &HashMap<String, String>) -> bool {
        let Some(attr_value) = attributes.get(&self.key) else {
            return false;
        };

        match self.operator {
            ConditionOperator::Equals => attr_value == &self.value,
            ConditionOperator::NotEquals => attr_value != &self.value,
            ConditionOperator::Contains => attr_value.contains(&self.value),
            ConditionOperator::StartsWith => attr_value.starts_with(&self.value),
            ConditionOperator::EndsWith => attr_value.ends_with(&self.value),
            ConditionOperator::GreaterThan => {
                match (attr_value.parse::<f64>(), self.value.parse::<f64>()) {
                    (Ok(a), Ok(b)) => a > b,
                    _ => false,
                }
            }
            ConditionOperator::LessThan => {
                match (attr_value.parse::<f64>(), self.value.parse::<f64>()) {
                    (Ok(a), Ok(b)) => a < b,
                    _ => false,
                }
            }
            ConditionOperator::In => {
                let allowed: Vec<&str> = self.value.split(',').map(|s| s.trim()).collect();
                allowed.contains(&attr_value.as_str())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// An ABAC policy that combines subject, scope, permissions, and attribute
/// conditions into a single access rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Unique policy identifier.
    pub id: PolicyId,
    /// The tenant this policy belongs to.
    pub tenant_id: TenantId,
    /// Human-readable name.
    pub name: String,
    /// Longer description of the policy's purpose.
    pub description: String,
    /// Whether this policy allows or denies.
    pub effect: PolicyEffect,
    /// Who this policy applies to.
    pub subjects: Vec<PolicySubject>,
    /// Where this policy applies (hierarchical scope).
    pub scope: Scope,
    /// What permissions this policy grants or denies.
    pub permissions: HashSet<Permission>,
    /// ABAC conditions — ALL must evaluate to `true` for the policy to match.
    pub conditions: Vec<AttributeCondition>,
    /// Higher-priority policies are evaluated first. At equal priority, deny
    /// wins over allow.
    pub priority: i32,
    /// Whether this policy is currently active.
    pub enabled: bool,
    /// When the policy was created.
    pub created_at: DateTime<Utc>,
}

impl Policy {
    /// Create a new policy with the given effect, name, description, and scope.
    ///
    /// Starts with no subjects, no permissions, no conditions, priority 0, and enabled.
    pub fn new(
        tenant_id: TenantId,
        name: impl Into<String>,
        description: impl Into<String>,
        effect: PolicyEffect,
        scope: Scope,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: PolicyId::new(),
            tenant_id,
            name: name.into(),
            description: description.into(),
            effect,
            subjects: Vec::new(),
            scope,
            permissions: HashSet::new(),
            conditions: Vec::new(),
            priority: 0,
            enabled: true,
            created_at: now,
        }
    }

    /// Create a new allow policy with sensible defaults.
    pub fn new_allow(
        tenant_id: TenantId,
        name: impl Into<String>,
        scope: Scope,
        permissions: HashSet<Permission>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: PolicyId::new(),
            tenant_id,
            name: name.into(),
            description: String::new(),
            effect: PolicyEffect::Allow,
            subjects: Vec::new(),
            scope,
            permissions,
            conditions: Vec::new(),
            priority: 0,
            enabled: true,
            created_at: now,
        }
    }

    /// Create a new deny policy with sensible defaults.
    pub fn new_deny(
        tenant_id: TenantId,
        name: impl Into<String>,
        scope: Scope,
        permissions: HashSet<Permission>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: PolicyId::new(),
            tenant_id,
            name: name.into(),
            description: String::new(),
            effect: PolicyEffect::Deny,
            subjects: Vec::new(),
            scope,
            permissions,
            conditions: Vec::new(),
            priority: 0,
            enabled: true,
            created_at: now,
        }
    }

    // ----- subject helpers ------------------------------------------------

    /// Add a subject to this policy.
    pub fn add_subject(&mut self, subject: PolicySubject) {
        if !self.subjects.contains(&subject) {
            self.subjects.push(subject);
        }
    }

    /// Check whether this policy applies to a specific account (directly).
    pub fn applies_to_account(&self, account_id: &AccountId) -> bool {
        self.subjects.iter().any(|s| match s {
            PolicySubject::Account(id) => id == account_id,
            PolicySubject::AllAuthenticated => true,
            PolicySubject::Everyone => true,
            _ => false,
        })
    }

    /// Check whether this policy applies to a specific team.
    pub fn applies_to_team(&self, team_id: &TeamId) -> bool {
        self.subjects.iter().any(|s| match s {
            PolicySubject::Team(id) => id == team_id,
            PolicySubject::AllAuthenticated => true,
            PolicySubject::Everyone => true,
            _ => false,
        })
    }

    /// Check whether this policy applies to a specific role.
    pub fn applies_to_role(&self, role_id: &RoleId) -> bool {
        self.subjects.iter().any(|s| match s {
            PolicySubject::Role(id) => id == role_id,
            PolicySubject::AllAuthenticated => true,
            PolicySubject::Everyone => true,
            _ => false,
        })
    }

    // ----- condition helpers ----------------------------------------------

    /// Add an attribute condition.
    pub fn add_condition(&mut self, condition: AttributeCondition) {
        self.conditions.push(condition);
    }

    /// Evaluate ALL conditions against the provided attributes.
    ///
    /// Returns `true` if every condition is satisfied (logical AND).
    /// A policy with no conditions always matches.
    pub fn evaluate_conditions(&self, attributes: &HashMap<String, String>) -> bool {
        self.conditions.iter().all(|c| c.evaluate(attributes))
    }

    // ----- effect helpers -------------------------------------------------

    /// Returns `true` if this is an Allow policy.
    pub fn is_allow(&self) -> bool {
        self.effect == PolicyEffect::Allow
    }

    /// Returns `true` if this is a Deny policy.
    pub fn is_deny(&self) -> bool {
        self.effect == PolicyEffect::Deny
    }

    /// Returns `true` if this policy is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable this policy.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable this policy.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Set the priority of this policy.
    pub fn set_priority(&mut self, priority: i32) {
        self.priority = priority;
    }

    /// Set the description.
    pub fn set_description(&mut self, description: impl Into<String>) {
        self.description = description.into();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::id::{AccountId, TeamId, TenantId, TreeId};

    // -- AttributeCondition ------------------------------------------------

    #[test]
    fn condition_equals() {
        let cond = AttributeCondition::new("department", ConditionOperator::Equals, "engineering");
        let mut attrs = HashMap::new();
        attrs.insert("department".to_string(), "engineering".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("department".to_string(), "marketing".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_not_equals() {
        let cond = AttributeCondition::new("env", ConditionOperator::NotEquals, "production");
        let mut attrs = HashMap::new();
        attrs.insert("env".to_string(), "staging".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("env".to_string(), "production".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_contains() {
        let cond = AttributeCondition::new("email", ConditionOperator::Contains, "@acme.com");
        let mut attrs = HashMap::new();
        attrs.insert("email".to_string(), "alice@acme.com".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("email".to_string(), "bob@other.org".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_starts_with() {
        let cond = AttributeCondition::new("ip", ConditionOperator::StartsWith, "192.168.");
        let mut attrs = HashMap::new();
        attrs.insert("ip".to_string(), "192.168.1.100".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("ip".to_string(), "10.0.0.1".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_ends_with() {
        let cond = AttributeCondition::new("file", ConditionOperator::EndsWith, ".rs");
        let mut attrs = HashMap::new();
        attrs.insert("file".to_string(), "main.rs".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("file".to_string(), "main.py".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_greater_than() {
        let cond = AttributeCondition::new("risk_score", ConditionOperator::GreaterThan, "50");
        let mut attrs = HashMap::new();
        attrs.insert("risk_score".to_string(), "75".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("risk_score".to_string(), "25".to_string());
        assert!(!cond.evaluate(&attrs));

        attrs.insert("risk_score".to_string(), "50".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_less_than() {
        let cond = AttributeCondition::new("hour", ConditionOperator::LessThan, "18");
        let mut attrs = HashMap::new();
        attrs.insert("hour".to_string(), "9".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("hour".to_string(), "20".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_in_operator() {
        let cond = AttributeCondition::new("region", ConditionOperator::In, "us-east-1, eu-west-1, ap-south-1");
        let mut attrs = HashMap::new();
        attrs.insert("region".to_string(), "eu-west-1".to_string());
        assert!(cond.evaluate(&attrs));

        attrs.insert("region".to_string(), "us-west-2".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_missing_attribute_fails() {
        let cond = AttributeCondition::new("department", ConditionOperator::Equals, "eng");
        let attrs = HashMap::new();
        assert!(!cond.evaluate(&attrs));
    }

    #[test]
    fn condition_non_numeric_greater_than_fails() {
        let cond = AttributeCondition::new("val", ConditionOperator::GreaterThan, "50");
        let mut attrs = HashMap::new();
        attrs.insert("val".to_string(), "not_a_number".to_string());
        assert!(!cond.evaluate(&attrs));
    }

    // -- Policy construction -----------------------------------------------

    #[test]
    fn new_allow_policy() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let policy = Policy::new_allow(tenant_id, "read-trees", Scope::Tenant(tenant_id), perms);

        assert!(policy.is_allow());
        assert!(!policy.is_deny());
        assert!(policy.is_enabled());
        assert_eq!(policy.name, "read-trees");
        assert_eq!(policy.priority, 0);
    }

    #[test]
    fn new_deny_policy() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeDelete].into_iter().collect();
        let policy = Policy::new_deny(tenant_id, "no-delete", Scope::Tenant(tenant_id), perms);

        assert!(policy.is_deny());
        assert!(!policy.is_allow());
    }

    // -- Subject matching --------------------------------------------------

    #[test]
    fn applies_to_account_direct() {
        let tenant_id = TenantId::new();
        let account_id = AccountId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);
        policy.add_subject(PolicySubject::Account(account_id));

        assert!(policy.applies_to_account(&account_id));
        assert!(!policy.applies_to_account(&AccountId::new()));
    }

    #[test]
    fn applies_to_account_all_authenticated() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);
        policy.add_subject(PolicySubject::AllAuthenticated);

        assert!(policy.applies_to_account(&AccountId::new()));
    }

    #[test]
    fn applies_to_account_everyone() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);
        policy.add_subject(PolicySubject::Everyone);

        assert!(policy.applies_to_account(&AccountId::new()));
    }

    #[test]
    fn applies_to_team_direct() {
        let tenant_id = TenantId::new();
        let team_id = TeamId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);
        policy.add_subject(PolicySubject::Team(team_id));

        assert!(policy.applies_to_team(&team_id));
        assert!(!policy.applies_to_team(&TeamId::new()));
    }

    // -- Condition evaluation on policy ------------------------------------

    #[test]
    fn evaluate_conditions_all_must_pass() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeWrite].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);
        policy.add_condition(AttributeCondition::new(
            "department",
            ConditionOperator::Equals,
            "engineering",
        ));
        policy.add_condition(AttributeCondition::new(
            "mfa_verified",
            ConditionOperator::Equals,
            "true",
        ));

        let mut attrs = HashMap::new();
        attrs.insert("department".to_string(), "engineering".to_string());
        attrs.insert("mfa_verified".to_string(), "true".to_string());
        assert!(policy.evaluate_conditions(&attrs));

        // Missing one attribute → fails
        let mut partial = HashMap::new();
        partial.insert("department".to_string(), "engineering".to_string());
        assert!(!policy.evaluate_conditions(&partial));

        // Wrong value → fails
        let mut wrong = HashMap::new();
        wrong.insert("department".to_string(), "marketing".to_string());
        wrong.insert("mfa_verified".to_string(), "true".to_string());
        assert!(!policy.evaluate_conditions(&wrong));
    }

    #[test]
    fn evaluate_conditions_empty_always_true() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);

        assert!(policy.evaluate_conditions(&HashMap::new()));
    }

    // -- Misc helpers ------------------------------------------------------

    #[test]
    fn enable_disable_policy() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);

        assert!(policy.is_enabled());
        policy.disable();
        assert!(!policy.is_enabled());
        policy.enable();
        assert!(policy.is_enabled());
    }

    #[test]
    fn set_priority() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);

        policy.set_priority(100);
        assert_eq!(policy.priority, 100);
    }

    #[test]
    fn duplicate_subject_not_added() {
        let tenant_id = TenantId::new();
        let account_id = AccountId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead].into_iter().collect();
        let mut policy = Policy::new_allow(tenant_id, "p", Scope::Tenant(tenant_id), perms);

        policy.add_subject(PolicySubject::Account(account_id));
        policy.add_subject(PolicySubject::Account(account_id));
        assert_eq!(policy.subjects.len(), 1);
    }

    #[test]
    fn policy_scope_tree_level() {
        let tenant_id = TenantId::new();
        let tree_id = TreeId::new();
        let perms: HashSet<Permission> = [Permission::TreeWrite, Permission::BranchCreate]
            .into_iter()
            .collect();
        let policy = Policy::new_deny(
            tenant_id,
            "deny-write-to-tree",
            Scope::Tree(tenant_id, tree_id),
            perms,
        );

        assert!(policy.is_deny());
        assert!(policy.permissions.contains(&Permission::TreeWrite));
        assert!(policy.permissions.contains(&Permission::BranchCreate));
    }

    // -- Display impls -----------------------------------------------------

    #[test]
    fn display_policy_effect() {
        assert_eq!(PolicyEffect::Allow.to_string(), "allow");
        assert_eq!(PolicyEffect::Deny.to_string(), "deny");
    }

    #[test]
    fn display_policy_subject() {
        let s = PolicySubject::AllAuthenticated;
        assert_eq!(s.to_string(), "all_authenticated");

        let s = PolicySubject::Everyone;
        assert_eq!(s.to_string(), "everyone");
    }

    #[test]
    fn display_condition_operator() {
        assert_eq!(ConditionOperator::Equals.to_string(), "eq");
        assert_eq!(ConditionOperator::In.to_string(), "in");
        assert_eq!(ConditionOperator::GreaterThan.to_string(), "gt");
    }

    // -- Serde roundtrip ---------------------------------------------------

    #[test]
    fn serde_roundtrip_policy() {
        let tenant_id = TenantId::new();
        let perms: HashSet<Permission> = [Permission::TreeRead, Permission::BranchRead]
            .into_iter()
            .collect();
        let mut policy = Policy::new_allow(tenant_id, "test-policy", Scope::Tenant(tenant_id), perms);
        policy.add_subject(PolicySubject::AllAuthenticated);
        policy.add_condition(AttributeCondition::new(
            "env",
            ConditionOperator::Equals,
            "prod",
        ));

        let json = serde_json::to_string(&policy).expect("serialize");
        let restored: Policy = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.id, policy.id);
        assert_eq!(restored.name, "test-policy");
        assert_eq!(restored.effect, PolicyEffect::Allow);
        assert_eq!(restored.permissions.len(), 2);
        assert_eq!(restored.conditions.len(), 1);
        assert_eq!(restored.subjects.len(), 1);
    }

    #[test]
    fn serde_roundtrip_condition_operator() {
        let ops = vec![
            ConditionOperator::Equals,
            ConditionOperator::NotEquals,
            ConditionOperator::Contains,
            ConditionOperator::StartsWith,
            ConditionOperator::EndsWith,
            ConditionOperator::GreaterThan,
            ConditionOperator::LessThan,
            ConditionOperator::In,
        ];
        for op in ops {
            let json = serde_json::to_string(&op).expect("serialize");
            let restored: ConditionOperator = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(op, restored);
        }
    }

    #[test]
    fn serde_roundtrip_policy_effect() {
        for effect in [PolicyEffect::Allow, PolicyEffect::Deny] {
            let json = serde_json::to_string(&effect).expect("serialize");
            let restored: PolicyEffect = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(effect, restored);
        }
    }
}
