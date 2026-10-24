//! Path ignore rules with `.gitignore` semantics.
//!
//! Patterns follow the gitignore format:
//!
//! * Blank lines and lines starting with `#` are skipped. Use `\#` for a
//!   literal leading `#`.
//! * Trailing spaces are removed unless escaped with a backslash.
//! * A leading `!` negates the pattern (re-includes a previously excluded
//!   path). Use `\!` for a literal leading `!`.
//! * A trailing `/` makes the pattern match directories only.
//! * A pattern containing a `/` at the start or in the middle is *anchored*:
//!   it is matched against the full path relative to the ignore root.
//!   Otherwise it matches the basename at any depth.
//! * `*` matches anything except `/`, `?` matches one character except `/`,
//!   `[...]` is a character class (`[!...]`/`[^...]` negate, ranges and
//!   POSIX classes such as `[[:digit:]]` are supported), `\x` escapes `x`.
//! * `**` as a whole path segment matches zero or more directories:
//!   `**/foo` matches `foo` anywhere, `a/**/b` matches `a/b`, `a/x/b`, ...,
//!   and `a/**` matches everything inside `a`. Elsewhere `**` acts like `*`.
//! * A path is excluded if it, or any of its parent directories, is
//!   excluded. As in Git, a file cannot be re-included if one of its parent
//!   directories is excluded.
//!
//! [`IgnoreEngine`] layers three pattern sets: hard built-ins (VCS metadata
//! directories, which can never be re-included), root patterns (e.g.
//! `.wt/ignore`, `.wtignore`; last match wins) and tree patterns, which may
//! only *add* exclusions — a tree-level `!pattern` can re-include a path
//! excluded by another tree pattern but never one excluded by a root pattern.
//!
//! Paths passed to the matching functions are relative to the ignore root,
//! `/`-separated (`\` is accepted and normalised for Windows callers). A
//! trailing `/` marks the path as a directory.

/// A single parsed ignore pattern.
///
/// `pattern` holds the glob with the `!` prefix, the leading `/` and any
/// trailing `/` removed; those are captured by `negated`, `anchored` and
/// `directory_only` respectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnorePattern {
    pub pattern: String,
    pub negated: bool,
    pub directory_only: bool,
    pub anchored: bool,
    pub source: IgnoreSource,
}

/// Where an ignore pattern came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreSource {
    BuiltIn,
    RootIgnore,
    TreeIgnore(String),
}

impl IgnorePattern {
    /// Parse one line of an ignore file. Returns `None` for blank lines,
    /// comments and lines that cannot match anything (e.g. a lone `!` or `/`).
    pub fn parse(line: &str, source: IgnoreSource) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let line = trim_unescaped_trailing_whitespace(line);
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let (negated, body) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };

        let mut directory_only = false;
        let mut body = body;
        while let Some(rest) = body.strip_suffix('/') {
            if rest.ends_with('\\') && !rest.ends_with("\\\\") {
                // `\/` — an escaped slash, not a directory marker.
                break;
            }
            directory_only = true;
            body = rest;
        }

        let (leading_slash, body) = match body.strip_prefix('/') {
            Some(rest) => (true, rest),
            None => (false, body),
        };
        if body.is_empty() {
            return None;
        }

        let anchored = leading_slash || body.contains('/');
        Some(Self {
            pattern: body.to_string(),
            negated,
            directory_only,
            anchored,
            source,
        })
    }

    /// Whether this pattern applies to `path` or to one of its parent
    /// directories (negation is not taken into account). A trailing `/`
    /// marks `path` as a directory; otherwise it is treated as a file.
    pub fn matches(&self, path: &str) -> bool {
        let (segments, is_dir) = normalize_path(path);
        let compiled = CompiledPattern::new(self.clone());
        (1..segments.len()).any(|k| compiled.matches_segments(&segments[..k], true))
            || compiled.matches_segments(&segments, is_dir)
    }

    /// Whether this pattern matches exactly `path` (not its parents).
    /// `is_dir` tells whether `path` is a directory, which matters for
    /// directory-only patterns (`build/`).
    pub fn matches_path(&self, path: &str, is_dir: bool) -> bool {
        let (segments, trailing_slash) = normalize_path(path);
        CompiledPattern::new(self.clone()).matches_segments(&segments, is_dir || trailing_slash)
    }

    /// Render the pattern back to its ignore-file line form.
    pub fn to_line(&self) -> String {
        let mut line = String::new();
        if self.negated {
            line.push('!');
        }
        // A pattern anchored only by a leading slash needs it back.
        if self.anchored && !self.pattern.contains('/') {
            line.push('/');
        }
        line.push_str(&self.pattern);
        if self.directory_only {
            line.push('/');
        }
        line
    }
}

impl std::fmt::Display for IgnorePattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_line())
    }
}

/// Remove trailing spaces/tabs that are not escaped with a backslash.
fn trim_unescaped_trailing_whitespace(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
        // Count the backslashes directly preceding this whitespace char.
        let backslashes = bytes[..end - 1]
            .iter()
            .rev()
            .take_while(|&&b| b == b'\\')
            .count();
        if backslashes % 2 == 1 {
            break;
        }
        end -= 1;
    }
    &line[..end]
}

/// Split a path into its segments, returning whether it was marked as a
/// directory with a trailing slash. Empty and `.` segments are dropped.
fn normalize_path(path: &str) -> (Vec<&str>, bool) {
    let is_dir = path.ends_with('/') || path.ends_with('\\');
    let segments = path
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    (segments, is_dir)
}

// ---------------------------------------------------------------------------
// Glob compilation and matching
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClassItem {
    Range(char, char),
    Named(NamedClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedClass {
    Alnum,
    Alpha,
    Blank,
    Digit,
    Lower,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl NamedClass {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "alnum" => Self::Alnum,
            "alpha" => Self::Alpha,
            "blank" => Self::Blank,
            "digit" => Self::Digit,
            "lower" => Self::Lower,
            "punct" => Self::Punct,
            "space" => Self::Space,
            "upper" => Self::Upper,
            "xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    fn contains(self, c: char) -> bool {
        match self {
            Self::Alnum => c.is_alphanumeric(),
            Self::Alpha => c.is_alphabetic(),
            Self::Blank => c == ' ' || c == '\t',
            Self::Digit => c.is_ascii_digit(),
            Self::Lower => c.is_lowercase(),
            Self::Punct => c.is_ascii_punctuation(),
            Self::Space => c.is_whitespace(),
            Self::Upper => c.is_uppercase(),
            Self::Xdigit => c.is_ascii_hexdigit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(char),
    AnyChar,
    Star,
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
}

impl Token {
    fn matches(&self, c: char) -> bool {
        match self {
            Token::Literal(l) => *l == c,
            Token::AnyChar => true,
            Token::Star => true,
            Token::Class { negated, items } => {
                let hit = items.iter().any(|item| match item {
                    ClassItem::Range(lo, hi) => (*lo..=*hi).contains(&c),
                    ClassItem::Named(n) => n.contains(c),
                });
                hit != *negated
            }
        }
    }
}

/// One `/`-separated segment of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    /// `**` — zero or more whole path segments.
    DoubleStar,
    Glob(Vec<Token>),
}

/// Tokenize a single path segment glob.
fn tokenize_segment(seg: &str) -> Vec<Token> {
    let chars: Vec<char> = seg.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                // Escaped character; a trailing lone backslash is literal.
                let c = chars.get(i + 1).copied().unwrap_or('\\');
                tokens.push(Token::Literal(c));
                i += 2;
            }
            '*' => {
                if tokens.last() != Some(&Token::Star) {
                    tokens.push(Token::Star);
                }
                i += 1;
            }
            '?' => {
                tokens.push(Token::AnyChar);
                i += 1;
            }
            '[' => match parse_class(&chars, i) {
                Some((token, next)) => {
                    tokens.push(token);
                    i = next;
                }
                None => {
                    tokens.push(Token::Literal('['));
                    i += 1;
                }
            },
            c => {
                tokens.push(Token::Literal(c));
                i += 1;
            }
        }
    }
    tokens
}

/// Parse a bracket expression starting at `chars[start] == '['`.
/// Returns the token and the index just past the closing `]`, or `None` if
/// the expression is unterminated (then `[` is treated literally).
fn parse_class(chars: &[char], start: usize) -> Option<(Token, usize)> {
    let mut i = start + 1;
    let negated = matches!(chars.get(i), Some('!') | Some('^'));
    if negated {
        i += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    loop {
        let c = *chars.get(i)?;
        if c == ']' && !first {
            return Some((Token::Class { negated, items }, i + 1));
        }
        first = false;

        // POSIX named class: [:name:]
        if c == '[' && chars.get(i + 1) == Some(&':') {
            if let Some(len) = chars[i + 2..]
                .windows(2)
                .position(|w| w[0] == ':' && w[1] == ']')
            {
                let name: String = chars[i + 2..i + 2 + len].iter().collect();
                if let Some(named) = NamedClass::from_name(&name) {
                    items.push(ClassItem::Named(named));
                    i += len + 4;
                    continue;
                }
            }
        }

        let (lo, after_lo) = if c == '\\' {
            (*chars.get(i + 1)?, i + 2)
        } else {
            (c, i + 1)
        };
        // Range `lo-hi` (a `-` right before `]` is literal).
        if chars.get(after_lo) == Some(&'-') && chars.get(after_lo + 1).is_some_and(|&n| n != ']') {
            let hi_idx = after_lo + 1;
            let (hi, after_hi) = if chars[hi_idx] == '\\' {
                (*chars.get(hi_idx + 1)?, hi_idx + 2)
            } else {
                (chars[hi_idx], hi_idx + 1)
            };
            items.push(ClassItem::Range(lo, hi));
            i = after_hi;
        } else {
            items.push(ClassItem::Range(lo, lo));
            i = after_lo;
        }
    }
}

/// Match a single path segment against tokens (`*` never crosses `/`
/// because segments contain no `/`). Linear-time greedy backtracking.
fn match_tokens(tokens: &[Token], text: &[char]) -> bool {
    let (mut ti, mut si) = (0usize, 0usize);
    let mut backtrack: Option<(usize, usize)> = None;
    while si < text.len() {
        if ti < tokens.len() {
            if tokens[ti] == Token::Star {
                backtrack = Some((ti, si));
                ti += 1;
                continue;
            }
            if tokens[ti].matches(text[si]) {
                ti += 1;
                si += 1;
                continue;
            }
        }
        match backtrack {
            Some((star_ti, star_si)) => {
                ti = star_ti + 1;
                si = star_si + 1;
                backtrack = Some((star_ti, star_si + 1));
            }
            None => return false,
        }
    }
    tokens[ti..].iter().all(|t| *t == Token::Star)
}

fn match_segment(tokens: &[Token], segment: &str) -> bool {
    let chars: Vec<char> = segment.chars().collect();
    match_tokens(tokens, &chars)
}

fn match_segments(pattern: &[Segment], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((Segment::DoubleStar, rest)) => {
            if rest.is_empty() {
                // Trailing `/**` matches everything *inside*, not the dir itself.
                !path.is_empty()
            } else {
                (0..=path.len()).any(|i| match_segments(rest, &path[i..]))
            }
        }
        Some((Segment::Glob(tokens), rest)) => match path.split_first() {
            Some((first, path_rest)) => {
                match_segment(tokens, first) && match_segments(rest, path_rest)
            }
            None => false,
        },
    }
}

/// An [`IgnorePattern`] with its glob pre-compiled.
#[derive(Debug, Clone)]
struct CompiledPattern {
    pattern: IgnorePattern,
    segments: Vec<Segment>,
}

impl CompiledPattern {
    fn new(pattern: IgnorePattern) -> Self {
        let segments = if pattern.anchored {
            pattern
                .pattern
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s == "**" {
                        Segment::DoubleStar
                    } else {
                        Segment::Glob(tokenize_segment(s))
                    }
                })
                .collect()
        } else {
            vec![Segment::Glob(tokenize_segment(&pattern.pattern))]
        };
        Self { pattern, segments }
    }

    /// Match the pattern against exactly this path.
    fn matches_segments(&self, path: &[&str], is_dir: bool) -> bool {
        if path.is_empty() || (self.pattern.directory_only && !is_dir) {
            return false;
        }
        if self.pattern.anchored {
            match_segments(&self.segments, path)
        } else {
            match (&self.segments[..], path.last()) {
                ([Segment::Glob(tokens)], Some(name)) => match_segment(tokens, name),
                _ => false,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Default pattern sets
// ---------------------------------------------------------------------------

/// Hard-coded built-in ignore patterns (VCS metadata; can never be
/// re-included).
pub fn builtin_ignores() -> Vec<IgnorePattern> {
    [".wt/", ".wt-tree/", ".git/"]
        .into_iter()
        .filter_map(|p| IgnorePattern::parse(p, IgnoreSource::BuiltIn))
        .collect()
}

/// Default soft ignore patterns (can be re-included with `!pattern`).
pub fn default_ignores() -> Vec<IgnorePattern> {
    [
        "node_modules/",
        "target/",
        "__pycache__/",
        ".DS_Store",
        "*.swp",
        "*.pyc",
        "*.pyo",
        ".env",
        ".venv/",
        "dist/",
        "build/",
        "*.o",
        "*.so",
        "*.dylib",
        "*.dll",
        "*.exe",
    ]
    .into_iter()
    .filter_map(|p| IgnorePattern::parse(p, IgnoreSource::BuiltIn))
    .collect()
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The outcome of evaluating one path against one layer set.
#[derive(Clone, Copy)]
struct Decision<'a> {
    ignored: bool,
    by: Option<&'a IgnorePattern>,
}

/// Hierarchical ignore rule engine.
#[derive(Debug, Clone)]
pub struct IgnoreEngine {
    builtin: Vec<CompiledPattern>,
    root_patterns: Vec<CompiledPattern>,
    tree_patterns: Vec<CompiledPattern>,
}

impl IgnoreEngine {
    /// Engine with only the hard built-in patterns.
    pub fn new() -> Self {
        Self {
            builtin: compile_all(builtin_ignores()),
            root_patterns: Vec::new(),
            tree_patterns: Vec::new(),
        }
    }

    /// Engine with built-ins plus [`default_ignores`] as root patterns.
    pub fn with_defaults() -> Self {
        let mut engine = Self::new();
        engine.root_patterns = compile_all(default_ignores());
        engine
    }

    pub fn add_root_pattern(&mut self, line: &str) {
        if let Some(pat) = IgnorePattern::parse(line, IgnoreSource::RootIgnore) {
            self.root_patterns.push(CompiledPattern::new(pat));
        }
    }

    pub fn add_root_patterns(&mut self, content: &str) {
        for line in content.lines() {
            self.add_root_pattern(line);
        }
    }

    /// Add a tree-level pattern. Tree patterns can only add exclusions; a
    /// negation that literally negates a root pattern is dropped, and any
    /// tree negation is only effective against tree-level exclusions.
    pub fn add_tree_pattern(&mut self, line: &str, tree_name: &str) {
        let Some(pat) = IgnorePattern::parse(line, IgnoreSource::TreeIgnore(tree_name.to_string()))
        else {
            return;
        };
        if pat.negated {
            let negates_root = self.root_patterns.iter().any(|rp| {
                !rp.pattern.negated
                    && rp.pattern.pattern == pat.pattern
                    && rp.pattern.anchored == pat.anchored
            });
            if negates_root {
                return;
            }
        }
        self.tree_patterns.push(CompiledPattern::new(pat));
    }

    pub fn add_tree_patterns(&mut self, content: &str, tree_name: &str) {
        for line in content.lines() {
            self.add_tree_pattern(line, tree_name);
        }
    }

    /// All patterns in evaluation order: built-ins, root, tree.
    pub fn patterns(&self) -> impl Iterator<Item = &IgnorePattern> {
        self.builtin
            .iter()
            .chain(&self.root_patterns)
            .chain(&self.tree_patterns)
            .map(|c| &c.pattern)
    }

    /// Check if a path should be ignored. A trailing `/` marks the path as a
    /// directory; otherwise it is treated as a file (so `build/` does not
    /// match a file named `build`). Use [`Self::is_ignored_path`] when the
    /// kind is known separately.
    pub fn is_ignored(&self, path: &str) -> bool {
        let (segments, is_dir) = normalize_path(path);
        self.evaluate(&segments, is_dir).ignored
    }

    /// Check if a path of known kind should be ignored.
    pub fn is_ignored_path(&self, path: &str, is_dir: bool) -> bool {
        let (segments, trailing_slash) = normalize_path(path);
        self.evaluate(&segments, is_dir || trailing_slash).ignored
    }

    /// Check if a directory should be ignored (callers walking a tree can
    /// skip the whole directory when this returns `true`).
    pub fn is_ignored_dir(&self, path: &str) -> bool {
        self.is_ignored_path(path, true)
    }

    /// The pattern that decided the outcome for `path`, if any pattern
    /// matched. If the path is excluded because a parent directory is, the
    /// parent's excluding pattern is returned. A returned negated pattern
    /// means the path was explicitly re-included.
    pub fn matching_pattern(&self, path: &str, is_dir: bool) -> Option<&IgnorePattern> {
        let (segments, trailing_slash) = normalize_path(path);
        self.evaluate(&segments, is_dir || trailing_slash).by
    }

    fn evaluate(&self, segments: &[&str], is_dir: bool) -> Decision<'_> {
        if segments.is_empty() {
            return Decision {
                ignored: false,
                by: None,
            };
        }
        // A path inside an excluded directory is excluded, regardless of
        // later negations (Git semantics).
        for k in 1..segments.len() {
            let parent = self.decide(&segments[..k], true);
            if parent.ignored {
                return parent;
            }
        }
        self.decide(segments, is_dir)
    }

    fn decide(&self, segments: &[&str], is_dir: bool) -> Decision<'_> {
        if let Some(b) = self
            .builtin
            .iter()
            .find(|p| !p.pattern.negated && p.matches_segments(segments, is_dir))
        {
            return Decision {
                ignored: true,
                by: Some(&b.pattern),
            };
        }

        let root = last_match(&self.root_patterns, segments, is_dir);
        if root.ignored {
            return root;
        }
        let tree = last_match(&self.tree_patterns, segments, is_dir);
        if tree.by.is_some() {
            tree
        } else {
            root
        }
    }
}

fn compile_all(patterns: Vec<IgnorePattern>) -> Vec<CompiledPattern> {
    patterns.into_iter().map(CompiledPattern::new).collect()
}

/// Last-match-wins evaluation within one layer.
fn last_match<'a>(
    patterns: &'a [CompiledPattern],
    segments: &[&str],
    is_dir: bool,
) -> Decision<'a> {
    patterns
        .iter()
        .rev()
        .find(|p| p.matches_segments(segments, is_dir))
        .map(|p| Decision {
            ignored: !p.pattern.negated,
            by: Some(&p.pattern),
        })
        .unwrap_or(Decision {
            ignored: false,
            by: None,
        })
}

impl Default for IgnoreEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(lines: &[&str]) -> IgnoreEngine {
        let mut engine = IgnoreEngine::new();
        for l in lines {
            engine.add_root_pattern(l);
        }
        engine
    }

    fn pat(line: &str) -> IgnorePattern {
        IgnorePattern::parse(line, IgnoreSource::RootIgnore).expect("pattern parses")
    }

    #[test]
    fn test_builtin_ignores() {
        let engine = IgnoreEngine::new();
        assert!(engine.is_ignored(".wt/config.toml"));
        assert!(engine.is_ignored(".git/objects"));
    }

    #[test]
    fn test_builtin_ignores_cover_dirs_and_nested() {
        let engine = IgnoreEngine::new();
        assert!(engine.is_ignored_dir(".wt"));
        assert!(engine.is_ignored(".wt/"));
        assert!(engine.is_ignored("sub/.git/HEAD"));
        assert!(engine.is_ignored("nested/.wt-tree/meta"));
        assert!(!engine.is_ignored(".wtignore"));
        assert!(!engine.is_ignored(".gitignore"));
        // A *file* called `.wt` is not a metadata directory.
        assert!(!engine.is_ignored(".wt"));
    }

    #[test]
    fn test_builtin_cannot_be_negated() {
        let engine = root(&["!.wt/", "!.wt/**", "!.git"]);
        assert!(engine.is_ignored(".wt/config.toml"));
        assert!(engine.is_ignored_dir(".git"));
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
    fn test_negation_last_match_wins() {
        let engine = root(&["!important.log", "*.log"]);
        assert!(engine.is_ignored("important.log"));
    }

    #[test]
    fn test_cannot_reinclude_file_in_excluded_dir() {
        let engine = root(&["logs/", "!logs/keep.log"]);
        assert!(engine.is_ignored("logs/keep.log"));
        // But excluding the contents (not the dir) allows re-inclusion.
        let engine = root(&["logs/*", "!logs/keep.log"]);
        assert!(!engine.is_ignored("logs/keep.log"));
        assert!(engine.is_ignored("logs/other.log"));
        assert!(!engine.is_ignored_dir("logs"));
    }

    #[test]
    fn test_doublestar() {
        let pat = IgnorePattern::parse("src/**/*.rs", IgnoreSource::RootIgnore).unwrap();
        assert!(pat.matches("src/main.rs"));
        assert!(pat.matches("src/deep/nested/file.rs"));
        assert!(!pat.matches("other/file.rs"));
    }

    #[test]
    fn test_leading_doublestar() {
        let engine = root(&["**/foo"]);
        assert!(engine.is_ignored("foo"));
        assert!(engine.is_ignored("a/b/foo"));
        assert!(engine.is_ignored("a/foo/bar.txt"));
        assert!(!engine.is_ignored("a/foobar"));

        let engine = root(&["**/foo/bar"]);
        assert!(engine.is_ignored("foo/bar"));
        assert!(engine.is_ignored("x/y/foo/bar"));
        assert!(!engine.is_ignored("foo/x/bar"));
    }

    #[test]
    fn test_trailing_doublestar() {
        let engine = root(&["abc/**"]);
        assert!(engine.is_ignored("abc/x"));
        assert!(engine.is_ignored("abc/x/y/z"));
        assert!(!engine.is_ignored_dir("abc"));
        assert!(!engine.is_ignored("xabc/x"));
    }

    #[test]
    fn test_middle_doublestar() {
        let engine = root(&["a/**/b"]);
        assert!(engine.is_ignored("a/b"));
        assert!(engine.is_ignored("a/x/b"));
        assert!(engine.is_ignored("a/x/y/b"));
        assert!(!engine.is_ignored("a/x/c"));
        assert!(!engine.is_ignored("z/a/b"));
    }

    #[test]
    fn test_non_segment_doublestar_is_single_star() {
        let engine = root(&["a**b"]);
        assert!(engine.is_ignored("axyb"));
        assert!(engine.is_ignored("dir/ab"));
        let p = pat("/x**y");
        assert!(!p.matches_path("x/a/y", false));
    }

    #[test]
    fn test_default_ignores() {
        let engine = IgnoreEngine::with_defaults();
        assert!(engine.is_ignored("node_modules/package.json"));
        assert!(engine.is_ignored("target/debug/binary"));
        assert!(!engine.is_ignored("src/main.rs"));
    }

    #[test]
    fn test_default_ignores_nested_and_reinclude() {
        let mut engine = IgnoreEngine::with_defaults();
        assert!(engine.is_ignored("web/node_modules/react/index.js"));
        assert!(engine.is_ignored(".file.swp"));
        assert!(engine.is_ignored("a/.DS_Store"));
        engine.add_root_pattern("!dist/");
        assert!(!engine.is_ignored("dist/app.js"));
    }

    #[test]
    fn test_tree_cannot_negate_root() {
        let mut engine = IgnoreEngine::new();
        engine.add_root_pattern("*.log");
        engine.add_tree_pattern("!*.log", "backend"); // Should be blocked
        assert!(engine.is_ignored("debug.log")); // Still ignored
    }

    #[test]
    fn test_tree_negation_cannot_override_root_via_other_pattern() {
        // Regression: a tree-level negation used to override a root ignore
        // whenever *any* tree-level ignore also matched the path.
        let mut engine = root(&["*.log"]);
        engine.add_tree_pattern("debug.*", "backend");
        engine.add_tree_pattern("!debug.log", "backend");
        assert!(engine.is_ignored("debug.log"));
    }

    #[test]
    fn test_tree_negation_overrides_tree_ignore() {
        let mut engine = IgnoreEngine::new();
        engine.add_tree_pattern("*.tmp", "backend");
        engine.add_tree_pattern("!keep.tmp", "backend");
        assert!(engine.is_ignored("x.tmp"));
        assert!(!engine.is_ignored("keep.tmp"));
    }

    #[test]
    fn test_tree_can_add_ignores_over_root_negation() {
        let mut engine = root(&["*.log", "!keep.log"]);
        engine.add_tree_pattern("keep.log", "t");
        assert!(engine.is_ignored("keep.log"));
    }

    #[test]
    fn test_anchoring_leading_slash() {
        let engine = root(&["/build"]);
        assert!(engine.is_ignored("build"));
        assert!(engine.is_ignored("build/out.o"));
        assert!(!engine.is_ignored("src/build"));
    }

    #[test]
    fn test_anchoring_middle_slash() {
        let engine = root(&["doc/frotz"]);
        assert!(engine.is_ignored("doc/frotz"));
        assert!(!engine.is_ignored("a/doc/frotz"));
    }

    #[test]
    fn test_unanchored_matches_any_depth() {
        let engine = root(&["frotz"]);
        assert!(engine.is_ignored("frotz"));
        assert!(engine.is_ignored("a/b/frotz"));
        assert!(engine.is_ignored("a/frotz/file"));
    }

    #[test]
    fn test_directory_only() {
        let engine = root(&["build/"]);
        assert!(engine.is_ignored_dir("build"));
        assert!(engine.is_ignored("build/"));
        assert!(engine.is_ignored("build/x.o"));
        assert!(engine.is_ignored("src/build/x.o"));
        // A regular file named `build` is not matched.
        assert!(!engine.is_ignored("build"));
        assert!(!engine.is_ignored_path("src/build", false));
    }

    #[test]
    fn test_star_does_not_cross_slash() {
        let engine = root(&["/src/*.rs"]);
        assert!(engine.is_ignored("src/main.rs"));
        assert!(!engine.is_ignored("src/sub/main.rs"));
    }

    #[test]
    fn test_question_mark() {
        let engine = root(&["file?.txt"]);
        assert!(engine.is_ignored("file1.txt"));
        assert!(!engine.is_ignored("file10.txt"));
        assert!(!engine.is_ignored("file.txt"));
        let p = pat("/a?b");
        assert!(!p.matches_path("a/b", false));
    }

    #[test]
    fn test_character_classes() {
        let engine = root(&["*.[oa]", "log[0-9]", "x[!a-c]", "[[:digit:]]*.tmp", "y[]]"]);
        assert!(engine.is_ignored("lib.o"));
        assert!(engine.is_ignored("lib.a"));
        assert!(!engine.is_ignored("lib.c"));
        assert!(engine.is_ignored("log7"));
        assert!(!engine.is_ignored("logx"));
        assert!(engine.is_ignored("xd"));
        assert!(!engine.is_ignored("xb"));
        assert!(engine.is_ignored("9abc.tmp"));
        assert!(!engine.is_ignored("abc.tmp"));
        assert!(engine.is_ignored("y]"));
    }

    #[test]
    fn test_unterminated_class_is_literal() {
        let engine = root(&["a[b"]);
        assert!(engine.is_ignored("a[b"));
        assert!(!engine.is_ignored("ab"));
    }

    #[test]
    fn test_comments_blank_and_escapes() {
        assert!(IgnorePattern::parse("# comment", IgnoreSource::RootIgnore).is_none());
        assert!(IgnorePattern::parse("   ", IgnoreSource::RootIgnore).is_none());
        assert!(IgnorePattern::parse("!", IgnoreSource::RootIgnore).is_none());
        assert!(IgnorePattern::parse("/", IgnoreSource::RootIgnore).is_none());

        let engine = root(&["\\#hash", "\\!bang", "star\\*"]);
        assert!(engine.is_ignored("#hash"));
        assert!(engine.is_ignored("!bang"));
        assert!(engine.is_ignored("star*"));
        assert!(!engine.is_ignored("starx"));
    }

    #[test]
    fn test_trailing_whitespace() {
        let p = pat("foo.txt   \r");
        assert_eq!(p.pattern, "foo.txt");
        let p = pat("foo\\ ");
        assert_eq!(p.pattern, "foo\\ ");
        let engine = root(&["foo\\ "]);
        assert!(engine.is_ignored("foo "));
        assert!(!engine.is_ignored("foo"));
    }

    #[test]
    fn test_parse_fields() {
        let p = pat("!/build/");
        assert!(p.negated);
        assert!(p.anchored);
        assert!(p.directory_only);
        assert_eq!(p.pattern, "build");
        assert_eq!(p.to_line(), "!/build/");
        assert_eq!(pat("a/b/").to_line(), "a/b/");
        assert_eq!(pat("*.log").to_string(), "*.log");
        assert!(!pat("*.log").anchored);
    }

    #[test]
    fn test_windows_and_dot_paths() {
        let engine = root(&["/build/", "*.log"]);
        assert!(engine.is_ignored("build\\out.o"));
        assert!(engine.is_ignored("./build/out.o"));
        assert!(engine.is_ignored("src\\x.log"));
        assert!(!engine.is_ignored(""));
    }

    #[test]
    fn test_matching_pattern_explains() {
        let engine = root(&["logs/", "*.log", "!keep.log"]);
        let p = engine.matching_pattern("logs/a.txt", false).unwrap();
        assert_eq!(p.pattern, "logs");
        let p = engine.matching_pattern("keep.log", false).unwrap();
        assert!(p.negated);
        assert!(engine.matching_pattern("src/main.rs", false).is_none());
        let p = engine.matching_pattern(".wt/HEAD", false).unwrap();
        assert_eq!(p.source, IgnoreSource::BuiltIn);
    }

    #[test]
    fn test_patterns_listing_order() {
        let mut engine = root(&["*.log"]);
        engine.add_tree_pattern("*.tmp", "t");
        let lines: Vec<String> = engine.patterns().map(IgnorePattern::to_line).collect();
        assert_eq!(lines, vec![".wt/", ".wt-tree/", ".git/", "*.log", "*.tmp"]);
    }

    #[test]
    fn test_pattern_matches_parent_and_exact() {
        let p = pat("target/");
        assert!(p.matches("target/debug/x"));
        assert!(!p.matches("target"));
        assert!(p.matches("target/"));
        assert!(p.matches_path("target", true));
        assert!(!p.matches_path("target/debug", true));
    }

    #[test]
    fn test_star_backtracking() {
        let engine = root(&["*a*b*c"]);
        assert!(engine.is_ignored("xxaxxbxxc"));
        assert!(engine.is_ignored("abc"));
        assert!(!engine.is_ignored("acb"));
        let engine = root(&["a*"]);
        assert!(engine.is_ignored("a"));
    }
}
