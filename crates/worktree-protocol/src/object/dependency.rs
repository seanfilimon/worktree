use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{TreeId, BranchId, SnapshotId, AccountId};

// ============================================================
// Level 1: Tree Dependencies
// ============================================================

/// A dependency between trees (declared in config)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeDependency {
    pub name: String,
    pub source_tree: TreeId,
    pub target_path: String,
    pub target_branch: String,
    pub required: bool,
}

// ============================================================
// Level 2: Branch Dependencies
// ============================================================

/// Status of a branch dependency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStatus {
    Active,
    Completed,
    Blocked,
    Stale,
}

impl Default for DependencyStatus {
    fn default() -> Self {
        DependencyStatus::Active
    }
}

/// A dependency between branches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchDependency {
    pub source_tree: TreeId,
    pub source_branch: BranchId,
    pub target_tree: TreeId,
    pub target_branch: String,
    pub status: DependencyStatus,
    pub blocking: bool,
    pub linked: bool,
    pub created_at: DateTime<Utc>,
}

impl BranchDependency {
    pub fn new(
        source_tree: TreeId,
        source_branch: BranchId,
        target_tree: TreeId,
        target_branch: &str,
    ) -> Self {
        Self {
            source_tree,
            source_branch,
            target_tree,
            target_branch: target_branch.to_string(),
            status: DependencyStatus::Active,
            blocking: false,
            linked: false,
            created_at: Utc::now(),
        }
    }

    pub fn with_blocking(mut self) -> Self {
        self.blocking = true;
        self
    }

    pub fn with_linked(mut self) -> Self {
        self.linked = true;
        self
    }

    pub fn complete(&mut self) {
        self.status = DependencyStatus::Completed;
    }

    pub fn block(&mut self) {
        self.status = DependencyStatus::Blocked;
    }

    pub fn is_blocking(&self) -> bool {
        self.blocking && self.status != DependencyStatus::Completed
    }
}

// ============================================================
// Level 2.5: Linked Branches
// ============================================================

/// A group of linked branches across trees that must be merged together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedBranchGroup {
    pub group_name: String,
    pub branches: Vec<LinkedBranch>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedBranch {
    pub tree: String,
    pub branch: String,
}

impl LinkedBranchGroup {
    pub fn new(name: &str, branches: Vec<LinkedBranch>) -> Self {
        Self {
            group_name: name.to_string(),
            branches,
            created_at: Utc::now(),
        }
    }

    pub fn add_branch(&mut self, tree: &str, branch: &str) {
        self.branches.push(LinkedBranch {
            tree: tree.to_string(),
            branch: branch.to_string(),
        });
    }

    pub fn contains_tree(&self, tree: &str) -> bool {
        self.branches.iter().any(|b| b.tree == tree)
    }
}

// ============================================================
// Level 3: Snapshot Dependencies
// ============================================================

/// Priority level for a snapshot dependency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum DependencyPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for DependencyPriority {
    fn default() -> Self {
        DependencyPriority::Medium
    }
}

/// A dependency declared at the snapshot level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDependency {
    pub tree: TreeId,
    pub branch: Option<String>,
    pub requirement: String,
    pub priority: DependencyPriority,
    pub status: DependencyStatus,
    pub blocking: bool,
    pub todo_branch: Option<String>,
    pub completed_by: Option<SnapshotId>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// TODO Branches (auto-generated from dependencies)
// ============================================================

/// State of a TODO item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoState {
    Open,
    Claimed,
    InProgress,
    Completed,
    Cancelled,
}

impl Default for TodoState {
    fn default() -> Self {
        TodoState::Open
    }
}

/// Origin of a TODO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoOrigin {
    pub tree: TreeId,
    pub branch: String,
    pub snapshot: SnapshotId,
    pub author: AccountId,
    pub timestamp: DateTime<Utc>,
}

/// Requirement details for a TODO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoRequirement {
    pub title: String,
    pub description: String,
    pub priority: DependencyPriority,
    pub blocking: bool,
    pub linked: bool,
    pub details: Option<String>,
}

/// A TODO item generated from a dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub from: TodoOrigin,
    pub requirement: TodoRequirement,
    pub state: TodoState,
    pub assigned_to: Option<AccountId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TodoItem {
    pub fn new(id: &str, from: TodoOrigin, requirement: TodoRequirement) -> Self {
        let now = Utc::now();
        Self {
            id: id.to_string(),
            from,
            requirement,
            state: TodoState::Open,
            assigned_to: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn claim(&mut self, account: AccountId) {
        self.state = TodoState::Claimed;
        self.assigned_to = Some(account);
        self.updated_at = Utc::now();
    }

    pub fn start(&mut self) {
        self.state = TodoState::InProgress;
        self.updated_at = Utc::now();
    }

    pub fn complete(&mut self) {
        self.state = TodoState::Completed;
        self.updated_at = Utc::now();
    }

    pub fn cancel(&mut self) {
        self.state = TodoState::Cancelled;
        self.updated_at = Utc::now();
    }

    pub fn is_open(&self) -> bool { self.state == TodoState::Open }
    pub fn is_completed(&self) -> bool { self.state == TodoState::Completed }
    pub fn is_blocking(&self) -> bool { self.requirement.blocking && !self.is_completed() }
}

// ============================================================
// Dependency Registry
// ============================================================

/// Central registry of all dependencies for a worktree
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DependencyRegistry {
    pub tree_deps: Vec<TreeDependency>,
    pub branch_deps: Vec<BranchDependency>,
    pub linked_groups: Vec<LinkedBranchGroup>,
    pub snapshot_deps: Vec<SnapshotDependency>,
    pub todos: Vec<TodoItem>,
}

impl DependencyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_tree_dependency(&mut self, dep: TreeDependency) {
        self.tree_deps.push(dep);
    }

    pub fn add_branch_dependency(&mut self, dep: BranchDependency) {
        self.branch_deps.push(dep);
    }

    pub fn add_linked_group(&mut self, group: LinkedBranchGroup) {
        self.linked_groups.push(group);
    }

    pub fn add_todo(&mut self, todo: TodoItem) {
        self.todos.push(todo);
    }

    pub fn blocking_deps_for_branch(&self, branch_id: &BranchId) -> Vec<&BranchDependency> {
        self.branch_deps
            .iter()
            .filter(|d| d.source_branch == *branch_id && d.is_blocking())
            .collect()
    }

    pub fn open_todos(&self) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.is_open()).collect()
    }

    pub fn todos_for_tree(&self, tree_id: &TreeId) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.from.tree == *tree_id).collect()
    }

    pub fn linked_group_for_branch(&self, tree: &str, branch: &str) -> Option<&LinkedBranchGroup> {
        self.linked_groups.iter().find(|g| {
            g.branches.iter().any(|b| b.tree == tree && b.branch == branch)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_dependency_blocking() {
        let mut dep = BranchDependency::new(
            TreeId::new(), BranchId::new(), TreeId::new(), "feature-x"
        ).with_blocking();
        assert!(dep.is_blocking());

        dep.complete();
        assert!(!dep.is_blocking());
    }

    #[test]
    fn test_linked_branch_group() {
        let mut group = LinkedBranchGroup::new("feature-auth", vec![]);
        group.add_branch("backend", "feature/auth-api");
        group.add_branch("frontend", "feature/auth-ui");
        assert!(group.contains_tree("backend"));
        assert!(group.contains_tree("frontend"));
        assert!(!group.contains_tree("shared"));
    }

    #[test]
    fn test_todo_lifecycle() {
        let origin = TodoOrigin {
            tree: TreeId::new(),
            branch: "main".to_string(),
            snapshot: SnapshotId::new(),
            author: AccountId::new(),
            timestamp: Utc::now(),
        };
        let req = TodoRequirement {
            title: "Add auth endpoint".to_string(),
            description: "Need /api/auth".to_string(),
            priority: DependencyPriority::High,
            blocking: true,
            linked: false,
            details: None,
        };
        let mut todo = TodoItem::new("TODO-001", origin, req);
        assert!(todo.is_open());
        assert!(todo.is_blocking());

        todo.claim(AccountId::new());
        assert_eq!(todo.state, TodoState::Claimed);

        todo.start();
        assert_eq!(todo.state, TodoState::InProgress);

        todo.complete();
        assert!(todo.is_completed());
        assert!(!todo.is_blocking());
    }

    #[test]
    fn test_dependency_registry() {
        let mut registry = DependencyRegistry::new();
        let branch_id = BranchId::new();

        let dep = BranchDependency::new(
            TreeId::new(), branch_id, TreeId::new(), "main"
        ).with_blocking();
        registry.add_branch_dependency(dep);

        let blocking = registry.blocking_deps_for_branch(&branch_id);
        assert_eq!(blocking.len(), 1);
    }
}
