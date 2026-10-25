//! Line attribution (`wt blame`, docs/ENGINEERING_PLAN.md §14.4).
//!
//! Every line of a file at a revision is attributed to the snapshot that
//! last changed it. The walk starts at the revision with all requested lines
//! pending and moves pending lines from a snapshot to its parents:
//!
//! - a parent whose entry for the path is identical (same object) takes all
//!   pending lines unchanged, without a diff (parents are tried in order,
//!   first parent first);
//! - otherwise each parent in order (first parent first) takes the pending
//!   lines that the line diff (`diff::match_lines`) matches to one of its
//!   lines; so a line of a merge snapshot goes to the first parent that has
//!   it, which makes lines brought in by a merged branch follow that
//!   branch's history;
//! - lines no parent takes were introduced by the snapshot itself.
//!
//! Snapshots are processed newest first, so lines reaching the same
//! snapshot along different paths are handled together. The walk stops when
//! no line is pending. Renames are not followed: a file that does not exist
//! in a parent gives that parent no lines. Parents missing locally (shallow
//! history) are treated as absent. A parent version that is binary gives
//! that parent no lines; blaming a binary file is an error.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::ops::RangeInclusive;
use std::rc::Rc;

use chrono::{DateTime, Utc};

use super::{names, Repository};
use crate::diff::{is_binary, match_lines, split_lines};
use crate::error::{Result, SdkError};
use crate::model::{BlameLine, ManifestEntry, Snapshot};
use crate::store::ObjectHash;

/// A snapshot waiting to hand its pending lines on.
struct Origin {
    snapshot: Snapshot,
    entry: ManifestEntry,
    /// (0-based line index in this snapshot's version, 0-based final line).
    lines: Vec<(usize, usize)>,
}

/// Heap key: newest first, ties by id (descending) for a stable order.
#[derive(PartialEq, Eq)]
struct Key {
    at: DateTime<Utc>,
    id: String,
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, &self.id).cmp(&(other.at, &other.id))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Per-call caches: the path's entry per manifest and decoded text per
/// object, so shared manifests and unchanged blobs are read once.
struct Walk<'a> {
    repo: &'a Repository,
    path: &'a str,
    entries: HashMap<ObjectHash, Option<ManifestEntry>>,
    texts: HashMap<(ObjectHash, bool), Option<Rc<String>>>,
}

impl Walk<'_> {
    /// The path's manifest entry in `snapshot`, if present.
    fn entry(&mut self, snapshot: &Snapshot) -> Result<Option<ManifestEntry>> {
        if let Some(e) = self.entries.get(&snapshot.manifest) {
            return Ok(e.clone());
        }
        let manifest = self.repo.manifest_of(snapshot)?;
        let entry = manifest.get(self.path).cloned();
        self.entries.insert(snapshot.manifest, entry.clone());
        Ok(entry)
    }

    /// Text of an entry; `None` when the content is binary.
    fn text(&mut self, entry: &ManifestEntry) -> Result<Option<Rc<String>>> {
        let key = (entry.hash, entry.chunked);
        if let Some(t) = self.texts.get(&key) {
            return Ok(t.clone());
        }
        let data = self.repo.store.read_entry(entry)?;
        let text = if is_binary(&data) {
            None
        } else {
            // is_binary guarantees valid UTF-8.
            Some(Rc::new(String::from_utf8(data).unwrap_or_default()))
        };
        self.texts.insert(key, text.clone());
        Ok(text)
    }
}

impl Repository {
    /// Attribute every line of `path` at `rev` (default: HEAD) to the
    /// snapshot that last changed it. See the module docs for the rules.
    /// Errors when the path does not exist at `rev` or is a binary file.
    pub fn blame(&self, tree: &str, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>> {
        self.blame_lines(tree, path, rev, None)
    }

    /// Like [`Repository::blame`], restricted to the 1-based inclusive line
    /// range `lines` (the end is clamped to the file length). Only the
    /// requested lines are traced, so a small range stops early.
    pub fn blame_lines(
        &self,
        tree: &str,
        path: &str,
        rev: Option<&str>,
        lines: Option<RangeInclusive<usize>>,
    ) -> Result<Vec<BlameLine>> {
        self.load_tree(tree)?;
        let start = match rev {
            Some(rev) => self.resolve(tree, rev)?,
            None => {
                let (branch, tip) = self.head_tip(tree)?;
                let id = tip.ok_or_else(|| {
                    SdkError::InvalidRevision(format!("HEAD: branch {branch} has no snapshots yet"))
                })?;
                self.get_snapshot(&id)?
            }
        };
        let rel = names::normalize_rel_path(path);
        let mut walk = Walk {
            repo: self,
            path: &rel,
            entries: HashMap::new(),
            texts: HashMap::new(),
        };
        let entry = walk.entry(&start)?.ok_or_else(|| {
            SdkError::InvalidArgument(format!(
                "{rel} does not exist in snapshot {}",
                start.short_id()
            ))
        })?;
        let text = walk
            .text(&entry)?
            .ok_or_else(|| SdkError::InvalidArgument(format!("{rel} is a binary file")))?;
        let final_lines: Vec<&str> = split_lines(&text);
        let total = final_lines.len();
        let range = match lines {
            None => 0..total,
            Some(r) => {
                let (lo, hi) = (*r.start(), *r.end());
                if lo == 0 || hi < lo {
                    return Err(SdkError::InvalidArgument(format!(
                        "invalid line range {lo},{hi}: lines are numbered from 1"
                    )));
                }
                if lo > total {
                    return Err(SdkError::InvalidArgument(format!(
                        "{rel} has only {total} lines"
                    )));
                }
                lo - 1..hi.min(total)
            }
        };

        // final line index - range.start -> attributed snapshot
        let mut owner: Vec<Option<Rc<Snapshot>>> = vec![None; range.len()];
        let mut pending: HashMap<String, Origin> = HashMap::new();
        let mut heap = BinaryHeap::new();
        if !range.is_empty() {
            heap.push(Key {
                at: start.created_at,
                id: start.id.clone(),
            });
            pending.insert(
                start.id.clone(),
                Origin {
                    snapshot: start.clone(),
                    entry,
                    lines: range.clone().map(|i| (i, i)).collect(),
                },
            );
        }

        while let Some(key) = heap.pop() {
            let Some(origin) = pending.remove(&key.id) else {
                continue;
            };
            let kept = self.pass_to_parents(&mut walk, origin, &mut pending, &mut heap)?;
            if let Some((snapshot, lines)) = kept {
                let snapshot = Rc::new(snapshot);
                for fin in lines {
                    owner[fin - range.start] = Some(snapshot.clone());
                }
            }
        }

        let mut out = Vec::with_capacity(range.len());
        for (i, fin) in range.clone().enumerate() {
            let snap = owner[i].as_ref().ok_or_else(|| {
                SdkError::Other(format!("blame: line {} was not attributed", fin + 1))
            })?;
            out.push(BlameLine {
                line: fin + 1,
                text: strip_eol(final_lines[fin]).to_string(),
                snapshot_id: snap.id.clone(),
                author: snap.author.clone(),
                message: snap.message.lines().next().unwrap_or("").to_string(),
                created_at: snap.created_at,
            });
        }
        Ok(out)
    }

    /// Hands `origin`'s lines to its parents; returns the snapshot with the
    /// final line indices no parent took (or `None` if all were passed on).
    fn pass_to_parents(
        &self,
        walk: &mut Walk<'_>,
        origin: Origin,
        pending: &mut HashMap<String, Origin>,
        heap: &mut BinaryHeap<Key>,
    ) -> Result<Option<(Snapshot, Vec<usize>)>> {
        let mut parents: Vec<(Snapshot, ManifestEntry)> = Vec::new();
        for id in &origin.snapshot.parents {
            if parents.iter().any(|(p, _)| &p.id == id) {
                continue;
            }
            // Missing parents (shallow history) are treated as absent.
            let Some(parent) = self.load_snapshot_opt(id)? else {
                continue;
            };
            if let Some(entry) = walk.entry(&parent)? {
                parents.push((parent, entry));
            }
        }

        let mut send = |parent: &Snapshot, entry: &ManifestEntry, lines: Vec<(usize, usize)>| {
            if lines.is_empty() {
                return;
            }
            match pending.get_mut(&parent.id) {
                Some(o) => o.lines.extend(lines),
                None => {
                    heap.push(Key {
                        at: parent.created_at,
                        id: parent.id.clone(),
                    });
                    pending.insert(
                        parent.id.clone(),
                        Origin {
                            snapshot: parent.clone(),
                            entry: entry.clone(),
                            lines,
                        },
                    );
                }
            }
        };

        // Unchanged in some parent: everything goes there, no diff needed.
        if let Some((parent, entry)) = parents
            .iter()
            .find(|(_, e)| e.hash == origin.entry.hash && e.chunked == origin.entry.chunked)
        {
            send(parent, entry, origin.lines);
            return Ok(None);
        }

        let mut remaining = origin.lines;
        if !parents.is_empty() {
            let text = walk.text(&origin.entry)?.unwrap_or_default();
            let cur_lines = split_lines(&text);
            for (parent, entry) in &parents {
                if remaining.is_empty() {
                    break;
                }
                let Some(parent_text) = walk.text(entry)? else {
                    continue;
                };
                let parent_lines = split_lines(&parent_text);
                let mut map: Vec<Option<usize>> = vec![None; cur_lines.len()];
                for (i, j) in match_lines(&cur_lines, &parent_lines) {
                    map[i] = Some(j);
                }
                let mut passed = Vec::new();
                remaining.retain(|&(i, fin)| match map.get(i).copied().flatten() {
                    Some(j) => {
                        passed.push((j, fin));
                        false
                    }
                    None => true,
                });
                send(parent, entry, passed);
            }
        }
        if remaining.is_empty() {
            return Ok(None);
        }
        Ok(Some((
            origin.snapshot,
            remaining.into_iter().map(|(_, fin)| fin).collect(),
        )))
    }
}

/// `line` without its `\n` or `\r\n` terminator.
fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_eol_handles_terminators() {
        assert_eq!(strip_eol("a\n"), "a");
        assert_eq!(strip_eol("a\r\n"), "a");
        assert_eq!(strip_eol("a"), "a");
        assert_eq!(strip_eol("\n"), "");
    }

    #[test]
    fn keys_order_newest_first() {
        let t0 = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let t1 = DateTime::from_timestamp(1_700_000_100, 0).unwrap();
        let mut heap = BinaryHeap::from([
            Key {
                at: t0,
                id: "a".into(),
            },
            Key {
                at: t1,
                id: "b".into(),
            },
            Key {
                at: t1,
                id: "c".into(),
            },
        ]);
        let order: Vec<String> = std::iter::from_fn(|| heap.pop().map(|k| k.id)).collect();
        assert_eq!(order, ["c", "b", "a"]);
    }
}
