use crate::engine::event::SemanticEvent;

/// Engine that evaluates semantic events and decides whether a new branch
/// should be suggested.
///
/// Heuristic: a burst of dependency-manifest changes usually marks the
/// start of upgrade/refactor work worth isolating. The daemon surfaces the
/// suggestion (it never creates branches behind the user's back).
pub struct AutoBranchEngine {
    /// Minimum number of dependency events before suggesting a branch.
    pub threshold: usize,
}

impl AutoBranchEngine {
    /// Create a new `AutoBranchEngine` with the given event threshold.
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }

    /// Evaluate a batch of semantic events and optionally return a
    /// suggested branch name.
    pub fn evaluate(&self, events: &[SemanticEvent]) -> Option<String> {
        let dependency_changes = events
            .iter()
            .filter(|e| matches!(e, SemanticEvent::DependencyChange { .. }))
            .count();

        if dependency_changes >= self.threshold.max(1) {
            let date = chrono::Utc::now().format("%Y%m%d");
            return Some(format!("auto/deps-{date}"));
        }
        None
    }
}

impl Default for AutoBranchEngine {
    fn default() -> Self {
        Self::new(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_branch_for_dependency_bursts() {
        let engine = AutoBranchEngine::new(2);
        let dep = |p: &str| SemanticEvent::DependencyChange {
            tree_name: "root".into(),
            path: p.into(),
        };
        assert!(engine.evaluate(&[dep("Cargo.toml")]).is_none());
        let suggestion = engine
            .evaluate(&[dep("Cargo.toml"), dep("package.json")])
            .unwrap();
        assert!(suggestion.starts_with("auto/deps-"), "{suggestion}");
    }

    #[test]
    fn code_changes_do_not_suggest() {
        let engine = AutoBranchEngine::new(1);
        let events = vec![SemanticEvent::CodeChange {
            tree_name: "root".into(),
            path: "src/main.rs".into(),
        }];
        assert!(engine.evaluate(&events).is_none());
    }
}
