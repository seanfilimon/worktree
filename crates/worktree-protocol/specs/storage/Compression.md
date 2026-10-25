# Object Compression and Store Integrity Specification

## Overview

Since v0.6 (`docs/ENGINEERING_PLAN.md` §14.6) object files in the local
`.wt/objects/` store and the server's `<data>/objects/` store may be
zstd-compressed, gRPC object transfers are gzip-compressed, and both sides
have integrity tooling: `wt gc` / `wt fsck` locally and
`POST /api/v1/admin/fsck` (`wt admin fsck`) on the server.

Compression is purely a storage and transport concern. Object ids are the
BLAKE3 hash of the **uncompressed** content, exactly as before, so ids,
manifests, chunk lists, snapshot records and the sync protocol are
unchanged, and stores written before v0.6 need no migration.

## Stored encoding (WTZ1)

An object file at `objects/<2 hex>/<62 hex>` holds one of:

| Form | Bytes |
|---|---|
| **Raw** | The object content itself. |
| **Compressed** | The 4-byte magic `WTZ1` (`57 54 5a 31`) followed by exactly one zstd frame whose decompressed content is the object. |

Writer rules (the SDK's `ObjectStore` and the server's `DiskStore`):

1. Content of fewer than **512 bytes** is stored raw.
2. Otherwise the content is compressed (zstd; the SDK uses level 3, the
   server `SpeedDefault`) and the compressed form is kept only when
   `4 + frame length <= 0.9 x content length` (it saves at least 10 %).
3. Content that itself begins with `WTZ1` is **always** stored compressed,
   whatever its size and whatever the compression setting. Therefore a
   stored file that starts with the magic is always the compressed form.
4. The hash is computed over the content before encoding; a file is never
   renamed or re-addressed because of its encoding.
5. Writes stay atomic: the encoded bytes go to a temp file in the fan-out
   directory, which is renamed into place. A rename that loses a race to a
   concurrent writer of the same object is success when the existing file
   decodes to the same content (either encoding).

Reader rules:

- A file without the magic is the content.
- A file with the magic is decoded as one zstd frame. Readers that know the
  expected hash (all of them) verify the decoded content; if decoding fails
  or the hash does not match, a file that hashes correctly **as raw bytes**
  is accepted as a legacy raw object (written before v0.6 by a writer that
  did not apply rule 3). Anything else is a corrupt object
  (`SdkError::CorruptObject` / `objstore.ErrCorrupt`).
- Frames may or may not carry a content checksum or content size; both
  implementations accept either (the Go encoder writes a checksum, the Rust
  one does not).

Store semantics are unchanged except for sizes:

| Operation | Behaviour |
|---|---|
| `get` / `Get` | Returns the decoded content (verified against the hash in the SDK). |
| `has` / `Has` | Whether the file exists. |
| `size` / `Size` / `ObjectSize` | **Stored** bytes (the compressed size for WTZ1 files). |
| `stats` / `Stats`, `list` / `List` | Count and **stored** bytes / hashes of every object file; temp files are skipped. |
| `put` / `Put` | Hash check over the content, then encode and write. |

Stored sizes feed the admin statistics, the server's tenant storage usage
and quotas (`MaxStorageBytes`), and gc's freed-bytes accounting.

Configuration:

| Side | Setting |
|---|---|
| Server | `--compress-objects` / `WT_COMPRESS_OBJECTS` (default `true`). With `false` new objects are written raw (rule 3 still applies); existing compressed objects are still read. |
| SDK | Always compresses new objects (`ObjectStore::with_compression(false)` exists for tests and tools). |

Compatibility vectors: `crates/worktree-sdk/src/store.rs` and
`services/server-go/internal/objstore/encoding_test.go` share a 1296-byte
content (`"W0rkTree WTZ1 compatibility vector: ..."` x 16, BLAKE3
`bcde4961...3c18`) and its WTZ1 encodings as produced by each side; each
side's tests decode both.

## gRPC message compression

- The server registers gRPC's `gzip` compressor
  (`google.golang.org/grpc/encoding/gzip`). grpc-go accepts gzip-compressed
  requests and compresses its responses with the encoding the request used.
- The SDK's `SyncClient` sends `UploadObjects` and `DownloadObjects`
  requests gzip-compressed and advertises `grpc-accept-encoding: gzip`, so
  object payloads (always uncompressed content on the wire) travel
  compressed in both directions. Other RPCs are not compressed.
- Fallback: when the server answers `UNIMPLEMENTED` mentioning compression
  or `grpc-encoding` (a server without gzip), the client repeats the call
  uncompressed and keeps compression off for the rest of that client's
  life.

## Local garbage collection (`wt gc`)

`wt gc [--dry-run|-n] [--prune DAYS]` (`Repository::gc`, `repo/storage.rs`)
deletes local objects and snapshot records that nothing references.

Roots:

- every branch tip (archived branches included), every remote-tracking ref
  (`refs/remotes/<remote>/*`) and every tag of every tree;
- `old` and `new` of reflog entries newer than the prune window
  (`--prune`, default **30** days; `0` = the reflog protects nothing);
- a pending merge: `MERGE_STATE.json`'s source tip, target tip and base;
- stash entries (`trees/<tree>/stash.jsonl`) and every other JSON or
  JSON-lines state file directly in `trees/<tree>/` (except `reflog.jsonl`
  and `index.json`) plus `conflicts/<tree>.json`: every string value that
  is a 64-hex object hash or the id of a stored snapshot is a root. Named
  objects are expanded as manifests when they decode as one. This keeps
  stash manifests and in-progress rebase / cherry-pick state alive without
  gc depending on their exact formats;
- in-flight protection: snapshot records and objects modified within
  **one hour** (or within the prune window when it is shorter) are kept, so
  a concurrent snapshot, fetch or merge that has stored objects but not yet
  moved a ref is never broken.

From a reachable snapshot its parents, its manifest, every entry's blob or
chunk list, and every chunk of those chunk lists are reachable. Missing
snapshot records (shallow history) and missing objects end the walk.

Safety:

- gc takes every tree's lock (sorted by name) for the whole run, except in
  a dry run, so CLI commands and the daemon cannot change refs underneath it.
- If a reachable snapshot record, manifest or chunk list cannot be read or
  decoded, gc stops **before deleting anything** and asks for `wt fsck`.
- Snapshot records that do not parse are never deleted (fsck reports them).

Actions (none in a dry run): delete unreachable objects past the grace
period, delete unreachable snapshot records, remove interrupted-write temp
files older than the grace period, and rewrite surviving raw objects in the
WTZ1 encoding when that saves at least 10 % ("compaction"; best effort).

Report (`GcReport`, also `--json`): `dry_run`, `prune_days`,
`objects_scanned`, `objects_reachable`, `objects_recent`,
`objects_deleted`, `bytes_deleted`, `snapshots_reachable`,
`snapshots_deleted`, `objects_compacted`, `bytes_compacted`,
`temp_files_deleted`, `bytes_before`, `bytes_after`. In a dry run the
`*_deleted` counts are what would be deleted and no compaction happens.

## Local integrity check (`wt fsck`)

`wt fsck [--full]` (`Repository::fsck`) checks:

| Check | Issue kind |
|---|---|
| Every snapshot record parses and is named after its id | `bad_snapshot` |
| Every parent of a snapshot is stored, unless the snapshot is in a shallow boundary | `missing_parent` |
| Every snapshot's manifest is stored | `missing_manifest` |
| Every manifest reads back with its hash and decodes | `corrupt_object` / `bad_manifest` |
| Every entry's blob or chunk list is stored; blobs of restricted (stubbed) paths may be absent (counted as `restricted_missing`) | `missing_object` |
| Every chunk list reads back, decodes and all its chunks are stored | `corrupt_object` / `bad_chunk_list` / `missing_object` |
| Every branch, remote-tracking ref and tag names a stored snapshot | `dangling_ref` |
| `--full`: every stored object reads back with its hash | `corrupt_object` |

The report (`FsckReport`: `full`, `objects_checked`, `snapshots_checked`,
`manifests_checked`, `refs_checked`, `restricted_missing`,
`errors[{kind, id, detail}]`) is printed (or emitted with `--json`); the
command exits 1 when `errors` is not empty.

## Server integrity check

`POST /api/v1/admin/fsck` (server admins; `401` anonymous, `403` others,
`501` without an object store) re-hashes every stored object (when the store
can list them) and walks every snapshot reachable from a branch tip or tag
of every tree, checking manifests, blobs, chunk lists and chunks:

```json
{
  "objects_checked": 1234,
  "corrupt": ["<hex>", "..."],
  "missing": [{"snapshot": "<id>", "object": "<hex>"}],
  "missing_snapshots": ["<id>"],
  "snapshots_checked": 87,
  "duration_ms": 412
}
```

`corrupt` lists objects whose stored bytes do not decode to content with
their hash plus manifests / chunk lists that do not parse; `missing` lists
each absent object once with the first snapshot found referencing it;
`missing_snapshots` lists ids named by refs or parents without a record.
`wt admin fsck` prints the report and exits 1 when any list is non-empty.

## Implementation status

| Area | Status |
|---|---|
| WTZ1 encoding, writer thresholds, legacy-raw fallback (SDK and server) | Built (v0.6 §14.6) |
| Stored-size semantics for `Size` / `ObjectSize` / `Stats` | Built (v0.6 §14.6) |
| `--compress-objects` / `WT_COMPRESS_OBJECTS` | Built (v0.6 §14.6) |
| Cross-implementation compatibility vectors | Built (v0.6 §14.6) |
| gRPC gzip for object transfers with uncompressed fallback | Built (v0.6 §14.6) |
| `wt gc` (roots, grace, dry run, compaction, temp files) | Built (v0.6 §14.6) |
| `wt fsck [--full]` | Built (v0.6 §14.6) |
| `POST /api/v1/admin/fsck`, `wt admin fsck` | Built (v0.6 §14.6) |
| Stash roots via `Repository::stash_roots()` | Partial: gc reads `stash.jsonl` (and other state files) generically |
| Server-side recompression of existing raw objects | Roadmap |

## Roadmap

- Server-side compaction of objects stored raw before v0.6 (a gc option).
- Dictionary-trained compression for small objects, and pack files with
  delta compression.
- Automatic re-fetch of objects `wt fsck` reports missing or corrupt.
- Streaming fsck progress and a time budget for very large server stores.
- Running local gc from the daemon's maintenance task.

## References

- SDK: `crates/worktree-sdk/src/store.rs` (encoding, `ObjectStore`),
  `src/repo/storage.rs` (gc, fsck), `src/remote/client.rs` (gzip)
- CLI: `crates/worktree-cli/src/commands/{gc,fsck,admin}.rs`
- Server: `services/server-go/internal/objstore/{encoding,disk}.go`,
  `internal/httpapi/fsck.go`, `internal/config/config.go`,
  `internal/server/server.go`
- Tests: `crates/worktree-sdk/tests/{storage_gc,remote_compression,store}.rs`,
  `crates/worktree-cli/tests/storage.rs`,
  `services/server-go/internal/objstore/encoding_test.go`,
  `internal/httpapi/fsck_test.go`, `internal/server/compression_test.go`
- `docs/ENGINEERING_PLAN.md` §14.6
- Related specs: [Storage](./Storage.md), [Sync](../sync/Sync.md),
  [Server](../server/Server.md)
