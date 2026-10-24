use worktree_protocol::object::tree::Tree;

/// Generates `.gitmodules` file content from a slice of Worktree trees.
///
/// Each child tree that has a parent is treated as a submodule entry.
/// The generated format follows the standard `.gitmodules` INI-like syntax:
///
/// ```text
/// [submodule "name"]
///     path = <root_path>
///     url = <placeholder>
/// ```
pub fn generate_gitmodules(trees: &[Tree]) -> String {
    todo!(
        "Generate .gitmodules content from {} tree(s): \
         iterate child trees, emit [submodule] sections with path and url",
        trees.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "not yet implemented")]
    fn generate_gitmodules_is_stub() {
        let trees: Vec<Tree> = Vec::new();
        let _ = generate_gitmodules(&trees);
    }
}
