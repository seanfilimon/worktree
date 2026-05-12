//! Sync protocol message types for communication between BGProcess and Server.

use crate::core::hash::ContentHash;
use crate::core::id::{AccountId, BranchId, SnapshotId, TenantId, TreeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================
// Sync Operations
// ============================================================

/// Types of sync messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncMessageType {
    // Staged snapshot sync
    StageUpload,
    StageAck,
    // Branch push/pull
    PushRequest,
    PushResponse,
    PullRequest,
    PullResponse,
    // Object transfer
    HaveWant,
    ObjectTransfer,
    // Access config sync
    AccessConfigSync,
    // Tag sync
    TagSync,
    // Chunk transfer (large files)
    ChunkUpload,
    ChunkDownload,
    // Health
    Ping,
    Pong,
}

// ============================================================
// Have/Want Negotiation (Delta Sync)
// ============================================================

/// Client announces which objects it has
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaveMessage {
    pub hashes: Vec<ContentHash>,
}

/// Client announces which objects it wants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WantMessage {
    pub hashes: Vec<ContentHash>,
}

/// Server responds with the set of objects to transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectTransferPlan {
    pub objects_to_send: Vec<ContentHash>,
    pub total_size: u64,
}

// ============================================================
// Push Protocol
// ============================================================

/// Request to push a branch tip to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub tenant_id: TenantId,
    pub tree_id: TreeId,
    pub branch_id: BranchId,
    pub branch_name: String,
    pub old_tip: Option<SnapshotId>,
    pub new_tip: SnapshotId,
    pub snapshot_chain: Vec<SnapshotId>,
    pub account_id: AccountId,
}

/// Server response to a push
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub accepted: bool,
    pub rejection_reason: Option<PushRejection>,
    pub new_tip: Option<SnapshotId>,
}

/// Reasons a push may be rejected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushRejection {
    ConflictDetected { server_tip: SnapshotId },
    BranchProtection { rule: String },
    AccessDenied { reason: String },
    LicenseViolation { path: String, license: String },
    CiChecksFailed { checks: Vec<String> },
    ReviewRequired { required: u32, current: u32 },
    QuotaExceeded { limit: String },
}

impl PushResponse {
    pub fn accepted(new_tip: SnapshotId) -> Self {
        Self {
            accepted: true,
            rejection_reason: None,
            new_tip: Some(new_tip),
        }
    }

    pub fn rejected(reason: PushRejection) -> Self {
        Self {
            accepted: false,
            rejection_reason: Some(reason),
            new_tip: None,
        }
    }
}

// ============================================================
// Pull Protocol
// ============================================================

/// Request to pull updates from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub tenant_id: TenantId,
    pub tree_id: TreeId,
    pub branch_id: BranchId,
    pub current_tip: Option<SnapshotId>,
    pub account_id: AccountId,
    pub depth: Option<u32>,
}

/// Server response with updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub has_updates: bool,
    pub new_tip: Option<SnapshotId>,
    pub snapshot_chain: Vec<SnapshotId>,
    pub objects_to_fetch: Vec<ContentHash>,
}

impl PullResponse {
    pub fn no_updates() -> Self {
        Self {
            has_updates: false,
            new_tip: None,
            snapshot_chain: Vec::new(),
            objects_to_fetch: Vec::new(),
        }
    }

    pub fn with_updates(
        new_tip: SnapshotId,
        chain: Vec<SnapshotId>,
        objects: Vec<ContentHash>,
    ) -> Self {
        Self {
            has_updates: true,
            new_tip: Some(new_tip),
            snapshot_chain: chain,
            objects_to_fetch: objects,
        }
    }
}

// ============================================================
// Staged Snapshot Upload
// ============================================================

/// Upload a staged snapshot to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageUploadRequest {
    pub tenant_id: TenantId,
    pub tree_id: TreeId,
    pub branch_id: BranchId,
    pub branch_name: String,
    pub snapshot_id: SnapshotId,
    pub files_changed: Vec<String>,
    pub files_added: u32,
    pub files_modified: u32,
    pub files_deleted: u32,
    pub message: Option<String>,
    pub account_id: AccountId,
}

/// Server acknowledgment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageUploadResponse {
    pub accepted: bool,
    pub error: Option<String>,
}

// ============================================================
// Access Config Sync
// ============================================================

/// Sync access configuration changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessConfigSyncRequest {
    pub tenant_id: TenantId,
    pub config_type: AccessConfigType,
    pub content: Vec<u8>,
    pub config_hash: ContentHash,
    pub account_id: AccountId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessConfigType {
    Roles,
    Policies,
    TenantAccess,
    BranchProtection,
    License,
}

/// Server validation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessConfigSyncResponse {
    pub accepted: bool,
    pub validation_errors: Vec<String>,
}

// ============================================================
// Sync State
// ============================================================

/// Tracks the sync state between client and server
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    pub last_sync: Option<DateTime<Utc>>,
    pub local_tip: Option<SnapshotId>,
    pub remote_tip: Option<SnapshotId>,
    pub pending_staged: u32,
    pub pending_objects: u32,
    pub is_syncing: bool,
    pub offline: bool,
}

impl SyncState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if local and remote tips differ (local may be behind OR ahead).
    /// Cannot determine direction without snapshot DAG traversal.
    pub fn is_out_of_sync(&self) -> bool {
        match (&self.local_tip, &self.remote_tip) {
            (Some(local), Some(remote)) => local != remote,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    pub fn is_ahead(&self) -> bool {
        self.pending_staged > 0 || self.pending_objects > 0
    }

    pub fn needs_sync(&self) -> bool {
        self.is_out_of_sync() || self.is_ahead()
    }
}

// ============================================================
// Sync Envelope (Wire Format)
// ============================================================

/// Envelope for sync messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub message_type: SyncMessageType,
    pub payload: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub sequence: u64,
}

impl SyncEnvelope {
    pub fn new(message_type: SyncMessageType, payload: Vec<u8>) -> Self {
        Self {
            message_type,
            payload,
            timestamp: Utc::now(),
            sequence: 0,
        }
    }

    pub fn with_sequence(mut self, seq: u64) -> Self {
        self.sequence = seq;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_response_accepted() {
        let resp = PushResponse::accepted(SnapshotId::new());
        assert!(resp.accepted);
        assert!(resp.new_tip.is_some());
        assert!(resp.rejection_reason.is_none());
    }

    #[test]
    fn test_push_response_rejected() {
        let resp = PushResponse::rejected(PushRejection::AccessDenied {
            reason: "no permission".to_string(),
        });
        assert!(!resp.accepted);
        assert!(resp.rejection_reason.is_some());
    }

    #[test]
    fn test_pull_response() {
        let resp = PullResponse::no_updates();
        assert!(!resp.has_updates);

        let resp = PullResponse::with_updates(SnapshotId::new(), vec![SnapshotId::new()], vec![]);
        assert!(resp.has_updates);
    }

    #[test]
    fn test_sync_state() {
        let mut state = SyncState::new();
        assert!(!state.needs_sync());

        state.remote_tip = Some(SnapshotId::new());
        assert!(state.is_out_of_sync());
        assert!(state.needs_sync());

        state.local_tip = state.remote_tip.clone();
        state.pending_staged = 2;
        assert!(state.is_ahead());
    }
}
