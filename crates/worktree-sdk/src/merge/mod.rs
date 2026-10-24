//! Pure merge algorithms (no I/O). Used by `Repository::merge`.
//!
//! * [`merge_text`] — diff3 three-way line merge with conflict markers.
//! * [`merge_manifests`] — per-path three-way merge of file listings.
//! * [`merge_base`] — best common ancestor in a snapshot DAG.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::diff::{match_lines, split_lines, strip_newline};
use crate::model::{ConflictKind, Manifest, ManifestEntry, SnapshotId};

/// Labels used in conflict markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeLabels {
    pub ours: String,   // e.g. "main"
    pub base: String,   // e.g. "base"
    pub theirs: String, // e.g. "feature"
}

impl Default for MergeLabels {
    fn default() -> Self {
        Self {
            ours: "ours".into(),
            base: "base".into(),
            theirs: "theirs".into(),
        }
    }
}

/// One conflicting region of a [`TextMergeResult::Conflicted`] merge. The
/// line vectors hold each side's lines without their `\n` terminators (a
/// CRLF `\r` is kept), as in [`crate::diff::DiffLine`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRegion {
    /// 1-based line in the merged output where `<<<<<<<` appears.
    pub start_line: usize,
    pub ours: Vec<String>,
    pub base: Vec<String>,
    pub theirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextMergeResult {
    Clean(String),
    /// `merged` contains diff3-style markers:
    /// `<<<<<<< ours` / `||||||| base` / `=======` / `>>>>>>> theirs`.
    Conflicted {
        merged: String,
        conflicts: Vec<ConflictRegion>,
    },
}

/// Line-based three-way merge (diff3 semantics). Non-overlapping changes
/// from both sides are combined; identical changes on both sides are taken
/// once.
///
/// The base is aligned with each side by a longest-common-subsequence
/// match. Base lines matched on *both* sides are stable; the stretches
/// between stable lines are resolved as follows: unchanged on one side →
/// take the other side; changed identically → take it once; otherwise
/// conflict. As in Git, edits that touch adjacent lines (with no stable line
/// between them) conflict. Lines that both sides agree on at the start or
/// end of a conflicting stretch are moved out of the markers (Git's
/// `zdiff3`), while the `|||||||` section always shows the full base.
///
/// Marker lines end with `\r\n` when the first line of `ours` (or, if ours
/// is empty, `theirs`, then `base`) does, and `\n` otherwise; a conflicting
/// section whose last line lacks a newline gets one so markers always start
/// a line. For a two-way merge (add/add) pass `""` as `base`.
pub fn merge_text(base: &str, ours: &str, theirs: &str, labels: &MergeLabels) -> TextMergeResult {
    let base_lines = split_lines(base);
    let our_lines = split_lines(ours);
    let their_lines = split_lines(theirs);
    let ours_of = base_to_side(&base_lines, &our_lines);
    let theirs_of = base_to_side(&base_lines, &their_lines);

    let eol = [&our_lines, &their_lines, &base_lines]
        .iter()
        .find_map(|lines| lines.first())
        .map_or("\n", |first| {
            if first.ends_with("\r\n") {
                "\r\n"
            } else {
                "\n"
            }
        });
    let mut out = MergeOutput {
        text: String::with_capacity(ours.len().max(theirs.len())),
        lines: 0,
        eol,
        labels,
        conflicts: Vec::new(),
    };

    // Positions just past the previous stable line in base/ours/theirs.
    let (mut b, mut o, mut t) = (0, 0, 0);
    for j in 0..=base_lines.len() {
        let stable = if j == base_lines.len() {
            Some((our_lines.len(), their_lines.len()))
        } else {
            ours_of[j].zip(theirs_of[j])
        };
        let Some((oj, tj)) = stable else { continue };
        out.chunk(&base_lines[b..j], &our_lines[o..oj], &their_lines[t..tj]);
        if j < base_lines.len() {
            out.push_line(base_lines[j]);
        }
        (b, o, t) = (j + 1, oj + 1, tj + 1);
    }

    if out.conflicts.is_empty() {
        TextMergeResult::Clean(out.text)
    } else {
        TextMergeResult::Conflicted {
            merged: out.text,
            conflicts: out.conflicts,
        }
    }
}

/// For each base line, the index of the line it is matched with in `side`.
fn base_to_side(base: &[&str], side: &[&str]) -> Vec<Option<usize>> {
    let mut map = vec![None; base.len()];
    for (i, j) in match_lines(base, side) {
        map[i] = Some(j);
    }
    map
}

/// Accumulates merged text and conflict records.
struct MergeOutput<'a> {
    text: String,
    /// Number of complete lines in `text`.
    lines: usize,
    eol: &'static str,
    labels: &'a MergeLabels,
    conflicts: Vec<ConflictRegion>,
}

impl MergeOutput<'_> {
    fn push_line(&mut self, line: &str) {
        self.text.push_str(line);
        self.lines += 1;
    }

    /// Pushes `line`, terminating it with `eol` if it has no newline.
    fn push_terminated(&mut self, line: &str) {
        self.push_line(line);
        if !line.ends_with('\n') {
            self.text.push_str(self.eol);
        }
    }

    fn push_marker(&mut self, marker: &str, label: &str) {
        self.text.push_str(marker);
        if !label.is_empty() {
            self.text.push(' ');
            self.text.push_str(label);
        }
        self.text.push_str(self.eol);
        self.lines += 1;
    }

    /// Resolves one unstable stretch between stable lines.
    fn chunk(&mut self, base: &[&str], ours: &[&str], theirs: &[&str]) {
        if ours == base || ours == theirs {
            theirs.iter().for_each(|line| self.push_line(line));
        } else if theirs == base {
            ours.iter().for_each(|line| self.push_line(line));
        } else {
            let prefix = ours.iter().zip(theirs).take_while(|(a, b)| a == b).count();
            let max_suffix = ours.len().min(theirs.len()) - prefix;
            let suffix = ours
                .iter()
                .rev()
                .zip(theirs.iter().rev())
                .take(max_suffix)
                .take_while(|(a, b)| a == b)
                .count();
            ours[..prefix].iter().for_each(|line| self.push_line(line));
            self.conflict(
                base,
                &ours[prefix..ours.len() - suffix],
                &theirs[prefix..theirs.len() - suffix],
            );
            ours[ours.len() - suffix..]
                .iter()
                .for_each(|line| self.push_line(line));
        }
    }

    /// Writes a marked-up conflict. `text` always ends with a newline here:
    /// only a file's last line can be unterminated, and such a line is
    /// either part of this conflict or of the common suffix after it.
    fn conflict(&mut self, base: &[&str], ours: &[&str], theirs: &[&str]) {
        let labels = self.labels;
        let start_line = self.lines + 1;
        self.push_marker("<<<<<<<", &labels.ours);
        ours.iter().for_each(|line| self.push_terminated(line));
        self.push_marker("|||||||", &labels.base);
        base.iter().for_each(|line| self.push_terminated(line));
        self.push_marker("=======", "");
        theirs.iter().for_each(|line| self.push_terminated(line));
        self.push_marker(">>>>>>>", &labels.theirs);

        let owned = |lines: &[&str]| -> Vec<String> {
            lines.iter().map(|l| strip_newline(l).to_owned()).collect()
        };
        self.conflicts.push(ConflictRegion {
            start_line,
            ours: owned(ours),
            base: owned(base),
            theirs: owned(theirs),
        });
    }
}

/// What to do with one path when merging manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolution {
    /// Keep ours unchanged (includes "both sides identical").
    KeepOurs(ManifestEntry),
    /// Take theirs (only theirs changed / added).
    TakeTheirs(ManifestEntry),
    /// Delete the path (deleted on one side, unchanged on the other).
    Delete,
    /// Both modified a text file: caller must run `merge_text` on the blobs.
    /// The merged file's executable bit is [`merge_executable`].
    MergeContent {
        base: Option<ManifestEntry>,
        ours: ManifestEntry,
        theirs: ManifestEntry,
    },
    /// Unresolvable without the user.
    Conflict {
        kind: ConflictKind,
        base: Option<ManifestEntry>,
        ours: Option<ManifestEntry>,
        theirs: Option<ManifestEntry>,
    },
}

/// Three-way merge of manifests. Returns one resolution per path present in
/// any input (except paths absent from both ours and theirs), sorted by path.
/// AddAdd with identical hashes is KeepOurs; AddAdd with different content
/// is MergeContent with base None (caller treats it as a 2-way merge and
/// reports ConflictKind::AddAdd if it conflicts).
///
/// Content and the executable bit merge independently: when one side only
/// toggled the executable bit and the other changed the content, the result
/// is the changed content with the toggled bit (`TakeTheirs`/`KeepOurs`
/// carrying a combined entry). A modified-vs-deleted path is a
/// `ModifyDelete` conflict. Binary detection is left to the caller.
pub fn merge_manifests(
    base: &Manifest,
    ours: &Manifest,
    theirs: &Manifest,
) -> Vec<(String, PathResolution)> {
    let (base, ours, theirs) = (by_path(base), by_path(ours), by_path(theirs));
    let paths: BTreeSet<&str> = ours.keys().chain(theirs.keys()).copied().collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let resolution = resolve_path(
                base.get(path).copied(),
                ours.get(path).copied(),
                theirs.get(path).copied(),
            )?;
            Some((path.to_owned(), resolution))
        })
        .collect()
}

/// Indexes a manifest by path without assuming its entries are sorted.
fn by_path(manifest: &Manifest) -> BTreeMap<&str, &ManifestEntry> {
    manifest
        .entries
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect()
}

/// Three-way merge of the executable bit: a side that changed it relative
/// to `base` wins; without a base, or when both sides agree, ours is kept.
pub fn merge_executable(
    base: Option<&ManifestEntry>,
    ours: &ManifestEntry,
    theirs: &ManifestEntry,
) -> bool {
    match base {
        Some(base) if ours.executable == base.executable => theirs.executable,
        _ => ours.executable,
    }
}

/// The resolution table for one path; `None` when it is absent from both
/// ours and theirs.
fn resolve_path(
    base: Option<&ManifestEntry>,
    ours: Option<&ManifestEntry>,
    theirs: Option<&ManifestEntry>,
) -> Option<PathResolution> {
    use PathResolution::*;
    let conflict = |kind| Conflict {
        kind,
        base: base.cloned(),
        ours: ours.cloned(),
        theirs: theirs.cloned(),
    };
    Some(match (base, ours, theirs) {
        (_, None, None) => return None,
        (_, Some(o), Some(t)) if o == t => KeepOurs(o.clone()),
        // Added on one side only.
        (None, Some(o), None) => KeepOurs(o.clone()),
        (None, None, Some(t)) => TakeTheirs(t.clone()),
        // Added on both sides.
        (None, Some(o), Some(t)) if o.hash == t.hash => KeepOurs(o.clone()),
        (None, Some(o), Some(t)) => MergeContent {
            base: None,
            ours: o.clone(),
            theirs: t.clone(),
        },
        // Deleted on one side.
        (Some(b), Some(o), None) if o == b => Delete,
        (Some(b), None, Some(t)) if t == b => Delete,
        (Some(_), Some(_), None) | (Some(_), None, Some(_)) => conflict(ConflictKind::ModifyDelete),
        // Present everywhere.
        (Some(b), Some(o), Some(t)) => {
            let executable = merge_executable(Some(b), o, t);
            if o == b {
                TakeTheirs(t.clone())
            } else if t == b {
                KeepOurs(o.clone())
            } else if o.hash == t.hash || t.hash == b.hash {
                // Content is ours'; only the executable bit can differ.
                KeepOurs(ManifestEntry {
                    executable,
                    ..o.clone()
                })
            } else if o.hash == b.hash {
                TakeTheirs(ManifestEntry {
                    executable,
                    ..t.clone()
                })
            } else {
                MergeContent {
                    base: Some(b.clone()),
                    ours: o.clone(),
                    theirs: t.clone(),
                }
            }
        }
    })
}

/// Best common ancestor of `a` and `b` in a snapshot DAG described by
/// `parents` (id -> parent ids). Picks the ancestor reachable from both that
/// is not an ancestor of another common ancestor; ties broken by the
/// smallest BFS distance from `a`, then by id. `None` if unrelated.
///
/// Every snapshot is its own ancestor, so if one of `a`/`b` is an ancestor
/// of the other it is the result. Ids missing from `parents` are treated as
/// roots. Runs in O(V + E) without recursion.
pub fn merge_base(
    a: &str,
    b: &str,
    parents: &HashMap<SnapshotId, Vec<SnapshotId>>,
) -> Option<SnapshotId> {
    let parents_of = |id: &str| parents.get(id).map(Vec::as_slice).unwrap_or_default();

    // BFS distances from `a`, then the subset also reachable from `b`.
    let from_a = bfs_distances(a, parents_of);
    let from_b = bfs_distances(b, parents_of);
    let common: Vec<&str> = from_a
        .keys()
        .copied()
        .filter(|id| from_b.contains_key(id))
        .collect();

    // Every proper ancestor of a common ancestor is itself common and is
    // dominated; one multi-source walk marks them all.
    let mut dominated: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = common
        .iter()
        .flat_map(|id| parents_of(id))
        .map(String::as_str)
        .collect();
    while let Some(id) = stack.pop() {
        if dominated.insert(id) {
            stack.extend(parents_of(id).iter().map(String::as_str));
        }
    }

    common
        .into_iter()
        .filter(|id| !dominated.contains(id))
        .min_by_key(|id| (from_a[id], *id))
        .map(str::to_owned)
}

/// Shortest parent-edge distance from `start` to each of its ancestors
/// (including `start` itself at distance 0).
fn bfs_distances<'a>(
    start: &'a str,
    parents_of: impl Fn(&str) -> &'a [SnapshotId],
) -> HashMap<&'a str, usize> {
    let mut dist = HashMap::from([(start, 0)]);
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        let next = dist[id] + 1;
        for parent in parents_of(id) {
            dist.entry(parent.as_str()).or_insert_with(|| {
                queue.push_back(parent.as_str());
                next
            });
        }
    }
    dist
}
