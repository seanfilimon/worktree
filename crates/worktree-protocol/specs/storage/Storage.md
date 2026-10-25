# Storage Architecture Specification

## Overview

W0rkTree stores file content in a content-addressable object store keyed by
BLAKE3-256. The same object bytes and hashes are used in the local `.wt/` store,
on the wire and on the server, so a push or a fetch copies bytes without
transcoding. Large files are split into content-defined chunks natively, so no
separate LFS exists. Metadata (snapshots, branches, tags, IAM and so on) is kept
apart from the object store: as JSON files and refs locally, and in an embedded
bbolt database on the server.

```
client (.wt/)                               server (--data dir)
  objects/<2>/<62>   ── UploadObjects ──►     objects/<2>/<62>      (BLAKE3 CAS, shared)
  snapshots/<id>.json ── Push(SnapshotRecord)► meta.db: snapshots    (bbolt)
  trees/<t>/refs/... ◄── Pull / ListBranches── meta.db: branches, tags, staged, ...
```

## Object model

| Kind | Bytes | Addressed by |
|---|---|---|
| **Blob** (`OBJECT_KIND_BLOB`) | Raw file content, or one chunk of a large file. | BLAKE3 of the bytes |
| **Manifest** (`OBJECT_KIND_MANIFEST`) | Protobuf `ManifestRecord`: the complete file list of one snapshot of one tree, `{path, blob_hash, size, executable, chunked}` per file, sorted by path, unique, `/`-separated. | BLAKE3 of the encoded bytes |
| **Chunk list** (`OBJECT_KIND_CHUNK_LIST`) | Protobuf `ChunkList{size, chunks[{hash, size}]}` for a chunked file. | BLAKE3 of the encoded bytes |

A manifest is flat: there are no per-directory tree objects. Directories are
implied by paths. An entry with `chunked = true` points at a chunk list instead
of a blob.

The following are **not** objects:

- **Snapshots.** A snapshot record (`id` UUID v4, `tree_id`, `branch_name`,
  `parents[]`, `manifest_hash`, `author`, `message`, `created_at`,
  `auto_generated`, `dependencies[]`, `reverts`, `signature`,
  `signing_key_id`) is metadata. Locally it is a JSON file. On the wire it is a
  `SnapshotRecord`. On the server it is a JSON value in the `snapshots` bucket,
  extended with `pushed_by`, `pushed_at` and `verified`. Snapshot ids are
  random, not content hashes, so two identical snapshots are distinct records
  that share one manifest.
- **Branches.** A branch is a mutable pointer to a tip snapshot. It moves only
  forward on the server (fast-forward pushes and merges, with compare-and-swap).
- **Tags.** A tag is an immutable name pointing at a snapshot, with an
  optional message and the tagger.
- **Releases.** A release is metadata on a tag. Its **assets** are stored as
  blobs in the object store (at most 64 MiB each) and referenced by hash.

## Content addressing

- The hash is BLAKE3-256 of the exact stored bytes. It is 32 raw bytes on the
  wire and 64 lowercase hex characters in paths and JSON (`hash.Hash`
  marshals as hex).
- Identical content produces the same hash and is stored once, across files,
  versions, branches, trees and tenants. This is automatic deduplication.
- Objects are immutable. Writing an existing hash is a no-op.
- Objects are stored raw, or zstd-compressed behind the 4-byte magic `WTZ1`
  when that saves at least 10 % (objects of 512 bytes or more). Hashes are
  always over the uncompressed content. See [Compression.md](./Compression.md).

## Large files

Files larger than **8 MiB** are stored chunked:

- **Algorithm.** FastCDC with a Gear rolling hash (`worktree_sdk::scan::cdc`).
  Chunks are at least 256 KiB, average 1 MiB, and at most 4 MiB.
- **Layout.** Each chunk is a blob. The entry's hash names a `ChunkList`, and
  the file is the concatenation of the chunks in order. Chunk sizes must sum to
  `ChunkList.size`, which both sides validate.
- **Deduplication.** Chunk boundaries depend on content, so an insertion only
  changes nearby chunks. Unchanged chunks are shared with earlier versions and
  other files, and only new chunks are uploaded.
- **Reading.** Every consumer reads file content through one entry reader that
  reassembles chunked files: `ObjectStore::read_entry` (SDK: checkout, restore,
  diff, merge, archive, git export, secret scanning, license detection) and
  `internal/content.ReadEntry` (server REST blob, compare and archive
  endpoints; at most 4 GiB in memory).
- **Bounds.** No single object, chunk or not, may exceed 64 MiB (the gRPC
  message limit).

## Local storage (`.wt/`)

The authoritative layout is the doc comment in
`crates/worktree-sdk/src/repo/mod.rs`. See [DotWt](../dot-wt/DotWt.md) for
the full directory reference. Storage-relevant parts:

```
.wt/
  objects/<hh>/<62 hex>          blobs, manifests, chunk lists (ObjectStore)
  snapshots/<id>.json            snapshot records
  trees/<tree>/refs/branches/<b> branch tip (snapshot id)
  trees/<tree>/refs/remotes/<remote>/<b>   last known server tip
  trees/<tree>/refs/tags/<tag>.json
  trees/<tree>/index.json        stat cache: path -> (size, mtime_ns, hash)
  trees/<tree>/reflog.jsonl      append-only reflog
  trees/<tree>/shallow           shallow-clone boundary snapshot ids
  trees/<tree>/stubbed           paths withheld by the server (restricted)
  trees/<tree>/LOCK              per-tree OS file lock
```

- The store lives **inside the worktree** (`.wt/objects`), not in a
  per-user application-data directory. Per-user state (credentials, daemon
  state, signing keys) lives in `$WT_HOME` (default `~/.wt`).
- Writes go to a uniquely named temp file in the fan-out directory and are
  renamed into place, so readers never see a partial object. A rename that
  loses a race against another writer of the same hash is treated as success.
- Every read re-hashes the object. A mismatch returns
  `SdkError::CorruptObject`, and a missing object returns `ObjectNotFound`.
  Downloads are verified before they are stored (`put_with_hash`).
- The per-tree `LOCK` serializes mutating operations of the CLI and the daemon
  on one tree. Read-only operations do not lock.
- Content is materialized as ordinary files on checkout. Stubbed paths are not
  written, and there is no virtual filesystem or lazy loading.
- The reflog is local only. `wt reflog expire` (default 90 days / 1,000 entries
  per tree, also run by the daemon maintenance task) prunes it.

## Server storage

Everything lives under the data directory (`--data` / `WT_DATA`, default
`./wt-data`, created with mode 0750):

```
<data>/
  meta.db                 bbolt database (all metadata)
  objects/<2 hex>/<62 hex>  content-addressed objects (objstore.DiskStore)
```

### Object store (`internal/objstore`)

`objstore.Store` is the interface: `Put`, `Get`, `Has` and `Delete`. The
optional interfaces `StatStore.Stats` (count and bytes), `ListStore.List` (for
GC) and `SizeStore.Size` are also implemented, and `objstore.ObjectSize` falls
back to reading the object. `DiskStore` writes:

- `Put` verifies `BLAKE3(data) == h` (`ErrHashMismatch`) and is idempotent.
  It writes a temp file in the fan-out directory, fsyncs it and renames it into
  place.
- Two-hex fan-out caps a directory at 256 subdirectories, the same convention as
  Git's `objects/aa/bbb…`.
- **Windows.** Sharing and lock violations, and `ERROR_ACCESS_DENIED` on a
  rename onto an existing or open file, are retried with backoff (1 ms to 50 ms,
  2 s budget). A failed install whose target already holds the right content is
  success.
- Objects are shared by all tenants. Isolation lives in the reference graph:
  objects are readable only through a tree that reaches them. `DownloadObjects`
  is tree-scoped, and REST content endpoints resolve paths through a tree's
  snapshots.

### Metadata store (`internal/meta`)

`meta.NewBolt(path)` is a `domain.MetaStore` and `domain.DocStore` persisted in
one bbolt file. The file is opened exclusively with a 5 s lock timeout; a second
server on the same data directory fails to start. Every method is one bbolt
transaction, so it is atomic, and writes are serialized. `meta.NewMemory()` is
the in-memory implementation used by tests. The `meta` bucket stores a schema
version (currently 1), and the server refuses to open a database from a newer
schema.

Primary buckets hold JSON values. `idx_*` buckets map a unique or secondary key
to a primary key. Composite keys join components with a NUL byte.

| Bucket | Key → value |
|---|---|
| `tenants`, `idx_tenant_slug` | id → Tenant; slug → id |
| `accounts`, `idx_account_name` | id → Account (with password hash); username → id |
| `memberships`, `idx_member_acct` | tenant␀account → Membership; reverse index |
| `tokens`, `idx_token_hash`, `idx_token_acct`, `idx_token_sa` | id → APIToken (hash only); lookups by hash, account, service account |
| `trees`, `idx_tree_name` | id → Tree; tenant␀name → id |
| `branches` | tree␀name → Branch `{name, tip, updated_at, ...}` |
| `snapshots`, `idx_snapshot_tree` | id → Snapshot; tree␀id → id |
| `counters` | named counters (big-endian 64-bit), e.g. snapshot counts |
| `tags` | tree␀name → Tag |
| `staged` | tree␀account␀branch → StagedSnapshot (one per account and branch) |
| `protections` | tree␀pattern → BranchProtection |
| `merge_requests`, `mr_seq` | tree␀be64(number) → MergeRequest; tree → last number |
| `audit` | be64(at)␀be64(seq) → AuditEvent (time ordered) |
| `licenses` | tree␀glob → LicenseRule |
| `access_config` | tenant␀kind → raw v0.4 access document |
| `teams`, `idx_team_slug`, `team_members`, `idx_team_member_acct` | IAM teams |
| `roles`, `policies`, `grants`, `idx_grant_subject` | custom roles, policies, tree grants |
| `service_accounts`, `idx_sa_name` | service accounts |
| `mfa`, `invitations`, `idx_invitation_code`, `password_resets` | MFA state, invitations, reset codes (hashed) |
| `sso`, `sso_identities`, `idx_sso_ident_acct` | SSO config and linked identities |
| `docs` | v0.5 document collections: a nested bucket per collection (key → JSON) |
| `doc_seq` | v0.5 counters (`DocStore.NextSeq`) |

**Document collections** (`domain.DocStore`; keys built with `domain.DocKey`,
typed helpers `meta.GetDocT`, `ListDocsT`, `UpdateDocT`, `DocPrefix`):

| Collection | Key | Value |
|---|---|---|
| `todos` | treeID␀%010d number | `Todo` |
| `todo_sources` | sourceSnapshotID␀targetTreeID␀title | `{tree_id, number}` |
| `releases` | treeID␀tag | `Release` (assets by hash) |
| `ci_status` | treeID␀snapshotID␀context | `CIStatus` |
| `mr_comments` | treeID␀%08d number␀id | `ReviewComment` |
| `linked_groups` | tenantID␀id | `LinkedGroup` |
| `tag_protections` | treeID␀pattern | `TagProtection` |
| `signing_keys` | accountID␀id | `SigningKey` |
| `signing_key_index` | key id or fingerprint | `{account_id, id}` |
| `access_docs` | tenantID␀treeID | `AccessConfigDoc` |
| `gc_marks` | hex hash | `{marked_at}` |
| `gc_runs` | RFC 3339 start time | `GCReport` (last 20 kept) |
| `tenant_usage` | tenantID | `{bytes}` |

### History queries

`meta.NewHistory(store)` implements `domain.History` over the `snapshots`
bucket:

- `IsAncestor(a, d)`: breadth-first search from `d`, stopping at `a`.
- `Log(tip, limit)`: newest first by `(created_at, id)`, reading only what the
  limit needs.
- `Missing(tip, haves)`: snapshots reachable from `tip` and not from `haves`,
  in topological order with parents first and ties broken by `(created_at,
  id)`. It answers `Pull`.

## Concurrency and consistency

- **Branch tips** move with `CompareAndSwapBranchTip(tree, branch, expected,
  new)`. A push that loses the race is rejected with `ConflictDetected` (or
  succeeds, if the winner pushed the same tip). Merge-request merges also use
  CAS. Linked groups move several tips in order and roll back on failure.
- **Push ordering.** Objects are uploaded first, snapshot records are stored
  next (parents first), and the tip moves last. A reader never sees a tip whose
  snapshots or objects are missing. A crash between storing records and moving
  the tip leaves unreferenced records and objects, which GC handles.
- **Staged writes** are serialized per (tree, account, branch) with striped
  locks, so lazy expiry never deletes a just-replaced entry.
- **Locally**, pull downloads objects before it writes snapshot records, and
  every mutating operation holds the tree `LOCK`.

## Garbage collection

`gc.Collector` runs every `--gc-interval` (default 24 h; `0` disables it) and on
demand (`POST /api/v1/admin/gc?dry_run=`, `wt admin gc [--dry-run]`):

1. **Mark.** It starts from every branch tip, tag, staged snapshot,
   merge-request source and target tip, and release snapshot and asset. It
   walks their history and collects the reachable objects: manifests, blobs,
   chunk lists and chunks.
2. **Sweep with a grace period.** Each unreachable object is recorded in
   `gc_marks` with `marked_at`. It is deleted once it has been marked for longer
   than `--gc-grace` (default 720 h = 30 days) and is still unreachable. A
   marked object that becomes reachable again is unmarked. The grace period
   protects objects uploaded by a push that is still in progress.
3. **Retention.** The same run deletes staged snapshots older than
   `--staged-retention` (default 168 h; `0` keeps them) and audit events older
   than `--audit-retention` (default `0` = keep, `MetaStore.PruneAudit`).
4. **Report.** Each run produces a `GCReport{dry_run, scanned, reachable,
   unreachable, marked, deleted, freed_bytes, started_at, finished_at}`, kept in
   `gc_runs` (last 20) and shown by `GET /api/v1/admin/gc` and `wt admin
   gc-status`. Metrics are `worktree_gc_runs_total` and `worktree_gc_deleted_objects_total`.

Deleting a tree (`DELETE /api/v1/trees/{tenant}/{tree}`) removes its metadata
(branches, tags, staged, protections, merge requests, licenses, grants,
snapshots) in one transaction and publishes `tree.deleted`. Owners of
tree-scoped document collections delete their documents from a bus subscriber.
Objects are not deleted directly: those no longer reachable from any tree are
collected by GC after the grace period.

## Quotas

Quotas are `domain.TenantLimits`: `max_trees`, `max_storage_bytes`,
`max_branches_per_tree`, `max_members`, `requests_per_minute`. A zero value
means the server default. The defaults come from `--default-max-trees` and
`--default-max-storage` and are unlimited unless set. Server admins set
per-tenant limits with `PUT /api/v1/admin/tenants/{t}/limits` or `wt admin
limits`.

- **Storage.** Usage is the `tenant_usage` document `{bytes}`, increased by the
  new objects of each accepted push. The push quota checker (`limits.Limiter`)
  rejects a push that would exceed `max_storage_bytes` with
  `QuotaExceeded{limit}`. Because objects are shared, usage counts what each
  tenant uploaded first, not unique bytes per tenant.
- `max_branches_per_tree` is enforced when a push or a REST call creates a
  branch.
- `max_trees` and `max_members` are enforced by the REST tree-create and
  member-add handlers (`409 quota_exceeded`).
- `GET /api/v1/tenants/{t}/usage` reports `{trees, branches, members,
  storage_bytes, limits, effective}`. Existing data is never deleted when a
  tenant is over quota.

There are no built-in plan tiers with fixed sizes. `Tenant.plan`
(`free|pro|enterprise`) is a label that a server admin sets.

## Backup and recovery

- The server state is the data directory: `meta.db` plus `objects/`. Back it
  up by stopping the server, or by snapshotting the filesystem, and copying
  both. Object files are immutable, so an object copy made after the `meta.db`
  copy is always a superset.
- A client's `.wt/` can be rebuilt from the server with `wt clone`. Local
  snapshots that were never pushed and the local reflog exist only on that
  machine.
- There is no repair tool for corrupt objects. A client detects corruption on
  read, and deleting the object file and fetching again restores it if the
  server still holds the object.

## Implementation status

| Area | Status |
|---|---|
| BLAKE3 content-addressed store, local (`.wt/objects`) and server (`<data>/objects`), atomic writes, Windows-safe | Built |
| Flat manifests, snapshot records as metadata, tags, releases with assets in the CAS | Built |
| FastCDC chunked large files (> 8 MiB) with entry readers on both sides | Built |
| bbolt metadata store (buckets above) and DocStore collections | Built |
| Fast-forward-only branch updates with compare-and-swap | Built |
| Mark-and-sweep GC with grace period, staged and audit retention | Built |
| Tenant quotas (storage, trees, branches, members) and usage reporting | Built |
| Shallow clone and fetch (`.wt/trees/<tree>/shallow`) | Built |
| Restricted (stubbed) paths (`.wt/trees/<tree>/stubbed`) | Built |
| Integrity verification on server reads | Partial: verified on upload; clients verify on download and on every local read |

## Roadmap

Described in earlier versions of this spec but not built:

- **Pack files and delta compression**: pack format, delta chains, base
  selection. Objects are stored and transferred whole.
- **Type-specific or dictionary-trained compression** (plain zstd per object
  is built; see Compression.md).
- **Per-directory tree objects** and symlink entries. Manifests are flat file
  lists.
- **Physical tenant isolation**: per-tenant volumes or buckets, dedicated
  workers or endpoints. Storage is one shared CAS plus one bbolt file.
- **Pluggable storage backends** (S3-compatible object storage, an external
  database), replication and point-in-time recovery.
- **Local store outside the worktree** (an application-data directory per
  worktree), a chunk cache with LRU eviction, pinning and prefetch, and
  `[storage.cache]` settings.
- **Lazy loading and virtual filesystems** (FUSE, ProjFS) and on-demand
  hydration of stubs.
- **Sparse or partial checkout** (`[sync.sparse]` include and exclude patterns).
- **Reflog sync to the server** for disaster recovery.
- **Automatic re-fetch of corrupt local objects** (`wt fsck` and the server
  `POST /admin/fsck` detect them; see Compression.md).
- **A configurable large-file threshold and chunk sizes.** Today they are
  constants: 8 MiB threshold and 256 KiB / 1 MiB / 4 MiB chunks.
- **Plan-based default quotas** (fixed tiers per plan).

## References

- SDK: `crates/worktree-sdk/src/store.rs`, `src/scan/cdc.rs`,
  `src/repo/mod.rs` (layout), `src/repo/{working,history,refs,reflog,lock}.rs`,
  `src/model.rs`
- Protocol: `crates/worktree-protocol/proto/sync.proto` (`ObjectKind`,
  `ManifestRecord`, `ChunkList`, `SnapshotRecord`),
  `crates/worktree-protocol/src/core/hash.rs`
- Server: `services/server-go/internal/objstore/` (`objstore.go`, `disk.go`,
  `fsretry_*.go`), `internal/meta/` (`bolt.go`, `docs.go`, `history.go`,
  `iam_bolt.go`, `memory.go`), `internal/content/content.go`, `internal/gc/`,
  `internal/limits/`, `internal/hash/`, `internal/domain/{types,v05}.go`,
  `internal/server/server.go`
- `docs/ENGINEERING_PLAN.md` §2, §3, §13.2, §13.3, §13.14
- Related specs: [Sync](../sync/Sync.md), [Server](../server/Server.md),
  [DotWt](../dot-wt/DotWt.md)
