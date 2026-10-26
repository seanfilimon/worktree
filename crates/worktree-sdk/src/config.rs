//! Configuration files and the per-user `$WT_HOME` directory.
//!
//! Configuration is layered (docs/ENGINEERING_PLAN.md §13.9), lowest
//! precedence first:
//!
//! 1. `<wt home>/config.toml` — user defaults; only `[user]`, `[signing]`,
//!    `[secret_scanning]` and `[alias]` are read from it.
//! 2. `.wt/config.toml` — the worktree configuration ([`WorktreeConfig`]).
//! 3. `<tree>/.wt-tree/config.toml` — per-tree overrides; only
//!    `[auto_snapshot]`, `[secret_scanning]` and `[hooks]` are read from it.
//!    For a nested tree the files of its enclosing trees apply first.
//!
//! A value in a higher layer replaces the same key of a lower layer, except
//! `secret_scanning.patterns` and `secret_scanning.exclude`, which are
//! concatenated across layers. Empty `user.name`, `user.email` and
//! `signing.key` strings count as unset. Keys a layer is not allowed to set
//! are ignored there (and left in the file).
//!
//! Unknown keys are preserved whenever W0rkTree rewrites a config file.
//! `auto_snapshot.inactivity_timeout_secs` is accepted as an alias of
//! `auto_snapshot.inactivity_secs`.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use toml::{Table, Value};

use crate::error::{Result, SdkError};
use crate::repo::{fsutil, Repository, TREE_MARKER_DIR};

/// File name of every configuration file (user, worktree and tree level).
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Per-user state directory: `$WT_HOME` if set, else `~/.wt`.
/// Holds `credentials.toml`, `bg.json`, `bg.toml`, `bg.log` and the user
/// configuration `config.toml`.
pub fn wt_home() -> PathBuf {
    if let Some(p) = std::env::var_os("WT_HOME") {
        return PathBuf::from(p);
    }
    dirs_home().join(".wt")
}

/// `<wt home>/config.toml`.
pub fn user_config_path() -> PathBuf {
    wt_home().join(CONFIG_FILE_NAME)
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
    /// v0.5 (ENGINEERING_PLAN §13.9): secret scanning rules.
    #[serde(default, skip_serializing_if = "SecretScanningSection::is_default")]
    pub secret_scanning: SecretScanningSection,
    /// v0.5 (§13.9): snapshot hooks.
    #[serde(default, skip_serializing_if = "HooksSection::is_default")]
    pub hooks: HooksSection,
    /// v0.5 (§13.6): snapshot signing.
    #[serde(default, skip_serializing_if = "SigningSection::is_default")]
    pub signing: SigningSection,
    // --- git-export (ENGINEERING_PLAN §13.8): `wt git remote add|list|remove`.
    /// Git remotes for `wt git push`, e.g. `[git_remote.github] url = "..."`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub git_remote: std::collections::BTreeMap<String, GitRemoteSection>,
    // --- end git-export
    // --- cli-ux (ENGINEERING_PLAN §14.8): command aliases.
    /// `[alias]`: `wt NAME args...` runs the aliased command line, e.g.
    /// `st = "status"`, `lg = "log --oneline -n 20"` (user and worktree
    /// layers; a worktree alias replaces a user alias of the same name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub alias: std::collections::BTreeMap<String, String>,
    // --- end cli-ux
}

// --- git-export (ENGINEERING_PLAN §13.8)
/// `[git_remote.NAME]`: a Git remote (URL or local path) for `wt git push`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRemoteSection {
    pub url: String,
}
// --- end git-export

/// `[secret_scanning]`: extra patterns and exclusions for the pre-snapshot scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretScanningSection {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Refuse the snapshot on a finding (false = warn only).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub block_on_match: bool,
    /// Extra rules: `{ name = "...", regex = "..." }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<SecretPattern>,
    /// Paths never scanned (gitignore-style globs, matched against both the
    /// tree-relative and the worktree-relative path).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl Default for SecretScanningSection {
    fn default() -> Self {
        Self {
            enabled: true,
            block_on_match: true,
            patterns: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

impl SecretScanningSection {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretPattern {
    pub name: String,
    pub regex: String,
}

/// `[hooks]`: snapshot hook settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksSection {
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Kill a hook after this many seconds (0 = no limit).
    #[serde(
        default = "default_hook_timeout",
        skip_serializing_if = "is_default_hook_timeout"
    )]
    pub timeout_secs: u64,
}

impl Default for HooksSection {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_secs: default_hook_timeout(),
        }
    }
}

impl HooksSection {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_hook_timeout() -> u64 {
    60
}

fn is_default_hook_timeout(v: &u64) -> bool {
    *v == default_hook_timeout()
}

/// `[signing]`: sign new snapshots with a local Ed25519 key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningSection {
    /// Key name under `<wt home>/keys/` ("" = signing disabled).
    #[serde(default)]
    pub key: String,
    /// Sign every snapshot automatically (else only `wt snapshot --sign`).
    #[serde(default)]
    pub auto: bool,
}

impl SigningSection {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
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

/// What the daemon does when a branch switch finds uncommitted changes
/// (`auto_snapshot.on_branch_switch`). In TOML also accepts a boolean
/// (`true` = `snapshot`, `false` = `refuse`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OnBranchSwitch {
    /// Snapshot the dirty tree before switching.
    Snapshot,
    /// Set the changes aside and restore them when switching back.
    Stash,
    /// Refuse the switch (the default; `wt switch --force` overrides).
    #[default]
    Refuse,
}

impl OnBranchSwitch {
    pub fn as_str(self) -> &'static str {
        match self {
            OnBranchSwitch::Snapshot => "snapshot",
            OnBranchSwitch::Stash => "stash",
            OnBranchSwitch::Refuse => "refuse",
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl std::str::FromStr for OnBranchSwitch {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "snapshot" | "true" => Ok(OnBranchSwitch::Snapshot),
            "stash" => Ok(OnBranchSwitch::Stash),
            "refuse" | "false" => Ok(OnBranchSwitch::Refuse),
            other => Err(format!("expected snapshot, stash or refuse, got {other:?}")),
        }
    }
}

impl<'de> Deserialize<'de> for OnBranchSwitch {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(OnBranchSwitch::Snapshot),
            Raw::Bool(false) => Ok(OnBranchSwitch::Refuse),
            Raw::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// Default `auto_snapshot.message_format`.
pub const DEFAULT_AUTO_MESSAGE_FORMAT: &str = "auto: {count} files changed";

/// Paths listed by the `{files}` placeholder before it is abbreviated.
const MAX_LISTED_FILES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoSnapshotSection {
    pub enabled: bool,
    /// Snapshot after this many seconds without file events
    /// (alias `inactivity_timeout_secs`).
    #[serde(alias = "inactivity_timeout_secs")]
    pub inactivity_secs: u64,
    /// Snapshot immediately once this many files have changed (0 = no limit).
    pub max_changed_files: usize,
    /// Snapshot immediately once the changed files total this many bytes
    /// (0 = no limit). TOML also accepts sizes such as `"50MB"` / `"1GiB"`.
    #[serde(deserialize_with = "deserialize_size", skip_serializing_if = "is_zero")]
    pub max_changed_bytes: u64,
    /// Message of auto-snapshots; placeholders `{count}` (changed files),
    /// `{files}` (their paths), `{branch}` and `{tree}`.
    #[serde(skip_serializing_if = "is_default_message_format")]
    pub message_format: String,
    /// Behaviour before a branch switch with uncommitted changes.
    #[serde(skip_serializing_if = "OnBranchSwitch::is_default")]
    pub on_branch_switch: OnBranchSwitch,
}

impl Default for AutoSnapshotSection {
    fn default() -> Self {
        Self {
            enabled: true,
            inactivity_secs: 30,
            max_changed_files: 50,
            max_changed_bytes: 0,
            message_format: DEFAULT_AUTO_MESSAGE_FORMAT.to_string(),
            on_branch_switch: OnBranchSwitch::default(),
        }
    }
}

impl AutoSnapshotSection {
    /// Render `message_format` for an auto-snapshot of `tree` on `branch`
    /// with the given changed paths. An empty format falls back to the
    /// default.
    pub fn format_message(&self, files: &[String], branch: &str, tree: &str) -> String {
        let format = if self.message_format.trim().is_empty() {
            DEFAULT_AUTO_MESSAGE_FORMAT
        } else {
            self.message_format.as_str()
        };
        let count = files.len().to_string();
        let mut listed = files
            .iter()
            .take(MAX_LISTED_FILES)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if files.len() > MAX_LISTED_FILES {
            listed.push_str(&format!(" (+{} more)", files.len() - MAX_LISTED_FILES));
        }
        format
            .replace("{count}", &count)
            .replace("{file_count}", &count)
            .replace("{files}", &listed)
            .replace("{branch}", branch)
            .replace("{tree}", tree)
    }
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

fn is_default_message_format(v: &String) -> bool {
    v == DEFAULT_AUTO_MESSAGE_FORMAT
}

fn deserialize_size<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(u64),
        Text(String),
    }
    match Raw::deserialize(d)? {
        Raw::Number(n) => Ok(n),
        Raw::Text(s) => parse_size(&s).map_err(serde::de::Error::custom),
    }
}

/// Parse a byte size: a plain integer or a number with a unit suffix
/// (`B`, `KB`/`MB`/`GB` = powers of 1000, `KiB`/`MiB`/`GiB` = powers of 1024).
pub fn parse_size(text: &str) -> std::result::Result<u64, String> {
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u64 = num
        .parse()
        .map_err(|_| format!("expected a size such as 1048576 or \"50MB\", got {text:?}"))?;
    let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1_000,
        "m" | "mb" => 1_000_000,
        "g" | "gb" => 1_000_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        _ => return Err(format!("unknown size unit in {text:?}")),
    };
    n.checked_mul(mult)
        .ok_or_else(|| format!("size {text:?} is too large"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSection {
    /// Upload auto-snapshots as staged (team-visible) snapshots.
    pub auto_stage: bool,
    /// Remote used by the daemon.
    pub remote: String,
    /// v0.5 (§13.11): the daemon fetches remote branches (WatchBranches
    /// stream, with a periodic fallback every `interval_secs`).
    #[serde(default = "yes")]
    pub auto_fetch: bool,
    #[serde(default = "default_sync_interval")]
    pub interval_secs: u64,
    // ---- sync workstream (§15.5): one-step sync and the offline queue ----
    /// How `wt sync` integrates the remote branch (`--merge` / `--rebase`
    /// override it).
    #[serde(default, skip_serializing_if = "SyncStrategy::is_default")]
    pub strategy: SyncStrategy,
    /// The daemon retries queued pushes (`.wt/trees/<tree>/outbox.jsonl`)
    /// with backoff until the server accepts them.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub auto_flush: bool,
    /// The daemon runs a full `wt sync` of every clean tree this often
    /// (0 = off).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub auto_sync_secs: u64,
    // ---- end sync workstream ----
}

fn default_sync_interval() -> u64 {
    300
}

impl Default for SyncSection {
    fn default() -> Self {
        Self {
            auto_stage: true,
            remote: "origin".into(),
            auto_fetch: true,
            interval_secs: default_sync_interval(),
            strategy: SyncStrategy::default(),
            auto_flush: true,
            auto_sync_secs: 0,
        }
    }
}

// ---- sync workstream (§15.5) ----
/// `sync.strategy`: how `wt sync` integrates the remote branch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStrategy {
    /// Three-way merge (a merge snapshot when both sides changed).
    #[default]
    Merge,
    /// Replay the local snapshots on top of the remote branch.
    Rebase,
}

impl SyncStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncStrategy::Merge => "merge",
            SyncStrategy::Rebase => "rebase",
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl fmt::Display for SyncStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl std::str::FromStr for SyncStrategy {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "merge" => Ok(SyncStrategy::Merge),
            "rebase" => Ok(SyncStrategy::Rebase),
            other => Err(format!("expected merge or rebase, got {other:?}")),
        }
    }
}
// ---- end sync workstream ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSection {
    /// Server base URL, e.g. `http://localhost:7420` (gRPC + REST share it).
    pub url: String,
    /// Tenant slug on the server.
    pub tenant: String,
}

/// Value type of a configuration key (drives parsing of `wt config set`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Str,
    Bool,
    UInt,
    Size,
    /// Comma-separated list of strings.
    List,
}

/// Scalar keys understood by [`WorktreeConfig::get`] / [`WorktreeConfig::set`]
/// (besides `remote.<name>.url`, `remote.<name>.tenant` and
/// `secret_scanning.patterns.<name>`).
const SCALAR_KEYS: &[(&str, Kind)] = &[
    ("auto_snapshot.enabled", Kind::Bool),
    ("auto_snapshot.inactivity_secs", Kind::UInt),
    ("auto_snapshot.max_changed_bytes", Kind::Size),
    ("auto_snapshot.max_changed_files", Kind::UInt),
    ("auto_snapshot.message_format", Kind::Str),
    ("auto_snapshot.on_branch_switch", Kind::Str),
    ("hooks.enabled", Kind::Bool),
    ("hooks.timeout_secs", Kind::UInt),
    ("secret_scanning.block_on_match", Kind::Bool),
    ("secret_scanning.enabled", Kind::Bool),
    ("secret_scanning.exclude", Kind::List),
    ("signing.auto", Kind::Bool),
    ("signing.key", Kind::Str),
    ("sync.auto_fetch", Kind::Bool),
    ("sync.auto_flush", Kind::Bool), // sync workstream (§15.5)
    ("sync.auto_stage", Kind::Bool),
    ("sync.auto_sync_secs", Kind::UInt), // sync workstream (§15.5)
    ("sync.interval_secs", Kind::UInt),
    ("sync.remote", Kind::Str),
    ("sync.strategy", Kind::Str), // sync workstream (§15.5)
    ("user.email", Kind::Str),
    ("user.name", Kind::Str),
    ("worktree.created_at", Kind::Str),
    ("worktree.default_branch", Kind::Str),
    ("worktree.id", Kind::Str),
    ("worktree.name", Kind::Str),
];

/// Accepted alternative spellings: (section, alias, canonical field).
const ALIASES: &[(&str, &str, &str)] = &[(
    "auto_snapshot",
    "inactivity_timeout_secs",
    "inactivity_secs",
)];

/// Array keys that accumulate across layers instead of being replaced.
const ADDITIVE_KEYS: &[(&str, &str)] = &[
    ("secret_scanning", "patterns"),
    ("secret_scanning", "exclude"),
];

/// String keys for which an empty value means "not set".
const EMPTY_IS_UNSET: &[(&str, &str)] = &[("user", "name"), ("user", "email"), ("signing", "key")];

/// Sections read from the user config file (`alias`: cli-ux §14.8).
pub const USER_SECTIONS: &[&str] = &["user", "signing", "secret_scanning", "alias"];
/// Sections read from a tree's `.wt-tree/config.toml`.
pub const TREE_SECTIONS: &[&str] = &["auto_snapshot", "secret_scanning", "hooks"];

/// Preferred order of top-level sections when writing a config file.
const SECTION_ORDER: &[&str] = &[
    "worktree",
    "user",
    "auto_snapshot",
    "sync",
    "remote",
    "secret_scanning",
    "hooks",
    "signing",
    "alias", // cli-ux (§14.8)
];

const PATTERNS_PREFIX: &str = "secret_scanning.patterns.";
/// cli-ux (§14.8): prefix of `alias.NAME` keys.
const ALIAS_PREFIX: &str = "alias.";

/// The canonical spelling of a dotted key (resolves aliases).
pub fn canonical_key(key: &str) -> String {
    for (section, alias, canonical) in ALIASES {
        if key.strip_prefix(section).and_then(|r| r.strip_prefix('.')) == Some(alias) {
            return format!("{section}.{canonical}");
        }
    }
    key.to_string()
}

fn scalar_kind(key: &str) -> Option<Kind> {
    SCALAR_KEYS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, kind)| *kind)
}

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
            secret_scanning: SecretScanningSection::default(),
            hooks: HooksSection::default(),
            signing: SigningSection::default(),
            git_remote: std::collections::BTreeMap::new(), // git-export (§13.8)
            alias: std::collections::BTreeMap::new(),      // cli-ux (§14.8)
        }
    }

    /// Read and parse a `config.toml`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let table = parse_table(&text, path)?;
        Self::from_table(table)
            .map_err(|e| SdkError::InvalidConfig(format!("{}: {e}", path.display())))
    }

    /// Deserialize from a TOML table (aliases resolved first).
    fn from_table(mut table: Table) -> std::result::Result<Self, String> {
        normalize(&mut table);
        Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| e.message().to_string())
    }

    /// The config as a TOML table (default-valued optional keys omitted).
    fn to_table(&self) -> Result<Table> {
        match Value::try_from(self)? {
            Value::Table(t) => Ok(t),
            _ => Err(SdkError::Serialization("config is not a table".into())),
        }
    }

    /// Write the config atomically. Keys in the existing file that this
    /// version of W0rkTree does not know are preserved.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut table = self.to_table()?;
        if let Some(text) = fsutil::read_string_opt(path)? {
            if let Ok(mut raw) = text.parse::<Table>() {
                normalize(&mut raw);
                if let Ok(known) = Self::from_table(raw.clone())
                    .and_then(|c| c.to_table().map_err(|e| e.to_string()))
                {
                    let mut unknown = subtract(&raw, &known);
                    // Unknown keys of a remote that no longer exists would
                    // resurrect it without its required fields.
                    if let Some(Value::Table(remotes)) = unknown.get_mut("remote") {
                        remotes.retain(|name, _| self.remote.contains_key(name));
                    }
                    merge_missing(&mut table, unknown);
                }
            }
        }
        write_table(path, &table)
    }

    /// Get a dotted key ("user.name", "remote.origin.url", ...).
    pub fn get(&self, key: &str) -> Option<String> {
        let key = canonical_key(key);
        if let Some((remote, field)) = parse_remote_key(&key) {
            let r = self.remote.get(remote)?;
            return match field {
                RemoteField::Url => Some(r.url.clone()),
                RemoteField::Tenant => Some(r.tenant.clone()),
            };
        }
        if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
            return self
                .secret_scanning
                .patterns
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.regex.clone());
        }
        // cli-ux (§14.8)
        if let Some(name) = key.strip_prefix(ALIAS_PREFIX) {
            return self.alias.get(name).cloned();
        }
        let a = &self.auto_snapshot;
        let value = match key.as_str() {
            "worktree.name" => self.worktree.name.clone(),
            "worktree.id" => self.worktree.id.clone(),
            "worktree.created_at" => self.worktree.created_at.clone(),
            "worktree.default_branch" => self.worktree.default_branch.clone(),
            "user.name" => self.user.name.clone(),
            "user.email" => self.user.email.clone(),
            "auto_snapshot.enabled" => a.enabled.to_string(),
            "auto_snapshot.inactivity_secs" => a.inactivity_secs.to_string(),
            "auto_snapshot.max_changed_files" => a.max_changed_files.to_string(),
            "auto_snapshot.max_changed_bytes" => a.max_changed_bytes.to_string(),
            "auto_snapshot.message_format" => a.message_format.clone(),
            "auto_snapshot.on_branch_switch" => a.on_branch_switch.as_str().to_string(),
            "sync.auto_stage" => self.sync.auto_stage.to_string(),
            "sync.remote" => self.sync.remote.clone(),
            "sync.auto_fetch" => self.sync.auto_fetch.to_string(),
            "sync.interval_secs" => self.sync.interval_secs.to_string(),
            // sync workstream (§15.5)
            "sync.strategy" => self.sync.strategy.as_str().to_string(),
            "sync.auto_flush" => self.sync.auto_flush.to_string(),
            "sync.auto_sync_secs" => self.sync.auto_sync_secs.to_string(),
            "secret_scanning.enabled" => self.secret_scanning.enabled.to_string(),
            "secret_scanning.block_on_match" => self.secret_scanning.block_on_match.to_string(),
            "secret_scanning.exclude" => self.secret_scanning.exclude.join(","),
            "hooks.enabled" => self.hooks.enabled.to_string(),
            "hooks.timeout_secs" => self.hooks.timeout_secs.to_string(),
            "signing.key" => self.signing.key.clone(),
            "signing.auto" => self.signing.auto.to_string(),
            _ => return None,
        };
        Some(value)
    }

    /// Set a dotted key; `InvalidConfig` for unknown keys or bad values.
    /// Setting `remote.<name>.url` or `.tenant` creates the remote if needed.
    /// `secret_scanning.patterns.<name>` adds or replaces an extra secret
    /// rule (an empty value removes it); `secret_scanning.exclude` takes a
    /// comma-separated list.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let key = canonical_key(key);
        let key = key.as_str();
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
        if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
            validate_pattern_name(name)?;
            let patterns = &mut self.secret_scanning.patterns;
            if value.is_empty() {
                patterns.retain(|p| p.name != name);
                return Ok(());
            }
            crate::scan::validate_secret_regex(value)
                .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
            match patterns.iter_mut().find(|p| p.name == name) {
                Some(p) => p.regex = value.to_string(),
                None => patterns.push(SecretPattern {
                    name: name.to_string(),
                    regex: value.to_string(),
                }),
            }
            return Ok(());
        }
        // cli-ux (§14.8): `alias.NAME`; an empty value removes the alias.
        if let Some(name) = key.strip_prefix(ALIAS_PREFIX) {
            validate_alias_name(name)?;
            if value.trim().is_empty() {
                self.alias.remove(name);
                return Ok(());
            }
            split_alias_command(value)
                .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
            self.alias
                .insert(name.to_string(), value.trim().to_string());
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
            "auto_snapshot.max_changed_bytes" => {
                self.auto_snapshot.max_changed_bytes = parse_size(value)
                    .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
            }
            "auto_snapshot.message_format" => {
                self.auto_snapshot.message_format = value.to_string();
            }
            "auto_snapshot.on_branch_switch" => {
                self.auto_snapshot.on_branch_switch = value
                    .parse()
                    .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
            }
            "sync.auto_stage" => self.sync.auto_stage = parse_bool(key, value)?,
            "sync.auto_fetch" => self.sync.auto_fetch = parse_bool(key, value)?,
            "sync.interval_secs" => self.sync.interval_secs = parse_num(key, value)?,
            // sync workstream (§15.5)
            "sync.strategy" => {
                self.sync.strategy = value
                    .parse()
                    .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
            }
            "sync.auto_flush" => self.sync.auto_flush = parse_bool(key, value)?,
            "sync.auto_sync_secs" => self.sync.auto_sync_secs = parse_num(key, value)?,
            "sync.remote" => {
                crate::repo::names::validate_remote_name(value)
                    .map_err(|e| SdkError::InvalidConfig(e.to_string()))?;
                self.sync.remote = value.to_string();
            }
            "secret_scanning.enabled" => {
                self.secret_scanning.enabled = parse_bool(key, value)?;
            }
            "secret_scanning.block_on_match" => {
                self.secret_scanning.block_on_match = parse_bool(key, value)?;
            }
            "secret_scanning.exclude" => {
                let globs = split_list(value);
                for glob in &globs {
                    crate::repo::ignore_rules::validate_ignore_pattern(glob)
                        .map_err(|e| SdkError::InvalidConfig(format!("{key}: {e}")))?;
                }
                self.secret_scanning.exclude = globs;
            }
            "hooks.enabled" => self.hooks.enabled = parse_bool(key, value)?,
            "hooks.timeout_secs" => self.hooks.timeout_secs = parse_num(key, value)?,
            "signing.key" => self.signing.key = value.trim().to_string(),
            "signing.auto" => self.signing.auto = parse_bool(key, value)?,
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
            .filter_map(|(k, _)| self.get(k).map(|v| (k.to_string(), v)))
            .collect();
        for (name, r) in &self.remote {
            out.push((format!("remote.{name}.tenant"), r.tenant.clone()));
            out.push((format!("remote.{name}.url"), r.url.clone()));
        }
        for p in &self.secret_scanning.patterns {
            out.push((format!("{PATTERNS_PREFIX}{}", p.name), p.regex.clone()));
        }
        for (name, command) in &self.alias {
            out.push((format!("{ALIAS_PREFIX}{name}"), command.clone())); // cli-ux (§14.8)
        }
        out.sort();
        out
    }

    /// The TOML value stored for `key` in a config file (after `set`).
    fn toml_value(&self, key: &str) -> Option<Value> {
        if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
            let p = self
                .secret_scanning
                .patterns
                .iter()
                .find(|p| p.name == name)?;
            return Value::try_from(p).ok();
        }
        let text = self.get(key)?;
        let kind = if parse_remote_key(key).is_some() || key.starts_with(ALIAS_PREFIX) {
            Kind::Str
        } else {
            scalar_kind(key)?
        };
        Some(match kind {
            Kind::Str => Value::String(text),
            Kind::Bool => Value::Boolean(text == "true"),
            Kind::UInt | Kind::Size => Value::Integer(text.parse::<i64>().ok()?),
            Kind::List => Value::Array(split_list(&text).into_iter().map(Value::String).collect()),
        })
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn validate_pattern_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SdkError::InvalidConfig(format!(
            "secret pattern name {name:?} must be non-empty and use only letters, digits, '-' and '_'"
        )));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// TOML table helpers
// ----------------------------------------------------------------------

fn parse_table(text: &str, path: &Path) -> Result<Table> {
    text.parse::<Table>()
        .map_err(|e| SdkError::InvalidConfig(format!("{}: {}", path.display(), e.message())))
}

/// Read and parse a TOML file; `None` if it does not exist.
fn read_table_opt(path: &Path) -> Result<Option<Table>> {
    match fsutil::read_string_opt(path)? {
        Some(text) => parse_table(&text, path).map(Some),
        None => Ok(None),
    }
}

/// Resolve aliases in place (the canonical key wins if both are present).
fn normalize(table: &mut Table) {
    for (section, alias, canonical) in ALIASES {
        if let Some(Value::Table(sec)) = table.get_mut(*section) {
            if let Some(v) = sec.remove(*alias) {
                if !sec.contains_key(*canonical) {
                    sec.insert(canonical.to_string(), v);
                }
            }
        }
    }
}

/// Drop empty strings of keys where empty means "unset".
fn drop_unset(table: &mut Table) {
    for (section, key) in EMPTY_IS_UNSET {
        if let Some(Value::Table(sec)) = table.get_mut(*section) {
            if matches!(sec.get(*key), Some(Value::String(s)) if s.trim().is_empty()) {
                sec.remove(*key);
            }
        }
    }
}

/// Keys of `raw` that do not appear in `known` (recursively for tables).
fn subtract(raw: &Table, known: &Table) -> Table {
    let mut out = Table::new();
    for (k, v) in raw {
        match (v, known.get(k)) {
            (_, None) => {
                out.insert(k.clone(), v.clone());
            }
            (Value::Table(r), Some(Value::Table(kn))) => {
                let sub = subtract(r, kn);
                if !sub.is_empty() {
                    out.insert(k.clone(), Value::Table(sub));
                }
            }
            _ => {}
        }
    }
    out
}

/// Add keys of `extra` that `table` lacks (recursively for tables).
fn merge_missing(table: &mut Table, extra: Table) {
    for (k, v) in extra {
        match (table.get_mut(&k), v) {
            (None, v) => {
                table.insert(k, v);
            }
            (Some(Value::Table(t)), Value::Table(e)) => merge_missing(t, e),
            _ => {}
        }
    }
}

/// Overlay `top` onto `base` (higher layer wins; additive arrays concatenate).
fn overlay(base: &mut Table, top: &Table, section: Option<&str>) {
    for (k, v) in top {
        let additive = section.is_some_and(|s| ADDITIVE_KEYS.contains(&(s, k.as_str())));
        match (base.get_mut(k), v) {
            (Some(Value::Table(b)), Value::Table(t)) if section.is_none() => {
                overlay(b, t, Some(k.as_str()));
            }
            (Some(Value::Table(b)), Value::Table(t)) => overlay(b, t, section),
            (Some(Value::Array(b)), Value::Array(t)) if additive => {
                for item in t {
                    if !b.contains(item) {
                        b.push(item.clone());
                    }
                }
            }
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Look up a dotted key in a table.
fn lookup<'a>(table: &'a Table, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let mut cur = table.get(parts.next()?)?;
    for part in parts {
        cur = cur.as_table()?.get(part)?;
    }
    Some(cur)
}

/// Whether a layer table defines the (canonical) dotted key.
fn defines(table: &Table, key: &str) -> bool {
    if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
        return matches!(
            lookup(table, "secret_scanning.patterns"),
            Some(Value::Array(items)) if items.iter().any(|i| {
                i.get("name").and_then(Value::as_str) == Some(name)
            })
        );
    }
    lookup(table, key).is_some()
}

/// Store `value` at the dotted `key` (creating tables as needed).
fn insert_key(table: &mut Table, key: &str, value: Value) -> Result<()> {
    if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
        let section = section_mut(table, "secret_scanning")?;
        let entry = section
            .entry("patterns")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(items) = entry else {
            return Err(SdkError::InvalidConfig(
                "secret_scanning.patterns is not an array".into(),
            ));
        };
        match items
            .iter_mut()
            .find(|i| i.get("name").and_then(Value::as_str) == Some(name))
        {
            Some(item) => *item = value,
            None => items.push(value),
        }
        return Ok(());
    }
    let parts: Vec<&str> = key.split('.').collect();
    let (last, parents) = parts
        .split_last()
        .ok_or_else(|| SdkError::InvalidConfig("empty config key".into()))?;
    let mut cur = table;
    for part in parents {
        cur = section_mut(cur, part)?;
    }
    cur.insert(last.to_string(), value);
    Ok(())
}

fn section_mut<'a>(table: &'a mut Table, name: &str) -> Result<&'a mut Table> {
    let entry = table
        .entry(name.to_string())
        .or_insert_with(|| Value::Table(Table::new()));
    entry
        .as_table_mut()
        .ok_or_else(|| SdkError::InvalidConfig(format!("{name} is not a table")))
}

/// Remove the dotted `key` (and its aliases); whether anything was removed.
fn remove_key(table: &mut Table, key: &str) -> bool {
    if let Some(name) = key.strip_prefix(PATTERNS_PREFIX) {
        let Some(Value::Table(sec)) = table.get_mut("secret_scanning") else {
            return false;
        };
        let Some(Value::Array(items)) = sec.get_mut("patterns") else {
            return false;
        };
        let before = items.len();
        items.retain(|i| i.get("name").and_then(Value::as_str) != Some(name));
        return items.len() != before;
    }
    let mut removed = false;
    let mut spellings = vec![key.to_string()];
    for (section, alias, canonical) in ALIASES {
        if key == format!("{section}.{canonical}") {
            spellings.push(format!("{section}.{alias}"));
        }
    }
    for spelling in spellings {
        let parts: Vec<&str> = spelling.split('.').collect();
        let Some((last, parents)) = parts.split_last() else {
            continue;
        };
        let mut cur = Some(&mut *table);
        for part in parents {
            cur = cur
                .and_then(|t| t.get_mut(*part))
                .and_then(Value::as_table_mut);
        }
        if let Some(t) = cur {
            removed |= t.remove(*last).is_some();
        }
    }
    removed
}

/// Serialize a table with the well-known sections in a stable order.
fn table_to_string(table: &Table) -> Result<String> {
    let mut out = String::new();
    let mut plain = Table::new();
    for (k, v) in table {
        if !v.is_table() {
            plain.insert(k.clone(), v.clone());
        }
    }
    if !plain.is_empty() {
        out.push_str(&toml::to_string_pretty(&plain)?);
    }
    let mut keys: Vec<&String> = table
        .iter()
        .filter(|(_, v)| v.is_table())
        .map(|(k, _)| k)
        .collect();
    keys.sort_by_key(|k| {
        (
            SECTION_ORDER
                .iter()
                .position(|s| s == k)
                .unwrap_or(SECTION_ORDER.len()),
            k.to_string(),
        )
    });
    for k in keys {
        let mut one = Table::new();
        one.insert(k.clone(), table[k.as_str()].clone());
        let text = toml::to_string_pretty(&one)?;
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&text);
    }
    Ok(out)
}

fn write_table(path: &Path, table: &Table) -> Result<()> {
    let text = table_to_string(table)?;
    fsutil::write_atomic(path, text.as_bytes())
}

// ----------------------------------------------------------------------
// Layers and origins
// ----------------------------------------------------------------------

/// Which configuration file a value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigLevel {
    /// Built-in default (no file sets the key).
    Default,
    /// `<wt home>/config.toml`.
    User,
    /// `.wt/config.toml`.
    Worktree,
    /// `<tree>/.wt-tree/config.toml`.
    Tree,
}

/// Where an effective configuration value was set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigOrigin {
    pub level: ConfigLevel,
    /// Tree whose `.wt-tree/config.toml` set the value (level `tree`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
    /// The file (absent for defaults).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl ConfigOrigin {
    fn default_origin() -> Self {
        Self {
            level: ConfigLevel::Default,
            tree: None,
            path: None,
        }
    }
}

impl fmt::Display for ConfigOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        match (self.level, &self.tree) {
            (ConfigLevel::Default, _) => f.write_str("default"),
            (ConfigLevel::User, _) => write!(f, "user:{path}"),
            (ConfigLevel::Worktree, _) => write!(f, "worktree:{path}"),
            (ConfigLevel::Tree, Some(tree)) => write!(f, "tree {tree}:{path}"),
            (ConfigLevel::Tree, None) => write!(f, "tree:{path}"),
        }
    }
}

/// One effective configuration value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub origin: ConfigOrigin,
}

/// Which file `wt config set` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigScope {
    /// `<wt home>/config.toml` (`--global`).
    User,
    /// `.wt/config.toml` (the default).
    Worktree,
    /// `<tree>/.wt-tree/config.toml` (`--tree NAME`).
    Tree(String),
}

#[derive(Debug, Clone)]
struct ConfigLayer {
    origin: ConfigOrigin,
    table: Table,
}

/// The configuration layers that apply to one tree (or to the worktree as
/// a whole), lowest precedence first.
#[derive(Debug, Clone)]
pub struct ConfigStack {
    layers: Vec<ConfigLayer>,
}

impl ConfigStack {
    /// Merge every layer into the effective configuration.
    pub fn effective(&self) -> Result<WorktreeConfig> {
        let mut merged = Table::new();
        for layer in &self.layers {
            overlay(&mut merged, &layer.table, None);
        }
        WorktreeConfig::from_table(merged).map_err(SdkError::InvalidConfig)
    }

    /// The file that set `key` (highest layer defining it), or `default`.
    pub fn origin(&self, key: &str) -> ConfigOrigin {
        let key = canonical_key(key);
        self.layers
            .iter()
            .rev()
            .find(|l| defines(&l.table, &key))
            .map(|l| l.origin.clone())
            .unwrap_or_else(ConfigOrigin::default_origin)
    }

    /// One effective value with its origin; `None` for unknown/unset keys.
    pub fn get(&self, key: &str) -> Result<Option<ConfigEntry>> {
        let key = canonical_key(key);
        Ok(self.effective()?.get(&key).map(|value| ConfigEntry {
            origin: self.origin(&key),
            key,
            value,
        }))
    }

    /// Every effective value with its origin, sorted by key.
    pub fn entries(&self) -> Result<Vec<ConfigEntry>> {
        Ok(self
            .effective()?
            .entries()
            .into_iter()
            .map(|(key, value)| ConfigEntry {
                origin: self.origin(&key),
                key,
                value,
            })
            .collect())
    }
}

/// Keep only the allowed top-level sections of a layer file.
fn filter_sections(table: Table, allowed: &[&str], path: &Path) -> Table {
    table
        .into_iter()
        .filter(|(k, v)| {
            let keep = allowed.contains(&k.as_str()) && v.is_table();
            if !keep && v.is_table() && SECTION_ORDER.contains(&k.as_str()) {
                tracing::debug!(section = %k, path = %path.display(), "config section not read at this level");
            }
            keep
        })
        .collect()
}

/// Check that a partial layer parses when applied over the defaults.
fn validate_layer(table: &Table, path: &Path) -> Result<()> {
    let mut probe = WorktreeConfig::new("probe", "main").to_table()?;
    overlay(&mut probe, table, None);
    WorktreeConfig::from_table(probe)
        .map(|_| ())
        .map_err(|e| SdkError::InvalidConfig(format!("{}: {e}", path.display())))
}

fn layer(level: ConfigLevel, tree: Option<String>, path: PathBuf, mut table: Table) -> ConfigLayer {
    normalize(&mut table);
    drop_unset(&mut table);
    ConfigLayer {
        origin: ConfigOrigin {
            level,
            tree,
            path: Some(path),
        },
        table,
    }
}

/// Which sections may be set at `scope` (`None` = all).
fn allowed_sections(scope: &ConfigScope) -> Option<&'static [&'static str]> {
    match scope {
        ConfigScope::User => Some(USER_SECTIONS),
        ConfigScope::Worktree => None,
        ConfigScope::Tree(_) => Some(TREE_SECTIONS),
    }
}

fn check_scope(scope: &ConfigScope, key: &str) -> Result<()> {
    let Some(allowed) = allowed_sections(scope) else {
        return Ok(());
    };
    let section = key.split('.').next().unwrap_or_default();
    if allowed.contains(&section) {
        return Ok(());
    }
    let level = match scope {
        ConfigScope::User => "the user config (--global)",
        _ => "a tree config (--tree)",
    };
    Err(SdkError::InvalidConfig(format!(
        "{key} cannot be set in {level}; allowed sections: {}",
        allowed.join(", ")
    )))
}

/// The `[user]` section used for authorship: worktree values, with empty
/// fields filled from the user config (`<wt home>/config.toml`).
pub fn effective_user(worktree: &UserSection) -> UserSection {
    let mut user = worktree.clone();
    if user.name.trim().is_empty() || user.email.trim().is_empty() {
        if let Ok(Some(t)) = read_table_opt(&user_config_path()) {
            let get = |k: &str| {
                lookup(&t, k)
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            };
            if user.name.trim().is_empty() {
                user.name = get("user.name");
            }
            if user.email.trim().is_empty() {
                user.email = get("user.email");
            }
        }
    }
    user
}

impl Repository {
    /// The configuration layers that apply to `tree` (user, worktree, then
    /// the `.wt-tree/config.toml` of each enclosing tree and the tree
    /// itself). With `None`, only the user and worktree layers.
    pub fn config_stack(&self, tree: Option<&str>) -> Result<ConfigStack> {
        let mut layers = Vec::new();
        let user_path = user_config_path();
        if let Some(t) = read_table_opt(&user_path)? {
            let t = filter_sections(t, USER_SECTIONS, &user_path);
            validate_layer(&t, &user_path)?;
            layers.push(layer(ConfigLevel::User, None, user_path, t));
        }
        let wt_path = self.wt_dir().join(CONFIG_FILE_NAME);
        let wt_table = match read_table_opt(&wt_path)? {
            Some(t) => t,
            None => self.config().to_table()?,
        };
        layers.push(layer(ConfigLevel::Worktree, None, wt_path, wt_table));
        if let Some(tree) = tree {
            for (name, dir) in self.tree_ancestry(tree)? {
                let path = dir.join(TREE_MARKER_DIR).join(CONFIG_FILE_NAME);
                if let Some(t) = read_table_opt(&path)? {
                    let t = filter_sections(t, TREE_SECTIONS, &path);
                    validate_layer(&t, &path)?;
                    layers.push(layer(ConfigLevel::Tree, Some(name), path, t));
                }
            }
        }
        Ok(ConfigStack { layers })
    }

    /// The effective configuration for `tree` (see [`Self::config_stack`]).
    pub fn effective_config(&self, tree: Option<&str>) -> Result<WorktreeConfig> {
        self.config_stack(tree)?.effective()
    }

    /// The secret scanner configured for `tree` (effective
    /// `[secret_scanning]`); `InvalidConfig` for a bad pattern or glob.
    pub fn secret_scanner(&self, tree: &str) -> Result<crate::scan::SecretScanner> {
        let config = self.effective_config(Some(tree))?;
        crate::scan::SecretScanner::new(&config.secret_scanning).map_err(SdkError::InvalidConfig)
    }

    /// `<tree dir>/.wt-tree/config.toml`; `None` for the root tree.
    pub fn tree_config_path(&self, tree: &str) -> Result<Option<PathBuf>> {
        let rec = self.load_tree(tree)?;
        if rec.path.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            self.tree_dir_of(&rec)
                .join(TREE_MARKER_DIR)
                .join(CONFIG_FILE_NAME),
        ))
    }

    /// Nested trees enclosing `tree` (outermost first, `tree` last) as
    /// (name, directory). Empty for the root tree.
    pub(crate) fn tree_ancestry(&self, tree: &str) -> Result<Vec<(String, PathBuf)>> {
        let rec = self.load_tree(tree)?;
        if rec.path.is_empty() {
            return Ok(Vec::new());
        }
        let mut out: Vec<_> = self
            .tree_records()?
            .into_iter()
            .filter(|r| {
                !r.path.is_empty() && crate::repo::names::path_has_prefix(&rec.path, &r.path)
            })
            .collect();
        out.sort_by_key(|r| r.path.split('/').count());
        Ok(out
            .into_iter()
            .map(|r| {
                let dir = self.tree_dir_of(&r);
                (r.name, dir)
            })
            .collect())
    }

    /// The file `scope` writes; `Tree` scope on the root tree is the
    /// worktree file.
    fn scope_path(&self, scope: &ConfigScope) -> Result<(PathBuf, ConfigScope)> {
        match scope {
            ConfigScope::User => Ok((user_config_path(), ConfigScope::User)),
            ConfigScope::Worktree => {
                Ok((self.wt_dir().join(CONFIG_FILE_NAME), ConfigScope::Worktree))
            }
            ConfigScope::Tree(name) => match self.tree_config_path(name)? {
                Some(path) => Ok((path, scope.clone())),
                None => Ok((self.wt_dir().join(CONFIG_FILE_NAME), ConfigScope::Worktree)),
            },
        }
    }

    /// Set one key in the file of `scope`, keeping every other key of that
    /// file (including unknown ones) as is. Values are validated exactly as
    /// [`WorktreeConfig::set`] does.
    pub fn set_config_value(&mut self, scope: &ConfigScope, key: &str, value: &str) -> Result<()> {
        let key = canonical_key(key);
        let (path, scope) = self.scope_path(scope)?;
        check_scope(&scope, &key)?;
        let mut probe = match scope {
            ConfigScope::Worktree => self.config().clone(),
            _ => WorktreeConfig::new("probe", "main"),
        };
        probe.set(&key, value)?;
        let mut table = read_table_opt(&path)?.unwrap_or_default();
        let removing_pattern = (key.starts_with(PATTERNS_PREFIX) && value.is_empty())
            || (key.starts_with(ALIAS_PREFIX) && value.trim().is_empty()); // cli-ux (§14.8)
        if removing_pattern {
            remove_key(&mut table, &key);
        } else {
            let typed = probe
                .toml_value(&key)
                .ok_or_else(|| SdkError::InvalidConfig(format!("unknown config key: {key}")))?;
            // Drop an alias spelling so the file does not hold both.
            remove_key(&mut table, &key);
            insert_key(&mut table, &key, typed)?;
        }
        self.write_layer(&scope, &path, &table)
    }

    /// Remove one key from the file of `scope`; whether it was set there.
    /// `worktree.*` and `remote.*` cannot be unset (use `wt remote remove`).
    pub fn unset_config_value(&mut self, scope: &ConfigScope, key: &str) -> Result<bool> {
        let key = canonical_key(key);
        if key.starts_with("worktree.") || key.starts_with("remote.") {
            return Err(SdkError::InvalidConfig(format!("{key} cannot be unset")));
        }
        if scalar_kind(&key).is_none()
            && !key.starts_with(PATTERNS_PREFIX)
            && !key.starts_with(ALIAS_PREFIX)
        {
            return Err(SdkError::InvalidConfig(format!(
                "unknown config key: {key}"
            )));
        }
        let (path, scope) = self.scope_path(scope)?;
        let Some(mut table) = read_table_opt(&path)? else {
            return Ok(false);
        };
        if !remove_key(&mut table, &key) {
            return Ok(false);
        }
        self.write_layer(&scope, &path, &table)?;
        Ok(true)
    }

    fn write_layer(&mut self, scope: &ConfigScope, path: &Path, table: &Table) -> Result<()> {
        let mut normalized = table.clone();
        normalize(&mut normalized);
        match scope {
            ConfigScope::Worktree => {
                WorktreeConfig::from_table(normalized)
                    .map_err(|e| SdkError::InvalidConfig(format!("{}: {e}", path.display())))?;
                write_table(path, table)?;
                *self = Repository::open(self.root())?;
            }
            ConfigScope::User => {
                validate_layer(&filter_sections(normalized, USER_SECTIONS, path), path)?;
                write_table(path, table)?;
            }
            ConfigScope::Tree(_) => {
                validate_layer(&filter_sections(normalized, TREE_SECTIONS, path), path)?;
                write_table(path, table)?;
            }
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------
// Tree layout: `wt tree validate` / `wt tree init-config`
// ----------------------------------------------------------------------

/// Severity of a [`TreeProblem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One finding of [`Repository::validate_tree`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TreeProblem {
    pub severity: Severity,
    /// Root-relative path the problem is about ('/'-separated).
    pub path: String,
    pub message: String,
}

/// Commented template appended to a scaffolded `.wt-tree/config.toml`.
const TREE_CONFIG_TEMPLATE: &str = "
# Tree-level settings; every key is optional and inherits from .wt/config.toml.
# Only [auto_snapshot], [secret_scanning] and [hooks] are read here.
#
# [auto_snapshot]
# enabled = true
# inactivity_secs = 30
# max_changed_files = 50
# max_changed_bytes = \"50MB\"
# message_format = \"auto: {count} files changed\"
#
# [secret_scanning]
# block_on_match = true
# exclude = [\"fixtures/\"]
# patterns = [{ name = \"internal-token\", regex = \"itk_[0-9a-f]{32}\" }]
#
# [hooks]
# timeout_secs = 60
";

const TREE_IGNORE_TEMPLATE: &str = "\
# Tree-level ignore patterns (gitignore syntax), relative to this tree.
# Additive: they cannot re-include anything ignored by .wt/ignore or the
# root .wtignore.
";

impl Repository {
    /// Check a tree's on-disk layout: the `.wt-tree/` marker and its
    /// `config.toml` (parses, matches the registry), the tree's ignore
    /// files, and that no nested `.wt/` directory hides inside the tree.
    pub fn validate_tree(&self, tree: &str) -> Result<Vec<TreeProblem>> {
        use crate::repo::ignore_rules::{ignore_file_problems, TREE_IGNORE_FILE};
        let rec = self.load_tree(tree)?;
        let dir = self.tree_dir_of(&rec);
        let rel = |p: &Path| {
            p.strip_prefix(self.root())
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let mut out = Vec::new();
        let mut problem = |severity, path: &Path, message: String| {
            out.push(TreeProblem {
                severity,
                path: rel(path),
                message,
            })
        };
        if !dir.is_dir() {
            problem(
                Severity::Error,
                &dir,
                "tree directory does not exist".into(),
            );
            return Ok(out);
        }
        let mut ignore_files = vec![dir.join(crate::repo::IGNORE_FILE_NAME)];
        if !rec.path.is_empty() {
            let marker = dir.join(TREE_MARKER_DIR);
            let config_path = marker.join(CONFIG_FILE_NAME);
            if !marker.is_dir() {
                problem(
                    Severity::Error,
                    &marker,
                    "missing .wt-tree/ marker directory (run `wt tree init-config`)".into(),
                );
            } else {
                match fsutil::read_string_opt(&config_path)? {
                    None => problem(
                        Severity::Error,
                        &config_path,
                        "missing .wt-tree/config.toml (run `wt tree init-config`)".into(),
                    ),
                    Some(text) => match text.parse::<Table>() {
                        Err(e) => problem(Severity::Error, &config_path, e.message().to_string()),
                        Ok(table) => {
                            for (key, want) in [("name", &rec.name), ("id", &rec.id)] {
                                match table.get(key).and_then(Value::as_str) {
                                    Some(v) if v == want.as_str() => {}
                                    Some(v) => problem(
                                        Severity::Warning,
                                        &config_path,
                                        format!(
                                            "{key} is {v:?} but the tree is registered as {want:?}"
                                        ),
                                    ),
                                    None => problem(
                                        Severity::Warning,
                                        &config_path,
                                        format!("{key} is not set"),
                                    ),
                                }
                            }
                            for (k, v) in &table {
                                if v.is_table() && !TREE_SECTIONS.contains(&k.as_str()) {
                                    problem(
                                        Severity::Warning,
                                        &config_path,
                                        format!(
                                            "[{k}] is not read at tree level (allowed: {})",
                                            TREE_SECTIONS.join(", ")
                                        ),
                                    );
                                }
                            }
                            let filtered = filter_sections(table, TREE_SECTIONS, &config_path);
                            let mut normalized = filtered;
                            normalize(&mut normalized);
                            if let Err(e) = validate_layer(&normalized, &config_path) {
                                problem(Severity::Error, &config_path, e.to_string());
                            } else if let Ok(c) = WorktreeConfig::from_table({
                                let mut probe = WorktreeConfig::new("probe", "main").to_table()?;
                                overlay(&mut probe, &normalized, None);
                                probe
                            }) {
                                if let Err(e) = crate::scan::SecretScanner::new(&c.secret_scanning)
                                {
                                    problem(Severity::Error, &config_path, e);
                                }
                            }
                        }
                    },
                }
            }
            ignore_files.push(marker.join(TREE_IGNORE_FILE));
        } else {
            ignore_files.push(self.wt_dir().join("ignore"));
        }
        for file in ignore_files {
            if let Some(text) = fsutil::read_string_opt(&file)? {
                for (line, message) in ignore_file_problems(&text) {
                    problem(Severity::Error, &file, format!("line {line}: {message}"));
                }
            }
        }
        let mut nested = Vec::new();
        find_nested_wt_dirs(&dir, &self.wt_dir(), &mut nested);
        for path in nested {
            problem(
                Severity::Error,
                &path,
                "nested .wt/ directory (a worktree inside a tree is not supported)".into(),
            );
        }
        Ok(out)
    }

    /// Scaffold `<tree>/.wt-tree/{config.toml,ignore,hooks/,access/}`,
    /// creating only what is missing. Returns the created paths. The root
    /// tree is configured in `.wt/` and has no `.wt-tree/`.
    pub fn init_tree_config(&self, tree: &str) -> Result<Vec<PathBuf>> {
        use crate::repo::ignore_rules::TREE_IGNORE_FILE;
        let rec = self.load_tree(tree)?;
        if rec.path.is_empty() {
            return Err(SdkError::InvalidArgument(
                "the root tree is configured in .wt/ (it has no .wt-tree/)".into(),
            ));
        }
        let marker = self.tree_dir_of(&rec).join(TREE_MARKER_DIR);
        let mut created = Vec::new();
        let config_path = marker.join(CONFIG_FILE_NAME);
        if !config_path.exists() {
            let mut table = Table::new();
            table.insert("name".into(), Value::String(rec.name.clone()));
            table.insert("id".into(), Value::String(rec.id.clone()));
            let text = format!("{}{TREE_CONFIG_TEMPLATE}", toml::to_string_pretty(&table)?);
            fsutil::write_atomic(&config_path, text.as_bytes())?;
            created.push(config_path);
        }
        let ignore_path = marker.join(TREE_IGNORE_FILE);
        if !ignore_path.exists() {
            fsutil::write_atomic(&ignore_path, TREE_IGNORE_TEMPLATE.as_bytes())?;
            created.push(ignore_path);
        }
        for sub in [crate::hooks::HOOKS_DIR, "access"] {
            let path = marker.join(sub);
            if !path.is_dir() {
                std::fs::create_dir_all(&path)?;
                created.push(path);
            }
        }
        Ok(created)
    }
}

/// Collect `.wt` directories below `dir` (other than the worktree's own).
fn find_nested_wt_dirs(dir: &Path, own: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        if name == crate::repo::WT_DIR {
            if path != own {
                out.push(path);
            }
            continue;
        }
        if name == ".git" || name == TREE_MARKER_DIR {
            continue;
        }
        find_nested_wt_dirs(&path, own, out);
    }
}

// ----------------------------------------------------------------------
// cli-ux (ENGINEERING_PLAN §14.8): command aliases and per-layer checks
// ----------------------------------------------------------------------

/// Alias names: 1-64 of `[A-Za-z0-9_-]`, starting with a letter or digit.
pub fn validate_alias_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(SdkError::InvalidConfig(format!(
            "alias name {name:?} must start with a letter or digit and use only letters, digits, '-' and '_'"
        )))
    }
}

/// Split an alias command line into words, shell style: whitespace
/// separates words, `'...'` quotes literally, `"..."` quotes with `\"` and
/// `\\` escapes, and outside quotes a backslash escapes whitespace, quotes
/// and itself (any other backslash is kept, so Windows paths work).
pub fn split_alias_command(value: &str) -> std::result::Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated ' quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            word.extend(chars.next());
                        }
                        Some(c) => word.push(c),
                        None => return Err("unterminated \" quote".into()),
                    }
                }
            }
            '\\' if chars
                .peek()
                .is_some_and(|n| n.is_whitespace() || matches!(n, '\'' | '"' | '\\')) =>
            {
                in_word = true;
                word.extend(chars.next());
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    if words.is_empty() {
        return Err("the alias is empty".into());
    }
    Ok(words)
}

/// The aliases that apply in `dir`: `[alias]` of the user config, replaced
/// name by name by `[alias]` of the worktree containing `dir` (if any).
/// Best effort and cheap (no repository is opened): unreadable or invalid
/// files and non-string values are skipped (`wt doctor` reports them).
pub fn load_aliases(dir: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut files = vec![user_config_path()];
    if let Ok(start) = std::path::absolute(dir) {
        if let Some(root) = start
            .ancestors()
            .find(|d| d.join(crate::repo::WT_DIR).is_dir())
        {
            files.push(root.join(crate::repo::WT_DIR).join(CONFIG_FILE_NAME));
        }
    }
    for file in files {
        let Ok(Some(table)) = read_table_opt(&file) else {
            continue;
        };
        if let Some(Value::Table(aliases)) = table.get("alias") {
            for (name, value) in aliases {
                if let Some(command) = value.as_str() {
                    out.insert(name.clone(), command.to_string());
                }
            }
        }
    }
    out
}

/// Check one configuration file the way it is read at `level` (user,
/// worktree or tree). `Ok(false)` when the file does not exist.
pub fn validate_config_file(path: &Path, level: ConfigLevel) -> Result<bool> {
    let Some(table) = read_table_opt(path)? else {
        return Ok(false);
    };
    match level {
        ConfigLevel::Worktree => {
            WorktreeConfig::load(path)?;
        }
        ConfigLevel::User | ConfigLevel::Tree | ConfigLevel::Default => {
            let allowed = if level == ConfigLevel::Tree {
                TREE_SECTIONS
            } else {
                USER_SECTIONS
            };
            let mut t = filter_sections(table, allowed, path);
            normalize(&mut t);
            drop_unset(&mut t);
            validate_layer(&t, path)?;
        }
    }
    Ok(true)
}

/// `wt config set --global KEY VALUE` outside a worktree: set one key of
/// the user config (`<wt home>/config.toml`), validated like
/// [`Repository::set_config_value`] with [`ConfigScope::User`].
pub fn set_user_config_value(key: &str, value: &str) -> Result<()> {
    let key = canonical_key(key);
    check_scope(&ConfigScope::User, &key)?;
    let mut probe = WorktreeConfig::new("probe", "main");
    probe.set(&key, value)?;
    let path = user_config_path();
    let mut table = read_table_opt(&path)?.unwrap_or_default();
    let removing = (key.starts_with(PATTERNS_PREFIX) && value.is_empty())
        || (key.starts_with(ALIAS_PREFIX) && value.trim().is_empty());
    remove_key(&mut table, &key);
    if !removing {
        let typed = probe
            .toml_value(&key)
            .ok_or_else(|| SdkError::InvalidConfig(format!("unknown config key: {key}")))?;
        insert_key(&mut table, &key, typed)?;
    }
    let mut normalized = table.clone();
    normalize(&mut normalized);
    validate_layer(&filter_sections(normalized, USER_SECTIONS, &path), &path)?;
    write_table(&path, &table)
}

/// `wt config unset --global KEY` outside a worktree; whether it was set.
pub fn unset_user_config_value(key: &str) -> Result<bool> {
    let key = canonical_key(key);
    check_scope(&ConfigScope::User, &key)?;
    if scalar_kind(&key).is_none()
        && !key.starts_with(PATTERNS_PREFIX)
        && !key.starts_with(ALIAS_PREFIX)
    {
        return Err(SdkError::InvalidConfig(format!(
            "unknown config key: {key}"
        )));
    }
    let path = user_config_path();
    let Some(mut table) = read_table_opt(&path)? else {
        return Ok(false);
    };
    if !remove_key(&mut table, &key) {
        return Ok(false);
    }
    write_table(&path, &table)?;
    Ok(true)
}

// --- end cli-ux

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
        assert_eq!(c.get("hooks.timeout_secs").as_deref(), Some("60"));
        assert_eq!(
            c.get("auto_snapshot.message_format").as_deref(),
            Some(DEFAULT_AUTO_MESSAGE_FORMAT)
        );
        assert_eq!(
            c.get("auto_snapshot.on_branch_switch").as_deref(),
            Some("refuse")
        );
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

    // sync workstream (§15.5)
    #[test]
    fn sync_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = cfg();
        assert_eq!(c.get("sync.strategy").as_deref(), Some("merge"));
        assert_eq!(c.get("sync.auto_flush").as_deref(), Some("true"));
        assert_eq!(c.get("sync.auto_sync_secs").as_deref(), Some("0"));
        // Defaults are not written out.
        c.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("strategy") && !text.contains("auto_flush"),
            "{text}"
        );

        c.set("sync.strategy", "Rebase").unwrap();
        c.set("sync.auto_flush", "false").unwrap();
        c.set("sync.auto_sync_secs", "120").unwrap();
        assert_eq!(c.sync.strategy, SyncStrategy::Rebase);
        assert!(!c.sync.auto_flush);
        assert_eq!(c.sync.auto_sync_secs, 120);
        assert!(matches!(
            c.set("sync.strategy", "squash"),
            Err(SdkError::InvalidConfig(m)) if m.contains("merge or rebase")
        ));
        assert!(c.set("sync.auto_sync_secs", "-1").is_err());
        c.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("strategy = \"rebase\""), "{text}");
        let loaded = WorktreeConfig::load(&path).unwrap();
        assert_eq!(loaded, c);
        assert_eq!(loaded.get("sync.strategy").as_deref(), Some("rebase"));
        assert_eq!(
            c.toml_value("sync.auto_sync_secs"),
            Some(Value::Integer(120))
        );
        assert!(c
            .entries()
            .iter()
            .any(|(k, v)| k == "sync.auto_flush" && v == "false"));
    }

    #[test]
    fn new_auto_snapshot_keys_and_alias() {
        let mut c = cfg();
        c.set("auto_snapshot.inactivity_timeout_secs", "9").unwrap();
        assert_eq!(c.auto_snapshot.inactivity_secs, 9);
        assert_eq!(
            c.get("auto_snapshot.inactivity_timeout_secs").as_deref(),
            Some("9")
        );
        c.set("auto_snapshot.max_changed_bytes", "50MB").unwrap();
        assert_eq!(c.auto_snapshot.max_changed_bytes, 50_000_000);
        c.set("auto_snapshot.max_changed_bytes", "2KiB").unwrap();
        assert_eq!(c.auto_snapshot.max_changed_bytes, 2048);
        assert!(c.set("auto_snapshot.max_changed_bytes", "lots").is_err());
        c.set("auto_snapshot.on_branch_switch", "stash").unwrap();
        assert_eq!(c.auto_snapshot.on_branch_switch, OnBranchSwitch::Stash);
        assert!(c.set("auto_snapshot.on_branch_switch", "panic").is_err());
        c.set("auto_snapshot.message_format", "wip on {branch}: {files}")
            .unwrap();
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        assert_eq!(
            c.auto_snapshot.format_message(&files, "main", "web"),
            "wip on main: a.rs, b.rs"
        );
    }

    #[test]
    fn message_format_placeholders() {
        let s = AutoSnapshotSection::default();
        let files: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
        assert_eq!(
            s.format_message(&files, "main", "t"),
            "auto: 12 files changed"
        );
        let s = AutoSnapshotSection {
            message_format: "{count}/{file_count} {tree}@{branch} {files}".into(),
            ..AutoSnapshotSection::default()
        };
        let msg = s.format_message(&files, "dev", "web");
        assert!(msg.starts_with("12/12 web@dev f0, f1"), "{msg}");
        assert!(msg.ends_with("f9 (+2 more)"), "{msg}");
        let empty = AutoSnapshotSection {
            message_format: " ".into(),
            ..AutoSnapshotSection::default()
        };
        assert_eq!(
            empty.format_message(&files[..1], "m", "t"),
            "auto: 1 files changed"
        );
    }

    #[test]
    fn toml_accepts_alias_sizes_and_bool_switch() {
        let text = r#"
[worktree]
name = "w"
id = "i"
created_at = "now"

[auto_snapshot]
inactivity_timeout_secs = 300
max_changed_bytes = "1MiB"
on_branch_switch = true
"#;
        let c = WorktreeConfig::from_table(text.parse().unwrap()).unwrap();
        assert_eq!(c.auto_snapshot.inactivity_secs, 300);
        assert_eq!(c.auto_snapshot.max_changed_bytes, 1 << 20);
        assert_eq!(c.auto_snapshot.on_branch_switch, OnBranchSwitch::Snapshot);
        assert!(c.auto_snapshot.enabled, "missing keys take defaults");
        assert_eq!(c.auto_snapshot.max_changed_files, 50);

        // Both spellings: the canonical one wins instead of a duplicate-key error.
        let both = text.replace(
            "inactivity_timeout_secs = 300",
            "inactivity_timeout_secs = 300\ninactivity_secs = 7",
        );
        let c = WorktreeConfig::from_table(both.parse().unwrap()).unwrap();
        assert_eq!(c.auto_snapshot.inactivity_secs, 7);
    }

    #[test]
    fn secret_scanning_keys() {
        let mut c = cfg();
        c.set("secret_scanning.block_on_match", "false").unwrap();
        c.set("secret_scanning.exclude", "fixtures/, *.pem ,")
            .unwrap();
        assert_eq!(c.secret_scanning.exclude, vec!["fixtures/", "*.pem"]);
        assert_eq!(
            c.get("secret_scanning.exclude").as_deref(),
            Some("fixtures/,*.pem")
        );
        c.set("secret_scanning.patterns.internal", r"ACME-[0-9]{8}")
            .unwrap();
        c.set("secret_scanning.patterns.internal", r"ACME-[0-9]{10}")
            .unwrap();
        assert_eq!(c.secret_scanning.patterns.len(), 1);
        assert_eq!(
            c.get("secret_scanning.patterns.internal").as_deref(),
            Some(r"ACME-[0-9]{10}")
        );
        assert!(c
            .entries()
            .iter()
            .any(|(k, _)| k == "secret_scanning.patterns.internal"));
        assert!(c.set("secret_scanning.patterns.bad", "(unclosed").is_err());
        assert!(c.set("secret_scanning.patterns.no spaces", "x").is_err());
        c.set("secret_scanning.patterns.internal", "").unwrap();
        assert!(c.secret_scanning.patterns.is_empty());
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
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[worktree]"), "{text}");
        std::fs::write(&path, "not = [valid").unwrap();
        assert!(matches!(
            WorktreeConfig::load(&path),
            Err(SdkError::InvalidConfig(_))
        ));
    }

    #[test]
    fn save_preserves_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = cfg();
        c.set("remote.origin.url", "http://h").unwrap();
        c.set("remote.gone.url", "http://g").unwrap();
        c.save(&path).unwrap();
        // Add keys this version does not know, at every nesting level.
        let mut raw: Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        raw.insert(
            "future".into(),
            Value::Table(Table::from_iter([(
                "flag".to_string(),
                Value::Boolean(true),
            )])),
        );
        raw["remote"]["origin"]
            .as_table_mut()
            .unwrap()
            .insert("insecure".into(), Value::Boolean(true));
        raw["remote"]["gone"]
            .as_table_mut()
            .unwrap()
            .insert("note".into(), Value::String("x".into()));
        raw["user"]
            .as_table_mut()
            .unwrap()
            .insert("signature".into(), Value::String("Ada".into()));
        raw["auto_snapshot"]
            .as_table_mut()
            .unwrap()
            .insert("inactivity_timeout_secs".into(), Value::Integer(99));
        raw["auto_snapshot"]
            .as_table_mut()
            .unwrap()
            .remove("inactivity_secs");
        std::fs::write(&path, toml::to_string(&raw).unwrap()).unwrap();

        let mut loaded = WorktreeConfig::load(&path).unwrap();
        assert_eq!(loaded.auto_snapshot.inactivity_secs, 99);
        loaded.user.name = "Ada".into();
        loaded.remote.remove("gone");
        loaded.save(&path).unwrap();

        let saved: Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(saved["future"]["flag"].as_bool(), Some(true));
        assert_eq!(saved["remote"]["origin"]["insecure"].as_bool(), Some(true));
        assert!(
            saved["remote"].get("gone").is_none(),
            "removed remote stays removed"
        );
        assert_eq!(saved["user"]["signature"].as_str(), Some("Ada"));
        assert_eq!(saved["user"]["name"].as_str(), Some("Ada"));
        assert!(saved["auto_snapshot"]
            .get("inactivity_timeout_secs")
            .is_none());
        assert_eq!(
            saved["auto_snapshot"]["inactivity_secs"].as_integer(),
            Some(99)
        );
        assert_eq!(WorktreeConfig::load(&path).unwrap(), loaded);
    }

    #[test]
    fn overlay_replaces_scalars_and_concatenates_additive_arrays() {
        let mut base: Table =
            "[secret_scanning]\nexclude = [\"a\"]\nenabled = true\n[hooks]\ntimeout_secs = 5\n"
                .parse()
                .unwrap();
        let top: Table = "[secret_scanning]\nexclude = [\"b\", \"a\"]\nenabled = false\n[hooks]\ntimeout_secs = 9\n"
            .parse()
            .unwrap();
        overlay(&mut base, &top, None);
        let ex: Vec<_> = base["secret_scanning"]["exclude"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ex, vec!["a", "b"]);
        assert_eq!(base["secret_scanning"]["enabled"].as_bool(), Some(false));
        assert_eq!(base["hooks"]["timeout_secs"].as_integer(), Some(9));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("123"), Ok(123));
        assert_eq!(parse_size("1kb"), Ok(1000));
        assert_eq!(parse_size(" 3 MiB "), Ok(3 << 20));
        assert_eq!(parse_size("1GB"), Ok(1_000_000_000));
        assert!(parse_size("1TB").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("99999999999999999999GiB").is_err());
    }

    // --- cli-ux (§14.8)
    #[test]
    fn aliases_get_set_and_list() {
        let mut c = cfg();
        c.set("alias.st", "status").unwrap();
        c.set("alias.lg", "  log --oneline -n 20 ").unwrap();
        assert_eq!(c.get("alias.st").as_deref(), Some("status"));
        assert_eq!(c.get("alias.lg").as_deref(), Some("log --oneline -n 20"));
        assert!(c
            .entries()
            .contains(&("alias.lg".into(), "log --oneline -n 20".into())));
        c.set("alias.st", "").unwrap();
        assert_eq!(c.get("alias.st"), None);
        for bad in ["", "-x", "a.b", "a b", "ä"] {
            assert!(c.set(&format!("alias.{bad}"), "status").is_err(), "{bad:?}");
        }
        assert!(c.set("alias.q", "log \"unterminated").is_err());
        let toml = toml::to_string(&c).unwrap();
        assert!(toml.contains("[alias]"), "{toml}");
        assert!(USER_SECTIONS.contains(&"alias"));
        assert!(!TREE_SECTIONS.contains(&"alias"));
    }

    #[test]
    fn alias_command_lines_split_like_a_shell() {
        let split = |s: &str| split_alias_command(s).unwrap();
        assert_eq!(
            split("log --oneline -n 20"),
            ["log", "--oneline", "-n", "20"]
        );
        assert_eq!(
            split("snapshot -m 'quick fix' --allow-empty"),
            ["snapshot", "-m", "quick fix", "--allow-empty"]
        );
        assert_eq!(split(r#"x "a \"b\" c" d\ e"#), ["x", "a \"b\" c", "d e"]);
        assert_eq!(
            split(r"restore C:\work\a.txt"),
            ["restore", r"C:\work\a.txt"]
        );
        assert_eq!(split("x ''"), ["x", ""]);
        assert!(split_alias_command("   ").is_err());
        assert!(split_alias_command("x 'open").is_err());
        assert!(validate_alias_name("ci-2_x").is_ok());
    }

    #[test]
    fn worktree_aliases_are_written_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = Repository::init(dir.path(), Default::default()).unwrap();
        repo.set_config_value(&ConfigScope::Worktree, "alias.lg", "log --oneline")
            .unwrap();
        assert_eq!(repo.config().alias["lg"], "log --oneline");
        let text = std::fs::read_to_string(repo.wt_dir().join(CONFIG_FILE_NAME)).unwrap();
        assert!(
            text.contains("[alias]") && text.contains("lg = \"log --oneline\""),
            "{text}"
        );
        let stack = repo.config_stack(None).unwrap();
        let entry = stack.get("alias.lg").unwrap().unwrap();
        assert_eq!(entry.origin.level, ConfigLevel::Worktree);

        let sub = dir.path().join("deep/er");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            load_aliases(&sub).get("lg").map(String::as_str),
            Some("log --oneline")
        );

        assert!(repo
            .set_config_value(&ConfigScope::Tree("x".into()), "alias.lg", "log")
            .is_err());
        assert!(repo
            .unset_config_value(&ConfigScope::Worktree, "alias.lg")
            .unwrap());
        assert!(!repo.config().alias.contains_key("lg"));
        assert!(
            validate_config_file(&repo.wt_dir().join(CONFIG_FILE_NAME), ConfigLevel::Worktree)
                .unwrap()
        );
    }

    #[test]
    fn layer_files_are_validated_per_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(!validate_config_file(&path, ConfigLevel::User).unwrap());
        std::fs::write(&path, "[user]\nname = \"Ada\"\n[alias]\nst = \"status\"\n").unwrap();
        assert!(validate_config_file(&path, ConfigLevel::User).unwrap());
        std::fs::write(&path, "[alias]\nst = 3\n").unwrap();
        assert!(validate_config_file(&path, ConfigLevel::User).is_err());
        // Tree level ignores [alias] entirely.
        assert!(validate_config_file(&path, ConfigLevel::Tree).unwrap());
        std::fs::write(&path, "[hooks]\ntimeout_secs = \"soon\"\n").unwrap();
        assert!(validate_config_file(&path, ConfigLevel::Tree).is_err());
        std::fs::write(&path, "not toml = = =").unwrap();
        assert!(validate_config_file(&path, ConfigLevel::User).is_err());
    }
    // --- end cli-ux

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
