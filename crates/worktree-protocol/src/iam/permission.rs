//! Atomic permission definitions.
//!
//! Permissions are the building blocks of both RBAC roles and ABAC policies.
//! Each permission represents a single, indivisible action that can be
//! allowed or denied within a given scope.

use serde::{Deserialize, Serialize};
use std::fmt;

/// An atomic permission representing a single action in the system.
///
/// Permissions are value types — they carry no identity of their own.
/// They are collected into sets within [`super::role::Role`] definitions
/// and [`super::policy::Policy`] grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // ── Tree permissions ────────────────────────────────────────────
    #[serde(rename = "tree:read")]
    TreeRead,
    #[serde(rename = "tree:write")]
    TreeWrite,
    #[serde(rename = "tree:create")]
    TreeCreate,
    #[serde(rename = "tree:delete")]
    TreeDelete,
    #[serde(rename = "tree:admin")]
    TreeAdmin,

    // ── Branch permissions ──────────────────────────────────────────
    #[serde(rename = "branch:read")]
    BranchRead,
    #[serde(rename = "branch:create")]
    BranchCreate,
    #[serde(rename = "branch:delete")]
    BranchDelete,
    #[serde(rename = "branch:merge")]
    BranchMerge,
    #[serde(rename = "branch:protect")]
    BranchProtect,

    // ── Snapshot permissions ────────────────────────────────────────
    #[serde(rename = "snapshot:create")]
    SnapshotCreate,
    #[serde(rename = "snapshot:read")]
    SnapshotRead,
    #[serde(rename = "snapshot:revert")]
    SnapshotRevert,
    #[serde(rename = "snapshot:sign")]
    SnapshotSign,

    // ── Sync permissions ────────────────────────────────────────────
    #[serde(rename = "sync:push")]
    SyncPush,
    #[serde(rename = "sync:pull")]
    SyncPull,
    #[serde(rename = "sync:force_push")]
    SyncForcePush,

    // ── Staged visibility permissions ───────────────────────────────
    #[serde(rename = "staged:create")]
    StagedCreate,
    #[serde(rename = "staged:list")]
    StagedList,

    // ── Management permissions ──────────────────────────────────────
    #[serde(rename = "manage:roles")]
    ManageRoles,
    #[serde(rename = "manage:teams")]
    ManageTeams,
    #[serde(rename = "manage:policies")]
    ManagePolicies,
    #[serde(rename = "account:manage")]
    AccountManage,

    // ── Administrative permissions ──────────────────────────────────
    #[serde(rename = "admin:tenant")]
    AdminTenant,
    #[serde(rename = "admin:audit_read")]
    AdminAuditRead,
    #[serde(rename = "admin:bypass_protection")]
    AdminBypassProtection,
    #[serde(rename = "global:admin")]
    GlobalAdmin,

    // ── Tags & Releases permissions ──────────────────────────────────
    #[serde(rename = "tag:create")]
    TagCreate,
    #[serde(rename = "tag:delete")]
    TagDelete,
    #[serde(rename = "release:create")]
    ReleaseCreate,
    #[serde(rename = "release:delete")]
    ReleaseDelete,

    // ── Merge Requests permissions ──────────────────────────────────
    #[serde(rename = "mr:create")]
    MrCreate,
    #[serde(rename = "mr:review")]
    MrReview,
    #[serde(rename = "mr:merge")]
    MrMerge,
}

/// Complete list of all permission variants, in declaration order.
static ALL_PERMISSIONS: &[Permission] = &[
    Permission::TreeRead,
    Permission::TreeWrite,
    Permission::TreeCreate,
    Permission::TreeDelete,
    Permission::TreeAdmin,
    Permission::BranchRead,
    Permission::BranchCreate,
    Permission::BranchDelete,
    Permission::BranchMerge,
    Permission::BranchProtect,
    Permission::SnapshotCreate,
    Permission::SnapshotRead,
    Permission::SnapshotRevert,
    Permission::SnapshotSign,
    Permission::SyncPush,
    Permission::SyncPull,
    Permission::SyncForcePush,
    Permission::StagedCreate,
    Permission::StagedList,
    Permission::ManageRoles,
    Permission::ManageTeams,
    Permission::ManagePolicies,
    Permission::AccountManage,
    Permission::AdminTenant,
    Permission::AdminAuditRead,
    Permission::AdminBypassProtection,
    Permission::GlobalAdmin,
    Permission::TagCreate,
    Permission::TagDelete,
    Permission::ReleaseCreate,
    Permission::ReleaseDelete,
    Permission::MrCreate,
    Permission::MrReview,
    Permission::MrMerge,
];

impl Permission {
    /// Returns a slice of every permission variant.
    pub fn all() -> &'static [Permission] {
        ALL_PERMISSIONS
    }

    /// Returns the stable string representation of this permission.
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::TreeRead => "tree:read",
            Permission::TreeWrite => "tree:write",
            Permission::TreeCreate => "tree:create",
            Permission::TreeDelete => "tree:delete",
            Permission::TreeAdmin => "tree:admin",
            Permission::BranchRead => "branch:read",
            Permission::BranchCreate => "branch:create",
            Permission::BranchDelete => "branch:delete",
            Permission::BranchMerge => "branch:merge",
            Permission::BranchProtect => "branch:protect",
            Permission::SnapshotCreate => "snapshot:create",
            Permission::SnapshotRead => "snapshot:read",
            Permission::SnapshotRevert => "snapshot:revert",
            Permission::SnapshotSign => "snapshot:sign",
            Permission::SyncPush => "sync:push",
            Permission::SyncPull => "sync:pull",
            Permission::SyncForcePush => "sync:force_push",
            Permission::StagedCreate => "staged:create",
            Permission::StagedList => "staged:list",
            Permission::ManageRoles => "manage:roles",
            Permission::ManageTeams => "manage:teams",
            Permission::ManagePolicies => "manage:policies",
            Permission::AccountManage => "account:manage",
            Permission::AdminTenant => "admin:tenant",
            Permission::AdminAuditRead => "admin:audit_read",
            Permission::AdminBypassProtection => "admin:bypass_protection",
            Permission::GlobalAdmin => "global:admin",
            Permission::TagCreate => "tag:create",
            Permission::TagDelete => "tag:delete",
            Permission::ReleaseCreate => "release:create",
            Permission::ReleaseDelete => "release:delete",
            Permission::MrCreate => "mr:create",
            Permission::MrReview => "mr:review",
            Permission::MrMerge => "mr:merge",
        }
    }

    /// Returns only tree-related permissions.
    pub fn tree_permissions() -> Vec<Permission> {
        vec![
            Permission::TreeRead,
            Permission::TreeWrite,
            Permission::TreeCreate,
            Permission::TreeDelete,
            Permission::TreeAdmin,
        ]
    }

    /// Returns only branch-related permissions.
    pub fn branch_permissions() -> Vec<Permission> {
        vec![
            Permission::BranchRead,
            Permission::BranchCreate,
            Permission::BranchDelete,
            Permission::BranchMerge,
            Permission::BranchProtect,
        ]
    }

    /// Returns only administrative / management permissions.
    pub fn admin_permissions() -> Vec<Permission> {
        vec![
            Permission::ManageRoles,
            Permission::ManageTeams,
            Permission::ManagePolicies,
            Permission::AdminTenant,
            Permission::AdminAuditRead,
            Permission::AdminBypassProtection,
            Permission::GlobalAdmin,
        ]
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_returns_every_variant() {
        assert_eq!(Permission::all().len(), 34);
    }

    #[test]
    fn all_variants_have_unique_strings() {
        let strings: HashSet<&str> = Permission::all().iter().map(|p| p.as_str()).collect();
        assert_eq!(strings.len(), Permission::all().len());
    }

    #[test]
    fn display_matches_as_str() {
        for p in Permission::all() {
            assert_eq!(p.to_string(), p.as_str());
        }
    }

    #[test]
    fn tree_permissions_are_tree_related() {
        for p in Permission::tree_permissions() {
            assert!(
                p.as_str().starts_with("tree:"),
                "{} should start with tree:",
                p
            );
        }
        assert_eq!(Permission::tree_permissions().len(), 5);
    }

    #[test]
    fn branch_permissions_are_branch_related() {
        for p in Permission::branch_permissions() {
            assert!(
                p.as_str().starts_with("branch:"),
                "{} should start with branch:",
                p
            );
        }
        assert_eq!(Permission::branch_permissions().len(), 5);
    }

    #[test]
    fn admin_permissions_list() {
        let admins = Permission::admin_permissions();
        assert!(admins.contains(&Permission::ManageRoles));
        assert!(admins.contains(&Permission::ManageTeams));
        assert!(admins.contains(&Permission::ManagePolicies));
        assert!(admins.contains(&Permission::AdminTenant));
        assert!(admins.contains(&Permission::AdminAuditRead));
        assert!(admins.contains(&Permission::AdminBypassProtection));
        assert!(admins.contains(&Permission::GlobalAdmin));
        assert_eq!(admins.len(), 7);
    }

    #[test]
    fn permission_is_copy() {
        let p = Permission::TreeRead;
        let q = p; // Copy
        assert_eq!(p, q);
    }

    #[test]
    fn permission_hash_set() {
        let mut set = HashSet::new();
        set.insert(Permission::TreeRead);
        set.insert(Permission::TreeRead);
        set.insert(Permission::TreeWrite);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn serde_roundtrip_json() {
        let perm = Permission::BranchMerge;
        let json = serde_json::to_string(&perm).expect("serialize");
        let back: Permission = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(perm, back);
    }

    #[test]
    fn serde_all_permissions_roundtrip() {
        for p in Permission::all() {
            let json = serde_json::to_string(p).expect("serialize");
            let back: Permission = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*p, back);
        }
    }
}
