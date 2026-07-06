//! VCS operations — one module per command surface.
//!
//! Every function takes a [`crate::WorktreeEngine`] and goes through
//! [`crate::persist`] for state; none of them serialize anything directly.

pub mod branch;
pub mod config;
pub mod dependency;
pub mod diff;
pub mod doctor;
pub mod ignore;
pub mod init;
pub mod log;
pub mod merge;
pub mod reflog;
pub mod snapshot;
pub mod status;
pub mod sync;
pub mod tag;
pub mod tree;
