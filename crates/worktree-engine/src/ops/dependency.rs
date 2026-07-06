use crate::engine::WorktreeEngine;
use crate::error::Result;
use crate::persist::load_state;

pub fn list_dependencies(engine: &WorktreeEngine) -> Result<Vec<String>> {
    let state = load_state(engine)?;
    let mut deps = Vec::new();
    for tree in &state.trees {
        if tree.name != "root" {
            deps.push(format!("tree: {}", tree.name));
        }
    }
    Ok(deps)
}
