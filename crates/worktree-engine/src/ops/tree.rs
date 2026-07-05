use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{load_state, save_state, BranchState, TreeState};
use chrono::Utc;
use std::fs;

pub fn add_tree(engine: &WorktreeEngine, path: &str) -> Result<TreeState> {
    // Validate path doesn't traverse upward
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(EngineError::InvalidConfig(
            "tree path must not contain '..' or start with '/' or '\\\\'".into(),
        ));
    }

    let mut state = load_state(engine)?;
    let name = path
        .replace('\\', "/")
        .split('/')
        .next_back()
        .unwrap_or(path)
        .to_string();

    if state.find_tree(&name).is_some() {
        return Err(EngineError::InvalidConfig(format!(
            "tree '{}' already exists",
            name
        )));
    }

    // Create the tree directory and .wt-tree
    let tree_dir = engine.root().join(path);
    fs::create_dir_all(&tree_dir)?;

    let wt_tree_dir = tree_dir.join(".wt-tree");
    fs::create_dir_all(&wt_tree_dir)?;

    // Write tree config
    let config = format!(
        r#"[tree]
name = "{}"
branch_strategy = "feature-branch"
"#,
        name
    );
    fs::write(wt_tree_dir.join("config.toml"), config)?;

    let now = Utc::now().to_rfc3339();
    let tree = TreeState {
        name: name.to_string(),
        path: path.to_string(),
        branches: vec![BranchState {
            name: "main".to_string(),
            tip: None,
            created_at: now,
        }],
        current_branch: "main".to_string(),
        snapshots: Vec::new(),
        tags: Vec::new(),
    };

    state.trees.push(tree.clone());
    save_state(engine, &state)?;
    Ok(tree)
}

pub fn list_trees(engine: &WorktreeEngine) -> Result<Vec<TreeState>> {
    let state = load_state(engine)?;
    Ok(state.trees.clone())
}

pub fn remove_tree(engine: &WorktreeEngine, name: &str) -> Result<()> {
    let mut state = load_state(engine)?;

    if name == "root" {
        return Err(EngineError::InvalidConfig(
            "cannot remove the root tree".into(),
        ));
    }

    let tree = state
        .find_tree(name)
        .ok_or(EngineError::TreeNotFound(name.to_string()))?;

    // Remove .wt-tree directory
    let wt_tree_dir = engine.root().join(&tree.path).join(".wt-tree");
    if wt_tree_dir.exists() {
        fs::remove_dir_all(&wt_tree_dir)?;
    }

    if let Some(current) = &state.current_tree {
        if current == name {
            state.current_tree = Some("root".to_string());
        }
    }

    state.trees.retain(|t| t.name != name);
    save_state(engine, &state)?;
    Ok(())
}
