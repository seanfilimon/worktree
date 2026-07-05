use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{load_state, save_state, BranchState};
use chrono::Utc;

pub fn create_branch(
    engine: &WorktreeEngine,
    name: &str,
    tree_name: Option<&str>,
) -> Result<BranchState> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.find_branch(name).is_some() {
        return Err(EngineError::InvalidConfig(format!(
            "branch '{}' already exists",
            name
        )));
    }

    let tip = tree.current_branch().and_then(|b| b.tip.clone());
    let branch = BranchState {
        name: name.to_string(),
        tip,
        created_at: Utc::now().to_rfc3339(),
    };

    tree.branches.push(branch.clone());
    save_state(engine, &state)?;
    Ok(branch)
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
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.find_branch(name).is_none() {
        return Err(EngineError::BranchNotFound(name.to_string()));
    }

    tree.current_branch = name.to_string();
    save_state(engine, &state)?;
    Ok(())
}

pub fn delete_branch(engine: &WorktreeEngine, name: &str, tree_name: Option<&str>) -> Result<()> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
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

    let before = tree.branches.len();
    tree.branches.retain(|b| b.name != name);
    if tree.branches.len() == before {
        return Err(EngineError::BranchNotFound(name.to_string()));
    }

    save_state(engine, &state)?;
    Ok(())
}
