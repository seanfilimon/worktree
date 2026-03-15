use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{BranchId, TreeId, AccountId, SnapshotId};

/// Status of a merge request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeRequestStatus {
    Open,
    InReview,
    Approved,
    ChangesRequested,
    Merged,
    Closed,
}

impl Default for MergeRequestStatus {
    fn default() -> Self {
        MergeRequestStatus::Open
    }
}

/// CI check status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Skipped,
}

impl Default for CiStatus {
    fn default() -> Self {
        CiStatus::Pending
    }
}

/// A review on a merge request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub reviewer: AccountId,
    pub status: ReviewStatus,
    pub comment: Option<String>,
    pub reviewed_at: DateTime<Utc>,
    pub snapshot_reviewed: SnapshotId,
    pub stale: bool,
}

/// Review decision
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    Commented,
}

/// A CI check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCheck {
    pub name: String,
    pub status: CiStatus,
    pub url: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// A merge request (built into the protocol, not an external service)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    pub id: u64,
    pub tree_id: TreeId,
    pub source_branch: BranchId,
    pub source_branch_name: String,
    pub target_branch: BranchId,
    pub target_branch_name: String,
    pub title: String,
    pub description: String,
    pub author: AccountId,
    pub status: MergeRequestStatus,
    pub reviews: Vec<Review>,
    pub ci_checks: Vec<CiCheck>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    pub merged_by: Option<AccountId>,
    pub merge_snapshot: Option<SnapshotId>,
    pub linked_merge_requests: Vec<u64>,
}

impl MergeRequest {
    pub fn new(
        id: u64,
        tree_id: TreeId,
        source_branch: BranchId,
        source_branch_name: &str,
        target_branch: BranchId,
        target_branch_name: &str,
        title: &str,
        author: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            tree_id,
            source_branch,
            source_branch_name: source_branch_name.to_string(),
            target_branch,
            target_branch_name: target_branch_name.to_string(),
            title: title.to_string(),
            description: String::new(),
            author,
            status: MergeRequestStatus::Open,
            reviews: Vec::new(),
            ci_checks: Vec::new(),
            created_at: now,
            updated_at: now,
            merged_at: None,
            merged_by: None,
            merge_snapshot: None,
            linked_merge_requests: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn add_review(&mut self, review: Review) {
        // Mark previous reviews from this reviewer as stale
        for existing in &mut self.reviews {
            if existing.reviewer == review.reviewer {
                existing.stale = true;
            }
        }
        self.reviews.push(review);
        self.updated_at = Utc::now();
        self.update_status();
    }

    pub fn add_ci_check(&mut self, check: CiCheck) {
        if let Some(existing) = self.ci_checks.iter_mut().find(|c| c.name == check.name) {
            *existing = check;
        } else {
            self.ci_checks.push(check);
        }
        self.updated_at = Utc::now();
    }

    pub fn approve_count(&self) -> usize {
        self.reviews.iter()
            .filter(|r| !r.stale && r.status == ReviewStatus::Approved)
            .count()
    }

    pub fn has_changes_requested(&self) -> bool {
        self.reviews.iter()
            .any(|r| !r.stale && r.status == ReviewStatus::ChangesRequested)
    }

    pub fn all_ci_passed(&self) -> bool {
        !self.ci_checks.is_empty() &&
        self.ci_checks.iter().all(|c| c.status == CiStatus::Passed || c.status == CiStatus::Skipped)
    }

    pub fn can_merge(&self, required_reviewers: u32) -> bool {
        self.status != MergeRequestStatus::Merged
            && self.status != MergeRequestStatus::Closed
            && !self.has_changes_requested()
            && self.approve_count() >= required_reviewers as usize
            && (self.ci_checks.is_empty() || self.all_ci_passed())
    }

    pub fn merge(&mut self, merged_by: AccountId, merge_snapshot: SnapshotId) {
        self.status = MergeRequestStatus::Merged;
        self.merged_at = Some(Utc::now());
        self.merged_by = Some(merged_by);
        self.merge_snapshot = Some(merge_snapshot);
        self.updated_at = Utc::now();
    }

    pub fn close(&mut self) {
        self.status = MergeRequestStatus::Closed;
        self.updated_at = Utc::now();
    }

    pub fn link_merge_request(&mut self, other_id: u64) {
        if !self.linked_merge_requests.contains(&other_id) {
            self.linked_merge_requests.push(other_id);
        }
    }

    fn update_status(&mut self) {
        // Never revert terminal states
        if self.status == MergeRequestStatus::Merged || self.status == MergeRequestStatus::Closed {
            return;
        }
        if self.has_changes_requested() {
            self.status = MergeRequestStatus::ChangesRequested;
        } else if self.approve_count() > 0 {
            self.status = MergeRequestStatus::Approved;
        } else if !self.reviews.is_empty() {
            self.status = MergeRequestStatus::InReview;
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, MergeRequestStatus::Open | MergeRequestStatus::InReview | MergeRequestStatus::Approved | MergeRequestStatus::ChangesRequested)
    }

    pub fn is_merged(&self) -> bool { self.status == MergeRequestStatus::Merged }
    pub fn is_closed(&self) -> bool { self.status == MergeRequestStatus::Closed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_request_lifecycle() {
        let mut mr = MergeRequest::new(
            1, TreeId::new(), BranchId::new(), "feature/x",
            BranchId::new(), "main", "Add feature X", AccountId::new(),
        );
        assert!(mr.is_open());
        assert!(!mr.can_merge(1));

        mr.add_review(Review {
            reviewer: AccountId::new(),
            status: ReviewStatus::Approved,
            comment: Some("LGTM".to_string()),
            reviewed_at: Utc::now(),
            snapshot_reviewed: SnapshotId::new(),
            stale: false,
        });
        assert_eq!(mr.approve_count(), 1);
        assert!(mr.can_merge(1));

        mr.merge(AccountId::new(), SnapshotId::new());
        assert!(mr.is_merged());
        assert!(!mr.can_merge(0));
    }

    #[test]
    fn test_changes_requested_blocks_merge() {
        let mut mr = MergeRequest::new(
            2, TreeId::new(), BranchId::new(), "fix/y",
            BranchId::new(), "main", "Fix Y", AccountId::new(),
        );
        mr.add_review(Review {
            reviewer: AccountId::new(),
            status: ReviewStatus::ChangesRequested,
            comment: Some("Needs work".to_string()),
            reviewed_at: Utc::now(),
            snapshot_reviewed: SnapshotId::new(),
            stale: false,
        });
        assert!(mr.has_changes_requested());
        assert!(!mr.can_merge(0));
    }

    #[test]
    fn test_ci_checks() {
        let mut mr = MergeRequest::new(
            3, TreeId::new(), BranchId::new(), "feature/z",
            BranchId::new(), "main", "Feature Z", AccountId::new(),
        );
        mr.add_ci_check(CiCheck {
            name: "tests".to_string(),
            status: CiStatus::Passed,
            url: None,
            started_at: None,
            completed_at: None,
        });
        mr.add_ci_check(CiCheck {
            name: "lint".to_string(),
            status: CiStatus::Failed,
            url: None,
            started_at: None,
            completed_at: None,
        });
        assert!(!mr.all_ci_passed());

        // Fix the lint check
        mr.add_ci_check(CiCheck {
            name: "lint".to_string(),
            status: CiStatus::Passed,
            url: None,
            started_at: None,
            completed_at: None,
        });
        assert!(mr.all_ci_passed());
    }

    #[test]
    fn test_stale_reviews() {
        let reviewer = AccountId::new();
        let mut mr = MergeRequest::new(
            4, TreeId::new(), BranchId::new(), "feature/a",
            BranchId::new(), "main", "Feature A", AccountId::new(),
        );

        // First review: changes requested
        mr.add_review(Review {
            reviewer,
            status: ReviewStatus::ChangesRequested,
            comment: None,
            reviewed_at: Utc::now(),
            snapshot_reviewed: SnapshotId::new(),
            stale: false,
        });
        assert!(mr.has_changes_requested());

        // Second review from same person: approved (marks first as stale)
        mr.add_review(Review {
            reviewer,
            status: ReviewStatus::Approved,
            comment: None,
            reviewed_at: Utc::now(),
            snapshot_reviewed: SnapshotId::new(),
            stale: false,
        });
        assert!(!mr.has_changes_requested());
        assert_eq!(mr.approve_count(), 1);
    }
}
