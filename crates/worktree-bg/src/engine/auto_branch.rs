use crate::engine::event::SemanticEvent;

/// Engine that evaluates semantic events and decides whether a new branch
/// should be created automatically.
///
/// For example, if a large cross-tree refactor is detected or a new
/// dependency is introduced, the engine may suggest isolating the work
/// on a dedicated branch.
pub struct AutoBranchEngine {
    /// Minimum number of events before considering a branch suggestion.
    pub threshold: usize,
}

impl AutoBranchEngine {
    /// Create a new `AutoBranchEngine` with the given event threshold.
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }

    /// Evaluate a batch of semantic events and optionally return a suggested
    /// branch name if the heuristics determine that a new branch is warranted.
    ///
    /// Returns `None` if no branch creation is recommended.
    pub fn evaluate(&self, events: &[SemanticEvent]) -> Option<String> {
        let _ = events;
        todo!("evaluate semantic events and suggest branch name if warranted")
    }
}

impl Default for AutoBranchEngine {
    fn default() -> Self {
        Self::new(5)
    }
}
