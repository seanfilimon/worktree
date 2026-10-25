//! License-aware exports (ENGINEERING_PLAN §13.8).
//!
//! A tree's license rules (`domain.LicenseRule` on the server) assign an
//! SPDX expression and a grant to the paths matched by a glob. Archives and
//! Git exports consult them to
//!
//! * skip paths whose rule forbids redistribution (unless the caller asks
//!   to keep them), and
//! * add a generated [`NOTICE_PATH`] file listing the SPDX id of every
//!   exported path covered by a rule.
//!
//! The glob syntax, the specificity order and the "proprietary" test are
//! ports of the Go server's `internal/license` package, so a path resolves
//! to the same rule locally and on the server.
//!
//! Rules are fetched from the server by the CLI and cached per tree in
//! `.wt/trees/<tree>/licenses.json` ([`load_cached_rules`],
//! [`save_cached_rules`]) so exports keep working offline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use worktree_sdk::{store::hash_bytes, Manifest, ManifestEntry};

use crate::error::{GitCompatError, Result};

/// Path of the generated notice inside an archive or exported commit.
pub const NOTICE_PATH: &str = "LICENSES/NOTICE";

/// Grant levels (`LicenseRule.grant`).
pub const GRANT_READ_ONLY: &str = "read-only";
pub const GRANT_MODIFY: &str = "modify";
pub const GRANT_REDISTRIBUTE: &str = "redistribute";

/// One license rule as served by `GET {tree}/licenses`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LicenseRule {
    pub path_glob: String,
    pub spdx: String,
    pub grant: String,
    /// Tenant slug the rule applies to ("" = every non-owning tenant).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub grantee: String,
    /// The rule stops applying afterwards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

impl LicenseRule {
    /// Whether the rule's license terms allow redistributing the files it
    /// covers: a `redistribute` grant, or no grant and a non-proprietary
    /// license (the server's license default). Unknown grants fail closed.
    pub fn permits_redistribution(&self) -> bool {
        match self.grant.as_str() {
            GRANT_REDISTRIBUTE => true,
            "" => !is_proprietary(&self.spdx),
            _ => false,
        }
    }
}

/// Rules that apply to an export at `now`: general rules (no grantee) that
/// have not expired. Grantee-specific rules describe what one other tenant
/// may do and do not change a file's license.
pub fn active_rules(rules: &[LicenseRule], now: DateTime<Utc>) -> Vec<LicenseRule> {
    rules
        .iter()
        .filter(|r| r.grantee.is_empty() && r.expires_at.is_none_or(|at| at > now))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Rule cache

/// `.wt/trees/<tree>/licenses.json`.
pub fn cache_path(wt_dir: &Path, tree: &str) -> PathBuf {
    wt_dir.join("trees").join(tree).join("licenses.json")
}

/// Cached rules of `tree`; `None` when nothing was cached yet.
pub fn load_cached_rules(wt_dir: &Path, tree: &str) -> Result<Option<Vec<LicenseRule>>> {
    let path = cache_path(wt_dir, tree);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let rules: Option<Vec<LicenseRule>> = serde_json::from_str(&text).map_err(|e| {
        GitCompatError::ConfigError(format!("{}: invalid license cache: {e}", path.display()))
    })?;
    Ok(Some(rules.unwrap_or_default()))
}

/// Replace the cached rules of `tree`.
pub fn save_cached_rules(wt_dir: &Path, tree: &str, rules: &[LicenseRule]) -> Result<()> {
    let path = cache_path(wt_dir, tree);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(rules)
        .map_err(|e| GitCompatError::ConfigError(format!("cannot encode license rules: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        // Windows refuses to rename over an existing file in some cases.
        std::fs::copy(&tmp, &path).map_err(|_| e)?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Policy

/// How an export treats licensed paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LicensePolicy {
    /// Rules in effect (see [`active_rules`]).
    pub rules: Vec<LicenseRule>,
    /// Keep paths whose rule forbids redistribution.
    pub include_restricted: bool,
}

/// A manifest after applying a [`LicensePolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licensed {
    /// Entries to export (sorted by path), including the notice if any.
    pub manifest: Manifest,
    /// Paths left out because their license forbids redistribution.
    pub restricted: Vec<String>,
    /// The generated notice (also present in `manifest` at [`NOTICE_PATH`]).
    pub notice: Option<Vec<u8>>,
}

impl LicensePolicy {
    pub fn new(rules: Vec<LicenseRule>, include_restricted: bool) -> Self {
        Self {
            rules,
            include_restricted,
        }
    }

    /// Filter `manifest` and add the notice. `header` lines are written at
    /// the top of the notice (keep them deterministic for Git exports).
    ///
    /// A snapshot's own `LICENSES/NOTICE` is replaced by the generated one
    /// when the rules cover any exported path or something was skipped;
    /// otherwise the manifest is returned unchanged and no notice is made.
    pub fn apply(&self, manifest: &Manifest, header: &[String]) -> Licensed {
        let set = RuleSet::compile(&self.rules);
        let mut kept = Vec::with_capacity(manifest.entries.len());
        let mut restricted = Vec::new();
        let mut by_spdx: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for entry in &manifest.entries {
            if entry.path == NOTICE_PATH {
                continue;
            }
            match set.resolve(&entry.path) {
                Some(rule) if !rule.permits_redistribution() && !self.include_restricted => {
                    restricted.push(entry.path.clone());
                }
                Some(rule) => {
                    by_spdx.entry(&rule.spdx).or_default().push(&entry.path);
                    kept.push(entry.clone());
                }
                None => kept.push(entry.clone()),
            }
        }
        if by_spdx.is_empty() && restricted.is_empty() {
            return Licensed {
                manifest: manifest.clone(),
                restricted,
                notice: None,
            };
        }
        let notice = render_notice(header, &by_spdx, restricted.len());
        kept.push(ManifestEntry {
            path: NOTICE_PATH.to_owned(),
            hash: hash_bytes(&notice),
            size: notice.len() as u64,
            executable: false,
            chunked: false,
        });
        kept.sort_by(|a, b| a.path.cmp(&b.path));
        Licensed {
            manifest: Manifest { entries: kept },
            restricted,
            notice: Some(notice),
        }
    }
}

/// The notice text (shared format with the server's archive endpoint).
fn render_notice(
    header: &[String],
    by_spdx: &BTreeMap<&str, Vec<&str>>,
    omitted: usize,
) -> Vec<u8> {
    let mut out = String::from("W0rkTree license notice\n=======================\n\n");
    for line in header {
        out.push_str(line);
        out.push('\n');
    }
    if !header.is_empty() {
        out.push('\n');
    }
    out.push_str(
        "SPDX license identifiers of the files in this export, from the tree's\n\
         license rules. Files that are not listed are not covered by a rule.\n",
    );
    for (spdx, paths) in by_spdx {
        out.push('\n');
        out.push_str(spdx);
        out.push('\n');
        let mut paths = paths.clone();
        paths.sort_unstable();
        for path in paths {
            out.push_str("    ");
            out.push_str(path);
            out.push('\n');
        }
    }
    if omitted > 0 {
        out.push_str(&format!(
            "\n{omitted} {} omitted: the license forbids redistribution.\n",
            if omitted == 1 {
                "path was"
            } else {
                "paths were"
            }
        ));
    }
    out.into_bytes()
}

/// Compiled rules ordered most specific first.
pub struct RuleSet<'r> {
    rules: Vec<(Glob, &'r LicenseRule)>,
}

impl<'r> RuleSet<'r> {
    /// Compile `rules`, ignoring malformed globs (the server validates them).
    pub fn compile(rules: &'r [LicenseRule]) -> Self {
        let mut compiled: Vec<(Glob, &LicenseRule)> = rules
            .iter()
            .filter_map(|r| Glob::compile(&r.path_glob).ok().map(|g| (g, r)))
            .collect();
        compiled.sort_by(|(a, _), (b, _)| a.cmp_specificity(b));
        Self { rules: compiled }
    }

    /// The rule governing `path`: the most specific matching one.
    pub fn resolve(&self, path: &str) -> Option<&'r LicenseRule> {
        let segs = split_path(path)?;
        self.rules
            .iter()
            .find(|(glob, _)| glob.matches(&segs))
            .map(|(_, rule)| *rule)
    }
}

/// [`RuleSet::resolve`] for a single lookup.
pub fn resolve<'r>(rules: &'r [LicenseRule], path: &str) -> Option<&'r LicenseRule> {
    RuleSet::compile(rules).resolve(path)
}

// ---------------------------------------------------------------------------
// Globs (port of services/server-go/internal/license/glob.go)

const MAX_GLOB_LEN: usize = 4096;

/// A compiled license path glob. See the server's `glob.go` for the syntax:
/// `**` segments match any depth, `*`/`?`/`[...]` work within a segment,
/// and a pattern matching a directory covers everything beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    raw: String,
    norm: String,
    segs: Vec<String>,
    prefix: usize,
    wild: usize,
    literals: usize,
}

fn normalize_glob(raw: &str) -> Vec<String> {
    let mut s = raw.replace('\\', "/");
    loop {
        if let Some(rest) = s.strip_prefix("./") {
            s = rest.to_owned();
        } else if let Some(rest) = s.strip_prefix('/') {
            s = rest.to_owned();
        } else {
            break;
        }
    }
    let dir = s.ends_with('/');
    let mut segs: Vec<String> = Vec::new();
    for seg in s.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == "**" && segs.last().is_some_and(|last| last == "**") {
            continue;
        }
        segs.push(seg.to_owned());
    }
    if dir && segs.last().is_some_and(|last| last != "**") {
        segs.push("**".to_owned());
    }
    if segs.is_empty() {
        segs.push("**".to_owned());
    }
    segs
}

impl Glob {
    pub fn compile(raw: &str) -> std::result::Result<Self, String> {
        if raw.trim().is_empty() {
            return Err("path glob is empty".into());
        }
        if raw.len() > MAX_GLOB_LEN {
            return Err(format!("path glob longer than {MAX_GLOB_LEN} bytes"));
        }
        if raw.contains('\0') {
            return Err("path glob contains a NUL byte".into());
        }
        let segs = normalize_glob(raw);
        for seg in &segs {
            if seg == ".." {
                return Err(format!("path glob {raw:?} must not contain '..'"));
            }
            if seg != "**" {
                let pattern: Vec<char> = seg.chars().collect();
                if match_segment(&pattern, &[]).is_err() || !well_formed(&pattern) {
                    return Err(format!("path glob {raw:?}: malformed segment {seg:?}"));
                }
            }
        }
        let norm = segs.join("/");
        let (prefix, wild, literals) = specificity(&norm);
        Ok(Self {
            raw: raw.to_owned(),
            norm,
            segs,
            prefix,
            wild,
            literals,
        })
    }

    /// Whether the glob covers the path segments (the path itself or one of
    /// its ancestor directories).
    pub fn matches(&self, path: &[String]) -> bool {
        let np = self.segs.len();
        let ns = path.len();
        let mut memo = vec![0u8; (np + 1) * (ns + 1)];
        self.match_from(0, 0, path, &mut memo)
    }

    fn match_from(&self, i: usize, j: usize, path: &[String], memo: &mut [u8]) -> bool {
        let ns = path.len();
        if i == self.segs.len() {
            return true;
        }
        let k = i * (ns + 1) + j;
        if memo[k] != 0 {
            return memo[k] == 2;
        }
        let ok = if self.segs[i] == "**" {
            self.match_from(i + 1, j, path, memo)
                || (j < ns && self.match_from(i, j + 1, path, memo))
        } else if j < ns {
            let pattern: Vec<char> = self.segs[i].chars().collect();
            let name: Vec<char> = path[j].chars().collect();
            match_segment(&pattern, &name).unwrap_or(false)
                && self.match_from(i + 1, j + 1, path, memo)
        } else {
            false
        };
        memo[k] = if ok { 2 } else { 1 };
        ok
    }

    /// Ordering: more specific globs first (see `glob.go`).
    fn cmp_specificity(&self, other: &Self) -> std::cmp::Ordering {
        other
            .prefix
            .cmp(&self.prefix)
            .then(self.wild.cmp(&other.wild))
            .then(other.literals.cmp(&self.literals))
            .then_with(|| self.norm.cmp(&other.norm))
            .then_with(|| self.raw.cmp(&other.raw))
    }
}

fn specificity(norm: &str) -> (usize, usize, usize) {
    let bytes = norm.as_bytes();
    let prefix = match norm.find(['*', '?', '[']) {
        None => norm.len(),
        Some(i) => {
            if norm[i..].starts_with("**") && i > 0 && bytes[i - 1] == b'/' {
                i - 1
            } else {
                i
            }
        }
    };
    let mut wild = 0;
    let mut literals = 0;
    for seg in norm.split('/') {
        if seg == "**" {
            wild += 1;
            continue;
        }
        let seg = seg.as_bytes();
        let mut in_star = false;
        let mut j = 0;
        while j < seg.len() {
            match seg[j] {
                b'*' => {
                    if !in_star {
                        wild += 1;
                    }
                    in_star = true;
                    j += 1;
                    continue;
                }
                b'?' => wild += 1,
                b'[' => {
                    wild += 1;
                    if let Some(k) = seg[j + 1..].iter().position(|&c| c == b']') {
                        j += k + 1;
                    }
                }
                _ => literals += 1,
            }
            in_star = false;
            j += 1;
        }
    }
    (prefix, wild, literals)
}

/// Validate bracket expressions and escapes the way Go's `path.Match` does.
fn well_formed(pattern: &[char]) -> bool {
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            '\\' => {
                if i + 1 >= pattern.len() {
                    return false;
                }
                i += 2;
            }
            '[' => match parse_class(pattern, i + 1) {
                Some((_, _, end)) => i = end,
                None => return false,
            },
            _ => i += 1,
        }
    }
    true
}

/// A character class starting after `[` at `start`: (negated, ranges,
/// index after `]`). `None` if malformed.
#[allow(clippy::type_complexity)]
fn parse_class(pattern: &[char], start: usize) -> Option<(bool, Vec<(char, char)>, usize)> {
    let mut i = start;
    let negated = pattern.get(i) == Some(&'^');
    if negated {
        i += 1;
    }
    let mut ranges = Vec::new();
    let mut first = true;
    loop {
        let c = *pattern.get(i)?;
        if c == ']' && !first {
            return Some((negated, ranges, i + 1));
        }
        first = false;
        let (lo, next) = class_char(pattern, i)?;
        i = next;
        let mut hi = lo;
        if pattern.get(i) == Some(&'-') {
            let (h, next) = class_char(pattern, i + 1)?;
            hi = h;
            i = next;
        }
        ranges.push((lo, hi));
    }
}

fn class_char(pattern: &[char], i: usize) -> Option<(char, usize)> {
    match *pattern.get(i)? {
        '-' | ']' => None,
        '\\' => pattern.get(i + 1).map(|c| (*c, i + 2)),
        c => Some((c, i + 1)),
    }
}

/// Go `path.Match` for one segment (no `/` in either input).
fn match_segment(pattern: &[char], name: &[char]) -> std::result::Result<bool, ()> {
    if pattern.is_empty() {
        return Ok(name.is_empty());
    }
    match pattern[0] {
        '*' => {
            let rest = &pattern[1..];
            for skip in 0..=name.len() {
                if match_segment(rest, &name[skip..])? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        '?' => {
            if name.is_empty() {
                return Ok(false);
            }
            match_segment(&pattern[1..], &name[1..])
        }
        '[' => {
            let (negated, ranges, end) = parse_class(pattern, 1).ok_or(())?;
            let Some(&c) = name.first() else {
                return Ok(false);
            };
            let inside = ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi);
            if inside == negated {
                return Ok(false);
            }
            match_segment(&pattern[end..], &name[1..])
        }
        '\\' => {
            let lit = *pattern.get(1).ok_or(())?;
            if name.first() != Some(&lit) {
                return Ok(false);
            }
            match_segment(&pattern[2..], &name[1..])
        }
        c => {
            if name.first() != Some(&c) {
                return Ok(false);
            }
            match_segment(&pattern[1..], &name[1..])
        }
    }
}

/// Normalize a tree-relative path into segments; `None` for empty paths or
/// paths escaping the root.
fn split_path(path: &str) -> Option<Vec<String>> {
    let path = path.replace('\\', "/");
    let mut segs: Vec<String> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop()?;
            }
            s => segs.push(s.to_owned()),
        }
    }
    (!segs.is_empty()).then_some(segs)
}

// ---------------------------------------------------------------------------
// SPDX (port of the "proprietary" test in spdx.go)

/// Whether an SPDX expression denotes proprietary code: every way of
/// satisfying it involves a proprietary license (`Proprietary`,
/// `LicenseRef-Proprietary`, any `LicenseRef-*proprietary*`). Expressions
/// that do not parse are treated as proprietary (fail closed).
pub fn is_proprietary(spdx: &str) -> bool {
    let tokens = tokenize(spdx);
    let mut parser = Parser { tokens, pos: 0 };
    match parser.or_expr() {
        Some(prop) if parser.pos == parser.tokens.len() => prop,
        _ => true,
    }
}

fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c == '(' || c == ')' || c.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if !c.is_whitespace() {
                out.push(c.to_string());
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

struct Parser {
    tokens: Vec<String>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self, op: &str) -> bool {
        self.tokens
            .get(self.pos)
            .is_some_and(|t| t.eq_ignore_ascii_case(op))
    }

    /// OR is proprietary only if every alternative is.
    fn or_expr(&mut self) -> Option<bool> {
        let mut all = self.and_expr()?;
        while self.peek_op("OR") {
            self.pos += 1;
            all &= self.and_expr()?;
        }
        Some(all)
    }

    /// AND is proprietary if any operand is.
    fn and_expr(&mut self) -> Option<bool> {
        let mut any = self.with_expr()?;
        while self.peek_op("AND") {
            self.pos += 1;
            any |= self.with_expr()?;
        }
        Some(any)
    }

    fn with_expr(&mut self) -> Option<bool> {
        let prop = self.atom()?;
        if self.peek_op("WITH") {
            self.pos += 1;
            let exception = self.tokens.get(self.pos)?;
            if !is_id(exception) {
                return None;
            }
            self.pos += 1;
        }
        Some(prop)
    }

    fn atom(&mut self) -> Option<bool> {
        let token = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        if token == "(" {
            let inner = self.or_expr()?;
            if self.tokens.get(self.pos).map(String::as_str) != Some(")") {
                return None;
            }
            self.pos += 1;
            return Some(inner);
        }
        if !is_id(&token)
            || ["AND", "OR", "WITH"]
                .iter()
                .any(|op| token.eq_ignore_ascii_case(op))
        {
            return None;
        }
        Some(is_proprietary_id(&token))
    }
}

fn is_id(token: &str) -> bool {
    let id = token.strip_suffix('+').unwrap_or(token);
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == ':')
}

fn is_proprietary_id(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    if lower == "proprietary" {
        return true;
    }
    match lower.find("licenseref-") {
        Some(i) => lower[i + "licenseref-".len()..].contains("proprietary"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn rule(glob: &str, spdx: &str, grant: &str) -> LicenseRule {
        LicenseRule {
            path_glob: glob.into(),
            spdx: spdx.into(),
            grant: grant.into(),
            ..LicenseRule::default()
        }
    }

    fn segs(p: &str) -> Vec<String> {
        split_path(p).unwrap()
    }

    #[test]
    fn glob_matching_follows_the_server() {
        let cases: &[(&str, &str, bool)] = &[
            ("**", "a/b/c.txt", true),
            ("vendor", "vendor/lib/a.rs", true),
            ("vendor/", "vendor/a.rs", true),
            ("vendor/**", "vendor/a.rs", true),
            ("vendor/**", "vendors/a.rs", false),
            ("*.rs", "a.rs", true),
            ("*.rs", "src/a.rs", false),
            ("**/*.rs", "src/x/a.rs", true),
            ("**/*.rs", "a.rs", true),
            ("services/*", "services/billing/src/main.rs", true),
            ("src/?.rs", "src/a.rs", true),
            ("src/?.rs", "src/ab.rs", false),
            ("src/[a-c].rs", "src/b.rs", true),
            ("src/[^a-c].rs", "src/b.rs", false),
            ("src/[^a-c].rs", "src/z.rs", true),
            ("./docs\\guide", "docs/guide/intro.md", true),
            ("/LICENSE", "LICENSE", true),
            ("a/**/z", "a/z", true),
            ("a/**/z", "a/b/c/z/d", true),
            ("a/**/z", "a/b/c", false),
        ];
        for (glob, path, want) in cases {
            let g = Glob::compile(glob).unwrap();
            assert_eq!(g.matches(&segs(path)), *want, "{glob} vs {path}");
        }
        // '\' is a separator (as on the server), so "x\" means "x/".
        assert!(Glob::compile("x\\").is_ok());
        for bad in ["", "  ", "a/../b", "src/[a-", "src/[]"] {
            assert!(Glob::compile(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn most_specific_rule_wins() {
        let rules = vec![
            rule("**", "MIT", ""),
            rule("src/**", "Apache-2.0", ""),
            rule("src/*.rs", "BSD-3-Clause", ""),
            rule("src/main.rs", "GPL-2.0-only", ""),
            rule("vendor", "LicenseRef-Proprietary", "read-only"),
        ];
        let set = RuleSet::compile(&rules);
        let spdx = |p: &str| set.resolve(p).map(|r| r.spdx.as_str());
        assert_eq!(spdx("README.md"), Some("MIT"));
        assert_eq!(spdx("src/lib/x.rs"), Some("Apache-2.0"));
        assert_eq!(spdx("src/lib.rs"), Some("BSD-3-Clause"));
        assert_eq!(spdx("src/main.rs"), Some("GPL-2.0-only"));
        assert_eq!(spdx("vendor/a/b.c"), Some("LicenseRef-Proprietary"));
        assert_eq!(resolve(&rules[1..2], "README.md"), None);
        assert_eq!(set.resolve("../escape"), None);
    }

    #[test]
    fn proprietary_detection() {
        for (expr, want) in [
            ("MIT", false),
            ("Proprietary", true),
            ("LicenseRef-Proprietary", true),
            ("LicenseRef-Acme-Proprietary", true),
            ("LicenseRef-Acme", false),
            ("MIT OR LicenseRef-Proprietary", false),
            ("MIT AND LicenseRef-Proprietary", true),
            (
                "(Proprietary OR LicenseRef-proprietary) WITH x-exception",
                true,
            ),
            ("GPL-2.0-only WITH Classpath-exception-2.0", false),
            ("Apache-2.0+", false),
            ("MIT OR", true),
            ("(MIT", true),
            ("", true),
        ] {
            assert_eq!(is_proprietary(expr), want, "{expr:?}");
        }
    }

    #[test]
    fn redistribution_follows_grants() {
        assert!(rule("**", "MIT", "").permits_redistribution());
        assert!(!rule("**", "Proprietary", "").permits_redistribution());
        assert!(rule("**", "Proprietary", "redistribute").permits_redistribution());
        assert!(!rule("**", "MIT", "read-only").permits_redistribution());
        assert!(!rule("**", "MIT", "modify").permits_redistribution());
        assert!(!rule("**", "MIT", "bogus").permits_redistribution());
    }

    #[test]
    fn active_rules_skip_expired_and_grantee_rules() {
        let now = Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap();
        let mut expired = rule("a", "MIT", "");
        expired.expires_at = Some(now - chrono::Duration::days(1));
        let mut future = rule("b", "MIT", "");
        future.expires_at = Some(now + chrono::Duration::days(1));
        let mut grantee = rule("c", "MIT", "");
        grantee.grantee = "other".into();
        let active = active_rules(
            &[expired, future.clone(), grantee, rule("d", "MIT", "")],
            now,
        );
        assert_eq!(active, vec![future, rule("d", "MIT", "")]);
    }

    fn entry(path: &str) -> ManifestEntry {
        ManifestEntry {
            path: path.into(),
            hash: hash_bytes(path.as_bytes()),
            size: path.len() as u64,
            executable: false,
            chunked: false,
        }
    }

    fn manifest(paths: &[&str]) -> Manifest {
        Manifest {
            entries: paths.iter().map(|p| entry(p)).collect(),
        }
    }

    #[test]
    fn policy_skips_restricted_and_writes_notice() {
        let rules = vec![
            rule("src/**", "MIT", ""),
            rule("secret/**", "LicenseRef-Proprietary", "read-only"),
        ];
        let m = manifest(&[
            "LICENSES/NOTICE",
            "README.md",
            "secret/key.txt",
            "src/a.rs",
            "src/b.rs",
        ]);
        let out = LicensePolicy::new(rules.clone(), false).apply(&m, &["Snapshot: s1".into()]);
        let paths: Vec<_> = out
            .manifest
            .entries
            .iter()
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["LICENSES/NOTICE", "README.md", "src/a.rs", "src/b.rs"]
        );
        assert_eq!(out.restricted, vec!["secret/key.txt".to_owned()]);
        let notice = String::from_utf8(out.notice.clone().unwrap()).unwrap();
        assert!(notice.starts_with("W0rkTree license notice\n"));
        assert!(notice.contains("Snapshot: s1\n"));
        assert!(notice.contains("\nMIT\n    src/a.rs\n    src/b.rs\n"));
        assert!(notice.contains("1 path was omitted"));
        assert!(!notice.contains("secret/key.txt"));
        let notice_entry = &out.manifest.entries[0];
        assert_eq!(notice_entry.hash, hash_bytes(notice.as_bytes()));

        // --include-restricted keeps the path and lists it.
        let out = LicensePolicy::new(rules, true).apply(&m, &[]);
        assert!(out.restricted.is_empty());
        let notice = String::from_utf8(out.notice.unwrap()).unwrap();
        assert!(notice.contains("\nLicenseRef-Proprietary\n    secret/key.txt\n"));
        assert!(!notice.contains("omitted"));
    }

    #[test]
    fn policy_without_matching_rules_changes_nothing() {
        let m = manifest(&["LICENSES/NOTICE", "a.txt"]);
        let out = LicensePolicy::new(vec![rule("docs/**", "MIT", "")], false).apply(&m, &[]);
        assert_eq!(out.manifest, m);
        assert!(out.notice.is_none());
        let out = LicensePolicy::default().apply(&m, &[]);
        assert_eq!(out.manifest, m);
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_cached_rules(dir.path(), "web").unwrap(), None);
        let rules = vec![rule("**", "MIT", "redistribute")];
        save_cached_rules(dir.path(), "web", &rules).unwrap();
        save_cached_rules(dir.path(), "web", &rules).unwrap();
        assert_eq!(load_cached_rules(dir.path(), "web").unwrap(), Some(rules));
        std::fs::write(cache_path(dir.path(), "web"), "null").unwrap();
        assert_eq!(load_cached_rules(dir.path(), "web").unwrap(), Some(vec![]));
        std::fs::write(cache_path(dir.path(), "web"), "{").unwrap();
        assert!(load_cached_rules(dir.path(), "web").is_err());
        // The server's JSON (tree_id, extra fields) decodes.
        let parsed: Vec<LicenseRule> = serde_json::from_str(
            r#"[{"tree_id":"t","path_glob":"a/**","spdx":"MIT","grant":"","expires_at":"2030-01-01T00:00:00Z"}]"#,
        )
        .unwrap();
        assert_eq!(parsed[0].path_glob, "a/**");
        assert!(parsed[0].expires_at.is_some());
    }
}
