//! # Scope
//!
//! Hierarchical scope system for permission evaluation. Permissions are always
//! evaluated within a scope, and broader scopes cover narrower ones.
//!
//! Hierarchy: `Global` → `Tenant` → `Tree` → `Branch`

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::core::id::{BranchId, TenantId, TreeId};

/// A hierarchical scope within which permissions are evaluated.
///
/// The scope hierarchy from broadest to narrowest:
/// - `Global` — applies everywhere (reserved for GlobalAdmin)
/// - `Tenant(TenantId)` — applies to everything within a specific tenant
/// - `Tree(TenantId, TreeId)` — applies to a specific tree within a tenant
/// - `Branch(TenantId, TreeId, BranchId)` — applies to a specific branch within a tree
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// Applies everywhere. Only meaningful for GlobalAdmin.
    Global,

    /// Applies to everything within a specific tenant.
    Tenant(TenantId),

    /// Applies to a specific tree within a tenant.
    Tree(TenantId, TreeId),

    /// Applies to a specific branch within a specific tree.
    Branch(TenantId, TreeId, BranchId),
}

impl Scope {
    /// Returns `true` if `self` is equal to or broader than `other`.
    ///
    /// Coverage rules:
    /// - `Global` covers everything.
    /// - `Tenant(t)` covers `Tenant(t)`, `Tree(t, _)`, and `Branch(t, _, _)`.
    /// - `Tree(t, tr)` covers `Tree(t, tr)` and `Branch(t, tr, _)`.
    /// - `Branch(t, tr, b)` only covers exactly `Branch(t, tr, b)`.
    /// - Cross-tenant scopes never cover each other.
    pub fn covers(&self, other: &Scope) -> bool {
        match self {
            Scope::Global => true,
            Scope::Tenant(self_tenant) => match other {
                Scope::Global => false,
                Scope::Tenant(other_tenant) => self_tenant == other_tenant,
                Scope::Tree(other_tenant, _) => self_tenant == other_tenant,
                Scope::Branch(other_tenant, _, _) => self_tenant == other_tenant,
            },
            Scope::Tree(self_tenant, self_tree) => match other {
                Scope::Global | Scope::Tenant(_) => false,
                Scope::Tree(other_tenant, other_tree) => {
                    self_tenant == other_tenant && self_tree == other_tree
                }
                Scope::Branch(other_tenant, other_tree, _) => {
                    self_tenant == other_tenant && self_tree == other_tree
                }
            },
            Scope::Branch(self_tenant, self_tree, self_branch) => match other {
                Scope::Global | Scope::Tenant(_) | Scope::Tree(_, _) => false,
                Scope::Branch(other_tenant, other_tree, other_branch) => {
                    self_tenant == other_tenant
                        && self_tree == other_tree
                        && self_branch == other_branch
                }
            },
        }
    }

    /// Extract the tenant ID from any scope (except Global).
    pub fn tenant_id(&self) -> Option<&TenantId> {
        match self {
            Scope::Global => None,
            Scope::Tenant(t) => Some(t),
            Scope::Tree(t, _) => Some(t),
            Scope::Branch(t, _, _) => Some(t),
        }
    }

    /// Extract the tree ID if the scope is `Tree` or `Branch`.
    pub fn tree_id(&self) -> Option<&TreeId> {
        match self {
            Scope::Global | Scope::Tenant(_) => None,
            Scope::Tree(_, tr) => Some(tr),
            Scope::Branch(_, tr, _) => Some(tr),
        }
    }

    /// Extract the branch ID if the scope is `Branch`.
    pub fn branch_id(&self) -> Option<&BranchId> {
        match self {
            Scope::Branch(_, _, b) => Some(b),
            _ => None,
        }
    }

    /// Returns `true` if this is the `Global` scope.
    pub fn is_global(&self) -> bool {
        matches!(self, Scope::Global)
    }

    /// Returns `true` if this is a `Tenant` scope.
    pub fn is_tenant(&self) -> bool {
        matches!(self, Scope::Tenant(_))
    }

    /// Returns `true` if this is a `Tree` scope.
    pub fn is_tree(&self) -> bool {
        matches!(self, Scope::Tree(_, _))
    }

    /// Returns `true` if this is a `Branch` scope.
    pub fn is_branch(&self) -> bool {
        matches!(self, Scope::Branch(_, _, _))
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Global => write!(f, "global"),
            Scope::Tenant(t) => write!(f, "tenant:{}", t),
            Scope::Tree(t, tr) => write!(f, "tree:{}:{}", t, tr),
            Scope::Branch(t, tr, b) => write!(f, "branch:{}:{}:{}", t, tr, b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ids() -> (TenantId, TenantId, TreeId, TreeId, BranchId, BranchId) {
        (
            TenantId::new(),
            TenantId::new(),
            TreeId::new(),
            TreeId::new(),
            BranchId::new(),
            BranchId::new(),
        )
    }

    #[test]
    fn global_covers_everything() {
        let (t1, _, tr1, _, b1, _) = make_ids();
        let global = Scope::Global;

        assert!(global.covers(&Scope::Global));
        assert!(global.covers(&Scope::Tenant(t1)));
        assert!(global.covers(&Scope::Tree(t1, tr1)));
        assert!(global.covers(&Scope::Branch(t1, tr1, b1)));
    }

    #[test]
    fn tenant_covers_own_subtree() {
        let (t1, _, tr1, _, b1, _) = make_ids();
        let tenant_scope = Scope::Tenant(t1);

        assert!(tenant_scope.covers(&Scope::Tenant(t1)));
        assert!(tenant_scope.covers(&Scope::Tree(t1, tr1)));
        assert!(tenant_scope.covers(&Scope::Branch(t1, tr1, b1)));
    }

    #[test]
    fn tenant_does_not_cover_global() {
        let (t1, _, _, _, _, _) = make_ids();
        let tenant_scope = Scope::Tenant(t1);

        assert!(!tenant_scope.covers(&Scope::Global));
    }

    #[test]
    fn cross_tenant_does_not_cover() {
        let (t1, t2, tr1, _, b1, _) = make_ids();
        let tenant_scope = Scope::Tenant(t1);

        assert!(!tenant_scope.covers(&Scope::Tenant(t2)));
        assert!(!tenant_scope.covers(&Scope::Tree(t2, tr1)));
        assert!(!tenant_scope.covers(&Scope::Branch(t2, tr1, b1)));
    }

    #[test]
    fn tree_covers_own_branches() {
        let (t1, _, tr1, _, b1, b2) = make_ids();
        let tree_scope = Scope::Tree(t1, tr1);

        assert!(tree_scope.covers(&Scope::Tree(t1, tr1)));
        assert!(tree_scope.covers(&Scope::Branch(t1, tr1, b1)));
        assert!(tree_scope.covers(&Scope::Branch(t1, tr1, b2)));
    }

    #[test]
    fn tree_does_not_cover_broader_scopes() {
        let (t1, _, tr1, _, _, _) = make_ids();
        let tree_scope = Scope::Tree(t1, tr1);

        assert!(!tree_scope.covers(&Scope::Global));
        assert!(!tree_scope.covers(&Scope::Tenant(t1)));
    }

    #[test]
    fn tree_does_not_cover_other_trees() {
        let (t1, _, tr1, tr2, b1, _) = make_ids();
        let tree_scope = Scope::Tree(t1, tr1);

        assert!(!tree_scope.covers(&Scope::Tree(t1, tr2)));
        assert!(!tree_scope.covers(&Scope::Branch(t1, tr2, b1)));
    }

    #[test]
    fn branch_covers_only_itself() {
        let (t1, _, tr1, _, b1, b2) = make_ids();
        let branch_scope = Scope::Branch(t1, tr1, b1);

        assert!(branch_scope.covers(&Scope::Branch(t1, tr1, b1)));
        assert!(!branch_scope.covers(&Scope::Branch(t1, tr1, b2)));
        assert!(!branch_scope.covers(&Scope::Tree(t1, tr1)));
        assert!(!branch_scope.covers(&Scope::Tenant(t1)));
        assert!(!branch_scope.covers(&Scope::Global));
    }

    #[test]
    fn cross_tenant_tree_does_not_cover() {
        let (t1, t2, tr1, _, b1, _) = make_ids();
        let tree_scope = Scope::Tree(t1, tr1);

        assert!(!tree_scope.covers(&Scope::Tree(t2, tr1)));
        assert!(!tree_scope.covers(&Scope::Branch(t2, tr1, b1)));
    }

    #[test]
    fn cross_tenant_branch_does_not_cover() {
        let (t1, t2, tr1, _, b1, _) = make_ids();
        let branch_scope = Scope::Branch(t1, tr1, b1);

        assert!(!branch_scope.covers(&Scope::Branch(t2, tr1, b1)));
    }

    #[test]
    fn tenant_id_extraction() {
        let (t1, _, tr1, _, b1, _) = make_ids();

        assert_eq!(Scope::Global.tenant_id(), None);
        assert_eq!(Scope::Tenant(t1).tenant_id(), Some(&t1));
        assert_eq!(Scope::Tree(t1, tr1).tenant_id(), Some(&t1));
        assert_eq!(Scope::Branch(t1, tr1, b1).tenant_id(), Some(&t1));
    }

    #[test]
    fn tree_id_extraction() {
        let (t1, _, tr1, _, b1, _) = make_ids();

        assert_eq!(Scope::Global.tree_id(), None);
        assert_eq!(Scope::Tenant(t1).tree_id(), None);
        assert_eq!(Scope::Tree(t1, tr1).tree_id(), Some(&tr1));
        assert_eq!(Scope::Branch(t1, tr1, b1).tree_id(), Some(&tr1));
    }

    #[test]
    fn branch_id_extraction() {
        let (t1, _, tr1, _, b1, _) = make_ids();

        assert_eq!(Scope::Global.branch_id(), None);
        assert_eq!(Scope::Tenant(t1).branch_id(), None);
        assert_eq!(Scope::Tree(t1, tr1).branch_id(), None);
        assert_eq!(Scope::Branch(t1, tr1, b1).branch_id(), Some(&b1));
    }

    #[test]
    fn type_predicates() {
        let (t1, _, tr1, _, b1, _) = make_ids();

        assert!(Scope::Global.is_global());
        assert!(!Scope::Global.is_tenant());

        assert!(Scope::Tenant(t1).is_tenant());
        assert!(!Scope::Tenant(t1).is_tree());

        assert!(Scope::Tree(t1, tr1).is_tree());
        assert!(!Scope::Tree(t1, tr1).is_branch());

        assert!(Scope::Branch(t1, tr1, b1).is_branch());
        assert!(!Scope::Branch(t1, tr1, b1).is_global());
    }

    #[test]
    fn display_global() {
        assert_eq!(Scope::Global.to_string(), "global");
    }

    #[test]
    fn display_tenant() {
        let t = TenantId::new();
        let scope = Scope::Tenant(t);
        assert_eq!(scope.to_string(), format!("tenant:{}", t));
    }

    #[test]
    fn display_tree() {
        let t = TenantId::new();
        let tr = TreeId::new();
        let scope = Scope::Tree(t, tr);
        assert_eq!(scope.to_string(), format!("tree:{}:{}", t, tr));
    }

    #[test]
    fn display_branch() {
        let t = TenantId::new();
        let tr = TreeId::new();
        let b = BranchId::new();
        let scope = Scope::Branch(t, tr, b);
        assert_eq!(scope.to_string(), format!("branch:{}:{}:{}", t, tr, b));
    }

    #[test]
    fn serde_roundtrip() {
        let (t1, _, tr1, _, b1, _) = make_ids();
        let scopes = vec![
            Scope::Global,
            Scope::Tenant(t1),
            Scope::Tree(t1, tr1),
            Scope::Branch(t1, tr1, b1),
        ];

        for scope in &scopes {
            let json = serde_json::to_string(scope).expect("serialize");
            let deserialized: Scope = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(scope, &deserialized);
        }
    }

    #[test]
    fn scope_equality() {
        let t = TenantId::new();
        let tr = TreeId::new();
        let b = BranchId::new();

        assert_eq!(Scope::Global, Scope::Global);
        assert_eq!(Scope::Tenant(t), Scope::Tenant(t));
        assert_eq!(Scope::Tree(t, tr), Scope::Tree(t, tr));
        assert_eq!(Scope::Branch(t, tr, b), Scope::Branch(t, tr, b));

        let t2 = TenantId::new();
        assert_ne!(Scope::Tenant(t), Scope::Tenant(t2));
    }

    #[test]
    fn scope_clone() {
        let t = TenantId::new();
        let tr = TreeId::new();
        let b = BranchId::new();
        let scope = Scope::Branch(t, tr, b);
        let cloned = scope.clone();
        assert_eq!(scope, cloned);
    }

    #[test]
    fn scope_hash() {
        use std::collections::HashSet;

        let t = TenantId::new();
        let tr = TreeId::new();
        let mut set = HashSet::new();
        set.insert(Scope::Tree(t, tr));
        assert!(set.contains(&Scope::Tree(t, tr)));
        assert!(!set.contains(&Scope::Tenant(t)));
    }
}
