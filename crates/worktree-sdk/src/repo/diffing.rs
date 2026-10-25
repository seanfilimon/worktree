//! Working-directory and revision diffs, with optional rename detection
//! and word diffs.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::PathBuf;

use super::snapshotting::compute_changes;
use super::{names, Repository};
use crate::diff::{diff_stat, is_binary, similarity, unified_diff, word_diff};
use crate::error::Result;
use crate::model::*;

/// Path used in patch headers for the missing side of an add or delete.
const DEV_NULL: &str = "/dev/null";

/// Similarity-based rename detection is skipped when it would compare more
/// than this many (deleted, added) pairs; exact renames are still found.
const RENAME_PAIR_LIMIT: usize = 100 * 100;

/// Files larger than this are only matched as exact renames.
const RENAME_MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;

/// Where the "new" side's file content comes from.
/// (rewrite §14.5: crate-visible for `stash show`.)
pub(crate) enum NewSide {
    /// Blobs in the object store.
    Store,
    /// Files in this working directory.
    Working(PathBuf),
}

/// A detected rename: old path -> (new path, similarity).
type Renames = BTreeMap<String, (String, u8)>;

impl Repository {
    // ----------------------------------------------------------------
    // Diff
    // ----------------------------------------------------------------

    /// Working directory vs. `rev` (default HEAD).
    pub fn diff_working(
        &self,
        tree: &str,
        rev: Option<&str>,
        opts: &DiffOptions,
    ) -> Result<Vec<FileDiff>> {
        let rec = self.load_tree(tree)?;
        let old = match rev {
            Some(rev) => self.manifest_of(&self.resolve(tree, rev)?)?,
            None => self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?,
        };
        let working = self.build_working(&rec, &old, false, false)?;
        if let Err(err) = self.save_index(tree, &working.index) {
            tracing::warn!(tree, %err, "could not refresh index");
        }
        self.diff_manifests(
            &old,
            &working.manifest,
            &NewSide::Working(self.tree_dir_of(&rec)),
            opts,
        )
    }

    /// `from` vs. `to` (both revisions).
    pub fn diff_revs(
        &self,
        tree: &str,
        from: &str,
        to: &str,
        opts: &DiffOptions,
    ) -> Result<Vec<FileDiff>> {
        let old = self.manifest_of(&self.resolve(tree, from)?)?;
        let new = self.manifest_of(&self.resolve(tree, to)?)?;
        self.diff_manifests(&old, &new, &NewSide::Store, opts)
    }

    pub(crate) fn diff_manifests(
        &self,
        old: &Manifest,
        new: &Manifest,
        new_side: &NewSide,
        opts: &DiffOptions,
    ) -> Result<Vec<FileDiff>> {
        let scopes: Vec<String> = opts
            .paths
            .iter()
            .map(|p| names::normalize_rel_path(p))
            .collect();
        let in_scope =
            |p: &str| scopes.is_empty() || scopes.iter().any(|s| names::path_has_prefix(p, s));
        let changes = compute_changes(old, new);
        let renames = match opts.find_renames {
            Some(threshold) => self.detect_renames(old, new, &changes, new_side, threshold)?,
            None => Renames::new(),
        };
        let renamed_to: HashMap<&str, &str> = renames
            .iter()
            .map(|(from, (to, _))| (to.as_str(), from.as_str()))
            .collect();

        let mut out = Vec::new();
        for change in &changes {
            // The deleted half of a rename is reported with its added half.
            if change.kind == ChangeKind::Deleted && renames.contains_key(&change.path) {
                continue;
            }
            let old_path = renamed_to.get(change.path.as_str()).copied();
            if !in_scope(&change.path) && !old_path.is_some_and(in_scope) {
                continue;
            }
            let old_entry = old.get(old_path.unwrap_or(&change.path));
            let new_entry = new.get(&change.path);
            let mut diff = FileDiff {
                path: change.path.clone(),
                kind: if old_path.is_some() {
                    ChangeKind::Modified
                } else {
                    change.kind
                },
                old_hash: old_entry.map(|e| e.hash),
                new_hash: new_entry.map(|e| e.hash),
                binary: false,
                patch: None,
                added: 0,
                removed: 0,
                old_path: old_path.map(str::to_owned),
                similarity: old_path.and_then(|from| renames.get(from).map(|(_, s)| *s)),
            };
            if !opts.name_only {
                let old_data = match old_entry {
                    Some(e) => self.store.read_entry(e)?,
                    None => Vec::new(),
                };
                let new_data = match new_entry {
                    Some(e) => self.read_new_side(e, new_side)?,
                    None => Vec::new(),
                };
                diff.binary = is_binary(&old_data) || is_binary(&new_data);
                if !diff.binary {
                    let old_text = String::from_utf8_lossy(&old_data);
                    let new_text = String::from_utf8_lossy(&new_data);
                    let old_name = match old_entry {
                        Some(e) => e.path.as_str(),
                        None => DEV_NULL,
                    };
                    let new_name = if new_entry.is_some() {
                        change.path.as_str()
                    } else {
                        DEV_NULL
                    };
                    let patch = if opts.word_diff {
                        word_diff(old_name, new_name, &old_text, &new_text, opts.context)
                    } else {
                        unified_diff(old_name, new_name, &old_text, &new_text, opts.context)
                    };
                    let stat = diff_stat(&old_text, &new_text);
                    diff.added = stat.added;
                    diff.removed = stat.removed;
                    diff.patch = (!patch.is_empty()).then_some(patch);
                }
            }
            out.push(diff);
        }
        Ok(out)
    }

    /// Content of a new-side entry (blob, or the working file).
    fn read_new_side(&self, entry: &ManifestEntry, new_side: &NewSide) -> Result<Vec<u8>> {
        match new_side {
            NewSide::Store => self.store.read_entry(entry),
            NewSide::Working(dir) => match fs::read(dir.join(&entry.path)) {
                Ok(d) => Ok(d),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
                Err(e) => Err(e.into()),
            },
        }
    }

    /// Pairs deleted and added paths into renames: first identical content
    /// (by hash, in path order), then the most similar remaining text files
    /// whose line similarity reaches `threshold` percent (greedy, best
    /// score first; ties by path).
    fn detect_renames(
        &self,
        old: &Manifest,
        new: &Manifest,
        changes: &[FileChange],
        new_side: &NewSide,
        threshold: u8,
    ) -> Result<Renames> {
        let mut deleted: Vec<&ManifestEntry> = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Deleted)
            .filter_map(|c| old.get(&c.path))
            .collect();
        let mut added: Vec<&ManifestEntry> = changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Added)
            .filter_map(|c| new.get(&c.path))
            .collect();
        let mut renames = Renames::new();

        // Exact renames.
        added.retain(|a| {
            match deleted
                .iter()
                .position(|d| d.hash == a.hash && d.chunked == a.chunked)
            {
                Some(i) => {
                    let d = deleted.remove(i);
                    renames.insert(d.path.clone(), (a.path.clone(), 100));
                    false
                }
                None => true,
            }
        });

        if threshold > 100
            || deleted.is_empty()
            || added.is_empty()
            || deleted.len() * added.len() > RENAME_PAIR_LIMIT
        {
            return Ok(renames);
        }
        let texts = |entries: &[&ManifestEntry], new_side: Option<&NewSide>| -> Result<Vec<_>> {
            let mut out = Vec::new();
            for e in entries {
                if e.size > RENAME_MAX_FILE_SIZE {
                    continue;
                }
                let data = match new_side {
                    Some(side) => self.read_new_side(e, side)?,
                    None => self.store.read_entry(e)?,
                };
                if !is_binary(&data) {
                    out.push((e.path.clone(), String::from_utf8_lossy(&data).into_owned()));
                }
            }
            Ok(out)
        };
        let old_texts = texts(&deleted, None)?;
        let new_texts = texts(&added, Some(new_side))?;
        let mut scored = Vec::new();
        for (from, old_text) in &old_texts {
            for (to, new_text) in &new_texts {
                let score = similarity(old_text, new_text);
                if score >= threshold && score > 0 {
                    scored.push((score, from, to));
                }
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)).then(a.2.cmp(b.2)));
        let mut taken_to = std::collections::HashSet::new();
        for (score, from, to) in scored {
            if renames.contains_key(from) || taken_to.contains(to) {
                continue;
            }
            taken_to.insert(to);
            renames.insert(from.clone(), (to.clone(), score));
        }
        Ok(renames)
    }
}
