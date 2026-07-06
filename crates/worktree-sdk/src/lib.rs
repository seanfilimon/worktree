//! W0rkTree client SDK.
//!
//! The [`Client`] is the single entry point for anything that wants to
//! operate on a worktree — the `wt` CLI, editor integrations, scripts.
//!
//! Dispatch strategy (BgProcess.md §14): prefer the running `worktree-bg`
//! daemon over IPC; fall back to an embedded [`worktree_engine`] when no
//! daemon is available (degraded mode). Commands without an IPC form yet
//! always use the embedded engine — safe either way, since every engine
//! mutation takes the store's writer lock.

mod client;
mod error;
mod remote;

pub use client::{Client, DaemonInfo};
pub use error::{Result, SdkError};

// Result/data types returned by `Client` methods.
pub use worktree_engine::ops::diff::{DiffEntry, DiffStatus};
pub use worktree_engine::ops::merge::MergeResult;
pub use worktree_engine::ops::status::WorkingStatus;
pub use worktree_engine::ops::sync::{PullResult, PushResult};
pub use worktree_engine::persist::{
    BranchState, DoctorReport, FileEntry, SnapshotState, TagState, TreeState, WorktreeState,
};
