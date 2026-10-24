//! `.gitignore` -> `.wt/ignore` conversion.
//!
//! W0rkTree ignore files use gitignore syntax, so conversion is a matter of
//! extracting the pattern lines: blank lines and comments are dropped and
//! unescaped trailing whitespace is removed (as Git does).

/// Extract the patterns of a `.gitignore` file, in order.
///
/// ```
/// use worktree_git::config::gitignore::gitignore_patterns;
///
/// let patterns = gitignore_patterns("# build\ntarget/\n\n*.log  \n!keep.log\n");
/// assert_eq!(patterns, vec!["target/", "*.log", "!keep.log"]);
/// ```
pub fn gitignore_patterns(content: &str) -> Vec<String> {
    content
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(trim_trailing_whitespace)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Remove trailing spaces/tabs unless the last one is escaped with `\`
/// (`"foo\ "` keeps its escaped space). Also drops a trailing `\r`.
fn trim_trailing_whitespace(line: &str) -> &str {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let trimmed = line.trim_end_matches([' ', '\t']);
    if trimmed.len() < line.len() && trimmed.ends_with('\\') && !trimmed.ends_with("\\\\") {
        // Keep the escaped whitespace character.
        &line[..trimmed.len() + 1]
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_comment_only() {
        assert!(gitignore_patterns("").is_empty());
        assert!(gitignore_patterns("# a\n\n   \n#b\n").is_empty());
    }

    #[test]
    fn keeps_order_and_negations() {
        assert_eq!(
            gitignore_patterns("target/\n*.log\n!important.log\n/build\n"),
            vec!["target/", "*.log", "!important.log", "/build"]
        );
    }

    #[test]
    fn handles_crlf_and_trailing_whitespace() {
        assert_eq!(
            gitignore_patterns("dist/\r\n*.tmp \t\r\n"),
            vec!["dist/", "*.tmp"]
        );
    }

    #[test]
    fn escaped_trailing_space_and_hash() {
        assert_eq!(
            gitignore_patterns("foo\\   \n\\#notcomment\n"),
            vec!["foo\\ ", "\\#notcomment"]
        );
    }

    #[test]
    fn leading_whitespace_is_significant() {
        // Git treats leading spaces as part of the pattern.
        assert_eq!(gitignore_patterns("  spaced\n"), vec!["  spaced"]);
    }
}
