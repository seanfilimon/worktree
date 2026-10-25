//! worktree-bg — the local W0rkTree background daemon.
//!
//! Watches registered worktrees and takes auto-snapshots after a quiet
//! period (per each worktree's `[auto_snapshot]` config), optionally staging
//! them on the configured remote. Clients talk to it over the
//! [`worktree_sdk::ipc`] protocol; it advertises itself in `$WT_HOME/bg.json`
//! and persists its watch list in `$WT_HOME/bg.toml`.
//!
//! Module map:
//! - [`daemon`] — watch manager, IPC handling, [`run`] lifecycle
//! - [`watcher`] — `notify` integration, event filtering, debouncing
//! - [`worker`] — per-worktree event loop and stats
//! - [`snapshot`] — the auto-snapshot + auto-stage pass
//! - [`sync`] — the per-worktree remote fetch loop (WatchBranches + polling)
//! - [`maintenance`] — periodic reflog expiry and stale temp cleanup
//! - [`config`], [`info`] — `bg.toml` and `bg.json`
//! - [`service`] — login-service definitions

pub mod config;
pub mod daemon;
pub mod error;
pub mod info;
pub mod maintenance;
pub mod paths;
pub mod service;
pub mod snapshot;
pub mod sync;
pub mod watcher;
pub mod worker;

pub use daemon::{run, run_with_options, Daemon, DaemonOptions, RunOutcome, VERSION};
pub use error::{BgError, Result};
