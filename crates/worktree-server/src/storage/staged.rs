use std::path::PathBuf;

use crate::error::ServerError;
use worktree_protocol::object::staged::{StagedIndex, StagedSnapshot};

/// Persistent server-side index for staged snapshots.
///
/// This intentionally lives outside the SDK `.wt/state.json` local cache. The
/// Rust server uses a JSON file for now so the endpoint contract can stabilize
/// before the Go IAM/server implementation replaces the backing store.
pub struct StagedStore {
    root: PathBuf,
}

impl StagedStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn staged_dir(&self) -> PathBuf {
        self.root.join("staged")
    }

    fn index_path(&self) -> PathBuf {
        self.staged_dir().join("index.json")
    }

    pub fn load_index(&self) -> Result<StagedIndex, ServerError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(StagedIndex::new());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            ServerError::Storage(format!("read staged index {}: {}", path.display(), e))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            ServerError::Storage(format!("parse staged index {}: {}", path.display(), e))
        })
    }

    pub fn save_index(&self, index: &StagedIndex) -> Result<(), ServerError> {
        let dir = self.staged_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            ServerError::Storage(format!("create staged dir {}: {}", dir.display(), e))
        })?;

        let path = self.index_path();
        let tmp_path = path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(index)
            .map_err(|e| ServerError::Storage(format!("serialize staged index: {}", e)))?;

        std::fs::write(&tmp_path, content).map_err(|e| {
            ServerError::Storage(format!("write staged index {}: {}", tmp_path.display(), e))
        })?;
        std::fs::rename(&tmp_path, &path).map_err(|e| {
            ServerError::Storage(format!("replace staged index {}: {}", path.display(), e))
        })?;
        Ok(())
    }

    pub fn add(&self, staged: StagedSnapshot) -> Result<(), ServerError> {
        let mut index = self.load_index()?;
        index.snapshots.retain(|existing| existing.id != staged.id);
        index.add(staged);
        self.save_index(&index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::core::id::{AccountId, BranchId, TreeId};

    #[test]
    fn add_persists_and_reloads_staged_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = StagedStore::new(dir.path().to_path_buf());
        let staged = StagedSnapshot::new(
            AccountId::new(),
            TreeId::new(),
            BranchId::new(),
            "main",
            vec!["src/lib.rs".to_string()],
        );
        let staged_id = staged.id;

        store.add(staged).unwrap();

        let index = store.load_index().unwrap();
        assert_eq!(index.snapshots.len(), 1);
        assert_eq!(index.snapshots[0].id, staged_id);
        assert_eq!(index.snapshots[0].files_changed, vec!["src/lib.rs"]);
    }
}
