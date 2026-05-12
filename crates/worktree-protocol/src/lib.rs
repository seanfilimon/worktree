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

pub mod access;
pub mod config;
#[allow(clippy::module_inception)]
pub mod core;
pub mod iam;
pub mod object;

// Feature modules
pub mod feature;

// Re-exports for convenience
pub use feature::archive;
pub use feature::audit;
pub use feature::compat;
pub use feature::diff;
pub use feature::ignore;
pub use feature::large_file;
pub use feature::licensing;
pub use feature::merge;
pub use feature::sync_protocol;
pub use feature::wire;
