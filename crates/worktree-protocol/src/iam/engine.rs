//! # Access Decision Engine
//!
//! The central evaluator that combines RBAC (role-based) and ABAC (attribute-based)
//! access control to produce allow/deny decisions.
//!
//! ## Evaluation Algorithm
//!
//! 1. Check if the account is active — inactive accounts are always denied
//!    (as is a request whose `account_id` does not match `account`).
//! 2. Collect all applicable roles from the account's teams.
//! 3. Collect all permissions from those roles (RBAC check). A held
//!    permission grants the requested one if it [implies](Permission::implies)
//!    it (`TenantAdmin` → everything in the tenant, `TreeAdmin` → tree ops).
//! 4. If the requested permission is granted AND the role's tenant covers the
//!    scope → tentatively Allow. `GlobalAdmin` allows everything everywhere.
//! 5. Evaluate the ABAC policies of the request's tenant that match subject,
//!    scope, permission and conditions. Conditions see the request
//!    attributes overlaid with the account's own attributes (account wins,
//!    since it is server-side data).
//! 6. If ANY matching Deny policy exists → Deny (deny always wins).
//! 7. If RBAC allowed OR any matching Allow policy exists → Allow.
//! 8. Otherwise → Deny("no matching policy").

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::core::id::AccountId;
use crate::iam::account::Account;
use crate::iam::permission::Permission;
use crate::iam::policy::{Policy, PolicySubject};
use crate::iam::role::Role;
use crate::iam::scope::Scope;
use crate::iam::team::Team;

/// The result of an access control evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessDecision {
    /// Access is granted.
    Allow,
    /// Access is denied, with a human-readable reason.
    Deny {
        /// Why access was denied.
        reason: String,
    },
}

impl AccessDecision {
    /// Returns `true` if the decision is `Allow`.
    pub fn is_allow(&self) -> bool {
        matches!(self, AccessDecision::Allow)
    }

    /// Returns `true` if the decision is `Deny`.
    pub fn is_deny(&self) -> bool {
        matches!(self, AccessDecision::Deny { .. })
    }

    /// Convenience constructor for a deny with a reason.
    pub fn deny(reason: impl Into<String>) -> Self {
        AccessDecision::Deny {
            reason: reason.into(),
        }
    }
}

/// A request to check whether an account has a specific permission in a scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// The account requesting access.
    pub account_id: AccountId,
    /// The permission being requested.
    pub permission: Permission,
    /// The scope in which the permission is being requested.
    pub scope: Scope,
    /// Contextual attributes for ABAC evaluation (e.g. "ip_address", "time_of_day", "mfa_verified").
    pub attributes: HashMap<String, String>,
}

impl AccessRequest {
    /// Create a new access request.
    pub fn new(account_id: AccountId, permission: Permission, scope: Scope) -> Self {
        Self {
            account_id,
            permission,
            scope,
            attributes: HashMap::new(),
        }
    }

    /// Create a new access request with attributes.
    pub fn with_attributes(
        account_id: AccountId,
        permission: Permission,
        scope: Scope,
        attributes: HashMap<String, String>,
    ) -> Self {
        Self {
            account_id,
            permission,
            scope,
            attributes,
        }
    }

    /// Add an attribute to the request.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(key.into(), value.into());
    }
}

/// The central access decision engine.
///
/// This is a stateless, pure evaluator. It takes all the information it needs
/// as parameters and returns a decision. This makes it easy to test and reason about.
#[derive(Debug, Clone, Default)]
pub struct AccessEngine;

impl AccessEngine {
    /// Create a new access engine.
    pub fn new() -> Self {
        Self
    }

    /// Evaluate an access request against the full RBAC + ABAC context.
    ///
    /// # Arguments
    ///
    /// * `request` — The access request to evaluate.
    /// * `account` — The account making the request.
    /// * `teams` — All teams the account may belong to (the engine filters to relevant ones).
    /// * `roles` — All roles that may be relevant (from the account's teams).
    /// * `policies` — All ABAC policies that may be relevant.
    ///
    /// # Returns
    ///
    /// An `AccessDecision` — either `Allow` or `Deny { reason }`.
    pub fn evaluate(
        &self,
        request: &AccessRequest,
        account: &Account,
        teams: &[Team],
        roles: &[Role],
        policies: &[Policy],
    ) -> AccessDecision {
        // Step 1: The request must be for this account, and it must be active.
        if request.account_id != account.id {
            return AccessDecision::deny("request account does not match the evaluated account");
        }
        if !account.is_active() {
            return AccessDecision::deny(format!(
                "account '{}' is not active (status: {:?})",
                account.username, account.status
            ));
        }

        // Step 2: Collect all applicable roles.
        // Find all teams this account is a member of.
        let account_teams: Vec<&Team> = teams
            .iter()
            .filter(|t| t.has_member(&request.account_id))
            .collect();

        // Collect all role IDs from those teams.
        let mut role_ids: HashSet<_> = HashSet::new();
        for team in &account_teams {
            for role_id in &team.roles {
                role_ids.insert(*role_id);
            }
        }

        // Step 3: Collect all permissions from those roles, considering scope.
        let mut rbac_allowed = false;
        let applicable_roles: Vec<&Role> =
            roles.iter().filter(|r| role_ids.contains(&r.id)).collect();

        // Check if any role grants the requested permission.
        // RBAC roles are scoped to the tenant level — if the role belongs to the same
        // tenant as the request scope, it applies.
        for role in &applicable_roles {
            if role
                .permissions
                .iter()
                .any(|held| held.implies(&request.permission))
            {
                // Check that the role's tenant scope covers the request scope.
                let role_scope = Scope::Tenant(role.tenant_id);
                if role_scope.covers(&request.scope) {
                    rbac_allowed = true;
                    break;
                }
            }
        }

        // Step 4: If GlobalAdmin is in the RBAC set, allow everything.
        for role in &applicable_roles {
            if role.has_permission(&Permission::GlobalAdmin) {
                rbac_allowed = true;
                break;
            }
        }

        // Step 5: Evaluate ABAC policies.
        // Collect all team IDs the account belongs to.
        let account_team_ids: HashSet<_> = account_teams.iter().map(|t| t.id).collect();

        // Account attributes are authoritative server-side data, so they
        // override any same-named attribute supplied with the request.
        let mut attributes = request.attributes.clone();
        attributes.extend(
            account
                .attributes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );

        // Filter and sort policies by priority (higher priority first).
        // A policy only ever applies inside its own tenant: otherwise a
        // tenant admin could write a `Scope::Global` policy granting (or
        // denying) access in other tenants.
        let request_tenant = request.scope.tenant_id();
        let mut matching_policies: Vec<&Policy> = policies
            .iter()
            .filter(|p| p.enabled)
            .filter(|p| request_tenant == Some(&p.tenant_id))
            .filter(|p| self.policy_scope_matches(p, &request.scope))
            .filter(|p| {
                self.policy_subject_matches(p, &request.account_id, &account_team_ids, &role_ids)
            })
            .filter(|p| p.permissions.contains(&request.permission))
            .filter(|p| p.evaluate_conditions(&attributes))
            .collect();

        // Sort by priority descending (higher priority evaluated first).
        matching_policies.sort_by_key(|p| std::cmp::Reverse(p.priority));

        // Step 6: If ANY matching Deny policy exists → Deny (deny always wins).
        let mut has_abac_allow = false;
        for policy in &matching_policies {
            if policy.is_deny() {
                return AccessDecision::deny(format!(
                    "denied by policy '{}' (priority {})",
                    policy.name, policy.priority
                ));
            }
            if policy.is_allow() {
                has_abac_allow = true;
            }
        }

        // Step 7: If RBAC allowed OR any matching Allow policy exists → Allow.
        if rbac_allowed || has_abac_allow {
            return AccessDecision::Allow;
        }

        // Step 8: Otherwise → Deny.
        AccessDecision::deny("no matching role or policy grants the requested permission")
    }

    /// Check if a policy's scope covers the request scope.
    fn policy_scope_matches(&self, policy: &Policy, request_scope: &Scope) -> bool {
        policy.scope.covers(request_scope)
    }

    /// Check if a policy's subjects match the requesting account.
    fn policy_subject_matches(
        &self,
        policy: &Policy,
        account_id: &AccountId,
        account_team_ids: &HashSet<crate::core::id::TeamId>,
        account_role_ids: &HashSet<crate::core::id::RoleId>,
    ) -> bool {
        // If no subjects are specified, the policy applies to nobody.
        if policy.subjects.is_empty() {
            return false;
        }

        for subject in &policy.subjects {
            match subject {
                PolicySubject::Account(id) => {
                    if id == account_id {
                        return true;
                    }
                }
                PolicySubject::Team(id) => {
                    if account_team_ids.contains(id) {
                        return true;
                    }
                }
                PolicySubject::Role(id) => {
                    if account_role_ids.contains(id) {
                        return true;
                    }
                }
                PolicySubject::AllAuthenticated => {
                    // Any authenticated account matches.
                    return true;
                }
                PolicySubject::Everyone => {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::id::*;
    use crate::iam::account::Account;
    use crate::iam::policy::{
        AttributeCondition, ConditionOperator, Policy, PolicyEffect, PolicySubject,
    };
    use crate::iam::role::Role;
    use crate::iam::scope::Scope;
    use crate::iam::team::Team;

    fn make_tenant() -> TenantId {
        TenantId::new()
    }

    fn make_active_account(tenant_id: TenantId) -> Account {
        Account::new(
            "testuser".to_string(),
            "test@example.com".to_string(),
            "Test User".to_string(),
            tenant_id,
        )
    }

    fn make_suspended_account(tenant_id: TenantId) -> Account {
        let mut account = make_active_account(tenant_id);
        account.suspend();
        account
    }

    fn make_team(tenant_id: TenantId, member: AccountId, role: RoleId) -> Team {
        let mut team = Team::new(
            tenant_id,
            "Test Team".to_string(),
            "A test team".to_string(),
        );
        team.add_member(member);
        team.add_role(role);
        team
    }

    #[test]
    fn test_inactive_account_denied() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_suspended_account(tenant_id);
        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[], &[], &[]);
        assert!(decision.is_deny());
        if let AccessDecision::Deny { reason } = &decision {
            assert!(reason.contains("not active"));
        }
    }

    #[test]
    fn test_rbac_allows_with_correct_role() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);
        let viewer_role = Role::viewer(tenant_id);

        let team = make_team(tenant_id, account.id, viewer_role.id);

        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[team], &[viewer_role], &[]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_rbac_denies_without_permission() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);
        let viewer_role = Role::viewer(tenant_id);

        let team = make_team(tenant_id, account.id, viewer_role.id);

        // Viewer role does not have TreeWrite
        let request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[team], &[viewer_role], &[]);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_abac_deny_overrides_rbac_allow() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);
        let dev_role = Role::developer(tenant_id);
        let team = make_team(tenant_id, account.id, dev_role.id);

        // Developer role allows TreeWrite via RBAC
        // But we create an ABAC deny policy
        let mut deny_policy = Policy::new(
            tenant_id,
            "Deny writes outside office hours".to_string(),
            "Denies tree writes when not in office hours".to_string(),
            PolicyEffect::Deny,
            Scope::Tenant(tenant_id),
        );
        deny_policy.subjects.push(PolicySubject::AllAuthenticated);
        deny_policy.permissions.insert(Permission::TreeWrite);
        deny_policy.conditions.push(AttributeCondition {
            key: "office_hours".to_string(),
            operator: ConditionOperator::Equals,
            value: "false".to_string(),
        });
        deny_policy.priority = 100;
        deny_policy.enabled = true;

        let mut request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));
        request.set_attribute("office_hours", "false");

        let decision = engine.evaluate(&request, &account, &[team], &[dev_role], &[deny_policy]);
        assert!(decision.is_deny());
        if let AccessDecision::Deny { reason } = &decision {
            assert!(reason.contains("denied by policy"));
        }
    }

    #[test]
    fn test_abac_allow_without_rbac() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);

        // No RBAC roles assigned, but an ABAC policy grants access.
        let mut allow_policy = Policy::new(
            tenant_id,
            "Allow all reads".to_string(),
            "Allow tree reads for all authenticated users".to_string(),
            PolicyEffect::Allow,
            Scope::Tenant(tenant_id),
        );
        allow_policy.subjects.push(PolicySubject::AllAuthenticated);
        allow_policy.permissions.insert(Permission::TreeRead);
        allow_policy.enabled = true;

        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[], &[], &[allow_policy]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_no_matching_policy_denies() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);

        let request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[], &[], &[]);
        assert!(decision.is_deny());
        if let AccessDecision::Deny { reason } = &decision {
            assert!(reason.contains("no matching"));
        }
    }

    #[test]
    fn test_condition_based_policy_not_met() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);

        // ABAC allow policy with a condition that won't be met.
        let mut policy = Policy::new(
            tenant_id,
            "Allow with MFA".to_string(),
            "Only allow if MFA is verified".to_string(),
            PolicyEffect::Allow,
            Scope::Tenant(tenant_id),
        );
        policy.subjects.push(PolicySubject::AllAuthenticated);
        policy.permissions.insert(Permission::TreeWrite);
        policy.conditions.push(AttributeCondition {
            key: "mfa_verified".to_string(),
            operator: ConditionOperator::Equals,
            value: "true".to_string(),
        });
        policy.enabled = true;

        // Request without mfa_verified attribute
        let request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[], &[], &[policy]);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_scope_tree_level_rbac() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let tree_id = TreeId::new();
        let account = make_active_account(tenant_id);
        let dev_role = Role::developer(tenant_id);
        let team = make_team(tenant_id, account.id, dev_role.id);

        // Developer role is tenant-scoped, so it should cover tree-level requests.
        let request = AccessRequest::new(
            account.id,
            Permission::TreeRead,
            Scope::Tree(tenant_id, tree_id),
        );

        let decision = engine.evaluate(&request, &account, &[team], &[dev_role], &[]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_scope_branch_level_rbac() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let tree_id = TreeId::new();
        let branch_id = BranchId::new();
        let account = make_active_account(tenant_id);
        let dev_role = Role::developer(tenant_id);
        let team = make_team(tenant_id, account.id, dev_role.id);

        let request = AccessRequest::new(
            account.id,
            Permission::BranchCreate,
            Scope::Branch(tenant_id, tree_id, branch_id),
        );

        let decision = engine.evaluate(&request, &account, &[team], &[dev_role], &[]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_cross_tenant_rbac_denied() {
        let engine = AccessEngine::new();
        let tenant_a = make_tenant();
        let tenant_b = make_tenant();
        let account = make_active_account(tenant_a);
        let dev_role = Role::developer(tenant_a);
        let team = make_team(tenant_a, account.id, dev_role.id);

        // Request is for tenant_b, but the role belongs to tenant_a.
        let request = AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_b));

        let decision = engine.evaluate(&request, &account, &[team], &[dev_role], &[]);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_global_admin_allows_everything() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let other_tenant = make_tenant();
        let account = make_active_account(tenant_id);
        let owner_role = Role::owner(tenant_id);

        // Create a custom role with GlobalAdmin
        let mut global_role = Role::new(
            tenant_id,
            "Global Admin".to_string(),
            "Superadmin role".to_string(),
        );
        global_role.grant(Permission::GlobalAdmin);

        let team = make_team(tenant_id, account.id, global_role.id);

        // Request for a different tenant should work with GlobalAdmin.
        let request = AccessRequest::new(
            account.id,
            Permission::TreeRead,
            Scope::Tenant(other_tenant),
        );

        let decision =
            engine.evaluate(&request, &account, &[team], &[global_role, owner_role], &[]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_policy_specific_account_subject() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);
        let other_account = make_active_account(tenant_id);

        let mut policy = Policy::new(
            tenant_id,
            "Allow specific account".to_string(),
            "Grants tree read to a specific account".to_string(),
            PolicyEffect::Allow,
            Scope::Tenant(tenant_id),
        );
        policy.subjects.push(PolicySubject::Account(account.id));
        policy.permissions.insert(Permission::TreeRead);
        policy.enabled = true;

        // The targeted account should be allowed.
        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));
        let decision = engine.evaluate(&request, &account, &[], &[], &[policy.clone()]);
        assert!(decision.is_allow());

        // A different account should be denied.
        let request2 = AccessRequest::new(
            other_account.id,
            Permission::TreeRead,
            Scope::Tenant(tenant_id),
        );
        let decision2 = engine.evaluate(&request2, &other_account, &[], &[], &[policy]);
        assert!(decision2.is_deny());
    }

    #[test]
    fn test_policy_team_subject() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);
        let team = Team::new(
            tenant_id,
            "Backend Team".to_string(),
            "Backend engineers".to_string(),
        );
        let mut team_with_member = team.clone();
        team_with_member.add_member(account.id);

        let mut policy = Policy::new(
            tenant_id,
            "Allow team writes".to_string(),
            "Grants tree write to the backend team".to_string(),
            PolicyEffect::Allow,
            Scope::Tenant(tenant_id),
        );
        policy
            .subjects
            .push(PolicySubject::Team(team_with_member.id));
        policy.permissions.insert(Permission::TreeWrite);
        policy.enabled = true;

        let request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[team_with_member], &[], &[policy]);
        assert!(decision.is_allow());
    }

    #[test]
    fn test_disabled_policy_ignored() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);

        let mut policy = Policy::new(
            tenant_id,
            "Disabled policy".to_string(),
            "This policy is disabled".to_string(),
            PolicyEffect::Allow,
            Scope::Tenant(tenant_id),
        );
        policy.subjects.push(PolicySubject::AllAuthenticated);
        policy.permissions.insert(Permission::TreeRead);
        policy.enabled = false; // disabled!

        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[], &[], &[policy]);
        assert!(decision.is_deny());
    }

    #[test]
    fn test_multiple_teams_aggregate_roles() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let account = make_active_account(tenant_id);

        // Team A has viewer role (TreeRead, BranchRead, SnapshotRead)
        let viewer_role = Role::viewer(tenant_id);
        let mut team_a = Team::new(tenant_id, "Team A".to_string(), "Viewers".to_string());
        team_a.add_member(account.id);
        team_a.add_role(viewer_role.id);

        // Team B has developer role (TreeRead, TreeWrite, etc.)
        let dev_role = Role::developer(tenant_id);
        let mut team_b = Team::new(tenant_id, "Team B".to_string(), "Developers".to_string());
        team_b.add_member(account.id);
        team_b.add_role(dev_role.id);

        // TreeWrite should be available via Team B's developer role.
        let request =
            AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(
            &request,
            &account,
            &[team_a, team_b],
            &[viewer_role, dev_role],
            &[],
        );
        assert!(decision.is_allow());
    }

    #[test]
    fn test_per_tree_abac_policy() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let tree_id = TreeId::new();
        let other_tree_id = TreeId::new();
        let account = make_active_account(tenant_id);

        let mut policy = Policy::new(
            tenant_id,
            "Tree-specific access".to_string(),
            "Allow reads on a specific tree".to_string(),
            PolicyEffect::Allow,
            Scope::Tree(tenant_id, tree_id),
        );
        policy.subjects.push(PolicySubject::Account(account.id));
        policy.permissions.insert(Permission::TreeRead);
        policy.enabled = true;

        // Should allow on the specific tree.
        let request = AccessRequest::new(
            account.id,
            Permission::TreeRead,
            Scope::Tree(tenant_id, tree_id),
        );
        let decision = engine.evaluate(&request, &account, &[], &[], &[policy.clone()]);
        assert!(decision.is_allow());

        // Should deny on a different tree.
        let request2 = AccessRequest::new(
            account.id,
            Permission::TreeRead,
            Scope::Tree(tenant_id, other_tree_id),
        );
        let decision2 = engine.evaluate(&request2, &account, &[], &[], &[policy]);
        assert!(decision2.is_deny());
    }

    #[test]
    fn test_per_branch_abac_policy() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let tree_id = TreeId::new();
        let main_branch = BranchId::new();
        let feature_branch = BranchId::new();
        let account = make_active_account(tenant_id);

        // Deny writes on main branch
        let mut deny_main = Policy::new(
            tenant_id,
            "Protect main".to_string(),
            "Deny direct writes to main branch".to_string(),
            PolicyEffect::Deny,
            Scope::Branch(tenant_id, tree_id, main_branch),
        );
        deny_main.subjects.push(PolicySubject::AllAuthenticated);
        deny_main.permissions.insert(Permission::TreeWrite);
        deny_main.enabled = true;

        // Developer role allows TreeWrite
        let dev_role = Role::developer(tenant_id);
        let team = make_team(tenant_id, account.id, dev_role.id);

        // Should be denied on main branch (ABAC deny overrides RBAC allow).
        let request_main = AccessRequest::new(
            account.id,
            Permission::TreeWrite,
            Scope::Branch(tenant_id, tree_id, main_branch),
        );
        let decision = engine.evaluate(
            &request_main,
            &account,
            std::slice::from_ref(&team),
            std::slice::from_ref(&dev_role),
            &[deny_main.clone()],
        );
        assert!(decision.is_deny());

        // Should be allowed on feature branch (no deny policy).
        let request_feature = AccessRequest::new(
            account.id,
            Permission::TreeWrite,
            Scope::Branch(tenant_id, tree_id, feature_branch),
        );
        let decision2 = engine.evaluate(
            &request_feature,
            &account,
            &[team],
            &[dev_role],
            &[deny_main],
        );
        assert!(decision2.is_allow());
    }

    #[test]
    fn test_access_decision_helpers() {
        let allow = AccessDecision::Allow;
        assert!(allow.is_allow());
        assert!(!allow.is_deny());

        let deny = AccessDecision::deny("test reason");
        assert!(deny.is_deny());
        assert!(!deny.is_allow());
        if let AccessDecision::Deny { reason } = deny {
            assert_eq!(reason, "test reason");
        }
    }

    #[test]
    fn test_request_set_attribute() {
        let mut request = AccessRequest::new(AccountId::new(), Permission::TreeRead, Scope::Global);
        request.set_attribute("ip_address", "192.168.1.1");
        assert_eq!(request.attributes.get("ip_address").unwrap(), "192.168.1.1");
    }

    #[test]
    fn test_deactivated_account_denied() {
        let engine = AccessEngine::new();
        let tenant_id = make_tenant();
        let mut account = make_active_account(tenant_id);
        account.deactivate();

        let dev_role = Role::developer(tenant_id);
        let team = make_team(tenant_id, account.id, dev_role.id);

        let request =
            AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(tenant_id));

        let decision = engine.evaluate(&request, &account, &[team], &[dev_role], &[]);
        assert!(decision.is_deny());
    }

    fn allow_everyone(tenant_id: TenantId, scope: Scope, perm: Permission) -> Policy {
        let mut p = Policy::new_allow(tenant_id, "allow", scope, [perm].into_iter().collect());
        p.add_subject(PolicySubject::Everyone);
        p
    }

    #[test]
    fn test_owner_role_is_not_cross_tenant() {
        // Regression: Role::owner used to include GlobalAdmin, so every
        // tenant owner could access every other tenant.
        let engine = AccessEngine::new();
        let (a, b) = (make_tenant(), make_tenant());
        let account = make_active_account(a);
        let owner = Role::owner(a);
        let team = make_team(a, account.id, owner.id);

        let own = AccessRequest::new(account.id, Permission::TreeDelete, Scope::Tenant(a));
        assert!(engine
            .evaluate(
                &own,
                &account,
                std::slice::from_ref(&team),
                std::slice::from_ref(&owner),
                &[]
            )
            .is_allow());

        let foreign = AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(b));
        assert!(engine
            .evaluate(&foreign, &account, &[team], &[owner], &[])
            .is_deny());
    }

    #[test]
    fn test_foreign_tenant_policy_is_ignored() {
        // Regression: a tenant-A policy with Scope::Global granted access
        // inside tenant B.
        let engine = AccessEngine::new();
        let (a, b) = (make_tenant(), make_tenant());
        let account = make_active_account(b);
        let evil = allow_everyone(a, Scope::Global, Permission::TreeRead);
        let request = AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(b));
        assert!(engine
            .evaluate(&request, &account, &[], &[], &[evil])
            .is_deny());

        // Same for deny policies: tenant A cannot lock users out of tenant B.
        let viewer = Role::viewer(b);
        let team = make_team(b, account.id, viewer.id);
        let mut deny = Policy::new_deny(
            a,
            "deny",
            Scope::Global,
            [Permission::TreeRead].into_iter().collect(),
        );
        deny.add_subject(PolicySubject::Everyone);
        assert!(engine
            .evaluate(&request, &account, &[team], &[viewer], &[deny])
            .is_allow());
    }

    #[test]
    fn test_global_policy_of_own_tenant_applies() {
        let engine = AccessEngine::new();
        let t = make_tenant();
        let account = make_active_account(t);
        let p = allow_everyone(t, Scope::Global, Permission::TreeRead);
        let request = AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(t));
        assert!(engine
            .evaluate(&request, &account, &[], &[], &[p])
            .is_allow());
    }

    #[test]
    fn test_account_attributes_feed_conditions() {
        let engine = AccessEngine::new();
        let t = make_tenant();
        let mut account = make_active_account(t);
        account.set_attribute("department", "eng");
        let mut p = allow_everyone(t, Scope::Tenant(t), Permission::TreeRead);
        p.add_condition(AttributeCondition::new(
            "department",
            ConditionOperator::Equals,
            "eng",
        ));
        let mut request = AccessRequest::new(account.id, Permission::TreeRead, Scope::Tenant(t));
        assert!(engine
            .evaluate(&request, &account, &[], &[], std::slice::from_ref(&p))
            .is_allow());

        // A client-supplied attribute cannot override the account's.
        account.set_attribute("department", "sales");
        request.set_attribute("department", "eng");
        assert!(engine
            .evaluate(&request, &account, &[], &[], &[p])
            .is_deny());
    }

    #[test]
    fn test_admin_permissions_imply_lower_ones() {
        let engine = AccessEngine::new();
        let t = make_tenant();
        let account = make_active_account(t);

        let mut tenant_admin = Role::new(t, "ta", "");
        tenant_admin.grant(Permission::TenantAdmin);
        let team = make_team(t, account.id, tenant_admin.id);
        let request = AccessRequest::new(account.id, Permission::BranchMerge, Scope::Tenant(t));
        assert!(engine
            .evaluate(
                &request,
                &account,
                std::slice::from_ref(&team),
                std::slice::from_ref(&tenant_admin),
                &[]
            )
            .is_allow());
        // TenantAdmin does not imply GlobalAdmin.
        let request = AccessRequest::new(account.id, Permission::GlobalAdmin, Scope::Global);
        assert!(engine
            .evaluate(&request, &account, &[team], &[tenant_admin], &[])
            .is_deny());

        let mut tree_admin = Role::new(t, "tra", "");
        tree_admin.grant(Permission::TreeAdmin);
        let team = make_team(t, account.id, tree_admin.id);
        let tree_scope = Scope::Tree(t, TreeId::new());
        let write = AccessRequest::new(account.id, Permission::TreeWrite, tree_scope.clone());
        assert!(engine
            .evaluate(
                &write,
                &account,
                std::slice::from_ref(&team),
                std::slice::from_ref(&tree_admin),
                &[]
            )
            .is_allow());
        let merge = AccessRequest::new(account.id, Permission::BranchMerge, tree_scope);
        assert!(engine
            .evaluate(&merge, &account, &[team], &[tree_admin], &[])
            .is_deny());
    }

    #[test]
    fn test_deny_overrides_allow_regardless_of_priority() {
        let engine = AccessEngine::new();
        let t = make_tenant();
        let account = make_active_account(t);
        let owner = Role::owner(t);
        let team = make_team(t, account.id, owner.id);

        let mut allow = allow_everyone(t, Scope::Tenant(t), Permission::TreeWrite);
        allow.priority = 1000;
        let mut deny = Policy::new_deny(
            t,
            "deny",
            Scope::Tenant(t),
            [Permission::TreeWrite].into_iter().collect(),
        );
        deny.priority = -1000;
        deny.add_subject(PolicySubject::Everyone);

        let request = AccessRequest::new(account.id, Permission::TreeWrite, Scope::Tenant(t));
        for policies in [vec![allow.clone(), deny.clone()], vec![deny, allow]] {
            assert!(engine
                .evaluate(
                    &request,
                    &account,
                    std::slice::from_ref(&team),
                    std::slice::from_ref(&owner),
                    &policies
                )
                .is_deny());
        }
    }

    #[test]
    fn test_mismatched_account_is_denied() {
        let engine = AccessEngine::new();
        let t = make_tenant();
        let account = make_active_account(t);
        let owner = Role::owner(t);
        let other = AccountId::new();
        let team = make_team(t, other, owner.id);
        let request = AccessRequest::new(other, Permission::TreeRead, Scope::Tenant(t));
        assert!(engine
            .evaluate(&request, &account, &[team], &[owner], &[])
            .is_deny());
    }
}
