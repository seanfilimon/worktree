//! State persistence.
//!
//! The state model and its load/save functions live behind this module so
//! that [`crate::ops`] never touches serialization directly. WT-PHASE-2
//! replaces the JSON-document backend with the content-addressable store
//! from `worktree-store`; only this module changes when that happens.

mod state_json;

pub use state_json::{load_state, save_state};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Persisted state of the worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeState {
    pub name: String,
    pub trees: Vec<TreeState>,
    pub current_tree: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeState {
    pub name: String,
    pub path: String,
    pub branches: Vec<BranchState>,
    pub current_branch: String,
    pub snapshots: Vec<SnapshotState>,
    pub tags: Vec<TagState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchState {
    pub name: String,
    pub tip: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotState {
    pub id: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub parents: Vec<String>,
    pub tree_name: String,
    pub branch_name: String,
    pub files: Vec<FileEntry>,
    pub auto_generated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagState {
    pub name: String,
    pub target_snapshot: String,
    pub message: Option<String>,
    pub tagger: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflogEntryState {
    pub action: String,
    pub branch: String,
    pub old_snapshot: Option<String>,
    pub new_snapshot: Option<String>,
    pub message: String,
    pub timestamp: String,
}

impl WorktreeState {
    pub fn new(name: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            name: name.to_string(),
            trees: vec![TreeState {
                name: "root".to_string(),
                path: ".".to_string(),
                branches: vec![BranchState {
                    name: "main".to_string(),
                    tip: None,
                    created_at: now.clone(),
                }],
                current_branch: "main".to_string(),
                snapshots: Vec::new(),
                tags: Vec::new(),
            }],
            current_tree: Some("root".to_string()),
            created_at: now,
        }
    }

    pub fn find_tree(&self, name: &str) -> Option<&TreeState> {
        self.trees.iter().find(|t| t.name == name)
    }

    pub fn find_tree_mut(&mut self, name: &str) -> Option<&mut TreeState> {
        self.trees.iter_mut().find(|t| t.name == name)
    }

    pub fn current_tree(&self) -> Option<&TreeState> {
        self.current_tree
            .as_ref()
            .and_then(|name| self.find_tree(name))
    }

    pub fn current_tree_mut(&mut self) -> Option<&mut TreeState> {
        let name = self.current_tree.clone();
        name.and_then(move |n| self.find_tree_mut(&n))
    }
}

impl TreeState {
    pub fn current_branch(&self) -> Option<&BranchState> {
        self.branches.iter().find(|b| b.name == self.current_branch)
    }

    pub fn find_branch(&self, name: &str) -> Option<&BranchState> {
        self.branches.iter().find(|b| b.name == name)
    }

    pub fn find_branch_mut(&mut self, name: &str) -> Option<&mut BranchState> {
        self.branches.iter_mut().find(|b| b.name == name)
    }

    pub fn snapshots_on_branch(&self, branch_name: &str) -> Vec<&SnapshotState> {
        self.snapshots
            .iter()
            .filter(|s| s.branch_name == branch_name)
            .collect()
    }
}
