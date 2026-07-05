//! W0rkTree local VCS engine.
//!
//! This crate owns every mutation of a working directory and its `.wt/`
//! state. It is embedded by `worktree-bg` (the daemon — the normal path)
//! and by `worktree-sdk` (degraded mode, when no daemon is running).
//!
//! Module map:
//! - [`engine`] — [`WorktreeEngine`]: worktree discovery and path layout
//! - [`ops`] — one module per VCS operation (snapshot, branch, merge, …)
//! - [`persist`] — state persistence (JSON today; swapped for the
//!   content-addressable store in WT-PHASE-2)
//! - [`workdir`] — working-directory scanning and hashing
//! - [`identity`] — author resolution
//! - [`hooks`] — lifecycle hooks (pre/post-snapshot)

pub mod engine;
pub mod error;
pub mod hooks;
pub mod identity;
pub mod ops;
pub mod persist;
pub mod workdir;

pub use engine::WorktreeEngine;
pub use error::{EngineError, Result};
