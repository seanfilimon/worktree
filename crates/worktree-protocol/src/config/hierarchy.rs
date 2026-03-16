use super::worktree_config::*;
use super::tree_config::*;

/// Resolved configuration for a specific tree, after merging root + tree-level configs.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub name: String,
    pub server: Option<String>,
    pub tenant: Option<String>,
    pub visibility: Visibility,
    pub auto_snapshot_enabled: bool,
    pub auto_snapshot_timeout_secs: u64,
    pub auto_snapshot_max_files: u32,
    pub large_file_threshold: u64,
    pub large_file_chunk_size: u64,
    pub lazy_loading: bool,
    pub reflog_retention_days: u32,
    pub reflog_max_entries: u32,
    pub sync_auto: bool,
    pub sync_interval: u64,
    pub conflict_strategy: ConflictStrategy,
    pub default_license: Option<String>,
    pub branch_protection: Vec<BranchProtectionRule>,
    pub registered_paths: Vec<RegisteredPath>,
}

impl ResolvedConfig {
    /// Resolve configuration by merging root config with tree-level overrides.
    /// The Permission Ceiling Model: tree-level can restrict but never expand.
    pub fn resolve(root: &WorktreeConfig, tree_config: Option<&TreeLevelConfig>) -> Self {
        let tree = tree_config.and_then(|tc| tc.tree.as_ref());
        let tree_auto = tree_config.and_then(|tc| tc.auto_snapshot.as_ref());
        let tree_lf = tree_config.and_then(|tc| tc.large_files.as_ref());
        let tree_reflog = tree_config.and_then(|tc| tc.reflog.as_ref());
        let tree_license = tree_config.and_then(|tc| tc.license.as_ref());

        // Merge branch protection: tree-level can only be stricter
        let mut branch_protection = root.branch_protection.clone();
        if let Some(tc) = tree_config {
            for tree_rule in &tc.branch_protection {
                // Find matching root rule or add new (additive is always allowed)
                if let Some(root_rule) = branch_protection.iter_mut().find(|r| r.pattern == tree_rule.pattern) {
                    // Ceiling model: tree can only make rules stricter
                    merge_protection_rule(root_rule, tree_rule);
                } else {
                    branch_protection.push(tree_rule.clone());
                }
            }
        }

        // Merge registered paths (additive)
        let mut registered_paths = root.registered_paths.clone();
        if let Some(tc) = tree_config {
            registered_paths.extend(tc.registered_paths.iter().cloned());
        }

        // Large file threshold: tree can be more restrictive (lower)
        let large_file_threshold = if let Some(tree_threshold) = tree_lf.and_then(|lf| lf.threshold_bytes) {
            tree_threshold.min(root.large_files.threshold_bytes)
        } else {
            root.large_files.threshold_bytes
        };

        let large_file_chunk_size = if let Some(tree_chunk) = tree_lf.and_then(|lf| lf.chunk_size_bytes) {
            tree_chunk.min(root.large_files.chunk_size_bytes)
        } else {
            root.large_files.chunk_size_bytes
        };

        // Reflog: tree can have shorter retention (ceiling model)
        let reflog_retention_days = if let Some(tree_ret) = tree_reflog.and_then(|r| r.retention_days) {
            tree_ret.min(root.reflog.retention_days)
        } else {
            root.reflog.retention_days
        };

        let reflog_max_entries = if let Some(tree_max) = tree_reflog.and_then(|r| r.max_entries) {
            tree_max.min(root.reflog.max_entries)
        } else {
            root.reflog.max_entries
        };

        Self {
            name: tree.map(|t| t.name.clone()).unwrap_or_else(|| root.worktree.name.clone()),
            server: root.worktree.server.clone(),
            tenant: root.worktree.tenant.clone(),
            visibility: root.worktree.visibility.clone(), // Cannot be overridden by tree
            auto_snapshot_enabled: tree_auto
                .and_then(|a| a.enabled)
                .unwrap_or(root.auto_snapshot.enabled)
                && root.auto_snapshot.enabled,
            auto_snapshot_timeout_secs: if let Some(tree_timeout) = tree_auto.and_then(|a| a.inactivity_timeout_secs) {
                tree_timeout.min(root.auto_snapshot.inactivity_timeout_secs)
            } else {
                root.auto_snapshot.inactivity_timeout_secs
            },
            auto_snapshot_max_files: if let Some(tree_max) = tree_auto.and_then(|a| a.max_changed_files) {
                tree_max.min(root.auto_snapshot.max_changed_files)
            } else {
                root.auto_snapshot.max_changed_files
            },
            large_file_threshold,
            large_file_chunk_size,
            lazy_loading: root.large_files.lazy_loading,
            reflog_retention_days,
            reflog_max_entries,
            sync_auto: root.sync.auto,
            sync_interval: root.sync.interval_secs,
            conflict_strategy: root.sync.conflict_strategy.clone(),
            default_license: tree_license
                .and_then(|l| l.default.clone())
                .or_else(|| root.license.default.clone()),
            branch_protection,
            registered_paths,
        }
    }
}

/// Merge protection rule: tree can only make stricter (set to true, raise counts)
fn merge_protection_rule(root: &mut BranchProtectionRule, tree: &BranchProtectionRule) {
    root.no_direct_push = root.no_direct_push || tree.no_direct_push;
    root.require_merge_review = root.require_merge_review || tree.require_merge_review;
    root.required_reviewers = root.required_reviewers.max(tree.required_reviewers);
    root.no_delete = root.no_delete || tree.no_delete;
    root.require_ci_pass = root.require_ci_pass || tree.require_ci_pass;
    root.require_snapshot_signature = root.require_snapshot_signature || tree.require_snapshot_signature;
    // Merge CI checks (additive)
    for check in &tree.required_ci_checks {
        if !root.required_ci_checks.contains(check) {
            root.required_ci_checks.push(check.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_defaults() {
        let root = WorktreeConfig::new("my-project");
        let resolved = ResolvedConfig::resolve(&root, None);
        assert_eq!(resolved.name, "my-project");
        assert!(resolved.auto_snapshot_enabled);
        assert_eq!(resolved.large_file_threshold, 10 * 1024 * 1024);
    }

    #[test]
    fn test_resolve_with_tree_overrides() {
        let root = WorktreeConfig::new("my-project");
        let tree = TreeLevelConfig {
            tree: Some(TreeSection {
                name: "backend".to_string(),
                description: None,
                branch_strategy: BranchStrategy::TrunkBased,
            }),
            auto_snapshot: Some(TreeAutoSnapshotSection {
                enabled: Some(false),
                inactivity_timeout_secs: None,
                max_changed_files: None,
            }),
            ..Default::default()
        };
        let resolved = ResolvedConfig::resolve(&root, Some(&tree));
        assert_eq!(resolved.name, "backend");
        assert!(!resolved.auto_snapshot_enabled);
    }

    #[test]
    fn test_ceiling_model_large_files() {
        let root = WorktreeConfig::new("proj");
        // Tree tries to set a HIGHER threshold — ceiling model prevents it
        let mut tree = TreeLevelConfig::default();
        tree.large_files = Some(TreeLargeFilesSection {
            threshold_bytes: Some(100 * 1024 * 1024), // 100MB > root's 10MB
            chunk_size_bytes: None,
        });
        let resolved = ResolvedConfig::resolve(&root, Some(&tree));
        // Should be clamped to root's 10MB
        assert_eq!(resolved.large_file_threshold, 10 * 1024 * 1024);
    }

    #[test]
    fn test_ceiling_model_branch_protection() {
        let mut root = WorktreeConfig::new("proj");
        root.branch_protection.push(BranchProtectionRule {
            pattern: "main".to_string(),
            no_direct_push: true,
            required_reviewers: 1,
            ..Default::default()
        });
        let mut tree = TreeLevelConfig::default();
        tree.branch_protection.push(BranchProtectionRule {
            pattern: "main".to_string(),
            no_direct_push: false, // tries to relax — will be kept true
            required_reviewers: 3, // stricter — will be applied
            require_ci_pass: true,
            ..Default::default()
        });
        let resolved = ResolvedConfig::resolve(&root, Some(&tree));
        let main_rule = resolved.branch_protection.iter().find(|r| r.pattern == "main").unwrap();
        assert!(main_rule.no_direct_push); // ceiling: stays true
        assert_eq!(main_rule.required_reviewers, 3); // stricter: applied
        assert!(main_rule.require_ci_pass); // new restriction: applied
    }
}
