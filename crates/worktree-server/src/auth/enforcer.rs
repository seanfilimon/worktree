use worktree_protocol::core::id::AccountId;
use worktree_protocol::iam::permission::Permission;
use worktree_protocol::iam::scope::Scope;

/// Enforces permission checks for authenticated users.
///
/// The `PermissionEnforcer` is the central authority for access control
/// decisions in the Worktree server. It evaluates whether a given user
/// holds a specific permission within a particular scope by consulting
/// the loaded policy set.
///
/// In the future this will support pluggable policy backends (e.g.
/// in-memory, database-backed, or remote policy services). For now
/// it holds a simple in-memory list of grants.
pub struct PermissionEnforcer {
    /// The set of permission grants that have been loaded.
    grants: Vec<PermissionGrant>,
}

/// A single permission grant that binds a user to a permission within a scope.
#[derive(Debug, Clone)]
pub struct PermissionGrant {
    /// The user this grant applies to.
    pub user_id: AccountId,
    /// The permission being granted.
    pub permission: Permission,
    /// The scope within which the permission is valid.
    pub scope: Scope,
}

impl PermissionEnforcer {
    /// Create a new `PermissionEnforcer` with no grants loaded.
    pub fn new() -> Self {
        Self {
            grants: Vec::new(),
        }
    }

    /// Add a permission grant to the enforcer.
    pub fn add_grant(&mut self, grant: PermissionGrant) {
        self.grants.push(grant);
    }

    /// Add a permission grant for a specific user, permission, and scope.
    pub fn grant(
        &mut self,
        user_id: AccountId,
        permission: Permission,
        scope: Scope,
    ) {
        self.grants.push(PermissionGrant {
            user_id,
            permission,
            scope,
        });
    }

    /// Check whether the given user holds the specified permission within
    /// the given scope.
    ///
    /// The check succeeds if there exists at least one grant for the user
    /// whose permission matches and whose scope covers the requested scope.
    ///
    /// # Arguments
    ///
    /// * `user`       — The user to check permissions for.
    /// * `permission` — The permission being requested.
    /// * `scope`      — The scope within which the permission is needed.
    ///
    /// # Returns
    ///
    /// `true` if the user is authorized, `false` otherwise.
    pub fn check(
        &self,
        user: &AccountId,
        permission: &Permission,
        scope: &Scope,
    ) -> bool {
        self.grants.iter().any(|grant| {
            grant.user_id == *user
                && grant.permission == *permission
                && grant.scope.covers(scope)
        })
    }

    /// Revoke all grants for a specific user.
    ///
    /// Returns the number of grants that were removed.
    pub fn revoke_all(&mut self, user: &AccountId) -> usize {
        let before = self.grants.len();
        self.grants.retain(|g| g.user_id != *user);
        before - self.grants.len()
    }

    /// Return the total number of grants currently loaded.
    pub fn grant_count(&self) -> usize {
        self.grants.len()
    }
}

impl Default for PermissionEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::core::id::TreeId;

    #[test]
    fn check_returns_false_with_no_grants() {
        let enforcer = PermissionEnforcer::new();
        let user = AccountId::new();
        assert!(!enforcer.check(&user, &Permission::TreeRead, &Scope::Global));
    }

    #[test]
    fn check_returns_true_for_matching_grant() {
        let mut enforcer = PermissionEnforcer::new();
        let user = AccountId::new();

        enforcer.grant(user, Permission::TreeRead, Scope::Global);

        assert!(enforcer.check(&user, &Permission::TreeRead, &Scope::Global));
    }

    #[test]
    fn global_grant_covers_tree_scope() {
        let mut enforcer = PermissionEnforcer::new();
        let user = AccountId::new();
        let tree_id = TreeId::new();

        enforcer.grant(user, Permission::TreeWrite, Scope::Global);

        assert!(enforcer.check(
            &user,
            &Permission::TreeWrite,
            &Scope::Tree(tree_id),
        ));
    }

    #[test]
    fn tree_grant_does_not_cover_different_tree() {
        let mut enforcer = PermissionEnforcer::new();
        let user = AccountId::new();
        let tree_a = TreeId::new();
        let tree_b = TreeId::new();

        enforcer.grant(user, Permission::TreeRead, Scope::Tree(tree_a));

        assert!(!enforcer.check(
            &user,
            &Permission::TreeRead,
            &Scope::Tree(tree_b),
        ));
    }

    #[test]
    fn wrong_permission_is_denied() {
        let mut enforcer = PermissionEnforcer::new();
        let user = AccountId::new();

        enforcer.grant(user, Permission::TreeRead, Scope::Global);

        assert!(!enforcer.check(&user, &Permission::TenantAdmin, &Scope::Global));
    }

    #[test]
    fn revoke_all_removes_user_grants() {
        let mut enforcer = PermissionEnforcer::new();
        let user = AccountId::new();

        enforcer.grant(user, Permission::TreeRead, Scope::Global);
        enforcer.grant(user, Permission::TreeWrite, Scope::Global);

        let removed = enforcer.revoke_all(&user);
        assert_eq!(removed, 2);
        assert!(!enforcer.check(&user, &Permission::TreeRead, &Scope::Global));
        assert_eq!(enforcer.grant_count(), 0);
    }
}
