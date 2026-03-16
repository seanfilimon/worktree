//! Resource definitions for access control targeting.
//!
//! Defines the [`Resource`] enum that represents which version-control
//! resources can have access control entries applied to them.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::core::id::{BranchId, TenantId, TreeId};
use crate::iam::scope::Scope;

/// A resource that can have access control rules applied to it.
///
/// Resources are the *objects* in the access-control model — the things
/// that subjects (accounts, teams, roles) are granted or denied access to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Resource {
    /// An entire worktree.
    Tree {
        /// The tenant that owns the tree.
        tenant_id: TenantId,
        /// The tree being targeted.
        tree_id: TreeId,
    },
    /// A specific branch within a tree.
    Branch {
        /// The tenant that owns the tree.
        tenant_id: TenantId,
        /// The tree the branch belongs to.
        tree_id: TreeId,
        /// The branch being targeted.
        branch_id: BranchId,
    },
    /// A subtree (path prefix) within a tree, for path-level access control.
    Subtree {
        /// The tenant that owns the tree.
        tenant_id: TenantId,
        /// The tree the subtree belongs to.
        tree_id: TreeId,
        /// The path prefix that defines the subtree scope.
        path_prefix: String,
    },
}

impl Resource {
    /// Returns the tenant ID for this resource.
    pub fn tenant_id(&self) -> &TenantId {
        match self {
            Resource::Tree { tenant_id, .. } => tenant_id,
            Resource::Branch { tenant_id, .. } => tenant_id,
            Resource::Subtree { tenant_id, .. } => tenant_id,
        }
    }

    /// Returns the tree ID for this resource.
    pub fn tree_id(&self) -> &TreeId {
        match self {
            Resource::Tree { tree_id, .. } => tree_id,
            Resource::Branch { tree_id, .. } => tree_id,
            Resource::Subtree { tree_id, .. } => tree_id,
        }
    }

    /// Returns the branch ID if this resource is a [`Resource::Branch`].
    pub fn branch_id(&self) -> Option<&BranchId> {
        match self {
            Resource::Branch { branch_id, .. } => Some(branch_id),
            _ => None,
        }
    }

    /// Returns the path prefix if this resource is a [`Resource::Subtree`].
    pub fn path_prefix(&self) -> Option<&str> {
        match self {
            Resource::Subtree { path_prefix, .. } => Some(path_prefix.as_str()),
            _ => None,
        }
    }

    /// Convert this resource into the corresponding IAM [`Scope`].
    ///
    /// - `Resource::Tree` maps to `Scope::Tree`
    /// - `Resource::Branch` maps to `Scope::Branch`
    /// - `Resource::Subtree` maps to `Scope::Tree` (subtree scoping is
    ///   finer-grained than the scope hierarchy supports, so it falls back
    ///   to the enclosing tree scope)
    pub fn to_scope(&self) -> Scope {
        match self {
            Resource::Tree {
                tenant_id,
                tree_id,
            } => Scope::Tree(*tenant_id, *tree_id),
            Resource::Branch {
                tenant_id,
                tree_id,
                branch_id,
            } => Scope::Branch(*tenant_id, *tree_id, *branch_id),
            Resource::Subtree {
                tenant_id,
                tree_id,
                ..
            } => Scope::Tree(*tenant_id, *tree_id),
        }
    }
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Resource::Tree {
                tenant_id,
                tree_id,
            } => write!(f, "tree:{}:{}", tenant_id, tree_id),
            Resource::Branch {
                tenant_id,
                tree_id,
                branch_id,
            } => write!(f, "branch:{}:{}:{}", tenant_id, tree_id, branch_id),
            Resource::Subtree {
                tenant_id,
                tree_id,
                path_prefix,
            } => write!(f, "subtree:{}:{}:{}", tenant_id, tree_id, path_prefix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_resource_accessors() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };

        assert_eq!(*resource.tenant_id(), tid);
        assert_eq!(*resource.tree_id(), trid);
        assert_eq!(resource.branch_id(), None);
        assert_eq!(resource.path_prefix(), None);
    }

    #[test]
    fn test_branch_resource_accessors() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let resource = Resource::Branch {
            tenant_id: tid,
            tree_id: trid,
            branch_id: bid,
        };

        assert_eq!(*resource.tenant_id(), tid);
        assert_eq!(*resource.tree_id(), trid);
        assert_eq!(resource.branch_id(), Some(&bid));
        assert_eq!(resource.path_prefix(), None);
    }

    #[test]
    fn test_subtree_resource_accessors() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Subtree {
            tenant_id: tid,
            tree_id: trid,
            path_prefix: "src/lib".to_string(),
        };

        assert_eq!(*resource.tenant_id(), tid);
        assert_eq!(*resource.tree_id(), trid);
        assert_eq!(resource.branch_id(), None);
        assert_eq!(resource.path_prefix(), Some("src/lib"));
    }

    #[test]
    fn test_tree_resource_to_scope() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };

        let scope = resource.to_scope();
        assert_eq!(scope, Scope::Tree(tid, trid));
    }

    #[test]
    fn test_branch_resource_to_scope() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let resource = Resource::Branch {
            tenant_id: tid,
            tree_id: trid,
            branch_id: bid,
        };

        let scope = resource.to_scope();
        assert_eq!(scope, Scope::Branch(tid, trid, bid));
    }

    #[test]
    fn test_subtree_resource_to_scope_falls_back_to_tree() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Subtree {
            tenant_id: tid,
            tree_id: trid,
            path_prefix: "docs/api".to_string(),
        };

        // Subtree has no direct scope equivalent; maps to enclosing Tree scope.
        let scope = resource.to_scope();
        assert_eq!(scope, Scope::Tree(tid, trid));
    }

    #[test]
    fn test_display_tree() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };
        let display = resource.to_string();
        assert!(display.starts_with("tree:"));
        assert!(display.contains(&tid.to_string()));
        assert!(display.contains(&trid.to_string()));
    }

    #[test]
    fn test_display_branch() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let bid = BranchId::new();
        let resource = Resource::Branch {
            tenant_id: tid,
            tree_id: trid,
            branch_id: bid,
        };
        let display = resource.to_string();
        assert!(display.starts_with("branch:"));
        assert!(display.contains(&bid.to_string()));
    }

    #[test]
    fn test_display_subtree() {
        let tid = TenantId::new();
        let trid = TreeId::new();
        let resource = Resource::Subtree {
            tenant_id: tid,
            tree_id: trid,
            path_prefix: "src/main".to_string(),
        };
        let display = resource.to_string();
        assert!(display.starts_with("subtree:"));
        assert!(display.contains("src/main"));
    }

    #[test]
    fn test_serde_json_roundtrip_tree() {
        let resource = Resource::Tree {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
        };
        let json = serde_json::to_string(&resource).expect("serialize");
        let deserialized: Resource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(resource, deserialized);
    }

    #[test]
    fn test_serde_json_roundtrip_branch() {
        let resource = Resource::Branch {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            branch_id: BranchId::new(),
        };
        let json = serde_json::to_string(&resource).expect("serialize");
        let deserialized: Resource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(resource, deserialized);
    }

    #[test]
    fn test_serde_json_roundtrip_subtree() {
        let resource = Resource::Subtree {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            path_prefix: "deep/nested/path".to_string(),
        };
        let json = serde_json::to_string(&resource).expect("serialize");
        let deserialized: Resource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(resource, deserialized);
    }

    #[test]
    fn test_resource_equality() {
        let tid = TenantId::new();
        let trid = TreeId::new();

        let r1 = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };
        let r2 = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };
        assert_eq!(r1, r2);

        let r3 = Resource::Tree {
            tenant_id: TenantId::new(),
            tree_id: trid,
        };
        assert_ne!(r1, r3);
    }

    #[test]
    fn test_resource_hash() {
        use std::collections::HashSet;

        let tid = TenantId::new();
        let trid = TreeId::new();
        let mut set = HashSet::new();

        let r1 = Resource::Tree {
            tenant_id: tid,
            tree_id: trid,
        };
        set.insert(r1.clone());
        assert!(set.contains(&r1));
        assert_eq!(set.len(), 1);

        // Inserting the same resource again doesn't increase size.
        set.insert(r1.clone());
        assert_eq!(set.len(), 1);

        // A different resource does increase size.
        let r2 = Resource::Branch {
            tenant_id: tid,
            tree_id: trid,
            branch_id: BranchId::new(),
        };
        set.insert(r2);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_clone() {
        let resource = Resource::Subtree {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            path_prefix: "abc".to_string(),
        };
        let cloned = resource.clone();
        assert_eq!(resource, cloned);
    }
}
