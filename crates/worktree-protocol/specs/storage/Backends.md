# Storage Backends and Backups Specification

## Overview

Since v0.7 (`docs/ENGINEERING_PLAN.md` §15.2) the server keeps its objects
either on local disk (the default) or in an S3-compatible bucket, and it
can write point-in-time **backups** of everything it stores: the metadata
database and every object, in one `.wtbak` archive. Backups are created
over the REST API (`wt admin backup create`, the admin panel's Backups
page) or on a schedule, and restored offline with
`worktree-server restore`.

The metadata database (`<data>/meta.db`, bbolt) always lives in the data
directory; only objects move to S3. Object ids, the stored encoding
([Compression](./Compression.md)) and the sync protocol are identical for
both backends, so a store can be migrated between them with a backup and a
restore.

## Object stores

| `--object-store` | Objects at | Notes |
|---|---|---|
| `disk` (default) | `<data>/objects/<2 hex>/<62 hex>` | Atomic temp-file + rename writes, Windows sharing-violation retries ([Storage](./Storage.md)). |
| `s3` | `s3://<bucket>/<prefix>objects/<2 hex>/<62 hex>` | Any S3-compatible API (AWS S3, MinIO, Ceph RGW, R2, ...). |

Both stores implement the same contract (`objstore.Store` plus the
optional `StatStore`, `ListStore` and `SizeStore`):

- **Put** verifies that the data hashes to the id (`ErrHashMismatch`
  otherwise, nothing stored), is a no-op when the object exists, and
  writes the WTZ1 stored encoding (compressed when that saves at least
  10 %, `--compress-objects`).
- **Get** returns the content (raw and WTZ1 files are both accepted;
  undecodable compressed data is `ErrCorrupt`), `ErrNotFound` when absent.
- **Has** / **Delete** (idempotent) / **Size** (stored bytes) / **Stats**
  (count and stored bytes) / **List** (ascending hash order; stops at the
  first callback error and returns it).

### S3 details

- Keys are `<prefix>objects/<2 hex>/<62 hex>`; lowercase hex sorts like
  the hash bytes, and ListObjectsV2 returns keys in UTF-8 binary order, so
  `List` is in hash order without sorting. Keys under the prefix that do
  not have this form are ignored. The stored bytes are exactly what the
  disk store writes, so an `objects/` tree can be copied into a bucket (or
  back) verbatim.
- Writes are single `PutObject` requests with `Content-MD5` and an
  unsigned payload (objects are at most 64 MiB on the wire); concurrent
  Puts of one object write identical bytes.
- `Has` uses `HeadObject`; S3 answers a HEAD on a missing bucket like a
  missing key, so the `/healthz` objects probe additionally calls
  `HeadBucket` (`S3Store.Ping`) and reports `"objects":"error"` (503) when
  the bucket is unreachable or gone.
- At startup the server checks the bucket (creating it when missing) and
  refuses to start when the endpoint is unreachable or the credentials are
  rejected (30 s timeout).
- Credentials: `--s3-access-key` / `--s3-secret-key` (prefer the
  `WT_S3_*` environment variables, flags are visible in process listings);
  when both are empty the `AWS_*` and `MINIO_*` environment variables and
  then the EC2/ECS instance role are used.
- Path-style requests for IP / localhost endpoints, virtual-host style for
  AWS (client default). Client: `github.com/minio/minio-go/v7` v7.0.95.

## Configuration

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--object-store` | `WT_OBJECT_STORE` | `disk` | `disk` or `s3` |
| `--s3-endpoint` | `WT_S3_ENDPOINT` | — | `host[:port]` or `http(s)://host[:port]` (`http://` implies `--s3-insecure`); required for `s3` |
| `--s3-bucket` | `WT_S3_BUCKET` | — | Bucket (created when missing); required for `s3` |
| `--s3-prefix` | `WT_S3_PREFIX` | empty | Key prefix (`wt/`; a trailing `/` is added) |
| `--s3-region` | `WT_S3_REGION` | discovered | Bucket region |
| `--s3-access-key` / `--s3-secret-key` | `WT_S3_ACCESS_KEY` / `WT_S3_SECRET_KEY` | — | Static credentials, given together |
| `--s3-insecure` | `WT_S3_INSECURE` | `false` | Plain HTTP |
| `--backup-interval` | `WT_BACKUP_INTERVAL` | `0` (off) | Period of scheduled backups (at least `1m`) |
| `--backup-keep` | `WT_BACKUP_KEEP` | `7` | Scheduled backups kept (0 keeps all) |

## Backup archives (`.wtbak`)

A backup is one file `<data>/backups/<id>.wtbak`, id
`bk-<yyyymmdd>T<hhmmss>Z-<6 hex>` (UTC creation time + random suffix; also
safe as a file name). The file is a **zstd-compressed POSIX tar stream**
(PAX headers) with, in this order:

| Member | Content |
|---|---|
| `meta.db` | A consistent copy of the bbolt database, written by `tx.WriteTo` inside one read transaction (writers are not blocked). |
| `objects/<2 hex>/<62 hex>` | Every object's **uncompressed content** (one member per object; the outer zstd frame compresses them, and the archive does not depend on `--compress-objects`). |
| `manifest.json` | Last, because it carries the counts. |

`manifest.json`:

```json
{
  "format": "wtbak",
  "version": 1,
  "id": "bk-20261025T120000Z-a1b2c3",
  "created_at": "2026-10-25T12:00:00.123Z",
  "created_by": "admin",
  "note": "before upgrading",
  "server_version": "0.7.0",
  "object_store": "s3",
  "meta_db": { "path": "meta.db", "size": 1048576, "sha256": "<hex>" },
  "objects": 1234,
  "object_bytes": 56789012
}
```

Consistency: the metadata snapshot is taken first, then the object store
is listed. Pushes upload objects before committing metadata, so every
object the snapshot references is listed; objects written later are
harmless extras, and an object deleted meanwhile (GC only deletes objects
unreachable for the whole grace period) is skipped. Unknown members are
ignored by readers (forward compatibility); a manifest `version` newer
than the reader supports is refused.

The archive is written to `<id>.wtbak.partial` and renamed when complete,
then indexed in the document collection `backups` (key = id, value
`BackupInfo`). One backup runs at a time (`409 backup_running`).

### Listing and retention

`GET /admin/backups` lists indexed backups whose file still exists plus
`.wtbak` files in the backup directory without an index entry (archives
copied in, or written after the snapshot a server was restored from),
newest first; the index entry's path is re-derived from the configured
directory. Scheduled backups (`created_by` = `system`) beyond the newest
`--backup-keep` are deleted after each scheduled run; backups created by
people are never pruned automatically.

## REST API (server admins)

| Method and path | Result |
|---|---|
| `POST /api/v1/admin/backups` `{"note"?}` (body optional, note ≤ 500 bytes) | `201 BackupInfo` once the archive is complete; continues if the client disconnects; `409 backup_running`; `501` when the stores cannot be backed up |
| `GET /api/v1/admin/backups` | `BackupStatus` `{config:{dir,interval,interval_seconds,keep}, store:{kind,root?,endpoint?,bucket?,prefix?,region?,secure?,compress}, supported, running, next_run_at?, last_error?, backups:[BackupInfo]}` |
| `GET /api/v1/admin/backups/{id}/download` | The archive (`application/octet-stream`, `Content-Disposition: attachment`, `Range` supported) |
| `DELETE /api/v1/admin/backups/{id}` | `204`; `404` for unknown or malformed ids |

`BackupInfo` = `{id, path, size_bytes, objects, created_at, created_by,
note?}` (`objects` is 0 for unindexed archives). Audit actions:
`backup.create`, `backup.download`, `backup.delete` (scheduled ones are
recorded by the `system` actor).

## Restore (offline)

```
worktree-server restore [--force] --data DIR [--object-store ... --s3-* ...] [--compress-objects] FILE.wtbak
```

The server must be stopped (bbolt holds an exclusive lock). Restore:

1. Refuses a data directory that exists and is not empty unless `--force`
   (exit 1); with `--force`, `meta.db` is replaced and objects are added
   next to existing ones (stale objects are collected by GC).
2. Streams the archive: `meta.db` goes to `meta.db.restoring`; every
   object is checked against its name (BLAKE3) and written to the selected
   object store (disk at `DIR`, or S3 with the same flags as the server),
   whose `Put` verifies the hash again.
3. Verifies `meta.db` against the manifest's size and SHA-256 and the
   object count against `objects`, opens the restored database (schema
   check), and only then renames it to `meta.db`.

A truncated, tampered or foreign archive fails with `corrupt archive`
(exit 1) and leaves no `meta.db` behind; objects already written are
harmless (content-addressed). Restoring into a different backend migrates
the store (disk → S3 or back). Tokens and sessions in the restored
database stay valid. Exit codes: 0 restored, 1 failed, 2 usage.

## CLI and admin panel

- `wt admin backup create [--note TEXT]`, `wt admin backup list`,
  `wt admin backup download ID [-o FILE]` (streams to `FILE.part`, then
  renames; default `./<id>.wtbak`), `wt admin backup delete ID [--yes]`.
  Create and download use a 4-hour request timeout.
- Admin panel `/backups`: backups table (download, delete with
  confirmation), create dialog with an optional note, the object store
  (kind, endpoint/bucket/prefix/region or directory, compression), the
  schedule and keep count, the last scheduled failure, and the restore
  command. Server administrators only.

## Implementation status

| Area | Status |
|---|---|
| `S3Store` (Store, StatStore, ListStore, SizeStore, Ping) with WTZ1 | Built (v0.7 §15.2) |
| `--object-store`, `--s3-*` flags and env, startup bucket check | Built (v0.7 §15.2) |
| In-process fake S3 (`objstore.FakeS3`) and shared store conformance suite | Built (v0.7 §15.2) |
| Real S3 / MinIO conformance test (`WT_S3_TEST_ENDPOINT`) | Built (optional) |
| `.wtbak` backups, index, listing of unindexed archives | Built (v0.7 §15.2) |
| REST endpoints, OpenAPI operations, audit | Built (v0.7 §15.2) |
| Scheduled backups and keep count | Built (v0.7 §15.2) |
| `worktree-server restore` with hash verification, `--force`, disk or S3 target | Built (v0.7 §15.2) |
| `wt admin backup`, admin Backups page | Built (v0.7 §15.2) |
| Uploading backups to S3 / remote targets | Roadmap |
| Incremental backups | Roadmap |

## Roadmap

- Write backups to a bucket (or any object store) instead of the data
  directory, with server-side encryption options.
- Incremental backups (objects since the previous backup's manifest) and
  restore chains.
- Online verification of an archive (`wt admin backup verify`) without
  restoring.
- Multipart uploads and server-side copy for very large objects; a local
  read cache in front of S3.
- Encrypting archives with an operator-provided key.

## References

- Server: `services/server-go/internal/objstore/{s3,s3fake}.go`,
  `internal/backup/{backup,archive,restore}.go`,
  `internal/httpapi/backups.go`, `internal/config/backends.go`,
  `internal/server/objects.go`, `internal/meta/snapshot.go`,
  `restore.go` (subcommand)
- CLI: `crates/worktree-cli/src/commands/admin_backup.rs`
- Admin: `apps/admin/src/pages/BackupsPage.tsx`, `src/api/backups.ts`
- Tests: `services/server-go/internal/objstore/s3_test.go`,
  `internal/backup/backup_test.go`, `internal/httpapi/backups_test.go`,
  `internal/config/backends_test.go`, `restore_test.go`,
  `apps/admin/src/pages/backups.test.tsx`,
  `crates/worktree-e2e/tests/e2e/backups.rs`
- `docs/ENGINEERING_PLAN.md` §15.2
- Related specs: [Storage](./Storage.md), [Compression](./Compression.md),
  [Server](../server/Server.md)
