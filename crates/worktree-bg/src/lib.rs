//! worktree-bg — local background daemon for W0rkTree.
//!
//! The only long-running process on a developer machine. It watches the
//! working directory, creates auto-snapshots, serves the `wt` CLI over IPC,
//! and (from WT-PHASE-5) syncs with the remote server.
//!
//! Spec: `../worktree-protocol/specs/bgprocess/BgProcess.md`.

pub mod config;
pub mod engine;
pub mod error;
pub mod ipc;
pub mod service;
pub mod sync;
pub mod watcher;
