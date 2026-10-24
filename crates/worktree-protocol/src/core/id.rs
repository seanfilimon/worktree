use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            pub fn nil() -> Self {
                Self(Uuid::nil())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

define_id!(TreeId, "Unique identifier for a worktree.");
define_id!(SnapshotId, "Unique identifier for a snapshot (commit).");
define_id!(BranchId, "Unique identifier for a branch.");
define_id!(TenantId, "Unique identifier for a tenant in multi-tenant setups.");
define_id!(AccountId, "Unique identifier for a user account.");
define_id!(TeamId, "Unique identifier for a team.");
define_id!(RoleId, "Unique identifier for a role.");
define_id!(PolicyId, "Unique identifier for an access-control policy.");
define_id!(SessionId, "Unique identifier for an authenticated session.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_id_new_is_unique() {
        let a = TreeId::new();
        let b = TreeId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn test_id_display_roundtrip() {
        let id = SnapshotId::new();
        let s = id.to_string();
        let parsed: SnapshotId = s.parse().expect("parse failed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_id_nil() {
        let id = BranchId::nil();
        assert_eq!(id.as_uuid(), &Uuid::nil());
    }

    #[test]
    fn test_id_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = TenantId::from_uuid(uuid);
        assert_eq!(*id.as_uuid(), uuid);
    }

    #[test]
    fn test_id_clone_copy() {
        let id = AccountId::new();
        let cloned = id;
        assert_eq!(id, cloned);
    }

    #[test]
    fn test_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let id = TeamId::new();
        set.insert(id);
        assert!(set.contains(&id));
    }

    #[test]
    fn test_id_serde_json_roundtrip() {
        let id = RoleId::new();
        let json = serde_json::to_string(&id).expect("serialize failed");
        let deserialized: RoleId = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_id_serde_bincode_roundtrip() {
        let id = PolicyId::new();
        let bytes = bincode::serialize(&id).expect("serialize failed");
        let deserialized: PolicyId = bincode::deserialize(&bytes).expect("deserialize failed");
        assert_eq!(id, deserialized);
    }

    #[test]
    fn test_session_id_default() {
        let id = SessionId::default();
        // default creates a new v4 uuid, so it should not be nil
        assert_ne!(id, SessionId::nil());
    }
}
