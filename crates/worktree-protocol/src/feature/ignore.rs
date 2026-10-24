use serde::{Deserialize, Serialize};

/// Compiled ignore pattern
#[derive(Debug, Clone)]
pub struct IgnorePattern {
    pub pattern: String,
    pub negated: bool,
    pub directory_only: bool,
    pub anchored: bool,
    pub source: IgnoreSource,
}

/// Where an ignore pattern came from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreSource {
    BuiltIn,
    RootIgnore,
    TreeIgnore(String),
}

impl IgnorePattern {
    pub fn parse(line: &str, source: IgnoreSource) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        let (negated, pattern) = if let Some(rest) = trimmed.strip_prefix('!') {
            (true, rest.to_string())
        } else {
            (false, trimmed.to_string())
        };
        let directory_only = pattern.ends_with('/');
        let pattern = if directory_only {
            pattern.trim_end_matches('/').to_string()
        } else {
            pattern
        };
        let anchored = pattern.contains('/');
        Some(Self { pattern, negated, directory_only, anchored, source })
    }

    /// Simple glob matching (supports * and **)
    pub fn matches(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        if self.anchored {
            glob_match(&self.pattern, &path)
        } else {
            // Match against any path component
            let filename = path.rsplit('/').next().unwrap_or(&path);
            glob_match(&self.pattern, filename) || glob_match(&self.pattern, &path)
        }
    }
}

/// Simple glob matching supporting * and **
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern_chars, &text_chars)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    if pattern.len() >= 2 && pattern[0] == '*' && pattern[1] == '*' {
        let rest = if pattern.len() > 2 && pattern[2] == '/' {
            &pattern[3..]
        } else {
            &pattern[2..]
        };
        // ** matches zero or more path segments
        for i in 0..=text.len() {
            if glob_match_inner(rest, &text[i..]) {
                return true;
            }
        }
        return false;
    }
    if pattern[0] == '*' {
        // * matches anything except /
        for i in 0..=text.len() {
            if i > 0 && text[i - 1] == '/' {
                break;
            }
            if glob_match_inner(&pattern[1..], &text[i..]) {
                return true;
            }
        }
        return false;
    }
    if text.is_empty() {
        return false;
    }
    if pattern[0] == '?' && text[0] != '/' {
        return glob_match_inner(&pattern[1..], &text[1..]);
    }
    if pattern[0] == text[0] {
        return glob_match_inner(&pattern[1..], &text[1..]);
    }
    false
}

/// Hard-coded built-in ignore patterns
pub fn builtin_ignores() -> Vec<IgnorePattern> {
    let patterns = vec![
        ".wt/", ".git/",
    ];
    patterns.into_iter()
        .filter_map(|p| IgnorePattern::parse(p, IgnoreSource::BuiltIn))
        .collect()
}

/// Default soft ignore patterns
pub fn default_ignores() -> Vec<IgnorePattern> {
    let patterns = vec![
        "node_modules/", "target/", "__pycache__/", ".DS_Store",
        "*.pyc", "*.pyo", ".env", ".venv/", "dist/", "build/",
        "*.o", "*.so", "*.dylib", "*.dll", "*.exe",
    ];
    patterns.into_iter()
        .filter_map(|p| IgnorePattern::parse(p, IgnoreSource::BuiltIn))
        .collect()
}

/// Hierarchical ignore rule engine
#[derive(Debug, Clone)]
pub struct IgnoreEngine {
    builtin: Vec<IgnorePattern>,
    root_patterns: Vec<IgnorePattern>,
    tree_patterns: Vec<IgnorePattern>,
}

impl IgnoreEngine {
    pub fn new() -> Self {
        Self {
            builtin: builtin_ignores(),
            root_patterns: Vec::new(),
            tree_patterns: Vec::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut engine = Self::new();
        engine.root_patterns = default_ignores();
        engine
    }

    pub fn add_root_pattern(&mut self, line: &str) {
        if let Some(pat) = IgnorePattern::parse(line, IgnoreSource::RootIgnore) {
            self.root_patterns.push(pat);
        }
    }

    pub fn add_root_patterns(&mut self, content: &str) {
        for line in content.lines() {
            self.add_root_pattern(line);
        }
    }

    pub fn add_tree_pattern(&mut self, line: &str, tree_name: &str) {
        if let Some(pat) = IgnorePattern::parse(line, IgnoreSource::TreeIgnore(tree_name.to_string())) {
            // Tree-level patterns cannot negate root patterns
            if pat.negated {
                // Check if this negates a root pattern — if so, skip it
                let base = pat.pattern.clone();
                let negates_root = self.root_patterns.iter().any(|rp| !rp.negated && rp.pattern == base);
                if negates_root {
                    return; // Cannot negate root patterns
                }
            }
            self.tree_patterns.push(pat);
        }
    }

    pub fn add_tree_patterns(&mut self, content: &str, tree_name: &str) {
        for line in content.lines() {
            self.add_tree_pattern(line, tree_name);
        }
    }

    /// Check if a path should be ignored
    pub fn is_ignored(&self, path: &str) -> bool {
        // Built-in patterns always match (hard ignores)
        for pat in &self.builtin {
            if !pat.negated && pat.matches(path) {
                return true;
            }
        }

        // Evaluate root patterns, then tree patterns
        // Last matching pattern wins
        let mut ignored = false;
        for pat in &self.root_patterns {
            if pat.matches(path) {
                ignored = !pat.negated;
            }
        }
        // Tree patterns can only ADD ignores, not remove root ignores
        for pat in &self.tree_patterns {
            if pat.matches(path) {
                if pat.negated {
                    // Tree negation only overrides tree-level ignores
                    if self.tree_patterns.iter().any(|tp| !tp.negated && tp.matches(path)) {
                        ignored = false;
                    }
                } else {
                    ignored = true;
                }
            }
        }
        ignored
    }
}

impl Default for IgnoreEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_ignores() {
        let engine = IgnoreEngine::new();
        assert!(engine.is_ignored(".wt/config.toml"));
        assert!(engine.is_ignored(".git/objects"));
    }

    #[test]
    fn test_pattern_matching() {
        let pat = IgnorePattern::parse("*.log", IgnoreSource::RootIgnore).unwrap();
        assert!(pat.matches("debug.log"));
        assert!(pat.matches("src/debug.log"));
        assert!(!pat.matches("log.txt"));
    }

    #[test]
    fn test_negation() {
        let mut engine = IgnoreEngine::new();
        engine.add_root_pattern("*.log");
        engine.add_root_pattern("!important.log");
        assert!(engine.is_ignored("debug.log"));
        assert!(!engine.is_ignored("important.log"));
    }

    #[test]
    fn test_doublestar() {
        let pat = IgnorePattern::parse("src/**/*.rs", IgnoreSource::RootIgnore).unwrap();
        assert!(pat.matches("src/main.rs"));
        assert!(pat.matches("src/deep/nested/file.rs"));
        assert!(!pat.matches("other/file.rs"));
    }

    #[test]
    fn test_default_ignores() {
        let engine = IgnoreEngine::with_defaults();
        assert!(engine.is_ignored("node_modules/package.json"));
        assert!(engine.is_ignored("target/debug/binary"));
        assert!(!engine.is_ignored("src/main.rs"));
    }

    #[test]
    fn test_tree_cannot_negate_root() {
        let mut engine = IgnoreEngine::new();
        engine.add_root_pattern("*.log");
        engine.add_tree_pattern("!*.log", "backend"); // Should be blocked
        assert!(engine.is_ignored("debug.log")); // Still ignored
    }
}
