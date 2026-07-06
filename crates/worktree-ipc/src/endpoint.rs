//! IPC endpoint naming (BgProcess.md §14.1).
//!
//! | Platform | Transport          | Address                              |
//! |----------|--------------------|--------------------------------------|
//! | Linux    | Unix domain socket | `/tmp/wt-worker-<hash>.sock`         |
//! | macOS    | Unix domain socket | `/tmp/wt-worker-<hash>.sock`         |
//! | Windows  | Named pipe         | `\\.\pipe\wt-worker-<hash>`          |
//!
//! `<hash>` is a stable hash of the worktree root path so each worktree has
//! its own channel. `WT_WORKER_SOCKET` (spec §15.2) overrides the address.

use std::path::Path;

/// Environment variable overriding the IPC endpoint address.
pub const WORKER_SOCKET_ENV: &str = "WT_WORKER_SOCKET";

/// Stable identifier for a worktree, derived from its canonicalized root
/// path — same derivation as `worktree-store::paths::worktree_hash` so the
/// store and the IPC channel agree on worktree identity.
pub fn worktree_hash(worktree_root: &Path) -> String {
    let canonical = worktree_root
        .canonicalize()
        .unwrap_or_else(|_| worktree_root.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let hash = blake3::hash(normalized.as_bytes());
    hash.to_hex()[..16].to_string()
}

/// Platform-appropriate endpoint address for a worktree's daemon.
pub fn endpoint_for(worktree_root: &Path) -> String {
    if let Ok(addr) = std::env::var(WORKER_SOCKET_ENV) {
        return addr;
    }
    let hash = worktree_hash(worktree_root);
    if cfg!(windows) {
        format!(r"\\.\pipe\wt-worker-{hash}")
    } else {
        format!("/tmp/wt-worker-{hash}.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_is_stable_per_path() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(endpoint_for(&cwd), endpoint_for(&cwd));
    }

    #[test]
    fn endpoint_has_platform_shape() {
        let cwd = std::env::current_dir().unwrap();
        let addr = endpoint_for(&cwd);
        if cfg!(windows) {
            assert!(addr.starts_with(r"\\.\pipe\wt-worker-"));
        } else {
            assert!(addr.starts_with("/tmp/wt-worker-"));
            assert!(addr.ends_with(".sock"));
        }
    }
}
