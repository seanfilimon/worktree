use crate::engine::event::SemanticEvent;

/// Engine responsible for generating a commit message for the grouped changes.
///
/// The `AutoCommitEngine` examines a batch of semantic events and determines
/// a suggested commit message describing the changes that occurred over the interval.
pub struct AutoCommitEngine {}

impl AutoCommitEngine {
    /// Create a new `AutoCommitEngine`.
    pub fn new() -> Self {
        Self {}
    }

    /// Evaluate a batch of semantic events and decide on a commit message.
    ///
    /// Returns `Some(message)` with a suggested snapshot message, or `None` if
    /// there are no events.
    pub fn evaluate(&self, events: &[SemanticEvent]) -> Option<String> {
        let count = events.len();

        if count == 0 {
            return None;
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

impl Default for AutoCommitEngine {
    fn default() -> Self {
        Self::new()
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
