use serde::{Deserialize, Serialize};

/// A named automation rule that pairs a trigger condition with an action.
///
/// Rules are evaluated by the engine whenever new events arrive. If a rule's
/// condition is satisfied, its action is executed (e.g. creating a snapshot,
/// branching, or sending a notification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Human-readable name for this rule (e.g. "auto-snapshot-on-inactivity").
    pub name: String,

    /// The condition that must be met for this rule to fire.
    pub condition: RuleCondition,

    /// The action to perform when the condition is satisfied.
    pub action: RuleAction,
}

/// Conditions that can trigger a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCondition {
    /// Fires when the number of changed (dirty) files exceeds the given threshold.
    FileCountExceeds(usize),

    /// Fires after the specified number of seconds of inactivity following a change.
    InactivityTimeout(u64),

    /// Fires when a changed file's path matches the given glob pattern.
    PathPattern(String),
}

/// Actions that a rule can perform when its condition is met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleAction {
    /// Create a snapshot (auto-commit) with the given message template.
    CreateSnapshot(String),

    /// Create a new branch with the given name template.
    CreateBranch(String),

    /// Send a notification with the given message.
    Notify(String),
}

impl Rule {
    /// Create a new rule with the given name, condition, and action.
    pub fn new(
        name: impl Into<String>,
        condition: RuleCondition,
        action: RuleAction,
    ) -> Self {
        Self {
            name: name.into(),
            condition,
            action,
        }
    }
}

impl std::fmt::Display for RuleCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleCondition::FileCountExceeds(n) => write!(f, "file_count > {}", n),
            RuleCondition::InactivityTimeout(secs) => {
                write!(f, "inactivity >= {}s", secs)
            }
            RuleCondition::PathPattern(pattern) => write!(f, "path matches '{}'", pattern),
        }
    }
}

impl std::fmt::Display for RuleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleAction::CreateSnapshot(msg) => write!(f, "create_snapshot(\"{}\")", msg),
            RuleAction::CreateBranch(name) => write!(f, "create_branch(\"{}\")", name),
            RuleAction::Notify(msg) => write!(f, "notify(\"{}\")", msg),
        }
    }
}

impl std::fmt::Display for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rule '{}': when {} => {}", self.name, self.condition, self.action)
    }
}
