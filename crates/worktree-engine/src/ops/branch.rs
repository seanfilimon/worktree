use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{self, load_state, BranchState};

pub fn create_branch(
    engine: &WorktreeEngine,
    name: &str,
    tree_name: Option<&str>,
) -> Result<BranchState> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.find_branch(name).is_some() {
        return Err(EngineError::InvalidConfig(format!(
            "branch '{}' already exists",
            name
        )));
    }

    let tip = tree.current_branch().and_then(|b| b.tip.clone());
    persist::create_branch(engine, &tree_name, name, tip)
}

pub fn list_branches(
    engine: &WorktreeEngine,
    tree_name: Option<&str>,
) -> Result<(Vec<BranchState>, String)> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    Ok((tree.branches.clone(), tree.current_branch.clone()))
}

pub fn switch_branch(engine: &WorktreeEngine, name: &str, tree_name: Option<&str>) -> Result<()> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.find_branch(name).is_none() {
        return Err(EngineError::BranchNotFound(name.to_string()));
    }

    persist::set_current_branch(engine, &tree_name, name)
}

pub fn delete_branch(engine: &WorktreeEngine, name: &str, tree_name: Option<&str>) -> Result<()> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if name == tree.current_branch {
        return Err(EngineError::InvalidConfig(
            "cannot delete the current branch".into(),
        ));
    }

    if name == "main" {
        return Err(EngineError::BranchProtection(
            "cannot delete the main branch".into(),
        ));
    }

    if tree.find_branch(name).is_none() {
        return Err(EngineError::BranchNotFound(name.to_string()));
    }

    persist::delete_branch(engine, &tree_name, name)
}
