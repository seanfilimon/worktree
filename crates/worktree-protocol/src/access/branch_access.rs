//! Per-branch access control entries.
//!
//! Provides [`BranchAccessRule`], [`BranchProtection`], and [`BranchAccessList`]
//! for managing which subjects are granted or denied specific permissions on
//! a branch, and for configuring branch protection rules (e.g. require review,
//! restrict force-push).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::core::id::{AccountId, BranchId, RoleId, TeamId, TenantId, TreeId};
use crate::iam::permission::Permission;
use crate::iam::policy::PolicyEffect;

// Re-use TreeAccessSubject from the sibling module.
use crate::access::tree_access::TreeAccessSubject;

// ---------------------------------------------------------------------------
// BranchAccessRule
// ---------------------------------------------------------------------------

/// A single access-control rule applied to a specific branch.
///
/// Each rule binds a [`TreeAccessSubject`] to a set of [`Permission`]s with
/// a given [`PolicyEffect`] (allow or deny), scoped to a particular branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchAccessRule {
    /// The tenant that owns the tree.
    pub tenant_id: TenantId,
    /// The tree the branch belongs to.
    pub tree_id: TreeId,
    /// The branch this rule applies to.
    pub branch_id: BranchId,
    /// The subject being granted or denied permissions.
    pub subject: TreeAccessSubject,
    /// Whether this rule allows or denies.
    pub effect: PolicyEffect,
    /// The set of permissions this rule covers.
    pub permissions: HashSet<Permission>,
    /// If `true`, this branch has protection rules applied.
    pub protected: bool,
    /// When this rule was created.
    pub created_at: DateTime<Utc>,
    /// The account that created this rule.
    pub created_by: AccountId,
}

impl BranchAccessRule {
    /// Create a new branch access rule.
    pub fn new(
        tenant_id: TenantId,
        tree_id: TreeId,
        branch_id: BranchId,
        subject: TreeAccessSubject,
        effect: PolicyEffect,
        permissions: HashSet<Permission>,
        created_by: AccountId,
    ) -> Self {
        Self {
            tenant_id,
            tree_id,
            branch_id,
            subject,
            effect,
            permissions,
            protected: false,
            created_at: Utc::now(),
            created_by,
        }
    }

    /// Return a copy of this rule with the `protected` flag set.
    pub fn with_protected(mut self, protected: bool) -> Self {
        self.protected = protected;
        self
    }

    /// Returns `true` if this rule grants the given permission.
    pub fn grants(&self, permission: &Permission) -> bool {
        self.effect == PolicyEffect::Allow && self.permissions.contains(permission)
    }

    /// Returns `true` if this rule denies the given permission.
    pub fn denies(&self, permission: &Permission) -> bool {
        self.effect == PolicyEffect::Deny && self.permissions.contains(permission)
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
// BranchProtection
// ---------------------------------------------------------------------------

/// Branch protection configuration.
///
/// Describes the constraints placed on a protected branch, such as requiring
/// reviews before merge, restricting who can push or merge, and controlling
/// force-push and deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchProtection {
    /// The branch this protection applies to.
    pub branch_id: BranchId,
    /// The tree that owns the branch.
    pub tree_id: TreeId,
    /// Whether snapshots must be reviewed before they can be merged.
    pub require_snapshot_review: bool,
    /// Whether all configured checks must pass before merge.
    pub require_passing_checks: bool,
    /// Only these subjects are allowed to push to the branch.
    /// An empty list means no restriction (anyone with write access can push).
    pub restrict_push: Vec<TreeAccessSubject>,
    /// Only these subjects are allowed to merge into the branch.
    /// An empty list means no restriction.
    pub restrict_merge: Vec<TreeAccessSubject>,
    /// Whether force-push is allowed on this branch. Default is `false`.
    pub allow_force_push: bool,
    /// Whether the branch can be deleted. Default is `false`.
    pub allow_deletion: bool,
}

impl BranchProtection {
    /// Create a new branch protection with sensible defaults (everything restricted).
    pub fn new(branch_id: BranchId, tree_id: TreeId) -> Self {
        Self {
            branch_id,
            tree_id,
            require_snapshot_review: true,
            require_passing_checks: true,
            restrict_push: Vec::new(),
            restrict_merge: Vec::new(),
            allow_force_push: false,
            allow_deletion: false,
        }
    }

    /// Returns `true` if the given subject is allowed to push.
    ///
    /// If `restrict_push` is empty, any subject is allowed.
    pub fn can_push(&self, subject: &TreeAccessSubject) -> bool {
        if self.restrict_push.is_empty() {
            return true;
        }
        self.restrict_push.contains(subject)
    }

    /// Returns `true` if the given subject is allowed to merge.
    ///
    /// If `restrict_merge` is empty, any subject is allowed.
    pub fn can_merge(&self, subject: &TreeAccessSubject) -> bool {
        if self.restrict_merge.is_empty() {
            return true;
        }
        self.restrict_merge.contains(subject)
    }

    /// Builder method: set `require_snapshot_review`.
    pub fn with_require_snapshot_review(mut self, require: bool) -> Self {
        self.require_snapshot_review = require;
        self
    }

    /// Builder method: set `require_passing_checks`.
    pub fn with_require_passing_checks(mut self, require: bool) -> Self {
        self.require_passing_checks = require;
        self
    }

    /// Builder method: set `allow_force_push`.
    pub fn with_allow_force_push(mut self, allow: bool) -> Self {
        self.allow_force_push = allow;
        self
    }

    /// Builder method: set `allow_deletion`.
    pub fn with_allow_deletion(mut self, allow: bool) -> Self {
        self.allow_deletion = allow;
        self
    }

    /// Builder method: set `restrict_push`.
    pub fn with_restrict_push(mut self, subjects: Vec<TreeAccessSubject>) -> Self {
        self.restrict_push = subjects;
        self
    }

    /// Builder method: set `restrict_merge`.
    pub fn with_restrict_merge(mut self, subjects: Vec<TreeAccessSubject>) -> Self {
        self.restrict_merge = subjects;
        self
    }
}

impl Default for BranchProtection {
    fn default() -> Self {
        Self {
            branch_id: BranchId::nil(),
            tree_id: TreeId::nil(),
            require_snapshot_review: true,
            require_passing_checks: true,
            restrict_push: Vec::new(),
            restrict_merge: Vec::new(),
            allow_force_push: false,
            allow_deletion: false,
        }
    }
}

// ---------------------------------------------------------------------------
// BranchAccessList
// ---------------------------------------------------------------------------

/// An ordered list of branch access rules.
///
/// Implements the same deny-overrides-allow evaluation strategy as
/// [`crate::access::tree_access::TreeAccessList`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchAccessList {
    /// The rules in this access list.
    pub rules: Vec<BranchAccessRule>,
}

impl BranchAccessList {
    /// Create a new, empty branch access list.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the access list.
    pub fn add_rule(&mut self, rule: BranchAccessRule) {
        self.rules.push(rule);
    }

    /// Remove a rule by index. Returns `Some(rule)` if the index was valid.
    pub fn remove_rule(&mut self, index: usize) -> Option<BranchAccessRule> {
        if index < self.rules.len() {
            Some(self.rules.remove(index))
        } else {
            None
        }
    }

    /// Return all rules that target the given account (directly or via wildcards).
    pub fn rules_for_account(&self, account_id: &AccountId) -> Vec<&BranchAccessRule> {
        self.rules
            .iter()
            .filter(|r| match &r.subject {
                TreeAccessSubject::Account(id) => id == account_id,
                TreeAccessSubject::AllAuthenticated | TreeAccessSubject::Public => true,
                _ => false,
            })
            .collect()
    }

    /// Return all rules that target the given team.
    pub fn rules_for_team(&self, team_id: &TeamId) -> Vec<&BranchAccessRule> {
        self.rules
            .iter()
            .filter(|r| matches!(&r.subject, TreeAccessSubject::Team(id) if id == team_id))
            .collect()
    }

    /// Return all rules that target the given role.
    pub fn rules_for_role(&self, role_id: &RoleId) -> Vec<&BranchAccessRule> {
        self.rules
            .iter()
            .filter(|r| matches!(&r.subject, TreeAccessSubject::Role(id) if id == role_id))
            .collect()
    }

    /// Evaluate whether the given subjects have access to the given permission.
    ///
    /// **Evaluation logic (deny overrides allow):**
    /// 1. Iterate all rules and find those matching any of the provided subjects.
    /// 2. Collect allows and denies for the requested permission.
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

impl Default for BranchAccessList {
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
        branch_id: BranchId,
        subject: TreeAccessSubject,
        permissions: Vec<Permission>,
        created_by: AccountId,
    ) -> BranchAccessRule {
        BranchAccessRule::new(
            tenant_id,
            tree_id,
            branch_id,
            subject,
            PolicyEffect::Allow,
            permissions.into_iter().collect(),
            created_by,
        )
    }

    fn make_deny_rule(
        tenant_id: TenantId,
        tree_id: TreeId,
        branch_id: BranchId,
        subject: TreeAccessSubject,
        permissions: Vec<Permission>,
        created_by: AccountId,
    ) -> BranchAccessRule {
        BranchAccessRule::new(
            tenant_id,
            tree_id,
            branch_id,
            subject,
            PolicyEffect::Deny,
            permissions.into_iter().collect(),
            created_by,
        )
    }

    // ── BranchAccessRule tests ──────────────────────────────────────────

    #[test]
    fn test_grants_and_denies() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let allow = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead, Permission::BranchMerge],
            admin,
        );

        assert!(allow.grants(&Permission::BranchRead));
        assert!(allow.grants(&Permission::BranchMerge));
        assert!(!allow.grants(&Permission::BranchDelete));
        assert!(!allow.denies(&Permission::BranchRead));

        let deny = make_deny_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchDelete],
            admin,
        );

        assert!(deny.denies(&Permission::BranchDelete));
        assert!(!deny.grants(&Permission::BranchDelete));
        assert!(!deny.denies(&Permission::BranchRead));
    }

    #[test]
    fn test_matches_any_account() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let other = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        );

        assert!(rule.matches_any(&[user], &[], &[]));
        assert!(!rule.matches_any(&[other], &[], &[]));
        assert!(!rule.matches_any(&[], &[], &[]));
    }

    #[test]
    fn test_matches_any_team() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let team = TeamId::new();
        let other_team = TeamId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team),
            vec![Permission::BranchRead],
            admin,
        );

        assert!(rule.matches_any(&[], &[team], &[]));
        assert!(rule.matches_any(&[], &[team, other_team], &[]));
        assert!(!rule.matches_any(&[], &[other_team], &[]));
    }

    #[test]
    fn test_matches_any_role() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let role = RoleId::new();
        let other_role = RoleId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Role(role),
            vec![Permission::BranchRead],
            admin,
        );

        assert!(rule.matches_any(&[], &[], &[role]));
        assert!(!rule.matches_any(&[], &[], &[other_role]));
    }

    #[test]
    fn test_matches_any_all_authenticated() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::AllAuthenticated,
            vec![Permission::BranchRead],
            admin,
        );

        assert!(rule.matches_any(&[user], &[], &[]));
        assert!(!rule.matches_any(&[], &[], &[]));
    }

    #[test]
    fn test_matches_any_public() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Public,
            vec![Permission::BranchRead],
            admin,
        );

        assert!(rule.matches_any(&[], &[], &[]));
    }

    #[test]
    fn test_with_protected() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        )
        .with_protected(true);

        assert!(rule.protected);
    }

    // ── BranchProtection tests ──────────────────────────────────────────

    #[test]
    fn test_protection_new_defaults() {
        let bid = BranchId::new();
        let trid = TreeId::new();
        let prot = BranchProtection::new(bid, trid);

        assert_eq!(prot.branch_id, bid);
        assert_eq!(prot.tree_id, trid);
        assert!(prot.require_snapshot_review);
        assert!(prot.require_passing_checks);
        assert!(prot.restrict_push.is_empty());
        assert!(prot.restrict_merge.is_empty());
        assert!(!prot.allow_force_push);
        assert!(!prot.allow_deletion);
    }

    #[test]
    fn test_protection_can_push_unrestricted() {
        let prot = BranchProtection::new(BranchId::new(), TreeId::new());
        let user = AccountId::new();

        // Empty restrict_push means anyone can push
        assert!(prot.can_push(&TreeAccessSubject::Account(user)));
        assert!(prot.can_push(&TreeAccessSubject::Public));
    }

    #[test]
    fn test_protection_can_push_restricted() {
        let user_a = AccountId::new();
        let user_b = AccountId::new();

        let prot = BranchProtection::new(BranchId::new(), TreeId::new())
            .with_restrict_push(vec![TreeAccessSubject::Account(user_a)]);

        assert!(prot.can_push(&TreeAccessSubject::Account(user_a)));
        assert!(!prot.can_push(&TreeAccessSubject::Account(user_b)));
    }

    #[test]
    fn test_protection_can_merge_unrestricted() {
        let prot = BranchProtection::new(BranchId::new(), TreeId::new());
        let user = AccountId::new();

        assert!(prot.can_merge(&TreeAccessSubject::Account(user)));
    }

    #[test]
    fn test_protection_can_merge_restricted() {
        let team = TeamId::new();
        let user = AccountId::new();

        let prot = BranchProtection::new(BranchId::new(), TreeId::new())
            .with_restrict_merge(vec![TreeAccessSubject::Team(team)]);

        assert!(prot.can_merge(&TreeAccessSubject::Team(team)));
        assert!(!prot.can_merge(&TreeAccessSubject::Account(user)));
    }

    #[test]
    fn test_protection_builder_methods() {
        let prot = BranchProtection::new(BranchId::new(), TreeId::new())
            .with_require_snapshot_review(false)
            .with_require_passing_checks(false)
            .with_allow_force_push(true)
            .with_allow_deletion(true);

        assert!(!prot.require_snapshot_review);
        assert!(!prot.require_passing_checks);
        assert!(prot.allow_force_push);
        assert!(prot.allow_deletion);
    }

    #[test]
    fn test_protection_default() {
        let prot = BranchProtection::default();
        assert_eq!(prot.branch_id, BranchId::nil());
        assert_eq!(prot.tree_id, TreeId::nil());
        assert!(prot.require_snapshot_review);
        assert!(prot.require_passing_checks);
        assert!(!prot.allow_force_push);
        assert!(!prot.allow_deletion);
    }

    #[test]
    fn test_protection_serde_json_roundtrip() {
        let user = AccountId::new();
        let team = TeamId::new();

        let prot = BranchProtection::new(BranchId::new(), TreeId::new())
            .with_restrict_push(vec![TreeAccessSubject::Account(user)])
            .with_restrict_merge(vec![TreeAccessSubject::Team(team)])
            .with_allow_force_push(true);

        let json = serde_json::to_string(&prot).expect("serialize");
        let deserialized: BranchProtection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(prot, deserialized);
    }

    // ── BranchAccessList tests ──────────────────────────────────────────

    #[test]
    fn test_empty_acl_denies_everything() {
        let acl = BranchAccessList::new();
        let user = AccountId::new();

        let result = acl.check(&[user], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_allow_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead, Permission::BranchMerge],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Allow);

        let result = acl.check(&[user], &[], &[], &Permission::BranchMerge);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_implicit_deny_for_unlisted_permission() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::BranchDelete);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_deny_overrides_allow() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team = TeamId::new();

        let mut acl = BranchAccessList::new();

        // Allow via account
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead, Permission::BranchMerge],
            admin,
        ));

        // Deny via team
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team),
            vec![Permission::BranchMerge],
            admin,
        ));

        // BranchRead is still allowed (no deny for it)
        let result = acl.check(&[user], &[team], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Allow);

        // BranchMerge is denied (deny overrides allow)
        let result = acl.check(&[user], &[team], &[], &Permission::BranchMerge);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_deny_overrides_allow_same_subject() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();

        // Allow first
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchDelete],
            admin,
        ));

        // Then deny same permission, same subject
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchDelete],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::BranchDelete);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_no_matching_subject_deny() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user_a = AccountId::new();
        let user_b = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user_a),
            vec![Permission::BranchRead],
            admin,
        ));

        let result = acl.check(&[user_b], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_team_based_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team = TeamId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team),
            vec![Permission::BranchRead, Permission::BranchCreate],
            admin,
        ));

        let result = acl.check(&[user], &[team], &[], &Permission::BranchCreate);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_role_based_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let role = RoleId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Role(role),
            vec![Permission::BranchProtect],
            admin,
        ));

        let result = acl.check(&[user], &[], &[role], &Permission::BranchProtect);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_public_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Public,
            vec![Permission::BranchRead],
            admin,
        ));

        let result = acl.check(&[], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Allow);
    }

    #[test]
    fn test_all_authenticated_access() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::AllAuthenticated,
            vec![Permission::BranchRead],
            admin,
        ));

        let result = acl.check(&[user], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Allow);

        // No accounts = not authenticated
        let result = acl.check(&[], &[], &[], &Permission::BranchRead);
        assert_eq!(result, PolicyEffect::Deny);
    }

    #[test]
    fn test_rules_for_account() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let other = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(other),
            vec![Permission::BranchMerge],
            admin,
        ));

        let user_rules = acl.rules_for_account(&user);
        assert_eq!(user_rules.len(), 1);
        assert!(user_rules[0].grants(&Permission::BranchRead));
    }

    #[test]
    fn test_rules_for_team() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let team_a = TeamId::new();
        let team_b = TeamId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team_a),
            vec![Permission::BranchRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team_b),
            vec![Permission::BranchMerge],
            admin,
        ));

        let rules = acl.rules_for_team(&team_a);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_rules_for_role() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let role_a = RoleId::new();
        let role_b = RoleId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Role(role_a),
            vec![Permission::BranchRead],
            admin,
        ));
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Role(role_b),
            vec![Permission::BranchProtect],
            admin,
        ));

        let rules = acl.rules_for_role(&role_a);
        assert_eq!(rules.len(), 1);
        assert!(rules[0].grants(&Permission::BranchRead));
    }

    #[test]
    fn test_remove_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        ));
        assert_eq!(acl.len(), 1);

        let removed = acl.remove_rule(0);
        assert!(removed.is_some());
        assert_eq!(acl.len(), 0);
        assert!(acl.is_empty());

        let removed = acl.remove_rule(99);
        assert!(removed.is_none());
    }

    #[test]
    fn test_default_acl() {
        let acl = BranchAccessList::default();
        assert!(acl.is_empty());
        assert_eq!(acl.len(), 0);
    }

    #[test]
    fn test_serde_json_roundtrip_rule() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let rule = make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead, Permission::BranchMerge],
            admin,
        )
        .with_protected(true);

        let json = serde_json::to_string(&rule).expect("serialize");
        let deserialized: BranchAccessRule = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn test_serde_json_roundtrip_acl() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();

        let mut acl = BranchAccessList::new();
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![Permission::BranchRead],
            admin,
        ));
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Public,
            vec![Permission::BranchDelete],
            admin,
        ));

        let json = serde_json::to_string(&acl).expect("serialize");
        let deserialized: BranchAccessList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(acl, deserialized);
    }

    #[test]
    fn test_complex_deny_overrides_scenario() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let admin = AccountId::new();
        let user = AccountId::new();
        let team_a = TeamId::new();
        let team_b = TeamId::new();
        let role = RoleId::new();

        let mut acl = BranchAccessList::new();

        // Allow via account
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Account(user),
            vec![
                Permission::BranchRead,
                Permission::BranchCreate,
                Permission::BranchDelete,
            ],
            admin,
        ));

        // Allow via team_a
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team_a),
            vec![Permission::BranchProtect],
            admin,
        ));

        // Deny BranchDelete via team_b
        acl.add_rule(make_deny_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Team(team_b),
            vec![Permission::BranchDelete],
            admin,
        ));

        // Allow via role
        acl.add_rule(make_allow_rule(
            tid,
            trid,
            bid,
            TreeAccessSubject::Role(role),
            vec![Permission::BranchDelete],
            admin,
        ));

        // BranchRead allowed
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::BranchRead,
        );
        assert_eq!(result, PolicyEffect::Allow);

        // BranchCreate allowed
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::BranchCreate,
        );
        assert_eq!(result, PolicyEffect::Allow);

        // BranchDelete denied (team_b denies, overrides all allows)
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::BranchDelete,
        );
        assert_eq!(result, PolicyEffect::Deny);

        // BranchProtect allowed via team_a (no deny for it)
        let result = acl.check(
            &[user],
            &[team_a, team_b],
            &[role],
            &Permission::BranchProtect,
        );
        assert_eq!(result, PolicyEffect::Allow);
    }
}
