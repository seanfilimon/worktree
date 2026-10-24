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

use serde::{Deserialize, Serialize};

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
