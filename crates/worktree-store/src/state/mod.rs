//! Store-level state: small metadata files and the writer lock.
//!
//! ```text
//! <store>/state/
//! ├── head          ← "<tree>/<branch>" of the current checkout
//! ├── meta.json     ← caller-defined worktree metadata
//! ├── sync_state    ← last synced position per branch (WT-PHASE-5)
//! └── lock          ← advisory single-writer lock
//! ```

mod lock;

pub use lock::StoreLock;

use crate::error::Result;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Handle to the `state/` half of a store.
#[derive(Debug, Clone)]
pub struct StateDir {
    root: PathBuf,
}

impl StateDir {
    /// Open the state dir rooted at `<store>/state` (created if missing).
    pub fn new(state_root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&state_root)?;
        Ok(Self { root: state_root })
    }

    /// Atomically write a named state file.
    pub fn write(&self, name: &str, contents: &str) -> Result<()> {
        let path = self.root.join(name);
        let mut tmp = tempfile::NamedTempFile::new_in(&self.root)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.flush()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Read a named state file; `None` if it does not exist.
    pub fn read(&self, name: &str) -> Result<Option<String>> {
        match std::fs::read_to_string(self.root.join(name)) {
            Ok(contents) => Ok(Some(contents)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Acquire the store's exclusive writer lock, waiting up to `timeout`.
    ///
    /// The lock is advisory (fs4) and released on drop — including process
    /// death, so a crashed writer never wedges the store.
    pub fn lock_exclusive(&self, timeout: Duration) -> Result<StoreLock> {
        StoreLock::acquire(self.root.join("lock"), timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::new(dir.path().join("state")).unwrap();
        assert_eq!(state.read("head").unwrap(), None);
        state.write("head", "root/main").unwrap();
        assert_eq!(state.read("head").unwrap().as_deref(), Some("root/main"));
    }

    #[test]
    fn lock_excludes_second_holder() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::new(dir.path().join("state")).unwrap();

        let first = state.lock_exclusive(Duration::from_millis(100)).unwrap();
        let second = state.lock_exclusive(Duration::from_millis(100));
        assert!(second.is_err(), "second lock should time out");

        drop(first);
        state
            .lock_exclusive(Duration::from_millis(100))
            .expect("lock reacquirable after release");
    }
}
