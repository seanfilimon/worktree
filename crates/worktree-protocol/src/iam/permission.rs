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
    /// Read tree contents, history, and manifests.
    TreeRead,
    /// Modify files within a tree.
    TreeWrite,
    /// Create new trees.
    TreeCreate,
    /// Delete trees.
    TreeDelete,
    /// Full administrative control over a tree (implies all tree-level ops).
    TreeAdmin,

    // ── Branch permissions ──────────────────────────────────────────
    /// View branches and their metadata.
    BranchRead,
    /// Create new branches.
    BranchCreate,
    /// Delete branches.
    BranchDelete,
    /// Merge branches.
    BranchMerge,
    /// Configure branch protection rules.
    BranchProtect,

    // ── Snapshot permissions ────────────────────────────────────────
    /// Create snapshots (commits).
    SnapshotCreate,
    /// View snapshot history.
    SnapshotRead,

    // ── Sync permissions ────────────────────────────────────────────
    /// Push to remotes.
    SyncPush,
    /// Pull from remotes.
    SyncPull,

    // ── Management permissions ──────────────────────────────────────
    /// Create, update, or deactivate accounts within the tenant.
    AccountManage,
    /// Create, update, or delete teams.
    TeamManage,
    /// Create, update, or delete roles.
    RoleManage,
    /// Create, update, or delete policies.
    PolicyManage,

    // ── Administrative permissions ──────────────────────────────────
    /// Full admin over the tenant (implies everything within that tenant).
    TenantAdmin,
    /// Superadmin — implies everything everywhere.
    GlobalAdmin,
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
    Permission::SyncPush,
    Permission::SyncPull,
    Permission::AccountManage,
    Permission::TeamManage,
    Permission::RoleManage,
    Permission::PolicyManage,
    Permission::TenantAdmin,
    Permission::GlobalAdmin,
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
            Permission::SyncPush => "sync:push",
            Permission::SyncPull => "sync:pull",
            Permission::AccountManage => "account:manage",
            Permission::TeamManage => "team:manage",
            Permission::RoleManage => "role:manage",
            Permission::PolicyManage => "policy:manage",
            Permission::TenantAdmin => "tenant:admin",
            Permission::GlobalAdmin => "global:admin",
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
            Permission::AccountManage,
            Permission::TeamManage,
            Permission::RoleManage,
            Permission::PolicyManage,
            Permission::TenantAdmin,
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
        assert_eq!(Permission::all().len(), 20);
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
        assert!(admins.contains(&Permission::TenantAdmin));
        assert!(admins.contains(&Permission::GlobalAdmin));
        assert!(admins.contains(&Permission::AccountManage));
        assert!(admins.contains(&Permission::TeamManage));
        assert!(admins.contains(&Permission::RoleManage));
        assert!(admins.contains(&Permission::PolicyManage));
        assert_eq!(admins.len(), 6);
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
