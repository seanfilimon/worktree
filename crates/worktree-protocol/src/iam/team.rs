//! # Team Management
//!
//! Teams group accounts within a tenant. Roles are assigned to teams,
//! and teams are granted access to trees/branches.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{AccountId, RoleId, TeamId, TenantId};

/// A team groups accounts within a tenant.
///
/// Roles are assigned to teams, and all members of the team inherit
/// those roles. Teams are the primary mechanism for granting access
/// to trees and branches at scale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Unique identifier for this team.
    pub id: TeamId,

    /// The tenant this team belongs to.
    pub tenant_id: TenantId,

    /// Human-readable team name.
    pub name: String,

    /// Description of the team's purpose.
    pub description: String,

    /// The accounts that are members of this team.
    pub members: Vec<AccountId>,

    /// The roles assigned to this team. All members inherit these roles.
    pub roles: Vec<RoleId>,

    /// When this team was created.
    pub created_at: DateTime<Utc>,
}

impl Team {
    /// Creates a new team with the given name within a tenant.
    pub fn new(tenant_id: TenantId, name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: TeamId::new(),
            tenant_id,
            name: name.into(),
            description: description.into(),
            members: Vec::new(),
            roles: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Creates a new team with a specific ID.
    pub fn with_id(
        id: TeamId,
        tenant_id: TenantId,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            name: name.into(),
            description: description.into(),
            members: Vec::new(),
            roles: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Adds a member to the team. Returns `true` if the member was added,
    /// `false` if they were already a member.
    pub fn add_member(&mut self, account_id: AccountId) -> bool {
        if self.has_member(&account_id) {
            return false;
        }
        self.members.push(account_id);
        true
    }

    /// Removes a member from the team. Returns `true` if the member was removed,
    /// `false` if they were not a member.
    pub fn remove_member(&mut self, account_id: &AccountId) -> bool {
        let len_before = self.members.len();
        self.members.retain(|id| id != account_id);
        self.members.len() < len_before
    }

    /// Checks whether the given account is a member of this team.
    pub fn has_member(&self, account_id: &AccountId) -> bool {
        self.members.contains(account_id)
    }

    /// Adds a role to the team. Returns `true` if the role was added,
    /// `false` if the team already had that role.
    pub fn add_role(&mut self, role_id: RoleId) -> bool {
        if self.has_role(&role_id) {
            return false;
        }
        self.roles.push(role_id);
        true
    }

    /// Removes a role from the team. Returns `true` if the role was removed,
    /// `false` if the team did not have that role.
    pub fn remove_role(&mut self, role_id: &RoleId) -> bool {
        let len_before = self.roles.len();
        self.roles.retain(|id| id != role_id);
        self.roles.len() < len_before
    }

    /// Checks whether the team has the given role assigned.
    pub fn has_role(&self, role_id: &RoleId) -> bool {
        self.roles.contains(role_id)
    }

    /// Returns the number of members in this team.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Returns the number of roles assigned to this team.
    pub fn role_count(&self) -> usize {
        self.roles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_team() -> Team {
        let tenant_id = TenantId::new();
        Team::new(tenant_id, "Engineering", "The engineering team")
    }

    #[test]
    fn test_new_team() {
        let team = make_team();
        assert_eq!(team.name, "Engineering");
        assert_eq!(team.description, "The engineering team");
        assert!(team.members.is_empty());
        assert!(team.roles.is_empty());
    }

    #[test]
    fn test_add_member() {
        let mut team = make_team();
        let account = AccountId::new();

        assert!(team.add_member(account));
        assert_eq!(team.member_count(), 1);
        assert!(team.has_member(&account));
    }

    #[test]
    fn test_add_member_duplicate() {
        let mut team = make_team();
        let account = AccountId::new();

        assert!(team.add_member(account));
        assert!(!team.add_member(account));
        assert_eq!(team.member_count(), 1);
    }

    #[test]
    fn test_remove_member() {
        let mut team = make_team();
        let account = AccountId::new();

        team.add_member(account);
        assert!(team.remove_member(&account));
        assert!(!team.has_member(&account));
        assert_eq!(team.member_count(), 0);
    }

    #[test]
    fn test_remove_member_not_found() {
        let mut team = make_team();
        let account = AccountId::new();
        assert!(!team.remove_member(&account));
    }

    #[test]
    fn test_add_role() {
        let mut team = make_team();
        let role = RoleId::new();

        assert!(team.add_role(role));
        assert_eq!(team.role_count(), 1);
        assert!(team.has_role(&role));
    }

    #[test]
    fn test_add_role_duplicate() {
        let mut team = make_team();
        let role = RoleId::new();

        assert!(team.add_role(role));
        assert!(!team.add_role(role));
        assert_eq!(team.role_count(), 1);
    }

    #[test]
    fn test_remove_role() {
        let mut team = make_team();
        let role = RoleId::new();

        team.add_role(role);
        assert!(team.remove_role(&role));
        assert!(!team.has_role(&role));
        assert_eq!(team.role_count(), 0);
    }

    #[test]
    fn test_remove_role_not_found() {
        let mut team = make_team();
        let role = RoleId::new();
        assert!(!team.remove_role(&role));
    }

    #[test]
    fn test_multiple_members_and_roles() {
        let mut team = make_team();
        let a1 = AccountId::new();
        let a2 = AccountId::new();
        let a3 = AccountId::new();
        let r1 = RoleId::new();
        let r2 = RoleId::new();

        team.add_member(a1);
        team.add_member(a2);
        team.add_member(a3);
        team.add_role(r1);
        team.add_role(r2);

        assert_eq!(team.member_count(), 3);
        assert_eq!(team.role_count(), 2);
        assert!(team.has_member(&a1));
        assert!(team.has_member(&a2));
        assert!(team.has_member(&a3));
        assert!(team.has_role(&r1));
        assert!(team.has_role(&r2));

        team.remove_member(&a2);
        assert_eq!(team.member_count(), 2);
        assert!(!team.has_member(&a2));
    }

    #[test]
    fn test_with_id() {
        let id = TeamId::new();
        let tenant_id = TenantId::new();
        let team = Team::with_id(id, tenant_id, "Ops", "Operations team");
        assert_eq!(team.id, id);
        assert_eq!(team.tenant_id, tenant_id);
        assert_eq!(team.name, "Ops");
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut team = make_team();
        team.add_member(AccountId::new());
        team.add_role(RoleId::new());

        let json = serde_json::to_string(&team).expect("serialize failed");
        let deserialized: Team = serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(deserialized.id, team.id);
        assert_eq!(deserialized.name, team.name);
        assert_eq!(deserialized.member_count(), 1);
        assert_eq!(deserialized.role_count(), 1);
    }
}
