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

/// Patterns whose paths are stored with Git LFS (`filter=lfs`). See
/// [`LfsRules`] for matching paths against them.
///
/// ```
/// use worktree_git::config::gitattributes::{lfs_patterns, parse_gitattributes};
///
/// let attrs = parse_gitattributes("*.psd filter=lfs diff=lfs merge=lfs -text\n*.rs text\n");
/// assert_eq!(lfs_patterns(&attrs), vec!["*.psd"]);
/// ```
pub fn lfs_patterns(attrs: &[GitAttribute]) -> Vec<&str> {
    attrs
        .iter()
        .filter(|a| a.attributes.iter().any(|t| t == "filter=lfs"))
        .map(|a| a.pattern.as_str())
        .collect()
}

/// The `filter=lfs` decisions of one `.gitattributes` file, compiled for
/// matching (`wt git import` uses them to decide which pointer-shaped files
/// are Git LFS files).
///
/// Matching follows gitattributes(5) where it matters for LFS: a pattern
/// without a `/` matches the file name at any depth below the file's
/// directory, a pattern with a `/` is matched against the path relative to
/// that directory (`**` crosses directories, `*` does not), the last
/// matching line wins, and patterns ending in `/` (directories) or starting
/// with `!` (forbidden in gitattributes) never match files. Macro
/// definitions (`[attr]...`) and quoted patterns are ignored.
///
/// ```
/// use worktree_git::config::gitattributes::LfsRules;
///
/// let rules = LfsRules::parse("*.psd filter=lfs diff=lfs merge=lfs -text\nkeep.psd -filter\n");
/// assert_eq!(rules.decision("art/cover.psd"), Some(true));
/// assert_eq!(rules.decision("keep.psd"), Some(false));
/// assert_eq!(rules.decision("README.md"), None);
/// ```
#[derive(Debug, Clone, Default)]
pub struct LfsRules {
    rules: Vec<LfsRule>,
}

#[derive(Debug, Clone)]
struct LfsRule {
    matcher: globset::GlobMatcher,
    /// Match against the file name only (pattern without `/`).
    basename: bool,
    /// `filter=lfs` (true) or any other filter state (false).
    lfs: bool,
}

impl LfsRules {
    /// Compile the lines of `content` that set, unset or change `filter`.
    /// Lines whose pattern cannot be compiled are skipped.
    pub fn parse(content: &str) -> Self {
        let mut rules = Vec::new();
        for attr in parse_gitattributes(content) {
            let Some(lfs) = filter_is_lfs(&attr.attributes) else {
                continue;
            };
            let pattern = attr.pattern.as_str();
            if pattern.starts_with("[attr]")
                || pattern.starts_with('!')
                || pattern.starts_with('"')
                || pattern.ends_with('/')
            {
                continue;
            }
            let basename = !pattern.contains('/');
            let pattern = pattern.strip_prefix('/').unwrap_or(pattern);
            let Ok(glob) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
            else {
                continue;
            };
            rules.push(LfsRule {
                matcher: glob.compile_matcher(),
                basename,
                lfs,
            });
        }
        Self { rules }
    }

    /// Whether no line of the file touches `filter`.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// `Some(true)` if the last line matching `rel_path` (relative to the
    /// directory of the `.gitattributes` file, `/`-separated) sets
    /// `filter=lfs`, `Some(false)` if it sets another filter or unsets it,
    /// `None` if no line matches.
    pub fn decision(&self, rel_path: &str) -> Option<bool> {
        let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
        self.rules
            .iter()
            .rev()
            .find(|rule| {
                rule.matcher
                    .is_match(if rule.basename { name } else { rel_path })
            })
            .map(|rule| rule.lfs)
    }
}

/// The `filter` state set by one line's attributes (the last mention
/// wins): `Some(true)` for `filter=lfs`, `Some(false)` for another value,
/// `-filter`, `!filter` or a bare `filter`, `None` if `filter` is absent.
fn filter_is_lfs(attributes: &[String]) -> Option<bool> {
    attributes.iter().rev().find_map(|token| {
        let token = token.as_str();
        if let Some(value) = token.strip_prefix("filter=") {
            Some(value == "lfs")
        } else if matches!(token, "filter" | "-filter" | "!filter") {
            Some(false)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfs_rules_follow_gitattributes_matching() {
        let rules = LfsRules::parse(
            "\
# comment
*.bin filter=lfs -text
docs/*.pdf filter=lfs
/top.dat filter=lfs
assets/** filter=lfs
assets/keep.txt -filter
dir/ filter=lfs
!neg filter=lfs
[attr]big filter=lfs
*.txt text
*.iso filter=lfs filter=other
",
        );
        assert!(!rules.is_empty());
        // Basename patterns match at any depth.
        assert_eq!(rules.decision("a.bin"), Some(true));
        assert_eq!(rules.decision("deep/er/a.bin"), Some(true));
        // Patterns with a slash are anchored; `*` does not cross '/'.
        assert_eq!(rules.decision("docs/a.pdf"), Some(true));
        assert_eq!(rules.decision("docs/sub/a.pdf"), None);
        assert_eq!(rules.decision("x/docs/a.pdf"), None);
        assert_eq!(rules.decision("top.dat"), Some(true));
        assert_eq!(rules.decision("sub/top.dat"), None);
        // `**` crosses directories; a later line wins.
        assert_eq!(rules.decision("assets/img/a.png"), Some(true));
        assert_eq!(rules.decision("assets/keep.txt"), Some(false));
        // Directory, negated and macro lines never match files.
        assert_eq!(rules.decision("dir"), None);
        assert_eq!(rules.decision("neg"), None);
        assert_eq!(rules.decision("big"), None);
        // Lines that do not mention filter are irrelevant.
        assert_eq!(rules.decision("notes.txt"), None);
        // Within a line the last filter token wins.
        assert_eq!(rules.decision("disk.iso"), Some(false));
        assert!(LfsRules::parse("*.rs text\n").is_empty());
    }

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
    fn lfs_detection() {
        let attrs = parse_gitattributes(
            "*.bin filter=lfs -text\n*.txt text\n*.zip -filter=lfs\nassets/** filter=lfs\n",
        );
        assert_eq!(lfs_patterns(&attrs), vec!["*.bin", "assets/**"]);
        assert!(lfs_patterns(&[]).is_empty());
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
