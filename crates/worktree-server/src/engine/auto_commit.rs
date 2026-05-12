use crate::engine::event::SemanticEvent;

/// Engine responsible for deciding when to automatically create a snapshot (commit).
///
/// The `AutoCommitEngine` examines a batch of semantic events and determines
/// whether a snapshot should be created. If so, it returns a suggested commit
/// message describing the changes.
pub struct AutoCommitEngine {
    /// Minimum number of events before considering an auto-commit.
    pub min_event_threshold: usize,

    /// Maximum number of events to accumulate before forcing a commit.
    pub max_event_threshold: usize,
}

impl AutoCommitEngine {
    /// Create a new `AutoCommitEngine` with sensible defaults.
    pub fn new() -> Self {
        Self {
            min_event_threshold: 1,
            max_event_threshold: 100,
        }
    }

    /// Create a new `AutoCommitEngine` with custom thresholds.
    pub fn with_thresholds(min_event_threshold: usize, max_event_threshold: usize) -> Self {
        Self {
            min_event_threshold,
            max_event_threshold,
        }
    }

    /// Evaluate a batch of semantic events and decide whether to create a snapshot.
    ///
    /// Returns `Some(message)` with a suggested snapshot message if a snapshot
    /// should be created, or `None` if the events do not yet warrant one.
    pub fn evaluate(&self, events: &[SemanticEvent]) -> Option<String> {
        let count = events.len();

        if count < self.min_event_threshold {
            return None;
        }

        if count >= self.max_event_threshold {
            return Some(format!(
                "auto-snapshot: {} files changed (threshold reached)",
                count
            ));
        }

        let has_dependency = events
            .iter()
            .any(|e| matches!(e, SemanticEvent::DependencyChange { .. }));
        let has_config = events
            .iter()
            .any(|e| matches!(e, SemanticEvent::ConfigChange { .. }));
        let code_path_count: usize = events
            .iter()
            .filter_map(|e| match e {
                SemanticEvent::CodeChange { paths, .. } => Some(paths.len()),
                _ => None,
            })
            .sum();

        if has_dependency {
            Some("auto-snapshot: dependency changes".to_string())
        } else if has_config {
            Some("auto-snapshot: config changes".to_string())
        } else {
            Some(format!(
                "auto-snapshot: {} file(s) changed",
                code_path_count.max(1)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use worktree_protocol::core::id::TreeId;

    fn code_event(n_paths: usize) -> SemanticEvent {
        SemanticEvent::CodeChange {
            tree_id: TreeId::nil(),
            paths: (0..n_paths)
                .map(|i| PathBuf::from(format!("src/file{}.rs", i)))
                .collect(),
        }
    }

    fn dep_event() -> SemanticEvent {
        SemanticEvent::DependencyChange {
            tree_id: TreeId::nil(),
            path: PathBuf::from("Cargo.toml"),
        }
    }

    fn cfg_event() -> SemanticEvent {
        SemanticEvent::ConfigChange {
            tree_id: TreeId::nil(),
            path: PathBuf::from(".editorconfig"),
        }
    }

    #[test]
    fn empty_events_returns_none() {
        let engine = AutoCommitEngine::new();
        assert_eq!(engine.evaluate(&[]), None);
    }

    #[test]
    fn below_min_threshold_returns_none() {
        let engine = AutoCommitEngine::with_thresholds(3, 100);
        assert_eq!(engine.evaluate(&[code_event(1), code_event(1)]), None);
    }

    #[test]
    fn at_or_above_max_threshold_returns_threshold_message() {
        let engine = AutoCommitEngine::with_thresholds(1, 2);
        let events = vec![code_event(1), code_event(1)];
        let msg = engine.evaluate(&events).unwrap();
        assert!(msg.contains("threshold reached"), "got: {msg}");
    }

    #[test]
    fn dependency_change_takes_priority() {
        let engine = AutoCommitEngine::new();
        let events = vec![code_event(2), dep_event()];
        let msg = engine.evaluate(&events).unwrap();
        assert_eq!(msg, "auto-snapshot: dependency changes");
    }

    #[test]
    fn config_change_message() {
        let engine = AutoCommitEngine::new();
        let events = vec![cfg_event()];
        let msg = engine.evaluate(&events).unwrap();
        assert_eq!(msg, "auto-snapshot: config changes");
    }

    #[test]
    fn code_change_message_includes_path_count() {
        let engine = AutoCommitEngine::new();
        let events = vec![code_event(3)];
        let msg = engine.evaluate(&events).unwrap();
        assert!(msg.contains('3'), "got: {msg}");
    }
}

impl Default for AutoCommitEngine {
    fn default() -> Self {
        Self::new()
    }
}
