# W0rkTree Go Server Roadmap

## Purpose

The remote W0rkTree server may be implemented in Go. The language choice is not the architectural constraint. The real constraints are that the remote server is the canonical source of truth, enforces IAM and policy decisions, stores authoritative history, and never behaves like the local bgprocess.

This roadmap defines how to rebuild the remote server as a production-ready Go service while preserving the Rust protocol/client ecosystem.

## Why Rebuild The Server Boundary

The current Rust `worktree-server` crate mixes remote-server concerns with bgprocess behavior. It watches local files, opens SDK state from a working directory, and exposes handlers that create local snapshots and branches. That is useful for early development, but it violates the server specification:

- The server must never watch a developer filesystem.
- The server must never create snapshots from filesystem observation.
- The server must never share local SDK `state.json` as canonical storage.
- The bgprocess must never be trusted for IAM decisions.
- The server must be deployable as a remote, multi-tenant authority.

Rebuilding the remote server in Go gives us a clean deployment-oriented boundary without carrying forward the mixed local/remote shape.

## Non-Negotiable Invariants

1. The server is the source of truth for canonical history.
2. The bgprocess is untrusted and only creates local snapshots and syncs them.
3. All IAM, license, branch protection, quota, and audit decisions happen server-side.
4. History is append-only.
5. Object identity uses BLAKE3 and remains compatible with `worktree-protocol`.
6. Protocol contracts are language-neutral.
7. The server never touches a working directory.
8. Local `.wt/` state is a cache, not authority.
9. Every rejected operation returns a structured machine-readable error.
10. Every access decision is auditable.

## Target Architecture

The production server should be a stateless Go service backed by durable external systems.

```text
clients / bgprocess
    |
    | gRPC sync, REST, WebSocket
    v
wt-server-go
    |
    +-- PostgreSQL: tenants, accounts, teams, policies, branches, staged indexes,
    |               merge requests, quotas, audit indexes
    |
    +-- S3-compatible object storage: content-addressed objects, chunks, packs
    |
    +-- Redis or equivalent: optional rate limits, sessions, fanout, ephemeral state
    |
    +-- OpenTelemetry / Prometheus / structured logs
```

Go should own the remote server runtime. Rust should continue to own the protocol crate, SDK, CLI, bgprocess, Git bridge, and local object construction unless later decisions change that.

## Proposed Go Layout

```text
server-go/
  cmd/wt-server/
  internal/
    api/              REST and WebSocket endpoints
    sync/             gRPC sync service
    iam/              policy parser, compiler, evaluator, cache
    auth/             JWT, API keys, mTLS identity
    storage/          content-addressed object store and metadata repositories
    tenants/          tenant, account, team, organization model
    branches/         canonical branch heads and append-only DAG
    staged/           staged snapshot storage, indexes, event fanout
    protection/       branch protection enforcement
    license/          SPDX and license grant enforcement
    merge/            merge request lifecycle
    quota/            storage and rate limit enforcement
    audit/            append-only structured audit events
    config/           server configuration
    observability/    logs, metrics, traces, health
  proto/
  migrations/
  deploy/
    docker/
    k8s/
    terraform/
```

## Phase 1: Spec And Contract Cleanup

Before implementation, update the specs so the server is explicitly language-neutral and remote-only.

Work:

- Treat [`docs/protocol-spec.md`](docs/protocol-spec.md) as the implementation-facing protocol contract for the Go server and Rust clients.
- Update `crates/worktree-protocol/specs/server/Server.md`.
- Update `crates/worktree-protocol/specs/sync/Sync.md`.
- Update `crates/worktree-protocol/specs/iam/IAM.md`.
- Update `crates/worktree-protocol/specs/visibility/StagedVisibility.md` if staged event semantics need clarification.
- Backfill `docs/protocol-spec.md` from the authoritative protocol specs so it names object formats, staged snapshot flow, push/pull contracts, wire framing, and compatibility rules in one place.
- Normalize permission names. The specs currently mix names like `sync:push`, `branch:push`, `PolicyManage`, and `policy:manage`.
- Define exact sync, branch, staged snapshot, and auth message schemas.
- Define which payloads are Protobuf, which remain bincode, and how object bytes are verified.

Why:

The Go server and Rust clients need one shared contract. Without this, the Go implementation will drift from the Rust protocol crate and become another incompatible server.

`docs/protocol-spec.md` is the bridge between product architecture and implementation. The detailed specs under `crates/worktree-protocol/specs/` remain authoritative, but the Go server should be built against the consolidated contract in `docs/protocol-spec.md` so deployment work does not depend on reading scattered TODOs or Rust-only implementation details.

Deliverables:

- Updated specs.
- Updated `docs/protocol-spec.md` with Go/Rust compatibility requirements.
- Permission vocabulary table.
- Initial `.proto` files for sync/auth/branch/staged APIs.
- Cross-language fixture plan for Rust-generated objects verified by Go.

## Phase 2: Go Server Skeleton

Create the deployable service shell before implementing product behavior.

Status: implemented in `server-go/` with standard-library HTTP routing, JSON logging, request IDs,
health/readiness endpoints, a Prometheus-style `/metrics` endpoint, graceful shutdown, TLS 1.3
minimum-version configuration, optional static bearer-token auth for protected endpoints,
tenant/account principal headers, file-backed JSONL audit records for staged endpoints, configurable
staged upload safety limits, gRPC listener startup, a two-stage Dockerfile, Docker Compose, and an
`.env.example`.

Work:

- Add `server-go/`.
- Add `cmd/wt-server`.
- Add server config loading from environment and config file.
- Add structured JSON logging.
- Add request IDs.
- Add health and readiness endpoints.
- Add Prometheus metrics endpoint.
- Add OpenTelemetry hooks.
- Add graceful shutdown.
- Add Dockerfile and local compose file.
- Add database migration system.
- Add CI tasks for `go test`, `go vet`, formatting, and static checks.

Why:

Deployment properties should be built into the service from the start. Retrofitting observability, shutdown, migrations, and packaging after core logic is already written usually causes avoidable rewrites.

Deliverables:

- `server-go` builds and runs.
- `/health`, `/ready`, and `/metrics` work.
- Empty gRPC and REST services boot.
- Dockerized local development stack works.

## Phase 3: Canonical Storage

Implement authoritative server-side storage independent of local SDK state.

Status: staged metadata now has both development file storage and a Postgres implementation.
`server-go/migrations/` contains tables for `staged_snapshots`, `staged_snapshot_objects`, and
`audit_events`. Staged storage now records canonical object refs (`path`, `hash`, `size`) and a
payload hash so retried uploads are idempotent only when they are semantically identical. Conflicting
retries are rejected instead of being silently ignored. `cmd/wt-server/main.go` selects Postgres
automatically when `WT_SERVER_DATABASE_URL` is set and can run embedded migrations when
`WT_SERVER_RUN_MIGRATIONS=true`. General canonical branch/snapshot storage remains planned.

Work:

- Design PostgreSQL schema for tenants, accounts, worktrees, trees, branches, branch heads, snapshots, snapshot parents, object references, staged snapshots, tags, releases, and audit metadata.
- Implement object storage interface:
  - local filesystem for development
  - S3-compatible backend for deployment
- Verify every uploaded object against its BLAKE3 hash.
- Store objects by content hash with fanout paths.
- Store tenant/worktree reference graphs separately from shared object bytes.
- Add idempotent writes for object uploads and sync operations.

Why:

The server must be canonical. Local `.wt/` and SDK files cannot be used as production storage. Separating object bytes from tenant reference graphs also allows deduplication without leaking data across tenants.

Deliverables:

- Durable metadata schema.
- CAS object storage with BLAKE3 verification.
- Repository interfaces and tests.
- Migration scripts.

## Phase 4: Auth And IAM

Build first-party IAM in Go instead of delegating core semantics to a generic policy engine.

Status: the enforcement seam is implemented and no longer defaults to `AllowAllAuthorizer` in the
production startup path. `Authorizer` is wired into `POST /staged`, `GET /staged`,
`SyncService.StageSnapshot`, and `SyncService.ListStagedSnapshots`, using normalized permission
actions `staged:create` and `staged:list`. A bearer-token authenticator now derives request
principals from validated credentials instead of trusting tenant/account headers, and the gRPC
server uses a unary auth interceptor. `AllowAllAuthorizer` remains available only for tests or an
explicit `WT_SERVER_IAM_MODE=allow-all-dev` development escape hatch; production config rejects it.
An initial default-deny policy authorizer gates staged create/list using principal scopes such as
`staged:*`. The server can now load bearer credentials from `WT_SERVER_AUTH_CREDENTIALS_PATH` and
policy rules from `WT_SERVER_IAM_POLICY_PATH`, which makes local demos multi-principal without
trusting headers. Full declarative `.wt/access/*.toml` parsing, team/role repository integration,
full RBAC/ABAC, scope resolution, and ceiling-model parity are still planned.

Work:

- Implement authentication:
  - JWT bearer tokens
  - API keys for automation
  - future mTLS identity support
- Implement tenant, account, team, and role resolution.
- Parse `.wt/access/*.toml` and `.wt-tree/access/*.toml`.
- Compile policies into server-side cached forms.
- Implement scope evaluation:
  - Global
  - Tenant
  - Tree
  - Branch
  - RegisteredPath
- Enforce default deny.
- Enforce deny-overrides at the same scope level.
- Enforce root/tree ceiling model.
- Add ABAC condition evaluation.
- Audit every decision.
- Gate every endpoint through auth and IAM from the beginning.

Why:

IAM is a protocol feature, not a hosting add-on. It must be deterministic, testable, and compatible with version-controlled policy files. Starting with IAM early prevents unauthenticated endpoint behavior from becoming embedded in the design.

Deliverables:

- IAM parser/compiler/evaluator.
- Policy cache invalidation.
- Access decision audit records.
- Unit and integration tests for deny precedence, specificity, and ceiling behavior.

Status: the Go server has a first audit recorder for staged upload/list allow and deny decisions.
This is not full IAM yet; it records the decision points that currently exist so later policy
evaluation can be wired into the same audit path.

## Phase 5: Staged Snapshot Sync

Implement automatic bgprocess upload of staged snapshots.

Rust prototype status: the SDK and mixed Rust server now have a compatibility implementation of the
single-snapshot staged upload path. `push_staged` posts to `POST /staged`; the endpoint verifies
BLAKE3 hashes, stores object bytes, and persists staged metadata in a JSON `StagedIndex`. This is
reference behavior for the Go implementation, not the production storage/IAM design.

Go prototype status: `server-go` now exposes `POST /staged` and `GET /staged` with the same
compatibility semantics. It verifies JSON-uploaded object bytes against BLAKE3 hashes, writes
objects to local fan-out storage, persists staged metadata through either the file store or
Postgres, and returns an ACK only after persistence. Staged allow/deny decisions are written to
audit, and staged uploads are bounded by configurable per-object and per-request object-count
limits. Tenant-aware quota accounting and WebSocket fanout are still pending.

The Go endpoint now has production-auth foundations: protected REST staged endpoints authenticate a
bearer token, derive the tenant/account principal from server-side credential config, scope results
to that principal, and then call the default-deny policy authorizer. The old tenant/account header
path remains only as `static-dev` compatibility behavior.

`GET /staged` is implemented as the first staged visibility read path. It lists staged snapshots
with tenant, worktree, and branch filters, and scopes results to the authenticated bearer tenant
when present. The gRPC `SyncService` also implements `StageSnapshot` and `ListStagedSnapshots` on
port `9877`, sharing object storage, staged storage, IAM, audit, and auth interception with REST.
Missing or invalid gRPC bearer metadata now returns unauthenticated before the handler runs.

Work:

- Implement have/want object negotiation.
- Accept staged snapshot object uploads.
- Verify all object hashes.
- Store staged snapshot metadata by tenant, worktree, tree, branch, author, timestamp, and session.
- Return synchronous ACK only after durable storage.
- Expose staged snapshot listing endpoints.
- Implement WebSocket staged event fanout.
- Add staged retention and cleanup worker.

Why:

Staged snapshots are a W0rkTree differentiator. They are visible to teammates before `wt push`, but they are not part of branch history. This must be a first-class server feature, not a background best-effort upload.

Deliverables:

- `StageSnapshot` gRPC flow.
- Object negotiation and upload.
- Staged snapshot index.
- WebSocket events.
- Retention cleanup.

## Phase 6: Branch Push And Pull

Implement explicit branch history movement.

Work:

- Implement branch creation and listing.
- Implement explicit push from staged snapshots to branch history.
- Enforce fast-forward checks using expected branch tip.
- Reject non-fast-forward pushes with structured conflict errors.
- Enforce append-only history.
- Implement pull and clone flows.
- Serve missing objects in dependency order.
- Support idempotency keys for retried pushes.
- Emit branch update WebSocket events.

Why:

Staging and pushing are distinct. Staged snapshots provide team visibility; push finalizes snapshots into canonical branch history. This distinction has to be enforced in server storage and APIs.

Deliverables:

- `PushBranch` gRPC flow.
- `PullBranch` and clone flows.
- Append-only DAG storage.
- Conflict detection tests.
- Branch update events.

## Phase 7: Branch Protection And Merge Requests

Add server-side governance for protected branches and collaboration workflows.

Work:

- Parse branch protection config.
- Enforce `no_direct_push`, `no_delete`, `no_force`, required reviews, required CI checks, and snapshot signature requirements.
- Implement merge request lifecycle:
  - create
  - review
  - approve
  - request changes
  - merge
  - close
- Implement stale review detection when source branch changes.
- Add CI status webhook endpoints.
- Update the merge request spec before implementing snapshot-based conflict resolution.

Why:

Protected branch rules are meaningless if they live only in the client. Merge requests are where server-side IAM, branch protection, CI, and audit intersect.

Deliverables:

- Branch protection enforcement.
- Merge request API.
- CI status storage.
- Tests for protected branch rejection and stale reviews.

## Phase 8: License Compliance, Quotas, And Rate Limits

Implement production policy controls beyond IAM.

Work:

- Parse per-path SPDX license assignments.
- Enforce license grants on sync, export, fork, archive, and cross-tenant movement.
- Implement tenant storage quota tracking.
- Reject new staged uploads and pushes when quota is exceeded.
- Implement tenant-aware request rate limits.
- Add upload size limits and streaming backpressure.

Why:

IAM decides whether a tenant may perform an action. License compliance decides whether the data itself may move. Quotas and rate limits protect the service from abuse and make hosted deployment viable.

Deliverables:

- License enforcement engine.
- Quota accounting.
- Rate limiting.
- Structured `LicenseDenied`, `QuotaExceeded`, and `RateLimitExceeded` errors.

## Phase 9: Production Hardening

Prepare the Go server for real deployment.

Work:

- Add TLS and mTLS support.
- Add key rotation.
- Add backup and restore documentation.
- Add database migration rollback strategy.
- Add object storage lifecycle policy.
- Add load tests for sync and staged uploads.
- Add partial upload and retry tests.
- Add chaos tests for storage/database interruption.
- Add Kubernetes manifests.
- Add Terraform examples for managed Postgres, object storage, ingress, and monitoring.
- Add runbooks for incident response.

Why:

A source-control server is infrastructure. It needs predictable operations, observability, recovery paths, and failure behavior before real users depend on it.

Deliverables:

- Production deployment manifests.
- Load and reliability tests.
- Backup/restore docs.
- Operator runbooks.

## Treatment Of Existing Rust `worktree-server`

The existing Rust crate should not be expanded as the production remote server.

Plan:

- Reclassify local watcher and auto-snapshot behavior as bgprocess responsibilities.
- Move or preserve useful bgprocess pieces where appropriate.
- Keep object and protocol semantics in Rust protocol crates.
- Stop adding remote-authority behavior to the mixed Rust server crate.
- Use the current implementation as reference material, not the deployment target.

Why:

Continuing to evolve the mixed crate risks preserving the exact boundary problem the rebuild is meant to fix.

## Testing Strategy

Testing must prove both Go correctness and Rust/Go compatibility.

Required test layers:

- Go unit tests for IAM, storage, branch DAG, staged sync, protection, quotas.
- Go integration tests with Postgres and local object storage.
- Cross-language fixtures:
  - Rust creates protocol objects.
  - Go verifies hashes and decodes/validates metadata.
  - Go stores objects.
  - Rust client pulls and verifies them.
- End-to-end tests:
  - CLI initializes tree.
  - bgprocess creates snapshot.
  - snapshot stages synchronously.
  - another client sees staged snapshot.
  - explicit push advances branch.
  - protected branch rejects direct push.
  - IAM denial produces structured audit event.

## Initial Milestone Definition

The first meaningful milestone is not "server starts." It is:

1. Go server runs locally with Postgres and object storage. **Done**
2. Rust client/bgprocess can upload a staged snapshot. **Done**
3. Server verifies BLAKE3 object integrity. **Done**
4. Server stores staged metadata durably. **Done for staged metadata in Postgres**
5. Server returns ACK only after durable write. **Done**
6. REST endpoint lists staged snapshots. **Done**
7. gRPC stages and lists staged snapshots. **Done**
8. IAM gates the operation through an authorizer seam. **Done**
9. Audit log records the decision. **Done**
10. WebSocket emits staged snapshot event. **Pending**

That milestone proves the new server boundary is correct.

## Immediate Next Steps

1. Replace `AllowAllAuthorizer` with JWT/API-key identity plus real policy evaluation.
2. Add branch push/pull storage so staged snapshots can be promoted into canonical history.
3. Add staged WebSocket fanout and retention cleanup.
4. Add tenant quota accounting and rate limiting against Postgres metadata.
5. Expand `.proto` contracts for branch push/pull, object negotiation, and auth.
6. Add integration tests that run Rust staged upload against the Docker Compose Go/Postgres stack.
