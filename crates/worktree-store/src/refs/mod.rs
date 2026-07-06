//! Ref storage.
//!
//! Layout per Storage.md:
//!
//! ```text
//! <store>/refs/branches/<tree>/<branch-name>
//! <store>/refs/tags/<tree>/<tag-name>
//! ```
//!
//! Refs are small text files written atomically. Branch names may contain
//! `/` (nested branches map to subdirectories). The store treats values as
//! opaque strings — callers define the schema.

use crate::error::{Result, StoreError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Handle to the `refs/` half of a store.
#[derive(Debug, Clone)]
pub struct RefStore {
    root: PathBuf,
}

impl RefStore {
    /// Open the ref store rooted at `<store>/refs`.
    pub fn new(refs_root: PathBuf) -> Self {
        Self { root: refs_root }
    }

    /// Reject names that would escape the refs directory.
    fn checked_path(&self, namespace: &str, name: &str) -> Result<PathBuf> {
        for segment in name.split(['/', '\\']) {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(StoreError::RefNotFound(format!(
                    "invalid ref name '{name}'"
                )));
            }
        }
        Ok(self.root.join(namespace).join(name))
    }

    /// Atomically write `value` to `refs/<namespace>/<name>`.
    pub fn write(&self, namespace: &str, name: &str, value: &str) -> Result<()> {
        let path = self.checked_path(namespace, name)?;
        let parent = path.parent().expect("ref path has parent");
        std::fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(value.as_bytes())?;
        tmp.flush()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Read `refs/<namespace>/<name>`; `None` if it does not exist.
    pub fn read(&self, namespace: &str, name: &str) -> Result<Option<String>> {
        let path = self.checked_path(namespace, name)?;
        match std::fs::read_to_string(&path) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete `refs/<namespace>/<name>`. Errors if the ref does not exist.
    pub fn delete(&self, namespace: &str, name: &str) -> Result<()> {
        let path = self.checked_path(namespace, name)?;
        std::fs::remove_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::RefNotFound(format!("{namespace}/{name}"))
            } else {
                StoreError::Io(e)
            }
        })?;
        // Prune now-empty parent directories up to the namespace root.
        let namespace_root = self.root.join(namespace);
        let mut dir = path.parent().map(Path::to_path_buf);
        while let Some(d) = dir {
            if d == namespace_root || std::fs::remove_dir(&d).is_err() {
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
        Ok(())
    }

    /// List all refs under `refs/<namespace>/` as `(name, value)` pairs,
    /// names using `/` separators, sorted by name.
    pub fn list(&self, namespace: &str) -> Result<Vec<(String, String)>> {
        let root = self.root.join(namespace);
        let mut out = Vec::new();
        if root.is_dir() {
            collect(&root, &root, &mut out)?;
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let name = path
                .strip_prefix(root)
                .expect("entry under root")
                .to_string_lossy()
                .replace('\\', "/");
            let value = std::fs::read_to_string(&path)?;
            out.push((name, value));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs() -> (tempfile::TempDir, RefStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = RefStore::new(dir.path().join("refs"));
        (dir, store)
    }

    #[test]
    fn write_read_delete() {
        let (_dir, refs) = refs();
        refs.write("branches/root", "main", "abc123").unwrap();
        assert_eq!(
            refs.read("branches/root", "main").unwrap().as_deref(),
            Some("abc123")
        );
        refs.delete("branches/root", "main").unwrap();
        assert_eq!(refs.read("branches/root", "main").unwrap(), None);
    }

    #[test]
    fn nested_branch_names_and_listing() {
        let (_dir, refs) = refs();
        refs.write("branches/root", "main", "a").unwrap();
        refs.write("branches/root", "feature/oauth", "b").unwrap();
        refs.write("branches/root", "feature/ui/dark", "c").unwrap();

        let listed = refs.list("branches/root").unwrap();
        let names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["feature/oauth", "feature/ui/dark", "main"]);
    }

    #[test]
    fn rejects_escaping_names() {
        let (_dir, refs) = refs();
        assert!(refs.write("branches/root", "../evil", "x").is_err());
        assert!(refs.write("branches/root", "a//b", "x").is_err());
    }

    #[test]
    fn delete_missing_is_ref_not_found() {
        let (_dir, refs) = refs();
        assert!(matches!(
            refs.delete("tags/root", "v1"),
            Err(StoreError::RefNotFound(_))
        ));
    }
}
