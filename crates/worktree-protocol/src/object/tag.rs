use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{SnapshotId, AccountId, TreeId};

/// Tag type — lightweight, annotated, or signed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TagKind {
    Lightweight,
    Annotated,
    Signed,
}

/// Immutable reference to a specific snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub target: SnapshotId,
    pub tree_id: TreeId,
    pub kind: TagKind,
    pub message: Option<String>,
    pub tagger: Option<AccountId>,
    pub created_at: DateTime<Utc>,
    pub signature: Option<Vec<u8>>,
}

impl Tag {
    pub fn lightweight(name: &str, target: SnapshotId, tree_id: TreeId) -> Self {
        Self {
            name: name.to_string(),
            target,
            tree_id,
            kind: TagKind::Lightweight,
            message: None,
            tagger: None,
            created_at: Utc::now(),
            signature: None,
        }
    }

    pub fn annotated(name: &str, target: SnapshotId, tree_id: TreeId, message: &str, tagger: AccountId) -> Self {
        Self {
            name: name.to_string(),
            target,
            tree_id,
            kind: TagKind::Annotated,
            message: Some(message.to_string()),
            tagger: Some(tagger),
            created_at: Utc::now(),
            signature: None,
        }
    }

    pub fn signed(name: &str, target: SnapshotId, tree_id: TreeId, message: &str, tagger: AccountId, signature: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            target,
            tree_id,
            kind: TagKind::Signed,
            message: Some(message.to_string()),
            tagger: Some(tagger),
            created_at: Utc::now(),
            signature: Some(signature),
        }
    }

    pub fn is_lightweight(&self) -> bool { self.kind == TagKind::Lightweight }
    pub fn is_annotated(&self) -> bool { self.kind == TagKind::Annotated }
    pub fn is_signed(&self) -> bool { self.kind == TagKind::Signed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lightweight_tag() {
        let tag = Tag::lightweight("v1.0", SnapshotId::new(), TreeId::new());
        assert!(tag.is_lightweight());
        assert!(tag.message.is_none());
        assert!(tag.tagger.is_none());
    }

    #[test]
    fn test_annotated_tag() {
        let tag = Tag::annotated("v2.0", SnapshotId::new(), TreeId::new(), "Release 2.0", AccountId::new());
        assert!(tag.is_annotated());
        assert_eq!(tag.message.as_deref(), Some("Release 2.0"));
        assert!(tag.tagger.is_some());
    }

    #[test]
    fn test_signed_tag() {
        let sig = vec![1, 2, 3, 4];
        let tag = Tag::signed("v3.0", SnapshotId::new(), TreeId::new(), "Signed release", AccountId::new(), sig);
        assert!(tag.is_signed());
        assert!(tag.signature.is_some());
    }
}
