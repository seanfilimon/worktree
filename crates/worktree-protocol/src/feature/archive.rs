//! Archive/export functionality for worktree snapshots.

use serde::{Deserialize, Serialize};
use crate::core::id::{TreeId, SnapshotId};

/// Archive format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl Default for ArchiveFormat {
    fn default() -> Self {
        ArchiveFormat::TarGz
    }
}

/// Options for creating an archive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveOptions {
    pub format: ArchiveFormat,
    pub tree_id: TreeId,
    pub snapshot_id: Option<SnapshotId>,
    pub prefix: Option<String>,
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub respect_ignore: bool,
    pub respect_licenses: bool,
}

impl ArchiveOptions {
    pub fn new(tree_id: TreeId) -> Self {
        Self {
            format: ArchiveFormat::TarGz,
            tree_id,
            snapshot_id: None,
            prefix: None,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
            respect_ignore: true,
            respect_licenses: true,
        }
    }

    pub fn with_format(mut self, format: ArchiveFormat) -> Self {
        self.format = format;
        self
    }

    pub fn with_snapshot(mut self, snapshot_id: SnapshotId) -> Self {
        self.snapshot_id = Some(snapshot_id);
        self
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    pub fn include(mut self, path: &str) -> Self {
        self.include_paths.push(path.to_string());
        self
    }

    pub fn exclude(mut self, path: &str) -> Self {
        self.exclude_paths.push(path.to_string());
        self
    }

    pub fn skip_license_check(mut self) -> Self {
        self.respect_licenses = false;
        self
    }

    /// Check if a path should be included in the archive
    pub fn should_include(&self, path: &str) -> bool {
        if !self.include_paths.is_empty() {
            if !self.include_paths.iter().any(|p| path.starts_with(p)) {
                return false;
            }
        }
        if self.exclude_paths.iter().any(|p| path.starts_with(p)) {
            return false;
        }
        true
    }
}

/// Result of an archive operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub format: ArchiveFormat,
    pub file_count: u32,
    pub total_size: u64,
    pub output_size: u64,
    pub excluded_by_ignore: u32,
    pub excluded_by_license: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archive_options_builder() {
        let opts = ArchiveOptions::new(TreeId::new())
            .with_format(ArchiveFormat::Zip)
            .with_prefix("my-project-v1.0/")
            .include("src/")
            .exclude("src/test/");

        assert_eq!(opts.format, ArchiveFormat::Zip);
        assert!(opts.should_include("src/main.rs"));
        assert!(!opts.should_include("src/test/test.rs"));
        assert!(!opts.should_include("docs/readme.md"));
    }

    #[test]
    fn test_no_filters_includes_all() {
        let opts = ArchiveOptions::new(TreeId::new());
        assert!(opts.should_include("anything"));
        assert!(opts.should_include("src/deep/file.rs"));
    }
}
