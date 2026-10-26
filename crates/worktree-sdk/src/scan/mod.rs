//! Content scanning: secret detection (pre-snapshot), SPDX license
//! detection, and content-defined chunking for large files. Pure functions,
//! no I/O.
//!
//! The three entry points are deliberately independent of the rest of the
//! SDK so they can be reused by the CLI, the background daemon and server
//! tooling alike:
//!
//! - [`scan_secrets`] — rule-based secret detection with placeholder
//!   suppression, entropy gating and redacted excerpts ([`rules`] lists the
//!   built-in rule set).
//! - [`detect_license`] — `SPDX-License-Identifier:` headers and full-text
//!   fingerprints of common license bodies.
//! - [`chunk_boundaries`] — FastCDC content-defined chunking used to split
//!   large files into deduplicable chunks ([`chunk_boundaries_with`] takes
//!   explicit sizes).

mod cdc;
mod license;
mod secrets;

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

use crate::config::SecretScanningSection;

pub use cdc::{chunk_boundaries_with, AVG_CHUNK_SIZE, MAX_CHUNK_SIZE, MIN_CHUNK_SIZE};
pub use secrets::{rules, Rule, ALLOW_MARKER, MAX_SCAN_LINE_LEN};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFinding {
    /// Rule id, e.g. "aws-access-key", "github-token", "private-key".
    pub rule: String,
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// The matching line with the secret redacted (e.g. "AKIA****MPLE").
    pub excerpt: String,
    /// Stable id of the secret (24 hex digits; same algorithm as the server,
    /// see `worktree_protocol::scan_rules`): what server allowlist entries
    /// (`wt compliance allow`) name.
    #[serde(default)]
    pub fingerprint: String,
}

/// Scan one file's content. Skips binary content. Honors inline
/// `wt:allow-secret` markers on the same line.
///
/// Findings are ordered by position in the file. Each finding's excerpt is
/// the (trimmed, possibly windowed) source line with every detected secret
/// on that line redacted, so excerpts are safe to print and log.
pub fn scan_secrets(path: &str, data: &[u8]) -> Vec<SecretFinding> {
    secrets::scan(path, data)
}

/// Check that `regex` is usable as a custom secret rule
/// (`[secret_scanning] patterns`).
pub fn validate_secret_regex(regex: &str) -> Result<(), String> {
    secrets::compile_custom("check", regex).map(|_| ())
}

/// Report findings that do not block the snapshot
/// (`block_on_match = false`) on stderr and in the log. Excerpts are
/// already redacted.
pub fn warn_findings(findings: &[SecretFinding]) {
    for f in findings {
        tracing::warn!(path = %f.path, line = f.line, rule = %f.rule, "possible secret (not blocking)");
        eprintln!(
            "warning: possible secret {}:{} [{}] {}",
            f.path, f.line, f.rule, f.excerpt
        );
    }
}

/// Secret scanning as configured by a `[secret_scanning]` section: the
/// built-in rules plus custom `patterns`, skipping `exclude`d paths.
#[derive(Debug, Clone)]
pub struct SecretScanner {
    enabled: bool,
    block_on_match: bool,
    custom: Vec<secrets::CustomRule>,
    exclude: Option<Gitignore>,
}

impl Default for SecretScanner {
    /// Built-in rules only, enabled and blocking.
    fn default() -> Self {
        Self {
            enabled: true,
            block_on_match: true,
            custom: Vec::new(),
            exclude: None,
        }
    }
}

impl SecretScanner {
    /// Compile a configuration; the error names the offending pattern.
    pub fn new(config: &SecretScanningSection) -> Result<Self, String> {
        let custom = config
            .patterns
            .iter()
            .map(|p| {
                secrets::compile_custom(&p.name, &p.regex)
                    .map_err(|e| format!("secret_scanning.patterns.{}: {e}", p.name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let exclude = if config.exclude.is_empty() {
            None
        } else {
            let mut builder = GitignoreBuilder::new("");
            for glob in &config.exclude {
                builder
                    .add_line(None, glob)
                    .map_err(|e| format!("secret_scanning.exclude: {glob:?}: {e}"))?;
            }
            Some(
                builder
                    .build()
                    .map_err(|e| format!("secret_scanning.exclude: {e}"))?,
            )
        };
        Ok(Self {
            enabled: config.enabled,
            block_on_match: config.block_on_match,
            custom,
            exclude,
        })
    }

    /// Whether scanning runs at all (`enabled`).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether findings refuse the snapshot (`block_on_match`); otherwise
    /// they are only reported as warnings.
    pub fn blocks(&self) -> bool {
        self.block_on_match
    }

    /// Whether any of the given spellings of a path ('/'-separated, e.g.
    /// tree-relative and worktree-relative) matches an `exclude` glob.
    pub fn is_excluded(&self, paths: &[&str]) -> bool {
        let Some(exclude) = &self.exclude else {
            return false;
        };
        paths.iter().any(|p| {
            !p.is_empty()
                && exclude
                    .matched_path_or_any_parents(Path::new(p), false)
                    .is_ignore()
        })
    }

    /// Scan one file; empty when disabled. `path` is used for reporting and
    /// for the exclusion check.
    pub fn scan(&self, path: &str, data: &[u8]) -> Vec<SecretFinding> {
        if !self.enabled || self.is_excluded(&[path]) {
            return Vec::new();
        }
        secrets::scan_with(path, data, &self.custom)
    }
}

/// Detect an SPDX license id from a LICENSE file body or an
/// `SPDX-License-Identifier:` header. None if unknown.
///
/// A header in the first 20 lines wins and is returned verbatim (modulo
/// whitespace and operator case), so compound expressions such as
/// `"MIT OR Apache-2.0"` are preserved. Otherwise the text is normalized and
/// matched against fingerprints of well-known license bodies.
pub fn detect_license(data: &[u8]) -> Option<String> {
    license::detect(data)
}

/// Content-defined chunk boundaries (FastCDC, min 256 KiB / avg 1 MiB /
/// max 4 MiB). Returns (offset, len) pairs covering `data` exactly.
pub fn chunk_boundaries(data: &[u8]) -> Vec<(usize, usize)> {
    chunk_boundaries_with(data, MIN_CHUNK_SIZE, AVG_CHUNK_SIZE, MAX_CHUNK_SIZE)
}

/// Bytes sniffed for NUL to classify content as binary (same heuristic and
/// window as Git).
const BINARY_SNIFF_LEN: usize = 8000;

/// True when `data` looks like binary content (contains a NUL byte within
/// the first [`BINARY_SNIFF_LEN`] bytes).
fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_SNIFF_LEN)].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_sniff_detects_nul_in_window_only() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"plain text\n"));
        let mut late = vec![b'a'; BINARY_SNIFF_LEN];
        late.push(0);
        assert!(!is_binary(&late), "NUL beyond the sniff window is ignored");
    }

    fn aws_line() -> String {
        // Assembled at runtime so this file holds no literal key.
        format!("KEY={}{}\n", "AKIA", "Z4Q7RJ2KXW3MPLQ9")
    }

    #[test]
    fn scanner_honours_enabled_exclude_and_patterns() {
        let data = aws_line();
        let default = SecretScanner::default();
        assert!(default.enabled() && default.blocks());
        assert_eq!(default.scan("a.env", data.as_bytes()).len(), 1);

        let off = SecretScanner::new(&SecretScanningSection {
            enabled: false,
            ..SecretScanningSection::default()
        })
        .unwrap();
        assert!(off.scan("a.env", data.as_bytes()).is_empty());

        let cfg = SecretScanningSection {
            block_on_match: false,
            exclude: vec!["fixtures/".into(), "*.sample".into()],
            patterns: vec![crate::config::SecretPattern {
                name: "acme".into(),
                regex: r"ACME-[0-9]{6}".into(),
            }],
            ..SecretScanningSection::default()
        };
        let s = SecretScanner::new(&cfg).unwrap();
        assert!(!s.blocks());
        assert!(s.scan("fixtures/keys/a.env", data.as_bytes()).is_empty());
        assert!(s.scan("x/conf.sample", data.as_bytes()).is_empty());
        assert!(s.is_excluded(&["lib/a.rs", "svc/fixtures/a.rs"]));
        assert!(!s.is_excluded(&["lib/a.rs", ""]));
        let found = s.scan("src/a.rs", b"id = ACME-123456\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "acme");

        let bad = SecretScanningSection {
            patterns: vec![crate::config::SecretPattern {
                name: "broken".into(),
                regex: "(".into(),
            }],
            ..SecretScanningSection::default()
        };
        let err = SecretScanner::new(&bad).unwrap_err();
        assert!(err.contains("secret_scanning.patterns.broken"), "{err}");
        assert!(validate_secret_regex("a+").is_ok());
        assert!(validate_secret_regex("[").is_err());
    }

    #[test]
    fn default_chunking_uses_contract_sizes() {
        assert_eq!(MIN_CHUNK_SIZE, 256 * 1024);
        assert_eq!(AVG_CHUNK_SIZE, 1024 * 1024);
        assert_eq!(MAX_CHUNK_SIZE, 4 * 1024 * 1024);
        let data = vec![7u8; 1000];
        assert_eq!(chunk_boundaries(&data), vec![(0, 1000)]);
        assert!(chunk_boundaries(&[]).is_empty());
    }
}
