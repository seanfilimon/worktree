# Contribution Progress

## Go Server — Implementation Progress

Target: production remote authority in `server-go/`, built from the language-neutral protocol
contract in `docs/protocol-spec.md` and the deployment sequence in `roadmap.md`.

### Phase A — Rust/Go staged contract compatibility
**Files:** `crates/worktree-sdk/src/engine/sync.rs`, `server-go/internal/httpapi/staged.go`, `docs/protocol-spec.md`
- Updated Rust staged upload requests to match the Go server contract:
  `snapshot_id`, `tenant`, `worktree`, `tree_id`, `branch`, and `objects`.
- Replaced the old `content_base64` JSON field with `content`; Go decodes it into `[]byte`.
- Added `WT_SERVER_URL` for Rust client endpoint selection.
- Added `WT_TENANT` for staged upload tenant selection.
- Added serialization tests that reject the old field shape.

### Phase B — Protocol contract hardening
**Files:** `docs/protocol-spec.md`, `crates/worktree-protocol/specs/sync/Sync.md`, `crates/worktree-protocol/specs/visibility/StagedVisibility.md`, `crates/worktree-protocol/specs/iam/IAM.md`
- Documented the staged REST compatibility request/response shape.
- Added structured error codes for malformed JSON, invalid staged snapshots, invalid objects,
  upload limits, auth failures, IAM denial, store/list failures, and unsupported methods.
- Normalized server IAM action names around strings such as `staged:create` and `staged:list`.
- Documented staged upload idempotency on canonical staged identity: tenant, worktree, tree/tree_id, branch, snapshot_id, and object refs (`path`, `hash`, `size`).
- Documented conflict behavior for retries with the same staged identity but different payloads.

### Phase C — Postgres staged metadata
**Files:** `server-go/migrations/`, `server-go/internal/staged/postgres.go`, `server-go/internal/staged/store.go`, `server-go/cmd/wt-server/main.go`
- Added migrations for `staged_snapshots`, `staged_snapshot_objects`, and `audit_events`.
- Added `PostgresStagedStore` implementing the staged store interface.
- Upgraded staged metadata to preserve canonical object refs (`path`, `hash`, `size`) and `payload_hash`.
- Fixed staged idempotency for both the file store and Postgres store:
  - identical retries return `already_exists` without adding duplicate metadata
  - conflicting retries return `ErrConflict`
  - Postgres uniqueness is scoped to tenant/worktree/tree/branch/snapshot identity instead of relying on bare `snapshot_id`
- Made `main.go` choose Postgres when `WT_SERVER_DATABASE_URL` is set, otherwise falling back to the file store for local development.
- Added an embedded migration runner that applies packaged SQL migrations when `WT_SERVER_RUN_MIGRATIONS=true`.

### Phase D — IAM authorizer seam and initial policy authorizer
**Files:** `server-go/internal/iam/authorizer.go`, `server-go/internal/httpapi/staged.go`, `server-go/internal/grpc/server.go`, `server-go/cmd/wt-server/main.go`
- Added the `Authorizer` interface.
- Added `AllowAllAuthorizer` and `DenyAllAuthorizer` for tests and explicit development mode.
- Replaced production startup wiring so `AllowAllAuthorizer` is no longer the default.
- Added an initial default-deny `PolicyAuthorizer` with wildcard action/resource matching and principal scope checks.
- Added JSON policy rule loading via `WT_SERVER_IAM_POLICY_PATH` for demo-ready allow/deny behavior.
- Wired authorizer checks into both `POST /staged` and `GET /staged`.
- Wired the same authorizer into `SyncService.StageSnapshot` and `SyncService.ListStagedSnapshots`.
- Shared the same action names with audit records: `staged:create` and `staged:list`.

### Phase E — gRPC sync service
**Files:** `server-go/proto/worktree/v1/sync.proto`, `server-go/internal/grpc/server.go`, `server-go/internal/grpc/auth_interceptor.go`, generated `worktreepb`
- Added `SyncService` via buf-generated Go bindings.
- Implemented `StageSnapshot`.
- Implemented `ListStagedSnapshots`.
- Shared object storage, staged storage, audit recorder, and IAM authorizer with REST.
- Added a gRPC unary auth interceptor that reads bearer credentials from metadata, validates them, and injects `auth.Principal` into context.
- Updated staged gRPC handlers to reject missing auth, enforce tenant match, pass the real principal into IAM, and audit the authenticated account.
- Started the gRPC server on `WT_SERVER_GRPC_ADDR`, default `127.0.0.1:9877`.

### Phase F — Docker and local production stack
**Files:** `server-go/Dockerfile`, `server-go/docker-compose.yml`, `server-go/.env.example`
- Added a two-stage Go Dockerfile.
- Added Docker Compose with Postgres, a migration runner, and the server.
- Exposed HTTP on `8080` and gRPC on `9877`.
- Added demo bearer-auth environment in Compose: `WT_SERVER_AUTH_TOKEN=dev-secret`, tenant `acme`, account `alice`, scopes `staged:*`, IAM mode `policy`.
- Added example credential and policy files under `server-go/examples/` for a multi-principal demo.
- Added environment examples for server config, Postgres, Rust client URL, tenant selection, and auth.

### Phase G — Production bearer auth for REST and gRPC
**Files:** `server-go/internal/auth/auth.go`, `server-go/internal/httpapi/middleware.go`, `server-go/internal/grpc/auth_interceptor.go`, `server-go/internal/config/config.go`, `server-go/cmd/wt-server/main.go`
- Added shared `auth.Authenticator` interface for HTTP and gRPC.
- Added bearer-token authenticator that stores SHA-256 token hashes and compares credentials in constant time.
- Added JSON credential file loading via `WT_SERVER_AUTH_CREDENTIALS_PATH`.
- Extended `auth.Principal` with tenant, account, subject, token ID, auth method, scopes, and authenticated state.
- Replaced direct static-auth usage in protected HTTP routes with the authenticator interface.
- Added production config guards so `WT_SERVER_ENV=production` requires bearer auth and rejects `allow-all-dev` IAM mode.
- Added `WT_SERVER_AUTH_MODE`, `WT_SERVER_IAM_MODE`, `WT_SERVER_AUTH_TENANT`, `WT_SERVER_AUTH_ACCOUNT`, `WT_SERVER_AUTH_SCOPES`, `WT_SERVER_AUTH_CREDENTIALS_PATH`, and `WT_SERVER_IAM_POLICY_PATH`.
- Updated Rust SDK staged sync to send `Authorization: Bearer` when `WT_SERVER_AUTH_TOKEN` is set.
- Kept static header-derived identity only as `static-dev` compatibility mode.

### Phase H — Protocol staged permissions
**Files:** `crates/worktree-protocol/src/iam/permission.rs`, `crates/worktree-protocol/src/iam/role.rs`, `crates/worktree-protocol/specs/iam/IAM.md`
- Added protocol permissions `staged:create` and `staged:list`.
- Updated built-in protocol roles so Viewer can list staged snapshots but cannot create them.
- Updated Developer, Maintainer, Admin, and Owner role behavior for staged create/list.
- Updated IAM spec status to reflect initial Go policy-backed decisions and production auth work.

### Phase I — Canonical Push/Pull REST Endpoints
**Files:** `crates/worktree-sdk/src/engine/sync.rs`
- Refactored `StagedReq` to `CanonicalPushReq` and `StagedObjectUpload` to `CanonicalObjectUpload`.
- Added `remote_tip` to enable Compare-And-Swap (CAS) negotiations on push.
- Introduced `CanonicalPullReq` including `tenant`, `worktree`, `tree_id`, `branch`, and `remote_tip`.
- Updated push flow to route to `/api/push` rather than legacy `/staged`.
- Updated pull flow to route to `/api/pull` rather than legacy `/status`.
- Updated serialization tests to assert that `CanonicalPushReq` conforms to the canonical shape expected by the Go Server.

### Step 1 — `docs(protocol): connect server roadmap to protocol contract`
**Files:** `docs/protocol-spec.md`, `roadmap.md`
- Connected the consolidated protocol spec to the Go server roadmap.
- Established that protocol behavior must be specified before Rust/Go implementation.
- Added explicit cross-language compatibility expectations.

### Step 2 — `feat(server-go): scaffold remote server runtime`
**Files:** `server-go/`
- Added a standalone Go module for the production remote server.
- Added standard-library HTTP server setup with graceful shutdown.
- Added JSON logging through `log/slog`.
- Added request ID middleware.
- Added `GET /health` and `GET /ready`.
- Added TLS 1.3 minimum-version configuration.

### Step 3 — `feat(server-go): add staged object upload`
**Files:**
- `server-go/internal/storage/object.go`
- `server-go/internal/staged/store.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/README.md`
- `docs/protocol-spec.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added local content-addressed object storage with BLAKE3 verification.
- Added `POST /staged` compatibility endpoint.
- Added JSON staged metadata persistence for local development.
- Later upgraded the file staged store to canonical identity idempotency with conflict detection.
- Added tests for object verification, staged upload behavior, idempotent replay, and conflicting replay.
- Updated docs to describe the Go endpoint as the current compatibility bridge.

**Result:** `go test ./...` passes in `server-go` with workspace-local Go cache settings.

### Step 4 — `feat(server-go): add auth tenant context`
**Files:**
- `server-go/internal/auth/auth.go`
- `server-go/internal/httpapi/middleware.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/README.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added the original static development auth path via `WT_SERVER_AUTH_TOKEN`.
- Later upgraded protected endpoints to use the shared `auth.Authenticator` interface.
- Added production bearer-token principal derivation from configured credentials.
- Kept `X-WT-Tenant` / `X-WT-Account` identity only for `static-dev` compatibility; production bearer mode does not trust those headers.
- Scoped auth middleware to protected endpoints instead of public health/readiness endpoints.
- Added `/staged` tenant mismatch rejection when authenticated tenant context is present.
- Added unit tests for static auth, bearer auth, and staged tenant mismatch behavior.

**Result:** `go -C server-go test ./...` passes.

### Step 5 — `feat(server-go): list staged snapshots`
**Files:**
- `server-go/internal/staged/store.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/README.md`
- `docs/protocol-spec.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added `Store.List` with tenant/worktree/branch filters.
- Added `GET /staged` compatibility endpoint.
- Scoped list results to the authenticated tenant when present.
- Rejected explicit tenant query mismatches.
- Updated list authorization to use a tenant-scoped staged resource instead of the old generic `staged` resource.
- Added store and HTTP tests for listing and tenant filtering.

**Result:** `go test ./...` passes in `server-go`.

### Step 6 — `feat(server-go): expose basic metrics`
**Files:**
- `server-go/internal/observability/metrics.go`
- `server-go/internal/httpapi/middleware.go`
- `server-go/internal/httpapi/router.go`
- `server-go/README.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added in-memory HTTP request counters.
- Added Prometheus-style `GET /metrics`.
- Added middleware that records method/path/status labels.
- Added unit tests for metrics rendering and router exposure.

**Result:** `go test ./...` passes in `server-go`.

### Step 7 — `feat(server-go): audit staged access decisions`
**Files:**
- `server-go/internal/audit/audit.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/internal/httpapi/middleware.go`
- `server-go/internal/config/config.go`
- `server-go/cmd/wt-server/main.go`
- `server-go/README.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added an audit recorder interface with a file-backed JSONL implementation.
- Added `WT_SERVER_AUDIT_PATH`, defaulting to `.wt-server-go/audit/audit.jsonl`.
- Recorded `POST /staged` and `GET /staged` allow/deny decisions with action, reason, tenant,
  account, resource, request ID, method, and path.
- Kept audit write failures out of the request path for this prototype slice.
- Added unit tests for JSONL audit persistence and staged audit events.

**Result:** `go test ./...` passes in `server-go`.

### Step 8 — `feat(server-go): limit staged upload payloads`
**Files:**
- `server-go/internal/config/config.go`
- `server-go/internal/config/config_test.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/internal/httpapi/staged_test.go`
- `server-go/cmd/wt-server/main.go`
- `server-go/README.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added `WT_SERVER_MAX_STAGED_OBJECT_BYTES`, defaulting to 64 MiB per staged object.
- Added `WT_SERVER_MAX_STAGED_OBJECTS`, defaulting to 1024 objects per staged upload request.
- Rejected oversized staged uploads with structured `StagedUploadTooLarge` responses.
- Recorded limit rejections through the same staged audit decision path.
- Added config and HTTP tests for limit parsing and staged upload rejection.

**Result:** `go test ./...` passes in `server-go`.

### Step 9 — `feat(server-go): Staged Visibility via WebSockets & JWT Auth`

- Implemented `POST /login` to issue 7-day expiring JWTs.
- Replaced static Bearer tokens with JWT authentication.
- Created `StagedBroadcaster` memory hub for real-time pub/sub.
- Added `GET /staged/ws` for WebSocket upgrade and live streaming.
- Integrated `broadcaster.Publish()` into the HTTP staging upload flow.

## Local Demo — Implementation Progress

Target: full local demo (CLI → bgprocess → server on localhost). No remote server needed.

### Step 1 — `feat(server): implement DiskStorage store/retrieve`
**File:** `crates/worktree-server/src/storage/disk.rs`
- `store()`: creates fan-out dir (`objects/XX/`) then writes object bytes atomically
- `retrieve()`: reads bytes by content hash; returns `ServerError::Storage` on miss
- Added 2 new tests: round-trip store→retrieve, missing-object error
- Added `tempfile = "3"` to `[dev-dependencies]` in `worktree-server/Cargo.toml`

**Also fixed:** pre-existing compile break in `auth/enforcer.rs` — `Scope::Tree` variant grew
a `TenantId` arg but 3 test call sites weren't updated. Moved `TenantId` import into
`#[cfg(test)]` to keep production code warning-free.

### Step 2 — `feat(server): implement classify_event`
**File:** `crates/worktree-server/src/engine/event.rs`
- Classifies `DebouncedEvent` → `SemanticEvent` by filename / path prefix:
  - `Cargo.toml`, `package.json`, `go.mod`, etc. → `DependencyChange`
  - `.wt/` or `.wt-tree/` path components, `.editorconfig`, `.gitignore`, etc. → `ConfigChange`
  - Everything else → `CodeChange`
- Uses `TreeId::nil()` placeholder (tree registry not yet wired)
- Added 6 unit tests covering all three variants + path membership check

**Result:** `cargo test -p worktree-server` → 32/32 pass, 0 warnings in server crate.

### Step 3 — `feat(sync): upload auto-snapshots as staged snapshots`
**Files:**
- `crates/worktree-sdk/src/engine/sync.rs`
- `crates/worktree-server/src/lib.rs`
- `crates/worktree-server/src/api/handlers.rs`
- `crates/worktree-server/src/storage/staged.rs`

- Added SDK staged sync:
  - `push()` now routes to `push_latest_staged()`
  - `push_staged(&engine, snapshot_id)` uploads one local snapshot to `POST /staged`
  - Computes added/modified/deleted paths relative to the parent snapshot
  - Uploads added/modified file bytes as base64 and verifies local BLAKE3 before send
- Added server `POST /staged` endpoint:
  - Validates uploaded paths are relative
  - Decodes base64 content
  - Verifies size and BLAKE3 hash
  - Stores content-addressed objects in server `DiskStorage`
  - Stores snapshot metadata as a server object
  - Records a protocol `StagedSnapshot` in a server-side staged index
- Added `StagedStore`:
  - Persists `StagedIndex` at server storage root under `staged/index.json`
  - Keeps staged visibility state separate from SDK `.wt/state.json`
- Wired bgprocess auto-snapshot path:
  - After `create_snapshot()` succeeds in `watcher_loop_blocking`, it synchronously calls
    `worktree_sdk::engine::sync::push_staged(&engine, &snap.id)`
  - Sync failures are logged without killing the watcher loop

### Step 4 — `feat(sync): decouple push queue and implement backfill`
**Files:**
- `crates/worktree-sdk/src/engine/sync.rs`
- `crates/worktree-server/src/lib.rs`

- **Decoupled Push Queue**:
  - Moved `push_staged` invocation in `bgprocess` from the blocking watcher thread to an asynchronous background MPSC queue.
  - Spawned a dedicated worker thread in `watcher_loop_blocking` to process `PushQueueEvent` messages.
  - Ensures filesystem events are processed immediately even during large blob uploads or network latency.
- **Backfill on Resume**:
  - Implemented `push_unpushed(engine)` in the SDK to compute and upload missing snapshot deltas based on `remote_tip` tracking.
  - Updated `push_staged` to persist `remote_tip` in `WorktreeState` upon successful server confirmation.
  - Added filesystem watcher hook to detect the deletion of `.wt/cache/sync_paused`.
  - Automatically triggers a backfill job when sync is resumed via CLI or manual file deletion.

**Go IAM/server rewrite note:** The Rust endpoint is intentionally a compatibility bridge. The
client contract is now `POST /staged`, and IAM remains server-side; bgprocess does not enforce
permissions. The Go server can replace the endpoint implementation without changing the SDK call
shape.

**Also fixed while verifying workspace:**
- Added `crates/worktree-git/build.rs` to link `advapi32` on Windows for libgit2 tests.
- Fixed protocol JSON serialization for `InMemoryHashIndex` by serializing as mappings rather
  than JSON object keys.
- Fixed directory-only ignore matching for `.wt/`, `.git/`, `node_modules/`, etc.

**Result:** `cargo fmt --all` and `cargo test --workspace` pass.

---

### Step 5 — `feat(cli/daemon): WebSocket listener and interactive watch UI`

- Added `wt auth login` CLI command to retrieve and cache JWTs.
- Updated `worktree-server` daemon with a background `tokio-tungstenite` task.
- Reconciles live WebSocket JSON payloads into `.wt/cache/staged_index.json`.
- Added interactive real-time `wt staged --watch` terminal UI.

## Git Interoperability

### `feat(git): implement libgit2 converter stubs`
- Implemented `WorktreeToGitConverter` to export W0rktree `Snapshot`, `Manifest`, and `Blob` into Git.
- Implemented `GitToWorktreeConverter` to import Git commits, trees, and blobs into W0rktree.
- Added deterministic mapping of Git object OIDs to W0rktree `SnapshotId`s and emails to `AccountId`s.
- **Deferred**: CLI integration for `wt git import` and `wt git export` is currently mocked and deferred until native network stability is finalized.

## Fixes

### `fix(git): sync InMemoryHashIndex with refactored HashIndex trait`
**File:** `crates/worktree-git/src/hash_index/store.rs`
- Rewrote `HashIndex` impl to match current trait signature
- `insert` now returns `bool` (dropped `Result`/associated error type)
- Renamed `lookup_by_git` → `get_blake3`, `lookup_by_content` → `get_sha1`
- Added missing `remove_by_blake3`, `remove_by_sha1`, `len`
- Fixed `HashMapping` field refs: `git_hash`/`content_hash` → `sha1`/`blake3`
- Updated tests; added coverage for remove operations and idempotent insert

**Also:** Added note to `CLAUDE.md` to keep `worktree-git` local impls in sync with protocol trait changes.
\n### Step 4 — `feat(server): fix Two-Runtime architectural violation`\n**Files:**\n- `crates/worktree-server/src/storage/server_state.rs`\n- `crates/worktree-server/src/api/handlers.rs`\n- `crates/worktree-server/src/lib.rs`\n- `agents.md`\n\n- Created an independent server-side canonical storage for the local daemon using `ServerStateStore` instead of relying on the local working directory's SDK `.wt/state.json`.\n- Decoupled API handlers (`handle_init`, `handle_status`, `handle_snapshot`, and `handle_branch`) from the local `WorktreeEngine` allowing the daemon/prototype server HTTP endpoints to safely manage server canonical state locally.\n- Kept the `watcher_loop_blocking` correctly connected to the SDK `WorktreeEngine` to honor the bgprocess responsibilities, completely decoupling the background watcher from the server's remote API emulation.\n- Updated `agents.md` to reflect that the server-side canonical storage constraint has now been implemented.
