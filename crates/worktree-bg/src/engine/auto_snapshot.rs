use crate::engine::event::SemanticEvent;

/// Engine responsible for deciding when to automatically create a snapshot.
///
/// The `AutoSnapshotEngine` examines a batch of semantic events and
/// determines whether a snapshot should be created. If so, it returns a
/// suggested snapshot message describing the changes.
pub struct AutoSnapshotEngine {
    /// Minimum number of events before considering an auto-snapshot.
    pub min_event_threshold: usize,

    /// Maximum number of events to accumulate before forcing a snapshot.
    pub max_event_threshold: usize,
}

impl AutoSnapshotEngine {
    /// Create a new `AutoSnapshotEngine` with sensible defaults.
    pub fn new() -> Self {
        Self {
            min_event_threshold: 1,
            max_event_threshold: 100,
        }
    }

    /// Create a new `AutoSnapshotEngine` with custom thresholds.
    pub fn with_thresholds(min_event_threshold: usize, max_event_threshold: usize) -> Self {
        Self {
            min_event_threshold,
            max_event_threshold,
        }
    }

    /// Evaluate a batch of semantic events and decide whether to create a
    /// snapshot.
    ///
    /// Returns `Some(message)` with a suggested snapshot message if a
    /// snapshot should be created, or `None` if the events do not yet
    /// warrant one. The timing side of the decision (inactivity windows,
    /// forced flushes) belongs to the daemon loop; this engine judges the
    /// batch itself.
    pub fn evaluate(&self, events: &[SemanticEvent]) -> Option<String> {
        if events.len() < self.min_event_threshold.max(1) {
            return None;
        }

        let mut code = 0usize;
        let mut deps = 0usize;
        let mut config = 0usize;
        for event in events {
            match event {
                SemanticEvent::CodeChange { .. } => code += 1,
                SemanticEvent::DependencyChange { .. } => deps += 1,
                SemanticEvent::ConfigChange { .. } => config += 1,
            }
        }

        let mut parts = Vec::new();
        if code > 0 {
            parts.push(format!("{code} code"));
        }
        if deps > 0 {
            parts.push(format!("{deps} dependency"));
        }
        if config > 0 {
            parts.push(format!("{config} config"));
        }

        // Name up to three files for context.
        let sample: Vec<&str> = events.iter().take(3).map(|e| e.path()).collect();
        let ellipsis = if events.len() > sample.len() {
            ", …"
        } else {
            ""
        };

        Some(format!(
            "auto-snapshot: {} file(s) changed ({}) — {}{}",
            events.len(),
            parts.join(", "),
            sample.join(", "),
            ellipsis
        ))
    }
}

impl Default for AutoSnapshotEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(path: &str) -> SemanticEvent {
        SemanticEvent::CodeChange {
            tree_name: "root".into(),
            path: path.into(),
        }
    }

    #[test]
    fn empty_batch_is_none() {
        assert!(AutoSnapshotEngine::new().evaluate(&[]).is_none());
    }

    #[test]
    fn below_min_threshold_is_none() {
        let engine = AutoSnapshotEngine::with_thresholds(3, 100);
        assert!(engine.evaluate(&[code("a.rs")]).is_none());
    }

    #[test]
    fn message_summarizes_batch() {
        let engine = AutoSnapshotEngine::new();
        let events = vec![
            code("src/a.rs"),
            code("src/b.rs"),
            SemanticEvent::DependencyChange {
                tree_name: "root".into(),
                path: "Cargo.toml".into(),
            },
        ];
        let message = engine.evaluate(&events).unwrap();
        assert!(message.contains("3 file(s)"), "{message}");
        assert!(message.contains("2 code"), "{message}");
        assert!(message.contains("1 dependency"), "{message}");
        assert!(message.contains("src/a.rs"), "{message}");
    }
}
