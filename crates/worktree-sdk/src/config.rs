//! `.wt/config.toml` and the per-user `$WT_HOME` directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, SdkError};

/// Per-user state directory: `$WT_HOME` if set, else `~/.wt`.
/// Holds `credentials.toml`, `bg.json`, `bg.toml`, `bg.log`.
pub fn wt_home() -> PathBuf {
    if let Some(p) = std::env::var_os("WT_HOME") {
        return PathBuf::from(p);
    }
    dirs_home().join(".wt")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeConfig {
    pub worktree: WorktreeSection,
    #[serde(default)]
    pub user: UserSection,
    #[serde(default)]
    pub auto_snapshot: AutoSnapshotSection,
    #[serde(default)]
    pub sync: SyncSection,
    /// Named remotes, e.g. `[remote.origin]`.
    #[serde(default)]
    pub remote: std::collections::BTreeMap<String, RemoteSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSection {
    pub name: String,
    pub id: String,
    pub created_at: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
}

fn default_branch() -> String {
    "main".into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSection {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoSnapshotSection {
    pub enabled: bool,
    /// Snapshot after this many seconds without file events.
    pub inactivity_secs: u64,
    /// Snapshot immediately once this many files have changed.
    pub max_changed_files: usize,
}

impl Default for AutoSnapshotSection {
    fn default() -> Self {
        Self {
            enabled: true,
            inactivity_secs: 30,
            max_changed_files: 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSection {
    /// Upload auto-snapshots as staged (team-visible) snapshots.
    pub auto_stage: bool,
    /// Remote used by the daemon.
    pub remote: String,
}

impl Default for SyncSection {
    fn default() -> Self {
        Self {
            auto_stage: true,
            remote: "origin".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSection {
    /// Server base URL, e.g. `http://localhost:7420` (gRPC + REST share it).
    pub url: String,
    /// Tenant slug on the server.
    pub tenant: String,
}

/// Scalar keys understood by [`WorktreeConfig::get`] / [`WorktreeConfig::set`]
/// (besides `remote.<name>.url` and `remote.<name>.tenant`).
const SCALAR_KEYS: &[&str] = &[
    "auto_snapshot.enabled",
    "auto_snapshot.inactivity_secs",
    "auto_snapshot.max_changed_files",
    "sync.auto_stage",
    "sync.remote",
    "user.email",
    "user.name",
    "worktree.created_at",
    "worktree.default_branch",
    "worktree.id",
    "worktree.name",
];

impl WorktreeConfig {
    /// A fresh configuration for a new worktree.
    pub fn new(name: impl Into<String>, default_branch: impl Into<String>) -> Self {
        Self {
            worktree: WorktreeSection {
                name: name.into(),
                id: uuid::Uuid::new_v4().to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                default_branch: default_branch.into(),
            },
            user: UserSection::default(),
            auto_snapshot: AutoSnapshotSection::default(),
            sync: SyncSection::default(),
            remote: std::collections::BTreeMap::new(),
        }
    }

    /// Read and parse a `config.toml`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text)
            .map_err(|e| SdkError::InvalidConfig(format!("{}: {}", path.display(), e.message())))
    }

    /// Write the config atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        crate::repo::fsutil::write_atomic(path, text.as_bytes())
    }

    /// Get a dotted key ("user.name", "remote.origin.url", ...).
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some((remote, field)) = parse_remote_key(key) {
            let r = self.remote.get(remote)?;
            return match field {
                RemoteField::Url => Some(r.url.clone()),
                RemoteField::Tenant => Some(r.tenant.clone()),
            };
        }
        let value = match key {
            "worktree.name" => self.worktree.name.clone(),
            "worktree.id" => self.worktree.id.clone(),
            "worktree.created_at" => self.worktree.created_at.clone(),
            "worktree.default_branch" => self.worktree.default_branch.clone(),
            "user.name" => self.user.name.clone(),
            "user.email" => self.user.email.clone(),
            "auto_snapshot.enabled" => self.auto_snapshot.enabled.to_string(),
            "auto_snapshot.inactivity_secs" => self.auto_snapshot.inactivity_secs.to_string(),
            "auto_snapshot.max_changed_files" => self.auto_snapshot.max_changed_files.to_string(),
            "sync.auto_stage" => self.sync.auto_stage.to_string(),
            "sync.remote" => self.sync.remote.clone(),
            _ => return None,
        };
        Some(value)
    }

    /// Set a dotted key; `InvalidConfig` for unknown keys or bad values.
    /// Setting `remote.<name>.url` or `.tenant` creates the remote if needed.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if let Some((remote, field)) = parse_remote_key(key) {
            crate::repo::names::validate_remote_name(remote)
                .map_err(|e| SdkError::InvalidConfig(e.to_string()))?;
            if field == RemoteField::Url && !is_http_url(value) {
                return Err(SdkError::InvalidConfig(format!(
                    "{key}: expected an http:// or https:// URL, got {value:?}"
                )));
            }
            let entry = self
                .remote
                .entry(remote.to_string())
                .or_insert_with(|| RemoteSection {
                    url: String::new(),
                    tenant: String::new(),
                });
            match field {
                RemoteField::Url => entry.url = value.trim_end_matches('/').to_string(),
                RemoteField::Tenant => entry.tenant = value.to_string(),
            }
            return Ok(());
        }
        match key {
            "worktree.name" => {
                crate::repo::names::validate_tree_name(value)
                    .map_err(|e| SdkError::InvalidConfig(e.to_string()))?;
                self.worktree.name = value.to_string();
            }
            "worktree.id" | "worktree.created_at" => {
                return Err(SdkError::InvalidConfig(format!("{key} is read-only")));
            }
            "worktree.default_branch" => {
                crate::repo::names::validate_ref_name("branch", value)
                    .map_err(|e| SdkError::InvalidConfig(e.to_string()))?;
                self.worktree.default_branch = value.to_string();
            }
            "user.name" => self.user.name = value.trim().to_string(),
            "user.email" => self.user.email = value.trim().to_string(),
            "auto_snapshot.enabled" => self.auto_snapshot.enabled = parse_bool(key, value)?,
            "auto_snapshot.inactivity_secs" => {
                self.auto_snapshot.inactivity_secs = parse_num(key, value)?;
            }
            "auto_snapshot.max_changed_files" => {
                self.auto_snapshot.max_changed_files = parse_num(key, value)?;
            }
            "sync.auto_stage" => self.sync.auto_stage = parse_bool(key, value)?,
            "sync.remote" => {
                crate::repo::names::validate_remote_name(value)
                    .map_err(|e| SdkError::InvalidConfig(e.to_string()))?;
                self.sync.remote = value.to_string();
            }
            _ => {
                return Err(SdkError::InvalidConfig(format!(
                    "unknown config key: {key}"
                )))
            }
        }
        Ok(())
    }

    /// All keys as (dotted key, value), sorted.
    pub fn entries(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = SCALAR_KEYS
            .iter()
            .filter_map(|k| self.get(k).map(|v| (k.to_string(), v)))
            .collect();
        for (name, r) in &self.remote {
            out.push((format!("remote.{name}.tenant"), r.tenant.clone()));
            out.push((format!("remote.{name}.url"), r.url.clone()));
        }
        out.sort();
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteField {
    Url,
    Tenant,
}

/// Split `remote.<name>.url|tenant` into (name, field).
fn parse_remote_key(key: &str) -> Option<(&str, RemoteField)> {
    let rest = key.strip_prefix("remote.")?;
    let (name, field) = rest.rsplit_once('.')?;
    let field = match field {
        "url" => RemoteField::Url,
        "tenant" => RemoteField::Tenant,
        _ => return None,
    };
    Some((name, field))
}

fn is_http_url(value: &str) -> bool {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"));
    matches!(rest, Some(host) if !host.is_empty())
}

fn parse_bool(key: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(SdkError::InvalidConfig(format!(
            "{key}: expected a boolean, got {value:?}"
        ))),
    }
}

fn parse_num<T: std::str::FromStr>(key: &str, value: &str) -> Result<T> {
    value.trim().parse().map_err(|_| {
        SdkError::InvalidConfig(format!(
            "{key}: expected a non-negative integer, got {value:?}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WorktreeConfig {
        WorktreeConfig::new("demo", "main")
    }

    #[test]
    fn get_known_keys() {
        let c = cfg();
        assert_eq!(c.get("worktree.name").as_deref(), Some("demo"));
        assert_eq!(c.get("worktree.default_branch").as_deref(), Some("main"));
        assert_eq!(c.get("auto_snapshot.enabled").as_deref(), Some("true"));
        assert_eq!(c.get("sync.remote").as_deref(), Some("origin"));
        assert_eq!(c.get("nope"), None);
        assert_eq!(c.get("remote.origin.url"), None);
    }

    #[test]
    fn set_and_validate() {
        let mut c = cfg();
        c.set("user.name", "Ada").unwrap();
        c.set("user.email", "ada@example.com").unwrap();
        c.set("auto_snapshot.enabled", "off").unwrap();
        c.set("auto_snapshot.inactivity_secs", "5").unwrap();
        c.set("auto_snapshot.max_changed_files", "7").unwrap();
        c.set("sync.auto_stage", "false").unwrap();
        c.set("worktree.default_branch", "trunk").unwrap();
        assert_eq!(c.user.name, "Ada");
        assert!(!c.auto_snapshot.enabled);
        assert_eq!(c.auto_snapshot.inactivity_secs, 5);
        assert_eq!(c.auto_snapshot.max_changed_files, 7);
        assert!(!c.sync.auto_stage);
        assert_eq!(c.worktree.default_branch, "trunk");

        assert!(matches!(
            c.set("bogus.key", "x"),
            Err(SdkError::InvalidConfig(_))
        ));
        assert!(matches!(
            c.set("auto_snapshot.enabled", "maybe"),
            Err(SdkError::InvalidConfig(_))
        ));
        assert!(matches!(
            c.set("auto_snapshot.inactivity_secs", "-1"),
            Err(SdkError::InvalidConfig(_))
        ));
        assert!(matches!(
            c.set("worktree.id", "x"),
            Err(SdkError::InvalidConfig(_))
        ));
        assert!(matches!(
            c.set("worktree.default_branch", "a b"),
            Err(SdkError::InvalidConfig(_))
        ));
    }

    #[test]
    fn remotes() {
        let mut c = cfg();
        c.set("remote.origin.url", "http://localhost:7420/")
            .unwrap();
        c.set("remote.origin.tenant", "acme").unwrap();
        assert_eq!(
            c.get("remote.origin.url").as_deref(),
            Some("http://localhost:7420")
        );
        assert_eq!(c.remote["origin"].tenant, "acme");
        assert!(c.set("remote.origin.url", "ftp://x").is_err());
        assert!(c.set("remote.bad.name.url", "http://x").is_err());
        assert!(c.set("remote.origin.other", "x").is_err());
    }

    #[test]
    fn entries_sorted_and_complete() {
        let mut c = cfg();
        c.set("remote.origin.url", "https://wt.example.com")
            .unwrap();
        let entries = c.entries();
        let keys: Vec<_> = entries.iter().map(|(k, _)| k.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(keys.contains(&"remote.origin.url"));
        assert!(keys.contains(&"remote.origin.tenant"));
        assert!(keys.contains(&"user.name"));
        assert_eq!(entries.len(), SCALAR_KEYS.len() + 2);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = cfg();
        c.set("remote.origin.url", "http://localhost:7420").unwrap();
        c.set("remote.origin.tenant", "t").unwrap();
        c.save(&path).unwrap();
        assert_eq!(WorktreeConfig::load(&path).unwrap(), c);
        std::fs::write(&path, "not = [valid").unwrap();
        assert!(matches!(
            WorktreeConfig::load(&path),
            Err(SdkError::InvalidConfig(_))
        ));
    }

    #[test]
    fn wt_home_honors_env() {
        // Only reads the variable; the test binary never mutates WT_HOME here.
        if let Some(home) = std::env::var_os("WT_HOME") {
            assert_eq!(wt_home(), PathBuf::from(home));
        } else {
            assert!(wt_home().ends_with(".wt"));
        }
    }
}
