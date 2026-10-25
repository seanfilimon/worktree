//! Sparse checkout and partial clones (ENGINEERING_PLAN §15.4,
//! `specs/tree/SparseCheckout.md`).
//!
//! - **Sparse checkout** — `.wt/trees/<tree>/sparse` lists gitignore-style
//!   patterns (one per line, `#` comments). Only head files matching them
//!   are written to the working directory; files outside the set are not
//!   scanned, are never reported as deleted, and snapshots keep their head
//!   entries (like restricted paths, §13.2). Paths of an unresolved merge
//!   conflict always count as inside the set.
//! - **Partial clone** — `.wt/trees/<tree>/partial` names the remote a
//!   sparse fetch skipped blobs from. A repository with a partial tree opens
//!   with a lazy fetcher on its object store ([`crate::store::ObjectFetcher`]):
//!   missing blobs, chunk lists and chunks are downloaded on demand by diff,
//!   merge, blame and checkout.
//!
//! Pattern rules: a pattern without glob characters (`*`, `?`, `[`) names a
//! path relative to the tree root — that file or directory and everything
//! below it (`src`, `docs/guide`). Other patterns follow gitignore syntax
//! (`*.md` matches at any depth, `/*` every top-level entry); `!pattern`
//! excludes again. For each path the deepest matching level wins, so
//! `docs` + `!docs/private` checks out `docs/` without `docs/private/`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

use super::trees::TreeRecord;
use super::{fsutil, names, Repository};
use crate::error::{Result, SdkError};
use crate::model::ManifestEntry;
use crate::store::{file_hash, hash_bytes, ObjectFetcher};

const SPARSE_FILE: &str = "sparse";
const PARTIAL_FILE: &str = "partial";

const SPARSE_HEADER: &str = "\
# W0rkTree sparse checkout patterns (`wt sparse`), relative to the tree root.
# Plain paths name a file or directory; other lines use gitignore syntax.
";

/// Compiled sparse patterns of one tree.
#[derive(Debug, Clone)]
pub struct SparseMatcher {
    patterns: Vec<String>,
    matcher: Arc<Gitignore>,
    /// Paths that are always inside (unresolved conflicts).
    extra: HashSet<String>,
}

impl SparseMatcher {
    /// Compile normalized patterns (see [`normalize_sparse_patterns`]).
    pub fn new(patterns: &[String]) -> Result<Self> {
        let mut builder = GitignoreBuilder::new("");
        for p in patterns {
            builder.add_line(None, p).map_err(|e| {
                SdkError::InvalidArgument(format!("invalid sparse pattern {p:?}: {e}"))
            })?;
        }
        let matcher = builder
            .build()
            .map_err(|e| SdkError::InvalidArgument(format!("sparse patterns: {e}")))?;
        Ok(Self {
            patterns: patterns.to_vec(),
            matcher: Arc::new(matcher),
            extra: HashSet::new(),
        })
    }

    /// The patterns, in file order.
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Whether the tree-relative file `path` is inside the sparse set: the
    /// deepest level (directory or the file itself) with a matching pattern
    /// decides; a negated match excludes.
    pub fn includes(&self, path: &str) -> bool {
        if self.extra.contains(path) {
            return true;
        }
        let mut included = false;
        let mut end = 0;
        loop {
            let next = path[end..].find('/').map(|i| end + i);
            let (prefix, is_dir) = match next {
                Some(i) => (&path[..i], true),
                None => (path, false),
            };
            let m = self.matcher.matched(Path::new(prefix), is_dir);
            if m.is_ignore() {
                included = true;
            } else if m.is_whitelist() {
                included = false;
            }
            match next {
                Some(i) => end = i + 1,
                None => break,
            }
        }
        included
    }
}

/// Normalize and validate user patterns: trims, `\` → `/`, drops a leading
/// `./`, anchors plain paths at the tree root (`src` → `/src`), maps `.` to
/// `/*` (everything). Duplicates are removed, order is kept.
pub fn normalize_sparse_patterns(patterns: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in patterns {
        let p = normalize_one(raw)?;
        if !out.contains(&p) {
            out.push(p);
        }
    }
    Ok(out)
}

fn normalize_one(raw: &str) -> Result<String> {
    let trimmed = raw.trim().replace('\\', "/");
    let (neg, body) = match trimmed.strip_prefix('!') {
        Some(rest) => ("!", rest.trim_start()),
        None => ("", trimmed.as_str()),
    };
    let mut body = body;
    while let Some(rest) = body.strip_prefix("./") {
        body = rest;
    }
    if body.is_empty() || body.starts_with('#') {
        return Err(SdkError::InvalidArgument(format!(
            "invalid sparse pattern {raw:?}: empty or a comment"
        )));
    }
    if body.split('/').any(|c| c == "..") {
        return Err(SdkError::InvalidArgument(format!(
            "invalid sparse pattern {raw:?}: '..' is not allowed"
        )));
    }
    let body = if body == "." || body == "/" {
        "/*".to_string()
    } else if body.starts_with('/') || body.contains(['*', '?', '[']) {
        body.to_string()
    } else {
        format!("/{body}")
    };
    let pattern = format!("{neg}{body}");
    GitignoreBuilder::new("")
        .add_line(None, &pattern)
        .map_err(|e| SdkError::InvalidArgument(format!("invalid sparse pattern {raw:?}: {e}")))?;
    Ok(pattern)
}

/// What a sparse command changed in the working directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparseReport {
    pub tree: String,
    /// The patterns now in effect (empty after `disable`).
    pub patterns: Vec<String>,
    /// Whether sparse checkout is enabled.
    pub enabled: bool,
    /// Files written because they entered the set.
    pub written: usize,
    /// Unmodified files removed because they left the set.
    pub removed: usize,
    /// Modified files outside the set that were left in place.
    pub kept_modified: Vec<String>,
    /// Objects downloaded on demand (partial clones).
    pub fetched: usize,
}

/// Sparse state of a tree for `wt status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparseStatus {
    pub patterns: Vec<String>,
    /// Files in the head snapshot.
    pub files_total: usize,
    /// Head files inside the sparse set (checked out).
    pub files_checked_out: usize,
    /// Content outside the set is downloaded on demand from this remote.
    pub partial_remote: Option<String>,
}

impl Repository {
    // ----------------------------------------------------------------
    // Patterns
    // ----------------------------------------------------------------

    /// The tree's sparse patterns; `None` when sparse checkout is off.
    pub fn sparse_patterns(&self, tree: &str) -> Result<Option<Vec<String>>> {
        let rec = self.load_tree(tree)?;
        self.read_sparse_file(&rec.name)
    }

    /// Whether sparse checkout is enabled for the tree.
    pub fn is_sparse(&self, tree: &str) -> Result<bool> {
        Ok(self.sparse_patterns(tree)?.is_some())
    }

    /// The compiled sparse set of `tree` (with unresolved conflict paths
    /// added), or `None` when sparse checkout is off.
    pub fn sparse_matcher(&self, tree: &str) -> Result<Option<SparseMatcher>> {
        let Some(patterns) = self.read_sparse_file(tree)? else {
            return Ok(None);
        };
        let mut matcher = SparseMatcher::new(&patterns)?;
        if let Some(state) = self.merge_state(tree)? {
            matcher
                .extra
                .extend(state.conflicts.into_iter().map(|c| c.path));
        }
        Ok(Some(matcher))
    }

    /// Enable sparse checkout with exactly `patterns` and update the
    /// working directory.
    pub fn sparse_set(&self, tree: &str, patterns: &[String]) -> Result<SparseReport> {
        let patterns = normalize_sparse_patterns(patterns)?;
        if patterns.is_empty() {
            return Err(SdkError::InvalidArgument(
                "`wt sparse set` needs at least one pattern".into(),
            ));
        }
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_merge(&rec.name)?;
        self.write_sparse_file(&rec.name, Some(&patterns))?;
        self.reapply_locked(&rec)
    }

    /// Add patterns to an enabled sparse checkout and write the files that
    /// enter the set (downloading them on demand in a partial clone).
    pub fn sparse_add(&self, tree: &str, patterns: &[String]) -> Result<SparseReport> {
        let new = normalize_sparse_patterns(patterns)?;
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_merge(&rec.name)?;
        let mut current = self.require_sparse(&rec.name)?;
        for p in new {
            if !current.contains(&p) {
                current.push(p);
            }
        }
        self.write_sparse_file(&rec.name, Some(&current))?;
        self.reapply_locked(&rec)
    }

    /// Remove patterns (`InvalidArgument` for a pattern that is not set, or
    /// when none would remain) and remove unmodified files leaving the set.
    pub fn sparse_remove(&self, tree: &str, patterns: &[String]) -> Result<SparseReport> {
        let gone = normalize_sparse_patterns(patterns)?;
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_merge(&rec.name)?;
        let mut current = self.require_sparse(&rec.name)?;
        for p in &gone {
            let Some(i) = current.iter().position(|c| c == p) else {
                return Err(SdkError::InvalidArgument(format!(
                    "sparse pattern {p:?} is not set (see `wt sparse list`)"
                )));
            };
            current.remove(i);
        }
        if current.is_empty() {
            return Err(SdkError::InvalidArgument(
                "removing every pattern would check out nothing; use `wt sparse disable` to check out everything".into(),
            ));
        }
        self.write_sparse_file(&rec.name, Some(&current))?;
        self.reapply_locked(&rec)
    }

    /// Turn sparse checkout off and write every head file (downloading
    /// missing content on demand in a partial clone).
    pub fn sparse_disable(&self, tree: &str) -> Result<SparseReport> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_merge(&rec.name)?;
        self.write_sparse_file(&rec.name, None)?;
        self.reapply_locked(&rec)
    }

    /// Make the working directory match the sparse set again: write head
    /// files inside it that are missing, remove unmodified head files
    /// outside it.
    pub fn sparse_reapply(&self, tree: &str) -> Result<SparseReport> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_no_merge(&rec.name)?;
        self.reapply_locked(&rec)
    }

    /// Sparse state for `wt status`; `None` when sparse checkout is off.
    pub fn sparse_status(&self, tree: &str) -> Result<Option<SparseStatus>> {
        let rec = self.load_tree(tree)?;
        let Some(matcher) = self.sparse_matcher(&rec.name)? else {
            return Ok(None);
        };
        let head = self.manifest_of_tip(self.head_tip(&rec.name)?.1.as_deref())?;
        Ok(Some(SparseStatus {
            patterns: matcher.patterns().to_vec(),
            files_total: head.entries.len(),
            files_checked_out: head
                .entries
                .iter()
                .filter(|e| matcher.includes(&e.path))
                .count(),
            partial_remote: self.partial_remote(&rec.name)?,
        }))
    }

    // ----------------------------------------------------------------
    // Partial clones
    // ----------------------------------------------------------------

    /// The remote a sparse fetch skipped content from (`None` when the
    /// tree's history is complete locally).
    pub fn partial_remote(&self, tree: &str) -> Result<Option<String>> {
        let text = fsutil::read_string_opt(&self.tree_meta_dir(tree).join(PARTIAL_FILE))?;
        Ok(text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()))
    }

    /// Whether the tree is a partial clone (blobs may be missing locally
    /// and are downloaded on demand).
    pub fn is_partial(&self, tree: &str) -> Result<bool> {
        Ok(self.partial_remote(tree)?.is_some())
    }

    /// Record that a fetch from `remote` skipped content of `tree`.
    pub(crate) fn mark_partial(&self, tree: &str, remote: &str) -> Result<()> {
        if self.partial_remote(tree)?.as_deref() == Some(remote) {
            return Ok(());
        }
        let path = self.tree_meta_dir(tree).join(PARTIAL_FILE);
        fsutil::write_atomic(&path, format!("{remote}\n").as_bytes())
    }

    /// Replace the object store's lazy fetcher (`None` removes it).
    pub fn set_object_fetcher(&mut self, fetcher: Option<Arc<dyn ObjectFetcher>>) {
        self.store = match fetcher {
            Some(f) => self.store.local_only().with_fetcher(f),
            None => self.store.local_only(),
        };
    }

    /// Install the remote lazy fetcher when any tree is a partial clone
    /// (called by `Repository::open` and after a sparse clone). Problems
    /// are logged, never fatal: without a fetcher missing content simply
    /// reads as `ObjectNotFound`.
    pub(crate) fn install_lazy_fetcher(&mut self) {
        match self.lazy_fetch_targets() {
            Ok(targets) if !targets.is_empty() => {
                self.set_object_fetcher(Some(crate::remote::lazy_fetcher(targets)));
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(%err, "cannot set up on-demand object download"),
        }
    }

    /// `(server URL, server tree id)` of every partial tree.
    fn lazy_fetch_targets(&self) -> Result<Vec<(String, String)>> {
        let trees_dir = self.wt_dir().join("trees");
        let any_partial = match fs::read_dir(&trees_dir) {
            Ok(rd) => rd.flatten().any(|e| e.path().join(PARTIAL_FILE).is_file()),
            Err(_) => false,
        };
        if !any_partial {
            return Ok(Vec::new());
        }
        let mut targets = Vec::new();
        for rec in self.tree_records()? {
            let Some(remote) = self.partial_remote(&rec.name)? else {
                continue;
            };
            let Some(section) = self.config.remote.get(&remote) else {
                continue;
            };
            let Ok(server) = crate::remote::normalize_server_url(&section.url) else {
                continue;
            };
            if !rec.id.is_empty() && !targets.contains(&(server.clone(), rec.id.clone())) {
                targets.push((server, rec.id.clone()));
            }
        }
        Ok(targets)
    }

    // ----------------------------------------------------------------
    // Internals
    // ----------------------------------------------------------------

    /// Write the normalized patterns for `wt clone --sparse` (nothing is
    /// checked out yet, so there is nothing to reapply).
    pub(crate) fn init_sparse_patterns(&self, tree: &str, patterns: &[String]) -> Result<()> {
        let patterns = normalize_sparse_patterns(patterns)?;
        self.write_sparse_file(tree, Some(&patterns))
    }

    fn sparse_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join(SPARSE_FILE)
    }

    fn read_sparse_file(&self, tree: &str) -> Result<Option<Vec<String>>> {
        let Some(text) = fsutil::read_string_opt(&self.sparse_path(tree))? else {
            return Ok(None);
        };
        Ok(Some(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect(),
        ))
    }

    fn write_sparse_file(&self, tree: &str, patterns: Option<&[String]>) -> Result<()> {
        let path = self.sparse_path(tree);
        match patterns {
            None => fsutil::remove_file_if_exists(&path),
            Some(patterns) => {
                let mut text = SPARSE_HEADER.to_string();
                for p in patterns {
                    text.push_str(p);
                    text.push('\n');
                }
                fsutil::write_atomic(&path, text.as_bytes())
            }
        }
    }

    fn require_sparse(&self, tree: &str) -> Result<Vec<String>> {
        self.read_sparse_file(tree)?.ok_or_else(|| {
            SdkError::InvalidArgument(
                "sparse checkout is not enabled for this tree; start with `wt sparse set PATTERN...`"
                    .into(),
            )
        })
    }

    fn ensure_no_merge(&self, tree: &str) -> Result<()> {
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        Ok(())
    }

    /// Bring the working directory in line with the sparse set (caller
    /// holds the tree lock).
    fn reapply_locked(&self, rec: &TreeRecord) -> Result<SparseReport> {
        let tree = rec.name.as_str();
        let matcher = self.sparse_matcher(tree)?;
        let head = self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?;
        let tree_dir = self.tree_dir_of(rec);
        let nested = self.nested_tree_paths(rec)?;
        let nested_rel = self.nested_tree_rel_paths(rec)?;

        // Every file on disk, regardless of the sparse set.
        let mut on_disk: HashMap<String, PathBuf> = HashMap::new();
        self.walk_files(&rec.path, &nested, true, &mut |rel, abs, _| {
            on_disk.insert(
                strip_tree_prefix(&rec.path, rel).to_string(),
                abs.to_path_buf(),
            );
            Ok(())
        })?;

        let stubbed: BTreeSet<String> = self.stubbed_paths(tree)?;
        let mut index = self.load_index(tree);
        let mut report = SparseReport {
            tree: tree.to_string(),
            patterns: matcher
                .as_ref()
                .map(|m| m.patterns().to_vec())
                .unwrap_or_default(),
            enabled: matcher.is_some(),
            ..SparseReport::default()
        };
        let mut to_write: Vec<&ManifestEntry> = Vec::new();
        for entry in &head.entries {
            if nested_rel
                .iter()
                .any(|n| names::path_has_prefix(&entry.path, n))
            {
                continue;
            }
            let inside = matcher.as_ref().is_none_or(|m| m.includes(&entry.path));
            match (inside, on_disk.get(&entry.path)) {
                (true, None) => {
                    let withheld = stubbed.contains(&entry.path) && !self.store.has_entry(entry);
                    if !withheld {
                        to_write.push(entry);
                    }
                }
                (false, Some(abs)) => {
                    if unmodified(abs, entry)? {
                        self.remove_working_file(&tree_dir, &entry.path)?;
                        index.remove(&entry.path);
                        report.removed += 1;
                    } else {
                        report.kept_modified.push(entry.path.clone());
                    }
                }
                _ => {}
            }
        }
        report.fetched = self.store.prefetch_entries(to_write.iter().copied())?;
        for entry in to_write {
            let cached = self.materialize(&tree_dir, entry)?;
            index.insert(entry.path.clone(), cached);
            report.written += 1;
        }
        self.save_index(tree, &index)?;
        Ok(report)
    }
}

/// Whether the file at `abs` still holds `entry`'s content.
fn unmodified(abs: &Path, entry: &ManifestEntry) -> Result<bool> {
    let data = match fs::read(abs) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };
    if data.len() as u64 != entry.size {
        return Ok(false);
    }
    // A large file may be recorded either chunked or (legacy) as one blob.
    Ok(file_hash(&data).0 == entry.hash || hash_bytes(&data) == entry.hash)
}

/// `rel` (root-relative) relative to the tree directory `tree_path`.
fn strip_tree_prefix<'a>(tree_path: &str, rel: &'a str) -> &'a str {
    if tree_path.is_empty() {
        return rel;
    }
    rel.strip_prefix(tree_path)
        .and_then(|r| r.strip_prefix('/'))
        .unwrap_or(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats(p: &[&str]) -> Vec<String> {
        normalize_sparse_patterns(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn normalization() {
        assert_eq!(
            pats(&[
                "src",
                "./docs/guide/",
                "*.md",
                "!private",
                "/abs",
                ".",
                "src"
            ]),
            vec!["/src", "/docs/guide/", "*.md", "!/private", "/abs", "/*"]
        );
        assert_eq!(pats(&["a\\b"]), vec!["/a/b"]);
        for bad in ["", "  ", "# comment", "../up", "a/../b"] {
            assert!(
                normalize_sparse_patterns(&[bad.to_string()]).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn plain_paths_are_anchored_directories_or_files() {
        let m = SparseMatcher::new(&pats(&["src", "docs/guide", "README.md"])).unwrap();
        assert!(m.includes("src/main.rs"));
        assert!(m.includes("src/deep/er/x.rs"));
        assert!(m.includes("docs/guide/a.md"));
        assert!(m.includes("README.md"));
        assert!(!m.includes("docs/other.md"));
        assert!(!m.includes("lib/src/x.rs"), "anchored at the tree root");
        assert!(!m.includes("srcx/y"));
        assert!(!m.includes("other.txt"));
    }

    #[test]
    fn globs_and_negation_deepest_level_wins() {
        let m =
            SparseMatcher::new(&pats(&["docs", "!docs/private", "docs/private/keep.md"])).unwrap();
        assert!(m.includes("docs/a.md"));
        assert!(!m.includes("docs/private/secret.md"));
        assert!(m.includes("docs/private/keep.md"));

        let md = SparseMatcher::new(&pats(&["*.md"])).unwrap();
        assert!(md.includes("README.md"));
        assert!(md.includes("a/b/c.md"));
        assert!(!md.includes("a/b/c.rs"));

        let top = SparseMatcher::new(&pats(&["/*", "!/*/"])).unwrap();
        assert!(top.includes("Cargo.toml"));
        assert!(!top.includes("src/lib.rs"));

        let all = SparseMatcher::new(&pats(&["."])).unwrap();
        assert!(all.includes("x") && all.includes("a/b/c"));
    }

    #[test]
    fn extra_paths_are_always_inside() {
        let mut m = SparseMatcher::new(&pats(&["src"])).unwrap();
        assert!(!m.includes("other/conflicted.txt"));
        m.extra.insert("other/conflicted.txt".into());
        assert!(m.includes("other/conflicted.txt"));
    }

    #[test]
    fn tree_prefix_is_stripped() {
        assert_eq!(strip_tree_prefix("", "a/b"), "a/b");
        assert_eq!(strip_tree_prefix("sub", "sub/a"), "a");
    }
}
