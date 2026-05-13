# Worktree Protocol Specification

The Worktree protocol defines the core data model, object formats, and semantics used by the Worktree version control system. This specification covers how objects are represented, how trees are structured, how data is serialized on the wire, and how diffs, merges, and snapshots operate.

This document is the language-neutral contract that the Rust client/bgprocess and the planned Go remote server must both obey. The production Go server implementation sequence is tracked in [`../roadmap.md`](../roadmap.md); that roadmap depends on this protocol spec for object identity, wire compatibility, staged snapshot semantics, branch history rules, and cross-language fixture requirements.

Implementation rule: protocol behavior is specified here first, then implemented in Rust and Go. The Go server may choose Go-native libraries and storage systems, but it must not invent object formats, permission names, sync messages, or history semantics outside this spec and the authoritative protocol specs under [`../crates/worktree-protocol/specs/`](../crates/worktree-protocol/specs/).

## Relationship To The Go Server Roadmap

The Go server roadmap is connected to this protocol specification in the following way:

- `docs/protocol-spec.md` defines the portable data and wire contracts.
- `roadmap.md` defines how the Go server implements those contracts for production deployment.
- `.proto` files, when added, should be derived from this spec and referenced from both documents.
- Cross-language compatibility tests should use this spec as the source of expected behavior: Rust creates protocol objects, Go validates/stores them, and Rust pulls/verifies them.
- Any Go server feature that changes object structure, staged snapshot flow, push/pull semantics, IAM action names, or wire framing requires a protocol-spec update before code.

Current implementation status:

- Rust staged uploads use this contract and are configurable with `WT_SERVER_URL` and `WT_TENANT`.
- Go REST implements `POST /staged` and `GET /staged`.
- Go gRPC implements `SyncService.StageSnapshot` and `SyncService.ListStagedSnapshots`.
- Go staged metadata is stored in Postgres when `WT_SERVER_DATABASE_URL` is set, or in a file store for development.
- REST and gRPC staged create/list authenticate bearer principals, pass those principals to the shared `Authorizer`, and audit normalized action names.

## Object Model

TODO: Define the core object types (blob, tree, snapshot, branch) and their relationships. Describe content-addressable storage using BLAKE3 hashes. Specify object identity, immutability guarantees, and garbage collection semantics.

## Tree Structure

TODO: Define the nested tree model. Describe how trees can contain sub-trees with independent histories. Specify tree mounting, path resolution across tree boundaries, and cross-tree reference semantics.

## Wire Format

TODO: Specify the binary and/or text serialization formats for protocol messages. Define encoding for objects, metadata, and transport frames. Cover versioning and backward compatibility of the wire format.

### Staged Snapshot Upload Compatibility Contract

The first Go server compatibility endpoint is `POST /staged`. It mirrors the current Rust
prototype boundary and now has an equivalent gRPC `StageSnapshot` method.

Request fields:

| Field         | Type   | Required | Description                                                      |
| ------------- | ------ | -------: | ---------------------------------------------------------------- |
| `snapshot_id` | string |      yes | Client-created snapshot identifier.                              |
| `tenant`      | string |      yes | Tenant slug that owns or stages the work.                        |
| `worktree`    | string |      yes | Worktree name within the tenant namespace.                       |
| `tree_id`     | string |      yes | Tree identifier associated with the snapshot.                    |
| `branch`      | string |      yes | Branch name where the staged work was produced.                  |
| `objects`     | array  |      yes | Uploaded added/modified objects required by the staged snapshot. |

Each object entry contains:

| Field     | Type         | Required | Description                                                      |
| --------- | ------------ | -------: | ---------------------------------------------------------------- |
| `path`    | string       |      yes | Relative worktree path for diagnostics and future policy checks. |
| `hash`    | string       |      yes | 64-character BLAKE3 hex digest of `content`.                     |
| `size`    | integer      |      yes | Byte length of decoded `content`.                                |
| `content` | base64 bytes |      yes | Raw object bytes encoded by JSON as base64.                      |

Server behavior:

1. Reject malformed JSON or missing required fields.
2. Reject unsafe paths, invalid BLAKE3 hashes, negative sizes, size mismatches, and hash/content mismatches.
3. Authenticate the bearer principal and authorize `staged:create` for the staged resource.
4. Store object bytes in content-addressed storage using BLAKE3 fan-out paths.
5. Persist staged snapshot metadata only after all objects are verified and stored.
6. Return an ACK after persistence:

```json
{
  "status": "staged",
  "snapshot_id": "snap-1",
  "objects": 1
}
```

This REST shape is a compatibility bridge. The production sync API should promote the same
semantics into a `StageSnapshot` gRPC method without changing object identity, verification, or ACK
rules.

The Rust SDK currently sends this shape from `push_staged`. `content` is intentionally the JSON
field name even though the value is base64 text; Go decodes it into bytes for verification.

### Staged Snapshot Listing Compatibility Contract

The Go server also exposes `GET /staged` as the first read-side compatibility endpoint for staged
visibility.

Query parameters:

| Parameter  | Required | Description                                                             |
| ---------- | -------: | ----------------------------------------------------------------------- |
| `tenant`   |       no | Tenant slug. If authenticated tenant context is present, it must match. |
| `worktree` |       no | Worktree name filter.                                                   |
| `branch`   |       no | Branch name filter.                                                     |

Response:

```json
{
  "snapshots": [],
  "count": 0
}
```

When an authenticated tenant is present, the server filters the list to that tenant and rejects
mismatched explicit `tenant` query parameters. The server authorizes `staged:list` against a
tenant-scoped staged resource before returning results.

### gRPC SyncService Contract

The Go server also exposes the staged flow through gRPC on `WT_SERVER_GRPC_ADDR`, default
`127.0.0.1:9877`.

```proto
service SyncService {
  rpc StageSnapshot(StageSnapshotRequest) returns (StageSnapshotResponse);
  rpc ListStagedSnapshots(ListStagedSnapshotsRequest) returns (ListStagedSnapshotsResponse);
}
```

`StageSnapshotRequest` carries the same logical fields as `POST /staged`. `StagedObject.content`
is raw `bytes` in protobuf rather than JSON base64 text. `ListStagedSnapshotsRequest` filters by
`tenant`, `worktree`, and `branch`. The service shares the same object store, staged store, audit
recorder, bearer-auth principal path, and IAM authorizer as REST.

## Error Codes

All Go server error responses use this envelope:

```json
{
  "error": {
    "code": "ErrorCodeCamelCase",
    "message": "human-readable description"
  }
}
```

| Code                     | HTTP Status | Meaning                                                           |
| ------------------------ | ----------- | ----------------------------------------------------------------- |
| `InvalidJSON`            | 400         | Request body is not valid JSON                                    |
| `InvalidStagedSnapshot`  | 422         | Required field missing or invalid                                 |
| `InvalidObject`          | 422         | Object size mismatch or BLAKE3 hash/content mismatch              |
| `StagedUploadTooLarge`   | 413         | Object count or byte size exceeds server limits                   |
| `TenantMismatch`         | 403         | Authenticated tenant does not match payload tenant                |
| `AuthenticationRequired` | 401         | Missing or invalid bearer token                                   |
| `AuthenticationFailed`   | 401         | Authentication backend rejected the request                       |
| `Forbidden`              | 403         | IAM authorizer denied the action                                  |
| `StagedConflict`         | 409         | Staged retry conflicts with an existing canonical staged identity |
| `StagedStoreFailed`      | 500         | Persistence layer error                                           |
| `StagedListFailed`       | 500         | Read-side storage error                                           |
| `MethodNotAllowed`       | 405         | Wrong HTTP method for route                                       |

For gRPC, missing/invalid bearer metadata maps to `Unauthenticated`, validation failures map to
`InvalidArgument`, IAM denials map to `PermissionDenied`, staged idempotency conflicts map to
`AlreadyExists`, and storage failures map to `Internal`.

## Permission Action Names

IAM permission strings follow the pattern `<resource>:<verb>`. Server implementations
must use these exact strings when recording audit events and calling the authorizer.

| Action          | Description                                   |
| --------------- | --------------------------------------------- |
| `staged:create` | Upload a staged snapshot via `POST /staged`   |
| `staged:list`   | List staged snapshots via `GET /staged`       |
| `branch:push`   | Finalize staged snapshots into branch history |
| `branch:pull`   | Download branch history to local store        |
| `tree:init`     | Initialize a new tree on the server           |
| `tenant:read`   | Read tenant metadata                          |

## Idempotency

`POST /staged` and `StageSnapshot` are idempotent on the canonical staged identity:
`tenant`, `worktree`, `tree_id`, `branch`, `snapshot_id`, and sorted object refs (`path`, `hash`,
`size`). If a request arrives with an already-persisted identity and the same payload hash:

1. The server MUST NOT create duplicate staged metadata.
2. The server SHOULD return success with an idempotent replay marker where the transport supports it.
3. Object bytes already stored by BLAKE3 key are skipped.

If the same identity arrives with different object refs or metadata, the server MUST reject it as a
conflict. The file store and Postgres store both enforce this behavior; Postgres uses a composite
identity constraint plus a canonical payload hash.

## Diff Semantics

TODO: Define how diffs are computed between snapshots. Specify the diff algorithm, handling of binary files, rename/move detection, and representation of changes across nested tree boundaries.

## Merge Semantics

TODO: Define the merge strategy and conflict resolution model. Specify three-way merge behavior, automatic resolution rules, conflict markers, and how merges propagate across nested trees.

## Snapshot Format

TODO: Define the snapshot (commit) format including metadata fields (author, timestamp, message, parent references). Specify how snapshots reference tree state, how snapshot chains form history, and how snapshots relate across branches.
