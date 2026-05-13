# Worktree Server Architecture

The current Rust `worktree-server` crate is a mixed local daemon/prototype server. It watches the
filesystem for local demo flows, creates auto-snapshots through the SDK, and exposes HTTP endpoints
on localhost. The production remote authority is implemented as a Go service; it must not watch
working directories or share SDK `.wt/state.json`.

The Go server now has an initial `server-go/` implementation scaffold. It provides health/readiness
endpoints, Prometheus-style request counters at `/metrics`, TLS 1.3 configuration, request IDs,
bearer-token authentication for protected endpoints, optional file-backed demo credentials,
local BLAKE3-verified content-addressed object storage, a `POST /staged` compatibility endpoint, a
filtered `GET /staged` listing endpoint, and file-backed JSONL audit records for staged allow/deny
decisions. It also starts a gRPC `SyncService` on `WT_SERVER_GRPC_ADDR`, default `127.0.0.1:9877`,
with `StageSnapshot` and `ListStagedSnapshots` behind the same bearer-auth principal path.

Staged metadata can now be stored durably in Postgres. When `WT_SERVER_DATABASE_URL` is set,
`main.go` selects `PostgresStagedStore`; otherwise it uses the local file store for development.
Both stores enforce staged idempotency on tenant/worktree/tree/branch/snapshot plus canonical object
refs (`path`, `hash`, `size`). Identical retries are accepted without duplicate metadata; conflicting
retries are rejected. `WT_SERVER_RUN_MIGRATIONS=true` runs embedded SQL migrations. Staged uploads
are bounded by configurable development limits before object persistence.

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
metadata to `staged/index.json` for local development only. `PostgresStagedStore` persists staged
metadata through the `staged_snapshots` and `staged_snapshot_objects` tables. Idempotency is keyed
by tenant/worktree/tree/branch/snapshot identity and a canonical payload hash, not bare
`snapshot_id`.

The Go server also writes staged access decisions through `server-go/internal/audit`. The current
development recorder appends JSON lines to `.wt-server-go/audit/audit.jsonl` by default, or the path
set by `WT_SERVER_AUDIT_PATH`. This covers `POST /staged` and `GET /staged` allow/deny outcomes
with action, reason, tenant, account, token ID, auth method, resource, request ID, and HTTP route
metadata. The production audit target should become immutable durable storage with query indexes.

The current staged upload guard rejects requests that exceed `WT_SERVER_MAX_STAGED_OBJECT_BYTES`
per object or `WT_SERVER_MAX_STAGED_OBJECTS` per request. These are deployment-safety limits, not
tenant quota accounting; production quotas still need tenant-aware storage accounting and durable
rate-limit state.

Three migrations currently exist in `server-go/migrations`: staged snapshot records, staged object
references, and audit events. Docker Compose runs these migrations before starting the server; the
binary can also apply embedded migrations with `WT_SERVER_RUN_MIGRATIONS=true`.

## API Surface

Current prototype HTTP endpoints:

| Method | Path        | Purpose                                                   |
| ------ | ----------- | --------------------------------------------------------- |
| `GET`  | `/health`   | Health check                                              |
| `GET`  | `/ready`    | Readiness check                                           |
| `GET`  | `/metrics`  | Prometheus-style HTTP request counters                    |
| `POST` | `/init`     | Demo initialization through SDK state                     |
| `POST` | `/status`   | Demo status through SDK state                             |
| `POST` | `/snapshot` | Demo/manual snapshot creation                             |
| `POST` | `/branch`   | Demo branch create/switch                                 |
| `POST` | `/staged`   | Upload one local snapshot as staged work                  |
| `GET`  | `/staged`   | List staged snapshots, filtered by tenant/worktree/branch |

`/staged` is the important contract for the production rewrite: the client sends snapshot metadata
and added/modified file bytes, while the server verifies hashes, stores objects, indexes staged
metadata, and returns an ACK only after persistence.

The Go implementation currently supports the same REST compatibility endpoint. Protected staged
endpoints authenticate bearer tokens, derive tenant/account principals from server-side credentials,
and call the shared `Authorizer` before persistence or listing. `WT_SERVER_AUTH_CREDENTIALS_PATH`
loads JSON demo credentials; `WT_SERVER_IAM_POLICY_PATH` loads JSON demo policy rules.

`AllowAllAuthorizer` is no longer the production default. It is only available for tests or the
explicit `WT_SERVER_IAM_MODE=allow-all-dev` escape hatch, which production config rejects. Full
JWT/OIDC, API-key lifecycle, declarative `.wt/access/*.toml` parsing, tenant/team/role repositories,
quota checks, and full RBAC/ABAC parity remain planned.

`GET /staged` applies the same tenant guard. When an authenticated tenant is present, the response is
scoped to that tenant; an explicit mismatched `tenant` query parameter is rejected.

### gRPC API Surface

The Go server also exposes:

| Service       | Method                | Purpose                                                   |
| ------------- | --------------------- | --------------------------------------------------------- |
| `SyncService` | `StageSnapshot`       | Upload one staged snapshot using protobuf bytes           |
| `SyncService` | `ListStagedSnapshots` | List staged snapshots with tenant/worktree/branch filters |

The gRPC service shares object storage, staged storage, audit, auth, and IAM with REST. A unary auth
interceptor rejects missing or invalid bearer metadata before handlers run, so both transports
exercise the same server boundary.

### Docker Compose

`server-go/docker-compose.yml` provides a local production-shaped stack:

| Service    | Purpose                                |
| ---------- | -------------------------------------- |
| `postgres` | Metadata database                      |
| `migrate`  | Applies `server-go/migrations`         |
| `server`   | Runs HTTP on `8080` and gRPC on `9877` |
