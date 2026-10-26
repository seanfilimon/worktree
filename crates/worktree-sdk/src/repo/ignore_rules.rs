//! Ignore rules (gitignore syntax, matched with the `ignore` crate).
//!
//! Evaluation order for a root-relative path (docs/ENGINEERING_PLAN.md §13.9):
//!
//! 1. **Hard ignores** — `.wt/`, `.wt-tree/` and `.git` are always ignored
//!    and can never be re-included. (A tree's own shared `.wt-tree/` files
//!    are tracked regardless: the scanner visits them directly, §15.10.)
//! 2. **Root rules** — `.wt/ignore` followed by the root `.wtignore`. They are
//!    authoritative: a path they ignore stays ignored whatever deeper files
//!    say. Within these files the last matching pattern wins as usual, so
//!    `!pattern` re-includes something ignored earlier in the root rules.
//! 3. **Tree rules** — `<tree>/.wt-tree/ignore` of every tree containing the
//!    path. Additive only: they can ignore more, never re-include.
//! 4. **Nested `.wtignore` files** — evaluated like Git (the deepest file
//!    with a matching pattern decides), but they cannot re-include anything
//!    ignored by steps 1–3.
//! 5. **Soft defaults** ([`SOFT_DEFAULTS`]) — apply unless the root rules
//!    re-include the path with a `!` pattern.
//!
//! Compiled matchers are cached process-wide, keyed by the ignore file and
//! validated against its content, so repeated scans of a tree do not
//! recompile unchanged rules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Serialize;

use super::{fsutil, names, Repository, IGNORE_FILE_NAME, TREE_MARKER_DIR, WT_DIR};
use crate::error::{Result, SdkError};

/// Name of a tree's ignore file inside its `.wt-tree/` directory.
pub const TREE_IGNORE_FILE: &str = "ignore";

/// Always ignored; cannot be re-included by any ignore file.
pub const HARD_IGNORES: &[&str] = &[".wt/", ".wt-tree/", ".git/"];

/// Ignored by default; re-include with a `!pattern` in `.wt/ignore` or the
/// root `.wtignore`.
pub const SOFT_DEFAULTS: &[&str] = &[
    "node_modules/",
    "target/",
    "__pycache__/",
    ".DS_Store",
    "Thumbs.db",
    "*.pyc",
    "*.pyo",
    "*.swp",
    "*.swo",
    "*~",
    ".env",
    ".venv/",
    "dist/",
    "build/",
    "*.log",
];

/// Most compiled matchers kept in the process-wide cache.
const MAX_CACHED_MATCHERS: usize = 4096;

/// Whether a path component is reserved metadata that is never tracked.
pub(crate) fn is_reserved_name(name: &str) -> bool {
    name == WT_DIR || name == TREE_MARKER_DIR
}

/// Whether a path component is hard-ignored (reserved or `.git`).
fn is_hard_ignored_name(name: &str) -> bool {
    is_reserved_name(name) || name == ".git"
}

/// Check one gitignore-syntax pattern.
pub(crate) fn validate_ignore_pattern(pattern: &str) -> std::result::Result<(), String> {
    GitignoreBuilder::new("")
        .add_line(None, pattern)
        .map(|_| ())
        .map_err(|e| format!("invalid pattern {pattern:?}: {e}"))
}

/// Problems (1-based line number and message) in the text of an ignore file.
pub(crate) fn ignore_file_problems(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|(i, l)| validate_ignore_pattern(l).err().map(|e| (i + 1, e)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LevelKind {
    /// `<tree>/.wt-tree/ignore`: additive.
    Tree,
    /// `<dir>/.wtignore`: Git-like, below the root rules.
    Nested,
}

/// Compiled rules for one directory level.
struct DirMatcher {
    /// Root-relative directory the patterns are relative to.
    dir: String,
    kind: LevelKind,
    matcher: Arc<Gitignore>,
}

/// The root rules plus a stack of per-directory matchers from the worktree
/// root down to the directory being examined.
pub(crate) struct IgnoreStack {
    root: Arc<Gitignore>,
    levels: Vec<DirMatcher>,
    /// Number of levels added by each successful [`Self::push_dir`].
    frames: Vec<usize>,
}

static SOFT_MATCHER: LazyLock<Gitignore> = LazyLock::new(|| {
    let mut builder = GitignoreBuilder::new("");
    for pattern in SOFT_DEFAULTS {
        add_line(&mut builder, pattern, "soft default");
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
});

type MatcherCache = HashMap<PathBuf, (blake3::Hash, Arc<Gitignore>)>;

static MATCHERS: LazyLock<Mutex<MatcherCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Compile `sources` (label, text) relative to `base`, reusing the cached
/// matcher stored under `key` when the texts are unchanged.
fn compiled(base: &Path, key: PathBuf, sources: &[(&str, &str)]) -> Result<Arc<Gitignore>> {
    let mut hasher = blake3::Hasher::new();
    for (label, text) in sources {
        hasher.update(label.as_bytes());
        hasher.update(&[0]);
        hasher.update(text.as_bytes());
        hasher.update(&[0]);
    }
    let digest = hasher.finalize();
    if let Ok(cache) = MATCHERS.lock() {
        if let Some((hash, matcher)) = cache.get(&key) {
            if *hash == digest {
                return Ok(matcher.clone());
            }
        }
    }
    let mut builder = GitignoreBuilder::new(base);
    for (label, text) in sources {
        for line in text.lines() {
            add_line(&mut builder, line, label);
        }
    }
    let matcher = Arc::new(build(builder)?);
    if let Ok(mut cache) = MATCHERS.lock() {
        if cache.len() >= MAX_CACHED_MATCHERS {
            cache.clear();
        }
        cache.insert(key, (digest, matcher.clone()));
    }
    Ok(matcher)
}

impl IgnoreStack {
    /// Root-level rules: `.wt/ignore`, then the root `.wtignore`.
    pub(crate) fn for_root(root: &Path) -> Result<Self> {
        let wt_ignore = fsutil::read_string_opt(&root.join(WT_DIR).join("ignore"))?;
        let wtignore = fsutil::read_string_opt(&root.join(IGNORE_FILE_NAME))?;
        let matcher = compiled(
            root,
            root.join(WT_DIR).join("ignore"),
            &[
                (".wt/ignore", wt_ignore.as_deref().unwrap_or_default()),
                (IGNORE_FILE_NAME, wtignore.as_deref().unwrap_or_default()),
            ],
        )?;
        // hardening (§15.10): a root `.wt-tree/ignore` (materialized by a
        // clone of a tree that shares it) is a tree-level, additive file
        // for the whole worktree.
        let mut levels = Vec::new();
        let root_tree_file = root.join(TREE_MARKER_DIR).join(TREE_IGNORE_FILE);
        if let Some(text) = fsutil::read_string_opt(&root_tree_file)? {
            let label = format!("{TREE_MARKER_DIR}/{TREE_IGNORE_FILE}");
            let matcher = compiled(root, root_tree_file, &[(&label, &text)])?;
            levels.push(DirMatcher {
                dir: String::new(),
                kind: LevelKind::Tree,
                matcher,
            });
        }
        Ok(Self {
            root: matcher,
            levels,
            frames: Vec::new(),
        })
    }

    /// Push the `.wt-tree/ignore` and `.wtignore` of `dir_rel` (root-relative,
    /// non-empty) if it has them. Returns whether anything was pushed (pass
    /// it to [`Self::pop`]).
    pub(crate) fn push_dir(&mut self, root: &Path, dir_rel: &str) -> Result<bool> {
        let dir = root.join(dir_rel);
        let mut added = 0;
        let tree_file = dir.join(TREE_MARKER_DIR).join(TREE_IGNORE_FILE);
        if let Some(text) = fsutil::read_string_opt(&tree_file)? {
            let label = format!("{dir_rel}/{TREE_MARKER_DIR}/{TREE_IGNORE_FILE}");
            let matcher = compiled(&dir, tree_file, &[(&label, &text)])?;
            self.levels.push(DirMatcher {
                dir: dir_rel.to_string(),
                kind: LevelKind::Tree,
                matcher,
            });
            added += 1;
        }
        let nested_file = dir.join(IGNORE_FILE_NAME);
        if let Some(text) = fsutil::read_string_opt(&nested_file)? {
            let label = format!("{dir_rel}/{IGNORE_FILE_NAME}");
            let matcher = compiled(&dir, nested_file, &[(&label, &text)])?;
            self.levels.push(DirMatcher {
                dir: dir_rel.to_string(),
                kind: LevelKind::Nested,
                matcher,
            });
            added += 1;
        }
        if added == 0 {
            return Ok(false);
        }
        self.frames.push(added);
        Ok(true)
    }

    /// Undo a successful [`Self::push_dir`].
    pub(crate) fn pop(&mut self, pushed: bool) {
        if !pushed {
            return;
        }
        if let Some(n) = self.frames.pop() {
            let keep = self.levels.len().saturating_sub(n);
            self.levels.truncate(keep);
        }
    }

    /// Whether the root-relative `rel` path is ignored. Parent directories
    /// are assumed to have been checked already (the walker prunes them).
    pub(crate) fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        if rel.split('/').any(is_hard_ignored_name) {
            return true;
        }
        let path = Path::new(rel);
        let m = self.root.matched(path, is_dir);
        if m.is_ignore() {
            return true;
        }
        let root_whitelisted = m.is_whitelist();

        let level_path = |level: &DirMatcher| -> Option<&str> {
            if level.dir.is_empty() {
                return Some(rel).filter(|r| !r.is_empty());
            }
            rel.strip_prefix(level.dir.as_str())
                .and_then(|r| r.strip_prefix('/'))
                .filter(|r| !r.is_empty())
        };
        for level in self.levels.iter().filter(|l| l.kind == LevelKind::Tree) {
            if let Some(sub) = level_path(level) {
                if level.matcher.matched(Path::new(sub), is_dir).is_ignore() {
                    return true;
                }
            }
        }
        for level in self
            .levels
            .iter()
            .rev()
            .filter(|l| l.kind == LevelKind::Nested)
        {
            let Some(sub) = level_path(level) else {
                continue;
            };
            let m = level.matcher.matched(Path::new(sub), is_dir);
            if m.is_ignore() {
                return true;
            }
            if m.is_whitelist() {
                break;
            }
        }
        !root_whitelisted && SOFT_MATCHER.matched(path, is_dir).is_ignore()
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

/// Rewrite a pattern from `<dir>/.wtignore` (or a tree ignore file) as an
/// equivalent root-relative pattern (for display). `None` for blank lines
/// and comments.
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

/// One effective ignore pattern and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IgnoreRule {
    /// Root-relative pattern (nested patterns are rewritten).
    pub pattern: String,
    /// `hard`, `soft`, `.wt/ignore`, `<dir>/.wtignore` or
    /// `<tree dir>/.wt-tree/ignore`.
    pub source: String,
}

impl Repository {
    // ----------------------------------------------------------------
    // Ignore
    // ----------------------------------------------------------------

    /// Effective ignore patterns: hard ignores, soft defaults, `.wt/ignore`,
    /// `.wtignore` files and tree ignore files. Patterns from nested files
    /// are shown rewritten relative to the worktree root.
    pub fn ignore_patterns(&self) -> Result<Vec<String>> {
        Ok(self
            .ignore_rules()?
            .into_iter()
            .map(|r| r.pattern)
            .collect())
    }

    /// Like [`Self::ignore_patterns`], with the source of each pattern, in
    /// evaluation order.
    pub fn ignore_rules(&self) -> Result<Vec<IgnoreRule>> {
        let rule = |pattern: String, source: &str| IgnoreRule {
            pattern,
            source: source.to_string(),
        };
        let mut out: Vec<IgnoreRule> = HARD_IGNORES
            .iter()
            .map(|p| rule(p.to_string(), "hard"))
            .collect();
        out.extend(SOFT_DEFAULTS.iter().map(|p| rule(p.to_string(), "soft")));
        if let Some(text) = fsutil::read_string_opt(&self.wt_dir().join("ignore"))? {
            out.extend(
                text.lines()
                    .filter_map(|l| root_relative_pattern("", l))
                    .map(|p| rule(p, ".wt/ignore")),
            );
        }
        for rec in self.tree_records()? {
            let path = self
                .tree_dir_of(&rec)
                .join(TREE_MARKER_DIR)
                .join(TREE_IGNORE_FILE);
            if let Some(text) = fsutil::read_string_opt(&path)? {
                // hardening (§15.10): the root may have a shared one too.
                let source = if rec.path.is_empty() {
                    format!("{TREE_MARKER_DIR}/{TREE_IGNORE_FILE}")
                } else {
                    format!("{}/{TREE_MARKER_DIR}/{TREE_IGNORE_FILE}", rec.path)
                };
                out.extend(
                    text.lines()
                        .filter_map(|l| root_relative_pattern(&rec.path, l))
                        .map(|p| rule(p, &source)),
                );
            }
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
            let source = if dir.is_empty() {
                IGNORE_FILE_NAME.to_string()
            } else {
                format!("{dir}/{IGNORE_FILE_NAME}")
            };
            if let Some(text) = fsutil::read_string_opt(&path)? {
                out.extend(
                    text.lines()
                        .filter_map(|l| root_relative_pattern(&dir, l))
                        .map(|p| rule(p, &source)),
                );
            }
        }
        Ok(out)
    }

    /// Append a pattern to `.wt/ignore` (no-op if already present).
    /// `InvalidArgument` if the pattern is empty or not valid gitignore syntax.
    pub fn add_ignore_pattern(&self, pattern: &str) -> Result<()> {
        append_pattern(&self.wt_dir().join("ignore"), pattern)
    }

    /// Append a pattern to `<tree>/.wt-tree/ignore` (the root tree uses
    /// `.wt/ignore`). Tree patterns are additive: they cannot re-include
    /// paths ignored by the root rules.
    pub fn add_tree_ignore_pattern(&self, tree: &str, pattern: &str) -> Result<()> {
        let rec = self.load_tree(tree)?;
        if rec.path.is_empty() {
            return self.add_ignore_pattern(pattern);
        }
        let path = self
            .tree_dir_of(&rec)
            .join(TREE_MARKER_DIR)
            .join(TREE_IGNORE_FILE);
        append_pattern(&path, pattern)
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
        // hardening (§15.10): a tree's own shared `.wt-tree/` files are
        // tracked (ignore rules do not apply to them).
        if let Some(i) = parts.iter().position(|p| *p == TREE_MARKER_DIR) {
            let dir = parts[..i].join("/");
            if crate::model::is_shared_tree_config_path(&parts[i..].join("/"))
                && self.tree_records()?.iter().any(|r| r.path == dir)
            {
                return Ok(false);
            }
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

/// Append `pattern` to the ignore file at `path` unless already present.
fn append_pattern(path: &Path, pattern: &str) -> Result<()> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.starts_with('#') || pattern.contains('\n') {
        return Err(SdkError::InvalidArgument(format!(
            "invalid ignore pattern {pattern:?}"
        )));
    }
    validate_ignore_pattern(pattern).map_err(SdkError::InvalidArgument)?;
    let mut text = fsutil::read_string_opt(path)?.unwrap_or_default();
    if text.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(pattern);
    text.push('\n');
    fsutil::write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn hard_and_soft_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.is_ignored(".wt", true));
        assert!(stack.is_ignored("sub/.wt-tree", true));
        assert!(stack.is_ignored(".git", true));
        assert!(stack.is_ignored("vendor/.git", false), ".git files too");
        assert!(stack.is_ignored("target", true));
        assert!(stack.is_ignored("a/node_modules", true));
        assert!(stack.is_ignored("x/.DS_Store", false));
        assert!(stack.is_ignored("notes.swp", false));
        for soft in ["pkg/__pycache__", ".venv", "web/dist", "build"] {
            assert!(stack.is_ignored(soft, true), "{soft}");
        }
        for soft in ["m.pyc", ".env", "logs/app.log"] {
            assert!(stack.is_ignored(soft, false), "{soft}");
        }
        assert!(
            !stack.is_ignored("target", false),
            "target/ only matches directories"
        );
        assert!(!stack.is_ignored("src/main.rs", false));
        assert!(!stack.is_ignored(".env.example", false));
    }

    #[test]
    fn root_rules_can_reinclude_soft_defaults_but_not_hard_ignores() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".wt/ignore", "!dist/\n!.git/\n");
        write(dir.path(), IGNORE_FILE_NAME, "!*.log\n");
        let stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(!stack.is_ignored("dist", true));
        assert!(!stack.is_ignored("app.log", false));
        assert!(stack.is_ignored(".git", true));
        assert!(stack.is_ignored("build", true), "other soft defaults stay");
    }

    #[test]
    fn nested_files_cannot_reinclude_soft_defaults() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "sub/.wtignore", "!*.log\n");
        let mut stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.push_dir(dir.path(), "sub").unwrap());
        assert!(stack.is_ignored("sub/a.log", false));
    }

    #[test]
    fn root_ignores_cannot_be_negated_by_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), IGNORE_FILE_NAME, "*.tmp\nsecret/\n");
        write(dir.path(), ".wt/ignore", "*.bak\n");
        write(
            dir.path(),
            "keep/.wtignore",
            "!*.tmp\n!*.bak\n!secret/\n/local\n",
        );
        let mut stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.is_ignored("a.tmp", false));
        assert!(stack.push_dir(dir.path(), "keep").unwrap());
        assert!(stack.is_ignored("keep/a.tmp", false), "root .wtignore wins");
        assert!(stack.is_ignored("keep/a.bak", false), ".wt/ignore wins");
        assert!(stack.is_ignored("keep/secret", true));
        assert!(stack.is_ignored("keep/local", false));
        assert!(
            !stack.is_ignored("keep/sub/local", false),
            "anchored to keep/"
        );
        stack.pop(true);
        assert!(stack.is_ignored("keep/a.tmp", false));
        assert!(!stack.push_dir(dir.path(), "nothing-here").unwrap());
    }

    #[test]
    fn root_negation_within_root_rules_still_works() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".wt/ignore", "*.tmp\n");
        write(dir.path(), IGNORE_FILE_NAME, "!keep.tmp\n");
        let stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.is_ignored("x.tmp", false));
        assert!(!stack.is_ignored("keep.tmp", false));
    }

    #[test]
    fn nested_levels_follow_git_precedence() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/.wtignore", "*.gen\n");
        write(dir.path(), "a/b/.wtignore", "!keep.gen\n");
        let mut stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.push_dir(dir.path(), "a").unwrap());
        assert!(stack.is_ignored("a/x.gen", false));
        assert!(stack.push_dir(dir.path(), "a/b").unwrap());
        assert!(!stack.is_ignored("a/b/keep.gen", false), "deeper file wins");
        assert!(stack.is_ignored("a/b/other.gen", false));
    }

    #[test]
    fn tree_ignore_is_additive() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), IGNORE_FILE_NAME, "*.tmp\n");
        write(
            dir.path(),
            "svc/.wt-tree/ignore",
            "fixtures/\n!*.tmp\n*.out\n!keep.out\n",
        );
        write(dir.path(), "svc/.wtignore", "!fixtures/\n");
        let mut stack = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(stack.push_dir(dir.path(), "svc").unwrap());
        assert!(stack.is_ignored("svc/fixtures", true));
        assert!(stack.is_ignored("svc/a.tmp", false), "cannot negate root");
        assert!(stack.is_ignored("svc/a.out", false));
        assert!(
            !stack.is_ignored("svc/keep.out", false),
            "negation within the same tree file works"
        );
        assert!(!stack.is_ignored("other/a.out", false));
        stack.pop(true);
        assert!(stack.levels.is_empty() && stack.frames.is_empty());
    }

    #[test]
    fn matcher_cache_tracks_content() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), IGNORE_FILE_NAME, "a.txt\n");
        let first = IgnoreStack::for_root(dir.path()).unwrap();
        let again = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(Arc::ptr_eq(&first.root, &again.root), "cached");
        // Same length, rewritten immediately: still picked up.
        write(dir.path(), IGNORE_FILE_NAME, "b.txt\n");
        let changed = IgnoreStack::for_root(dir.path()).unwrap();
        assert!(!changed.is_ignored("a.txt", false));
        assert!(changed.is_ignored("b.txt", false));
    }

    #[test]
    fn pattern_validation() {
        assert!(validate_ignore_pattern("*.log").is_ok());
        assert!(validate_ignore_pattern("a[z-a]").is_err());
        let problems = ignore_file_problems("# c\n*.ok\n\nbad[z-a]\n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].0, 4);
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
