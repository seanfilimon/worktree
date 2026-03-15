use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{AccountId, TreeId};
use crate::core::hash::ContentHash;

/// Release status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseStatus {
    Draft,
    Published,
    Archived,
}

impl Default for ReleaseStatus {
    fn default() -> Self {
        ReleaseStatus::Draft
    }
}

/// A release artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseArtifact {
    pub name: String,
    pub content_hash: ContentHash,
    pub size: u64,
    pub content_type: String,
    pub uploaded_at: DateTime<Utc>,
}

/// A release bundles a tag with release notes and artifacts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub tree_id: TreeId,
    pub title: String,
    pub notes: String,
    pub status: ReleaseStatus,
    pub author: AccountId,
    pub artifacts: Vec<ReleaseArtifact>,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

impl Release {
    pub fn new(tag_name: &str, tree_id: TreeId, title: &str, notes: &str, author: AccountId) -> Self {
        Self {
            tag_name: tag_name.to_string(),
            tree_id,
            title: title.to_string(),
            notes: notes.to_string(),
            status: ReleaseStatus::Draft,
            author,
            artifacts: Vec::new(),
            created_at: Utc::now(),
            published_at: None,
        }
    }

    pub fn add_artifact(&mut self, artifact: ReleaseArtifact) {
        self.artifacts.push(artifact);
    }

    pub fn publish(&mut self) {
        self.status = ReleaseStatus::Published;
        self.published_at = Some(Utc::now());
    }

    pub fn archive(&mut self) {
        self.status = ReleaseStatus::Archived;
    }

    pub fn is_draft(&self) -> bool { self.status == ReleaseStatus::Draft }
    pub fn is_published(&self) -> bool { self.status == ReleaseStatus::Published }
    pub fn is_archived(&self) -> bool { self.status == ReleaseStatus::Archived }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_lifecycle() {
        let mut release = Release::new("v1.0", TreeId::new(), "Version 1.0", "First release", AccountId::new());
        assert!(release.is_draft());
        assert!(release.published_at.is_none());

        release.publish();
        assert!(release.is_published());
        assert!(release.published_at.is_some());

        release.archive();
        assert!(release.is_archived());
    }

    #[test]
    fn test_release_artifacts() {
        let mut release = Release::new("v1.0", TreeId::new(), "V1", "Notes", AccountId::new());
        assert!(release.artifacts.is_empty());

        release.add_artifact(ReleaseArtifact {
            name: "app.tar.gz".to_string(),
            content_hash: crate::core::hash::ContentHash::ZERO,
            size: 1024,
            content_type: "application/gzip".to_string(),
            uploaded_at: Utc::now(),
        });
        assert_eq!(release.artifacts.len(), 1);
    }
}
