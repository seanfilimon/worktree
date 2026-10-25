//! Snapshot records, manifests, revision resolution and graph traversal.

use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use super::{fsutil, names, Repository};
use crate::error::{Result, SdkError};
use crate::model::{Manifest, Snapshot, SnapshotId};
use crate::store::ObjectHash;

/// Minimum length of an abbreviated snapshot id.
const MIN_PREFIX_LEN: usize = 4;

/// One step of a revision suffix: `~N` (N first-parent steps) or `^N`
/// (the N-th parent, 1-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevStep {
    Ancestor(usize),
    Parent(usize),
}

impl Repository {
    // ----------------------------------------------------------------
    // Snapshots
    // ----------------------------------------------------------------

    /// Snapshots reachable from `rev` (default: current branch head), newest
    /// first, at most `limit`. Empty if the branch has no snapshots yet.
    pub fn log(
        &self,
        tree: &str,
        rev: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Snapshot>> {
        self.load_tree(tree)?;
        let start = match rev {
            Some(rev) => self.resolve(tree, rev)?,
            None => match self.head_tip(tree)?.1 {
                Some(id) => self.get_snapshot(&id)?,
                None => return Ok(Vec::new()),
            },
        };
        let limit = limit.unwrap_or(usize::MAX);
        let mut out = Vec::new();
        let mut seen = HashSet::from([start.id.clone()]);
        let mut heap = BinaryHeap::from([ByTime(start)]);
        while let Some(ByTime(snap)) = heap.pop() {
            if out.len() >= limit {
                break;
            }
            for parent in &snap.parents {
                if seen.insert(parent.clone()) {
                    if let Some(p) = self.load_snapshot_opt(parent)? {
                        heap.push(ByTime(p));
                    }
                }
            }
            out.push(snap);
        }
        Ok(out)
    }

    /// Load a snapshot by full id or unique prefix (>= 4 chars).
    pub fn get_snapshot(&self, id: &str) -> Result<Snapshot> {
        if let Some(snap) = self.load_snapshot_opt(id)? {
            return Ok(snap);
        }
        let prefix = id.to_ascii_lowercase();
        if prefix.len() < MIN_PREFIX_LEN || !is_valid_id(&prefix) {
            return Err(SdkError::SnapshotNotFound(id.to_string()));
        }
        let mut matches = Vec::new();
        let entries = match fs::read_dir(self.snapshots_dir()) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SdkError::SnapshotNotFound(id.to_string()))
            }
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(candidate) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
                continue;
            };
            if candidate.to_ascii_lowercase().starts_with(&prefix) {
                matches.push(candidate.to_string());
                if matches.len() > 1 {
                    return Err(SdkError::AmbiguousId(id.to_string()));
                }
            }
        }
        match matches.pop() {
            Some(full) => self
                .load_snapshot_opt(&full)?
                .ok_or_else(|| SdkError::SnapshotNotFound(id.to_string())),
            None => Err(SdkError::SnapshotNotFound(id.to_string())),
        }
    }

    /// Whether a snapshot with exactly this id is stored.
    pub fn has_snapshot(&self, id: &str) -> bool {
        is_valid_id(id) && self.snapshot_path(id).is_file()
    }

    /// Resolve a revision in `tree`: "HEAD", "HEAD~N", "<branch>",
    /// "<branch>~N", "origin/<branch>", "<tag>", or an id prefix. `~N`
    /// follows first parents, `^N` selects the N-th parent (`^` = `^1`);
    /// suffixes may be chained (`main~2^2`). Reflog revisions: `@{N}` is
    /// the N-th newest reflog entry of the current branch, `BRANCH@{N}` of
    /// that branch and `HEAD@{N}` of the whole tree (`main@{1}~2` works).
    pub fn resolve(&self, tree: &str, rev: &str) -> Result<Snapshot> {
        self.load_tree(tree)?;
        let rev = rev.trim();
        let (base, steps) = parse_rev(rev)?;
        let mut snap = self.resolve_base(tree, base, rev)?;
        for step in steps {
            let (count, nth) = match step {
                RevStep::Ancestor(n) => (n, 0),
                RevStep::Parent(0) => continue,
                RevStep::Parent(n) => (1, n - 1),
            };
            for _ in 0..count {
                let parent = snap.parents.get(nth).ok_or_else(|| {
                    SdkError::InvalidRevision(format!(
                        "{rev}: snapshot {} has no such parent",
                        snap.short_id()
                    ))
                })?;
                snap = self.get_snapshot(parent)?;
            }
        }
        Ok(snap)
    }

    /// The manifest a snapshot points at.
    pub fn manifest_of(&self, snapshot: &Snapshot) -> Result<Manifest> {
        self.read_manifest(&snapshot.manifest)
    }

    /// Content of `path` in `snapshot`.
    pub fn read_file_at(&self, snapshot: &Snapshot, path: &str) -> Result<Vec<u8>> {
        let manifest = self.manifest_of(snapshot)?;
        let rel = names::normalize_rel_path(path);
        let entry = manifest.get(&rel).ok_or_else(|| {
            SdkError::InvalidArgument(format!(
                "{rel} does not exist in snapshot {}",
                snapshot.short_id()
            ))
        })?;
        self.store.read_entry(entry)
    }

    // ----------------------------------------------------------------
    // Plumbing
    // ----------------------------------------------------------------

    /// Store a manifest object, returning its hash.
    pub fn write_manifest(&self, manifest: &Manifest) -> Result<ObjectHash> {
        self.store.put(&manifest.to_bytes())
    }

    /// Load and decode a manifest object.
    pub fn read_manifest(&self, hash: &ObjectHash) -> Result<Manifest> {
        let data = self.store.get(hash)?;
        Manifest::from_bytes(&data)
            .map_err(|e| SdkError::CorruptObject(hash.to_hex(), e.to_string()))
    }

    /// Persist a snapshot record (idempotent). Does not move any ref.
    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if !is_valid_id(&snapshot.id) {
            return Err(SdkError::InvalidArgument(format!(
                "invalid snapshot id {:?}",
                snapshot.id
            )));
        }
        let path = self.snapshot_path(&snapshot.id);
        if path.is_file() {
            return Ok(());
        }
        fsutil::write_json(&path, snapshot)
    }

    /// All snapshots reachable from `tip`, parents before children.
    /// Parents that are not stored locally (shallow history) are skipped.
    pub fn ancestry(&self, tip: &str) -> Result<Vec<Snapshot>> {
        let tip = self.get_snapshot(tip)?;
        let mut order = Vec::new();
        let mut visited = HashSet::from([tip.id.clone()]);
        // (snapshot, index of the next parent to visit)
        let mut stack = vec![(tip, 0usize)];
        while let Some(top) = stack.last_mut() {
            let next = top.0.parents.get(top.1).cloned();
            top.1 += 1;
            match next {
                Some(parent) => {
                    if visited.insert(parent.clone()) {
                        if let Some(p) = self.load_snapshot_opt(&parent)? {
                            stack.push((p, 0));
                        }
                    }
                }
                None => {
                    if let Some((snap, _)) = stack.pop() {
                        order.push(snap);
                    }
                }
            }
        }
        Ok(order)
    }

    /// Whether `ancestor` is reachable from `descendant` (a snapshot is its
    /// own ancestor).
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let ancestor = self.get_snapshot(ancestor)?.id;
        let descendant = self.get_snapshot(descendant)?;
        let mut seen = HashSet::from([descendant.id.clone()]);
        let mut queue = VecDeque::from([descendant]);
        while let Some(snap) = queue.pop_front() {
            if snap.id == ancestor {
                return Ok(true);
            }
            for parent in &snap.parents {
                if seen.insert(parent.clone()) {
                    if let Some(p) = self.load_snapshot_opt(parent)? {
                        queue.push_back(p);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Best common ancestor of two snapshots (see `crate::merge::merge_base`).
    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<SnapshotId>> {
        let a_id = self.get_snapshot(a)?.id;
        let b_id = self.get_snapshot(b)?.id;
        let mut parents: HashMap<SnapshotId, Vec<SnapshotId>> = HashMap::new();
        for tip in [&a_id, &b_id] {
            for snap in self.ancestry(tip)? {
                parents.entry(snap.id).or_insert(snap.parents);
            }
        }
        Ok(crate::merge::merge_base(&a_id, &b_id, &parents))
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    fn snapshot_path(&self, id: &str) -> PathBuf {
        self.snapshots_dir().join(format!("{id}.json"))
    }

    /// Load a snapshot by exact id; `None` if absent or not a valid id.
    pub(crate) fn load_snapshot_opt(&self, id: &str) -> Result<Option<Snapshot>> {
        if !is_valid_id(id) {
            return Ok(None);
        }
        fsutil::read_json_opt(&self.snapshot_path(id))
    }

    /// Manifest of an optional tip (empty for `None`).
    pub(crate) fn manifest_of_tip(&self, tip: Option<&str>) -> Result<Manifest> {
        match tip {
            Some(id) => {
                let snap = self.get_snapshot(id)?;
                self.manifest_of(&snap)
            }
            None => Ok(Manifest::default()),
        }
    }

    fn resolve_base(&self, tree: &str, base: &str, rev: &str) -> Result<Snapshot> {
        let empty_branch = |branch: &str| {
            SdkError::InvalidRevision(format!("{rev}: branch {branch} has no snapshots yet"))
        };
        if let Some(parsed) = super::reflog::parse_reflog_rev(base) {
            let (name, n) = parsed.map_err(|()| {
                SdkError::InvalidRevision(format!("{rev}: expected NAME@{{N}} with a number N"))
            })?;
            return self.resolve_reflog_rev(tree, name, n, rev);
        }
        if base == "HEAD" || base == "@" {
            let (branch, tip) = self.head_tip(tree)?;
            let id = tip.ok_or_else(|| empty_branch(&branch))?;
            return self.get_snapshot(&id);
        }
        if names::validate_ref_name("branch", base).is_ok() {
            if self.branch_exists(tree, base) {
                let id = self
                    .read_tip(tree, base)?
                    .ok_or_else(|| empty_branch(base))?;
                return self.get_snapshot(&id);
            }
            if let Some(tag) = self.find_tag(tree, base)? {
                return self.get_snapshot(&tag.target);
            }
            if let Some((remote, branch)) = base.split_once('/') {
                if names::validate_remote_name(remote).is_ok() {
                    if let Some(id) = self.remote_tip(tree, remote, branch)? {
                        return self.get_snapshot(&id);
                    }
                }
            }
        }
        if is_valid_id(base) && base.len() >= MIN_PREFIX_LEN {
            return match self.get_snapshot(base) {
                Err(SdkError::SnapshotNotFound(_)) => {
                    Err(SdkError::InvalidRevision(rev.to_string()))
                }
                other => other,
            };
        }
        Err(SdkError::InvalidRevision(rev.to_string()))
    }
}

/// Snapshot ids are UUIDs; accept any short alphanumeric/dash/underscore
/// token so foreign ids (e.g. imported) work, while keeping path traversal
/// impossible.
pub(crate) fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Split `base~2^2` into `("base", [Ancestor(2), Parent(2)])`.
fn parse_rev(rev: &str) -> Result<(&str, Vec<RevStep>)> {
    let invalid = || SdkError::InvalidRevision(rev.to_string());
    let split = rev.find(['~', '^']).unwrap_or(rev.len());
    let (base, mut rest) = rev.split_at(split);
    if base.is_empty() {
        return Err(invalid());
    }
    let mut steps = Vec::new();
    while let Some(op) = rest.chars().next() {
        rest = &rest[op.len_utf8()..];
        let digits_len = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let (digits, tail) = rest.split_at(digits_len);
        rest = tail;
        let n = if digits.is_empty() {
            1
        } else {
            digits.parse().map_err(|_| invalid())?
        };
        steps.push(match op {
            '~' => RevStep::Ancestor(n),
            '^' => RevStep::Parent(n),
            _ => return Err(invalid()),
        });
    }
    Ok((base, steps))
}

/// Max-heap ordering by creation time (newest first), ties by id.
struct ByTime(Snapshot);

impl ByTime {
    fn key(&self) -> (DateTime<Utc>, &str) {
        (self.0.created_at, self.0.id.as_str())
    }
}

impl PartialEq for ByTime {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for ByTime {}

impl PartialOrd for ByTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ByTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rev_suffixes() {
        assert_eq!(parse_rev("HEAD").unwrap(), ("HEAD", vec![]));
        assert_eq!(
            parse_rev("HEAD~3").unwrap(),
            ("HEAD", vec![RevStep::Ancestor(3)])
        );
        assert_eq!(
            parse_rev("main~").unwrap(),
            ("main", vec![RevStep::Ancestor(1)])
        );
        assert_eq!(
            parse_rev("feature/x^2~1").unwrap(),
            ("feature/x", vec![RevStep::Parent(2), RevStep::Ancestor(1)])
        );
        assert_eq!(parse_rev("a^").unwrap(), ("a", vec![RevStep::Parent(1)]));
        assert!(parse_rev("~1").is_err());
        assert!(parse_rev("HEAD~x").is_err());
    }

    #[test]
    fn valid_ids() {
        assert!(is_valid_id("0f1e2d3c-aaaa-bbbb-cccc-000000000000"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("../x"));
        assert!(!is_valid_id("a/b"));
    }
}
