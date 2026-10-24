//! Pure text diffing (no I/O). Used by `Repository::diff_*`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

/// A unified-diff hunk. Line numbers are 1-based; a zero-length side has
/// start = line before the hunk (0 at file start), as in GNU diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStat {
    pub added: usize,
    pub removed: usize,
}

/// Line diff (Myers or histogram) grouped into hunks with `context` lines.
/// Handles a missing trailing newline.
pub fn diff_lines(old: &str, new: &str, context: usize) -> Vec<Hunk> {
    let _ = (old, new, context);
    todo!("sdk-merge-diff: diff_lines")
}

/// Full unified diff text with `--- a/<old_path>` / `+++ b/<new_path>`
/// headers (use /dev/null for added/deleted files). Empty string if equal.
pub fn unified_diff(old_path: &str, new_path: &str, old: &str, new: &str, context: usize) -> String {
    let _ = (old_path, new_path, old, new, context);
    todo!("sdk-merge-diff: unified_diff")
}

/// Count added/removed lines.
pub fn diff_stat(old: &str, new: &str) -> DiffStat {
    let _ = (old, new);
    todo!("sdk-merge-diff: diff_stat")
}

/// Heuristic: NUL byte in the first 8 KiB, or invalid UTF-8.
pub fn is_binary(data: &[u8]) -> bool {
    let _ = data;
    todo!("sdk-merge-diff: is_binary")
}
