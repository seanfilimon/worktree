# Worktree Server Architecture

The current Rust `worktree-server` crate is a mixed local daemon/prototype server. It watches the
filesystem for local demo flows, creates auto-snapshots through the SDK, and exposes HTTP endpoints
on localhost. The production remote authority is planned as a Go service; it must not watch working
directories or share SDK `.wt/state.json`.

The Go server now has an initial `server-go/` implementation scaffold. It provides health/readiness
endpoints, TLS 1.3 configuration, request IDs, optional static bearer-token authentication for
protected endpoints, tenant/account principal headers, local BLAKE3-verified content-addressed
object storage, a `POST /staged` compatibility endpoint, and a filtered `GET /staged` listing
endpoint. This is still development storage: staged metadata is written to a JSON index for now,
while the production path remains PostgreSQL metadata plus S3-compatible object storage.

Recent prototype work added the first real staged-sync boundary: after an auto-snapshot is created,
the bgprocess synchronously uploads that snapshot to `POST /staged`. The endpoint verifies uploaded
object bytes with BLAKE3 and stores staged metadata in server-side storage, separate from local SDK
state.

## Daemon Lifecycle

TODO: Document how the server daemon starts, stops, and manages its lifecycle. Cover process management, signal handling, graceful shutdown, and crash recovery.

## Filesystem Watcher

TODO: Document the filesystem watching subsystem. Cover platform-specific backends (inotify, FSEvents, ReadDirectoryChangesW), debouncing strategies, ignore patterns, and performance considerations for large trees.

## Event Engine

TODO: Document the event processing pipeline. Cover how filesystem events are collected, filtered, batched, and dispatched to downstream consumers. Include event types, ordering guarantees, and backpressure handling.

## Auto-Commit Engine

The auto-snapshot path lives in `watcher_loop_blocking` and uses the SDK snapshot engine. Filesystem
events are debounced, classified into semantic changes, and evaluated by `AutoCommitEngine`. When a
snapshot is created successfully, the watcher calls `worktree_sdk::engine::sync::push_staged` before
continuing the loop. Sync errors are logged but do not terminate the watcher.

## Auto-Branch Engine

TODO: Document automatic branch management. Cover heuristics for detecting logical branches of work, branch naming strategies, automatic branch switching, and integration with the snapshot engine.

## Storage Backend

The Rust prototype has a disk content-addressable storage backend using BLAKE3 fan-out paths:
`objects/XX/<remaining-hash>`. Staged snapshot metadata is persisted by `storage::staged::StagedStore`
as `staged/index.json` under the server storage root. This is a prototype persistence layer, not the
planned production canonical storage model.

The Go server mirrors that first boundary under `server-go/internal/storage` and
`server-go/internal/staged`. `LocalObjectStore` verifies uploaded bytes against a 64-character BLAKE3
hex digest before writing to `objects/XX/<remaining-hash>`. `staged.FileStore` persists staged
metadata to `staged/index.json` for local development only.

## API Surface

Current prototype HTTP endpoints:

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | Health check |
| `POST` | `/init` | Demo initialization through SDK state |
| `POST` | `/status` | Demo status through SDK state |
| `POST` | `/snapshot` | Demo/manual snapshot creation |
| `POST` | `/branch` | Demo branch create/switch |
| `POST` | `/staged` | Upload one local snapshot as staged work |
| `GET` | `/staged` | List staged snapshots, filtered by tenant/worktree/branch |

`/staged` is the important contract for the production rewrite: the client sends snapshot metadata
and added/modified file bytes, while the server verifies hashes, stores objects, indexes staged
metadata, and returns an ACK only after persistence.

The Go implementation currently supports the same REST compatibility endpoint. The next production
step is to put auth, tenant resolution, IAM, quota checks, and audit logging in front of this
handler before widening the API surface.

Current Go auth is intentionally minimal: `WT_SERVER_AUTH_TOKEN` enables static bearer-token checks,
and `X-WT-Tenant` / `X-WT-Account` populate request principal context. `/staged` rejects requests
when a principal tenant is present and does not match the staged snapshot tenant. Full JWT/API-key
auth, IAM evaluation, and audit logging remain planned work.

`GET /staged` applies the same tenant guard. When `X-WT-Tenant` is present, the response is scoped
to that tenant; an explicit mismatched `tenant` query parameter is rejected.
