//! # worktree-sdk
//!
//! The local W0rkTree engine. Everything that touches a `.wt/` directory
//! lives here; the CLI (`wt`), the background daemon (`worktree-bg`) and the
//! Git bridge (`worktree-git`) are thin layers over this crate.
//!
//! Module ownership (see `docs/ENGINEERING_PLAN.md`):
//!
//! - [`repo`], [`store`], [`model`], [`config`], [`error`] — local engine
//! - [`diff`], [`merge`] — pure text/manifest algorithms
//! - [`scan`] — secret scanning, license detection, content chunking
//! - [`remote`] — gRPC/REST client for the W0rkTree server
//! - [`ipc`] — CLI <-> background daemon protocol
//! - [`engine`] — legacy JSON-state engine, to be removed

pub mod config;
pub mod diff;
pub mod error;
pub mod ipc;
pub mod merge;
pub mod model;
pub mod remote;
pub mod repo;
pub mod scan;
pub mod store;

/// Legacy engine kept until the CLI no longer depends on it.
pub mod engine;

pub use error::{Result, SdkError};
pub use model::*;
pub use repo::Repository;
pub use store::{ObjectHash, ObjectStore};

/// Legacy alias.
pub use engine::WorktreeEngine;
