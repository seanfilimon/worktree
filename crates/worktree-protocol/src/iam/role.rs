//! # RBAC Role Definitions
//!
//! A role is a named set of permissions that can be assigned to accounts or teams.
//! Roles support both built-in (immutable) and custom (tenant-defined) variants.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::id::{RoleId, TenantId};
use crate::iam::permission::Permission;

/// Whether a role is built-in (cannot be deleted) or custom (tenant-defined).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleKind {
    /// Built-in roles are created by the system and cannot be deleted.
    BuiltIn,
    /// Custom roles are created by tenant administrators.
    Custom,
}

/// An RBAC role — a named collection of permissions.
///
/// Roles are scoped to a tenant and can be assigned to accounts or teams.
/// Built-in roles provide sensible defaults; custom roles allow fine-grained control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique identifier for this role.
    pub id: RoleId,
    /// The tenant this role belongs to.
    pub tenant_id: TenantId,
    /// Human-readable name (e.g., "Owner", "Developer", "Viewer").
    pub name: String,
    /// Description of what this role grants.
    pub description: String,
    /// Whether this role is built-in or custom.
    pub kind: RoleKind,
    /// The set of permissions this role grants.
    pub permissions: HashSet<Permission>,
    /// When this role was created.
    pub created_at: DateTime<Utc>,
}

impl Role {
    /// Create a new custom role with the given name and no permissions.
    pub fn new(tenant_id: TenantId, name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: RoleId::new(),
            tenant_id,
            name: name.into(),
            description: description.into(),
            kind: RoleKind::Custom,
            permissions: HashSet::new(),
            created_at: Utc::now(),
        }
    }

    /// Grant a permission to this role. Returns `true` if it was newly inserted.
    pub fn grant(&mut self, permission: Permission) -> bool {
        self.permissions.insert(permission)
    }

    /// Revoke a permission from this role. Returns `true` if it was present.
    pub fn revoke(&mut self, permission: Permission) -> bool {
        self.permissions.remove(&permission)
    }

    /// Check whether this role includes a specific permission.
    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.permissions.contains(permission)
    }

    /// Returns `true` if this is a built-in role.
    pub fn is_builtin(&self) -> bool {
        self.kind == RoleKind::BuiltIn
    }

    // -----------------------------------------------------------------------
    // Built-in role constructors
    // -----------------------------------------------------------------------

    /// **Owner** — full tenant admin plus every permission in the system.
    ///
    /// This is the highest-privilege built-in role within a tenant.
    pub fn owner(tenant_id: TenantId) -> Self {
        let permissions: HashSet<Permission> = Permission::all().iter().copied().collect();
        Self {
            id: RoleId::new(),
            tenant_id,
            name: "Owner".into(),
            description: "Full tenant administrator with all permissions.".into(),
            kind: RoleKind::BuiltIn,
            permissions,
            created_at: Utc::now(),
        }
    }

    /// **Admin** — tree, branch, account, and team management.
    ///
    /// Admins can manage organizational resources but do not have `TenantAdmin`
    /// or `GlobalAdmin`.
    pub fn admin(tenant_id: TenantId) -> Self {
        let perms = vec![
            // Tree
            Permission::TreeRead,
            Permission::TreeWrite,
            Permission::TreeCreate,
            Permission::TreeDelete,
            Permission::TreeAdmin,
            // Branch
            Permission::BranchRead,
            Permission::BranchCreate,
            Permission::BranchDelete,
            Permission::BranchMerge,
            Permission::BranchProtect,
            // Snapshot
            Permission::SnapshotCreate,
            Permission::SnapshotRead,
            // Sync
            Permission::SyncPush,
            Permission::SyncPull,
            // Management
            Permission::AccountManage,
            Permission::TeamManage,
            Permission::RoleManage,
            Permission::PolicyManage,
        ];
        Self {
            id: RoleId::new(),
            tenant_id,
            name: "Admin".into(),
            description: "Organizational administrator — manages trees, branches, accounts, and teams.".into(),
            kind: RoleKind::BuiltIn,
            permissions: perms.into_iter().collect(),
            created_at: Utc::now(),
        }
    }

    /// **Maintainer** — tree write, branch lifecycle, and snapshot creation.
    ///
    /// Maintainers can modify trees, manage branches (create/merge/delete),
    /// and create snapshots, but cannot manage accounts or teams.
    pub fn maintainer(tenant_id: TenantId) -> Self {
        let perms = vec![
            Permission::TreeRead,
            Permission::TreeWrite,
            Permission::BranchRead,
            Permission::BranchCreate,
            Permission::BranchDelete,
            Permission::BranchMerge,
            Permission::SnapshotCreate,
            Permission::SnapshotRead,
            Permission::SyncPush,
            Permission::SyncPull,
        ];
        Self {
            id: RoleId::new(),
            tenant_id,
            name: "Maintainer".into(),
            description: "Can write to trees, manage branches, and create snapshots.".into(),
            kind: RoleKind::BuiltIn,
            permissions: perms.into_iter().collect(),
            created_at: Utc::now(),
        }
    }

    /// **Developer** — tree read/write, branch create, snapshot create, sync.
    ///
    /// Developers can contribute code but cannot delete branches or manage
    /// organizational resources.
    pub fn developer(tenant_id: TenantId) -> Self {
        let perms = vec![
            Permission::TreeRead,
            Permission::TreeWrite,
            Permission::BranchRead,
            Permission::BranchCreate,
            Permission::SnapshotCreate,
            Permission::SnapshotRead,
            Permission::SyncPush,
            Permission::SyncPull,
        ];
        Self {
            id: RoleId::new(),
            tenant_id,
            name: "Developer".into(),
            description: "Can read/write trees, create branches, create snapshots, and sync.".into(),
            kind: RoleKind::BuiltIn,
            permissions: perms.into_iter().collect(),
            created_at: Utc::now(),
        }
    }

    /// **Viewer** — read-only access to trees, branches, and snapshots.
    pub fn viewer(tenant_id: TenantId) -> Self {
        let perms = vec![
            Permission::TreeRead,
            Permission::BranchRead,
            Permission::SnapshotRead,
        ];
        Self {
            id: RoleId::new(),
            tenant_id,
            name: "Viewer".into(),
            description: "Read-only access to trees, branches, and snapshots.".into(),
            kind: RoleKind::BuiltIn,
            permissions: perms.into_iter().collect(),
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant() -> TenantId {
        TenantId::new()
    }

    // ------------------------------------------------------------------
    // Custom role basics
    // ------------------------------------------------------------------

    #[test]
    fn custom_role_starts_empty() {
        let role = Role::new(tenant(), "TestRole", "A test role");
        assert_eq!(role.name, "TestRole");
        assert_eq!(role.kind, RoleKind::Custom);
        assert!(role.permissions.is_empty());
        assert!(!role.is_builtin());
    }

    #[test]
    fn grant_and_revoke() {
        let mut role = Role::new(tenant(), "R", "");
        assert!(role.grant(Permission::TreeRead));
        assert!(role.has_permission(&Permission::TreeRead));
        // Granting again returns false (already present).
        assert!(!role.grant(Permission::TreeRead));

        assert!(role.revoke(Permission::TreeRead));
        assert!(!role.has_permission(&Permission::TreeRead));
        // Revoking again returns false (not present).
        assert!(!role.revoke(Permission::TreeRead));
    }

    // ------------------------------------------------------------------
    // Built-in role: Owner
    // ------------------------------------------------------------------

    #[test]
    fn owner_has_all_permissions() {
        let role = Role::owner(tenant());
        assert!(role.is_builtin());
        assert_eq!(role.name, "Owner");
        for perm in Permission::all() {
            assert!(
                role.has_permission(perm),
                "Owner missing permission: {:?}",
                perm
            );
        }
    }

    #[test]
    fn owner_includes_global_admin() {
        let role = Role::owner(tenant());
        assert!(role.has_permission(&Permission::GlobalAdmin));
        assert!(role.has_permission(&Permission::TenantAdmin));
    }

    // ------------------------------------------------------------------
    // Built-in role: Admin
    // ------------------------------------------------------------------

    #[test]
    fn admin_has_management_permissions() {
        let role = Role::admin(tenant());
        assert!(role.is_builtin());
        assert!(role.has_permission(&Permission::AccountManage));
        assert!(role.has_permission(&Permission::TeamManage));
        assert!(role.has_permission(&Permission::RoleManage));
        assert!(role.has_permission(&Permission::PolicyManage));
    }

    #[test]
    fn admin_does_not_have_global_or_tenant_admin() {
        let role = Role::admin(tenant());
        assert!(!role.has_permission(&Permission::TenantAdmin));
        assert!(!role.has_permission(&Permission::GlobalAdmin));
    }

    #[test]
    fn admin_has_tree_and_branch_permissions() {
        let role = Role::admin(tenant());
        assert!(role.has_permission(&Permission::TreeRead));
        assert!(role.has_permission(&Permission::TreeWrite));
        assert!(role.has_permission(&Permission::TreeCreate));
        assert!(role.has_permission(&Permission::TreeDelete));
        assert!(role.has_permission(&Permission::TreeAdmin));
        assert!(role.has_permission(&Permission::BranchRead));
        assert!(role.has_permission(&Permission::BranchCreate));
        assert!(role.has_permission(&Permission::BranchDelete));
        assert!(role.has_permission(&Permission::BranchMerge));
        assert!(role.has_permission(&Permission::BranchProtect));
    }

    // ------------------------------------------------------------------
    // Built-in role: Maintainer
    // ------------------------------------------------------------------

    #[test]
    fn maintainer_permissions() {
        let role = Role::maintainer(tenant());
        assert!(role.is_builtin());
        assert!(role.has_permission(&Permission::TreeRead));
        assert!(role.has_permission(&Permission::TreeWrite));
        assert!(role.has_permission(&Permission::BranchRead));
        assert!(role.has_permission(&Permission::BranchCreate));
        assert!(role.has_permission(&Permission::BranchDelete));
        assert!(role.has_permission(&Permission::BranchMerge));
        assert!(role.has_permission(&Permission::SnapshotCreate));
        assert!(role.has_permission(&Permission::SnapshotRead));
        assert!(role.has_permission(&Permission::SyncPush));
        assert!(role.has_permission(&Permission::SyncPull));
    }

    #[test]
    fn maintainer_does_not_have_admin_permissions() {
        let role = Role::maintainer(tenant());
        assert!(!role.has_permission(&Permission::AccountManage));
        assert!(!role.has_permission(&Permission::TeamManage));
        assert!(!role.has_permission(&Permission::RoleManage));
        assert!(!role.has_permission(&Permission::PolicyManage));
        assert!(!role.has_permission(&Permission::TenantAdmin));
        assert!(!role.has_permission(&Permission::GlobalAdmin));
        assert!(!role.has_permission(&Permission::TreeCreate));
        assert!(!role.has_permission(&Permission::TreeDelete));
        assert!(!role.has_permission(&Permission::TreeAdmin));
        assert!(!role.has_permission(&Permission::BranchProtect));
    }

    // ------------------------------------------------------------------
    // Built-in role: Developer
    // ------------------------------------------------------------------

    #[test]
    fn developer_permissions() {
        let role = Role::developer(tenant());
        assert!(role.is_builtin());
        assert!(role.has_permission(&Permission::TreeRead));
        assert!(role.has_permission(&Permission::TreeWrite));
        assert!(role.has_permission(&Permission::BranchRead));
        assert!(role.has_permission(&Permission::BranchCreate));
        assert!(role.has_permission(&Permission::SnapshotCreate));
        assert!(role.has_permission(&Permission::SnapshotRead));
        assert!(role.has_permission(&Permission::SyncPush));
        assert!(role.has_permission(&Permission::SyncPull));
    }

    #[test]
    fn developer_cannot_delete_branches() {
        let role = Role::developer(tenant());
        assert!(!role.has_permission(&Permission::BranchDelete));
        assert!(!role.has_permission(&Permission::BranchMerge));
    }

    // ------------------------------------------------------------------
    // Built-in role: Viewer
    // ------------------------------------------------------------------

    #[test]
    fn viewer_read_only() {
        let role = Role::viewer(tenant());
        assert!(role.is_builtin());
        assert!(role.has_permission(&Permission::TreeRead));
        assert!(role.has_permission(&Permission::BranchRead));
        assert!(role.has_permission(&Permission::SnapshotRead));
        assert_eq!(role.permissions.len(), 3);
    }

    #[test]
    fn viewer_cannot_write() {
        let role = Role::viewer(tenant());
        assert!(!role.has_permission(&Permission::TreeWrite));
        assert!(!role.has_permission(&Permission::TreeCreate));
        assert!(!role.has_permission(&Permission::BranchCreate));
        assert!(!role.has_permission(&Permission::SnapshotCreate));
        assert!(!role.has_permission(&Permission::SyncPush));
    }

    // ------------------------------------------------------------------
    // Hierarchy: owner ⊃ admin ⊃ maintainer ⊃ developer ⊃ viewer
    // ------------------------------------------------------------------

    #[test]
    fn role_hierarchy_is_superset_chain() {
        let t = tenant();
        let owner = Role::owner(t);
        let admin = Role::admin(t);
        let maintainer = Role::maintainer(t);
        let developer = Role::developer(t);
        let viewer = Role::viewer(t);

        assert!(owner.permissions.is_superset(&admin.permissions));
        assert!(admin.permissions.is_superset(&maintainer.permissions));
        assert!(maintainer.permissions.is_superset(&developer.permissions));
        assert!(developer.permissions.is_superset(&viewer.permissions));
    }

    // ------------------------------------------------------------------
    // Serde round-trip
    // ------------------------------------------------------------------

    #[test]
    fn role_serde_roundtrip() {
        let mut role = Role::owner(tenant());
        role.grant(Permission::TreeRead);
        let json = serde_json::to_string(&role).expect("serialize");
        let deser: Role = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(role.id, deser.id);
        assert_eq!(role.name, deser.name);
        assert_eq!(role.permissions, deser.permissions);
    }
}
