//! Content scanning: secret detection (pre-snapshot), SPDX license
//! detection, and content-defined chunking for large files. Pure functions,
//! no I/O.

use serde::{Deserialize, Serialize};

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
pub fn scan_secrets(path: &str, data: &[u8]) -> Vec<SecretFinding> {
    let _ = (path, data);
    todo!("sdk-scan: scan_secrets")
}

/// Detect an SPDX license id from a LICENSE file body or an
/// `SPDX-License-Identifier:` header. None if unknown.
pub fn detect_license(data: &[u8]) -> Option<String> {
    let _ = data;
    todo!("sdk-scan: detect_license")
}

/// Content-defined chunk boundaries (FastCDC, min 256 KiB / avg 1 MiB /
/// max 4 MiB). Returns (offset, len) pairs covering `data` exactly.
pub fn chunk_boundaries(data: &[u8]) -> Vec<(usize, usize)> {
    let _ = data;
    todo!("sdk-scan: chunk_boundaries")
}
