use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::id::TreeId;

/// Configuration options for a worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeConfig {
    /// Whether snapshots are automatically created on file changes.
    pub auto_snapshot: bool,
    /// Glob patterns for paths to ignore.
    pub ignore_patterns: Vec<String>,
}

impl Default for TreeConfig {
    fn default() -> Self {
        Self {
            auto_snapshot: true,
            ignore_patterns: Vec::new(),
        }
    }
}

/// A worktree represents a tracked directory tree with version control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tree {
    /// Unique identifier for this tree.
    pub id: TreeId,
    /// Human-readable name for the tree.
    pub name: String,
    /// Optional parent tree (for nested / linked trees).
    pub parent: Option<TreeId>,
    /// The root filesystem path this tree tracks.
    pub root_path: PathBuf,
    /// When this tree was created.
    pub created_at: DateTime<Utc>,
    /// Configuration for this tree.
    pub config: TreeConfig,
}

impl Tree {
    /// Create a new tree with the given name and root path, using default config.
    pub fn new(name: impl Into<String>, root_path: impl Into<PathBuf>) -> Self {
        Self {
            id: TreeId::new(),
            name: name.into(),
            parent: None,
            root_path: root_path.into(),
            created_at: Utc::now(),
            config: TreeConfig::default(),
        }
    }

    /// Create a new tree that is a child of the given parent tree.
    pub fn with_parent(
        name: impl Into<String>,
        root_path: impl Into<PathBuf>,
        parent: TreeId,
    ) -> Self {
        Self {
            id: TreeId::new(),
            name: name.into(),
            parent: Some(parent),
            root_path: root_path.into(),
            created_at: Utc::now(),
            config: TreeConfig::default(),
        }
    }

    /// Returns `true` if this tree has a parent.
    pub fn has_parent(&self) -> bool {
        self.parent.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tree() {
        let tree = Tree::new("my-project", "/home/user/project");
        assert_eq!(tree.name, "my-project");
        assert_eq!(tree.root_path, PathBuf::from("/home/user/project"));
        assert!(tree.parent.is_none());
        assert!(!tree.has_parent());
        assert!(tree.config.auto_snapshot);
        assert!(tree.config.ignore_patterns.is_empty());
    }

    #[test]
    fn test_with_parent() {
        let parent = Tree::new("parent", "/parent");
        let child = Tree::with_parent("child", "/parent/child", parent.id);

        assert_eq!(child.parent, Some(parent.id));
        assert!(child.has_parent());
        assert_ne!(child.id, parent.id);
    }

    #[test]
    fn test_unique_ids() {
        let a = Tree::new("a", "/a");
        let b = Tree::new("b", "/b");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn test_tree_config_default() {
        let config = TreeConfig::default();
        assert!(config.auto_snapshot);
        assert!(config.ignore_patterns.is_empty());
    }

    #[test]
    fn test_tree_serde_json_roundtrip() {
        let tree = Tree::new("serde-test", "/tmp/serde");
        let json = serde_json::to_string(&tree).expect("serialize");
        let deserialized: Tree = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(tree.id, deserialized.id);
        assert_eq!(tree.name, deserialized.name);
        assert_eq!(tree.root_path, deserialized.root_path);
        assert_eq!(tree.parent, deserialized.parent);
        assert_eq!(tree.config, deserialized.config);
    }

    #[test]
    fn test_tree_serde_bincode_roundtrip() {
        let tree = Tree::new("bincode-test", "/tmp/bincode");
        let bytes = bincode::serialize(&tree).expect("serialize");
        let deserialized: Tree = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(tree.id, deserialized.id);
        assert_eq!(tree.name, deserialized.name);
        assert_eq!(tree.root_path, deserialized.root_path);
    }

    #[test]
    fn test_tree_config_custom() {
        let mut tree = Tree::new("custom", "/custom");
        tree.config.auto_snapshot = false;
        tree.config.ignore_patterns = vec!["*.tmp".into(), "node_modules".into()];

        assert!(!tree.config.auto_snapshot);
        assert_eq!(tree.config.ignore_patterns.len(), 2);
    }
}
