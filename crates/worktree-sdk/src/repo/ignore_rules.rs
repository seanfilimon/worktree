//! Ignore rules: built-ins, `.wt/ignore` and per-directory `.wtignore`
//! files, all in gitignore syntax (matched with the `ignore` crate).
//!
//! Rules are evaluated hierarchically like Git: patterns in a deeper
//! `.wtignore` take precedence over shallower ones, and within one file the
//! last matching pattern wins (so `!pattern` can re-include). `.wt` and
//! `.wt-tree` directories are always excluded and cannot be re-included.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use super::{fsutil, names, Repository, IGNORE_FILE_NAME, TREE_MARKER_DIR, WT_DIR};
use crate::error::{Result, SdkError};

/// Patterns applied to every worktree before any user patterns.
pub(crate) const BUILTIN_IGNORES: &[&str] = &[
    ".wt/",
    ".wt-tree/",
    ".git/",
    "target/",
    "node_modules/",
    ".DS_Store",
    "*.swp",
];

/// Whether a path component is reserved metadata that is never tracked.
pub(crate) fn is_reserved_name(name: &str) -> bool {
    name == WT_DIR || name == TREE_MARKER_DIR
}

/// Compiled rules for one directory level.
struct DirMatcher {
    /// Root-relative directory the patterns are relative to ("" = root).
    dir: String,
    matcher: Gitignore,
}

/// A stack of matchers from the worktree root down to the directory being
/// examined.
pub(crate) struct IgnoreStack {
    levels: Vec<DirMatcher>,
}

impl IgnoreStack {
    /// Root-level rules: built-ins, `.wt/ignore`, then the root `.wtignore`.
    pub(crate) fn for_root(root: &Path) -> Result<Self> {
        let mut builder = GitignoreBuilder::new(root);
        for pattern in BUILTIN_IGNORES {
            add_line(&mut builder, pattern, "built-in");
        }
        let wt_ignore = root.join(WT_DIR).join("ignore");
        if let Some(text) = fsutil::read_string_opt(&wt_ignore)? {
            for line in text.lines() {
                add_line(&mut builder, line, ".wt/ignore");
            }
        }
        if let Some(text) = fsutil::read_string_opt(&root.join(IGNORE_FILE_NAME))? {
            for line in text.lines() {
                add_line(&mut builder, line, IGNORE_FILE_NAME);
            }
        }
        let matcher = build(builder)?;
        Ok(Self {
            levels: vec![DirMatcher {
                dir: String::new(),
                matcher,
            }],
        })
    }

    /// Push the `.wtignore` of `dir_rel` (root-relative, non-empty) if it
    /// has one. Returns whether a level was pushed (pass it to [`Self::pop`]).
    pub(crate) fn push_dir(&mut self, root: &Path, dir_rel: &str) -> Result<bool> {
        let dir = root.join(dir_rel);
        let Some(text) = fsutil::read_string_opt(&dir.join(IGNORE_FILE_NAME))? else {
            return Ok(false);
        };
        let mut builder = GitignoreBuilder::new(&dir);
        let source = format!("{dir_rel}/{IGNORE_FILE_NAME}");
        for line in text.lines() {
            add_line(&mut builder, line, &source);
        }
        let matcher = build(builder)?;
        self.levels.push(DirMatcher {
            dir: dir_rel.to_string(),
            matcher,
        });
        Ok(true)
    }

    /// Undo a successful [`Self::push_dir`].
    pub(crate) fn pop(&mut self, pushed: bool) {
        if pushed && self.levels.len() > 1 {
            self.levels.pop();
        }
    }

    /// Whether the root-relative `rel` path is ignored. Parent directories
    /// are assumed to have been checked already (the walker prunes them).
    pub(crate) fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        if rel.split('/').any(is_reserved_name) {
            return true;
        }
        for level in self.levels.iter().rev() {
            let sub = if level.dir.is_empty() {
                rel
            } else {
                match rel
                    .strip_prefix(level.dir.as_str())
                    .and_then(|r| r.strip_prefix('/'))
                {
                    Some(sub) => sub,
                    None => continue,
                }
            };
            let m = level.matcher.matched(Path::new(sub), is_dir);
            if m.is_ignore() {
                return true;
            }
            if m.is_whitelist() {
                return false;
            }
        }
        false
    }
}

fn add_line(builder: &mut GitignoreBuilder, line: &str, source: &str) {
    if let Err(err) = builder.add_line(None, line) {
        tracing::warn!(source, line, %err, "ignoring invalid ignore pattern");
    }
}

fn build(builder: GitignoreBuilder) -> Result<Gitignore> {
    builder
        .build()
        .map_err(|e| SdkError::InvalidConfig(format!("ignore rules: {e}")))
}

/// Rewrite a pattern from `<dir>/.wtignore` as an equivalent root-relative
/// pattern (for display). `None` for blank lines and comments.
fn root_relative_pattern(dir: &str, line: &str) -> Option<String> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    if dir.is_empty() {
        return Some(line.to_string());
    }
    let (neg, pat) = match line.strip_prefix('!') {
        Some(rest) => ("!", rest),
        None => ("", line),
    };
    let anchored = pat.trim_end_matches('/').contains('/');
    let pat = pat.trim_start_matches('/');
    Some(if anchored {
        format!("{neg}{dir}/{pat}")
    } else {
        format!("{neg}{dir}/**/{pat}")
    })
}

impl Repository {
    // ----------------------------------------------------------------
    // Ignore
    // ----------------------------------------------------------------

    /// Effective ignore patterns: built-ins, `.wt/ignore`, `.wtignore` files.
    /// Patterns from nested `.wtignore` files are shown rewritten relative
    /// to the worktree root.
    pub fn ignore_patterns(&self) -> Result<Vec<String>> {
        let mut out: Vec<String> = BUILTIN_IGNORES.iter().map(|p| p.to_string()).collect();
        if let Some(text) = fsutil::read_string_opt(&self.wt_dir().join("ignore"))? {
            out.extend(text.lines().filter_map(|l| root_relative_pattern("", l)));
        }
        let mut files = Vec::new();
        self.walk_files("", &[], false, &mut |rel, _, _| {
            let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            if name == IGNORE_FILE_NAME {
                files.push(dir.to_string());
            }
            Ok(())
        })?;
        // Shallow files first (root = depth 0), matching evaluation order.
        let depth = |d: &str| {
            if d.is_empty() {
                0
            } else {
                d.split('/').count()
            }
        };
        files.sort_by(|a, b| (depth(a), a).cmp(&(depth(b), b)));
        for dir in files {
            let path = self.root.join(&dir).join(IGNORE_FILE_NAME);
            if let Some(text) = fsutil::read_string_opt(&path)? {
                out.extend(text.lines().filter_map(|l| root_relative_pattern(&dir, l)));
            }
        }
        Ok(out)
    }

    /// Append a pattern to `.wt/ignore` (no-op if already present).
    /// `InvalidArgument` if the pattern is empty or not valid gitignore syntax.
    pub fn add_ignore_pattern(&self, pattern: &str) -> Result<()> {
        let pattern = pattern.trim();
        if pattern.is_empty() || pattern.starts_with('#') || pattern.contains('\n') {
            return Err(SdkError::InvalidArgument(format!(
                "invalid ignore pattern {pattern:?}"
            )));
        }
        GitignoreBuilder::new(&self.root)
            .add_line(None, pattern)
            .map_err(|e| {
                SdkError::InvalidArgument(format!("invalid ignore pattern {pattern:?}: {e}"))
            })?;
        let path = self.wt_dir().join("ignore");
        let mut text = fsutil::read_string_opt(&path)?.unwrap_or_default();
        if text.lines().any(|l| l.trim() == pattern) {
            return Ok(());
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(pattern);
        text.push('\n');
        fsutil::write_atomic(&path, text.as_bytes())
    }

    /// Whether `rel_path` ('/'-separated, relative to root) is ignored.
    /// A path is ignored if it or any parent directory matches.
    pub fn is_ignored(&self, rel_path: &str) -> Result<bool> {
        let rel = names::normalize_rel_path(rel_path);
        if rel.is_empty() {
            return Ok(false);
        }
        let parts: Vec<&str> = rel.split('/').collect();
        if parts.contains(&"..") {
            return Err(SdkError::InvalidArgument(format!(
                "path escapes the worktree: {rel_path}"
            )));
        }
        let mut stack = IgnoreStack::for_root(&self.root)?;
        let mut prefix = String::new();
        for (i, part) in parts.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let last = i + 1 == parts.len();
            let is_dir = !last || rel_path.ends_with('/') || self.root.join(&prefix).is_dir();
            if stack.is_ignored(&prefix, is_dir) {
                return Ok(true);
            }
            if !last {
                stack.push_dir(&self.root, &prefix)?;
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_and_reserved() {
        let dir = tempfile::tempdir().unwrap();
        let stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.is_ignored(".wt", true));
        assert!(stack.is_ignored("sub/.wt-tree", true));
        assert!(stack.is_ignored("target", true));
        assert!(stack.is_ignored("a/node_modules", true));
        assert!(stack.is_ignored("x/.DS_Store", false));
        assert!(stack.is_ignored("notes.swp", false));
        assert!(
            !stack.is_ignored("target", false),
            "target/ only matches directories"
        );
        assert!(!stack.is_ignored("src/main.rs", false));
    }

    #[test]
    fn nested_levels_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(IGNORE_FILE_NAME), "*.log\n").unwrap();
        std::fs::create_dir_all(dir.path().join("keep")).unwrap();
        std::fs::write(
            dir.path().join("keep").join(IGNORE_FILE_NAME),
            "!*.log\n/local\n",
        )
        .unwrap();
        let mut stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.is_ignored("a.log", false));
        assert!(stack.push_dir(dir.path(), "keep").unwrap());
        assert!(!stack.is_ignored("keep/a.log", false));
        assert!(stack.is_ignored("keep/local", false));
        assert!(
            !stack.is_ignored("keep/sub/local", false),
            "anchored to keep/"
        );
        stack.pop(true);
        assert!(stack.is_ignored("keep/a.log", false));
        assert!(!stack.push_dir(dir.path(), "nothing-here").unwrap());
    }

    #[test]
    fn rewrite_nested_patterns() {
        assert_eq!(root_relative_pattern("", "*.log").as_deref(), Some("*.log"));
        assert_eq!(
            root_relative_pattern("a", "*.log").as_deref(),
            Some("a/**/*.log")
        );
        assert_eq!(root_relative_pattern("a", "/b").as_deref(), Some("a/b"));
        assert_eq!(root_relative_pattern("a", "b/c").as_deref(), Some("a/b/c"));
        assert_eq!(
            root_relative_pattern("a", "!x/").as_deref(),
            Some("!a/**/x/")
        );
        assert_eq!(root_relative_pattern("a", "# c"), None);
        assert_eq!(root_relative_pattern("a", "   "), None);
    }
}
