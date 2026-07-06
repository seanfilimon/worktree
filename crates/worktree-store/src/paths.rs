//! Store location resolution.
//!
//! Per Storage.md, each worktree gets its own store directory under the
//! platform-native data location, keyed by a stable hash of the worktree
//! root path:
//!
//! - Windows: `%APPDATA%\W0rkTree\stores\<worktree-hash>\`
//! - Linux:   `~/.local/share/w0rktree/stores/<worktree-hash>/`
//! - macOS:   `~/Library/Application Support/W0rkTree/stores/<worktree-hash>/`
//!
//! `WT_STORAGE_DIR` (spec §15.2) overrides the base directory — tests use it
//! to keep stores inside temp dirs.

use crate::error::{Result, StoreError};
use std::path::{Path, PathBuf};

/// Environment variable overriding the platform storage directory.
pub const STORAGE_DIR_ENV: &str = "WT_STORAGE_DIR";

/// Stable identifier for a worktree, derived from its canonicalized root path.
///
/// First 16 hex characters of the BLAKE3 hash of the path (normalized to
/// forward slashes so the hash is stable across path spellings).
pub fn worktree_hash(worktree_root: &Path) -> String {
    let canonical = worktree_root
        .canonicalize()
        .unwrap_or_else(|_| worktree_root.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let hash = blake3::hash(normalized.as_bytes());
    hash.to_hex()[..16].to_string()
}

/// Base data directory for all W0rkTree stores on this machine.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(STORAGE_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|d| d.join("W0rkTree"))
        .ok_or(StoreError::NoDataDir)
}

/// Store directory for a specific worktree.
pub fn store_dir(worktree_root: &Path) -> Result<PathBuf> {
    Ok(data_dir()?
        .join("stores")
        .join(worktree_hash(worktree_root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_short() {
        let dir = tempfile::tempdir().unwrap();
        let a = worktree_hash(dir.path());
        let b = worktree_hash(dir.path());
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_paths_hash_differently() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_ne!(worktree_hash(a.path()), worktree_hash(b.path()));
    }
}
