//! worktree-bg — local background daemon for W0rkTree.
//!
//! See `../README.md` for role, and
//! `../worktree-protocol/specs/bgprocess/BgProcess.md` for the full spec.
//! Modules are stubs until WT-EXTRACT-2 lands.

pub mod config;
pub mod engine;
pub mod error;
pub mod git;
pub mod ipc;
pub mod service;
pub mod storage;
pub mod sync;
pub mod watcher;
