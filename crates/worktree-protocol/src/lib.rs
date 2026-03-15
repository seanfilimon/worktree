//! # Worktree Protocol
//!
//! Protocol definitions for the Worktree version control system.
//! Organized by feature domain:
//!
//! - **core** — Foundational primitives: hashing, IDs, errors
//! - **object** — Version control objects: trees, blobs, snapshots, branches, manifests
//! - **iam** — Identity & Access Management: accounts, tenants, teams, RBAC, ABAC
//! - **access** — Resource-level access control: per-tree and per-branch permissions
//! - **config** — Configuration management
//! - **diff** — Diff computation and patch application
//! - **merge** — Merge strategies and conflict resolution
//! - **wire** — Binary wire format: encoding, decoding, versioning
//! - **compat** — Git compatibility: object mapping, ref mapping, hash bridging
//! - **ignore** — Ignore patterns and path filtering
//! - **licensing** — License detection and compliance
//! - **large_file** — Large file storage and chunking
//! - **sync_protocol** — Synchronization protocol for push/pull operations

#[allow(clippy::module_inception)]
pub mod core;
pub mod object;
pub mod iam;
pub mod access;
pub mod config;

// Feature modules
pub mod feature;

// Re-exports for convenience
pub use feature::diff;
pub use feature::merge;
pub use feature::wire;
pub use feature::compat;
pub use feature::ignore;
pub use feature::licensing;
pub use feature::large_file;
pub use feature::sync_protocol;
pub use feature::archive;
pub use feature::audit;
