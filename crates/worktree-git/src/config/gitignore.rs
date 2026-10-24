/// Convert `.gitignore` content to `.worktreeignore` format.
///
/// Currently the two formats are identical, so this is a pass-through.
/// Future versions may need to handle divergences in pattern syntax.
pub fn gitignore_to_worktreeignore(content: &str) -> String {
    content.to_string()
}

/// Convert `.worktreeignore` content to `.gitignore` format.
///
/// Currently the two formats are identical, so this is a pass-through.
/// Future versions may need to handle divergences in pattern syntax.
pub fn worktreeignore_to_gitignore(content: &str) -> String {
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_gitignore() {
        let content = "target/\n*.log\n.env\n";
        let converted = gitignore_to_worktreeignore(content);
        let back = worktreeignore_to_gitignore(&converted);
        assert_eq!(content, back);
    }

    #[test]
    fn empty_content() {
        assert_eq!(gitignore_to_worktreeignore(""), "");
        assert_eq!(worktreeignore_to_gitignore(""), "");
    }
}
