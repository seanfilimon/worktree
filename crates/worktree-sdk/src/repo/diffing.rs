//! Working-directory and revision diffs.

use std::fs;
use std::io;
use std::path::PathBuf;

use super::snapshotting::compute_changes;
use super::{names, Repository};
use crate::diff::{diff_stat, is_binary, unified_diff};
use crate::error::Result;
use crate::model::*;

/// Path used in patch headers for the missing side of an add or delete.
const DEV_NULL: &str = "/dev/null";

/// Where the "new" side's file content comes from.
enum NewSide {
    /// Blobs in the object store.
    Store,
    /// Files in this working directory.
    Working(PathBuf),
}

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

    fn diff_manifests(
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
        let mut out = Vec::new();
        for change in compute_changes(old, new) {
            if !scopes.is_empty()
                && !scopes
                    .iter()
                    .any(|s| names::path_has_prefix(&change.path, s))
            {
                continue;
            }
            let old_entry = old.get(&change.path);
            let new_entry = new.get(&change.path);
            let mut diff = FileDiff {
                path: change.path.clone(),
                kind: change.kind,
                old_hash: old_entry.map(|e| e.hash),
                new_hash: new_entry.map(|e| e.hash),
                binary: false,
                patch: None,
                added: 0,
                removed: 0,
            };
            if !opts.name_only {
                let old_data = match old_entry {
                    Some(e) => self.store.get(&e.hash)?,
                    None => Vec::new(),
                };
                let new_data = match (new_entry, new_side) {
                    (None, _) => Vec::new(),
                    (Some(e), NewSide::Store) => self.store.get(&e.hash)?,
                    (Some(_), NewSide::Working(dir)) => match fs::read(dir.join(&change.path)) {
                        Ok(d) => d,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
                        Err(e) => return Err(e.into()),
                    },
                };
                diff.binary = is_binary(&old_data) || is_binary(&new_data);
                if !diff.binary {
                    let old_text = String::from_utf8_lossy(&old_data);
                    let new_text = String::from_utf8_lossy(&new_data);
                    let old_path = if old_entry.is_some() {
                        change.path.as_str()
                    } else {
                        DEV_NULL
                    };
                    let new_path = if new_entry.is_some() {
                        change.path.as_str()
                    } else {
                        DEV_NULL
                    };
                    let patch =
                        unified_diff(old_path, new_path, &old_text, &new_text, opts.context);
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
}
