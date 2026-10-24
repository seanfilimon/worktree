//! Type definitions for Worktree Admin Panel

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Server status information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerStatus {
    pub id: Uuid,
    pub name: String,
    pub running: bool,
    pub uptime_seconds: u64,
    pub active_connections: usize,
    pub tracked_repositories: usize,
    pub last_updated: DateTime<Utc>,
}

impl Default for ServerStatus {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "worktree-server".to_string(),
            running: true,
            uptime_seconds: 3600,
            active_connections: 5,
            tracked_repositories: 12,
            last_updated: Utc::now(),
        }
    }
}

/// Repository information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryInfo {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub branch_count: usize,
    pub commit_count: usize,
    pub last_activity: DateTime<Utc>,
    pub size_bytes: u64,
    pub status: RepositoryStatus,
}

impl RepositoryInfo {
    /// Create a mock repository for testing
    pub fn mock(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            path: format!("/home/user/repos/{}", name),
            branch_count: 3,
            commit_count: 142,
            last_activity: Utc::now(),
            size_bytes: 1024 * 1024 * 25, // 25 MB
            status: RepositoryStatus::Active,
        }
    }
}

/// Repository status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepositoryStatus {
    Active,
    Idle,
    Syncing,
    Error,
}

impl RepositoryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Syncing => "syncing",
            Self::Error => "error",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Idle => "Idle",
            Self::Syncing => "Syncing",
            Self::Error => "Error",
        }
    }

    pub fn badge_class(&self) -> &'static str {
        match self {
            Self::Active => "badge-success",
            Self::Idle => "badge-secondary",
            Self::Syncing => "badge-warning",
            Self::Error => "badge-error",
        }
    }
}

/// Server statistics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerStats {
    pub total_repositories: usize,
    pub total_commits: usize,
    pub total_branches: usize,
    pub total_storage_bytes: u64,
    pub total_operations: u64,
    pub collected_at: DateTime<Utc>,
}

impl Default for ServerStats {
    fn default() -> Self {
        Self {
            total_repositories: 12,
            total_commits: 5420,
            total_branches: 48,
            total_storage_bytes: 1024 * 1024 * 250, // 250 MB
            total_operations: 12340,
            collected_at: Utc::now(),
        }
    }
}

/// Application settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: Theme,
    pub auto_refresh: bool,
    pub refresh_interval_secs: u64,
    pub items_per_page: usize,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            auto_refresh: true,
            refresh_interval_secs: 30,
            items_per_page: 20,
        }
    }
}

/// Application theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
    Auto,
}

impl Theme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Auto => "auto",
        }
    }
}

/// API response wrapper
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ApiResponse<T> {
    Success { data: T },
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_status() {
        assert_eq!(RepositoryStatus::Active.as_str(), "active");
        assert_eq!(RepositoryStatus::Idle.label(), "Idle");
        assert_eq!(RepositoryStatus::Active.badge_class(), "badge-success");
    }

    #[test]
    fn test_theme() {
        assert_eq!(Theme::Dark.as_str(), "dark");
    }

    #[test]
    fn test_mock_repository() {
        let repo = RepositoryInfo::mock("test-repo");
        assert_eq!(repo.name, "test-repo");
        assert!(repo.path.contains("test-repo"));
    }
}
