//! `$WT_HOME/bg.json`: how clients find the running daemon.

use std::path::{Path, PathBuf};

use worktree_sdk::ipc::{read_daemon_info_at, DaemonInfo};

use crate::config::write_atomic;
use crate::error::Result;

/// File name of the daemon info file inside `$WT_HOME`.
pub const INFO_FILE: &str = "bg.json";

/// Owns a written `bg.json`; removes it when dropped, unless another daemon
/// instance has replaced it in the meantime.
#[derive(Debug)]
pub struct DaemonInfoFile {
    path: PathBuf,
    info: DaemonInfo,
}

impl DaemonInfoFile {
    /// Atomically write `info` to `path`.
    pub fn create(path: &Path, info: DaemonInfo) -> Result<Self> {
        let json = serde_json::to_vec_pretty(&info)?;
        write_atomic(path, &json)?;
        // The file holds the IPC token: owner-only where the OS supports it
        // (on Windows the per-user profile directory's ACL protects it).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            info,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self) -> &DaemonInfo {
        &self.info
    }
}

impl Drop for DaemonInfoFile {
    fn drop(&mut self) {
        match read_daemon_info_at(&self.path) {
            Ok(Some(current)) if current == self.info => {
                if let Err(e) = std::fs::remove_file(&self.path) {
                    tracing::warn!(path = %self.path.display(), error = %e, "failed to remove daemon info file");
                }
            }
            Ok(Some(_)) => {
                tracing::warn!(path = %self.path.display(), "daemon info file belongs to another instance; leaving it");
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(path = %self.path.display(), error = %e, "unreadable daemon info file; leaving it");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(port: u16) -> DaemonInfo {
        DaemonInfo {
            pid: std::process::id(),
            port,
            version: "1.2.3".into(),
            token: "tok".into(),
        }
    }

    #[test]
    fn write_read_and_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INFO_FILE);
        {
            let file = DaemonInfoFile::create(&path, info(4242)).unwrap();
            assert_eq!(file.path(), path);
            assert_eq!(
                read_daemon_info_at(&path).unwrap().as_ref(),
                Some(file.info())
            );
        }
        assert!(!path.exists(), "bg.json must be removed on drop");
    }

    #[test]
    fn create_overwrites_stale_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INFO_FILE);
        std::fs::write(&path, "{\"pid\":1,\"port\":1,\"version\":\"old\"}").unwrap();
        let file = DaemonInfoFile::create(&path, info(7)).unwrap();
        assert_eq!(read_daemon_info_at(&path).unwrap(), Some(info(7)));
        drop(file);
        assert!(!path.exists());
    }

    #[test]
    fn drop_leaves_file_of_another_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INFO_FILE);
        let ours = DaemonInfoFile::create(&path, info(1000)).unwrap();
        let theirs = DaemonInfo {
            pid: 1,
            port: 2000,
            version: "x".into(),
            token: String::new(),
        };
        std::fs::write(&path, serde_json::to_vec(&theirs).unwrap()).unwrap();
        drop(ours);
        assert_eq!(read_daemon_info_at(&path).unwrap(), Some(theirs));
    }

    #[test]
    fn drop_tolerates_already_removed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INFO_FILE);
        let file = DaemonInfoFile::create(&path, info(1)).unwrap();
        std::fs::remove_file(&path).unwrap();
        drop(file);
        assert!(!path.exists());
    }
}
