//! Per-tree advisory lock.
//!
//! Mutating operations hold an exclusive OS file lock on
//! `.wt/trees/<tree>/LOCK`. OS locks are released when the holding process
//! exits, so a crash or Ctrl-C never leaves a stale lock behind. The lock is
//! not re-entrant: public entry points take it once and call unlocked
//! internal helpers.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::{Result, SdkError};

/// How long to wait for another process to release the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(10);

/// RAII guard; the lock is released when dropped.
#[derive(Debug)]
pub(crate) struct TreeLock {
    _file: File,
}

impl TreeLock {
    /// Block (up to [`LOCK_TIMEOUT`]) until the lock at `path` is acquired.
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(SdkError::Other(format!(
                        "timed out waiting for lock {} held by another operation",
                        path.display()
                    )));
                }
                Err(TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("LOCK");
        let inside = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let inside = inside.clone();
                let max_seen = max_seen.clone();
                std::thread::spawn(move || {
                    let _guard = TreeLock::acquire(&path).unwrap();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    inside.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
        // Free again afterwards.
        drop(TreeLock::acquire(&path).unwrap());
    }
}
