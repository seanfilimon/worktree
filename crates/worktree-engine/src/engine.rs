use crate::error::{EngineError, Result};
use std::path::{Component, Path, PathBuf};

/// Lexically normalize a path: drop `.` segments and resolve `..` against
/// the built-up prefix. Keeps roots stable (no `\dir\.` suffixes) so path
/// prefix-stripping and worktree hashing behave consistently.
fn normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Handle to a worktree on disk.
///
/// Knows where the worktree root and its `.wt/` directory live; all
/// operations in [`crate::ops`] take an engine reference to locate state.
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
        current = normalize(current);
        loop {
            if current.join(".wt").is_dir() {
                return Ok(Self { root: current });
            }
            if !current.pop() {
                return Err(EngineError::NotAWorktree);
            }
        }
    }

    /// Create a new worktree at the given path.
    pub fn init(path: &Path) -> Result<Self> {
        let path = normalize(if path.is_relative() {
            std::env::current_dir()?.join(path)
        } else {
            path.to_path_buf()
        });
        if path.join(".wt").exists() {
            return Err(EngineError::AlreadyInitialized);
        }
        crate::ops::init::initialize(&path)?;
        Ok(Self { root: path })
    }

    /// Wrap a path as an engine without checking for `.wt/` — used during
    /// init, before the directory structure exists.
    pub(crate) fn open_unchecked(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Root path of this worktree
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to the `.wt/` directory
    pub fn wt_dir(&self) -> PathBuf {
        self.root.join(".wt")
    }

    /// Path to the legacy JSON state file (pre-CAS worktrees; migrated on
    /// first read).
    pub fn state_file(&self) -> PathBuf {
        self.wt_dir().join("state.json")
    }

    /// Path to the reflog directory (DotWt.md: reflog lives in `.wt/`).
    pub fn reflog_dir(&self) -> PathBuf {
        self.wt_dir().join("reflog")
    }
}
