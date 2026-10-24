use serde::{Deserialize, Serialize};

/// License grant level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseGrantLevel {
    ReadOnly,
    Modify,
    Redistribute,
}

/// License category for classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LicenseCategory {
    Permissive,
    Copyleft,
    Attribution,
    Proprietary,
    PublicDomain,
}

/// SPDX license metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub spdx_id: String,
    pub category: LicenseCategory,
    pub name: String,
}

impl LicenseInfo {
    pub fn new(spdx_id: &str, category: LicenseCategory) -> Self {
        Self {
            spdx_id: spdx_id.to_string(),
            category,
            name: spdx_id.to_string(),
        }
    }

    pub fn is_proprietary(&self) -> bool {
        self.category == LicenseCategory::Proprietary
    }
    pub fn is_copyleft(&self) -> bool {
        self.category == LicenseCategory::Copyleft
    }
    pub fn is_permissive(&self) -> bool {
        self.category == LicenseCategory::Permissive
    }
}

/// A license grant for a specific tenant on a specific path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseGrant {
    pub path: String,
    pub tenant: String,
    pub level: LicenseGrantLevel,
}

/// License compliance engine
#[derive(Debug, Clone)]
pub struct LicenseEngine {
    default_license: Option<String>,
    path_licenses: Vec<(String, String)>, // (path_pattern, spdx_id)
    grants: Vec<LicenseGrant>,
    spdx_strict: bool,
}

/// Result of a license check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseCheckResult {
    Allowed,
    Denied {
        path: String,
        license: String,
        required_grant: LicenseGrantLevel,
        reason: String,
    },
}

/// Operations that require license checks
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseOperation {
    Read,
    Modify,
    Export,
    Fork,
    Archive,
    Sync,
    PublicBrowse,
    CrossTenantView,
}

impl LicenseEngine {
    pub fn new() -> Self {
        Self {
            default_license: None,
            path_licenses: Vec::new(),
            grants: Vec::new(),
            spdx_strict: false,
        }
    }

    pub fn set_default(&mut self, spdx_id: &str) {
        self.default_license = Some(spdx_id.to_string());
    }

    pub fn add_path_license(&mut self, path: &str, spdx_id: &str) {
        self.path_licenses
            .push((path.to_string(), spdx_id.to_string()));
    }

    pub fn add_grant(&mut self, grant: LicenseGrant) {
        self.grants.push(grant);
    }

    pub fn set_spdx_strict(&mut self, strict: bool) {
        self.spdx_strict = strict;
    }

    /// Get the license for a specific path.
    ///
    /// Path patterns are directory prefixes (`src/enterprise/`, optionally
    /// followed by `*`/`**`, which are equivalent); `*`, `**` or an empty
    /// pattern match every path. The most specific (longest prefix) match
    /// wins; on a tie the first-added pattern wins.
    pub fn license_for_path(&self, path: &str) -> Option<&str> {
        let mut best: Option<(usize, &str)> = None;
        for (pat, lic) in &self.path_licenses {
            if let Some(len) = prefix_match(pat, path) {
                if best.is_none_or(|(best_len, _)| len > best_len) {
                    best = Some((len, lic.as_str()));
                }
            }
        }
        best.map(|(_, lic)| lic).or(self.default_license.as_deref())
    }

    /// Check if an operation is allowed on a path for a tenant
    pub fn check(
        &self,
        path: &str,
        tenant: &str,
        operation: &LicenseOperation,
    ) -> LicenseCheckResult {
        let license = match self.license_for_path(path) {
            Some(l) => l.to_string(),
            None => return LicenseCheckResult::Allowed, // No license = no restriction
        };

        let category = categorize_license(&license);
        let required_grant = operation_to_grant(operation);

        // Permissive and public domain licenses don't need grants
        if category == LicenseCategory::Permissive || category == LicenseCategory::PublicDomain {
            return LicenseCheckResult::Allowed;
        }

        // Check if tenant has appropriate grant
        let has_grant = self.grants.iter().any(|g| {
            g.tenant == tenant && prefix_match(&g.path, path).is_some() && g.level >= required_grant
        });

        if has_grant {
            LicenseCheckResult::Allowed
        } else {
            LicenseCheckResult::Denied {
                path: path.to_string(),
                license,
                required_grant,
                reason: format!(
                    "Tenant '{}' lacks required grant for this operation",
                    tenant
                ),
            }
        }
    }
}

impl Default for LicenseEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Match a directory-prefix pattern against a `/`-separated path.
///
/// Returns the length of the matched prefix (0 for match-all patterns), or
/// `None`. A prefix only matches at a segment boundary, so `src` matches
/// `src` and `src/a.rs` but not `src2/a.rs`. Backslashes and a leading `/`
/// or `./` are normalised away on both sides.
fn prefix_match(pattern: &str, path: &str) -> Option<usize> {
    fn normalize(s: &str) -> String {
        let s = s.replace('\\', "/");
        let mut s = s.as_str();
        loop {
            if let Some(rest) = s.strip_prefix("./") {
                s = rest;
            } else if let Some(rest) = s.strip_prefix('/') {
                s = rest;
            } else {
                break s.to_string();
            }
        }
    }
    let pattern = normalize(pattern);
    let path = normalize(path);
    let prefix = pattern.trim_end_matches(['*', '/']);
    if prefix.is_empty() {
        return Some(0);
    }
    let boundary = path.len() == prefix.len() || path.as_bytes().get(prefix.len()) == Some(&b'/');
    (path.starts_with(prefix) && boundary).then_some(prefix.len())
}

fn operation_to_grant(op: &LicenseOperation) -> LicenseGrantLevel {
    match op {
        LicenseOperation::Read
        | LicenseOperation::PublicBrowse
        | LicenseOperation::CrossTenantView => LicenseGrantLevel::ReadOnly,
        LicenseOperation::Modify | LicenseOperation::Sync => LicenseGrantLevel::Modify,
        LicenseOperation::Export | LicenseOperation::Fork | LicenseOperation::Archive => {
            LicenseGrantLevel::Redistribute
        }
    }
}

fn categorize_license(spdx: &str) -> LicenseCategory {
    match spdx.to_uppercase().as_str() {
        "MIT" | "BSD-2-CLAUSE" | "BSD-3-CLAUSE" | "ISC" | "ZLIB" => LicenseCategory::Permissive,
        "GPL-2.0" | "GPL-2.0-ONLY" | "GPL-3.0" | "GPL-3.0-ONLY" | "AGPL-3.0" | "LGPL-2.1"
        | "LGPL-3.0" | "MPL-2.0" => LicenseCategory::Copyleft,
        "APACHE-2.0" => LicenseCategory::Attribution,
        "CC0-1.0" | "UNLICENSE" | "0BSD" => LicenseCategory::PublicDomain,
        _ if spdx.to_lowercase().contains("proprietary") => LicenseCategory::Proprietary,
        _ => LicenseCategory::Proprietary, // Default unknown to proprietary (safe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_for_path() {
        let mut engine = LicenseEngine::new();
        engine.set_default("MIT");
        engine.add_path_license("src/enterprise/", "Proprietary");
        engine.add_path_license("vendor/openssl/", "Apache-2.0");

        assert_eq!(engine.license_for_path("src/main.rs"), Some("MIT"));
        assert_eq!(
            engine.license_for_path("src/enterprise/billing.rs"),
            Some("Proprietary")
        );
        assert_eq!(
            engine.license_for_path("vendor/openssl/crypto.c"),
            Some("Apache-2.0")
        );
    }

    #[test]
    fn test_permissive_always_allowed() {
        let mut engine = LicenseEngine::new();
        engine.set_default("MIT");
        let result = engine.check("src/main.rs", "external-team", &LicenseOperation::Export);
        assert_eq!(result, LicenseCheckResult::Allowed);
    }

    #[test]
    fn test_proprietary_requires_grant() {
        let mut engine = LicenseEngine::new();
        engine.add_path_license("src/secret/", "Proprietary");
        let result = engine.check("src/secret/algo.rs", "partner", &LicenseOperation::Read);
        assert!(matches!(result, LicenseCheckResult::Denied { .. }));

        // Add grant
        engine.add_grant(LicenseGrant {
            path: "src/secret/".to_string(),
            tenant: "partner".to_string(),
            level: LicenseGrantLevel::ReadOnly,
        });
        let result = engine.check("src/secret/algo.rs", "partner", &LicenseOperation::Read);
        assert_eq!(result, LicenseCheckResult::Allowed);
    }

    #[test]
    fn test_grant_level_hierarchy() {
        let mut engine = LicenseEngine::new();
        engine.add_path_license("src/enterprise/", "Proprietary");
        engine.add_grant(LicenseGrant {
            path: "src/enterprise/".to_string(),
            tenant: "partner".to_string(),
            level: LicenseGrantLevel::Modify,
        });

        // Read (lower) should be allowed
        assert_eq!(
            engine.check("src/enterprise/a.rs", "partner", &LicenseOperation::Read),
            LicenseCheckResult::Allowed
        );
        // Modify (equal) should be allowed
        assert_eq!(
            engine.check("src/enterprise/a.rs", "partner", &LicenseOperation::Modify),
            LicenseCheckResult::Allowed
        );
        // Export (higher) should be denied
        assert!(matches!(
            engine.check("src/enterprise/a.rs", "partner", &LicenseOperation::Export),
            LicenseCheckResult::Denied { .. }
        ));
    }

    #[test]
    fn test_root_wildcard_matches_everything() {
        // Regression: "**" trimmed to an empty prefix and then required the
        // path to start with '/', so it never matched.
        let mut engine = LicenseEngine::new();
        engine.add_path_license("**", "Proprietary");
        assert_eq!(engine.license_for_path("src/a.rs"), Some("Proprietary"));

        let mut engine = LicenseEngine::new();
        engine.set_default("Proprietary");
        engine.add_grant(LicenseGrant {
            path: "**".into(),
            tenant: "p".into(),
            level: LicenseGrantLevel::ReadOnly,
        });
        assert_eq!(
            engine.check("a/b.rs", "p", &LicenseOperation::Read),
            LicenseCheckResult::Allowed
        );
    }

    #[test]
    fn test_most_specific_prefix_wins_not_longest_pattern() {
        // Regression: specificity was measured by raw pattern length, so
        // "vendor/**" (9 chars) beat the more specific "vendor/x" (8 chars).
        let mut engine = LicenseEngine::new();
        engine.add_path_license("vendor/**", "MIT");
        engine.add_path_license("vendor/x", "GPL-3.0");
        assert_eq!(engine.license_for_path("vendor/x/a.c"), Some("GPL-3.0"));
        assert_eq!(engine.license_for_path("vendor/y/a.c"), Some("MIT"));
    }

    #[test]
    fn test_prefix_respects_segment_boundaries() {
        let mut engine = LicenseEngine::new();
        engine.add_path_license("src", "Proprietary");
        assert_eq!(engine.license_for_path("src"), Some("Proprietary"));
        assert_eq!(engine.license_for_path("src/a.rs"), Some("Proprietary"));
        assert_eq!(engine.license_for_path("src2/a.rs"), None);
        assert_eq!(engine.license_for_path("src\\a.rs"), Some("Proprietary"));
        assert_eq!(engine.license_for_path("./src/a.rs"), Some("Proprietary"));
    }
}
