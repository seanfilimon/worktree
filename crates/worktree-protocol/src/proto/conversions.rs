//! Conversions between hand-coded domain types in [`crate::feature::sync_protocol`]
//! and codegen'd wire types in [`crate::proto::sync`].
//!
//! Bridges the two type layers introduced by WT-PROTO-1. Domain types are the
//! in-memory representation used by Rust call sites today; wire types are the
//! protobuf-shaped representation used at gRPC boundaries. Once all callers
//! migrate to wire types (WT-BG-4/5/6, WT-SRV-4), the domain layer deprecates
//! and these conversions delete with it.
//!
//! Conventions:
//! - `From<DomainType> for ProtoType` for domain→wire (infallible — UUIDs
//!   always stringify; required fields always present in domain land).
//! - `TryFrom<ProtoType> for DomainType` with `Error = ConversionError` for
//!   wire→domain (fallible — wire may have None for required-in-spirit
//!   fields, invalid UUIDs, missing oneof variants, out-of-range timestamps).
//! - `#[non_exhaustive]` on the error enum to allow additive evolution.

use std::str::FromStr;

use thiserror::Error;
use uuid::Uuid;

use crate::core::hash::ContentHash;
use crate::core::id::{AccountId, BranchId, SnapshotId, TenantId, TreeId};
use crate::proto::sync as wire;

/// Error type for fallible wire→domain conversions.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConversionError {
    /// A UUID-shaped wire string failed to parse.
    #[error("invalid UUID for `{field}`: {source}")]
    InvalidUuid {
        field: &'static str,
        #[source]
        source: uuid::Error,
    },

    /// A `bytes`-shaped wire ContentHash had the wrong length.
    #[error("invalid content hash for `{field}`: expected 32 bytes, got {actual}")]
    InvalidContentHash { field: &'static str, actual: usize },

    /// A required-in-spirit message field was None on the wire.
    /// (proto3 message fields are always `Option<T>` in prost codegen even
    /// when the .proto marks them as required.)
    #[error("required field `{0}` is missing")]
    MissingRequired(&'static str),

    /// A proto3 enum field had a value not recognized by the codegen'd enum
    /// (typically zero / UNSPECIFIED or an unknown future value).
    #[error("invalid `{enum_name}` value: {value}")]
    InvalidEnum { enum_name: &'static str, value: i32 },

    /// A google.protobuf.Timestamp had seconds/nanos out of representable range.
    #[error("invalid timestamp for `{field}`: {reason}")]
    InvalidTimestamp {
        field: &'static str,
        reason: &'static str,
    },

    /// A `oneof` wire field arrived with no variant set
    /// (`Option<...::Reason>` was None).
    #[error("oneof `{0}` has no variant set")]
    MissingOneofVariant(&'static str),
}

// ============================================================
// Helpers
// ============================================================

/// Convert a `chrono::DateTime<Utc>` into a prost-types Timestamp.
pub(crate) fn datetime_to_prost(dt: chrono::DateTime<chrono::Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// Convert a prost-types Timestamp back into `chrono::DateTime<Utc>`.
pub(crate) fn prost_to_datetime(
    ts: prost_types::Timestamp,
    field: &'static str,
) -> Result<chrono::DateTime<chrono::Utc>, ConversionError> {
    chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).ok_or(
        ConversionError::InvalidTimestamp {
            field,
            reason: "out of range",
        },
    )
}

// ============================================================
// ID type conversions
// ============================================================

// Macro to generate the boilerplate for UUID-backed ID types.
macro_rules! impl_uuid_id_conversions {
    ($domain:ty, $wire:ty, $field_name:literal) => {
        impl From<$domain> for $wire {
            fn from(id: $domain) -> Self {
                Self {
                    value: id.as_uuid().to_string(),
                }
            }
        }

        impl TryFrom<$wire> for $domain {
            type Error = ConversionError;

            fn try_from(w: $wire) -> Result<Self, Self::Error> {
                let uuid =
                    Uuid::from_str(&w.value).map_err(|source| ConversionError::InvalidUuid {
                        field: $field_name,
                        source,
                    })?;
                Ok(<$domain>::from_uuid(uuid))
            }
        }
    };
}

impl_uuid_id_conversions!(TenantId, wire::TenantId, "TenantId.value");
impl_uuid_id_conversions!(TreeId, wire::TreeId, "TreeId.value");
impl_uuid_id_conversions!(BranchId, wire::BranchId, "BranchId.value");
impl_uuid_id_conversions!(SnapshotId, wire::SnapshotId, "SnapshotId.value");
impl_uuid_id_conversions!(AccountId, wire::AccountId, "AccountId.value");

// ContentHash is bytes-backed (32 bytes BLAKE3).
impl From<ContentHash> for wire::ContentHash {
    fn from(h: ContentHash) -> Self {
        Self {
            value: h.as_bytes().to_vec(),
        }
    }
}

impl TryFrom<wire::ContentHash> for ContentHash {
    type Error = ConversionError;

    fn try_from(w: wire::ContentHash) -> Result<Self, Self::Error> {
        let arr: [u8; 32] =
            w.value
                .as_slice()
                .try_into()
                .map_err(|_| ConversionError::InvalidContentHash {
                    field: "ContentHash.value",
                    actual: w.value.len(),
                })?;
        Ok(ContentHash::from_bytes(arr))
    }
}

// ============================================================
// Helper: collect a Vec via element-wise TryFrom
// ============================================================

/// Collect a `Vec<W>` of wire elements into a `Vec<D>` of domain elements,
/// short-circuiting on the first conversion error.
fn try_collect_vec<W, D>(items: Vec<W>) -> Result<Vec<D>, ConversionError>
where
    D: TryFrom<W, Error = ConversionError>,
{
    items.into_iter().map(D::try_from).collect()
}

/// Unwrap a wire `Option<W>` produced for a required-in-spirit message field
/// and convert it into the domain type `D`.
fn required<W, D>(opt: Option<W>, field: &'static str) -> Result<D, ConversionError>
where
    D: TryFrom<W, Error = ConversionError>,
{
    let raw = opt.ok_or(ConversionError::MissingRequired(field))?;
    D::try_from(raw)
}

// ============================================================
// Enum conversions: SyncMessageType, AccessConfigType
// ============================================================

use crate::feature::sync_protocol as dom;

impl From<dom::SyncMessageType> for wire::SyncMessageType {
    fn from(d: dom::SyncMessageType) -> Self {
        use dom::SyncMessageType::*;
        match d {
            StageUpload => Self::StageUpload,
            StageAck => Self::StageAck,
            PushRequest => Self::PushRequest,
            PushResponse => Self::PushResponse,
            PullRequest => Self::PullRequest,
            PullResponse => Self::PullResponse,
            HaveWant => Self::HaveWant,
            ObjectTransfer => Self::ObjectTransfer,
            AccessConfigSync => Self::AccessConfigSync,
            TagSync => Self::TagSync,
            ChunkUpload => Self::ChunkUpload,
            ChunkDownload => Self::ChunkDownload,
            Ping => Self::Ping,
            Pong => Self::Pong,
        }
    }
}

impl TryFrom<wire::SyncMessageType> for dom::SyncMessageType {
    type Error = ConversionError;

    fn try_from(w: wire::SyncMessageType) -> Result<Self, Self::Error> {
        use wire::SyncMessageType::*;
        Ok(match w {
            Unspecified => {
                return Err(ConversionError::InvalidEnum {
                    enum_name: "SyncMessageType",
                    value: 0,
                });
            }
            StageUpload => Self::StageUpload,
            StageAck => Self::StageAck,
            PushRequest => Self::PushRequest,
            PushResponse => Self::PushResponse,
            PullRequest => Self::PullRequest,
            PullResponse => Self::PullResponse,
            HaveWant => Self::HaveWant,
            ObjectTransfer => Self::ObjectTransfer,
            AccessConfigSync => Self::AccessConfigSync,
            TagSync => Self::TagSync,
            ChunkUpload => Self::ChunkUpload,
            ChunkDownload => Self::ChunkDownload,
            Ping => Self::Ping,
            Pong => Self::Pong,
        })
    }
}

impl From<dom::AccessConfigType> for wire::AccessConfigType {
    fn from(d: dom::AccessConfigType) -> Self {
        use dom::AccessConfigType::*;
        match d {
            Roles => Self::Roles,
            Policies => Self::Policies,
            TenantAccess => Self::TenantAccess,
            BranchProtection => Self::BranchProtection,
            License => Self::License,
        }
    }
}

impl TryFrom<wire::AccessConfigType> for dom::AccessConfigType {
    type Error = ConversionError;

    fn try_from(w: wire::AccessConfigType) -> Result<Self, Self::Error> {
        use wire::AccessConfigType::*;
        Ok(match w {
            Unspecified => {
                return Err(ConversionError::InvalidEnum {
                    enum_name: "AccessConfigType",
                    value: 0,
                });
            }
            Roles => Self::Roles,
            Policies => Self::Policies,
            TenantAccess => Self::TenantAccess,
            BranchProtection => Self::BranchProtection,
            License => Self::License,
        })
    }
}

// ============================================================
// Have/Want negotiation messages
// ============================================================

impl From<dom::HaveMessage> for wire::HaveMessage {
    fn from(d: dom::HaveMessage) -> Self {
        Self {
            hashes: d.hashes.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<wire::HaveMessage> for dom::HaveMessage {
    type Error = ConversionError;

    fn try_from(w: wire::HaveMessage) -> Result<Self, Self::Error> {
        Ok(Self {
            hashes: try_collect_vec(w.hashes)?,
        })
    }
}

impl From<dom::WantMessage> for wire::WantMessage {
    fn from(d: dom::WantMessage) -> Self {
        Self {
            hashes: d.hashes.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<wire::WantMessage> for dom::WantMessage {
    type Error = ConversionError;

    fn try_from(w: wire::WantMessage) -> Result<Self, Self::Error> {
        Ok(Self {
            hashes: try_collect_vec(w.hashes)?,
        })
    }
}

impl From<dom::ObjectTransferPlan> for wire::ObjectTransferPlan {
    fn from(d: dom::ObjectTransferPlan) -> Self {
        Self {
            objects_to_send: d.objects_to_send.into_iter().map(Into::into).collect(),
            total_size: d.total_size,
        }
    }
}

impl TryFrom<wire::ObjectTransferPlan> for dom::ObjectTransferPlan {
    type Error = ConversionError;

    fn try_from(w: wire::ObjectTransferPlan) -> Result<Self, Self::Error> {
        Ok(Self {
            objects_to_send: try_collect_vec(w.objects_to_send)?,
            total_size: w.total_size,
        })
    }
}

// ============================================================
// Push request (PushResponse + PushRejection in commit 3)
// ============================================================

impl From<dom::PushRequest> for wire::PushRequest {
    fn from(d: dom::PushRequest) -> Self {
        Self {
            tenant_id: Some(d.tenant_id.into()),
            tree_id: Some(d.tree_id.into()),
            branch_id: Some(d.branch_id.into()),
            branch_name: d.branch_name,
            old_tip: d.old_tip.map(Into::into),
            new_tip: Some(d.new_tip.into()),
            snapshot_chain: d.snapshot_chain.into_iter().map(Into::into).collect(),
            account_id: Some(d.account_id.into()),
        }
    }
}

impl TryFrom<wire::PushRequest> for dom::PushRequest {
    type Error = ConversionError;

    fn try_from(w: wire::PushRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            tenant_id: required(w.tenant_id, "PushRequest.tenant_id")?,
            tree_id: required(w.tree_id, "PushRequest.tree_id")?,
            branch_id: required(w.branch_id, "PushRequest.branch_id")?,
            branch_name: w.branch_name,
            old_tip: w.old_tip.map(SnapshotId::try_from).transpose()?,
            new_tip: required(w.new_tip, "PushRequest.new_tip")?,
            snapshot_chain: try_collect_vec(w.snapshot_chain)?,
            account_id: required(w.account_id, "PushRequest.account_id")?,
        })
    }
}

// ============================================================
// Pull request + response
// ============================================================

impl From<dom::PullRequest> for wire::PullRequest {
    fn from(d: dom::PullRequest) -> Self {
        Self {
            tenant_id: Some(d.tenant_id.into()),
            tree_id: Some(d.tree_id.into()),
            branch_id: Some(d.branch_id.into()),
            current_tip: d.current_tip.map(Into::into),
            account_id: Some(d.account_id.into()),
            depth: d.depth,
        }
    }
}

impl TryFrom<wire::PullRequest> for dom::PullRequest {
    type Error = ConversionError;

    fn try_from(w: wire::PullRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            tenant_id: required(w.tenant_id, "PullRequest.tenant_id")?,
            tree_id: required(w.tree_id, "PullRequest.tree_id")?,
            branch_id: required(w.branch_id, "PullRequest.branch_id")?,
            current_tip: w.current_tip.map(SnapshotId::try_from).transpose()?,
            account_id: required(w.account_id, "PullRequest.account_id")?,
            depth: w.depth,
        })
    }
}

impl From<dom::PullResponse> for wire::PullResponse {
    fn from(d: dom::PullResponse) -> Self {
        Self {
            has_updates: d.has_updates,
            new_tip: d.new_tip.map(Into::into),
            snapshot_chain: d.snapshot_chain.into_iter().map(Into::into).collect(),
            objects_to_fetch: d.objects_to_fetch.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<wire::PullResponse> for dom::PullResponse {
    type Error = ConversionError;

    fn try_from(w: wire::PullResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            has_updates: w.has_updates,
            new_tip: w.new_tip.map(SnapshotId::try_from).transpose()?,
            snapshot_chain: try_collect_vec(w.snapshot_chain)?,
            objects_to_fetch: try_collect_vec(w.objects_to_fetch)?,
        })
    }
}

// ============================================================
// Stage upload request + response
// ============================================================

impl From<dom::StageUploadRequest> for wire::StageUploadRequest {
    fn from(d: dom::StageUploadRequest) -> Self {
        Self {
            tenant_id: Some(d.tenant_id.into()),
            tree_id: Some(d.tree_id.into()),
            branch_id: Some(d.branch_id.into()),
            branch_name: d.branch_name,
            snapshot_id: Some(d.snapshot_id.into()),
            files_changed: d.files_changed,
            files_added: d.files_added,
            files_modified: d.files_modified,
            files_deleted: d.files_deleted,
            message: d.message,
            account_id: Some(d.account_id.into()),
        }
    }
}

impl TryFrom<wire::StageUploadRequest> for dom::StageUploadRequest {
    type Error = ConversionError;

    fn try_from(w: wire::StageUploadRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            tenant_id: required(w.tenant_id, "StageUploadRequest.tenant_id")?,
            tree_id: required(w.tree_id, "StageUploadRequest.tree_id")?,
            branch_id: required(w.branch_id, "StageUploadRequest.branch_id")?,
            branch_name: w.branch_name,
            snapshot_id: required(w.snapshot_id, "StageUploadRequest.snapshot_id")?,
            files_changed: w.files_changed,
            files_added: w.files_added,
            files_modified: w.files_modified,
            files_deleted: w.files_deleted,
            message: w.message,
            account_id: required(w.account_id, "StageUploadRequest.account_id")?,
        })
    }
}

impl From<dom::StageUploadResponse> for wire::StageUploadResponse {
    fn from(d: dom::StageUploadResponse) -> Self {
        Self {
            accepted: d.accepted,
            error: d.error,
        }
    }
}

impl TryFrom<wire::StageUploadResponse> for dom::StageUploadResponse {
    type Error = ConversionError;

    fn try_from(w: wire::StageUploadResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            accepted: w.accepted,
            error: w.error,
        })
    }
}

// ============================================================
// Access config sync request + response
// ============================================================

impl From<dom::AccessConfigSyncRequest> for wire::AccessConfigSyncRequest {
    fn from(d: dom::AccessConfigSyncRequest) -> Self {
        Self {
            tenant_id: Some(d.tenant_id.into()),
            config_type: wire::AccessConfigType::from(d.config_type) as i32,
            content: d.content,
            config_hash: Some(d.config_hash.into()),
            account_id: Some(d.account_id.into()),
        }
    }
}

impl TryFrom<wire::AccessConfigSyncRequest> for dom::AccessConfigSyncRequest {
    type Error = ConversionError;

    fn try_from(w: wire::AccessConfigSyncRequest) -> Result<Self, Self::Error> {
        let cfg_wire = wire::AccessConfigType::try_from(w.config_type).map_err(|_| {
            ConversionError::InvalidEnum {
                enum_name: "AccessConfigType",
                value: w.config_type,
            }
        })?;
        Ok(Self {
            tenant_id: required(w.tenant_id, "AccessConfigSyncRequest.tenant_id")?,
            config_type: cfg_wire.try_into()?,
            content: w.content,
            config_hash: required(w.config_hash, "AccessConfigSyncRequest.config_hash")?,
            account_id: required(w.account_id, "AccessConfigSyncRequest.account_id")?,
        })
    }
}

impl From<dom::AccessConfigSyncResponse> for wire::AccessConfigSyncResponse {
    fn from(d: dom::AccessConfigSyncResponse) -> Self {
        Self {
            accepted: d.accepted,
            validation_errors: d.validation_errors,
        }
    }
}

impl TryFrom<wire::AccessConfigSyncResponse> for dom::AccessConfigSyncResponse {
    type Error = ConversionError;

    fn try_from(w: wire::AccessConfigSyncResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            accepted: w.accepted,
            validation_errors: w.validation_errors,
        })
    }
}

// ============================================================
// Sync state + sync envelope (use timestamp helpers + enum)
// ============================================================

impl From<dom::SyncState> for wire::SyncState {
    fn from(d: dom::SyncState) -> Self {
        Self {
            last_sync: d.last_sync.map(datetime_to_prost),
            local_tip: d.local_tip.map(Into::into),
            remote_tip: d.remote_tip.map(Into::into),
            pending_staged: d.pending_staged,
            pending_objects: d.pending_objects,
            is_syncing: d.is_syncing,
            offline: d.offline,
        }
    }
}

impl TryFrom<wire::SyncState> for dom::SyncState {
    type Error = ConversionError;

    fn try_from(w: wire::SyncState) -> Result<Self, Self::Error> {
        Ok(Self {
            last_sync: w
                .last_sync
                .map(|ts| prost_to_datetime(ts, "SyncState.last_sync"))
                .transpose()?,
            local_tip: w.local_tip.map(SnapshotId::try_from).transpose()?,
            remote_tip: w.remote_tip.map(SnapshotId::try_from).transpose()?,
            pending_staged: w.pending_staged,
            pending_objects: w.pending_objects,
            is_syncing: w.is_syncing,
            offline: w.offline,
        })
    }
}

impl From<dom::SyncEnvelope> for wire::SyncEnvelope {
    fn from(d: dom::SyncEnvelope) -> Self {
        Self {
            message_type: wire::SyncMessageType::from(d.message_type) as i32,
            payload: d.payload,
            timestamp: Some(datetime_to_prost(d.timestamp)),
            sequence: d.sequence,
        }
    }
}

impl TryFrom<wire::SyncEnvelope> for dom::SyncEnvelope {
    type Error = ConversionError;

    fn try_from(w: wire::SyncEnvelope) -> Result<Self, Self::Error> {
        let mt_wire = wire::SyncMessageType::try_from(w.message_type).map_err(|_| {
            ConversionError::InvalidEnum {
                enum_name: "SyncMessageType",
                value: w.message_type,
            }
        })?;
        Ok(Self {
            message_type: mt_wire.try_into()?,
            payload: w.payload,
            timestamp: prost_to_datetime(
                w.timestamp
                    .ok_or(ConversionError::MissingRequired("SyncEnvelope.timestamp"))?,
                "SyncEnvelope.timestamp",
            )?,
            sequence: w.sequence,
        })
    }
}

// ============================================================
// PushRejection (oneof) + PushResponse
// ============================================================

impl From<dom::PushRejection> for wire::PushRejection {
    fn from(d: dom::PushRejection) -> Self {
        use dom::PushRejection::*;
        use wire::push_rejection as pr;
        let reason = match d {
            ConflictDetected { server_tip } => pr::Reason::ConflictDetected(pr::ConflictDetected {
                server_tip: Some(server_tip.into()),
            }),
            BranchProtection { rule } => {
                pr::Reason::BranchProtection(pr::BranchProtection { rule })
            }
            AccessDenied { reason } => pr::Reason::AccessDenied(pr::AccessDenied { reason }),
            LicenseViolation { path, license } => {
                pr::Reason::LicenseViolation(pr::LicenseViolation { path, license })
            }
            CiChecksFailed { checks } => pr::Reason::CiChecksFailed(pr::CiChecksFailed { checks }),
            ReviewRequired { required, current } => {
                pr::Reason::ReviewRequired(pr::ReviewRequired { required, current })
            }
            QuotaExceeded { limit } => pr::Reason::QuotaExceeded(pr::QuotaExceeded { limit }),
        };
        Self {
            reason: Some(reason),
        }
    }
}

impl TryFrom<wire::PushRejection> for dom::PushRejection {
    type Error = ConversionError;

    fn try_from(w: wire::PushRejection) -> Result<Self, Self::Error> {
        use wire::push_rejection::Reason;
        let reason = w
            .reason
            .ok_or(ConversionError::MissingOneofVariant("PushRejection.reason"))?;
        Ok(match reason {
            Reason::ConflictDetected(v) => dom::PushRejection::ConflictDetected {
                server_tip: required(v.server_tip, "PushRejection.ConflictDetected.server_tip")?,
            },
            Reason::BranchProtection(v) => dom::PushRejection::BranchProtection { rule: v.rule },
            Reason::AccessDenied(v) => dom::PushRejection::AccessDenied { reason: v.reason },
            Reason::LicenseViolation(v) => dom::PushRejection::LicenseViolation {
                path: v.path,
                license: v.license,
            },
            Reason::CiChecksFailed(v) => dom::PushRejection::CiChecksFailed { checks: v.checks },
            Reason::ReviewRequired(v) => dom::PushRejection::ReviewRequired {
                required: v.required,
                current: v.current,
            },
            Reason::QuotaExceeded(v) => dom::PushRejection::QuotaExceeded { limit: v.limit },
        })
    }
}

impl From<dom::PushResponse> for wire::PushResponse {
    fn from(d: dom::PushResponse) -> Self {
        Self {
            accepted: d.accepted,
            rejection_reason: d.rejection_reason.map(Into::into),
            new_tip: d.new_tip.map(Into::into),
        }
    }
}

impl TryFrom<wire::PushResponse> for dom::PushResponse {
    type Error = ConversionError;

    fn try_from(w: wire::PushResponse) -> Result<Self, Self::Error> {
        Ok(dom::PushResponse {
            accepted: w.accepted,
            rejection_reason: w
                .rejection_reason
                .map(dom::PushRejection::try_from)
                .transpose()?,
            new_tip: w.new_tip.map(SnapshotId::try_from).transpose()?,
        })
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;

    // ID roundtrips

    #[test]
    fn tenant_id_roundtrip() {
        let id = TenantId::new();
        let wire: wire::TenantId = id.into();
        let back: TenantId = wire.try_into().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn tree_id_roundtrip() {
        let id = TreeId::new();
        let wire: wire::TreeId = id.into();
        let back: TreeId = wire.try_into().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn branch_id_roundtrip() {
        let id = BranchId::new();
        let wire: wire::BranchId = id.into();
        let back: BranchId = wire.try_into().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn snapshot_id_roundtrip() {
        let id = SnapshotId::new();
        let wire: wire::SnapshotId = id.into();
        let back: SnapshotId = wire.try_into().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn account_id_roundtrip() {
        let id = AccountId::new();
        let wire: wire::AccountId = id.into();
        let back: AccountId = wire.try_into().unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn content_hash_roundtrip() {
        let h = hash_bytes(b"hello worktree");
        let wire: wire::ContentHash = h.into();
        let back: ContentHash = wire.try_into().unwrap();
        assert_eq!(h, back);
    }

    // Error cases

    #[test]
    fn tenant_id_invalid_uuid_error() {
        let wire = wire::TenantId {
            value: "not-a-uuid".into(),
        };
        let err = TenantId::try_from(wire).unwrap_err();
        match err {
            ConversionError::InvalidUuid { field, .. } => {
                assert_eq!(field, "TenantId.value");
            }
            _ => panic!("expected InvalidUuid, got {err:?}"),
        }
    }

    #[test]
    fn content_hash_wrong_length_error() {
        let wire = wire::ContentHash {
            value: vec![0u8; 31],
        };
        let err = ContentHash::try_from(wire).unwrap_err();
        match err {
            ConversionError::InvalidContentHash { field, actual } => {
                assert_eq!(field, "ContentHash.value");
                assert_eq!(actual, 31);
            }
            _ => panic!("expected InvalidContentHash, got {err:?}"),
        }
    }

    // Timestamp helpers

    #[test]
    fn datetime_roundtrip() {
        let now = chrono::Utc::now();
        let prost = datetime_to_prost(now);
        let back = prost_to_datetime(prost, "test").unwrap();
        // chrono nanos precision is preserved
        assert_eq!(now, back);
    }

    // Helpers for tests below
    fn sample_have_message() -> dom::HaveMessage {
        dom::HaveMessage {
            hashes: vec![hash_bytes(b"a"), hash_bytes(b"b")],
        }
    }

    fn sample_push_request() -> dom::PushRequest {
        dom::PushRequest {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            branch_id: BranchId::new(),
            branch_name: "main".into(),
            old_tip: Some(SnapshotId::new()),
            new_tip: SnapshotId::new(),
            snapshot_chain: vec![SnapshotId::new(), SnapshotId::new()],
            account_id: AccountId::new(),
        }
    }

    // Enum conversions

    #[test]
    fn sync_message_type_roundtrip_all_variants() {
        use dom::SyncMessageType::*;
        for d in [
            StageUpload,
            StageAck,
            PushRequest,
            PushResponse,
            PullRequest,
            PullResponse,
            HaveWant,
            ObjectTransfer,
            AccessConfigSync,
            TagSync,
            ChunkUpload,
            ChunkDownload,
            Ping,
            Pong,
        ] {
            let w: wire::SyncMessageType = d.clone().into();
            let back: dom::SyncMessageType = w.try_into().unwrap();
            assert_eq!(d, back);
        }
    }

    #[test]
    fn sync_message_type_unspecified_is_error() {
        let err = dom::SyncMessageType::try_from(wire::SyncMessageType::Unspecified).unwrap_err();
        match err {
            ConversionError::InvalidEnum { enum_name, value } => {
                assert_eq!(enum_name, "SyncMessageType");
                assert_eq!(value, 0);
            }
            _ => panic!("expected InvalidEnum, got {err:?}"),
        }
    }

    #[test]
    fn access_config_type_roundtrip_all_variants() {
        use dom::AccessConfigType::*;
        for d in [Roles, Policies, TenantAccess, BranchProtection, License] {
            let w: wire::AccessConfigType = d.clone().into();
            let back: dom::AccessConfigType = w.try_into().unwrap();
            assert_eq!(d, back);
        }
    }

    // Have/Want messages

    #[test]
    fn have_message_roundtrip() {
        let d = sample_have_message();
        let w: wire::HaveMessage = d.clone().into();
        let back: dom::HaveMessage = w.try_into().unwrap();
        assert_eq!(d.hashes, back.hashes);
    }

    #[test]
    fn want_message_roundtrip() {
        let d = dom::WantMessage {
            hashes: vec![hash_bytes(b"x")],
        };
        let w: wire::WantMessage = d.clone().into();
        let back: dom::WantMessage = w.try_into().unwrap();
        assert_eq!(d.hashes, back.hashes);
    }

    #[test]
    fn object_transfer_plan_roundtrip() {
        let d = dom::ObjectTransferPlan {
            objects_to_send: vec![hash_bytes(b"obj1"), hash_bytes(b"obj2")],
            total_size: 12345,
        };
        let w: wire::ObjectTransferPlan = d.clone().into();
        let back: dom::ObjectTransferPlan = w.try_into().unwrap();
        assert_eq!(d.objects_to_send, back.objects_to_send);
        assert_eq!(d.total_size, back.total_size);
    }

    // Push request

    #[test]
    fn push_request_roundtrip() {
        let d = sample_push_request();
        let w: wire::PushRequest = d.clone().into();
        let back: dom::PushRequest = w.try_into().unwrap();
        assert_eq!(d.tenant_id, back.tenant_id);
        assert_eq!(d.tree_id, back.tree_id);
        assert_eq!(d.branch_id, back.branch_id);
        assert_eq!(d.branch_name, back.branch_name);
        assert_eq!(d.old_tip, back.old_tip);
        assert_eq!(d.new_tip, back.new_tip);
        assert_eq!(d.snapshot_chain, back.snapshot_chain);
        assert_eq!(d.account_id, back.account_id);
    }

    #[test]
    fn push_request_missing_tenant_id_error() {
        let w = wire::PushRequest {
            tenant_id: None,
            tree_id: Some(TreeId::new().into()),
            branch_id: Some(BranchId::new().into()),
            branch_name: "main".into(),
            old_tip: None,
            new_tip: Some(SnapshotId::new().into()),
            snapshot_chain: vec![],
            account_id: Some(AccountId::new().into()),
        };
        let err = dom::PushRequest::try_from(w).unwrap_err();
        match err {
            ConversionError::MissingRequired(field) => {
                assert_eq!(field, "PushRequest.tenant_id");
            }
            _ => panic!("expected MissingRequired, got {err:?}"),
        }
    }

    // Pull request + response

    #[test]
    fn pull_request_roundtrip() {
        let d = dom::PullRequest {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            branch_id: BranchId::new(),
            current_tip: None,
            account_id: AccountId::new(),
            depth: Some(10),
        };
        let w: wire::PullRequest = d.clone().into();
        let back: dom::PullRequest = w.try_into().unwrap();
        assert_eq!(d.tenant_id, back.tenant_id);
        assert_eq!(d.current_tip, back.current_tip);
        assert_eq!(d.depth, back.depth);
    }

    #[test]
    fn pull_response_with_updates_roundtrip() {
        let d = dom::PullResponse::with_updates(
            SnapshotId::new(),
            vec![SnapshotId::new()],
            vec![hash_bytes(b"obj")],
        );
        let w: wire::PullResponse = d.clone().into();
        let back: dom::PullResponse = w.try_into().unwrap();
        assert_eq!(d.has_updates, back.has_updates);
        assert_eq!(d.new_tip, back.new_tip);
    }

    #[test]
    fn pull_response_no_updates_roundtrip() {
        let d = dom::PullResponse::no_updates();
        let w: wire::PullResponse = d.clone().into();
        let back: dom::PullResponse = w.try_into().unwrap();
        assert!(!back.has_updates);
        assert!(back.new_tip.is_none());
    }

    // Stage upload

    #[test]
    fn stage_upload_request_roundtrip() {
        let d = dom::StageUploadRequest {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            branch_id: BranchId::new(),
            branch_name: "feature/foo".into(),
            snapshot_id: SnapshotId::new(),
            files_changed: vec!["src/main.rs".into(), "README.md".into()],
            files_added: 1,
            files_modified: 1,
            files_deleted: 0,
            message: Some("wip".into()),
            account_id: AccountId::new(),
        };
        let w: wire::StageUploadRequest = d.clone().into();
        let back: dom::StageUploadRequest = w.try_into().unwrap();
        assert_eq!(d.tenant_id, back.tenant_id);
        assert_eq!(d.branch_name, back.branch_name);
        assert_eq!(d.files_changed, back.files_changed);
        assert_eq!(d.message, back.message);
    }

    #[test]
    fn stage_upload_response_roundtrip() {
        let d = dom::StageUploadResponse {
            accepted: false,
            error: Some("quota".into()),
        };
        let w: wire::StageUploadResponse = d.clone().into();
        let back: dom::StageUploadResponse = w.try_into().unwrap();
        assert_eq!(d.accepted, back.accepted);
        assert_eq!(d.error, back.error);
    }

    // Access config sync

    #[test]
    fn access_config_sync_request_roundtrip() {
        let d = dom::AccessConfigSyncRequest {
            tenant_id: TenantId::new(),
            config_type: dom::AccessConfigType::Roles,
            content: b"<roles toml>".to_vec(),
            config_hash: hash_bytes(b"<roles toml>"),
            account_id: AccountId::new(),
        };
        let w: wire::AccessConfigSyncRequest = d.clone().into();
        let back: dom::AccessConfigSyncRequest = w.try_into().unwrap();
        assert_eq!(d.tenant_id, back.tenant_id);
        assert_eq!(d.config_type, back.config_type);
        assert_eq!(d.content, back.content);
        assert_eq!(d.config_hash, back.config_hash);
    }

    #[test]
    fn access_config_sync_response_roundtrip() {
        let d = dom::AccessConfigSyncResponse {
            accepted: true,
            validation_errors: vec![],
        };
        let w: wire::AccessConfigSyncResponse = d.clone().into();
        let back: dom::AccessConfigSyncResponse = w.try_into().unwrap();
        assert_eq!(d.accepted, back.accepted);
        assert_eq!(d.validation_errors, back.validation_errors);
    }

    // Sync state + sync envelope

    #[test]
    fn sync_state_roundtrip() {
        let d = dom::SyncState {
            last_sync: Some(chrono::Utc::now()),
            local_tip: Some(SnapshotId::new()),
            remote_tip: Some(SnapshotId::new()),
            pending_staged: 3,
            pending_objects: 7,
            is_syncing: true,
            offline: false,
        };
        let w: wire::SyncState = d.clone().into();
        let back: dom::SyncState = w.try_into().unwrap();
        assert_eq!(d.local_tip, back.local_tip);
        assert_eq!(d.pending_staged, back.pending_staged);
        assert_eq!(d.is_syncing, back.is_syncing);
    }

    #[test]
    fn sync_envelope_roundtrip() {
        let d = dom::SyncEnvelope {
            message_type: dom::SyncMessageType::StageUpload,
            payload: vec![1, 2, 3, 4],
            timestamp: chrono::Utc::now(),
            sequence: 42,
        };
        let w: wire::SyncEnvelope = d.clone().into();
        let back: dom::SyncEnvelope = w.try_into().unwrap();
        assert_eq!(d.message_type, back.message_type);
        assert_eq!(d.payload, back.payload);
        assert_eq!(d.sequence, back.sequence);
    }

    // PushRejection oneof variant roundtrips

    fn push_rejection_roundtrip(d: dom::PushRejection) {
        let w: wire::PushRejection = d.clone().into();
        let back: dom::PushRejection = w.try_into().unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn push_rejection_conflict_detected_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::ConflictDetected {
            server_tip: SnapshotId::new(),
        });
    }

    #[test]
    fn push_rejection_branch_protection_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::BranchProtection {
            rule: "no-force-push-to-main".to_string(),
        });
    }

    #[test]
    fn push_rejection_access_denied_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::AccessDenied {
            reason: "account lacks write permission".to_string(),
        });
    }

    #[test]
    fn push_rejection_license_violation_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::LicenseViolation {
            path: "vendor/forbidden.rs".to_string(),
            license: "GPL-3.0".to_string(),
        });
    }

    #[test]
    fn push_rejection_ci_checks_failed_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::CiChecksFailed {
            checks: vec!["clippy".to_string(), "fmt".to_string()],
        });
    }

    #[test]
    fn push_rejection_review_required_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::ReviewRequired {
            required: 2,
            current: 1,
        });
    }

    #[test]
    fn push_rejection_quota_exceeded_roundtrip() {
        push_rejection_roundtrip(dom::PushRejection::QuotaExceeded {
            limit: "100GB".to_string(),
        });
    }

    #[test]
    fn push_rejection_missing_oneof_variant_error() {
        let w = wire::PushRejection { reason: None };
        let err = dom::PushRejection::try_from(w).unwrap_err();
        match err {
            ConversionError::MissingOneofVariant(field) => {
                assert_eq!(field, "PushRejection.reason");
            }
            other => panic!("expected MissingOneofVariant, got {:?}", other),
        }
    }

    // PushResponse roundtrips

    #[test]
    fn push_response_accepted_roundtrip() {
        let new_tip = SnapshotId::new();
        let d = dom::PushResponse::accepted(new_tip);
        let w: wire::PushResponse = d.clone().into();
        let back: dom::PushResponse = w.try_into().unwrap();
        assert!(back.accepted);
        assert_eq!(back.new_tip, Some(new_tip));
        assert!(back.rejection_reason.is_none());
    }

    #[test]
    fn push_response_rejected_roundtrip() {
        let d = dom::PushResponse::rejected(dom::PushRejection::AccessDenied {
            reason: "denied".to_string(),
        });
        let w: wire::PushResponse = d.clone().into();
        let back: dom::PushResponse = w.try_into().unwrap();
        assert!(!back.accepted);
        assert!(back.new_tip.is_none());
        match back.rejection_reason {
            Some(dom::PushRejection::AccessDenied { reason }) => assert_eq!(reason, "denied"),
            other => panic!("expected AccessDenied, got {:?}", other),
        }
    }
}
