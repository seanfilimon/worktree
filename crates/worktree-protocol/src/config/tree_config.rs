use serde::{Deserialize, Serialize};

/// Per-tree configuration (`.wt-tree/config.toml`)
/// All fields are optional — omitted fields inherit from the root `.wt/config.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TreeLevelConfig {
    #[serde(default)]
    pub tree: Option<TreeSection>,
    #[serde(default)]
    pub auto_snapshot: Option<TreeAutoSnapshotSection>,
    #[serde(default)]
    pub large_files: Option<TreeLargeFilesSection>,
    #[serde(default)]
    pub reflog: Option<TreeReflogSection>,
    #[serde(default)]
    pub license: Option<TreeLicenseSection>,
    #[serde(default)]
    pub registered_paths: Vec<super::worktree_config::RegisteredPath>,
    #[serde(default)]
    pub branch_protection: Vec<super::worktree_config::BranchProtectionRule>,
    #[serde(default)]
    pub dependencies: Vec<TreeDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSection {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_branch_strategy")]
    pub branch_strategy: BranchStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BranchStrategy {
    FeatureBranch,
    TrunkBased,
    ReleaseTrain,
}

impl Default for BranchStrategy {
    fn default() -> Self {
        BranchStrategy::FeatureBranch
    }
}

fn default_branch_strategy() -> BranchStrategy {
    BranchStrategy::FeatureBranch
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeAutoSnapshotSection {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub inactivity_timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_changed_files: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeLargeFilesSection {
    #[serde(default)]
    pub threshold_bytes: Option<u64>,
    #[serde(default)]
    pub chunk_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeReflogSection {
    #[serde(default)]
    pub retention_days: Option<u32>,
    #[serde(default)]
    pub max_entries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TreeLicenseSection {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub paths: Vec<super::worktree_config::LicensePath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeDependency {
    pub name: String,
    pub path: String,
    #[serde(default = "default_main")]
    pub branch: String,
    #[serde(default)]
    pub required: bool,
}

fn default_main() -> String {
    "main".to_string()
}

impl TreeLevelConfig {
    pub fn new(name: &str) -> Self {
        Self {
            tree: Some(TreeSection {
                name: name.to_string(),
                description: None,
                branch_strategy: BranchStrategy::default(),
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_config_new() {
        let cfg = TreeLevelConfig::new("backend");
        assert_eq!(cfg.tree.as_ref().unwrap().name, "backend");
        assert_eq!(cfg.tree.as_ref().unwrap().branch_strategy, BranchStrategy::FeatureBranch);
    }

    #[test]
    fn test_tree_config_default_is_empty() {
        let cfg = TreeLevelConfig::default();
        assert!(cfg.tree.is_none());
        assert!(cfg.auto_snapshot.is_none());
        assert!(cfg.dependencies.is_empty());
    }
}
