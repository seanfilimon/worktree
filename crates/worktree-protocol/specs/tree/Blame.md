# Blame

> Status: built in v0.6 (`docs/ENGINEERING_PLAN.md` §14.4).

## Overview

Blame attributes every line of a text file at a revision to the snapshot
that last changed it. The same algorithm runs locally (Rust SDK,
`Repository::blame`, `wt blame`) and on the server (Go package
`internal/blame`, `GET {tree}/blame/{rev}/{path...}`); for the same history
both produce the same attribution. A shared scenario in both test suites
keeps them in agreement.

---

## 1. Attribution rules

A **line** is everything up to and including a `\n`; a final line without a
terminator is a different line from the same text with one (as in the line
diff). Reported line text omits the `\n` or `\r\n` terminator.

The walk starts at the revision with every requested line pending. A
snapshot hands its pending lines to its parents:

1. **Unchanged file.** If a parent's manifest entry for the path names the
   same object (same hash, same chunked flag), that parent takes every
   pending line unchanged, without a diff. Parents are tried in order, first
   parent first.
2. **Changed file.** Otherwise each parent in order (first parent first)
   takes the pending lines that the line diff (Myers LCS; SDK
   `diff::match_lines`, server `textdiff.Matches`) matches to one of its
   lines. A line of a merge snapshot therefore goes to the first parent that
   has it: lines on the main line follow the first-parent history, lines
   brought in by a merged branch follow that branch's history.
3. **Introduced here.** Lines no parent takes are attributed to the
   snapshot itself. This covers new and edited lines and lines written while
   resolving merge conflicts.

Snapshots are processed newest first (by `created_at`, ties by id), so lines
that reach the same snapshot along different paths are processed together.
The result does not depend on the processing order. The walk ends as soon
as no line is pending: a file that stopped changing long ago costs one
manifest lookup per snapshot back to its last change, without reading
blobs.

Special cases:

| Case | Behaviour |
|---|---|
| Path missing in a parent | That parent takes no lines. **Renames are not followed**: after a rename every line belongs to the renaming snapshot. |
| Parent version is binary | That parent takes no lines. |
| Blamed version is binary (NUL in the first 8 KiB or invalid UTF-8) | Error. |
| Path missing at the revision | Error (`does not exist in snapshot ...` locally, `404 not_found` on the server). |
| Empty file | Empty result. |
| Chunked file (> 8 MiB) | Read through `ObjectStore::read_entry` / `content.ReadEntry`. |
| Parent missing locally (shallow history) | Treated as absent: lines stop at the boundary snapshot. On the server a dangling parent is corruption (500). |
| Root snapshot | Takes every line still pending. |

Each attributed line carries the snapshot id, author, the first line of the
message and the snapshot's creation time.

### Line ranges

A blame can be restricted to a 1-based inclusive range `START..END`. The
end is clamped to the file length; `START` past the end of the file, `START
= 0` or `END < START` are errors. Only the requested lines are traced, so a
small range of a long-lived file stops early.

---

## 2. SDK

```rust
// crates/worktree-sdk/src/repo/blame.rs
impl Repository {
    pub fn blame(&self, tree: &str, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>>;
    pub fn blame_lines(&self, tree: &str, path: &str, rev: Option<&str>,
                       lines: Option<RangeInclusive<usize>>) -> Result<Vec<BlameLine>>;
}

// crates/worktree-sdk/src/model.rs
pub struct BlameLine {
    pub line: usize,            // 1-based
    pub text: String,           // without the line terminator
    pub snapshot_id: SnapshotId,
    pub author: String,
    pub message: String,        // first line
    #[serde(rename = "at")]     // JSON field "at", same as the server's BlameLine
    pub created_at: DateTime<Utc>,
}
```

`rev` accepts every revision form of `Repository::resolve` (default: HEAD;
an empty branch is `InvalidRevision`). `path` is relative to the tree root.
Binary files, missing paths and bad ranges are `InvalidArgument`. Per call
the walk caches the path's entry per manifest and decoded text per object.

## 3. CLI

```text
wt blame PATH [--rev R] [-L START,END] [--porcelain | --json]
```

- `PATH` is relative to the current directory (it must name a file of the
  selected tree).
- `-L` accepts `START,END`, `START,+COUNT`, `START,` and `START` (the last
  two run to the end of the file).
- Default output, one row per line with aligned columns: short snapshot id,
  author name (the `<email>` part dropped), local date, line number, text:

  ```text
  1f2e3d4c (Ada Lovelace 2026-10-25 1) fn main() {
  9a8b7c6d (Bob          2026-10-26 2)     run();
  ```

- `--porcelain`: per line a block (like `git blame --line-porcelain`):

  ```text
  <snapshot id> <line>
  author <author>
  author-time <unix seconds>
  created-at <RFC 3339>
  summary <first line of the message>
  filename <path>
  \t<text>
  ```

- `--json`: `{"path","snapshot":Snapshot,"lines":[BlameLine]}` where
  `snapshot` is the snapshot the revision resolved to. `--porcelain` and
  `--json` are mutually exclusive.

## 4. Server

```text
GET /api/v1/trees/{tenant}/{tree}/blame/{rev}/{path...}[?start=N&end=M]
→ 200 {"lines":[BlameLine],"snapshot":Snapshot}
```

`domain.BlameLine` is `{"line","text","snapshot_id","author","message","at"}`.
`rev` is a snapshot id, branch name, tag name or unique id prefix of the
tree (one path segment, so branch names containing `/` must be given by
snapshot id). `start`/`end` restrict the lines as in §1.

Read checks are those of the blob endpoint (Security §13.1):

| Condition | Response |
|---|---|
| Caller lacks `tree:read` | `403`/`404` as for every tree route |
| Unknown revision | `404 not_found` |
| Path denied by a `tree:read` path policy | `403 path_denied` (checked before the lookup, so existence is not revealed) |
| Path restricted by license rules | `451 license_restricted` |
| File missing at the revision | `404 not_found` |
| Binary file | `422 binary_file` |
| A version larger than 32 MiB (`blame.MaxFileSize`) | `413 payload_too_large` |
| Bad `start`/`end` or range past the end | `400 bad_request` |

The Go implementation reads snapshot records from the `MetaStore` and
manifests and blobs from the object store, with the same per-call caches as
the SDK, and checks the request context between snapshots.

---

## Implementation Status

| Feature | Status |
|---|---|
| SDK `Repository::blame` / `blame_lines`, `BlameLine` | Built (v0.6 §14.4) |
| Merge handling (first parent first, merged-branch lines follow their branch) | Built (v0.6 §14.4) |
| Chunked files, binary detection, early stop, per-call caches | Built (v0.6 §14.4) |
| `wt blame` with `--rev`, `-L`, `--porcelain`, `--json` | Built (v0.6 §14.4) |
| `GET {tree}/blame/{rev}/{path...}` with path-deny and license read checks | Built (v0.6 §14.4) |
| Same attribution in Go and Rust (shared test scenario) | Built (v0.6 §14.4) |
| Following renames and copies | Roadmap |

## Roadmap

- **Rename and copy following** (`-M`/`-C` in git terms), using the
  similarity-based rename detection of `wt diff --find-renames`.
- **Blaming the working copy** (uncommitted lines shown as "not snapshotted
  yet").
- **Ignoring revisions** (`--ignore-rev`, an ignore-revs file for formatting
  snapshots) and whitespace-insensitive matching.
- **Incremental / streaming output** for very long histories and a server
  response cache per (snapshot, path).
- Multiple `-L` ranges and function-name ranges.

## References

- `crates/worktree-sdk/src/repo/blame.rs` — SDK algorithm
- `crates/worktree-sdk/src/model.rs` — `BlameLine`
- `crates/worktree-sdk/src/diff/myers.rs` — `match_lines`
- `crates/worktree-sdk/tests/blame.rs` — SDK tests, shared scenario
- `crates/worktree-cli/src/commands/blame.rs` — `wt blame`
- `crates/worktree-cli/tests/blame.rs` — CLI tests
- `services/server-go/internal/blame/` — server algorithm and tests (shared scenario)
- `services/server-go/internal/textdiff/match.go` — `Lines`, `Matches`
- `services/server-go/internal/httpapi/blame.go` — REST endpoint
- `services/server-go/internal/domain/v06.go` — `BlameLine`
