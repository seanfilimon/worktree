//! Data model shared by every SDK consumer.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use worktree_protocol::proto::sync as wire;

use crate::error::{Result, SdkError};
use crate::store::ObjectHash;

/// Snapshot ids are UUID v4 strings (matches `SnapshotId.value` on the wire).
pub type SnapshotId = String;

/// One file in a manifest. `path` is '/'-separated and relative to the tree root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub hash: ObjectHash,
    pub size: u64,
    pub executable: bool,
    /// When true, `hash` names a chunk-list object (large file split into
    /// content-defined chunks) instead of a blob. See ENGINEERING_PLAN §13.2.
    #[serde(default)]
    pub chunked: bool,
}

/// The complete file listing of a snapshot. Entries are sorted by path and
/// unique. Stored in the object store as a protobuf-encoded
/// `worktree.sync.v1.ManifestRecord`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Build from entries in any order (sorts + dedups by path, last wins).
    pub fn from_entries(entries: Vec<ManifestEntry>) -> Self {
        let mut entries = entries;
        // Stable sort keeps duplicates in input order, so "last wins" means
        // keeping the final element of each run.
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let mut out: Vec<ManifestEntry> = Vec::with_capacity(entries.len());
        for entry in entries {
            match out.last_mut() {
                Some(last) if last.path == entry.path => *last = entry,
                _ => out.push(entry),
            }
        }
        Self { entries: out }
    }

    /// The entry for `path`, if present.
    pub fn get(&self, path: &str) -> Option<&ManifestEntry> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// Canonical protobuf encoding (ManifestRecord).
    pub fn to_bytes(&self) -> Vec<u8> {
        use prost::Message;
        let record = wire::ManifestRecord {
            entries: self
                .entries
                .iter()
                .map(|e| wire::ManifestEntry {
                    path: e.path.clone(),
                    blob_hash: Some(hash_to_wire(&e.hash)),
                    size: e.size,
                    executable: e.executable,
                    chunked: e.chunked,
                })
                .collect(),
        };
        record.encode_to_vec()
    }

    /// Decode a protobuf `ManifestRecord`. Rejects unsorted or duplicate
    /// entries, malformed hashes and unsafe paths (see [`validate_manifest_path`]).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        use prost::Message;
        let record = wire::ManifestRecord::decode(data)
            .map_err(|e| SdkError::Serialization(format!("invalid manifest: {e}")))?;
        let mut entries = Vec::with_capacity(record.entries.len());
        for e in record.entries {
            validate_manifest_path(&e.path)?;
            let hash = e
                .blob_hash
                .as_ref()
                .ok_or_else(|| {
                    SdkError::Serialization(format!("manifest entry {} has no hash", e.path))
                })
                .and_then(|h| hash_from_wire(h, "blob_hash"))?;
            entries.push(ManifestEntry {
                path: e.path,
                hash,
                size: e.size,
                executable: e.executable,
                chunked: e.chunked,
            });
        }
        if entries.windows(2).any(|w| w[0].path >= w[1].path) {
            return Err(SdkError::Serialization(
                "manifest entries are not sorted and unique".into(),
            ));
        }
        Ok(Self { entries })
    }

    /// BLAKE3 of `to_bytes()`.
    pub fn hash(&self) -> ObjectHash {
        crate::store::hash_bytes(&self.to_bytes())
    }
}

/// Check that a manifest path is a safe, normalized, '/'-separated relative
/// path: non-empty, no leading '/', no empty, `.` or `..` components, no
/// backslashes or NUL bytes, and never inside `.wt/`. A `.wt-tree/`
/// component is only allowed as the first component, for the tree's own
/// shared configuration ([`is_shared_tree_config_path`]); a `.wt-tree/`
/// deeper down belongs to a nested tree and is rejected.
pub fn validate_manifest_path(path: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(SdkError::InvalidArgument(format!(
            "invalid path {path:?}: {why}"
        )))
    };
    if path.is_empty() {
        return bad("empty");
    }
    if path.contains('\\') || path.contains('\0') {
        return bad("contains a backslash or NUL byte");
    }
    for (i, component) in path.split('/').enumerate() {
        match component {
            "" => return bad("empty component or leading/trailing '/'"),
            "." | ".." => return bad("relative component"),
            ".wt" => return bad("reserved directory"),
            // hardening (§15.10): the tree's own `.wt-tree/` is shared.
            ".wt-tree" if i == 0 && is_shared_tree_config_path(path) => {}
            ".wt-tree" if i == 0 => {
                return bad("only .wt-tree/{config.toml,ignore,branches.toml,protections.toml,hooks/**,access/**} are tracked")
            }
            ".wt-tree" => return bad("reserved directory (a nested tree's marker)"),
            _ => {}
        }
    }
    Ok(())
}

// ---- hardening (ENGINEERING_PLAN §15.10): shared `.wt-tree/` files ----

/// Whether a tree-relative path names one of the tree's shared `.wt-tree/`
/// files: `.wt-tree/config.toml`, `.wt-tree/ignore`, `.wt-tree/branches.toml`,
/// `.wt-tree/protections.toml` (config-as-code, §16.3), or anything below
/// `.wt-tree/hooks/` or `.wt-tree/access/`. These are snapshotted, pushed
/// and cloned with the tree; everything else in `.wt-tree/` (the local
/// identity file `local.toml`) stays local. Components are not otherwise
/// validated here (see [`validate_manifest_path`]).
pub fn is_shared_tree_config_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(".wt-tree/") else {
        return false;
    };
    match rest.split_once('/') {
        // config-as-code (§16.3): branches.toml and protections.toml too.
        None => matches!(
            rest,
            "config.toml" | "ignore" | "branches.toml" | "protections.toml"
        ),
        Some((dir, sub)) => {
            (dir == "hooks" || dir == "access")
                && !sub.is_empty()
                && !sub.split('/').any(|c| c == ".wt" || c == ".wt-tree")
        }
    }
}

/// Whether a tree-relative path is a shared hook script
/// (`.wt-tree/hooks/...`).
pub fn is_shared_tree_hook_path(path: &str) -> bool {
    is_shared_tree_config_path(path) && path.starts_with(".wt-tree/hooks/")
}

fn hash_to_wire(hash: &ObjectHash) -> wire::ContentHash {
    wire::ContentHash {
        value: hash.as_bytes().to_vec(),
    }
}

fn hash_from_wire(hash: &wire::ContentHash, field: &str) -> Result<ObjectHash> {
    let bytes: [u8; 32] = hash.value.as_slice().try_into().map_err(|_| {
        SdkError::Serialization(format!(
            "{field}: expected 32 hash bytes, got {}",
            hash.value.len()
        ))
    })?;
    Ok(ObjectHash::from_bytes(bytes))
}

/// An immutable point-in-time record of a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    /// Stable tree id (UUID string); also the server's tree id once linked.
    pub tree_id: String,
    pub branch: String,
    pub parents: Vec<SnapshotId>,
    pub manifest: ObjectHash,
    pub author: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub auto_generated: bool,
    /// Declared dependencies on other trees/branches (become TODOs on push).
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// Set on revert snapshots: the snapshot whose changes this undoes.
    #[serde(default)]
    pub reverts: Option<SnapshotId>,
    /// Ed25519 signature (ENGINEERING_PLAN §13.6); None = unsigned.
    #[serde(default)]
    pub signature: Option<SnapshotSignature>,
}

/// A dependency declared on a snapshot (`wt snapshot --depend tree[@branch]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Target tenant slug; empty = same tenant as the dependent tree.
    #[serde(default)]
    pub tenant: String,
    pub tree: String,
    /// Empty = the target tree's default branch.
    #[serde(default)]
    pub branch: String,
    pub title: String,
    #[serde(default)]
    pub details: String,
    /// "low" | "medium" | "high" | "critical" (empty = "medium").
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub blocking: bool,
    #[serde(default)]
    pub linked: bool,
}

impl Dependency {
    pub fn to_wire(&self) -> wire::Dependency {
        wire::Dependency {
            tenant_slug: self.tenant.clone(),
            tree: self.tree.clone(),
            branch: self.branch.clone(),
            title: self.title.clone(),
            details: self.details.clone(),
            priority: self.priority.clone(),
            blocking: self.blocking,
            linked: self.linked,
        }
    }

    pub fn from_wire(w: &wire::Dependency) -> Self {
        Self {
            tenant: w.tenant_slug.clone(),
            tree: w.tree.clone(),
            branch: w.branch.clone(),
            title: w.title.clone(),
            details: w.details.clone(),
            priority: w.priority.clone(),
            blocking: w.blocking,
            linked: w.linked,
        }
    }
}

/// Signature over the snapshot's canonical record (signature fields cleared).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSignature {
    /// Server-side id of the registered signing key.
    pub key_id: String,
    /// Raw 64-byte Ed25519 signature.
    pub signature: Vec<u8>,
}

impl Snapshot {
    /// First 8 characters of the id.
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }

    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Convert to the wire record.
    pub fn to_record(&self) -> worktree_protocol::proto::sync::SnapshotRecord {
        wire::SnapshotRecord {
            id: Some(wire::SnapshotId {
                value: self.id.clone(),
            }),
            tree_id: Some(wire::TreeId {
                value: self.tree_id.clone(),
            }),
            branch_name: self.branch.clone(),
            parents: self
                .parents
                .iter()
                .map(|p| wire::SnapshotId { value: p.clone() })
                .collect(),
            manifest_hash: Some(hash_to_wire(&self.manifest)),
            author: self.author.clone(),
            message: self.message.clone(),
            created_at: Some(prost_types::Timestamp {
                seconds: self.created_at.timestamp(),
                nanos: i32::try_from(self.created_at.timestamp_subsec_nanos()).unwrap_or(0),
            }),
            auto_generated: self.auto_generated,
            dependencies: self.dependencies.iter().map(Dependency::to_wire).collect(),
            reverts: self
                .reverts
                .as_ref()
                .map(|r| wire::SnapshotId { value: r.clone() }),
            signature: self
                .signature
                .as_ref()
                .map(|s| s.signature.clone())
                .unwrap_or_default(),
            signing_key_id: self
                .signature
                .as_ref()
                .map(|s| s.key_id.clone())
                .unwrap_or_default(),
        }
    }

    /// Convert from the wire record. `Serialization` if a required field is
    /// missing or malformed.
    pub fn from_record(rec: &worktree_protocol::proto::sync::SnapshotRecord) -> Result<Self> {
        let missing = |f: &str| SdkError::Serialization(format!("snapshot record: missing {f}"));
        let id = rec
            .id
            .as_ref()
            .map(|i| i.value.clone())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| missing("id"))?;
        let tree_id = rec
            .tree_id
            .as_ref()
            .map(|t| t.value.clone())
            .unwrap_or_default();
        let manifest = rec
            .manifest_hash
            .as_ref()
            .ok_or_else(|| missing("manifest_hash"))
            .and_then(|h| hash_from_wire(h, "manifest_hash"))?;
        let ts = rec
            .created_at
            .as_ref()
            .ok_or_else(|| missing("created_at"))?;
        let nanos = u32::try_from(ts.nanos)
            .map_err(|_| SdkError::Serialization("snapshot record: negative nanos".into()))?;
        let created_at = DateTime::from_timestamp(ts.seconds, nanos).ok_or_else(|| {
            SdkError::Serialization("snapshot record: created_at out of range".into())
        })?;
        Ok(Self {
            id,
            tree_id,
            branch: rec.branch_name.clone(),
            parents: rec.parents.iter().map(|p| p.value.clone()).collect(),
            manifest,
            author: rec.author.clone(),
            message: rec.message.clone(),
            created_at,
            auto_generated: rec.auto_generated,
            dependencies: rec.dependencies.iter().map(Dependency::from_wire).collect(),
            reverts: rec
                .reverts
                .as_ref()
                .map(|r| r.value.clone())
                .filter(|v| !v.is_empty()),
            signature: (!rec.signature.is_empty()).then(|| SnapshotSignature {
                key_id: rec.signing_key_id.clone(),
                signature: rec.signature.clone(),
            }),
        })
    }
}

/// A tree (independently versioned sub-project) inside the worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeInfo {
    pub name: String,
    pub id: String,
    /// Path relative to the worktree root ("" for the root tree).
    pub path: PathBuf,
    pub current_branch: String,
    pub default_branch: String,
    /// Name of the tree on the server (defaults to `name`).
    pub remote_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub tip: Option<SnapshotId>,
    pub is_current: bool,
    pub created_at: DateTime<Utc>,
    /// Soft-deleted ("archived") branches are hidden from listings by default.
    pub archived: bool,
    /// Tip of `refs/remotes/origin/<name>` if known.
    pub upstream_tip: Option<SnapshotId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub target: SnapshotId,
    pub message: Option<String>,
    pub tagger: String,
    pub created_at: DateTime<Utc>,
    // v0.7 trust (ENGINEERING_PLAN §15.8): Ed25519 signature over the
    // canonical TagRecord (`signing::tag_signed_payload`); None = unsigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SnapshotSignature>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub tree: String,
    pub branch: String,
    pub head: Option<SnapshotId>,
    /// Working directory vs. head, sorted by path.
    pub changes: Vec<FileChange>,
    pub merge: Option<MergeState>,
}

impl StatusReport {
    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOptions {
    pub message: String,
    /// Overrides the configured author.
    pub author: Option<String>,
    pub auto_generated: bool,
    /// Create a snapshot even if nothing changed.
    pub allow_empty: bool,
    /// Skip the pre-snapshot secret scan.
    pub skip_secret_scan: bool,
    /// Dependencies to record on the snapshot (`wt snapshot --depend`).
    pub dependencies: Vec<Dependency>,
    /// signing (§13.6): sign this snapshot even when `[signing] auto` is
    /// off (`wt snapshot --sign`).
    #[serde(default)]
    pub sign: bool,
    /// local-config (§13.9): skip pre/post-snapshot hooks (`--no-verify`).
    #[serde(default)]
    pub skip_hooks: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchOptions {
    /// Create the branch at the current head first.
    pub create: bool,
    /// Discard uncommitted changes.
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeOptions {
    pub message: Option<String>,
    pub no_fast_forward: bool,
    pub author: Option<String>,
    /// How conflicting changes are resolved (default: conflict markers).
    #[serde(default)]
    pub strategy: MergeStrategy,
}

/// How a three-way merge resolves changes that conflict
/// (`wt merge --strategy`). Non-conflicting changes from both sides are
/// always combined; the strategy only decides overlapping hunks and
/// file-level conflicts (modify/delete, binary, add/add).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStrategy {
    /// Write diff3 conflict markers and stop for the user to resolve.
    #[default]
    Manual,
    /// Resolve every conflict in favour of the current branch.
    Ours,
    /// Resolve every conflict in favour of the merged source.
    Theirs,
}

impl MergeStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Ours => "ours",
            Self::Theirs => "theirs",
        }
    }
}

impl std::str::FromStr for MergeStrategy {
    type Err = SdkError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Ok(Self::Manual),
            "ours" => Ok(Self::Ours),
            "theirs" => Ok(Self::Theirs),
            other => Err(SdkError::InvalidArgument(format!(
                "unknown merge strategy {other:?} (expected ours, theirs or manual)"
            ))),
        }
    }
}

/// One conflicting region of a file, in 1-based line numbers of each side's
/// version of the file. A zero-length side starts at the line before the
/// region (0 at the start of the file), as in unified diffs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictHunk {
    pub ours_start: usize,
    pub ours_len: usize,
    pub theirs_start: usize,
    pub theirs_len: usize,
}

/// How to resolve one conflicted path (`wt merge resolve`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Resolve conflicting hunks with the current branch's side (text files
    /// keep the source's non-conflicting changes); for file-level conflicts
    /// keep (or delete) the current branch's version.
    Ours,
    /// As `Ours`, favouring the merged source.
    Theirs,
    /// Use exactly these bytes as the resolved content.
    Content(Vec<u8>),
}

/// Options for `Repository::revert_with` (`wt revert`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevertOptions {
    /// Message of the revert snapshot (default: `Revert "<subject>"`).
    pub message: Option<String>,
    /// 1-based parent to revert against; required for merge snapshots
    /// (the parent whose line of history is kept).
    pub parent: Option<usize>,
}

/// One conflict of an in-progress merge, as reported by
/// `Repository::merge_status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictStatus {
    pub path: String,
    pub kind: ConflictKind,
    #[serde(default)]
    pub hunks: Vec<ConflictHunk>,
    /// Explicitly resolved (`wt merge resolve`), or a text conflict whose
    /// markers are gone.
    pub resolved: bool,
}

/// An in-progress conflicted merge (`wt merge status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeStatus {
    pub tree: String,
    pub branch: String,
    pub source_branch: String,
    pub source_tip: SnapshotId,
    pub target_tip: Option<SnapshotId>,
    pub base: Option<SnapshotId>,
    pub message: String,
    pub conflicts: Vec<ConflictStatus>,
}

impl MergeStatus {
    /// Paths that still block `wt merge --continue` or need attention.
    pub fn unresolved(&self) -> impl Iterator<Item = &ConflictStatus> {
        self.conflicts.iter().filter(|c| !c.resolved)
    }
}

/// Result of `Repository::expire_reflog`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogExpireReport {
    pub removed: usize,
    pub kept: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Both sides changed overlapping lines; markers written to the file.
    Content,
    /// One side modified, the other deleted.
    ModifyDelete,
    /// Both sides added the path with different content.
    AddAdd,
    /// Binary file changed on both sides; "ours" kept in the working tree.
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictInfo {
    pub path: String,
    pub kind: ConflictKind,
    /// Conflicting regions of a text conflict (empty for file-level
    /// conflicts and records written before v0.5).
    #[serde(default)]
    pub hunks: Vec<ConflictHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // returned once per merge; boxing buys nothing
pub enum MergeOutcome {
    UpToDate,
    FastForward {
        to: SnapshotId,
    },
    Merged {
        snapshot: Snapshot,
    },
    /// Working tree contains the partial merge; `.wt/conflicts/` describes it.
    Conflicts {
        conflicts: Vec<ConflictInfo>,
    },
}

/// Persisted while a conflicted merge awaits resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeState {
    pub source_branch: String,
    pub source_tip: SnapshotId,
    pub target_tip: Option<SnapshotId>,
    pub base: Option<SnapshotId>,
    pub conflicts: Vec<ConflictInfo>,
    pub message: String,
    /// Paths resolved with `wt merge resolve` (never re-checked for markers).
    #[serde(default)]
    pub resolved: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffOptions {
    /// Lines of context in patches.
    pub context: usize,
    /// Skip patch generation.
    pub name_only: bool,
    /// Restrict to these paths (prefix match); empty = all.
    pub paths: Vec<String>,
    /// Detect renames: deleted/added pairs with identical content, or whose
    /// line similarity is at least this percentage (0-100). `None` = off.
    #[serde(default)]
    pub find_renames: Option<u8>,
    /// Patches mark changed words inline (`[-old-]{+new+}`) instead of
    /// whole lines.
    #[serde(default)]
    pub word_diff: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            context: 3,
            name_only: false,
            paths: Vec::new(),
            find_renames: None,
            word_diff: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub kind: ChangeKind,
    pub old_hash: Option<ObjectHash>,
    pub new_hash: Option<ObjectHash>,
    pub binary: bool,
    /// Unified diff text (None when name_only or binary).
    pub patch: Option<String>,
    pub added: usize,
    pub removed: usize,
    /// Set when the file was renamed (or moved) from this path; `kind` is
    /// then `Modified` and `path` is the new path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// Similarity percentage of a detected rename (100 = identical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogEntry {
    pub at: DateTime<Utc>,
    pub tree: String,
    pub branch: String,
    /// "init", "snapshot", "auto-snapshot", "switch", "branch.create",
    /// "branch.archive", "merge", "fast-forward", "revert", "restore",
    /// "pull", "push", "tag.create", "tag.delete".
    pub action: String,
    pub old: Option<SnapshotId>,
    pub new: Option<SnapshotId>,
    pub message: String,
}

// ---- v0.6 blame workstream (docs/ENGINEERING_PLAN.md §14.4) ----

/// One line of a file attributed to the snapshot that last changed it
/// (`Repository::blame`). Mirrors the server's `domain.BlameLine`, with
/// `created_at` in place of the server's `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// 1-based line number in the blamed revision.
    pub line: usize,
    /// The line without its `\n` (or `\r\n`) terminator.
    pub text: String,
    pub snapshot_id: SnapshotId,
    pub author: String,
    /// First line of the snapshot's message.
    pub message: String,
    /// When the snapshot was created (JSON `at`, like the server's BlameLine).
    #[serde(rename = "at", alias = "created_at")]
    pub created_at: DateTime<Utc>,
}

// ---- end v0.6 blame workstream ----

/// Options for `Repository::init`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InitOptions {
    /// Name of the worktree and its root tree (default: directory name).
    pub name: Option<String>,
    /// Default branch (default: "main").
    pub default_branch: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::hash_bytes;

    fn entry(path: &str, content: &[u8]) -> ManifestEntry {
        ManifestEntry {
            path: path.into(),
            hash: hash_bytes(content),
            size: content.len() as u64,
            executable: false,
            chunked: false,
        }
    }

    #[test]
    fn from_entries_sorts_and_last_wins() {
        let m = Manifest::from_entries(vec![
            entry("b.txt", b"1"),
            entry("a.txt", b"2"),
            entry("b.txt", b"3"),
        ]);
        let paths: Vec<_> = m.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt"]);
        assert_eq!(m.get("b.txt").unwrap().hash, hash_bytes(b"3"));
        assert!(m.get("c.txt").is_none());
    }

    #[test]
    fn manifest_bytes_roundtrip_and_deterministic() {
        let mut e = entry("dir/x.sh", b"#!/bin/sh");
        e.executable = true;
        let m = Manifest::from_entries(vec![e, entry("a", b""), entry("z/y", b"y")]);
        let bytes = m.to_bytes();
        assert_eq!(bytes, m.clone().to_bytes());
        let back = Manifest::from_bytes(&bytes).unwrap();
        assert_eq!(back, m);
        assert_eq!(m.hash(), hash_bytes(&bytes));
        assert_eq!(Manifest::default().to_bytes(), Vec::<u8>::new());
    }

    #[test]
    fn from_bytes_rejects_unsorted_and_unsafe() {
        use prost::Message;
        let mk = |paths: &[&str]| {
            wire::ManifestRecord {
                entries: paths
                    .iter()
                    .map(|p| wire::ManifestEntry {
                        path: (*p).into(),
                        blob_hash: Some(hash_to_wire(&hash_bytes(b"x"))),
                        size: 1,
                        executable: false,
                        chunked: false,
                    })
                    .collect(),
            }
            .encode_to_vec()
        };
        assert!(Manifest::from_bytes(&mk(&["b", "a"])).is_err());
        assert!(Manifest::from_bytes(&mk(&["a", "a"])).is_err());
        assert!(Manifest::from_bytes(&mk(&["../evil"])).is_err());
        assert!(Manifest::from_bytes(&mk(&[".wt/config.toml"])).is_err());
        assert!(Manifest::from_bytes(&mk(&["/abs"])).is_err());
        assert!(Manifest::from_bytes(b"\xff\xff\xff").is_err());
        assert!(Manifest::from_bytes(&mk(&["a", "b/c"])).is_ok());
    }

    #[test]
    fn validate_manifest_path_rules() {
        for ok in ["a", "a/b.txt", ".wtignore", "x/.hidden", "a..b"] {
            assert!(validate_manifest_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/a",
            "a/",
            "a//b",
            "./a",
            "a/../b",
            "a\\b",
            ".wt",
            "x/.wt-tree/y",
        ] {
            assert!(validate_manifest_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn shared_tree_config_paths() {
        // hardening (§15.10): the tree's own `.wt-tree/` files are tracked.
        for ok in [
            ".wt-tree/config.toml",
            ".wt-tree/ignore",
            ".wt-tree/hooks/pre-snapshot",
            ".wt-tree/hooks/lib/common.sh",
            ".wt-tree/access/policies.toml",
        ] {
            assert!(is_shared_tree_config_path(ok), "{ok}");
            assert!(validate_manifest_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            ".wt-tree",
            ".wt-tree/",
            ".wt-tree/local.toml",
            ".wt-tree/other/x",
            ".wt-tree/hooks",
            ".wt-tree/hooks/",
            ".wt-tree/hooks/.wt/x",
            ".wt-tree/access/.wt-tree/x",
            ".wt-tree/.wt-tree/config.toml",
            "a/.wt-tree/config.toml",
            "a/.wt-tree/hooks/pre-snapshot",
            ".wt/config.toml",
        ] {
            assert!(validate_manifest_path(bad).is_err(), "{bad}");
        }
        assert!(is_shared_tree_hook_path(".wt-tree/hooks/post-snapshot"));
        assert!(!is_shared_tree_hook_path(".wt-tree/access/policies.toml"));
        assert!(!is_shared_tree_hook_path("hooks/x"));
    }

    #[test]
    fn snapshot_record_roundtrip() {
        let snap = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: uuid::Uuid::new_v4().to_string(),
            branch: "main".into(),
            parents: vec!["p1".into(), "p2".into()],
            manifest: hash_bytes(b"manifest"),
            author: "Ada <ada@example.com>".into(),
            message: "msg".into(),
            created_at: DateTime::from_timestamp(1_700_000_000, 123_456_789).unwrap(),
            auto_generated: true,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        };
        let rec = snap.to_record();
        assert_eq!(rec.parents.len(), 2);
        assert_eq!(Snapshot::from_record(&rec).unwrap(), snap);
        assert!(snap.is_merge());
        assert_eq!(snap.short_id().len(), 8);
    }

    #[test]
    fn snapshot_record_requires_fields() {
        let snap = Snapshot {
            id: "abc".into(),
            tree_id: "t".into(),
            branch: "main".into(),
            parents: vec![],
            manifest: hash_bytes(b"m"),
            author: String::new(),
            message: String::new(),
            created_at: Utc::now(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
        };
        let mut rec = snap.to_record();
        rec.manifest_hash = Some(wire::ContentHash {
            value: vec![1, 2, 3],
        });
        assert!(Snapshot::from_record(&rec).is_err());
        let mut rec = snap.to_record();
        rec.id = None;
        assert!(Snapshot::from_record(&rec).is_err());
        let mut rec = snap.to_record();
        rec.created_at = None;
        assert!(Snapshot::from_record(&rec).is_err());
    }

    #[test]
    fn config_as_code_paths_are_shared() {
        // config-as-code (§16.3): branch metadata and protections travel too.
        for ok in [".wt-tree/branches.toml", ".wt-tree/protections.toml"] {
            assert!(is_shared_tree_config_path(ok), "{ok}");
            assert!(validate_manifest_path(ok).is_ok(), "{ok}");
        }
        for bad in [".wt-tree/branches.toml/x", "a/.wt-tree/protections.toml"] {
            assert!(validate_manifest_path(bad).is_err(), "{bad}");
        }
    }
}
