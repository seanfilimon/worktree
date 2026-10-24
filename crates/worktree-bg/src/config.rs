//! `$WT_HOME/bg.toml`: the persisted list of watched worktree roots.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{BgError, Result};

/// File name of the daemon config inside `$WT_HOME`.
pub const CONFIG_FILE: &str = "bg.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgConfig {
    /// Worktree roots the daemon watches, in insertion order.
    #[serde(default)]
    pub watched: Vec<PathBuf>,
}

impl BgConfig {
    /// Load the config at `path`; a missing file is an empty config.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        toml::from_str(&text).map_err(|e| BgError::Config {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    /// Atomically write the config to `path`, creating parent directories.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        write_atomic(path, text.as_bytes())
    }

    /// Add `root` if absent; returns whether the list changed.
    pub fn add(&mut self, root: &Path) -> bool {
        if self.contains(root) {
            return false;
        }
        self.watched.push(root.to_path_buf());
        true
    }

    /// Remove `root` if present; returns whether the list changed.
    pub fn remove(&mut self, root: &Path) -> bool {
        let before = self.watched.len();
        self.watched.retain(|p| p != root);
        self.watched.len() != before
    }

    pub fn contains(&self, root: &Path) -> bool {
        self.watched.iter().any(|p| p == root)
    }
}

/// Write `bytes` to `path` via a sibling temp file and rename, so readers
/// never observe a partially written file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        // Windows refuses to replace a file another process holds open;
        // fall back to an in-place write rather than failing.
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            let result = std::fs::write(path, bytes);
            let _ = std::fs::remove_file(&tmp);
            result.map_err(Into::into)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = BgConfig::load(&dir.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(cfg, BgConfig::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(CONFIG_FILE);
        let mut cfg = BgConfig::default();
        assert!(cfg.add(Path::new("/work/a")));
        assert!(cfg.add(Path::new("/work/b")));
        cfg.save(&path).unwrap();

        let loaded = BgConfig::load(&path).unwrap();
        assert_eq!(loaded, cfg);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("watched"), "unexpected file contents: {text}");
    }

    #[test]
    fn add_and_remove_are_idempotent() {
        let mut cfg = BgConfig::default();
        assert!(cfg.add(Path::new("/w")));
        assert!(!cfg.add(Path::new("/w")));
        assert_eq!(cfg.watched.len(), 1);
        assert!(cfg.remove(Path::new("/w")));
        assert!(!cfg.remove(Path::new("/w")));
        assert!(cfg.watched.is_empty());
    }

    #[test]
    fn overwrite_replaces_previous_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        let mut cfg = BgConfig::default();
        cfg.add(Path::new("/one"));
        cfg.save(&path).unwrap();
        cfg.remove(Path::new("/one"));
        cfg.save(&path).unwrap();
        assert!(BgConfig::load(&path).unwrap().watched.is_empty());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temp files must not be left behind");
    }

    #[test]
    fn empty_file_is_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        std::fs::write(&path, "").unwrap();
        assert_eq!(BgConfig::load(&path).unwrap(), BgConfig::default());
    }

    #[test]
    fn corrupt_file_is_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE);
        std::fs::write(&path, "watched = 12").unwrap();
        assert!(matches!(BgConfig::load(&path), Err(BgError::Config { .. })));
    }
}
