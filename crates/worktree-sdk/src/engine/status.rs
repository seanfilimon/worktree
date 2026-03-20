use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use chrono::Utc;
use crate::error::{SdkError, Result};

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
        self.current_tree.as_ref().and_then(|name| self.find_tree(name))
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
        self.snapshots.iter()
            .filter(|s| s.branch_name == branch_name)
            .collect()
    }
}

/// Helper to check if a relative path starts with a given directory name.
/// Matches exactly the directory name followed by a separator, or the name itself.
fn is_ignored_dir(rel_path: &str, dir_name: &str) -> bool {
    rel_path == dir_name
        || rel_path.starts_with(&format!("{}/", dir_name))
        || rel_path.starts_with(&format!("{}\\", dir_name))
}

/// Helper to check if a relative path contains a given directory as a path segment.
fn contains_ignored_dir(rel_path: &str, dir_name: &str) -> bool {
    let with_slashes = format!("/{}/", dir_name);
    let with_backslashes = format!("\\{}\\", dir_name);
    let with_mixed1 = format!("/{}\\", dir_name);
    let with_mixed2 = format!("\\{}/", dir_name);
    rel_path.starts_with(&format!("{}/", dir_name))
        || rel_path.starts_with(&format!("{}\\", dir_name))
        || rel_path.contains(&with_slashes)
        || rel_path.contains(&with_backslashes)
        || rel_path.contains(&with_mixed1)
        || rel_path.contains(&with_mixed2)
}

/// Load state from disk
pub fn load_state(engine: &super::WorktreeEngine) -> Result<WorktreeState> {
    let state_file = engine.state_file();
    if !state_file.exists() {
        return Err(SdkError::NotAWorktree);
    }
    let content = std::fs::read_to_string(&state_file)?;
    serde_json::from_str(&content)
        .map_err(|e| SdkError::Serialization(e.to_string()))
}

/// Save state to disk (atomic write via temp file + rename)
pub fn save_state(engine: &super::WorktreeEngine, state: &WorktreeState) -> Result<()> {
    let state_file = engine.state_file();
    let tmp_file = state_file.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| SdkError::Serialization(e.to_string()))?;
    std::fs::write(&tmp_file, &content)?;
    std::fs::rename(&tmp_file, &state_file)?;
    Ok(())
}

/// Compute the current working tree status
pub fn compute_status(engine: &super::WorktreeEngine) -> Result<WorkingStatus> {
    let state = load_state(engine)?;
    let tree = state.current_tree().ok_or(SdkError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;

    // Get files from last snapshot
    let last_snapshot = tree.snapshots_on_branch(branch).last().cloned();
    let mut known_files: HashMap<String, String> = HashMap::new();
    if let Some(snap) = &last_snapshot {
        for f in &snap.files {
            known_files.insert(f.path.clone(), f.hash.clone());
        }
    }

    // Walk the working directory
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    let root = engine.root();
    if let Ok(walker) = walkdir_files(root) {
        for file_path in walker {
            let rel_path = file_path.strip_prefix(root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .replace('\\', "/");

            // Skip .wt and .git directories
            if is_ignored_dir(&rel_path, ".wt") || is_ignored_dir(&rel_path, ".git") {
                continue;
            }

            let hash = hash_file_quick(&file_path);
            if let Some(old_hash) = known_files.remove(&rel_path) {
                if hash != old_hash {
                    modified.push(rel_path);
                }
            } else {
                added.push(rel_path);
            }
        }
    }

    // Remaining known files are deleted
    for path in known_files.keys() {
        deleted.push(path.clone());
    }

    Ok(WorkingStatus {
        tree_name: tree.name.clone(),
        branch_name: branch.clone(),
        added,
        modified,
        deleted,
        snapshot_count: tree.snapshots_on_branch(branch).len(),
    })
}

#[derive(Debug, Clone)]
pub struct WorkingStatus {
    pub tree_name: String,
    pub branch_name: String,
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub snapshot_count: usize,
}

impl WorkingStatus {
    pub fn is_clean(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn total_changes(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}

fn walkdir_files(root: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.path().to_path_buf();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel.to_string_lossy();
            if !is_ignored_dir(&rel_str, ".wt")
                && !is_ignored_dir(&rel_str, ".git")
                && !contains_ignored_dir(&rel_str, "node_modules")
            {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn hash_file_quick(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(data) => {
            let hash = blake3::hash(&data);
            hash.to_hex().to_string()
        }
        Err(_) => String::new(),
    }
}
