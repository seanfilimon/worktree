# Storage format

This page describes what a worktree stores on disk. The authoritative definition is the module
documentation of [`crates/worktree-sdk/src/repo/mod.rs`](../crates/worktree-sdk/src/repo/mod.rs);
server-side storage is covered in [server architecture](./server-architecture.md#storage).

All files are plain TOML, JSON or raw bytes. Nothing in `.wt/` is required to be edited by hand,
but everything is readable.

## Layout

```text
<worktree root>/
  .wt/
    config.toml                                WorktreeConfig (see configuration.md)
    ignore                                     extra ignore patterns, one per line
    objects/<hh>/<62 hex>                      blobs + manifests (content-addressed)
    snapshots/<id>.json                        Snapshot records
    trees/<tree>/tree.toml                     TreeInfo: name, id, path, default branch, remote name
    trees/<tree>/HEAD                          current branch name
    trees/<tree>/refs/branches/<branch>        snapshot id (empty file = branch without snapshots)
    trees/<tree>/refs/branches.meta.json       per-branch created_at / archived flags
    trees/<tree>/refs/remotes/<remote>/<branch> last known remote tip (snapshot id)
    trees/<tree>/refs/tags/<tag>.json          TagInfo
    trees/<tree>/index.json                    stat cache: path -> (size, mtime_ns, hash)
    trees/<tree>/reflog.jsonl                  one ReflogEntry per line, append-only
    trees/<tree>/MERGE_STATE.json              present only while a conflicted merge is pending
    conflicts/<tree>.json                      machine-readable conflict list during a merge
    git-map.json                               snapshot id <-> Git commit SHA (written by wt git import/export)
  <nested tree dir>/
    .wt-tree/config.toml                       marker for a nested tree: { name, id }
```

Branch and tag names may contain `/` (for example `feature/login`); they map to subdirectories under
`refs/branches/` and `refs/tags/`.

Mutating operations on a tree hold an exclusive OS file lock on `.wt/trees/<tree>/LOCK`, so
concurrent `wt` invocations and the daemon do not interleave. OS locks are released when a process
exits, so a crash never leaves a stale lock.

## Objects

`objects/` is a content-addressed store shared by all trees of the worktree.

| Property | Value |
|---|---|
| Hash | BLAKE3-256 of the exact stored bytes |
| Path | `objects/<first 2 hex chars>/<remaining 62 hex chars>` |
| Encoding | Raw and uncompressed |
| Writes | Atomic (temporary file + rename), idempotent, safe under concurrent writers |
| Reads | Hash is verified; a mismatch is a `CorruptObject` error |

There are two kinds of objects:

| Kind | Content |
|---|---|
| **Blob** | The raw bytes of one file. |
| **Manifest** | A protobuf-encoded `worktree.sync.v1.ManifestRecord`: the complete file list of a snapshot. |

The same bytes and hashes are used on the wire and on the server, so push and pull copy objects
without re-encoding. Hashes interoperate across Rust and Go: both use the 32-byte binary form on
the wire and 64-character lowercase hex in JSON and paths.

### Manifests

A manifest lists every file of a tree at one point in time:

| Field | Meaning |
|---|---|
| `path` | `/`-separated, relative to the tree's root directory |
| `blob_hash` | Hash of the file's blob |
| `size` | Size in bytes |
| `executable` | Executable bit (from the Unix file mode) |

Entries are sorted by `path` and unique, which makes the encoding canonical: the same set of files
always produces the same manifest bytes and hash. Directories are implicit; empty directories are
not recorded.

Files larger than the chunking threshold are currently stored as single blobs. Content-defined
chunking (FastCDC; `worktree_sdk::scan::chunk_boundaries`) is implemented as a function but not
yet used by the storage format — chunked storage and lazy loading are *Roadmap*
([Storage spec](../crates/worktree-protocol/specs/storage/Storage.md)).

## Snapshots

Snapshots are **not** objects. Each is a JSON file `snapshots/<id>.json`:

```json
{
  "id": "5d0c2f7e-3b1a-4d55-9a43-0f6f6f2b7a10",
  "tree_id": "0b6f6d8e-1f7a-4c39-9a51-3c2d7c1e4b20",
  "branch": "main",
  "parents": ["c1a7e0d4-8b2f-4f3e-a1d9-6e5b4c3a2f10"],
  "manifest": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "author": "Ada Lovelace <ada@example.com>",
  "message": "Add parser",
  "created_at": "2026-10-24T12:34:56Z",
  "auto_generated": false
}
```

| Field | Meaning |
|---|---|
| `id` | UUID v4 string. Unique, not derived from content (Git import uses UUID v5 — see [Git compatibility](./git-compatibility.md#determinism)). |
| `tree_id` | Stable id of the tree; equals the server's tree id once linked. |
| `branch` | Branch the snapshot was created on. |
| `parents` | Zero (first snapshot), one, or two (merge) parent ids. The first parent is the branch's previous tip. |
| `manifest` | Hex hash of the manifest object. |
| `auto_generated` | `true` for daemon auto-snapshots (message `auto: N files changed`). |

Snapshot records are immutable and written idempotently. On the wire they travel as
`SnapshotRecord` messages ([protocol](./protocol-spec.md#snapshots)).

## Trees

`trees/<tree>/tree.toml`:

```toml
name = "web"
id = "7c3e5a14-9d2b-4b8e-8f61-2a0d9c4e5b37"
path = "apps/web"          # "" for the root tree
default_branch = "main"
remote_name = "web"        # the tree's name on the server
```

`trees/<tree>/HEAD` holds the current branch name. A nested tree's directory additionally contains
`.wt-tree/config.toml` with its `name` and `id`; `wt tree remove` unregisters the tree but keeps its
files.

A tree's snapshot contains the files under its directory, minus ignored paths and minus the
directories of trees nested inside it. `.wt/` and `.wt-tree/` are never snapshotted.

## Refs

| Ref | File | Content |
|---|---|---|
| Local branch | `refs/branches/<branch>` | Snapshot id, or empty for a branch that has no snapshots yet (for example right after `wt init`). |
| Branch metadata | `refs/branches.meta.json` | `{"<branch>": {"created_at": "...", "archived": false}}` |
| Remote-tracking branch | `refs/remotes/<remote>/<branch>` | Last tip seen on the server; updated by `push`, `fetch`, `pull`, `clone`. |
| Tag | `refs/tags/<tag>.json` | `TagInfo`: `name`, `target`, `message`, `tagger`, `created_at`. |

Archiving a branch sets `archived: true`; the ref file and its history remain.

## Index (stat cache)

`trees/<tree>/index.json` caches `(size, mtime_ns, hash, executable)` per path so that `wt status`
does not re-hash unchanged files. It is a cache, not a staging area: deleting it is safe and only
costs a re-hash. Entries modified within a short window before the index was saved are re-hashed to
avoid missing same-tick writes.

## Reflog

`trees/<tree>/reflog.jsonl` is append-only, one JSON object per line:

```json
{"at":"2026-10-24T12:34:56Z","tree":"demo","branch":"main","action":"snapshot","old":"c1a7e0d4-...","new":"5d0c2f7e-...","message":"Add parser"}
```

Actions: `init`, `snapshot`, `auto-snapshot`, `switch`, `branch.create`, `branch.archive`, `merge`,
`fast-forward`, `revert`, `restore`, `pull`, `push`, `tag.create`, `tag.delete`.
`wt reflog` prints it newest first.

## Merge state

While a conflicted merge is pending:

- `trees/<tree>/MERGE_STATE.json` holds `MergeState`: `source_branch`, `source_tip`, `target_tip`,
  `base`, `conflicts`, `message`.
- `conflicts/<tree>.json` lists the conflicted paths and their kinds (`content`, `modify_delete`,
  `add_add`, `binary`) for tools.

Both are removed by `wt merge --continue` (or a successful `wt snapshot`) and `wt merge --abort`.
