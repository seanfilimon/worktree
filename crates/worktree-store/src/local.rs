//! `LocalStore` — facade over one worktree's store directory.

use crate::cas::ObjectStore;
use crate::error::Result;
use crate::paths;
use crate::refs::RefStore;
use crate::state::StateDir;
use std::path::{Path, PathBuf};

/// One worktree's local store, rooted in the platform data directory
/// (Storage.md §Local Storage). Objects and refs never live inside the
/// working directory.
#[derive(Debug, Clone)]
pub struct LocalStore {
    dir: PathBuf,
}

impl LocalStore {
    /// Open (creating if needed) the store for the worktree at `worktree_root`.
    pub fn open(worktree_root: &Path) -> Result<Self> {
        let dir = paths::store_dir(worktree_root)?;
        Self::open_at(dir)
    }

    /// Open a store at an explicit directory (used by tests and, later, the
    /// server's per-tenant layout which roots stores elsewhere).
    pub fn open_at(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(dir.join("objects"))?;
        std::fs::create_dir_all(dir.join("refs").join("branches"))?;
        std::fs::create_dir_all(dir.join("refs").join("tags"))?;
        std::fs::create_dir_all(dir.join("stubs"))?;
        std::fs::create_dir_all(dir.join("state"))?;
        Ok(Self { dir })
    }

    /// The store's root directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Content-addressable objects.
    pub fn objects(&self) -> ObjectStore {
        ObjectStore::new(self.dir.join("objects"))
    }

    /// Branch/tag references.
    pub fn refs(&self) -> RefStore {
        RefStore::new(self.dir.join("refs"))
    }

    /// State files (head, meta, sync state) and the writer lock.
    pub fn state(&self) -> Result<StateDir> {
        StateDir::new(self.dir.join("state"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_at_creates_layout() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open_at(dir.path().join("s")).unwrap();
        for sub in ["objects", "refs/branches", "refs/tags", "stubs", "state"] {
            assert!(store.dir().join(sub).is_dir(), "{sub} missing");
        }
    }
}
