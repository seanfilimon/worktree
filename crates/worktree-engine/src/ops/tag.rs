use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::identity;
use crate::persist::{load_state, save_state, TagState};
use chrono::Utc;

pub fn create_tag(
    engine: &WorktreeEngine,
    name: &str,
    message: Option<&str>,
    tree_name: Option<&str>,
) -> Result<TagState> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    if tree.tags.iter().any(|t| t.name == name) {
        return Err(EngineError::TagExists(name.to_string()));
    }

    let tip = tree
        .current_branch()
        .and_then(|b| b.tip.clone())
        .ok_or(EngineError::SnapshotNotFound("no snapshots to tag".into()))?;

    let tag = TagState {
        name: name.to_string(),
        target_snapshot: tip,
        message: message.map(|m| m.to_string()),
        tagger: identity::author_opt(),
        created_at: Utc::now().to_rfc3339(),
    };

    tree.tags.push(tag.clone());
    save_state(engine, &state)?;
    Ok(tag)
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
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(EngineError::TreeNotFound(tree_name.clone()))?;

    let before = tree.tags.len();
    tree.tags.retain(|t| t.name != name);
    if tree.tags.len() == before {
        return Err(EngineError::TagNotFound(name.to_string()));
    }

    save_state(engine, &state)?;
    Ok(())
}
