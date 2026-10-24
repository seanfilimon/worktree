//! # Feature Modules
//!
//! Individual protocol features organized by domain. Each sub-module
//! encapsulates a self-contained capability of the Worktree protocol:
//!
//! - **diff** — Diff computation between manifests and patch application
//! - **merge** — Merge strategies, conflict representation and resolution
//! - **wire** — Binary wire format: encoding, decoding, versioning
//! - **compat** — Git compatibility: object mapping, ref mapping, hash bridging
//! - **ignore** — Path ignore/exclusion patterns (analogous to .gitignore)
//! - **licensing** — License metadata and SPDX compliance
//! - **large_file** — Large file storage and chunked transfer
//! - **sync_protocol** — Synchronisation protocol: push, pull, negotiation

pub mod diff;
pub mod merge;
pub mod wire;
pub mod compat;
pub mod ignore;
pub mod licensing;
pub mod large_file;
pub mod sync_protocol;
pub mod archive;
pub mod audit;
