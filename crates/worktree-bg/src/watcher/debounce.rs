//! Auto-snapshot debouncing, driven by a worktree's `[auto_snapshot]` config.
//!
//! The [`Debouncer`] is a pure state machine over [`tokio::time::Instant`], so
//! it can be tested deterministically with a paused tokio clock. It tracks the
//! set of changed paths since the last snapshot and decides when a snapshot
//! is due: after `inactivity` without new events, or immediately once
//! `max_changed_files` distinct paths have changed.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use tokio::time::Instant;
use worktree_sdk::config::AutoSnapshotSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebounceSettings {
    /// Auto-snapshots are taken at all.
    pub enabled: bool,
    /// Quiet period after the last event before a snapshot is due.
    pub inactivity: Duration,
    /// Snapshot as soon as this many distinct paths changed (0 = no limit).
    pub max_changed_files: usize,
    /// local-config (§13.9): snapshot as soon as the changed files total
    /// this many bytes (0 = no limit).
    pub max_changed_bytes: u64,
}

impl From<&AutoSnapshotSection> for DebounceSettings {
    fn from(section: &AutoSnapshotSection) -> Self {
        Self {
            enabled: section.enabled,
            inactivity: Duration::from_secs(section.inactivity_secs),
            max_changed_files: section.max_changed_files,
            max_changed_bytes: section.max_changed_bytes,
        }
    }
}

impl Default for DebounceSettings {
    fn default() -> Self {
        Self::from(&AutoSnapshotSection::default())
    }
}

#[derive(Debug, Clone)]
pub struct Debouncer {
    settings: DebounceSettings,
    paused: bool,
    pending: BTreeSet<String>,
    last_event: Option<Instant>,
    /// local-config (§13.9): latest known size of each changed path.
    pending_bytes: BTreeMap<String, u64>,
}

impl Debouncer {
    pub fn new(settings: DebounceSettings) -> Self {
        Self {
            settings,
            paused: false,
            pending: BTreeSet::new(),
            last_event: None,
            pending_bytes: BTreeMap::new(),
        }
    }

    /// local-config (§13.9): note the current size of a changed path (for
    /// `max_changed_bytes`). Call alongside [`Self::record`].
    pub fn record_bytes(&mut self, path: &str, bytes: u64) {
        self.pending_bytes.insert(path.to_string(), bytes);
    }

    /// Total bytes of the changed paths with a known size.
    pub fn pending_bytes(&self) -> u64 {
        self.pending_bytes.values().sum()
    }

    pub fn settings(&self) -> DebounceSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: DebounceSettings) {
        self.settings = settings;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// While paused, changes keep accumulating but no snapshot becomes due.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Record a change to `path` observed at `now`.
    pub fn record(&mut self, path: impl Into<String>, now: Instant) {
        self.pending.insert(path.into());
        self.last_event = Some(now);
    }

    /// Distinct paths changed since the last [`take`](Self::take).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn armed(&self) -> bool {
        self.settings.enabled && !self.paused && !self.pending.is_empty()
    }

    fn over_limit(&self) -> bool {
        (self.settings.max_changed_files > 0
            && self.pending.len() >= self.settings.max_changed_files)
            || (self.settings.max_changed_bytes > 0
                && self.pending_bytes() >= self.settings.max_changed_bytes)
    }

    /// When the next snapshot becomes due, if one is pending at all.
    pub fn deadline(&self) -> Option<Instant> {
        if !self.armed() {
            return None;
        }
        if self.over_limit() {
            return self.last_event;
        }
        self.last_event.map(|t| t + self.settings.inactivity)
    }

    /// Whether a snapshot is due at `now`.
    pub fn is_ready(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }

    /// Clear the pending set, returning the changed paths (sorted).
    pub fn take(&mut self) -> Vec<String> {
        self.last_event = None;
        self.pending_bytes.clear();
        std::mem::take(&mut self.pending).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(inactivity_secs: u64, max: usize) -> DebounceSettings {
        DebounceSettings {
            enabled: true,
            inactivity: Duration::from_secs(inactivity_secs),
            max_changed_files: max,
            max_changed_bytes: 0,
        }
    }

    #[test]
    fn max_changed_bytes_fires_immediately() {
        let mut d = Debouncer::new(DebounceSettings {
            max_changed_bytes: 100,
            ..settings(30, 0)
        });
        let t0 = Instant::now();
        d.record_bytes("a", 60);
        d.record("a", t0);
        assert!(!d.is_ready(t0));
        // A rewrite of the same path replaces its size.
        d.record_bytes("a", 70);
        d.record("a", t0);
        assert!(!d.is_ready(t0));
        d.record_bytes("b", 30);
        d.record("b", t0);
        assert_eq!(d.pending_bytes(), 100);
        assert!(d.is_ready(t0));
        d.take();
        assert_eq!(d.pending_bytes(), 0);
    }

    #[test]
    fn settings_follow_config_section() {
        let section = AutoSnapshotSection {
            enabled: false,
            inactivity_secs: 7,
            max_changed_files: 3,
            max_changed_bytes: 9,
            ..AutoSnapshotSection::default()
        };
        let s = DebounceSettings::from(&section);
        assert!(!s.enabled);
        assert_eq!(s.inactivity, Duration::from_secs(7));
        assert_eq!(s.max_changed_files, 3);
        assert_eq!(s.max_changed_bytes, 9);
        assert_eq!(
            DebounceSettings::default(),
            DebounceSettings::from(&AutoSnapshotSection::default())
        );
    }

    #[test]
    fn idle_debouncer_has_no_deadline() {
        let d = Debouncer::new(settings(30, 50));
        assert_eq!(d.deadline(), None);
        assert!(!d.is_ready(Instant::now()));
        assert_eq!(d.pending_count(), 0);
    }

    #[test]
    fn deadline_is_inactivity_after_last_event() {
        let mut d = Debouncer::new(settings(30, 50));
        let t0 = Instant::now();
        d.record("a.txt", t0);
        assert_eq!(d.deadline(), Some(t0 + Duration::from_secs(30)));
        assert!(!d.is_ready(t0 + Duration::from_secs(29)));
        assert!(d.is_ready(t0 + Duration::from_secs(30)));

        // A later event pushes the deadline out.
        d.record("b.txt", t0 + Duration::from_secs(20));
        assert_eq!(d.deadline(), Some(t0 + Duration::from_secs(50)));
        assert!(!d.is_ready(t0 + Duration::from_secs(30)));
    }

    #[test]
    fn same_path_counts_once() {
        let mut d = Debouncer::new(settings(30, 50));
        let t0 = Instant::now();
        d.record("a.txt", t0);
        d.record("a.txt", t0 + Duration::from_secs(1));
        d.record("b.txt", t0 + Duration::from_secs(2));
        assert_eq!(d.pending_count(), 2);
    }

    #[test]
    fn reaching_max_changed_files_is_immediately_ready() {
        let mut d = Debouncer::new(settings(30, 3));
        let t0 = Instant::now();
        d.record("a", t0);
        d.record("b", t0);
        assert!(!d.is_ready(t0));
        d.record("c", t0);
        assert!(d.is_ready(t0));
        assert_eq!(d.deadline(), Some(t0));
    }

    #[test]
    fn zero_max_means_no_limit() {
        let mut d = Debouncer::new(settings(30, 0));
        let t0 = Instant::now();
        for i in 0..1000 {
            d.record(format!("f{i}"), t0);
        }
        assert!(!d.is_ready(t0));
        assert!(d.is_ready(t0 + Duration::from_secs(30)));
    }

    #[test]
    fn disabled_never_becomes_ready_but_tracks_changes() {
        let mut d = Debouncer::new(DebounceSettings {
            enabled: false,
            ..settings(1, 1)
        });
        let t0 = Instant::now();
        d.record("a", t0);
        assert_eq!(d.pending_count(), 1);
        assert_eq!(d.deadline(), None);
        assert!(!d.is_ready(t0 + Duration::from_secs(3600)));
    }

    #[test]
    fn pause_suspends_and_resume_restores_deadline() {
        let mut d = Debouncer::new(settings(10, 50));
        let t0 = Instant::now();
        d.set_paused(true);
        assert!(d.is_paused());
        d.record("a", t0);
        assert!(!d.is_ready(t0 + Duration::from_secs(60)));
        d.set_paused(false);
        assert!(d.is_ready(t0 + Duration::from_secs(60)));
    }

    #[test]
    fn take_clears_state_and_returns_sorted_paths() {
        let mut d = Debouncer::new(settings(10, 50));
        let t0 = Instant::now();
        d.record("z", t0);
        d.record("a", t0);
        assert_eq!(d.take(), vec!["a".to_string(), "z".to_string()]);
        assert_eq!(d.pending_count(), 0);
        assert_eq!(d.deadline(), None);
    }

    #[test]
    fn settings_change_applies_to_pending_changes() {
        let mut d = Debouncer::new(settings(30, 50));
        let t0 = Instant::now();
        d.record("a", t0);
        d.set_settings(settings(5, 50));
        assert_eq!(d.settings().inactivity, Duration::from_secs(5));
        assert!(d.is_ready(t0 + Duration::from_secs(5)));
    }
}
