//! Persistent snapshot-id <-> Git commit-sha mapping (`.wt/git-map.json`).
//!
//! Import records every converted commit; export consults the mapping so
//! snapshots that already exist as Git commits are reused (incremental
//! export) and records the commits it creates.
//!
//! File format (keys sorted, so the file is stable across runs):
//!
//! ```json
//! { "version": 1, "snapshots": { "<snapshot id>": "<commit sha>" } }
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GitCompatError, Result};

/// File name of the mapping inside `.wt/`.
pub const GIT_MAP_FILE: &str = "git-map.json";

const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct MapFile {
    version: u32,
    snapshots: BTreeMap<String, String>,
}

/// Bidirectional snapshot <-> commit mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitMap {
    by_snapshot: BTreeMap<String, String>,
    by_commit: HashMap<String, String>,
}

impl GitMap {
    /// Location of the mapping file for the `.wt/` directory `wt_dir`.
    pub fn path(wt_dir: &Path) -> PathBuf {
        wt_dir.join(GIT_MAP_FILE)
    }

    /// Load the mapping; a missing file yields an empty mapping.
    pub fn load(wt_dir: &Path) -> Result<Self> {
        let path = Self::path(wt_dir);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let file: MapFile = serde_json::from_slice(&data)
            .map_err(|e| GitCompatError::MapError(format!("{}: {e}", path.display())))?;
        if file.version != FORMAT_VERSION {
            return Err(GitCompatError::MapError(format!(
                "{}: unsupported version {} (expected {FORMAT_VERSION})",
                path.display(),
                file.version
            )));
        }
        let mut map = Self::default();
        for (snapshot, commit) in file.snapshots {
            map.insert(&snapshot, &commit);
        }
        Ok(map)
    }

    /// Persist the mapping atomically (temp file + rename).
    pub fn save(&self, wt_dir: &Path) -> Result<()> {
        let path = Self::path(wt_dir);
        let file = MapFile {
            version: FORMAT_VERSION,
            snapshots: self.by_snapshot.clone(),
        };
        let mut data = serde_json::to_vec_pretty(&file)
            .map_err(|e| GitCompatError::MapError(e.to_string()))?;
        data.push(b'\n');
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Commit sha recorded for `snapshot_id`.
    pub fn commit_for(&self, snapshot_id: &str) -> Option<&str> {
        self.by_snapshot.get(snapshot_id).map(String::as_str)
    }

    /// Snapshot id recorded for `commit_sha` (the most recently inserted one
    /// if several snapshots map to the same commit).
    pub fn snapshot_for(&self, commit_sha: &str) -> Option<&str> {
        self.by_commit.get(commit_sha).map(String::as_str)
    }

    /// Record (or replace) the commit for `snapshot_id`.
    pub fn insert(&mut self, snapshot_id: &str, commit_sha: &str) {
        if let Some(old) = self
            .by_snapshot
            .insert(snapshot_id.to_owned(), commit_sha.to_owned())
        {
            if old != commit_sha
                && self.by_commit.get(&old).map(String::as_str) == Some(snapshot_id)
            {
                self.by_commit.remove(&old);
            }
        }
        self.by_commit
            .insert(commit_sha.to_owned(), snapshot_id.to_owned());
    }

    /// Record the commit for `snapshot_id` unless one is already recorded.
    /// Returns whether the mapping changed.
    pub fn insert_if_absent(&mut self, snapshot_id: &str, commit_sha: &str) -> bool {
        if self.by_snapshot.contains_key(snapshot_id) {
            return false;
        }
        self.insert(snapshot_id, commit_sha);
        true
    }

    pub fn len(&self) -> usize {
        self.by_snapshot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_snapshot.is_empty()
    }

    /// `(snapshot id, commit sha)` pairs ordered by snapshot id.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_snapshot
            .iter()
            .map(|(s, c)| (s.as_str(), c.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C1: &str = "1111111111111111111111111111111111111111";
    const C2: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let map = GitMap::load(dir.path()).expect("load");
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut map = GitMap::default();
        map.insert("snap-b", C2);
        map.insert("snap-a", C1);
        map.save(dir.path()).expect("save");

        let loaded = GitMap::load(dir.path()).expect("load");
        assert_eq!(loaded, map);
        assert_eq!(loaded.commit_for("snap-a"), Some(C1));
        assert_eq!(loaded.snapshot_for(C2), Some("snap-b"));
        assert_eq!(
            loaded.iter().collect::<Vec<_>>(),
            vec![("snap-a", C1), ("snap-b", C2)]
        );
        assert!(!GitMap::path(dir.path()).with_extension("json.tmp").exists());
    }

    #[test]
    fn serialization_is_sorted_and_stable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut map = GitMap::default();
        map.insert("z", C1);
        map.insert("a", C2);
        map.save(dir.path()).expect("save");
        let first = fs::read_to_string(GitMap::path(dir.path())).expect("read");
        assert!(first.find("\"a\"").expect("a") < first.find("\"z\"").expect("z"));
        assert!(first.contains("\"version\": 1"));

        GitMap::load(dir.path())
            .expect("load")
            .save(dir.path())
            .expect("save again");
        let second = fs::read_to_string(GitMap::path(dir.path())).expect("read");
        assert_eq!(first, second);
    }

    #[test]
    fn insert_replaces_and_updates_reverse_index() {
        let mut map = GitMap::default();
        map.insert("s", C1);
        map.insert("s", C2);
        assert_eq!(map.commit_for("s"), Some(C2));
        assert_eq!(map.snapshot_for(C2), Some("s"));
        assert_eq!(map.snapshot_for(C1), None);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn shared_commit_keeps_reverse_entry_of_other_snapshot() {
        let mut map = GitMap::default();
        map.insert("s1", C1);
        map.insert("s2", C1);
        assert_eq!(map.snapshot_for(C1), Some("s2"));
        // Moving s1 away must not drop the reverse entry owned by s2.
        map.insert("s1", C2);
        assert_eq!(map.snapshot_for(C1), Some("s2"));
        assert_eq!(map.snapshot_for(C2), Some("s1"));
    }

    #[test]
    fn insert_if_absent_keeps_existing() {
        let mut map = GitMap::default();
        assert!(map.insert_if_absent("s", C1));
        assert!(!map.insert_if_absent("s", C2));
        assert_eq!(map.commit_for("s"), Some(C1));
    }

    #[test]
    fn malformed_or_unknown_version_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(GitMap::path(dir.path()), b"{not json").expect("write");
        assert!(matches!(
            GitMap::load(dir.path()),
            Err(GitCompatError::MapError(_))
        ));

        fs::write(
            GitMap::path(dir.path()),
            br#"{"version": 99, "snapshots": {}}"#,
        )
        .expect("write");
        let err = GitMap::load(dir.path()).expect_err("version");
        assert!(err.to_string().contains("unsupported version 99"));
    }
}
