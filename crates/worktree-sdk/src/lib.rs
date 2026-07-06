//! W0rkTree client SDK.
//!
//! The [`Client`] is the single entry point for anything that wants to
//! operate on a worktree — the `wt` CLI, editor integrations, scripts.
//!
//! Dispatch strategy (BgProcess.md §14): prefer the running `worktree-bg`
//! daemon over IPC; fall back to an embedded [`worktree_engine`] when no
//! daemon is available (degraded mode). WT-PHASE-1 ships the embedded path;
//! the IPC path lands with the daemon in WT-PHASE-3.

mod client;
mod error;
mod remote;

pub use client::Client;
pub use error::{Result, SdkError};

// Result/data types returned by `Client` methods.
pub use worktree_engine::ops::diff::{DiffEntry, DiffStatus};
pub use worktree_engine::ops::merge::MergeResult;
pub use worktree_engine::ops::status::WorkingStatus;
pub use worktree_engine::ops::sync::{PullResult, PushResult};
pub use worktree_engine::persist::{
    BranchState, DoctorReport, FileEntry, SnapshotState, TagState, TreeState, WorktreeState,
};
