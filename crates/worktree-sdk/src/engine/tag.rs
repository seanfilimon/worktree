use chrono::Utc;
use crate::error::{SdkError, Result};
use super::status::{TagState, load_state, save_state};

pub fn create_tag(
    engine: &super::WorktreeEngine,
    name: &str,
    message: Option<&str>,
    tree_name: Option<&str>,
) -> Result<TagState> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree_mut(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    if tree.tags.iter().any(|t| t.name == name) {
        return Err(SdkError::TagExists(name.to_string()));
    }

    let tip = tree.current_branch()
        .and_then(|b| b.tip.clone())
        .ok_or(SdkError::SnapshotNotFound("no snapshots to tag".into()))?;

    let tagger = std::env::var("WT_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .ok();

    let tag = TagState {
        name: name.to_string(),
        target_snapshot: tip,
        message: message.map(|m| m.to_string()),
        tagger,
        created_at: Utc::now().to_rfc3339(),
    };

    tree.tags.push(tag.clone());
    save_state(engine, &state)?;
    Ok(tag)
}

pub fn list_tags(
    engine: &super::WorktreeEngine,
    tree_name: Option<&str>,
) -> Result<Vec<TagState>> {
    let state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    Ok(tree.tags.clone())
}

pub fn delete_tag(
    engine: &super::WorktreeEngine,
    name: &str,
    tree_name: Option<&str>,
) -> Result<()> {
    let mut state = load_state(engine)?;
    let tree_name = tree_name
        .map(|s| s.to_string())
        .or_else(|| state.current_tree.clone())
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state.find_tree_mut(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    let before = tree.tags.len();
    tree.tags.retain(|t| t.name != name);
    if tree.tags.len() == before {
        return Err(SdkError::TagNotFound(name.to_string()));
    }

    save_state(engine, &state)?;
    Ok(())
}
