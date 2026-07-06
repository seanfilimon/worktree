use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{self, load_state, TagState};

pub fn create_tag(
    engine: &WorktreeEngine,
    name: &str,
    message: Option<&str>,
    tree_name: Option<&str>,
) -> Result<TagState> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.tags.iter().any(|t| t.name == name) {
        return Err(EngineError::TagExists(name.to_string()));
    }

    let tip = tree
        .current_branch()
        .and_then(|b| b.tip.clone())
        .ok_or(EngineError::SnapshotNotFound("no snapshots to tag".into()))?;

    persist::create_tag(engine, &tree_name, name, &tip, message)
}

pub fn list_tags(engine: &WorktreeEngine, tree_name: Option<&str>) -> Result<Vec<TagState>> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    Ok(tree.tags.clone())
}

pub fn delete_tag(engine: &WorktreeEngine, name: &str, tree_name: Option<&str>) -> Result<()> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    // Existence check for a friendly error before hitting the ref store.
    let tree = state
        .find_tree(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;
    if !tree.tags.iter().any(|t| t.name == name) {
        return Err(EngineError::TagNotFound(name.to_string()));
    }

    persist::delete_tag(engine, &tree_name, name)
}
