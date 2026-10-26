//! The secret scanning rule set shared by the client and the server
//! (docs/ENGINEERING_PLAN.md §16.7, specs/security/SecretScanning.md).
//!
//! `scan_rules.json` is the single source of truth: the SDK scanner
//! (`worktree_sdk::scan`) compiles it at first use, and the Go server embeds
//! a byte-identical copy (`services/server-go/internal/scan/rules.json`, kept
//! in sync by a Go test). `scan_rule_cases.json` holds the parity cases both
//! scanners must reproduce exactly.

use std::sync::LazyLock;

use serde::Deserialize;

/// The rule file, verbatim.
pub const SCAN_RULES_JSON: &str = include_str!("scan_rules.json");

/// Parity cases (input parts and expected findings) shared with the Go
/// scanner's tests.
#[doc(hidden)]
pub const SCAN_RULE_CASES_JSON: &str = include_str!("scan_rule_cases.json");

/// Domain separator of finding fingerprints (see [`ScanRuleSet`]).
pub const FINGERPRINT_PREFIX: &str = "wt-secret-v1";

/// Hex digits kept of a fingerprint.
pub const FINGERPRINT_HEX_LEN: usize = 24;

/// The decoded `scan_rules.json`.
///
/// A finding's fingerprint is the first [`FINGERPRINT_HEX_LEN`] hex digits
/// of BLAKE3(`FINGERPRINT_PREFIX` NUL rule-id NUL value), where value is the
/// reported secret bytes (for `key_body` rules: the header, a newline and the
/// first line of the key body). It identifies the same secret wherever it
/// appears, and is what allowlist entries name.
#[derive(Debug, Clone, Deserialize)]
pub struct ScanRuleSet {
    pub version: u32,
    /// Inline marker suppressing every finding on its line.
    pub allow_marker: String,
    /// Only this many leading bytes of each line are scanned.
    pub max_scan_line_len: usize,
    /// Lower-case substrings marking a value as a documentation placeholder.
    pub placeholder_markers: Vec<String>,
    /// Rules in priority order (earlier rules win overlapping matches).
    pub rules: Vec<ScanRule>,
}

/// One rule of the shared set.
#[derive(Debug, Clone, Deserialize)]
pub struct ScanRule {
    /// Stable id reported in findings; several rules may share an id.
    pub id: String,
    pub description: String,
    /// Regex source (common subset of Rust `regex` and Go RE2), matched
    /// against bytes in multi-line mode.
    pub pattern: String,
    /// The rule runs only when the input contains one of these (ASCII
    /// case-insensitive). Every match of the pattern contains one of them.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Optimization hint: a matching line holds a run of at least this many
    /// bytes other than space, \t, \r, \n and \f (implied by the pattern;
    /// the Go scanner skips other lines). 0 = none.
    #[serde(default)]
    pub min_run: usize,
    /// Minimum Shannon entropy (bits per byte) of the value; 0 = none.
    #[serde(default)]
    pub min_entropy: f64,
    /// The value must contain an ASCII letter and an ASCII digit.
    #[serde(default)]
    pub needs_letter_and_digit: bool,
    /// Values containing a placeholder marker are not findings.
    #[serde(default)]
    pub skip_placeholders: bool,
    /// The value is a structural marker shown as is; the rest of the line is
    /// masked instead (private-key headers).
    #[serde(default)]
    pub mask_rest_of_line: bool,
    /// Extra validation of the bytes after the match: `"key_body"` requires a
    /// base64 key body to follow (next lines or after an escaped `\n`).
    #[serde(default)]
    pub validate_tail: Option<String>,
}

static RULE_SET: LazyLock<ScanRuleSet> = LazyLock::new(|| {
    serde_json::from_str(SCAN_RULES_JSON)
        .unwrap_or_else(|e| panic!("scan_rules.json is invalid: {e}"))
});

/// The shared rule set (parsed once).
pub fn scan_rule_set() -> &'static ScanRuleSet {
    &RULE_SET
}

/// The fingerprint of a finding of `rule` whose secret is `value`.
pub fn fingerprint(rule: &str, value: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(FINGERPRINT_PREFIX.as_bytes());
    h.update(&[0]);
    h.update(rule.as_bytes());
    h.update(&[0]);
    h.update(value);
    let mut hex = h.finalize().to_hex().to_string();
    hex.truncate(FINGERPRINT_HEX_LEN);
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_set_parses_and_is_well_formed() {
        let set = scan_rule_set();
        assert_eq!(set.version, 1);
        assert_eq!(set.allow_marker, "wt:allow-secret");
        assert_eq!(set.max_scan_line_len, 8192);
        assert!(set.rules.len() >= 15);
        for r in &set.rules {
            assert!(!r.id.is_empty() && !r.description.is_empty() && !r.pattern.is_empty());
            assert!(!r.keywords.is_empty(), "rule {} has no keywords", r.id);
            for k in &r.keywords {
                assert_eq!(k, &k.to_ascii_lowercase(), "keywords are lower case");
            }
            if let Some(v) = &r.validate_tail {
                assert_eq!(v, "key_body");
            }
        }
        let cases: serde_json::Value = serde_json::from_str(SCAN_RULE_CASES_JSON).expect("cases");
        assert!(cases["cases"].as_array().is_some_and(|c| c.len() >= 10));
    }

    #[test]
    fn fingerprints_are_stable_and_short() {
        let a = fingerprint("github-token", b"value");
        assert_eq!(a.len(), FINGERPRINT_HEX_LEN);
        assert_eq!(a, fingerprint("github-token", b"value"));
        assert_ne!(a, fingerprint("jwt", b"value"));
        assert_ne!(a, fingerprint("github-token", b"other"));
    }
}
