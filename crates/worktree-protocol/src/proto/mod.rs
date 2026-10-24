//! Protobuf-generated wire types and gRPC service definitions.
//!
//! Generated from `proto/sync.proto` via `build.rs` (tonic-build, vendored
//! `protoc`). Coexists with the hand-coded Rust domain types in
//! [`crate::feature::sync_protocol`] during the WT-PROTO migration.
//! Conversion impls and helpers between domain and wire types live in
//! [`conversions`].

/// Codegen'd `worktree.sync.v1` messages, `SyncServiceClient` and
/// `SyncServiceServer`.
// Generated code we do not control: tonic's client methods return
// `Result<_, tonic::Status>` (a large error type by design).
#[allow(clippy::result_large_err)]
pub mod sync {
    tonic::include_proto!("worktree.sync.v1");
}

pub mod conversions;
