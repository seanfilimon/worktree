//! File walkers and content hashing for the working directory.
//!
//! The skip rules here are a hard-coded minimum (`.wt/`, `.git/`,
//! `node_modules/`); the full hierarchical ignore engine from
//! `worktree-protocol::feature::ignore` gets wired in with the CAS backend
//! in WT-PHASE-2.

use crate::error::Result;
use crate::persist::FileEntry;
use std::path::{Path, PathBuf};

/// Directory names that are always skipped when scanning.
const HARD_SKIP_DIRS: &[&str] = &[".wt", ".git", "node_modules"];

/// True if `rel_path` is, or lives anywhere under, a directory named `dir_name`.
fn under_dir(rel_path: &str, dir_name: &str) -> bool {
    rel_path
        .split(['/', '\\'])
        .any(|segment| segment == dir_name)
}

/// True if the path should be skipped by the scanner.
fn is_skipped(rel_path: &str) -> bool {
    HARD_SKIP_DIRS.iter().any(|dir| under_dir(rel_path, dir))
}

/// Walk all regular files under `root`, skipping hard-ignored directories.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.path().to_path_buf();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if !is_skipped(&rel.to_string_lossy()) {
                files.push(path);
            }
        }
    }
    files
}

/// BLAKE3-hash a file's contents, returning an empty string on read failure.
pub fn hash_file_quick(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(data) => blake3::hash(&data).to_hex().to_string(),
        Err(_) => String::new(),
    }
}

/// Collect `FileEntry` records for every tracked file in a tree.
///
/// `tree_name == "root"` scans the whole worktree; otherwise scans the
/// tree's subdirectory. Paths are normalized to forward slashes and made
/// relative to the worktree root.
pub fn collect_files(root: &Path, tree_name: &str) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();
    let scan_root = if tree_name == "root" {
        root.to_path_buf()
    } else {
        root.join(tree_name)
    };

    if !scan_root.exists() {
        return Ok(files);
    }

    for path in walk_files(&scan_root) {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let metadata = std::fs::metadata(&path)?;
        let data = std::fs::read(&path)?;
        let hash = blake3::hash(&data).to_hex().to_string();

        files.push(FileEntry {
            path: rel_str,
            hash,
            size: metadata.len(),
        });
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_wt_and_git_dirs() {
        assert!(is_skipped(".wt/config.toml"));
        assert!(is_skipped(".git/objects/ab"));
        assert!(is_skipped("sub/node_modules/pkg/index.js"));
        assert!(is_skipped(r"sub\.wt\state.json"));
        assert!(!is_skipped("src/main.rs"));
        assert!(!is_skipped("wt/notes.txt"));
    }
}
