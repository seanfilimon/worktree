//! Protobuf-generated wire types and gRPC service definitions.
//!
//! Generated from `proto/sync.proto` via `build.rs` (tonic-build).
//! Coexists with the hand-coded Rust domain types in
//! [`crate::feature::sync_protocol`] during the WT-PROTO migration.
//! Conversion impls between domain and wire types live in
//! [`conversions`].

pub mod sync {
    tonic::include_proto!("worktree.sync.v1");
}

pub mod conversions;
