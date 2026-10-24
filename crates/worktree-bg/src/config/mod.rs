//! Local-daemon configuration types.
//!
//! These are the bg-side slice of what used to be `worktree-server`'s
//! composite `ServerConfig`. The server retains its own narrower
//! `ServerConfig` (data_dir + listen_addr) and is no longer aware of
//! bg-side config; the two runtimes own their config independently.

pub mod auto_snapshot;
pub mod watcher;

pub use auto_snapshot::AutoSnapshotConfig;
pub use watcher::WatcherConfig;
