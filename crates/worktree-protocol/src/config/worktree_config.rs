use serde::{Deserialize, Serialize};

/// Root worktree configuration (`.wt/config.toml`)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorktreeConfig {
    #[serde(default)]
    pub worktree: WorktreeSection,
    #[serde(default)]
    pub sync: SyncSection,
    #[serde(default)]
    pub auto_snapshot: AutoSnapshotSection,
    #[serde(default)]
    pub large_files: LargeFilesSection,
    #[serde(default)]
    pub reflog: ReflogSection,
    #[serde(default)]
    pub shallow: ShallowSection,
    #[serde(default)]
    pub license: LicenseSection,
    #[serde(default)]
    pub registered_paths: Vec<RegisteredPath>,
    #[serde(default)]
    pub tenant_access: Vec<TenantAccessGrant>,
    #[serde(default)]
    pub branch_protection: Vec<BranchProtectionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSection {
    pub name: String,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(default = "default_visibility")]
    pub visibility: Visibility,
}

impl Default for WorktreeSection {
    fn default() -> Self {
        Self {
            name: String::new(),
            server: None,
            tenant: None,
            visibility: Visibility::Private,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Private,
    Shared,
    Public,
}

fn default_visibility() -> Visibility {
    Visibility::Private
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Private
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSection {
    #[serde(default = "default_true")]
    pub auto: bool,
    #[serde(default = "default_sync_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
    #[serde(default = "default_conflict_strategy")]
    pub conflict_strategy: ConflictStrategy,
}

impl Default for SyncSection {
    fn default() -> Self {
        Self {
            auto: true,
            interval_secs: 30,
            retry_count: 3,
            conflict_strategy: ConflictStrategy::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConflictStrategy {
    Auto,
    Manual,
    Ours,
    Theirs,
}

impl Default for ConflictStrategy {
    fn default() -> Self {
        ConflictStrategy::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSnapshotSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_inactivity_timeout")]
    pub inactivity_timeout_secs: u64,
    #[serde(default = "default_max_changed_files")]
    pub max_changed_files: u32,
    #[serde(default = "default_max_changed_bytes")]
    pub max_changed_bytes: u64,
    #[serde(default = "default_true")]
    pub on_branch_switch: bool,
}

impl Default for AutoSnapshotSection {
    fn default() -> Self {
        Self {
            enabled: true,
            inactivity_timeout_secs: 30,
            max_changed_files: 50,
            max_changed_bytes: 10 * 1024 * 1024,
            on_branch_switch: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFilesSection {
    #[serde(default = "default_large_file_threshold")]
    pub threshold_bytes: u64,
    #[serde(default = "default_chunk_size")]
    pub chunk_size_bytes: u64,
    #[serde(default = "default_true")]
    pub lazy_loading: bool,
    #[serde(default)]
    pub preload_patterns: Vec<String>,
}

impl Default for LargeFilesSection {
    fn default() -> Self {
        Self {
            threshold_bytes: 10 * 1024 * 1024, // 10MB
            chunk_size_bytes: 4 * 1024 * 1024,  // 4MB
            lazy_loading: true,
            preload_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflogSection {
    #[serde(default = "default_reflog_retention")]
    pub retention_days: u32,
    #[serde(default = "default_reflog_max")]
    pub max_entries: u32,
    #[serde(default = "default_true")]
    pub sync_to_server: bool,
    #[serde(default = "default_true")]
    pub compression: bool,
}

impl Default for ReflogSection {
    fn default() -> Self {
        Self {
            retention_days: 90,
            max_entries: 10000,
            sync_to_server: true,
            compression: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShallowSection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_shallow_depth")]
    pub depth: u32,
    #[serde(default = "default_true")]
    pub auto_deepen: bool,
    #[serde(default = "default_true")]
    pub lazy_blobs: bool,
}

impl Default for ShallowSection {
    fn default() -> Self {
        Self {
            enabled: false,
            depth: 100,
            auto_deepen: true,
            lazy_blobs: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LicenseSection {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub spdx_strict: bool,
    #[serde(default)]
    pub paths: Vec<LicensePath>,
    #[serde(default)]
    pub grants: Vec<LicenseGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePath {
    pub path: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseGrant {
    pub path: String,
    pub tenant: String,
    pub level: GrantLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrantLevel {
    ReadOnly,
    Modify,
    Redistribute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPath {
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantAccessGrant {
    pub tenant: String,
    pub role: String,
    #[serde(default)]
    pub trees: Option<Vec<String>>,
    #[serde(default)]
    pub expires: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProtectionRule {
    pub pattern: String,
    #[serde(default)]
    pub no_direct_push: bool,
    #[serde(default)]
    pub require_merge_review: bool,
    #[serde(default)]
    pub required_reviewers: u32,
    #[serde(default)]
    pub no_delete: bool,
    #[serde(default)]
    pub require_ci_pass: bool,
    #[serde(default)]
    pub required_ci_checks: Vec<String>,
    #[serde(default)]
    pub require_snapshot_signature: bool,
}

impl Default for BranchProtectionRule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            no_direct_push: false,
            require_merge_review: false,
            required_reviewers: 0,
            no_delete: false,
            require_ci_pass: false,
            required_ci_checks: Vec::new(),
            require_snapshot_signature: false,
        }
    }
}

impl WorktreeConfig {
    pub fn new(name: &str) -> Self {
        Self {
            worktree: WorktreeSection {
                name: name.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn with_server(mut self, server: &str) -> Self {
        self.worktree.server = Some(server.to_string());
        self
    }

    pub fn with_tenant(mut self, tenant: &str) -> Self {
        self.worktree.tenant = Some(tenant.to_string());
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.worktree.visibility = visibility;
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.worktree.name.is_empty() {
            errors.push("worktree name must not be empty".to_string());
        }
        if self.large_files.threshold_bytes == 0 {
            errors.push("large file threshold must be > 0".to_string());
        }
        if self.large_files.chunk_size_bytes == 0 {
            errors.push("chunk size must be > 0".to_string());
        }
        for bp in &self.branch_protection {
            if bp.pattern.is_empty() {
                errors.push("branch protection pattern must not be empty".to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn default_true() -> bool { true }
fn default_sync_interval() -> u64 { 30 }
fn default_retry_count() -> u32 { 3 }
fn default_conflict_strategy() -> ConflictStrategy { ConflictStrategy::Auto }
fn default_inactivity_timeout() -> u64 { 30 }
fn default_max_changed_files() -> u32 { 50 }
fn default_max_changed_bytes() -> u64 { 10 * 1024 * 1024 }
fn default_large_file_threshold() -> u64 { 10 * 1024 * 1024 }
fn default_chunk_size() -> u64 { 4 * 1024 * 1024 }
fn default_reflog_retention() -> u32 { 90 }
fn default_reflog_max() -> u32 { 10000 }
fn default_shallow_depth() -> u32 { 100 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WorktreeConfig::new("my-project");
        assert_eq!(config.worktree.name, "my-project");
        assert_eq!(config.worktree.visibility, Visibility::Private);
        assert!(config.sync.auto);
        assert!(config.auto_snapshot.enabled);
        assert_eq!(config.large_files.threshold_bytes, 10 * 1024 * 1024);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_empty_name() {
        let config = WorktreeConfig::default();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_builder_pattern() {
        let config = WorktreeConfig::new("test")
            .with_server("https://wt.example.com")
            .with_tenant("my-org")
            .with_visibility(Visibility::Shared);
        assert_eq!(config.worktree.server.as_deref(), Some("https://wt.example.com"));
        assert_eq!(config.worktree.tenant.as_deref(), Some("my-org"));
        assert_eq!(config.worktree.visibility, Visibility::Shared);
    }
}
