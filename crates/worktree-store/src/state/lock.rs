//! Advisory single-writer lock (Storage.md §Concurrent Access).

use crate::error::{Result, StoreError};
use fs4::fs_std::FileExt;
use std::fs::File;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Held exclusive lock on a store; released on drop.
#[derive(Debug)]
pub struct StoreLock {
    // Held for its OS-level lock; never read.
    _file: File,
}

impl StoreLock {
    /// Try to acquire the lock at `path`, retrying until `timeout` elapses.
    pub fn acquire(path: PathBuf, timeout: Duration) -> Result<Self> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;

        let deadline = Instant::now() + timeout;
        loop {
            // Contention surfaces as an Err on all platforms with fs4.
            if file.try_lock_exclusive().is_ok() {
                return Ok(Self { _file: file });
            }
            if Instant::now() >= deadline {
                return Err(StoreError::Locked);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
