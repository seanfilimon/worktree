//! Per-tree access control entries.
//!
//! Provides [`TreeAccessSubject`], [`TreeAccessRule`], and [`TreeAccessList`]
//! for managing which subjects (accounts, teams, roles, etc.) are granted
//! or denied specific permissions on a worktree.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

use crate::core::id::{AccountId, RoleId, TeamId, TenantId, TreeId};
use crate::iam::permission::Permission;
use crate::iam::policy::PolicyEffect;

// ---------------------------------------------------------------------------
// TreeAccessSubject
// ---------------------------------------------------------------------------

/// A subject that can be granted or denied access to a tree.
///
/// Subjects are the *who* in the access-control model. They can be specific
/// accounts, teams, roles, all authenticated users, or the public.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TreeAccessSubject {
    /// A specific user account.
    Account(AccountId),
    /// A team (group of accounts).
    Team(TeamId),
    /// A role (set of permissions).
    Role(RoleId),
    /// Any authenticated user (logged in but not necessarily a member of the tree).
    AllAuthenticated,
    /// Unauthenticated / anonymous access.
    Public,
}

impl fmt::Display for TreeAccessSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeAccessSubject::Account(id) => write!(f, "account:{}", id),
            TreeAccessSubject::Team(id) => write!(f, "team:{}", id),
            TreeAccessSubject::Role(id) => write!(f, "role:{}", id),
            TreeAccessSubject::AllAuthenticated => write!(f, "all_authenticated"),
            TreeAccessSubject::Public => write!(f, "public"),
        }
    }
}

// ---------------------------------------------------------------------------
// TreeAccessRule
// ---------------------------------------------------------------------------

/// A single access-control rule applied to a tree.
///
/// Each rule binds a [`TreeAccessSubject`] to a set of [`Permission`]s with
/// a given [`PolicyEffect`] (allow or deny).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeAccessRule {
    /// The tenant that owns the tree.
    pub tenant_id: TenantId,
    /// The tree this rule applies to.
    pub tree_id: TreeId,
    /// The subject being granted or denied permissions.
    pub subject: TreeAccessSubject,
    /// Whether this rule allows or denies.
    pub effect: PolicyEffect,
    /// The set of permissions this rule covers.
    pub permissions: HashSet<Permission>,
    /// Whether this rule was inherited from a parent tree or tenant.
    pub inherited: bool,
    /// When this rule was created.
    pub created_at: DateTime<Utc>,
    /// The account that created this rule.
    pub created_by: AccountId,
}

impl TreeAccessRule {
    /// Create a new tree access rule.
    pub fn new(
        tenant_id: TenantId,
        tree_id: TreeId,
        subject: TreeAccessSubject,
        effect: PolicyEffect,
        permissions: HashSet<Permission>,
        created_by: AccountId,
    ) -> Self {
        Self {
            tenant_id,
            tree_id,
            subject,
            effect,
            permissions,
            inherited: false,
            created_at: Utc::now(),
            created_by,
        }
    }

    /// Create an inherited copy of this rule.
    pub fn as_inherited(&self) -> Self {
        let mut rule = self.clone();
        rule.inherited = true;
        rule
    }

    /// Returns `true` if this rule grants the given permission.
    pub fn grants(&self, permission: &Permission) -> bool {
        self.effect == PolicyEffect::Allow && self.permissions.contains(permission)
    }

    /// Returns `true` if this rule denies the given permission.
    pub fn denies(&self, permission: &Permission) -> bool {
        self.effect == PolicyEffect::Deny && self.permissions.contains(permission)
    }

    /// Returns `true` if this rule matches the given account ID.
    pub fn matches_account(&self, account_id: &AccountId) -> bool {
        match &self.subject {
            TreeAccessSubject::Account(id) => id == account_id,
            TreeAccessSubject::AllAuthenticated => true,
            TreeAccessSubject::Public => true,
            _ => false,
        }
    }

    /// Returns `true` if this rule matches any of the given team IDs.
    pub fn matches_team(&self, team_ids: &[TeamId]) -> bool {
        match &self.subject {
            TreeAccessSubject::Team(id) => team_ids.contains(id),
            _ => false,
        }
    }

    /// Returns `true` if this rule matches any of the given role IDs.
    pub fn matches_role(&self, role_ids: &[RoleId]) -> bool {
        match &self.subject {
            TreeAccessSubject::Role(id) => role_ids.contains(id),
            _ => false,
        }
    }

    /// Returns `true` if this rule matches any of the given subjects.
    pub fn matches_any(
        &self,
        account_ids: &[AccountId],
        team_ids: &[TeamId],
        role_ids: &[RoleId],
    ) -> bool {
        match &self.subject {
            TreeAccessSubject::Account(id) => account_ids.contains(id),
            TreeAccessSubject::Team(id) => team_ids.contains(id),
            TreeAccessSubject::Role(id) => role_ids.contains(id),
            TreeAccessSubject::AllAuthenticated => !account_ids.is_empty(),
            TreeAccessSubject::Public => true,
        }
    }
}

// ---------------------------------------------------------------------------
// TreeAccessList
// ---------------------------------------------------------------------------

/// An ordered list of tree access rules.
///
/// The list implements a deny-overrides-allow evaluation strategy: if any
/// matching rule denies a permission, the result is [`PolicyEffect::Deny`]
/// regardless of any allow rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeAccessList {
    /// The rules in this access list.
    pub rules: Vec<TreeAccessRule>,
}

impl TreeAccessList {
    /// Create a new, empty tree access list.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the access list.
    pub fn add_rule(&mut self, rule: TreeAccessRule) {
        self.rules.push(rule);
    }

    /// Remove a rule by index. Returns `Some(rule)` if the index was valid.
    pub fn remove_rule(&mut self, index: usize) -> Option<TreeAccessRule> {
        if index < self.rules.len() {
            Some(self.rules.remove(index))
        } else {
            None
        }
    }

    /// Return all rules that target the given account.
    pub fn rules_for_account(&self, account_id: &AccountId) -> Vec<&TreeAccessRule> {
        self.rules
            .iter()
            .filter(|r| r.matches_account(account_id))
            .collect()
    }

    /// Return all rules that target the given team.
    pub fn rules_for_team(&self, team_id: &TeamId) -> Vec<&TreeAccessRule> {
        self.rules
            .iter()
            .filter(|r| matches!(&r.subject, TreeAccessSubject::Team(id) if id == team_id))
            .collect()
    }

    /// Return all rules that target the given role.
    pub fn rules_for_role(&self, role_id: &RoleId) -> Vec<&TreeAccessRule> {
        self.rules
            .iter()
            .filter(|r| matches!(&r.subject, TreeAccessSubject::Role(id) if id == role_id))
            .collect()
    }

    /// Evaluate whether the given subjects have access to the given permission.
    ///
    /// **Evaluation logic (deny overrides allow):**
    /// 1. Iterate all rules and find those matching any of the provided subjects.
    /// 2. Collect all allows and denies for the requested permission.
    /// 3. If *any* matching rule denies the permission, return `Deny`.
    /// 4. If at least one matching rule allows the permission, return `Allow`.
    /// 5. If no matching rule exists, return `Deny` (implicit deny).
    pub fn check(
        &self,
        subject_accounts: &[AccountId],
        subject_teams: &[TeamId],
        subject_roles: &[RoleId],
        permission: &Permission,
    ) -> PolicyEffect {
        let mut has_allow = false;

        for rule in &self.rules {
            if !rule.matches_any(subject_accounts, subject_teams, subject_roles) {
                continue;
            }

            if !rule.permissions.contains(permission) {
                continue;
            }

            match rule.effect {
                PolicyEffect::Deny => return PolicyEffect::Deny,
                PolicyEffect::Allow => {
                    has_allow = true;
                }
            }
        }

        if has_allow {
            PolicyEffect::Allow
        } else {
            PolicyEffect::Deny
        }
    }

    /// Returns the number of rules in the access list.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` if the access list has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

impl Default for TreeAccessList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_allow_rule(
        tenant_id: TenantId,
        tree_id: TreeId,
        subject: TreeAccessSubject,
        permissions: Vec<Permission>,
        created_by: AccountId,
    ) -> TreeAccessRule {
        TreeAccessRule::new(
            tenant_id,
            tree_id,
            subject,
            PolicyEffect::Allow,
            permissions.into_iter().collect(),
            created_by,
        )
    }

    fn make_deny_rule(
        tenant_id: TenantId,
        tree_id: TreeId,
        subject: TreeAccessSubject,
        permissions: Vec<Permission>,
        created_by: AccountId,
    ) -> TreeAccessRule {
        TreeAccessRule::new(
            tenant_id,
            tree_id,
            subject,
            PolicyEffect::Deny,
            permissions.into_iter().collect(),
            created_by,
        )
    }

    #[test]
    fn test_grants_and_denies() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let allow_rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        );

        assert!(allow_rule.grants(&Permission::TreeRead));
        assert!(allow_rule.grants(&Permission::TreeWrite));
        assert!(!allow_rule.grants(&Permission::TreeDelete));
        assert!(!allow_rule.denies(&Permission::TreeRead));

        let deny_rule = make_deny_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeDelete],
            admin,
        );

        assert!(deny_rule.denies(&Permission::TreeDelete));
        assert!(!deny_rule.grants(&Permission::TreeDelete));
        assert!(!deny_rule.denies(&Permission::TreeRead));
    }

    #[test]
    fn test_matches_account() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let other = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        );

        assert!(rule.matches_account(&user));
        assert!(!rule.matches_account(&other));
    }

    #[test]
    fn test_matches_team() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let team = TeamId::new();
        let other_team = TeamId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team),
            vec![Permission::TreeRead],
            admin,
        );

        assert!(rule.matches_team(&[team]));
        assert!(rule.matches_team(&[team, other_team]));
        assert!(!rule.matches_team(&[other_team]));
        assert!(!rule.matches_team(&[]));
    }

    #[test]
    fn test_matches_role() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let role = RoleId::new();
        let other_role = RoleId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Role(role),
            vec![Permission::TreeRead],
            admin,
        );

        assert!(rule.matches_role(&[role]));
        assert!(!rule.matches_role(&[other_role]));
    }

    #[test]
    fn test_all_authenticated_matches_any_account() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::AllAuthenticated,
            vec![Permission::TreeRead],
            admin,
        );

        assert!(rule.matches_account(&user));
        assert!(rule.matches_account(&admin));
    }

    #[test]
    fn test_public_matches_everyone() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Public,
            vec![Permission::TreeRead],
            admin,
        );

        assert!(rule.matches_account(&AccountId::new()));
    }

    #[test]
    fn test_access_list_allow() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);

        let result = acl.check(&[user], &[], &[], &Permission::TreeWrite);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_access_list_implicit_deny() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        ));

        // TreeWrite is not in the allow rule, so it should be implicitly denied.
        let result = acl.check(&[user], &[], &[], &Permission::TreeWrite);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_access_list_explicit_deny_overrides_allow() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team = TeamId::new();

        let mut acl = TreeAccessList::new();

        // Allow TreeWrite to the user's account
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        ));

        // Deny TreeWrite to the team the user belongs to
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team),
            vec![Permission::TreeWrite],
            admin,
        ));

        // TreeRead should still be allowed (no deny for it)
        let result = acl.check(&[user], &[team], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);

        // TreeWrite should be denied (deny overrides allow)
        let result = acl.check(&[user], &[team], &[], &Permission::TreeWrite);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_deny_overrides_allow_same_subject() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();

        // Allow first
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeDelete],
            admin,
        ));

        // Then deny the same permission for the same account
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeDelete],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::TreeDelete);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_empty_acl_denies_everything() {
        let acl = TreeAccessList::new();
        let user = AccountId::new();

        let result = acl.check(&[user], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_no_matching_subjects_deny() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user_a = AccountId::new();
        let user_b = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user_a),
            vec![Permission::TreeRead],
            admin,
        ));

        // user_b is not user_a, so no matching rules
        let result = acl.check(&[user_b], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_team_based_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let dev_team = TeamId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(dev_team),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        ));

        // User is in the dev_team
        let result = acl.check(&[user], &[dev_team], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_role_based_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let editor_role = RoleId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Role(editor_role),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        ));

        let result = acl.check(&[user], &[], &[editor_role], &Permission::TreeWrite);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_all_authenticated_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::AllAuthenticated,
            vec![Permission::TreeRead],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);

        // No accounts means not authenticated
        let result = acl.check(&[], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_public_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Public,
            vec![Permission::TreeRead],
            admin,
        ));

        // Even with no subjects at all, Public matches
        let result = acl.check(&[], &[], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_rules_for_account() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let other = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(other),
            vec![Permission::TreeWrite],
            admin,
        ));

        let user_rules = acl.rules_for_account(&user);
        assert_eq!(user_rules.len(), 1);
        assert!(user_rules[0].grants(&Permission::TreeRead));
    }

    #[test]
    fn test_rules_for_team() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let team_a = TeamId::new();
        let team_b = TeamId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team_a),
            vec![Permission::TreeRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team_b),
            vec![Permission::TreeWrite],
            admin,
        ));

        let team_a_rules = acl.rules_for_team(&team_a);
        assert_eq!(team_a_rules.len(), 1);
    }

    #[test]
    fn test_remove_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        ));
        assert_eq!(acl.len(), 1);

        let removed = acl.remove_rule(0);
        assert!(removed.is_some());
        assert_eq!(acl.len(), 0);
        assert!(acl.is_empty());

        // Out of bounds
        let removed = acl.remove_rule(99);
        assert!(removed.is_none());
    }

    #[test]
    fn test_inherited_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        );
        assert!(!rule.inherited);

        let inherited = rule.as_inherited();
        assert!(inherited.inherited);
        assert_eq!(inherited.subject, rule.subject);
        assert_eq!(inherited.effect, rule.effect);
        assert_eq!(inherited.permissions, rule.permissions);
    }

    #[test]
    fn test_multiple_allow_rules_combined() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team = TeamId::new();

        let mut acl = TreeAccessList::new();

        // Account-level allow for Read
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        ));

        // Team-level allow for Write
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team),
            vec![Permission::TreeWrite],
            admin,
        ));

        // Both should be allowed
        let result = acl.check(&[user], &[team], &[], &Permission::TreeRead);
        assert_eq!(result, PolicyEffect::Allow);

        let result = acl.check(&[user], &[team], &[], &Permission::TreeWrite);
        assert_eq!(result, PolicyEffect::Allow);

        // Delete is still denied
        let result = acl.check(&[user], &[team], &[], &Permission::TreeDelete);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_subject_display() {
        let account_id = AccountId::new();
        let team_id = TeamId::new();
        let role_id = RoleId::new();

        let s = TreeAccessSubject::Account(account_id).to_string();
        assert!(s.starts_with("account:"));

        let s = TreeAccessSubject::Team(team_id).to_string();
        assert!(s.starts_with("team:"));

        let s = TreeAccessSubject::Role(role_id).to_string();
        assert!(s.starts_with("role:"));

        assert_eq!(
            TreeAccessSubject::AllAuthenticated.to_string(),
            "all_authenticated"
        );
        assert_eq!(TreeAccessSubject::Public.to_string(), "public");
    }

    #[test]
    fn test_serde_json_roundtrip_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead, Permission::TreeWrite],
            admin,
        );

        let json = serde_json::to_string(&rule).expect("serialize");
        let deserialized: TreeAccessRule = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn test_serde_json_roundtrip_acl() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead],
            admin,
        ));
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            TreeAccessSubject::Public,
            vec![Permission::TreeWrite],
            admin,
        ));

        let json = serde_json::to_string(&acl).expect("serialize");
        let deserialized: TreeAccessList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(acl, deserialized);
    }

    #[test]
    fn test_default_access_list() {
        let acl = TreeAccessList::default();
        assert!(acl.is_empty());
        assert_eq!(acl.len(), 0);
    }

    #[test]
    fn test_complex_deny_overrides_scenario() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team_a = TeamId::new();
        let team_b = TeamId::new();
        let role = RoleId::new();

        let mut acl = TreeAccessList::new();

        // Allow via account
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Account(user),
            vec![Permission::TreeRead, Permission::TreeWrite, Permission::TreeDelete],
            admin,
        ));

        // Allow via team_a
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team_a),
            vec![Permission::TreeAdmin],
            admin,
        ));

        // Deny TreeDelete via team_b
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            TreeAccessSubject::Team(team_b),
            vec![Permission::TreeDelete],
            admin,
        ));

        // Allow via role
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Role(role),
            vec![Permission::TreeDelete],
            admin,
        ));

        // TreeRead and TreeWrite should be allowed
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::TreeRead,
        );
        assert_eq!(result, PolicyEffect::Allow);

        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::TreeWrite,
        );
        assert_eq!(result, PolicyEffect::Allow);

        // TreeDelete should be denied because team_b denies it (deny overrides all allows)
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::TreeDelete,
        );
        assert_eq!(result, PolicyEffect::Deny);

        // TreeAdmin should be allowed via team_a (no deny for it)
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::TreeAdmin,
        );
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_rules_for_role() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let admin = AccountId::new();
        let role_a = RoleId::new();
        let role_b = RoleId::new();

        let mut acl = TreeAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Role(role_a),
            vec![Permission::TreeRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            TreeAccessSubject::Role(role_b),
            vec![Permission::TreeWrite],
            admin,
        ));

        let role_a_rules = acl.rules_for_role(&role_a);
        assert_eq!(role_a_rules.len(), 1);
        assert!(role_a_rules[0].grants(&Permission::TreeRead));

        let role_b_rules = acl.rules_for_role(&role_b);
        assert_eq!(role_b_rules.len(), 1);
        assert!(role_b_rules[0].grants(&Permission::TreeWrite));
    }
}
