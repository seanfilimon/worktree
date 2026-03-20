use crate::error::Result;

pub fn list_dependencies(engine: &super::WorktreeEngine) -> Result<Vec<String>> {
    let state = super::status::load_state(engine)?;
    let mut deps = Vec::new();
    for tree in &state.trees {
        if tree.name != "root" {
            deps.push(format!("tree: {}", tree.name));
        }
    }
    Ok(deps)
}
