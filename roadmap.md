# W0rkTree Go Server Roadmap

## Purpose

The remote W0rkTree server may be implemented in Go. The language choice is not the architectural constraint. The real constraints are that the remote server is the canonical source of truth, enforces IAM and policy decisions, stores authoritative history, and never behaves like the local bgprocess.

This roadmap defines how to rebuild the remote server as a production-ready Go service while preserving the Rust protocol/client ecosystem. It tracks the progress of implementing the core project spec (two-runtime architecture, staged visibility, append-only history, FastCDC chunking, licensing, and cross-tree dependencies).

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
    | gRPC sync (migrating to QUIC), REST, WebSocket
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
    canonical/        authoritative branch histories and tip advancement
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
```

## Phase 1: Spec And Contract Cleanup

**Status: Done**

Work:
- Treated `docs/protocol-spec.md` as the implementation-facing protocol contract.
- Normalized permission names and established permission vocabulary.
- Defined initial `.proto` files for sync/auth/branch/staged APIs.

## Phase 2: Go Server Skeleton

**Status: Done**

Service shell successfully created before implementing product behavior. It is implemented in `server-go/` with standard-library HTTP routing, JSON logging, request IDs, health/readiness endpoints, a Prometheus `/metrics` endpoint, graceful shutdown, TLS 1.3 configuration, and a two-stage Dockerfile.

## Phase 3: Canonical Storage

**Status: In Progress**

Implement authoritative server-side storage independent of local SDK state.
Staged metadata now has both development file storage and a Postgres implementation. The `canonical` service orchestrates CAS tip advancement and blob validation. 

Work:
- Design PostgreSQL schema for tenants, accounts, worktrees, trees, branches, branch heads, snapshots. (Partially completed via `canonical/service.go`)
- Store objects by content hash with fanout paths (Implemented).
- Add idempotent writes for object uploads and sync operations (Implemented).

*Architectural Decision Needed:* How should pack files and delta-compressed objects be stored server-side to maximize S3 efficiency while minimizing extraction times during client clones?

## Phase 4: Auth And IAM

**Status: Partially Done**

Build first-party IAM in Go instead of delegating core semantics to a generic policy engine.
The enforcement seam is implemented. `Authorizer` is wired into `POST /staged`, `GET /staged`, and gRPC sync services. An initial default-deny policy authorizer is live. Audit recording is functional.

Work:
- Parse `.wt/access/*.toml` and `.wt-tree/access/*.toml` declaratively.
- Compile policies into server-side cached forms.
- Enforce root/tree ceiling model permissions.
- Implement full ABAC condition evaluation.

*Architectural Decision Needed:* Where should `.wt/access/*.toml` files be compiled and evaluated? Should the server parse these directly from the BLAKE3 hashed blobs on the fly, or should push events trigger a compilation to a relational database representation for fast O(1) lookups?

## Phase 5: Staged Snapshot Sync

**Status: Done (Core API and WebSocket Fanout)**

Implement automatic bgprocess upload of staged snapshots.
The Rust daemon (`bgprocess`) has an asynchronous background push queue. `server-go` exposes `POST /staged` and gRPC `StageSnapshot`, verifying JSON/protobuf object bytes against BLAKE3 hashes and returning ACKs after durability.

Work:
- Add staged retention and cleanup worker.

## Phase 6: Branch Push And Pull

**Status: In Progress**

Implement explicit branch history movement. `canonical.Service` logic is written to ensure missing objects are checked before tip advancement.

Work:
- Integrate the `PushInput`/`PullInput` logic to HTTP/gRPC handlers.
- Emit branch update WebSocket events.
- Implement clone flows and serve objects in dependency order.

## Phase 7: License Compliance Engine

**Status: Pending**

Implement SPDX file-level policy controls and ensure proprietary paths cannot leak.

Work:
- Parse per-path SPDX license assignments from configuration.
- Enforce license grants on sync, export, fork, archive, and cross-tenant movement. Both IAM and license checks must pass.

*Architectural Decision Needed:* Since licenses apply at the path level and objects are content-addressed, how do we efficiently enforce path-level licensing across large trees during `git export` or `pull` without scanning the entire tree on every request?

## Phase 8: Native Large File Handling (FastCDC & VFS)

**Status: Pending**

Handle large files natively without separate LFS endpoints.

Work:
- Replace the simple fixed-size chunker placeholder in `large_file.rs` with the true FastCDC algorithm.
- Implement lazy loading via virtual filesystems (FUSE on Linux, FUSE-T on macOS, ProjFS on Windows) in the local bgprocess.
- Add server-side chunk deduplication and partial upload streaming backpressure.

*Architectural Decision Needed:* Should the VFS FUSE layer be implemented natively in the Rust `bgprocess` daemon, or delegated to a platform-specific sidecar? Determine the optimal sliding-window parameters for FastCDC to balance chunking speed vs deduplication ratios for typical monorepo assets.

## Phase 9: Cross-Tree Dependencies & Merge Atomicity

**Status: Pending**

Implement the cross-tree dependency logic described in the spec to allow multi-repo development with the safety of a monorepo.

Work:
- Implement server-side cross-branch enforcement for linked dependencies (e.g., if `frontend/feature-x` merges to `main`, `backend/feature-x` must also merge atomically).
- Parse `wt depend add` metadata.

*Architectural Decision Needed:* How to handle distributed transactions when merging linked branches across different trees in PostgreSQL to avoid race conditions or partial deployments.

## Phase 10: Transport Layer Evolution (gRPC / QUIC)

**Status: Pending**

Upgrade the transport layer to support connection migration and 0-RTT.

Work:
- Migrate existing standard gRPC/HTTP implementation to run over QUIC.
- Implement delta sync, have/want negotiation, and zstd compression in transit.

*Architectural Decision Needed:* Should QUIC be implemented using `quic-go` or wait for deeper standard library support? Need a fallback strategy to HTTP/2 for corporate firewalls that block UDP traffic.

## Phase 11: Git Compatibility Bridge

**Status: In Progress**

Speak Git natively when needed for risk-free gradual adoption. `worktree-git` crate scaffolding exists.

Work:
- Implement full bidirectional conversion of Git commits to W0rkTree snapshots.
- Build Live Mirror mode (`wt git mirror`).

*Architectural Decision Needed:* Mapping Git's mutable history (rebases, force pushes) to W0rkTree's append-only history model. How to represent a force-pushed Git branch as an append-only snapshot chain without losing or duplicating data confusingly.

## Branch Protection And Merge Requests

**Status: Pending**

Work:
- Enforce `no_direct_push`, `no_delete`, `no_force`, required reviews.
- Implement merge request lifecycle (create, review, approve, merge).
- Add CI status webhook endpoints.

## Testing Strategy

Testing must prove both Go correctness and Rust/Go compatibility.

Required test layers:
- Go unit tests for IAM, storage, branch DAG, staged sync, protection, quotas.
- Go integration tests with Postgres and local object storage.
- Cross-language fixtures (Rust creates objects -> Go verifies and stores -> Rust pulls).
- End-to-end tests validating staging visibility, push rejections, and IAM denial audit events.

## Initial Milestone Definition

1. Go server runs locally with Postgres and object storage. **Done**
2. Rust client/bgprocess can upload a staged snapshot. **Done**
3. Server verifies BLAKE3 object integrity. **Done**
4. Server stores staged metadata durably. **Done**
5. Server returns ACK only after durable write. **Done**
6. REST and gRPC endpoints list staged snapshots. **Done**
7. IAM gates the operation through an authorizer seam. **Done**
8. Audit log records the decision. **Done**
9. Branch tip CAS logic written (`canonical` service). **Done**
10. WebSocket emits staged snapshot event. **Done**

## Immediate Next Steps

1. Expose the `canonical.Service` logic (Push/Pull) through HTTP/gRPC handlers so explicitly pushed snapshots land in history.
2. Replace `AllowAllAuthorizer` with real `.wt/access/*.toml` policy evaluation.
3. Add the FastCDC implementation to the Rust SDK.
5. Expand `.proto` contracts for object negotiation and chunked streaming.