pub mod init;
pub mod status;
pub mod snapshot;
pub mod branch;
pub mod tree;
pub mod log;
pub mod merge;
pub mod sync;
pub mod tag;
pub mod diff;
pub mod config;
pub mod reflog;
pub mod dependency;
pub mod ignore;

use std::path::{Path, PathBuf};
use crate::error::{SdkError, Result};

/// The core engine that performs all worktree operations locally.
/// Operates on `.wt/` and `.wt-tree/` directories.
pub struct WorktreeEngine {
    /// Root directory of the worktree
    root: PathBuf,
}

impl WorktreeEngine {
    /// Open an existing worktree at the given path.
    /// Walks up from the path to find the nearest `.wt/` directory.
    pub fn open(path: &Path) -> Result<Self> {
        let mut current = path.to_path_buf();
        if current.is_relative() {
            current = std::env::current_dir()?.join(current);
        }
        loop {
            if current.join(".wt").is_dir() {
                return Ok(Self { root: current });
            }
            if !current.pop() {
                return Err(SdkError::NotAWorktree);
            }
        }
    }

    /// Create a new worktree at the given path.
    pub fn init(path: &Path) -> Result<Self> {
        let path = if path.is_relative() {
            std::env::current_dir()?.join(path)
        } else {
            path.to_path_buf()
        };
        if path.join(".wt").exists() {
            return Err(SdkError::AlreadyInitialized);
        }
        init::initialize(&path)?;
        Ok(Self { root: path })
    }

    /// Root path of this worktree
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to the `.wt/` directory
    pub fn wt_dir(&self) -> PathBuf {
        self.root.join(".wt")
    }

    /// Path to the state file
    pub fn state_file(&self) -> PathBuf {
        self.wt_dir().join("state.json")
    }

    /// Path to the objects directory
    pub fn objects_dir(&self) -> PathBuf {
        self.wt_dir().join("objects")
    }

    /// Path to the refs directory
    pub fn refs_dir(&self) -> PathBuf {
        self.wt_dir().join("refs")
    }

    /// Path to the reflog directory
    pub fn reflog_dir(&self) -> PathBuf {
        self.wt_dir().join("reflog")
    }
}
