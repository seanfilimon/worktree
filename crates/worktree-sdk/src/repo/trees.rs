//! Tree registry: `.wt/trees/<tree>/tree.toml` plus `.wt-tree/` markers for
//! nested trees.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::lock::TreeLock;
use super::{fsutil, names, Repository, TREE_MARKER_DIR, WT_DIR};
use crate::error::{Result, SdkError};
use crate::model::{ReflogEntry, TreeInfo};

/// Persisted form of a tree (`tree.toml`). The current branch lives in
/// `HEAD` and is not part of this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TreeRecord {
    pub(crate) name: String,
    pub(crate) id: String,
    /// '/'-separated path relative to the worktree root ("" for the root tree).
    pub(crate) path: String,
    pub(crate) default_branch: String,
    pub(crate) remote_name: String,
}

/// Contents of `<tree dir>/.wt-tree/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MarkerConfig {
    name: String,
    id: String,
}

impl Repository {
    // ----------------------------------------------------------------
    // Trees
    // ----------------------------------------------------------------

    /// All registered trees, root tree first, then by path.
    pub fn trees(&self) -> Result<Vec<TreeInfo>> {
        self.tree_records()?
            .iter()
            .map(|r| self.tree_info(r))
            .collect()
    }

    /// Look up a tree by name; `TreeNotFound` if it does not exist.
    pub fn tree(&self, name: &str) -> Result<TreeInfo> {
        let rec = self.load_tree(name)?;
        self.tree_info(&rec)
    }

    /// The root tree (created by init).
    pub fn root_tree(&self) -> Result<TreeInfo> {
        let rec = self.root_tree_record()?;
        self.tree_info(&rec)
    }

    /// Deepest tree whose directory contains `path` (absolute or relative to
    /// the current dir). Falls back to the root tree.
    pub fn tree_for_path(&self, path: &Path) -> Result<TreeInfo> {
        let abs = std::path::absolute(path)?;
        let records = self.tree_records()?;
        let rel = abs.strip_prefix(&self.root).ok().map(path_to_slash);
        let best = rel.and_then(|rel| {
            records
                .iter()
                .filter(|r| names::path_has_prefix(&rel, &r.path))
                .max_by_key(|r| {
                    if r.path.is_empty() {
                        0
                    } else {
                        r.path.split('/').count()
                    }
                })
        });
        match best {
            Some(rec) => self.tree_info(rec),
            None => self.root_tree(),
        }
    }

    /// Register `rel_path` (relative to the root; created if missing) as a
    /// nested tree, writing `<rel_path>/.wt-tree/config.toml`.
    pub fn add_tree(&self, name: &str, rel_path: &Path) -> Result<TreeInfo> {
        names::validate_tree_name(name)?;
        if self.tree_meta_dir(name).join("tree.toml").is_file() {
            return Err(SdkError::TreeExists(name.to_string()));
        }
        let rel = relative_tree_path(rel_path)?;
        if let Some(existing) = self.tree_records()?.into_iter().find(|r| r.path == rel) {
            return Err(SdkError::TreeExists(format!(
                "{rel} (registered as {})",
                existing.name
            )));
        }
        let dir = self.root.join(&rel);
        fs::create_dir_all(&dir)?;
        let rec = self.create_tree_record(name, &rel, &self.config.worktree.default_branch)?;
        let marker = MarkerConfig {
            name: rec.name.clone(),
            id: rec.id.clone(),
        };
        write_marker(&dir, &marker)?;
        self.tree_info(&rec)
    }

    /// Unregister a nested tree (files are kept). The root tree cannot be removed.
    pub fn remove_tree(&self, name: &str) -> Result<()> {
        let rec = self.load_tree(name)?;
        if rec.path.is_empty() {
            return Err(SdkError::InvalidArgument(
                "the root tree cannot be removed".into(),
            ));
        }
        fsutil::remove_dir_all_if_exists(&self.root.join(&rec.path).join(TREE_MARKER_DIR))?;
        fsutil::remove_file_if_exists(&self.conflicts_path(name))?;
        fsutil::remove_dir_all_if_exists(&self.tree_meta_dir(name))
    }

    /// Set the server-side name of a tree.
    pub fn set_tree_remote_name(&self, tree: &str, remote_name: &str) -> Result<()> {
        if remote_name.trim().is_empty() {
            return Err(SdkError::InvalidArgument(
                "remote tree name must not be empty".into(),
            ));
        }
        let _lock = self.lock_tree(tree)?;
        let mut rec = self.load_tree(tree)?;
        rec.remote_name = remote_name.to_string();
        self.save_tree_record(&rec)
    }

    /// Replace a tree's id (used by clone so the local id matches the server).
    pub fn set_tree_id(&self, tree: &str, id: &str) -> Result<()> {
        if id.trim().is_empty() {
            return Err(SdkError::InvalidArgument(
                "tree id must not be empty".into(),
            ));
        }
        let _lock = self.lock_tree(tree)?;
        let mut rec = self.load_tree(tree)?;
        rec.id = id.to_string();
        self.save_tree_record(&rec)?;
        if !rec.path.is_empty() {
            let dir = self.root.join(&rec.path);
            write_marker(
                &dir,
                &MarkerConfig {
                    name: rec.name.clone(),
                    id: rec.id.clone(),
                },
            )?;
        }
        Ok(())
    }

    /// Absolute directory of a tree.
    pub fn tree_dir(&self, tree: &str) -> Result<PathBuf> {
        let rec = self.load_tree(tree)?;
        Ok(self.tree_dir_of(&rec))
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    pub(crate) fn tree_dir_of(&self, rec: &TreeRecord) -> PathBuf {
        if rec.path.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&rec.path)
        }
    }

    /// Take the per-tree lock (fails with `TreeNotFound` for unknown trees).
    pub(crate) fn lock_tree(&self, tree: &str) -> Result<TreeLock> {
        let dir = self.tree_meta_dir(tree);
        if names::validate_tree_name(tree).is_err() || !dir.join("tree.toml").is_file() {
            return Err(SdkError::TreeNotFound(tree.to_string()));
        }
        TreeLock::acquire(&dir.join("LOCK"))
    }

    pub(crate) fn load_tree(&self, name: &str) -> Result<TreeRecord> {
        if names::validate_tree_name(name).is_err() {
            return Err(SdkError::TreeNotFound(name.to_string()));
        }
        let path = self.tree_meta_dir(name).join("tree.toml");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SdkError::TreeNotFound(name.to_string()))
            }
            Err(e) => return Err(e.into()),
        };
        let rec: TreeRecord = toml::from_str(&text)
            .map_err(|e| SdkError::InvalidConfig(format!("{}: {}", path.display(), e.message())))?;
        Ok(rec)
    }

    fn save_tree_record(&self, rec: &TreeRecord) -> Result<()> {
        let text = toml::to_string_pretty(rec)?;
        fsutil::write_atomic(
            &self.tree_meta_dir(&rec.name).join("tree.toml"),
            text.as_bytes(),
        )
    }

    /// All tree records, root first, then sorted by path.
    pub(crate) fn tree_records(&self) -> Result<Vec<TreeRecord>> {
        let dir = self.wt_dir().join("trees");
        let mut out = Vec::new();
        let entries = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !entry.path().join("tree.toml").is_file() {
                continue;
            }
            out.push(self.load_tree(&name)?);
        }
        out.sort_by(|a, b| (!a.path.is_empty(), &a.path).cmp(&(!b.path.is_empty(), &b.path)));
        Ok(out)
    }

    pub(crate) fn root_tree_record(&self) -> Result<TreeRecord> {
        self.tree_records()?
            .into_iter()
            .find(|r| r.path.is_empty())
            .ok_or_else(|| SdkError::TreeNotFound("<root>".into()))
    }

    /// Root-relative paths of trees nested strictly inside `rec`.
    pub(crate) fn nested_tree_paths(&self, rec: &TreeRecord) -> Result<Vec<String>> {
        Ok(self
            .tree_records()?
            .into_iter()
            .filter(|r| r.path != rec.path && names::path_has_prefix(&r.path, &rec.path))
            .map(|r| r.path)
            .collect())
    }

    pub(crate) fn tree_info(&self, rec: &TreeRecord) -> Result<TreeInfo> {
        Ok(TreeInfo {
            name: rec.name.clone(),
            id: rec.id.clone(),
            path: PathBuf::from(&rec.path),
            current_branch: self.head_branch(&rec.name)?,
            default_branch: rec.default_branch.clone(),
            remote_name: rec.remote_name.clone(),
        })
    }

    /// Create `.wt/trees/<name>/` with an empty `branch` and an "init"
    /// reflog entry.
    pub(crate) fn create_tree_record(
        &self,
        name: &str,
        path: &str,
        branch: &str,
    ) -> Result<TreeRecord> {
        names::validate_ref_name("branch", branch)?;
        let dir = self.tree_meta_dir(name);
        for sub in ["refs/branches", "refs/tags", "refs/remotes"] {
            fs::create_dir_all(dir.join(sub))?;
        }
        let rec = TreeRecord {
            name: name.to_string(),
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_string(),
            default_branch: branch.to_string(),
            remote_name: name.to_string(),
        };
        self.save_tree_record(&rec)?;
        self.write_head(name, branch)?;
        self.write_tip(name, branch, None)?;
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: name.to_string(),
            branch: branch.to_string(),
            action: "init".into(),
            old: None,
            new: None,
            message: format!("initialized tree {name}"),
        })?;
        Ok(rec)
    }
}

fn write_marker(dir: &Path, marker: &MarkerConfig) -> Result<()> {
    let path = dir.join(TREE_MARKER_DIR).join("config.toml");
    // local-config (§13.9): the marker file also holds tree-level config
    // sections; keep everything except the identity keys rewritten here.
    let existing = fsutil::read_string_opt(&path)?
        .and_then(|text| text.parse::<toml::Table>().ok())
        .filter(|t| !t.is_empty());
    let text = match existing {
        Some(mut table) => {
            table.insert("name".into(), toml::Value::String(marker.name.clone()));
            table.insert("id".into(), toml::Value::String(marker.id.clone()));
            toml::to_string_pretty(&table)?
        }
        None => toml::to_string_pretty(marker)?,
    };
    fsutil::write_atomic(&path, text.as_bytes())
}

/// '/'-joined string form of a relative path.
pub(crate) fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Validate and normalize a nested tree's path relative to the root.
fn relative_tree_path(rel_path: &Path) -> Result<String> {
    let bad = |why: &str| {
        Err(SdkError::InvalidArgument(format!(
            "invalid tree path {}: {why}",
            rel_path.display()
        )))
    };
    let mut parts = Vec::new();
    for c in rel_path.components() {
        match c {
            Component::Normal(s) => {
                let Some(s) = s.to_str() else {
                    return bad("not valid UTF-8");
                };
                if s == WT_DIR || s == TREE_MARKER_DIR {
                    return bad("reserved directory name");
                }
                parts.push(s.to_string());
            }
            Component::CurDir => {}
            _ => return bad("must be a relative path inside the worktree"),
        }
    }
    if parts.is_empty() {
        return bad("must not be the worktree root");
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_tree_path_validation() {
        assert_eq!(relative_tree_path(Path::new("a/b")).unwrap(), "a/b");
        assert_eq!(relative_tree_path(Path::new("./a")).unwrap(), "a");
        assert!(relative_tree_path(Path::new("")).is_err());
        assert!(relative_tree_path(Path::new("../x")).is_err());
        assert!(relative_tree_path(Path::new(".wt/x")).is_err());
        assert!(relative_tree_path(&std::env::temp_dir()).is_err());
    }

    #[test]
    fn slash_paths() {
        assert_eq!(path_to_slash(&Path::new("a").join("b").join("c")), "a/b/c");
        assert_eq!(path_to_slash(Path::new("")), "");
    }
}
