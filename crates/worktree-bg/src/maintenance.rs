//! Periodic maintenance of watched worktrees (ENGINEERING_PLAN §13.11):
//! reflog expiry and stale temp/lock leftovers.
//!
//! Reflog expiry belongs in the SDK (`merge-diff` workstream, §13.10); until
//! that API exists this module rewrites `.wt/trees/<tree>/reflog.jsonl`
//! itself, holding the same per-tree OS lock (`trees/<tree>/LOCK`) the SDK
//! takes for mutations so it never races a concurrent append. Switch to the
//! SDK call once it lands.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use worktree_sdk::model::ReflogEntry;

/// Default reflog retention: entries older than this are removed...
pub const REFLOG_MAX_AGE: chrono::Duration = chrono::Duration::days(90);
/// ...and at most this many (the newest) are kept per tree.
pub const REFLOG_MAX_ENTRIES: usize = 1000;
/// SDK temp files (`.wt-tmp-*`) older than this are leftovers of a crash.
pub const STALE_TEMP_AGE: Duration = Duration::from_secs(3600);
/// How long to wait for a tree lock before skipping the tree this round.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// Prefix of SDK temp files (`repo::fsutil::TEMP_PREFIX`).
const TEMP_PREFIX: &str = ".wt-tmp-";

/// What one maintenance pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub reflog_entries_removed: usize,
    pub temp_files_removed: usize,
    pub errors: Vec<String>,
}

impl MaintenanceReport {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "expired {} reflog entr{}, removed {} stale temp file(s)",
            self.reflog_entries_removed,
            if self.reflog_entries_removed == 1 {
                "y"
            } else {
                "ies"
            },
            self.temp_files_removed
        );
        if !self.errors.is_empty() {
            s.push_str(&format!("; errors: {}", self.errors.join("; ")));
        }
        s
    }
}

/// Run maintenance on the worktree at `root` (blocking I/O).
pub fn run(root: &Path, now: DateTime<Utc>) -> MaintenanceReport {
    let mut report = MaintenanceReport::default();
    let trees_dir = root.join(".wt").join("trees");
    match std::fs::read_dir(&trees_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.join("tree.toml").is_file() {
                    continue;
                }
                match expire_tree_reflog(&dir, now, REFLOG_MAX_AGE, REFLOG_MAX_ENTRIES) {
                    Ok(n) => report.reflog_entries_removed += n,
                    Err(e) => report
                        .errors
                        .push(format!("reflog of {}: {e}", dir.display())),
                }
            }
        }
        Err(e) => report
            .errors
            .push(format!("reading {}: {e}", trees_dir.display())),
    }
    report.temp_files_removed = remove_stale_temp_files(&root.join(".wt"), STALE_TEMP_AGE);
    report
}

/// Expire the reflog in tree metadata directory `dir` under its lock.
/// Returns the number of entries removed (0 if the lock is busy).
pub fn expire_tree_reflog(
    dir: &Path,
    now: DateTime<Utc>,
    max_age: chrono::Duration,
    keep: usize,
) -> std::io::Result<usize> {
    let path = dir.join("reflog.jsonl");
    if !path.is_file() {
        return Ok(0);
    }
    let Some(_lock) = lock_tree(&dir.join("LOCK"))? else {
        tracing::debug!(dir = %dir.display(), "tree busy; skipping reflog expiry");
        return Ok(0);
    };
    let text = std::fs::read_to_string(&path)?;
    let (kept, removed) = expire_lines(&text, now, max_age, keep);
    if removed == 0 {
        return Ok(0);
    }
    let tmp = dir.join(format!("{TEMP_PREFIX}reflog-{}", std::process::id()));
    std::fs::write(&tmp, kept)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(removed)
}

/// Keep the newest `keep` entries that are at most `max_age` old (and drop
/// unparsable lines). Returns the new file text and the count removed.
pub fn expire_lines(
    text: &str,
    now: DateTime<Utc>,
    max_age: chrono::Duration,
    keep: usize,
) -> (String, usize) {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let cutoff = now - max_age;
    let mut kept_rev = Vec::new();
    for line in lines.iter().rev() {
        if kept_rev.len() >= keep {
            break;
        }
        match serde_json::from_str::<ReflogEntry>(line) {
            Ok(e) if e.at >= cutoff => kept_rev.push(*line),
            _ => {}
        }
    }
    let removed = lines.len() - kept_rev.len();
    let mut out = String::new();
    for line in kept_rev.iter().rev() {
        out.push_str(line);
        out.push('\n');
    }
    (out, removed)
}

/// Take the SDK's per-tree OS lock; `None` if still busy after [`LOCK_WAIT`].
fn lock_tree(path: &Path) -> std::io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// Remove `.wt-tmp-*` files under `dir` (recursively) older than `age`.
pub fn remove_stale_temp_files(dir: &Path, age: Duration) -> usize {
    let now = SystemTime::now();
    let mut removed = 0;
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|elapsed| elapsed >= age);
            if old && std::fs::remove_file(&path).is_ok() {
                tracing::info!(path = %path.display(), "removed stale temp file");
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: DateTime<Utc>, n: usize) -> String {
        serde_json::to_string(&ReflogEntry {
            at,
            tree: "t".into(),
            branch: "main".into(),
            action: "snapshot".into(),
            old: None,
            new: Some(format!("s{n}")),
            message: format!("m{n}"),
        })
        .unwrap()
    }

    #[test]
    fn expiry_keeps_recent_entries_up_to_the_cap() {
        let now = Utc::now();
        let mut text = String::new();
        text.push_str(&entry(now - chrono::Duration::days(200), 0));
        text.push('\n');
        text.push_str("{not json\n");
        for i in 1..=5 {
            text.push_str(&entry(now - chrono::Duration::days(10 - i as i64), i));
            text.push('\n');
        }
        let (kept, removed) = expire_lines(&text, now, REFLOG_MAX_AGE, 3);
        assert_eq!(removed, 4, "old, malformed and two over the cap");
        let msgs: Vec<String> = kept
            .lines()
            .map(|l| serde_json::from_str::<ReflogEntry>(l).unwrap().message)
            .collect();
        assert_eq!(msgs, vec!["m3", "m4", "m5"], "newest kept, order preserved");

        let (same, none) = expire_lines(&kept, now, REFLOG_MAX_AGE, 3);
        assert_eq!((same.as_str(), none), (kept.as_str(), 0));
    }

    #[test]
    fn tree_reflog_is_rewritten_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let text = format!(
            "{}\n{}\n",
            entry(now - chrono::Duration::days(100), 1),
            entry(now, 2)
        );
        std::fs::write(dir.path().join("reflog.jsonl"), text).unwrap();
        let n = expire_tree_reflog(dir.path(), now, REFLOG_MAX_AGE, 1000).unwrap();
        assert_eq!(n, 1);
        let left = std::fs::read_to_string(dir.path().join("reflog.jsonl")).unwrap();
        assert_eq!(left.lines().count(), 1);
        assert!(left.contains("\"m2\""));
        assert_eq!(
            expire_tree_reflog(&dir.path().join("missing"), now, REFLOG_MAX_AGE, 10).unwrap(),
            0
        );
    }

    #[test]
    fn busy_tree_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let now = Utc::now();
        std::fs::write(
            dir.path().join("reflog.jsonl"),
            format!("{}\n", entry(now - chrono::Duration::days(365), 1)),
        )
        .unwrap();
        let held = lock_tree(&dir.path().join("LOCK")).unwrap().unwrap();
        // A second handle in this process contends like another process would.
        let n = expire_tree_reflog(dir.path(), now, REFLOG_MAX_AGE, 10).unwrap();
        assert_eq!(n, 0);
        drop(held);
        assert_eq!(
            expire_tree_reflog(dir.path(), now, REFLOG_MAX_AGE, 10).unwrap(),
            1
        );
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("objects").join("ab");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join(".wt-tmp-x-1-2-3"), b"junk").unwrap();
        std::fs::write(nested.join("keep"), b"data").unwrap();
        assert_eq!(
            remove_stale_temp_files(dir.path(), STALE_TEMP_AGE),
            0,
            "fresh"
        );
        assert_eq!(remove_stale_temp_files(dir.path(), Duration::ZERO), 1);
        assert!(nested.join("keep").exists());
        assert!(!nested.join(".wt-tmp-x-1-2-3").exists());
    }

    #[test]
    fn run_on_a_real_worktree() {
        let dir = tempfile::tempdir().unwrap();
        worktree_sdk::Repository::init(dir.path(), Default::default()).unwrap();
        let report = run(dir.path(), Utc::now() + chrono::Duration::days(365));
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(report.reflog_entries_removed, 1, "the init entry expired");
        assert!(report.summary().starts_with("expired 1 reflog entry"));
    }
}
