# worktree-sdk — implementation notes

How the SDK is built and which invariants it maintains. The public API is defined by the source
(`src/{lib,model,store,config,repo/mod,diff,merge,scan,remote/mod,ipc}.rs`); the v2 contract is
[docs/ENGINEERING_PLAN.md](../../docs/ENGINEERING_PLAN.md) §3–4. User-level docs:
[SDK guide](../../docs/sdk-guide.md).

## Layering

```
remote ─┐                ┌─ ipc (daemon client, JSON over TCP)
        ▼                │
      repo ──► store ──► .wt/objects
        │  └─► config ─► .wt/config.toml
        ├─► diff, merge   (pure algorithms)
        └─► scan          (pure: secrets, licenses, chunking)
model: shared data types, wire conversions (Snapshot <-> SnapshotRecord, Manifest <-> ManifestRecord)
```

`diff`, `merge` and `scan` perform no I/O and depend on nothing else in the crate except `model`,
so they are tested exhaustively in isolation. The legacy JSON-state `engine` module is superseded by
`repo` and is being removed; new code must not use it.

## Object store (`store.rs`)

- Raw, uncompressed objects at `objects/<hh>/<62 hex>`, keyed by BLAKE3-256 of the exact bytes —
  identical to the server's layout and the wire bytes, so push/pull is a byte copy.
- `put` writes to a temporary file in the same directory and renames it into place. If the rename
  fails because the target already exists (Windows reports "Access is denied" under concurrency),
  the write is treated as successful: the content is identical by construction.
- `get` verifies the hash (`CorruptObject` on mismatch). `put_with_hash` verifies before writing.

## Manifests and snapshots (`model.rs`)

- `Manifest::from_entries` sorts by path and de-duplicates (last wins); `to_bytes` produces the
  canonical protobuf `ManifestRecord` encoding, and `hash` = BLAKE3 of those bytes.
- Paths are `/`-separated and relative to the **tree** root, never the worktree root.
- `Snapshot` is stored as JSON in `snapshots/<id>.json` (hashes as hex) and converted to/from the
  wire `SnapshotRecord` by `to_record` / `from_record`. Ids are UUID v4 (UUID v5 for Git imports).

## Repository (`repo/`)

| Concern | Approach |
|---|---|
| Discovery | `open` walks up from the given path until it finds `.wt/`. |
| Trees | Registry in `.wt/trees/<tree>/tree.toml`; nested trees also have a `<path>/.wt-tree/config.toml` marker. `tree_for_path` picks the deepest containing tree. |
| Working-directory scan | Walks the tree directory, skipping built-in ignores, `.wt/ignore`, `.wtignore` patterns (gitignore syntax via `worktree_protocol::feature::ignore`) and nested-tree directories. |
| Status | Compares the scan with the head manifest. `index.json` caches `(size, mtime_ns, hash, executable)` per path so unchanged files are not re-hashed; entries written within a small window before the index was saved ("racy" entries) are always re-hashed. |
| Snapshot | Scan → secret scan of added/modified files (unless `skip_secret_scan`) → store blobs → write manifest → write snapshot record → move the branch ref → append reflog. `NoChanges` if the manifest equals head and not `allow_empty`. |
| Revisions | `HEAD`, `HEAD~N`, `<branch>[~N]`, `<remote>/<branch>`, `<tag>`, id prefix (≥ 4 characters, `AmbiguousId` if not unique). `~N` follows first parents. |
| Branches | Ref files under `refs/branches/`; `branches.meta.json` stores `created_at` and the `archived` flag. Archiving never deletes the ref. |
| Checkout | `checkout`/`switch_branch` rewrite tracked files to the target manifest, delete tracked files absent from it, and never touch ignored or nested-tree paths. Refuses with `DirtyWorkingTree(n)` unless clean or forced. |
| Merge | `merge::merge_base` over the snapshot DAG → `merge::merge_manifests` → `merge::merge_text` for paths changed on both sides. Clean results create a two-parent snapshot (or fast-forward). Conflicts write diff3 markers into files, persist `MERGE_STATE.json` and `.wt/conflicts/<tree>.json`, and return `MergeOutcome::Conflicts`. Binary conflicts keep "ours" in the working tree. |
| Continue / abort | `continue_merge` refuses while conflict markers remain (`MergeConflict`); `abort_merge` checks out the pre-merge head and removes the merge state. |
| Revert | New snapshot on the current branch that undoes `rev`'s changes relative to its first parent, never rewriting history. Requires a clean working tree; `MergeConflict` if later snapshots changed the same lines, `NoChanges` if nothing is left to undo. |
| Reflog | `reflog.jsonl`, one `ReflogEntry` per line, append-only. |
| Locking | Mutating operations take an exclusive OS file lock on `.wt/trees/<tree>/LOCK` (released automatically if the process dies), so `wt` and the daemon can operate on the same worktree. |
| Atomic writes | Refs, records and config are written via temp file + rename. |

## Remote client (`remote/`)

- One tonic channel per server URL (h2c, 64 MiB message limit) plus a small REST client for
  `/api/v1/auth/*`. Server URLs are normalized (scheme/host lower-cased, no trailing slash) and used
  as keys in `$WT_HOME/credentials.toml`.
- **push**: `ResolveTree(create_if_missing)` → `ListBranches` for the remote tip → compute
  snapshots in `ancestry(local tip)` but not in `ancestry(remote tip)` → `Negotiate` with their
  manifests and blobs → `UploadObjects` (streamed) → `Push` → update `refs/remotes/<remote>/<branch>`
  → push missing tags. A local branch behind the remote tip is rejected before any upload.
- **fetch**: for each remote branch with a new tip, `Pull{current_tip}` → `DownloadObjects` for
  objects not present locally (with `x-wt-tree-id` metadata so public trees work anonymously) → verify
  hashes → write snapshots → move remote-tracking refs → `ListTags`.
- **pull** = fetch + fast-forward/checkout, else `Repository::merge`.
- **clone** = init + `[remote.origin]` + `set_tree_id`/`set_tree_remote_name` + fetch + local
  branches + checkout of the default branch.
- **stage** = `manifest_from_working` → upload missing objects → `Stage` with the change summary vs.
  head.
- gRPC status codes map to `SdkError` (`UNAUTHENTICATED` → `NotLoggedIn`, `PERMISSION_DENIED` →
  `PermissionDenied`, `PushRejection` → `PushRejected`, others → `Remote`).

## IPC client (`ipc.rs`)

Reads `$WT_HOME/bg.json`, connects to `127.0.0.1:<port>`, writes one JSON request line and reads one
response line. A missing file or refused connection is `DaemonNotRunning`. The server side lives in
`worktree-bg`.

## Testing

- Unit tests sit next to the code; `repo` tests use `tempfile` worktrees.
- `WT_HOME` must point to a temporary directory in every test that touches credentials or IPC.
- Tests needing a live server are `#[ignore = "integration: ..."]`; the full flow is covered by
  `crates/worktree-e2e` (`WT_E2E=1`).
