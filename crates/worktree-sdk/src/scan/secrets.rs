//! Rule-based secret detection.
//!
//! Each [`Rule`] is a byte regex plus cheap post-filters (placeholder
//! suppression, Shannon-entropy floor, letter+digit requirement, and for
//! private keys a check that a key body actually follows the PEM header).
//! Every rule is run once over the whole buffer — the regex engine's literal
//! prefilters make this far faster than per-line matching — and matches are
//! then mapped to lines, deduplicated (the earliest rule in [`rules`] wins
//! when two rules flag the same bytes) and rendered as redacted excerpts.
//!
//! Patterns never match across a line break, so every finding belongs to
//! exactly one line.

use std::sync::LazyLock;

use regex::bytes::{Regex, RegexBuilder};

use super::{is_binary, SecretFinding};

/// Inline marker that suppresses every finding on the line containing it,
/// e.g. `let key = "..."; // wt:allow-secret`.
pub const ALLOW_MARKER: &str = "wt:allow-secret";

/// Only the first this-many bytes of each line are scanned. Longer lines are
/// almost always minified or generated content; capping bounds the work and
/// the false-positive surface.
pub const MAX_SCAN_LINE_LEN: usize = 8 * 1024;

/// Excerpts longer than this are windowed around the finding.
const MAX_EXCERPT_LEN: usize = 200;
/// Bytes of context kept on each side of a secret in a windowed excerpt.
const EXCERPT_CONTEXT: usize = 60;
/// Secrets shorter than this are fully masked instead of keeping 4+4 chars.
const MIN_PARTIAL_REDACT_LEN: usize = 12;
const MASK: &str = "****";

/// A built-in secret detection rule.
///
/// The secret value a rule reports is its first participating capture group,
/// or the whole match when the pattern has no groups. Post-filters run on
/// that value.
#[derive(Debug)]
pub struct Rule {
    /// Stable identifier reported in [`SecretFinding::rule`].
    pub id: &'static str,
    /// Human-readable description for documentation and CLI listings.
    pub description: &'static str,
    /// Regex source (Rust `regex` syntax, matched against bytes with Unicode
    /// disabled and multi-line mode on).
    pub pattern: &'static str,
    /// Minimum Shannon entropy (bits per byte) of the secret value; 0 = none.
    min_entropy: f64,
    /// The value must contain at least one ASCII letter and one ASCII digit.
    needs_letter_and_digit: bool,
    /// Suppress values that look like documentation placeholders.
    skip_placeholders: bool,
    /// The value is a structural marker, not secret material: show it as is
    /// and mask the remainder of the line instead (private-key headers).
    mask_rest_of_line: bool,
    /// Extra validation on the bytes following the match.
    validate_tail: Option<fn(&[u8]) -> bool>,
}

impl Rule {
    /// Minimum Shannon entropy (bits per byte) required of the secret value,
    /// or 0.0 when the rule has no entropy gate.
    pub fn min_entropy(&self) -> f64 {
        self.min_entropy
    }

    /// Whether the post-filters accept `value` as a real secret.
    fn accepts(&self, value: &[u8]) -> bool {
        if self.skip_placeholders && is_placeholder(value) {
            return false;
        }
        if self.needs_letter_and_digit
            && !(value.iter().any(u8::is_ascii_digit) && value.iter().any(u8::is_ascii_alphabetic))
        {
            return false;
        }
        self.min_entropy <= 0.0 || shannon_entropy(value) >= self.min_entropy
    }
}

/// Entropy floor applied to vendor tokens: rejects runs like `AKIA` + 16 `A`s
/// while accepting any realistic random token.
const TOKEN_ENTROPY: f64 = 2.5;
/// Entropy floor for context-only (keyword = value) rules.
const GENERIC_ENTROPY: f64 = 3.5;

const fn token_rule(id: &'static str, description: &'static str, pattern: &'static str) -> Rule {
    Rule {
        id,
        description,
        pattern,
        min_entropy: TOKEN_ENTROPY,
        needs_letter_and_digit: false,
        skip_placeholders: true,
        mask_rest_of_line: false,
        validate_tail: None,
    }
}

/// Built-in rules in priority order (earlier rules win overlaps).
static RULES: [Rule; 16] = [
    Rule {
        id: "private-key",
        description: "PEM / OpenSSH / PGP private key block",
        pattern: r"-----BEGIN[ ](?:(?:RSA|EC|DSA|OPENSSH|PGP|ENCRYPTED)[ ])?PRIVATE[ ]KEY(?:[ ]BLOCK)?-----",
        min_entropy: 0.0,
        needs_letter_and_digit: false,
        skip_placeholders: false,
        mask_rest_of_line: true,
        validate_tail: Some(followed_by_key_body),
    },
    token_rule(
        "aws-access-key",
        "AWS access key id (AKIA/ASIA/ABIA/ACCA + 16 characters)",
        r"\b((?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16})\b",
    ),
    Rule {
        needs_letter_and_digit: true,
        min_entropy: GENERIC_ENTROPY,
        ..token_rule(
            "aws-secret-key",
            "AWS secret access key assigned to an aws_*secret* / *_sk variable",
            r#"(?i)aws[a-z0-9_.-]{0,20}(?:secret|sk)[a-z0-9_.-]{0,20}["']?[ \t]*(?::=|=>|=|:)[ \t]*["']?([a-z0-9/+=]{40})(?:[^a-z0-9/+=]|$)"#,
        )
    },
    token_rule(
        "github-fine-grained-token",
        "GitHub fine-grained personal access token (github_pat_)",
        r"\bgithub_pat_[A-Za-z0-9_]{60,255}\b",
    ),
    token_rule(
        "github-token",
        "GitHub token (ghp_, gho_, ghu_, ghs_, ghr_)",
        r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b",
    ),
    token_rule(
        "gitlab-token",
        "GitLab personal access token (glpat-)",
        r"\bglpat-[A-Za-z0-9_-]{20,}",
    ),
    token_rule(
        "slack-webhook",
        "Slack incoming webhook URL",
        r"https://hooks\.slack\.com/(?:services|workflows|triggers)/T[A-Z0-9]+/[A-Z0-9]+/([A-Za-z0-9]{16,})",
    ),
    Rule {
        needs_letter_and_digit: true,
        ..token_rule(
            "slack-token",
            "Slack API token (xoxb-, xoxa-, xoxp-, xoxr-, xoxs-)",
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}",
        )
    },
    token_rule(
        "stripe-secret-key",
        "Stripe live secret or restricted key (sk_live_, rk_live_)",
        r"\b(?:sk|rk)_live_[A-Za-z0-9]{24,}",
    ),
    token_rule(
        "google-api-key",
        "Google API key (AIza + 35 characters)",
        r"\bAIza[0-9A-Za-z_-]{35}(?:[^0-9A-Za-z_-]|$)",
    ),
    token_rule(
        "npm-token",
        "npm access token (npm_)",
        r"\bnpm_[A-Za-z0-9]{36}\b",
    ),
    token_rule(
        "anthropic-api-key",
        "Anthropic API key (sk-ant-)",
        r"\bsk-ant-[A-Za-z0-9_-]{32,}",
    ),
    Rule {
        needs_letter_and_digit: true,
        ..token_rule(
            "openai-api-key",
            "OpenAI-style API key (sk-, sk-proj-, sk-svcacct-, sk-admin-)",
            r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{32,}",
        )
    },
    token_rule(
        "jwt",
        "JSON Web Token (signed, three base64url segments)",
        r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
    ),
    Rule {
        needs_letter_and_digit: true,
        min_entropy: GENERIC_ENTROPY,
        ..token_rule(
            "generic-secret",
            "High-entropy quoted value assigned to a password/secret/api_key/token variable",
            r#"(?i)(?:password|passwd|pwd|secret|api[_-]?key|apikey|token|credential|access[_-]?key|private[_-]?key)[a-z0-9_.-]{0,32}["'\]]{0,2}[ \t]*(?::=|=>|=|:)[ \t]*(?:"([^"\s]{20,256})"|'([^'\s]{20,256})'|`([^`\s]{20,256})`)"#,
        )
    },
    Rule {
        needs_letter_and_digit: true,
        min_entropy: GENERIC_ENTROPY,
        ..token_rule(
            "generic-secret",
            "High-entropy unquoted value assigned to a password/secret/api_key/token variable (.env / YAML style)",
            r#"(?i)(?:password|passwd|pwd|secret|api[_-]?key|apikey|token|credential|access[_-]?key|private[_-]?key)[a-z0-9_.-]{0,32}[ \t]*(?:=|:)[ \t]*([a-z0-9+/_-]{20,256})(?:[ \t;,#\r]|$)"#,
        )
    },
];

/// The built-in rule set, in priority order. Rule ids are stable; several
/// rules may share an id when they detect the same kind of secret in
/// different syntaxes.
pub fn rules() -> &'static [Rule] {
    &RULES
}

static COMPILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    RULES
        .iter()
        .map(|rule| {
            RegexBuilder::new(rule.pattern)
                .unicode(false)
                .multi_line(true)
                .build()
                .unwrap_or_else(|e| panic!("built-in secret rule {} is invalid: {e}", rule.id))
        })
        .collect()
});

/// A user-defined rule (`[secret_scanning] patterns`). It has no post-filters:
/// every match of its regex (its first capture group, if any) is a finding.
#[derive(Debug, Clone)]
pub(super) struct CustomRule {
    pub(super) id: String,
    regex: Regex,
}

/// Compile a custom rule with the same regex flags as the built-in rules.
pub(super) fn compile_custom(id: &str, pattern: &str) -> Result<CustomRule, String> {
    let regex = RegexBuilder::new(pattern)
        .unicode(false)
        .multi_line(true)
        .build()
        .map_err(|e| format!("invalid regex {pattern:?}: {e}"))?;
    Ok(CustomRule {
        id: id.to_string(),
        regex,
    })
}

/// Rule id of candidate index `idx` (built-ins first, then custom rules).
fn rule_id(idx: usize, custom: &[CustomRule]) -> &str {
    match RULES.get(idx) {
        Some(rule) => rule.id,
        None => custom
            .get(idx - RULES.len())
            .map_or("custom", |c| c.id.as_str()),
    }
}

/// Whether the rule at `idx` masks the rest of the line (private-key headers).
fn masks_rest_of_line(idx: usize) -> bool {
    RULES.get(idx).is_some_and(|r| r.mask_rest_of_line)
}

/// A candidate secret: byte span of the value plus the rule that found it.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    rule: usize,
    start: usize,
    end: usize,
}

/// Scan one buffer with the built-in rules; see [`super::scan_secrets`].
pub(super) fn scan(path: &str, data: &[u8]) -> Vec<SecretFinding> {
    scan_with(path, data, &[])
}

/// Scan one buffer with the built-in rules plus `custom` rules.
pub(super) fn scan_with(path: &str, data: &[u8], custom: &[CustomRule]) -> Vec<SecretFinding> {
    if data.is_empty() || is_binary(data) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for (j, rule) in custom.iter().enumerate() {
        for caps in rule.regex.captures_iter(data) {
            let Some(whole) = caps.get(0) else { continue };
            let secret = caps.iter().skip(1).flatten().next().unwrap_or(whole);
            if secret.start() == secret.end() {
                continue;
            }
            candidates.push(Candidate {
                rule: RULES.len() + j,
                start: secret.start(),
                end: secret.end(),
            });
        }
    }
    for (idx, (rule, re)) in RULES.iter().zip(COMPILED.iter()).enumerate() {
        for caps in re.captures_iter(data) {
            let Some(whole) = caps.get(0) else { continue };
            let secret = caps.iter().skip(1).flatten().next().unwrap_or(whole);
            if !rule.accepts(secret.as_bytes()) {
                continue;
            }
            if let Some(validate) = rule.validate_tail {
                if !validate(&data[whole.end()..]) {
                    continue;
                }
            }
            candidates.push(Candidate {
                rule: idx,
                start: secret.start(),
                end: secret.end(),
            });
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }
    candidates.sort_by_key(|c| (c.start, c.rule));

    let mut findings = Vec::new();
    let mut line_no = 1;
    let mut counted_to = 0;
    let mut i = 0;
    while i < candidates.len() {
        let start = candidates[i].start;
        line_no += count_newlines(&data[counted_to..start]);
        counted_to = start;

        let line_start = data[..start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let line_end = data[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |p| start + p);
        let group_end = candidates[i..]
            .iter()
            .position(|c| c.start >= line_end)
            .map_or(candidates.len(), |p| i + p);

        let line = &data[line_start..line_end];
        if !contains(line, ALLOW_MARKER.as_bytes()) {
            let local: Vec<Candidate> = candidates[i..group_end]
                .iter()
                .filter(|c| c.end <= line_end && c.end - line_start <= MAX_SCAN_LINE_LEN)
                .map(|c| Candidate {
                    rule: c.rule,
                    start: c.start - line_start,
                    end: c.end - line_start,
                })
                .collect();
            emit_line(path, line_no, line, local, custom, &mut findings);
        }
        i = group_end;
    }
    findings
}

/// Deduplicate one line's candidates and append findings with a shared
/// redacted excerpt. Candidate offsets are relative to `line`.
fn emit_line(
    path: &str,
    line_no: usize,
    line: &[u8],
    mut local: Vec<Candidate>,
    custom: &[CustomRule],
    out: &mut Vec<SecretFinding>,
) {
    if local.is_empty() {
        return;
    }
    // Earlier rules win; drop anything overlapping an accepted span.
    local.sort_by_key(|c| (c.rule, c.start));
    let mut accepted: Vec<Candidate> = Vec::with_capacity(local.len());
    for c in local {
        if accepted
            .iter()
            .all(|a| c.end <= a.start || c.start >= a.end)
        {
            accepted.push(c);
        }
    }
    accepted.sort_by_key(|c| c.start);

    let (redacted, positions) = redact_line(line, &accepted);
    for (c, pos) in accepted.iter().zip(positions) {
        out.push(SecretFinding {
            rule: rule_id(c.rule, custom).to_string(),
            path: path.to_string(),
            line: line_no,
            excerpt: excerpt(&redacted, pos),
        });
    }
}

/// Render `line` with every accepted span redacted. Returns the redacted
/// text and, per span, its byte offset in that text (`None` if the span was
/// swallowed by a masked line tail).
fn redact_line(line: &[u8], spans: &[Candidate]) -> (String, Vec<Option<usize>>) {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut out = String::with_capacity(line.len());
    let mut positions = Vec::with_capacity(spans.len());
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor {
            positions.push(None);
            continue;
        }
        let end = span.end.min(line.len());
        out.push_str(&String::from_utf8_lossy(&line[cursor..span.start]));
        positions.push(Some(out.len()));
        let value = &line[span.start..end];
        if masks_rest_of_line(span.rule) {
            out.push_str(&String::from_utf8_lossy(value));
            if line[end..].iter().any(|b| !b.is_ascii_whitespace()) {
                out.push_str(MASK);
            }
            cursor = line.len();
        } else {
            out.push_str(&redact(value));
            cursor = end;
        }
    }
    if cursor < line.len() {
        out.push_str(&String::from_utf8_lossy(&line[cursor..]));
    }
    (out, positions)
}

/// Keep the first and last four characters of a secret, masking the rest
/// (`AKIAIOSFODNN7EXAMPLE` → `AKIA****MPLE`). Short values are fully masked.
fn redact(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < MIN_PARTIAL_REDACT_LEN {
        return MASK.to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}{MASK}{tail}")
}

/// Trim the redacted line and, if it is long, window it around `pos`.
fn excerpt(redacted: &str, pos: Option<usize>) -> String {
    let trimmed = redacted.trim();
    if trimmed.len() <= MAX_EXCERPT_LEN {
        return trimmed.to_string();
    }
    let lead = redacted.len() - redacted.trim_start().len();
    let pos = pos.map_or(0, |p| p.saturating_sub(lead)).min(trimmed.len());
    let from = floor_char_boundary(trimmed, pos.saturating_sub(EXCERPT_CONTEXT));
    let to = floor_char_boundary(trimmed, (pos + EXCERPT_CONTEXT * 2).min(trimmed.len()));
    let mut out = String::with_capacity(to - from + 8);
    if from > 0 {
        out.push('…');
    }
    out.push_str(trimmed[from..to].trim());
    if to < trimmed.len() {
        out.push('…');
    }
    out
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Substrings (lower-cased) that mark a value as a documentation placeholder
/// rather than a live credential.
const PLACEHOLDER_MARKERS: &[&str] = &[
    "example",
    "xxx",
    "changeme",
    "change_me",
    "change-me",
    "placeholder",
    "dummy",
    "your",
    "redacted",
    "replace",
    "insert",
    "sample",
    "<",
    ">",
    "${",
    "{{",
    "}}",
    "****",
    "...",
];

fn is_placeholder(value: &[u8]) -> bool {
    let lower = value.to_ascii_lowercase();
    PLACEHOLDER_MARKERS
        .iter()
        .any(|m| contains(&lower, m.as_bytes()))
}

/// Shannon entropy of `value` in bits per byte.
fn shannon_entropy(value: &[u8]) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in value {
        counts[b as usize] += 1;
    }
    let len = value.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Validates that a PEM / PGP header is followed by key material — either on
/// the next lines or after an escaped `\n` (keys embedded in JSON/env
/// strings) — so code that merely mentions the header is not flagged.
fn followed_by_key_body(tail: &[u8]) -> bool {
    let mut rest = tail;
    // Allow a few blank or `Name: value` armor-header lines before the body.
    for _ in 0..4 {
        rest = match strip_line_break(rest) {
            Some(r) => r,
            None => return false,
        };
        let line_len = next_line_break(rest);
        let line = trim_quotes(&rest[..line_len]);
        if line.is_empty() || line.contains(&b':') {
            rest = &rest[line_len..];
            continue;
        }
        return line.len() >= 16
            && line
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
    }
    false
}

/// Strip one leading line break: real (`\n`, `\r\n`) or escaped (`\n`
/// written as backslash-n, optionally `\r\n`).
fn strip_line_break(s: &[u8]) -> Option<&[u8]> {
    let s = s.strip_prefix(b"\r").unwrap_or(s);
    if let Some(r) = s.strip_prefix(b"\n") {
        return Some(r);
    }
    let s = s.strip_prefix(b"\\r").unwrap_or(s);
    s.strip_prefix(b"\\n")
}

/// Length of the content before the next real or escaped line break.
fn next_line_break(s: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'\n' | b'\r' => return i,
            b'\\' if matches!(s.get(i + 1), Some(b'n' | b'r')) => return i,
            _ => i += 1,
        }
    }
    s.len()
}

fn trim_quotes(s: &[u8]) -> &[u8] {
    let is_trim = |b: &u8| b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',' | b'+');
    let start = s.iter().position(|b| !is_trim(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is_trim(b)).map_or(start, |p| p + 1);
    &s[start..end]
}

fn count_newlines(s: &[u8]) -> usize {
    s.iter().filter(|&&b| b == b'\n').count()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::super::scan_secrets;
    use super::*;
    use std::collections::HashSet;

    // Test secrets are assembled at runtime from fragments so this source
    // file never contains a literal token (keeps W0rkTree's own scanner and
    // hosting-provider push protection quiet).

    const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const UPPER_DIGIT: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// Deterministic random string over `charset` that always contains a
    /// digit and (if the charset has them) letters.
    fn rand_str(seed: u64, len: usize, charset: &[u8]) -> String {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut s: Vec<u8> = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                charset[(state % charset.len() as u64) as usize]
            })
            .collect();
        // Guarantee mixed classes regardless of the draw.
        if len >= 2 {
            s[len / 2] = b'7';
            s[len / 3] = charset[0];
        }
        String::from_utf8(s).unwrap_or_default()
    }

    fn alnum(seed: u64, len: usize) -> String {
        rand_str(seed, len, ALNUM)
    }

    fn scan(text: &str) -> Vec<SecretFinding> {
        scan_secrets("src/config.rs", text.as_bytes())
    }

    fn rule_ids(text: &str) -> Vec<String> {
        scan(text).into_iter().map(|f| f.rule).collect()
    }

    fn assert_detects(text: &str, rule: &str) {
        let found = scan(text);
        assert!(
            found.iter().any(|f| f.rule == rule),
            "expected {rule} in {text:?}, got {found:?}"
        );
    }

    fn assert_clean(text: &str) {
        let found = scan(text);
        assert!(
            found.is_empty(),
            "expected no findings in {text:?}, got {found:?}"
        );
    }

    fn aws_key() -> String {
        ["AK", "IA", &rand_str(11, 16, UPPER_DIGIT)].concat()
    }

    fn private_key_block(kind: &str) -> String {
        let kind = if kind.is_empty() {
            String::new()
        } else {
            format!("{kind} ")
        };
        let body: String = (0..3).map(|i| rand_str(i, 64, B64) + "\n").collect();
        format!(
            "-----BEGIN {kind}PRIVATE {}-----\n{body}-----END {kind}PRIVATE KEY-----\n",
            "KEY"
        )
    }

    // ----- rule metadata ---------------------------------------------------

    #[test]
    fn rules_are_listed_and_compile() {
        let rules = rules();
        assert!(rules.len() >= 15);
        assert_eq!(COMPILED.len(), rules.len());
        for r in rules {
            assert!(!r.id.is_empty() && !r.description.is_empty() && !r.pattern.is_empty());
            assert!(r.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
        }
        let ids: HashSet<&str> = rules.iter().map(|r| r.id).collect();
        for want in [
            "aws-access-key",
            "aws-secret-key",
            "github-token",
            "github-fine-grained-token",
            "gitlab-token",
            "slack-token",
            "slack-webhook",
            "stripe-secret-key",
            "google-api-key",
            "private-key",
            "npm-token",
            "openai-api-key",
            "anthropic-api-key",
            "jwt",
            "generic-secret",
        ] {
            assert!(ids.contains(want), "missing rule {want}");
        }
    }

    // ----- AWS -------------------------------------------------------------

    #[test]
    fn aws_access_key_detected() {
        assert_detects(
            &format!("aws_access_key_id = {}\n", aws_key()),
            "aws-access-key",
        );
    }

    #[test]
    fn aws_temporary_access_key_detected() {
        let key = ["AS", "IA", &rand_str(12, 16, UPPER_DIGIT)].concat();
        assert_detects(&format!("export KEY_ID=\"{key}\""), "aws-access-key");
    }

    #[test]
    fn aws_access_key_wrong_length_ignored() {
        let short = ["AK", "IA", &rand_str(13, 12, UPPER_DIGIT)].concat();
        let long = ["AK", "IA", &rand_str(14, 20, UPPER_DIGIT)].concat();
        assert_clean(&format!("a = {short}\nb = {long}\n"));
    }

    #[test]
    fn aws_documentation_example_key_suppressed() {
        assert_clean(&format!("key = {}{}\n", "AKIA", "IOSFODNN7EXAMPLE"));
    }

    #[test]
    fn aws_secret_key_in_assignment() {
        let secret = rand_str(21, 40, B64);
        assert_detects(
            &format!("aws_secret_access_key = {secret}\n"),
            "aws-secret-key",
        );
        assert_detects(
            &format!("  \"AWS_SECRET_ACCESS_KEY\": \"{secret}\",\n"),
            "aws-secret-key",
        );
    }

    #[test]
    fn aws_secret_without_context_ignored() {
        let secret = rand_str(22, 40, B64);
        assert_clean(&format!("checksum {secret}\n"));
    }

    // ----- GitHub / GitLab -------------------------------------------------

    #[test]
    fn github_token_prefixes_detected() {
        for (i, p) in ["p", "o", "u", "s", "r"].iter().enumerate() {
            let token = ["gh", p, "_", &alnum(30 + i as u64, 36)].concat();
            assert_eq!(
                rule_ids(&format!("GH={token}\n")),
                vec!["github-token"],
                "{token}"
            );
        }
    }

    #[test]
    fn github_unknown_prefix_or_short_ignored() {
        let body = alnum(40, 36);
        assert_clean(&format!("x = gh{}_{body}\n", "x"));
        assert_clean(&format!("x = gh{}_{}\n", "p", alnum(41, 20)));
    }

    #[test]
    fn github_fine_grained_token_detected() {
        let token = ["github", "_pat_", &alnum(42, 22), "_", &alnum(43, 59)].concat();
        assert_eq!(
            rule_ids(&format!("token: {token}\n")),
            vec!["github-fine-grained-token"]
        );
    }

    #[test]
    fn gitlab_token_detected() {
        let token = ["gl", "pat-", &alnum(44, 20)].concat();
        assert_detects(&format!("GITLAB={token}"), "gitlab-token");
        assert_clean(&format!("GITLAB=gl{}-{}", "pat", alnum(45, 8)));
    }

    // ----- Slack -----------------------------------------------------------

    #[test]
    fn slack_token_detected() {
        let token = format!(
            "xo{}-{}-{}-{}",
            "xb",
            "1234567890",
            "9876543210123",
            alnum(50, 24)
        );
        assert_detects(&format!("SLACK={token}\n"), "slack-token");
    }

    #[test]
    fn slack_placeholder_suppressed() {
        assert_clean(&format!("SLACK=xo{}-your-bot-token-here\n", "xb"));
    }

    #[test]
    fn slack_webhook_detected_and_redacted() {
        let token = alnum(51, 24);
        let url = format!(
            "https://hooks.slack.com/{}/T0123ABCD/B0456EFGH/{token}",
            "services"
        );
        let found = scan(&format!("webhook = \"{url}\"\n"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "slack-webhook");
        assert!(!found[0].excerpt.contains(&token));
        assert!(found[0]
            .excerpt
            .contains("hooks.slack.com/services/T0123ABCD/"));
    }

    // ----- Stripe / Google / npm -------------------------------------------

    #[test]
    fn stripe_live_keys_detected() {
        for p in ["sk", "rk"] {
            let key = [p, "_live_", &alnum(60, 24)].concat();
            assert_detects(&format!("STRIPE={key}"), "stripe-secret-key");
        }
    }

    #[test]
    fn stripe_test_key_ignored() {
        assert_clean(&format!("STRIPE=sk_{}_{}", "test", alnum(61, 24)));
    }

    #[test]
    fn google_api_key_detected() {
        let key = ["AI", "za", &rand_str(62, 35, ALNUM)].concat();
        assert_detects(&format!("const key = \"{key}\";"), "google-api-key");
    }

    #[test]
    fn google_api_key_wrong_length_ignored() {
        assert_clean(&format!("k = AI{}{}\n", "za", alnum(63, 20)));
    }

    #[test]
    fn npm_token_detected() {
        let token = ["np", "m_", &alnum(64, 36)].concat();
        assert_detects(
            &format!("//registry.npmjs.org/:_authToken={token}\n"),
            "npm-token",
        );
    }

    // ----- OpenAI / Anthropic ---------------------------------------------

    #[test]
    fn openai_keys_detected() {
        let legacy = ["sk", "-", &alnum(70, 48)].concat();
        let project = ["sk", "-proj-", &alnum(71, 64)].concat();
        assert_eq!(
            rule_ids(&format!("OPENAI={legacy}")),
            vec!["openai-api-key"]
        );
        assert_eq!(
            rule_ids(&format!("OPENAI={project}")),
            vec!["openai-api-key"]
        );
    }

    #[test]
    fn anthropic_key_wins_over_openai_rule() {
        let key = ["sk", "-ant-", "api03-", &alnum(72, 80)].concat();
        assert_eq!(
            rule_ids(&format!("ANTHROPIC_API_KEY={key}\n")),
            vec!["anthropic-api-key"]
        );
    }

    #[test]
    fn hyphenated_words_after_sk_ignored() {
        assert_clean("class=\"sk-very-long-css-class-name-for-the-skeleton-loader\"\n");
        assert_clean("let task_name = \"task-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1\";\n");
    }

    // ----- JWT ---------------------------------------------------------------

    #[test]
    fn jwt_detected() {
        let jwt = format!(
            "ey{}.ey{}.{}",
            "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "JzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ",
            alnum(80, 43)
        );
        assert_detects(&format!("Authorization: Bearer {jwt}\n"), "jwt");
    }

    #[test]
    fn unsigned_or_partial_jwt_ignored() {
        assert_clean(&format!("h = ey{}\n", "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        assert_clean(&format!(
            "h = ey{}.ey{}.\n",
            "JhbGciOiJub25lIn0", "JzdWIiOiIxIn0"
        ));
    }

    // ----- Private keys -----------------------------------------------------

    #[test]
    fn private_key_blocks_detected() {
        for kind in ["", "RSA", "EC", "DSA", "OPENSSH", "ENCRYPTED"] {
            let found = scan(&format!("# key\n{}", private_key_block(kind)));
            assert_eq!(found.len(), 1, "kind {kind:?}: {found:?}");
            assert_eq!(found[0].rule, "private-key");
            assert_eq!(found[0].line, 2);
        }
    }

    #[test]
    fn pgp_private_key_block_with_armor_headers_detected() {
        let block = format!(
            "-----BEGIN PGP PRIVATE KEY {}-----\nVersion: GnuPG v2\n\n{}\n",
            "BLOCK",
            rand_str(90, 64, B64)
        );
        assert_detects(&block, "private-key");
    }

    #[test]
    fn private_key_header_without_body_ignored() {
        let header = format!("-----BEGIN RSA PRIVATE {}-----", "KEY");
        assert_clean(&format!(
            "if line.starts_with(\"{header}\") {{\n    parse(line);\n}}\n"
        ));
        assert_clean(&format!("{header}\n...\n"));
    }

    #[test]
    fn private_key_in_json_string_detected_without_leaking_body() {
        let body = rand_str(91, 64, B64);
        let json = format!(
            "{{\"type\": \"service_account\", \"private_key\": \"-----BEGIN PRIVATE {}-----\\n{body}\\n-----END PRIVATE KEY-----\\n\"}}\n",
            "KEY"
        );
        let found = scan(&json);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "private-key");
        assert!(
            !found[0].excerpt.contains(&body[..16]),
            "{}",
            found[0].excerpt
        );
        assert!(found[0].excerpt.contains("PRIVATE KEY-----****"));
    }

    // ----- Generic assignments ---------------------------------------------

    #[test]
    fn generic_quoted_secret_detected() {
        let value = alnum(100, 32);
        assert_detects(&format!("db_password = \"{value}\"\n"), "generic-secret");
        assert_detects(&format!("API_KEY: '{value}'\n"), "generic-secret");
        assert_detects(
            &format!("  \"client_secret\": \"{value}\",\n"),
            "generic-secret",
        );
        assert_detects(&format!("authToken := `{value}`\n"), "generic-secret");
        assert_detects(
            &format!("$config['secret'] => \"{value}\";\n"),
            "generic-secret",
        );
    }

    #[test]
    fn generic_unquoted_env_style_detected() {
        let value = alnum(101, 40);
        assert_detects(&format!("SECRET_KEY={value}\n"), "generic-secret");
        assert_detects(&format!("access_token: {value}\r\n"), "generic-secret");
    }

    #[test]
    fn generic_low_entropy_ignored() {
        assert_clean("password = \"aaaaaaaaaaaaaaaaaaaaaaaa1\"\n");
        assert_clean("token = \"1212121212121212121212ab\"\n");
    }

    #[test]
    fn generic_short_value_ignored() {
        assert_clean(&format!("password = \"{}\"\n", alnum(102, 12)));
    }

    #[test]
    fn generic_identifier_like_values_ignored() {
        assert_clean("token = \"ThisIsAVeryLongIdentifierName\"\n");
        assert_clean("password_field = \"user_password_confirmation_input\"\n");
        assert_clean("token = generateSecureRandomToken1(length)\n");
    }

    #[test]
    fn generic_env_references_ignored() {
        assert_clean("password = \"${DB_PASSWORD_FROM_ENVIRONMENT_1}\"\n");
        assert_clean("api_key: {{ .Values.apiKeyFromSecretStore1 }}\n");
        assert_clean("token = os.environ[\"GITHUB_TOKEN\"]\n");
        assert_clean("secret = process.env.CLIENT_SECRET_VALUE_FOR_PROD_1;\n");
    }

    #[test]
    fn generic_without_keyword_ignored() {
        assert_clean(&format!("checksum = \"{}\"\n", alnum(103, 40)));
    }

    // ----- Placeholders / allow marker -------------------------------------

    #[test]
    fn placeholders_suppressed() {
        let ghp = |body: &str| ["gh", "p_", body].concat();
        assert_clean(&format!("t = {}\n", ghp(&"x".repeat(36))));
        assert_clean(&format!(
            "t = {}\n",
            ghp("XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX")
        ));
        assert_clean("password = \"changeme-changeme-123456\"\n");
        assert_clean("password = \"<your-password-goes-here-1>\"\n");
        assert_clean("api_key = \"EXAMPLE1234567890abcdefgh\"\n");
        assert_clean("api_key = \"replace-with-real-key-12345\"\n");
        assert_clean(&format!(
            "STRIPE=sk_{}_{}\n",
            "live", "0000000000000000000000000000"
        ));
    }

    #[test]
    fn allow_marker_suppresses_same_line() {
        let token = ["gh", "p_", &alnum(110, 36)].concat();
        assert_clean(&format!("t = \"{token}\" // wt:allow-secret\n"));
        assert_clean(&format!("# wt:allow-secret\tt = {token}\n"));
    }

    #[test]
    fn allow_marker_only_affects_its_line() {
        let token = ["gh", "p_", &alnum(111, 36)].concat();
        let text =
            format!("// wt:allow-secret\nt = \"{token}\"\nu = \"{token}\" // wt:allow-secret\n");
        let found = scan(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }

    // ----- Redaction / excerpts / positions ---------------------------------

    #[test]
    fn redaction_keeps_four_and_four() {
        assert_eq!(redact(b"AKIAIOSFODNN7EXAMPLE"), "AKIA****MPLE");
        assert_eq!(redact(b"short"), "****");
        assert_eq!(redact(b"exactly12chr"), "exac****2chr");
    }

    #[test]
    fn excerpt_redacts_and_never_contains_secret() {
        let key = aws_key();
        let found = scan(&format!("    aws_access_key_id = {key}   \n"));
        assert_eq!(found.len(), 1);
        let want = format!("aws_access_key_id = {}****{}", &key[..4], &key[16..]);
        assert_eq!(found[0].excerpt, want);
        assert!(!found[0].excerpt.contains(&key));
    }

    #[test]
    fn multiple_secrets_on_one_line_are_all_redacted() {
        let a = aws_key();
        let b = ["gh", "p_", &alnum(120, 36)].concat();
        let found = scan(&format!("keys {a} {b}\n"));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].rule, "aws-access-key");
        assert_eq!(found[1].rule, "github-token");
        for f in &found {
            assert!(
                !f.excerpt.contains(&a) && !f.excerpt.contains(&b),
                "{}",
                f.excerpt
            );
        }
    }

    #[test]
    fn overlapping_rules_reported_once() {
        let token = ["gh", "p_", &alnum(121, 36)].concat();
        assert_eq!(
            rule_ids(&format!("github_token = \"{token}\"\n")),
            vec!["github-token"]
        );
    }

    #[test]
    fn line_numbers_are_one_based_and_crlf_safe() {
        let token = ["gh", "p_", &alnum(122, 36)].concat();
        let text = format!("a\r\nb\r\n\r\nt = {token}\r\nc\r\nt2 = {token}\r\n");
        let found = scan(&text);
        assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), vec![4, 6]);
        assert!(found.iter().all(|f| !f.excerpt.ends_with('\r')));
    }

    #[test]
    fn path_is_reported() {
        let found = scan_secrets("deploy/.env", format!("K={}\n", aws_key()).as_bytes());
        assert_eq!(found[0].path, "deploy/.env");
    }

    #[test]
    fn long_lines_are_capped_and_excerpts_windowed() {
        let token = ["gh", "p_", &alnum(123, 36)].concat();
        let filler = "a".repeat(MAX_SCAN_LINE_LEN);
        // Secret past the cap: ignored.
        assert_clean(&format!("{filler} {token}\n"));
        // Secret within the cap on a long line: found, excerpt windowed.
        let found = scan(&format!("x = {token} {filler}\n"));
        assert_eq!(found.len(), 1);
        assert!(found[0].excerpt.chars().count() <= 2 * EXCERPT_CONTEXT + 2 + 8);
        assert!(found[0].excerpt.ends_with('…'));
        assert!(found[0].excerpt.contains("ghp_****"));
    }

    #[test]
    fn binary_content_skipped() {
        let mut data = format!("K={}\n", aws_key()).into_bytes();
        data.extend_from_slice(&[0, 1, 2, 3]);
        assert!(scan_secrets("blob.bin", &data).is_empty());
    }

    #[test]
    fn non_utf8_text_still_scanned() {
        let mut data = b"caf\xe9 = 1\n".to_vec();
        data.extend_from_slice(format!("K={}\n", aws_key()).as_bytes());
        let found = scan_secrets("latin1.txt", &data);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 2);
    }

    #[test]
    fn empty_and_clean_inputs() {
        assert!(scan_secrets("empty", b"").is_empty());
        assert_clean("fn main() {\n    println!(\"hello, world\");\n}\n");
        assert_clean("let password = read_password_from_stdin()?;\n");
    }

    #[test]
    fn custom_rules_report_their_name_and_redact() {
        let rule = compile_custom("acme-token", r"\bacme_([0-9a-f]{16})\b").unwrap();
        let text = "a = 1\nid: acme_0123456789abcdef\nb = acme_short\n";
        let found = scan_with("x.cfg", text.as_bytes(), std::slice::from_ref(&rule));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].rule, "acme-token");
        assert_eq!(found[0].line, 2);
        assert!(!found[0].excerpt.contains("0123456789abcdef"));
        // The allow marker applies to custom rules too.
        let allowed = "id: acme_0123456789abcdef // wt:allow-secret\n";
        assert!(scan_with("x", allowed.as_bytes(), &[rule]).is_empty());
        assert!(compile_custom("bad", "(").is_err());
    }

    #[test]
    fn builtin_rules_win_overlaps_with_custom_rules() {
        let token = ["gh", "p_", &alnum(124, 36)].concat();
        let rule = compile_custom("any-gh", r"gh[a-z]_[A-Za-z0-9]+").unwrap();
        let found = scan_with("x", format!("t = {token}\n").as_bytes(), &[rule]);
        assert_eq!(
            found.iter().map(|f| f.rule.as_str()).collect::<Vec<_>>(),
            vec!["github-token"]
        );
    }

    #[test]
    fn entropy_values() {
        assert_eq!(shannon_entropy(b""), 0.0);
        assert_eq!(shannon_entropy(b"aaaa"), 0.0);
        assert!((shannon_entropy(b"abcd") - 2.0).abs() < 1e-9);
        assert!(shannon_entropy(alnum(130, 40).as_bytes()) > GENERIC_ENTROPY);
    }

    /// 10 MB scan budget. Run with
    /// `cargo test -p worktree-sdk --release --lib scan -- --ignored`.
    #[test]
    #[ignore = "performance check; meaningful only in release builds"]
    fn perf_scan_10mb_under_one_second() {
        let mut text = String::with_capacity(10 * 1024 * 1024 + 256);
        let mut i = 0u64;
        while text.len() < 10 * 1024 * 1024 {
            text.push_str(&format!(
                "    let value_{i} = compute(\"{}\", config.token_ttl, {i}); // password policy\n",
                alnum(i, 24)
            ));
            i += 1;
        }
        let start = std::time::Instant::now();
        let found = scan_secrets("big.rs", text.as_bytes());
        let elapsed = start.elapsed();
        assert!(
            found.is_empty(),
            "unexpected findings: {:?}",
            &found[..found.len().min(3)]
        );
        assert!(
            elapsed.as_secs_f64() < 1.0,
            "scan of 10 MB took {elapsed:?}"
        );
    }
}
