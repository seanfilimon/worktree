use serde::{Deserialize, Serialize};

/// A single rule from a `.gitattributes` file, pairing a path pattern
/// with one or more attribute declarations.
///
/// For example, the line `*.rs text diff=rust` would be represented as:
///
/// ```ignore
/// GitAttribute {
///     pattern: "*.rs".into(),
///     attributes: vec!["text".into(), "diff=rust".into()],
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitAttribute {
    /// The glob pattern that selects which paths this rule applies to.
    pub pattern: String,
    /// The attribute declarations attached to matching paths.
    /// Each entry is a single token such as `"text"`, `"-text"`, `"diff=rust"`, etc.
    pub attributes: Vec<String>,
}

/// Parse the contents of a `.gitattributes` file into a list of [`GitAttribute`] rules.
///
/// Blank lines and comment lines (starting with `#`) are skipped.  Each
/// remaining line is split on whitespace: the first token is the pattern and
/// all subsequent tokens are attribute declarations.
///
/// # Examples
///
/// ```
/// use worktree_git::config::gitattributes::parse_gitattributes;
///
/// let content = "*.rs text diff=rust\n# comment\n*.md text\n";
/// let attrs = parse_gitattributes(content);
/// assert_eq!(attrs.len(), 2);
/// assert_eq!(attrs[0].pattern, "*.rs");
/// assert_eq!(attrs[0].attributes, vec!["text", "diff=rust"]);
/// assert_eq!(attrs[1].pattern, "*.md");
/// assert_eq!(attrs[1].attributes, vec!["text"]);
/// ```
pub fn parse_gitattributes(content: &str) -> Vec<GitAttribute> {
    let mut result = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut tokens = trimmed.split_whitespace();

        // The first token is always the path pattern.
        let pattern = match tokens.next() {
            Some(p) => p.to_owned(),
            None => continue,
        };

        let attributes: Vec<String> = tokens.map(|t| t.to_owned()).collect();

        // A line with only a pattern and no attributes is technically valid
        // (it resets attributes to unspecified), so we include it.
        result.push(GitAttribute {
            pattern,
            attributes,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_content() {
        let attrs = parse_gitattributes("");
        assert!(attrs.is_empty());
    }

    #[test]
    fn parse_comments_and_blanks() {
        let content = "# this is a comment\n\n   # another comment\n   \n";
        let attrs = parse_gitattributes(content);
        assert!(attrs.is_empty());
    }

    #[test]
    fn parse_single_rule() {
        let content = "*.rs text diff=rust\n";
        let attrs = parse_gitattributes(content);
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].pattern, "*.rs");
        assert_eq!(attrs[0].attributes, vec!["text", "diff=rust"]);
    }

    #[test]
    fn parse_multiple_rules() {
        let content = "\
*.rs    text diff=rust
*.jpg   binary
*.md    text
# trailing comment
";
        let attrs = parse_gitattributes(content);
        assert_eq!(attrs.len(), 3);

        assert_eq!(attrs[0].pattern, "*.rs");
        assert_eq!(attrs[0].attributes, vec!["text", "diff=rust"]);

        assert_eq!(attrs[1].pattern, "*.jpg");
        assert_eq!(attrs[1].attributes, vec!["binary"]);

        assert_eq!(attrs[2].pattern, "*.md");
        assert_eq!(attrs[2].attributes, vec!["text"]);
    }

    #[test]
    fn parse_pattern_only_line() {
        let content = "*.lock\n";
        let attrs = parse_gitattributes(content);
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].pattern, "*.lock");
        assert!(attrs[0].attributes.is_empty());
    }

    #[test]
    fn parse_negated_and_set_attributes() {
        let content = "*.bin -text -diff\n*.txt text eol=lf\n";
        let attrs = parse_gitattributes(content);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0].attributes, vec!["-text", "-diff"]);
        assert_eq!(attrs[1].attributes, vec!["text", "eol=lf"]);
    }

    #[test]
    fn serde_roundtrip() {
        let attr = GitAttribute {
            pattern: "*.rs".to_string(),
            attributes: vec!["text".to_string(), "diff=rust".to_string()],
        };
        let json = serde_json::to_string(&attr).unwrap();
        let deserialized: GitAttribute = serde_json::from_str(&json).unwrap();
        assert_eq!(attr, deserialized);
    }
}
