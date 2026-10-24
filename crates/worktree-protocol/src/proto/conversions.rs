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
//! - Where a wire message has no complete domain counterpart (the
//!   content-addressed object/record messages: `ObjectData`,
//!   `ManifestRecord`, `SnapshotRecord`, `TreeInfo`, `BranchInfo`,
//!   `StagedInfo`, `TagRecord`), conversions are inherent helper methods on
//!   the wire type that take the missing context as arguments, plus
//!   `From`/`TryFrom` impls where nothing is missing.
//! - `PushRequest.snapshots` / `PullResponse.snapshots` (`SnapshotRecord`s)
//!   have no counterpart in the legacy domain types: domain→wire leaves them
//!   empty and wire→domain drops them. Callers that need records build the
//!   wire messages directly.

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

    /// A manifest path violated the wire contract (relative, `/`-separated,
    /// no `.`/`..`/empty segments, sorted and unique within a record).
    #[error("invalid manifest path `{path}`: {reason}")]
    InvalidPath { path: String, reason: &'static str },

    /// A content-addressed object's bytes do not hash to its declared hash.
    #[error("object hash mismatch: declared {expected}, computed {actual}")]
    HashMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },

    /// An object of the wrong kind was supplied (e.g. a BLOB where a
    /// MANIFEST was expected).
    #[error("unexpected object kind: expected {expected}, got {actual}")]
    UnexpectedObjectKind {
        expected: &'static str,
        actual: &'static str,
    },

    /// Protobuf bytes (e.g. a MANIFEST object body) failed to decode.
    #[error("failed to decode {what}: {source}")]
    Decode {
        what: &'static str,
        #[source]
        source: prost::DecodeError,
    },

    /// The domain value cannot be represented on the wire.
    #[error("unsupported conversion: {0}")]
    Unsupported(&'static str),
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
    // protobuf requires 0 <= nanos < 1e9; chrono would otherwise accept
    // 1e9..2e9 as a leap-second encoding.
    let nanos = u32::try_from(ts.nanos)
        .ok()
        .filter(|n| *n < 1_000_000_000)
        .ok_or(ConversionError::InvalidTimestamp {
            field,
            reason: "nanos out of range",
        })?;
    chrono::DateTime::from_timestamp(ts.seconds, nanos).ok_or(ConversionError::InvalidTimestamp {
        field,
        reason: "out of range",
    })
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
            snapshots: Vec::new(),
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
            snapshots: Vec::new(),
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
// ContentHash helpers (borrowing conversions + hex)
// ============================================================

impl From<&ContentHash> for wire::ContentHash {
    fn from(h: &ContentHash) -> Self {
        (*h).into()
    }
}

impl TryFrom<&wire::ContentHash> for ContentHash {
    type Error = ConversionError;

    fn try_from(w: &wire::ContentHash) -> Result<Self, Self::Error> {
        ContentHash::from_slice(&w.value).ok_or(ConversionError::InvalidContentHash {
            field: "ContentHash.value",
            actual: w.value.len(),
        })
    }
}

impl wire::ContentHash {
    /// Convert to the domain hash without consuming `self`.
    pub fn to_domain(&self) -> Result<ContentHash, ConversionError> {
        ContentHash::try_from(self)
    }

    /// Lowercase hex of the raw bytes (whatever their length).
    pub fn to_hex(&self) -> String {
        self.value.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Convert a required wire hash field, naming the field on failure.
fn required_hash(
    hash: Option<&wire::ContentHash>,
    field: &'static str,
) -> Result<ContentHash, ConversionError> {
    let hash = hash.ok_or(ConversionError::MissingRequired(field))?;
    ContentHash::from_slice(&hash.value).ok_or(ConversionError::InvalidContentHash {
        field,
        actual: hash.value.len(),
    })
}

/// Parse a UUID-backed wire id by reference, naming the field on failure.
fn parse_uuid(value: &str, field: &'static str) -> Result<Uuid, ConversionError> {
    Uuid::from_str(value).map_err(|source| ConversionError::InvalidUuid { field, source })
}

fn required_snapshot_id(
    id: Option<&wire::SnapshotId>,
    field: &'static str,
) -> Result<SnapshotId, ConversionError> {
    let id = id.ok_or(ConversionError::MissingRequired(field))?;
    parse_uuid(&id.value, field).map(SnapshotId::from_uuid)
}

fn required_timestamp(
    ts: Option<&prost_types::Timestamp>,
    field: &'static str,
) -> Result<chrono::DateTime<chrono::Utc>, ConversionError> {
    let ts = ts.ok_or(ConversionError::MissingRequired(field))?;
    prost_to_datetime(*ts, field)
}

// ============================================================
// Manifest paths
// ============================================================

/// Validate a manifest path: non-empty, relative, `/`-separated, with no
/// empty, `.` or `..` segments, no backslashes, NUL bytes or drive prefixes.
///
/// This is the wire contract for `ManifestEntry.path`; receivers must call it
/// (directly or via [`wire::ManifestRecord::validate`]) before writing files,
/// since it is what prevents path traversal out of the worktree.
pub fn validate_manifest_path(path: &str) -> Result<(), ConversionError> {
    let invalid = |reason: &'static str| ConversionError::InvalidPath {
        path: path.to_string(),
        reason,
    };
    if path.is_empty() {
        return Err(invalid("empty path"));
    }
    if path.starts_with('/') {
        return Err(invalid("absolute path"));
    }
    if path.contains('\\') {
        return Err(invalid("backslash in path (use '/')"));
    }
    if path.contains('\0') {
        return Err(invalid("NUL byte in path"));
    }
    for (i, segment) in path.split('/').enumerate() {
        match segment {
            "" => return Err(invalid("empty path segment")),
            "." | ".." => return Err(invalid("'.' or '..' path segment")),
            s if i == 0 && s.len() == 2 && s.ends_with(':') => {
                return Err(invalid("drive prefix in path"))
            }
            _ => {}
        }
    }
    Ok(())
}

/// Render a domain path as a wire path (`/`-separated, UTF-8, validated).
fn path_to_wire(path: &std::path::Path) -> Result<String, ConversionError> {
    let s = path.to_str().ok_or_else(|| ConversionError::InvalidPath {
        path: path.to_string_lossy().into_owned(),
        reason: "path is not valid UTF-8",
    })?;
    let s = s.replace('\\', "/");
    validate_manifest_path(&s)?;
    Ok(s)
}

// ============================================================
// ManifestEntry / ManifestRecord  <->  object::manifest
// ============================================================

use crate::object::manifest::{EntryKind, Manifest, ManifestEntry as DomainManifestEntry};

/// Domain → wire. Only regular files are representable on the wire
/// (directories are implied by paths; symlinks are not supported by
/// `ManifestRecord`), so other kinds are rejected.
impl TryFrom<&DomainManifestEntry> for wire::ManifestEntry {
    type Error = ConversionError;

    fn try_from(e: &DomainManifestEntry) -> Result<Self, Self::Error> {
        if e.kind != EntryKind::File {
            return Err(ConversionError::Unsupported(
                "only file entries can be encoded in a ManifestRecord",
            ));
        }
        Ok(Self {
            path: path_to_wire(&e.path)?,
            blob_hash: Some(e.hash.into()),
            size: e.size,
            executable: e.executable,
        })
    }
}

/// Wire → domain. Produces a [`EntryKind::File`] entry after validating the
/// path and hash.
impl TryFrom<wire::ManifestEntry> for DomainManifestEntry {
    type Error = ConversionError;

    fn try_from(w: wire::ManifestEntry) -> Result<Self, Self::Error> {
        validate_manifest_path(&w.path)?;
        let hash = required_hash(w.blob_hash.as_ref(), "ManifestEntry.blob_hash")?;
        Ok(DomainManifestEntry::file(w.path, hash, w.size).with_executable(w.executable))
    }
}

/// Domain → wire. Directory entries are dropped (implied by file paths);
/// symlinks are rejected. Entries are sorted by path string (byte-wise, as
/// the wire contract and the Go server require — note this differs from
/// `Path` ordering) and must be unique.
impl TryFrom<&Manifest> for wire::ManifestRecord {
    type Error = ConversionError;

    fn try_from(m: &Manifest) -> Result<Self, Self::Error> {
        let entries = m
            .entries
            .iter()
            .filter(|e| e.kind != EntryKind::Directory)
            .map(wire::ManifestEntry::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let mut record = Self { entries };
        record.entries.sort_by(|a, b| a.path.cmp(&b.path));
        record.validate()?;
        Ok(record)
    }
}

impl wire::ManifestRecord {
    /// Build a canonical record: entries sorted by path; for duplicate paths
    /// the last entry wins.
    pub fn from_entries(entries: impl IntoIterator<Item = wire::ManifestEntry>) -> Self {
        let mut by_path = std::collections::BTreeMap::new();
        for entry in entries {
            by_path.insert(entry.path.clone(), entry);
        }
        Self {
            entries: by_path.into_values().collect(),
        }
    }

    /// Check the wire contract: every path valid, paths strictly increasing
    /// (sorted and unique), and every blob hash present and 32 bytes.
    pub fn validate(&self) -> Result<(), ConversionError> {
        let mut previous: Option<&str> = None;
        for entry in &self.entries {
            validate_manifest_path(&entry.path)?;
            required_hash(entry.blob_hash.as_ref(), "ManifestEntry.blob_hash")?;
            if let Some(prev) = previous {
                if prev >= entry.path.as_str() {
                    return Err(ConversionError::InvalidPath {
                        path: entry.path.clone(),
                        reason: if prev == entry.path {
                            "duplicate manifest path"
                        } else {
                            "manifest entries not sorted by path"
                        },
                    });
                }
            }
            previous = Some(&entry.path);
        }
        Ok(())
    }

    /// Protobuf encoding of this record — the exact bytes of the MANIFEST
    /// object.
    pub fn encode_object_bytes(&self) -> Vec<u8> {
        prost::Message::encode_to_vec(self)
    }

    /// The object hash of this manifest: BLAKE3 of its protobuf encoding.
    pub fn content_hash(&self) -> ContentHash {
        crate::core::hash::hash_bytes(&self.encode_object_bytes())
    }

    /// Wrap this record as a MANIFEST [`wire::ObjectData`].
    pub fn to_object(&self) -> wire::ObjectData {
        wire::ObjectData::new(wire::ObjectKind::Manifest, self.encode_object_bytes())
    }

    /// Decode a record from MANIFEST object bytes and validate it.
    pub fn decode_object_bytes(data: &[u8]) -> Result<Self, ConversionError> {
        let record: Self =
            prost::Message::decode(data).map_err(|source| ConversionError::Decode {
                what: "ManifestRecord",
                source,
            })?;
        record.validate()?;
        Ok(record)
    }

    /// Convert into a domain [`Manifest`] for `tree_id` (validated first).
    pub fn into_manifest(self, tree_id: TreeId) -> Result<Manifest, ConversionError> {
        self.validate()?;
        let mut manifest = Manifest::new(tree_id);
        for entry in self.entries {
            manifest.add_entry(entry.try_into()?);
        }
        Ok(manifest)
    }
}

// ============================================================
// ObjectData  <->  object::blob::Blob
// ============================================================

use crate::object::blob::Blob;

impl wire::ObjectData {
    /// Build an object of `kind` from its exact bytes, computing the hash.
    pub fn new(kind: wire::ObjectKind, data: Vec<u8>) -> Self {
        Self {
            hash: Some(crate::core::hash::hash_bytes(&data).into()),
            kind: kind as i32,
            data,
        }
    }

    /// A BLOB object holding raw file content.
    pub fn blob(data: Vec<u8>) -> Self {
        Self::new(wire::ObjectKind::Blob, data)
    }

    /// The object kind, rejecting UNSPECIFIED and unknown values.
    pub fn object_kind(&self) -> Result<wire::ObjectKind, ConversionError> {
        match wire::ObjectKind::try_from(self.kind) {
            Ok(wire::ObjectKind::Unspecified) | Err(_) => Err(ConversionError::InvalidEnum {
                enum_name: "ObjectKind",
                value: self.kind,
            }),
            Ok(kind) => Ok(kind),
        }
    }

    /// BLAKE3 of `data`.
    pub fn computed_hash(&self) -> ContentHash {
        crate::core::hash::hash_bytes(&self.data)
    }

    /// Check that `hash` is present and equals BLAKE3(`data`); returns it.
    pub fn verify(&self) -> Result<ContentHash, ConversionError> {
        let expected = required_hash(self.hash.as_ref(), "ObjectData.hash")?;
        let actual = self.computed_hash();
        if expected != actual {
            return Err(ConversionError::HashMismatch { expected, actual });
        }
        Ok(expected)
    }

    /// Verify and decode a MANIFEST object.
    pub fn decode_manifest(&self) -> Result<wire::ManifestRecord, ConversionError> {
        self.expect_kind(wire::ObjectKind::Manifest)?;
        self.verify()?;
        wire::ManifestRecord::decode_object_bytes(&self.data)
    }

    fn expect_kind(&self, expected: wire::ObjectKind) -> Result<(), ConversionError> {
        let actual = self.object_kind()?;
        if actual != expected {
            return Err(ConversionError::UnexpectedObjectKind {
                expected: expected.as_str_name(),
                actual: actual.as_str_name(),
            });
        }
        Ok(())
    }
}

impl From<Blob> for wire::ObjectData {
    fn from(b: Blob) -> Self {
        // Hash recomputed from the bytes: the wire hash must match the data
        // even if the domain blob was constructed inconsistently.
        wire::ObjectData::blob(b.content)
    }
}

/// Wire → domain. The object must be a BLOB whose hash matches its bytes.
impl TryFrom<wire::ObjectData> for Blob {
    type Error = ConversionError;

    fn try_from(w: wire::ObjectData) -> Result<Self, Self::Error> {
        w.expect_kind(wire::ObjectKind::Blob)?;
        w.verify()?;
        Ok(Blob::from_bytes(w.data))
    }
}

// ============================================================
// SnapshotRecord  <->  object::snapshot::Snapshot
// ============================================================

use crate::object::snapshot::Snapshot;

impl wire::SnapshotRecord {
    /// Domain → wire. The domain snapshot carries no branch name, so the
    /// caller supplies it; `author` is the account id string.
    pub fn from_snapshot(s: &Snapshot, branch_name: impl Into<String>) -> Self {
        Self {
            id: Some(s.id.into()),
            tree_id: Some(s.tree_id.into()),
            branch_name: branch_name.into(),
            parents: s.parents.iter().copied().map(Into::into).collect(),
            manifest_hash: Some(s.manifest_hash.into()),
            author: s.author.to_string(),
            message: s.message.clone(),
            created_at: Some(datetime_to_prost(s.timestamp)),
            auto_generated: s.auto_generated,
        }
    }

    /// Check required fields: id, tree id, parents (valid UUIDs), a 32-byte
    /// manifest hash and a valid `created_at`.
    pub fn validate(&self) -> Result<(), ConversionError> {
        self.snapshot_id()?;
        let tree = self
            .tree_id
            .as_ref()
            .ok_or(ConversionError::MissingRequired("SnapshotRecord.tree_id"))?;
        parse_uuid(&tree.value, "SnapshotRecord.tree_id")?;
        self.parent_ids()?;
        required_hash(self.manifest_hash.as_ref(), "SnapshotRecord.manifest_hash")?;
        required_timestamp(self.created_at.as_ref(), "SnapshotRecord.created_at")?;
        Ok(())
    }

    /// The snapshot id.
    pub fn snapshot_id(&self) -> Result<SnapshotId, ConversionError> {
        required_snapshot_id(self.id.as_ref(), "SnapshotRecord.id")
    }

    /// The parent ids, in order.
    pub fn parent_ids(&self) -> Result<Vec<SnapshotId>, ConversionError> {
        self.parents
            .iter()
            .map(|p| parse_uuid(&p.value, "SnapshotRecord.parents").map(SnapshotId::from_uuid))
            .collect()
    }

    /// Wire → domain with an explicitly resolved author (the wire `author`
    /// is usually a username, which the domain type cannot hold).
    pub fn to_snapshot_with_author(&self, author: AccountId) -> Result<Snapshot, ConversionError> {
        let tree = self
            .tree_id
            .as_ref()
            .ok_or(ConversionError::MissingRequired("SnapshotRecord.tree_id"))?;
        Ok(Snapshot {
            id: self.snapshot_id()?,
            tree_id: TreeId::from_uuid(parse_uuid(&tree.value, "SnapshotRecord.tree_id")?),
            manifest_hash: required_hash(
                self.manifest_hash.as_ref(),
                "SnapshotRecord.manifest_hash",
            )?,
            parents: self.parent_ids()?,
            message: self.message.clone(),
            author,
            timestamp: required_timestamp(self.created_at.as_ref(), "SnapshotRecord.created_at")?,
            auto_generated: self.auto_generated,
        })
    }
}

/// Wire → domain. Requires `author` to be an account id (as written by
/// [`wire::SnapshotRecord::from_snapshot`]); use
/// [`wire::SnapshotRecord::to_snapshot_with_author`] otherwise. The branch
/// name is not carried by the domain type.
impl TryFrom<wire::SnapshotRecord> for Snapshot {
    type Error = ConversionError;

    fn try_from(w: wire::SnapshotRecord) -> Result<Self, Self::Error> {
        let author = AccountId::from_uuid(parse_uuid(&w.author, "SnapshotRecord.author")?);
        w.to_snapshot_with_author(author)
    }
}

// ============================================================
// TreeInfo / BranchInfo typed accessors (+ Branch → BranchInfo)
// ============================================================

use crate::object::branch::Branch;

impl wire::TreeInfo {
    /// The server tree id.
    pub fn tree_id_domain(&self) -> Result<TreeId, ConversionError> {
        let id = self
            .tree_id
            .as_ref()
            .ok_or(ConversionError::MissingRequired("TreeInfo.tree_id"))?;
        parse_uuid(&id.value, "TreeInfo.tree_id").map(TreeId::from_uuid)
    }

    /// The owning tenant id.
    pub fn tenant_id_domain(&self) -> Result<TenantId, ConversionError> {
        let id = self
            .tenant_id
            .as_ref()
            .ok_or(ConversionError::MissingRequired("TreeInfo.tenant_id"))?;
        parse_uuid(&id.value, "TreeInfo.tenant_id").map(TenantId::from_uuid)
    }
}

impl wire::BranchInfo {
    /// Domain → wire. The domain branch does not track protection or its
    /// last update, so the caller supplies both.
    pub fn from_branch(
        branch: &Branch,
        protected: bool,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            name: branch.name.clone(),
            tip: Some(branch.tip.into()),
            protected,
            updated_at: Some(datetime_to_prost(updated_at)),
        }
    }

    /// The tip snapshot, or `None` for an empty branch.
    pub fn tip_id(&self) -> Result<Option<SnapshotId>, ConversionError> {
        self.tip
            .as_ref()
            .map(|t| parse_uuid(&t.value, "BranchInfo.tip").map(SnapshotId::from_uuid))
            .transpose()
    }

    /// When the branch tip last moved, if the server reported it.
    pub fn updated_at_utc(&self) -> Result<Option<chrono::DateTime<chrono::Utc>>, ConversionError> {
        self.updated_at
            .map(|ts| prost_to_datetime(ts, "BranchInfo.updated_at"))
            .transpose()
    }
}

// ============================================================
// StagedInfo  <->  object::staged::StagedSnapshot
// ============================================================

use crate::object::staged::{StagedSnapshot, StagedStatus};

impl wire::StagedInfo {
    /// Domain → wire with an explicit display author (e.g. a username).
    pub fn from_staged(s: &StagedSnapshot, author: impl Into<String>) -> Self {
        Self {
            snapshot_id: Some(s.id.into()),
            author: author.into(),
            branch_name: s.branch_name.clone(),
            files_changed: s.files_changed.clone(),
            files_added: s.files_added,
            files_modified: s.files_modified,
            files_deleted: s.files_deleted,
            message: s.message.clone(),
            staged_at: Some(datetime_to_prost(s.timestamp)),
        }
    }

    /// Wire → domain. The wire message carries neither the tree, the branch
    /// id nor the user id, so the caller supplies them. Status is `Staged`.
    pub fn to_staged(
        &self,
        user: AccountId,
        tree_id: TreeId,
        branch_id: BranchId,
    ) -> Result<StagedSnapshot, ConversionError> {
        Ok(StagedSnapshot {
            id: required_snapshot_id(self.snapshot_id.as_ref(), "StagedInfo.snapshot_id")?,
            user,
            tree_id,
            branch_id,
            branch_name: self.branch_name.clone(),
            files_changed: self.files_changed.clone(),
            files_added: self.files_added,
            files_modified: self.files_modified,
            files_deleted: self.files_deleted,
            message: self.message.clone(),
            status: StagedStatus::Staged,
            timestamp: required_timestamp(self.staged_at.as_ref(), "StagedInfo.staged_at")?,
        })
    }
}

/// Domain → wire; `author` is the user's account id string.
impl From<StagedSnapshot> for wire::StagedInfo {
    fn from(s: StagedSnapshot) -> Self {
        let author = s.user.to_string();
        wire::StagedInfo::from_staged(&s, author)
    }
}

// ============================================================
// TagRecord  <->  object::tag::Tag
// ============================================================

use crate::object::tag::{Tag, TagKind};

impl wire::TagRecord {
    /// Domain → wire with an explicit display tagger (e.g. a username).
    /// Signatures are not carried on the wire.
    pub fn from_tag(tag: &Tag, tagger: impl Into<String>) -> Self {
        Self {
            name: tag.name.clone(),
            target: Some(tag.target.into()),
            message: tag.message.clone(),
            tagger: tagger.into(),
            created_at: Some(datetime_to_prost(tag.created_at)),
        }
    }

    /// Wire → domain for `tree_id`. The kind is `Annotated` when a message
    /// is present, else `Lightweight`; `tagger` is kept only if it is an
    /// account id.
    pub fn to_tag(&self, tree_id: TreeId) -> Result<Tag, ConversionError> {
        Ok(Tag {
            name: self.name.clone(),
            target: required_snapshot_id(self.target.as_ref(), "TagRecord.target")?,
            tree_id,
            kind: if self.message.is_some() {
                TagKind::Annotated
            } else {
                TagKind::Lightweight
            },
            message: self.message.clone(),
            tagger: Uuid::from_str(&self.tagger).ok().map(AccountId::from_uuid),
            created_at: required_timestamp(self.created_at.as_ref(), "TagRecord.created_at")?,
            signature: None,
        })
    }
}

/// Domain → wire; `tagger` is the tagger's account id string (or empty).
impl From<Tag> for wire::TagRecord {
    fn from(t: Tag) -> Self {
        let tagger = t.tagger.map(|a| a.to_string()).unwrap_or_default();
        wire::TagRecord::from_tag(&t, tagger)
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
            snapshots: vec![],
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

#[cfg(test)]
mod record_tests {
    use super::*;
    use crate::core::hash::hash_bytes;
    use crate::object::manifest::ManifestEntry as DomainEntry;

    fn wire_entry(path: &str, data: &[u8]) -> wire::ManifestEntry {
        wire::ManifestEntry {
            path: path.into(),
            blob_hash: Some(hash_bytes(data).into()),
            size: data.len() as u64,
            executable: false,
        }
    }

    #[test]
    fn content_hash_borrowed_conversions_and_hex() {
        let h = hash_bytes(b"borrowed");
        let w: wire::ContentHash = (&h).into();
        assert_eq!(w.to_domain().unwrap(), h);
        assert_eq!(ContentHash::try_from(&w).unwrap(), h);
        assert_eq!(w.to_hex(), h.to_hex());
        let short = wire::ContentHash { value: vec![1; 31] };
        assert!(matches!(
            short.to_domain(),
            Err(ConversionError::InvalidContentHash { actual: 31, .. })
        ));
    }

    #[test]
    fn manifest_path_validation() {
        for ok in ["a", "a/b.txt", "dir/sub/file", ".hidden", "a..b", "x:y"] {
            assert!(validate_manifest_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "/abs", "a//b", "a/", "./a", "a/./b", "../x", "a/../b", "a\\b", "C:", "C:/x",
            "nul\0",
        ] {
            assert!(
                matches!(
                    validate_manifest_path(bad),
                    Err(ConversionError::InvalidPath { .. })
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn manifest_record_roundtrip_via_domain() {
        let tree = TreeId::new();
        let mut m = Manifest::new(tree);
        // Path ordering differs from string ordering: "a/b" < "a-b" as
        // paths, but "a-b" < "a/b" as strings (the wire order).
        m.add_entry(DomainEntry::file("a/b", hash_bytes(b"1"), 1));
        m.add_entry(DomainEntry::file("a-b", hash_bytes(b"2"), 1).with_executable(true));
        m.add_entry(DomainEntry::directory("a"));

        let record = wire::ManifestRecord::try_from(&m).unwrap();
        let paths: Vec<&str> = record.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["a-b", "a/b"]);
        record.validate().unwrap();

        let back = record.clone().into_manifest(tree).unwrap();
        assert_eq!(back.len(), 2);
        let e = back.find_entry(std::path::Path::new("a-b")).unwrap();
        assert!(e.executable);
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(wire::ManifestRecord::try_from(&back).unwrap(), record);
    }

    #[test]
    fn manifest_record_rejects_symlinks_and_bad_paths() {
        let mut m = Manifest::new(TreeId::new());
        m.add_entry(DomainEntry::symlink("ln", hash_bytes(b"t")));
        assert!(matches!(
            wire::ManifestRecord::try_from(&m),
            Err(ConversionError::Unsupported(_))
        ));

        let mut m = Manifest::new(TreeId::new());
        m.add_entry(DomainEntry::file("../escape", hash_bytes(b"x"), 1));
        assert!(matches!(
            wire::ManifestRecord::try_from(&m),
            Err(ConversionError::InvalidPath { .. })
        ));

        let mut m = Manifest::new(TreeId::new());
        m.add_entry(DomainEntry::file("win\\path.txt", hash_bytes(b"x"), 1));
        let r = wire::ManifestRecord::try_from(&m).unwrap();
        assert_eq!(r.entries[0].path, "win/path.txt");
    }

    #[test]
    fn manifest_record_validate_order_and_uniqueness() {
        let unsorted = wire::ManifestRecord {
            entries: vec![wire_entry("b", b"1"), wire_entry("a", b"2")],
        };
        assert!(matches!(
            unsorted.validate(),
            Err(ConversionError::InvalidPath {
                reason: "manifest entries not sorted by path",
                ..
            })
        ));
        let dup = wire::ManifestRecord {
            entries: vec![wire_entry("a", b"1"), wire_entry("a", b"2")],
        };
        assert!(matches!(
            dup.validate(),
            Err(ConversionError::InvalidPath {
                reason: "duplicate manifest path",
                ..
            })
        ));
        let mut no_hash = wire_entry("a", b"1");
        no_hash.blob_hash = None;
        let missing = wire::ManifestRecord {
            entries: vec![no_hash],
        };
        assert!(matches!(
            missing.validate(),
            Err(ConversionError::MissingRequired("ManifestEntry.blob_hash"))
        ));

        // from_entries canonicalises: sorted, last duplicate wins.
        let canon = wire::ManifestRecord::from_entries(vec![
            wire_entry("b", b"1"),
            wire_entry("a", b"2"),
            wire_entry("b", b"3"),
        ]);
        canon.validate().unwrap();
        assert_eq!(canon.entries.len(), 2);
        assert_eq!(canon.entries[1].blob_hash, Some(hash_bytes(b"3").into()));
    }

    #[test]
    fn manifest_object_roundtrip_and_hash() {
        let record = wire::ManifestRecord::from_entries(vec![
            wire_entry("src/main.rs", b"fn main() {}"),
            wire_entry("README.md", b"# hi"),
        ]);
        let obj = record.to_object();
        assert_eq!(obj.object_kind().unwrap(), wire::ObjectKind::Manifest);
        // Object hash = BLAKE3 of the exact protobuf bytes.
        assert_eq!(obj.verify().unwrap(), record.content_hash());
        assert_eq!(
            record.content_hash(),
            hash_bytes(&prost::Message::encode_to_vec(&record))
        );
        assert_eq!(obj.decode_manifest().unwrap(), record);

        // A BLOB is not a manifest.
        let blob = wire::ObjectData::blob(b"data".to_vec());
        assert!(matches!(
            blob.decode_manifest(),
            Err(ConversionError::UnexpectedObjectKind { .. })
        ));

        // Garbage bytes under a valid hash fail to decode.
        let junk = wire::ObjectData::new(wire::ObjectKind::Manifest, vec![0xff, 0xff, 0xff]);
        assert!(matches!(
            junk.decode_manifest(),
            Err(ConversionError::Decode { .. })
        ));
    }

    #[test]
    fn object_data_blob_roundtrip_and_verification() {
        let blob = Blob::from_bytes(b"hello".to_vec());
        let obj: wire::ObjectData = blob.clone().into();
        assert_eq!(obj.object_kind().unwrap(), wire::ObjectKind::Blob);
        assert_eq!(obj.verify().unwrap(), blob.hash);
        let back = Blob::try_from(obj.clone()).unwrap();
        assert_eq!(back, blob);

        let mut tampered = obj.clone();
        tampered.data.push(b'!');
        assert!(matches!(
            Blob::try_from(tampered),
            Err(ConversionError::HashMismatch { .. })
        ));

        let mut no_hash = obj.clone();
        no_hash.hash = None;
        assert!(matches!(
            no_hash.verify(),
            Err(ConversionError::MissingRequired("ObjectData.hash"))
        ));

        let mut unspecified = obj;
        unspecified.kind = wire::ObjectKind::Unspecified as i32;
        assert!(matches!(
            Blob::try_from(unspecified.clone()),
            Err(ConversionError::InvalidEnum { .. })
        ));
        unspecified.kind = 99;
        assert!(unspecified.object_kind().is_err());

        let manifest_obj = wire::ManifestRecord::default().to_object();
        assert!(matches!(
            Blob::try_from(manifest_obj),
            Err(ConversionError::UnexpectedObjectKind { .. })
        ));
    }

    fn sample_snapshot() -> Snapshot {
        let mut s = Snapshot::new(
            TreeId::new(),
            hash_bytes(b"manifest"),
            vec![SnapshotId::new(), SnapshotId::new()],
            "merge",
            AccountId::new(),
        );
        s.auto_generated = true;
        s
    }

    #[test]
    fn snapshot_record_roundtrip() {
        let s = sample_snapshot();
        let rec = wire::SnapshotRecord::from_snapshot(&s, "main");
        assert_eq!(rec.branch_name, "main");
        rec.validate().unwrap();
        assert_eq!(rec.snapshot_id().unwrap(), s.id);
        assert_eq!(rec.parent_ids().unwrap(), s.parents);

        // Survives protobuf encoding.
        let bytes = prost::Message::encode_to_vec(&rec);
        let decoded: wire::SnapshotRecord = prost::Message::decode(bytes.as_slice()).unwrap();
        let back = Snapshot::try_from(decoded).unwrap();
        assert_eq!(back.id, s.id);
        assert_eq!(back.tree_id, s.tree_id);
        assert_eq!(back.manifest_hash, s.manifest_hash);
        assert_eq!(back.parents, s.parents);
        assert_eq!(back.message, s.message);
        assert_eq!(back.author, s.author);
        assert_eq!(back.timestamp, s.timestamp);
        assert!(back.auto_generated);
    }

    #[test]
    fn snapshot_record_with_username_author() {
        let s = sample_snapshot();
        let mut rec = wire::SnapshotRecord::from_snapshot(&s, "main");
        rec.author = "alice".into();
        assert!(matches!(
            Snapshot::try_from(rec.clone()),
            Err(ConversionError::InvalidUuid {
                field: "SnapshotRecord.author",
                ..
            })
        ));
        let who = AccountId::new();
        assert_eq!(rec.to_snapshot_with_author(who).unwrap().author, who);
    }

    #[test]
    fn snapshot_record_validation_errors() {
        let base = wire::SnapshotRecord::from_snapshot(&sample_snapshot(), "main");

        let mut r = base.clone();
        r.id = None;
        assert!(matches!(
            r.validate(),
            Err(ConversionError::MissingRequired("SnapshotRecord.id"))
        ));

        let mut r = base.clone();
        r.manifest_hash = Some(wire::ContentHash { value: vec![0; 3] });
        assert!(matches!(
            r.validate(),
            Err(ConversionError::InvalidContentHash { actual: 3, .. })
        ));

        let mut r = base.clone();
        r.parents.push(wire::SnapshotId {
            value: "nope".into(),
        });
        assert!(r.validate().is_err());

        let mut r = base.clone();
        r.created_at = Some(prost_types::Timestamp {
            seconds: 0,
            nanos: -1,
        });
        assert!(matches!(
            r.validate(),
            Err(ConversionError::InvalidTimestamp { .. })
        ));

        let mut r = base;
        r.created_at = None;
        assert!(matches!(
            r.validate(),
            Err(ConversionError::MissingRequired(
                "SnapshotRecord.created_at"
            ))
        ));
    }

    #[test]
    fn timestamp_rejects_leap_second_nanos() {
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 1_500_000_000,
        };
        assert!(prost_to_datetime(ts, "t").is_err());
    }

    #[test]
    fn tree_info_accessors() {
        let (tree, tenant) = (TreeId::new(), TenantId::new());
        let info = wire::TreeInfo {
            tree_id: Some(tree.into()),
            tenant_id: Some(tenant.into()),
            tenant_slug: "acme".into(),
            name: "web".into(),
            default_branch: "main".into(),
            created: true,
        };
        assert_eq!(info.tree_id_domain().unwrap(), tree);
        assert_eq!(info.tenant_id_domain().unwrap(), tenant);
        let empty = wire::TreeInfo::default();
        assert!(matches!(
            empty.tree_id_domain(),
            Err(ConversionError::MissingRequired("TreeInfo.tree_id"))
        ));
    }

    #[test]
    fn branch_info_from_branch_and_accessors() {
        let branch = Branch::new(TreeId::new(), "main", SnapshotId::new(), AccountId::new());
        let updated = chrono::Utc::now();
        let info = wire::BranchInfo::from_branch(&branch, true, updated);
        assert_eq!(info.name, "main");
        assert!(info.protected);
        assert_eq!(info.tip_id().unwrap(), Some(branch.tip));
        assert_eq!(info.updated_at_utc().unwrap(), Some(updated));

        let empty = wire::BranchInfo {
            name: "new".into(),
            ..Default::default()
        };
        assert_eq!(empty.tip_id().unwrap(), None);
        assert_eq!(empty.updated_at_utc().unwrap(), None);
    }

    #[test]
    fn staged_info_roundtrip() {
        let staged = StagedSnapshot::new(
            AccountId::new(),
            TreeId::new(),
            BranchId::new(),
            "feature",
            vec!["src/a.rs".into(), "src/b.rs".into()],
        )
        .with_message("wip")
        .with_counts(1, 1, 0);

        let info: wire::StagedInfo = staged.clone().into();
        assert_eq!(info.author, staged.user.to_string());
        let back = info
            .to_staged(staged.user, staged.tree_id, staged.branch_id)
            .unwrap();
        assert_eq!(back.id, staged.id);
        assert_eq!(back.branch_name, "feature");
        assert_eq!(back.files_changed, staged.files_changed);
        assert_eq!(back.total_changes(), 2);
        assert_eq!(back.message.as_deref(), Some("wip"));
        assert_eq!(back.timestamp, staged.timestamp);
        assert!(back.is_staged());

        let named = wire::StagedInfo::from_staged(&staged, "alice");
        assert_eq!(named.author, "alice");
        let mut broken = named;
        broken.staged_at = None;
        assert!(broken
            .to_staged(staged.user, staged.tree_id, staged.branch_id)
            .is_err());
    }

    #[test]
    fn tag_record_roundtrip() {
        let tree = TreeId::new();
        let tagger = AccountId::new();
        let tag = Tag::annotated("v1.0", SnapshotId::new(), tree, "release", tagger);
        let rec: wire::TagRecord = tag.clone().into();
        assert_eq!(rec.tagger, tagger.to_string());
        let back = rec.to_tag(tree).unwrap();
        assert_eq!(back.name, "v1.0");
        assert_eq!(back.target, tag.target);
        assert_eq!(back.kind, TagKind::Annotated);
        assert_eq!(back.message.as_deref(), Some("release"));
        assert_eq!(back.tagger, Some(tagger));
        assert_eq!(back.created_at, tag.created_at);

        let light = Tag::lightweight("v0", SnapshotId::new(), tree);
        let rec = wire::TagRecord::from_tag(&light, "bob");
        let back = rec.to_tag(tree).unwrap();
        assert_eq!(back.kind, TagKind::Lightweight);
        assert_eq!(back.tagger, None);

        let mut no_target = rec;
        no_target.target = None;
        assert!(matches!(
            no_target.to_tag(tree),
            Err(ConversionError::MissingRequired("TagRecord.target"))
        ));
    }

    #[test]
    fn push_and_pull_domain_conversions_leave_snapshots_empty() {
        let req = dom::PushRequest {
            tenant_id: TenantId::new(),
            tree_id: TreeId::new(),
            branch_id: BranchId::new(),
            branch_name: "main".into(),
            old_tip: None,
            new_tip: SnapshotId::new(),
            snapshot_chain: vec![],
            account_id: AccountId::new(),
        };
        let mut w: wire::PushRequest = req.into();
        assert!(w.snapshots.is_empty());
        // Records present on the wire are dropped by wire→domain.
        w.snapshots.push(wire::SnapshotRecord::from_snapshot(
            &sample_snapshot(),
            "main",
        ));
        assert!(dom::PushRequest::try_from(w).is_ok());

        let resp = dom::PullResponse {
            has_updates: false,
            new_tip: None,
            snapshot_chain: vec![],
            objects_to_fetch: vec![],
        };
        let w: wire::PullResponse = resp.into();
        assert!(w.snapshots.is_empty());
    }
}
