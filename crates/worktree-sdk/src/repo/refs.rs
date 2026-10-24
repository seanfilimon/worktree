//! Branches, remote-tracking refs and tags.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{fsutil, names, Repository};
use crate::error::{Result, SdkError};
use crate::model::{BranchInfo, ReflogEntry, SnapshotId, SwitchOptions, TagInfo};

/// Remote whose tracking refs populate [`BranchInfo::upstream_tip`].
const UPSTREAM_REMOTE: &str = "origin";

/// Per-branch metadata stored in `refs/branches.meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BranchMeta {
    created_at: DateTime<Utc>,
    #[serde(default)]
    archived: bool,
}

type BranchMetaMap = BTreeMap<String, BranchMeta>;

impl Repository {
    // ----------------------------------------------------------------
    // Branches
    // ----------------------------------------------------------------

    /// Non-archived branches (all if `include_archived`), sorted by name.
    pub fn branches(&self, tree: &str, include_archived: bool) -> Result<Vec<BranchInfo>> {
        self.load_tree(tree)?;
        let current = self.head_branch(tree)?;
        let meta = self.branch_meta(tree)?;
        let dir = self.branches_dir(tree);
        let mut out = Vec::new();
        for name in list_ref_files(&dir, "")? {
            let m = meta.get(&name);
            let archived = m.is_some_and(|m| m.archived);
            if archived && !include_archived {
                continue;
            }
            let created_at = match m {
                Some(m) => m.created_at,
                None => file_mtime(&dir.join(&name))?,
            };
            out.push(BranchInfo {
                tip: self.read_tip(tree, &name)?,
                is_current: name == current,
                created_at,
                archived,
                upstream_tip: self.remote_tip(tree, UPSTREAM_REMOTE, &name)?,
                name,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// The branch `HEAD` points at.
    pub fn current_branch(&self, tree: &str) -> Result<String> {
        self.load_tree(tree)?;
        self.head_branch(tree)
    }

    /// Create `name` pointing at `from` (a revision; default current head).
    /// `BranchExists` if the name is taken (including by an archived branch).
    pub fn create_branch(&self, tree: &str, name: &str, from: Option<&str>) -> Result<BranchInfo> {
        let _lock = self.lock_tree(tree)?;
        self.create_branch_unlocked(tree, name, from)
    }

    /// Switch branches, rewriting the working directory to the target head.
    /// `DirtyWorkingTree` if there are uncommitted changes and not `force`.
    /// When both branches point at the same snapshot, uncommitted changes
    /// are carried over untouched (unless `force`).
    pub fn switch_branch(&self, tree: &str, name: &str, opts: SwitchOptions) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        if self.merge_state(tree)?.is_some() {
            return Err(SdkError::MergeInProgress);
        }
        let current = self.head_branch(tree)?;
        let current_tip = self.tip_or_none(tree, &current)?;
        if opts.create {
            self.create_branch_unlocked(tree, name, None)?;
        } else {
            self.ensure_active_branch(tree, name)?;
        }
        if name == current && !opts.force {
            return Ok(());
        }
        let target_tip = self.read_tip(tree, name)?;
        if target_tip != current_tip || opts.force {
            let head = self.manifest_of_tip(current_tip.as_deref())?;
            let mut working = self.build_working(&rec, &head, false, false)?;
            let changes = super::snapshotting::compute_changes(&head, &working.manifest);
            if !changes.is_empty() && !opts.force {
                return Err(SdkError::DirtyWorkingTree(changes.len()));
            }
            let target = self.manifest_of_tip(target_tip.as_deref())?;
            let tracked = head.entries.iter().map(|e| e.path.clone()).collect();
            self.apply_manifest(
                &rec,
                &target,
                &tracked,
                &working.manifest,
                &mut working.index,
            )?;
            self.save_index(tree, &working.index)?;
        }
        self.write_head(tree, name)?;
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: name.to_string(),
            action: "switch".into(),
            old: current_tip,
            new: target_tip,
            message: format!("moving from {current} to {name}"),
        })
    }

    /// Soft-delete (archive) a branch. History stays reachable; the name can
    /// be reused only after `restore_branch`. Cannot archive the current or
    /// default branch.
    pub fn archive_branch(&self, tree: &str, name: &str) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        self.ensure_active_branch(tree, name)?;
        if name == self.head_branch(tree)? {
            return Err(SdkError::InvalidArgument(format!(
                "cannot archive the current branch {name}"
            )));
        }
        if name == rec.default_branch {
            return Err(SdkError::InvalidArgument(format!(
                "cannot archive the default branch {name}"
            )));
        }
        self.set_archived(tree, name, true)?;
        let tip = self.read_tip(tree, name)?;
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: name.to_string(),
            action: "branch.archive".into(),
            old: tip,
            new: None,
            message: format!("archived branch {name}"),
        })
    }

    /// Un-archive a branch previously archived with `archive_branch`.
    pub fn restore_branch(&self, tree: &str, name: &str) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        self.load_tree(tree)?;
        if !self.branch_ref_path(tree, name)?.is_file() {
            return Err(SdkError::BranchNotFound(name.to_string()));
        }
        let meta = self.branch_meta(tree)?;
        if !meta.get(name).is_some_and(|m| m.archived) {
            return Err(SdkError::InvalidArgument(format!(
                "branch {name} is not archived"
            )));
        }
        self.set_archived(tree, name, false)?;
        let tip = self.read_tip(tree, name)?;
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: name.to_string(),
            action: "branch.restore".into(),
            old: None,
            new: tip,
            message: format!("restored branch {name}"),
        })
    }

    // ----------------------------------------------------------------
    // Plumbing: branch and remote tips
    // ----------------------------------------------------------------

    /// Tip of a local branch (`None` for an empty branch);
    /// `BranchNotFound` if the branch does not exist.
    pub fn branch_tip(&self, tree: &str, branch: &str) -> Result<Option<SnapshotId>> {
        self.load_tree(tree)?;
        self.read_tip(tree, branch)
    }

    /// Move (or create) a branch ref. Does not touch the working tree.
    /// `SnapshotNotFound` if `tip` is not a stored snapshot.
    pub fn set_branch_tip(&self, tree: &str, branch: &str, tip: Option<&str>) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        if let Some(id) = tip {
            if !self.has_snapshot(id) {
                return Err(SdkError::SnapshotNotFound(id.to_string()));
            }
        }
        self.write_tip(tree, branch, tip)
    }

    /// Last known tip of `<remote>/<branch>`; `None` if unknown.
    pub fn remote_tip(&self, tree: &str, remote: &str, branch: &str) -> Result<Option<SnapshotId>> {
        let path = self.remote_ref_path(tree, remote, branch)?;
        read_ref(&path).map(Option::flatten)
    }

    /// Record (or with `None`, forget) the tip of `<remote>/<branch>`.
    pub fn set_remote_tip(
        &self,
        tree: &str,
        remote: &str,
        branch: &str,
        tip: Option<&str>,
    ) -> Result<()> {
        self.load_tree(tree)?;
        let path = self.remote_ref_path(tree, remote, branch)?;
        match tip {
            Some(id) => fsutil::write_atomic(&path, id.as_bytes()),
            None => {
                fsutil::remove_file_if_exists(&path)?;
                if let Some(parent) = path.parent() {
                    fsutil::prune_empty_dirs(parent, &self.remotes_dir(tree));
                }
                Ok(())
            }
        }
    }

    /// All remote-tracking refs of `remote` as (branch, tip), sorted by branch.
    pub fn remote_branches(&self, tree: &str, remote: &str) -> Result<Vec<(String, SnapshotId)>> {
        self.load_tree(tree)?;
        names::validate_remote_name(remote)?;
        let dir = self.remotes_dir(tree).join(remote);
        let mut out = Vec::new();
        for name in list_ref_files(&dir, "")? {
            if let Some(Some(tip)) = read_ref(&dir.join(&name))? {
                out.push((name, tip));
            }
        }
        out.sort();
        Ok(out)
    }

    // ----------------------------------------------------------------
    // Tags
    // ----------------------------------------------------------------

    /// Tag `rev` (default HEAD) as `name`. `TagExists` if taken.
    pub fn create_tag(
        &self,
        tree: &str,
        name: &str,
        rev: Option<&str>,
        message: Option<&str>,
    ) -> Result<TagInfo> {
        let _lock = self.lock_tree(tree)?;
        let path = self.tag_path(tree, name)?;
        if path.is_file() {
            return Err(SdkError::TagExists(name.to_string()));
        }
        let target = self.resolve(tree, rev.unwrap_or("HEAD"))?;
        let tag = TagInfo {
            name: name.to_string(),
            target: target.id.clone(),
            message: message.map(str::to_string),
            tagger: self.author(),
            created_at: Utc::now(),
        };
        fsutil::write_json(&path, &tag)?;
        self.append_reflog(&ReflogEntry {
            at: tag.created_at,
            tree: tree.to_string(),
            branch: self.head_branch(tree)?,
            action: "tag.create".into(),
            old: None,
            new: Some(target.id),
            message: format!("tagged {name}"),
        })?;
        Ok(tag)
    }

    /// All tags of a tree, sorted by name.
    pub fn tags(&self, tree: &str) -> Result<Vec<TagInfo>> {
        self.load_tree(tree)?;
        let dir = self.tags_dir(tree);
        let mut out = Vec::new();
        for name in list_ref_files(&dir, ".json")? {
            if let Some(tag) = fsutil::read_json_opt::<TagInfo>(&dir.join(format!("{name}.json")))?
            {
                out.push(tag);
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Delete a tag; `TagNotFound` if absent. The tagged snapshot is kept.
    pub fn delete_tag(&self, tree: &str, name: &str) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let path = self.tag_path(tree, name)?;
        let tag: TagInfo =
            fsutil::read_json_opt(&path)?.ok_or_else(|| SdkError::TagNotFound(name.to_string()))?;
        fsutil::remove_file_if_exists(&path)?;
        if let Some(parent) = path.parent() {
            fsutil::prune_empty_dirs(parent, &self.tags_dir(tree));
        }
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: self.head_branch(tree)?,
            action: "tag.delete".into(),
            old: Some(tag.target),
            new: None,
            message: format!("deleted tag {name}"),
        })
    }

    /// Write a tag record directly (used by pull/import).
    pub fn put_tag(&self, tree: &str, tag: &TagInfo) -> Result<()> {
        self.load_tree(tree)?;
        let path = self.tag_path(tree, &tag.name)?;
        fsutil::write_json(&path, tag)
    }

    /// Look up one tag; `None` if absent.
    pub(crate) fn find_tag(&self, tree: &str, name: &str) -> Result<Option<TagInfo>> {
        if names::validate_ref_name("tag", name).is_err() {
            return Ok(None);
        }
        fsutil::read_json_opt(&self.tag_path(tree, name)?)
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    fn branches_dir(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("refs").join("branches")
    }

    fn remotes_dir(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("refs").join("remotes")
    }

    fn tags_dir(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree).join("refs").join("tags")
    }

    fn branch_meta_path(&self, tree: &str) -> PathBuf {
        self.tree_meta_dir(tree)
            .join("refs")
            .join("branches.meta.json")
    }

    fn branch_ref_path(&self, tree: &str, branch: &str) -> Result<PathBuf> {
        names::validate_ref_name("branch", branch)?;
        Ok(self.branches_dir(tree).join(branch))
    }

    fn remote_ref_path(&self, tree: &str, remote: &str, branch: &str) -> Result<PathBuf> {
        names::validate_remote_name(remote)?;
        names::validate_ref_name("branch", branch)?;
        Ok(self.remotes_dir(tree).join(remote).join(branch))
    }

    fn tag_path(&self, tree: &str, name: &str) -> Result<PathBuf> {
        names::validate_ref_name("tag", name)?;
        Ok(self.tags_dir(tree).join(format!("{name}.json")))
    }

    /// Branch name stored in `HEAD`.
    pub(crate) fn head_branch(&self, tree: &str) -> Result<String> {
        let path = self.tree_meta_dir(tree).join("HEAD");
        let head = fsutil::read_string_opt(&path)?
            .ok_or_else(|| SdkError::TreeNotFound(tree.to_string()))?;
        let head = head.trim();
        if head.is_empty() {
            return Err(SdkError::InvalidConfig(format!(
                "{} is empty",
                path.display()
            )));
        }
        Ok(head.to_string())
    }

    pub(crate) fn write_head(&self, tree: &str, branch: &str) -> Result<()> {
        names::validate_ref_name("branch", branch)?;
        fsutil::write_atomic(
            &self.tree_meta_dir(tree).join("HEAD"),
            format!("{branch}\n").as_bytes(),
        )
    }

    /// Whether a local branch ref exists (archived or not).
    pub(crate) fn branch_exists(&self, tree: &str, branch: &str) -> bool {
        self.branch_ref_path(tree, branch)
            .is_ok_and(|p| p.is_file())
    }

    /// Tip of an existing branch; `BranchNotFound` if it does not exist.
    pub(crate) fn read_tip(&self, tree: &str, branch: &str) -> Result<Option<SnapshotId>> {
        let path = self.branch_ref_path(tree, branch)?;
        read_ref(&path)?.ok_or_else(|| SdkError::BranchNotFound(branch.to_string()))
    }

    /// Tip of a branch, treating a missing branch like an empty one.
    pub(crate) fn tip_or_none(&self, tree: &str, branch: &str) -> Result<Option<SnapshotId>> {
        match self.read_tip(tree, branch) {
            Err(SdkError::BranchNotFound(_)) => Ok(None),
            other => other,
        }
    }

    /// Current branch name and its tip.
    pub(crate) fn head_tip(&self, tree: &str) -> Result<(String, Option<SnapshotId>)> {
        let branch = self.head_branch(tree)?;
        let tip = self.tip_or_none(tree, &branch)?;
        Ok((branch, tip))
    }

    /// Write a branch ref (creating the branch and its metadata if needed).
    pub(crate) fn write_tip(&self, tree: &str, branch: &str, tip: Option<&str>) -> Result<()> {
        let path = self.branch_ref_path(tree, branch)?;
        if !path.is_file() {
            self.check_ref_layout(&self.branches_dir(tree), branch)?;
        }
        fsutil::write_atomic(&path, tip.unwrap_or("").as_bytes())?;
        let mut meta = self.branch_meta(tree)?;
        if !meta.contains_key(branch) {
            meta.insert(
                branch.to_string(),
                BranchMeta {
                    created_at: Utc::now(),
                    archived: false,
                },
            );
            fsutil::write_json(&self.branch_meta_path(tree), &meta)?;
        }
        Ok(())
    }

    /// Refuse names that collide with the directory layout of existing refs
    /// (`a` vs `a/b`).
    fn check_ref_layout(&self, dir: &Path, name: &str) -> Result<()> {
        let mut prefix = String::new();
        let parts: Vec<&str> = name.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if dir.join(&prefix).is_file() {
                return Err(SdkError::InvalidArgument(format!(
                    "branch name {name} conflicts with existing branch {prefix}"
                )));
            }
        }
        if dir.join(name).is_dir() {
            return Err(SdkError::InvalidArgument(format!(
                "branch name {name} conflicts with existing branches under {name}/"
            )));
        }
        Ok(())
    }

    fn branch_meta(&self, tree: &str) -> Result<BranchMetaMap> {
        Ok(fsutil::read_json_opt(&self.branch_meta_path(tree))?.unwrap_or_default())
    }

    fn set_archived(&self, tree: &str, branch: &str, archived: bool) -> Result<()> {
        let mut meta = self.branch_meta(tree)?;
        let created_at = match meta.get(branch) {
            Some(m) => m.created_at,
            None => file_mtime(&self.branch_ref_path(tree, branch)?)?,
        };
        meta.insert(
            branch.to_string(),
            BranchMeta {
                created_at,
                archived,
            },
        );
        fsutil::write_json(&self.branch_meta_path(tree), &meta)
    }

    /// `BranchNotFound` if missing; `InvalidArgument` if archived.
    fn ensure_active_branch(&self, tree: &str, name: &str) -> Result<()> {
        if !self.branch_exists(tree, name) {
            return Err(SdkError::BranchNotFound(name.to_string()));
        }
        if self
            .branch_meta(tree)?
            .get(name)
            .is_some_and(|m| m.archived)
        {
            return Err(SdkError::InvalidArgument(format!(
                "branch {name} is archived; restore it first"
            )));
        }
        Ok(())
    }

    pub(crate) fn create_branch_unlocked(
        &self,
        tree: &str,
        name: &str,
        from: Option<&str>,
    ) -> Result<BranchInfo> {
        self.load_tree(tree)?;
        names::validate_ref_name("branch", name)?;
        if self.branch_exists(tree, name) {
            return Err(SdkError::BranchExists(name.to_string()));
        }
        let tip = match from {
            Some(rev) => Some(self.resolve(tree, rev)?.id),
            None => self.head_tip(tree)?.1,
        };
        self.write_tip(tree, name, tip.as_deref())?;
        self.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree.to_string(),
            branch: name.to_string(),
            action: "branch.create".into(),
            old: None,
            new: tip.clone(),
            message: format!("created branch {name}"),
        })?;
        let created_at = self
            .branch_meta(tree)?
            .get(name)
            .map(|m| m.created_at)
            .unwrap_or_else(Utc::now);
        Ok(BranchInfo {
            name: name.to_string(),
            tip,
            is_current: self.head_branch(tree)? == name,
            created_at,
            archived: false,
            upstream_tip: self.remote_tip(tree, UPSTREAM_REMOTE, name)?,
        })
    }
}

/// Read a ref file: `None` if missing, `Some(None)` if empty.
fn read_ref(path: &Path) -> Result<Option<Option<SnapshotId>>> {
    Ok(fsutil::read_string_opt(path)?.map(|s| {
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_string())
    }))
}

fn file_mtime(path: &Path) -> Result<DateTime<Utc>> {
    Ok(DateTime::<Utc>::from(fs::metadata(path)?.modified()?))
}

/// Recursively list ref files under `dir` as '/'-joined names, stripping
/// `suffix` (files without it are skipped). Temp files are ignored.
fn list_ref_files(dir: &Path, suffix: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = stack.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with(fsutil::TEMP_PREFIX) {
                continue;
            }
            let full = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push((entry.path(), full));
            } else if ft.is_file() {
                if let Some(stripped) = full.strip_suffix(suffix) {
                    out.push(stripped.to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}
