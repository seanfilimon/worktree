use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// The kind of file-system event that was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A new file or directory was created.
    Created,
    /// An existing file was modified (content or metadata).
    Modified,
    /// A file or directory was deleted.
    Deleted,
    /// A file or directory was renamed (may appear as delete + create on some platforms).
    Renamed,
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventKind::Created => write!(f, "created"),
            EventKind::Modified => write!(f, "modified"),
            EventKind::Deleted => write!(f, "deleted"),
            EventKind::Renamed => write!(f, "renamed"),
        }
    }
}

/// A file-system event that has been tagged with a timestamp and normalized
/// path, ready for debouncing.
#[derive(Debug, Clone)]
pub struct DebouncedEvent {
    /// The path that was affected.
    pub path: PathBuf,
    /// What kind of change occurred.
    pub kind: EventKind,
    /// When the event was observed (UTC).
    pub timestamp: DateTime<Utc>,
}

impl DebouncedEvent {
    /// Create a new `DebouncedEvent` with the current UTC timestamp.
    pub fn now(path: PathBuf, kind: EventKind) -> Self {
        Self {
            path,
            kind,
            timestamp: Utc::now(),
        }
    }

    /// Create a new `DebouncedEvent` with an explicit timestamp.
    pub fn new(path: PathBuf, kind: EventKind, timestamp: DateTime<Utc>) -> Self {
        Self {
            path,
            kind,
            timestamp,
        }
    }
}

/// Collects raw file-system events and collapses rapid successive changes
/// to the same path within a configurable delay window.
///
/// The debouncer keeps an internal buffer of events. Callers [`push`] events
/// in as they arrive and periodically call [`flush`] to retrieve the
/// deduplicated batch.
pub struct Debouncer {
    /// The debounce window in milliseconds. Events affecting the same path
    /// within this window are collapsed into the most recent event.
    pub delay_ms: u64,

    /// Internal buffer of pending events, keyed by path for fast dedup.
    pending: Vec<DebouncedEvent>,
}

impl Debouncer {
    /// Create a new `Debouncer` with the given delay window in milliseconds.
    pub fn new(delay_ms: u64) -> Self {
        Self {
            delay_ms,
            pending: Vec::new(),
        }
    }

    /// Push a new event into the debouncer.
    ///
    /// If an event for the same path already exists in the buffer and the new
    /// event falls within the debounce window, the existing event is replaced
    /// (keeping only the latest change for that path). Otherwise the event is
    /// appended.
    pub fn push(&mut self, event: DebouncedEvent) {
        let dominated = self.pending.iter().position(|existing| {
            if existing.path != event.path {
                return false;
            }
            let delta = event
                .timestamp
                .signed_duration_since(existing.timestamp)
                .num_milliseconds()
                .unsigned_abs();
            delta <= self.delay_ms as u64
        });

        if let Some(idx) = dominated {
            // Replace the older event with the newer one.
            self.pending[idx] = event;
        } else {
            self.pending.push(event);
        }
    }

    /// Flush all pending events whose debounce window has elapsed relative to
    /// `now`, returning them in chronological order.
    ///
    /// Events whose window has **not** yet elapsed remain in the buffer for a
    /// future flush.
    pub fn flush(&mut self) -> Vec<DebouncedEvent> {
        let now = Utc::now();
        let delay = chrono::Duration::milliseconds(self.delay_ms as i64);

        let mut ready = Vec::new();
        let mut remaining = Vec::new();

        for event in self.pending.drain(..) {
            if now.signed_duration_since(event.timestamp) >= delay {
                ready.push(event);
            } else {
                remaining.push(event);
            }
        }

        self.pending = remaining;

        // Return in chronological order.
        ready.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        ready
    }

    /// Flush **all** pending events regardless of whether their debounce window
    /// has elapsed. Useful during shutdown.
    pub fn flush_all(&mut self) -> Vec<DebouncedEvent> {
        let mut events: Vec<DebouncedEvent> = self.pending.drain(..).collect();
        events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        events
    }

    /// Returns the number of events currently buffered.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Returns `true` if there are no pending events.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn push_collapses_same_path_within_window() {
        let mut debouncer = Debouncer::new(500);
        let now = Utc::now();

        debouncer.push(DebouncedEvent::new(
            PathBuf::from("src/main.rs"),
            EventKind::Modified,
            now,
        ));
        debouncer.push(DebouncedEvent::new(
            PathBuf::from("src/main.rs"),
            EventKind::Modified,
            now + Duration::milliseconds(100),
        ));

        assert_eq!(debouncer.pending_count(), 1);
    }

    #[test]
    fn push_keeps_different_paths() {
        let mut debouncer = Debouncer::new(500);
        let now = Utc::now();

        debouncer.push(DebouncedEvent::new(
            PathBuf::from("src/main.rs"),
            EventKind::Modified,
            now,
        ));
        debouncer.push(DebouncedEvent::new(
            PathBuf::from("src/lib.rs"),
            EventKind::Created,
            now,
        ));

        assert_eq!(debouncer.pending_count(), 2);
    }

    #[test]
    fn flush_all_drains_everything() {
        let mut debouncer = Debouncer::new(500);

        debouncer.push(DebouncedEvent::now(PathBuf::from("a.rs"), EventKind::Created));
        debouncer.push(DebouncedEvent::now(PathBuf::from("b.rs"), EventKind::Deleted));

        let events = debouncer.flush_all();
        assert_eq!(events.len(), 2);
        assert!(debouncer.is_empty());
    }
}
