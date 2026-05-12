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

## Object Model

TODO: Define the core object types (blob, tree, snapshot, branch) and their relationships. Describe content-addressable storage using BLAKE3 hashes. Specify object identity, immutability guarantees, and garbage collection semantics.

## Tree Structure

TODO: Define the nested tree model. Describe how trees can contain sub-trees with independent histories. Specify tree mounting, path resolution across tree boundaries, and cross-tree reference semantics.

## Wire Format

TODO: Specify the binary and/or text serialization formats for protocol messages. Define encoding for objects, metadata, and transport frames. Cover versioning and backward compatibility of the wire format.

### Staged Snapshot Upload Compatibility Contract

The first Go server compatibility endpoint is `POST /staged`. It mirrors the current Rust
prototype boundary while the gRPC sync service is being designed.

Request fields:

| Field | Type | Required | Description |
|---|---|---:|---|
| `snapshot_id` | string | yes | Client-created snapshot identifier. |
| `tenant` | string | yes | Tenant slug that owns or stages the work. |
| `worktree` | string | yes | Worktree name within the tenant namespace. |
| `tree_id` | string | yes | Tree identifier associated with the snapshot. |
| `branch` | string | yes | Branch name where the staged work was produced. |
| `objects` | array | yes | Uploaded added/modified objects required by the staged snapshot. |

Each object entry contains:

| Field | Type | Required | Description |
|---|---|---:|---|
| `path` | string | yes | Relative worktree path for diagnostics and future policy checks. |
| `hash` | string | yes | 64-character BLAKE3 hex digest of `content`. |
| `size` | integer | yes | Byte length of decoded `content`. |
| `content` | base64 bytes | yes | Raw object bytes encoded by JSON as base64. |

Server behavior:

1. Reject malformed JSON or missing required fields.
2. Reject invalid BLAKE3 hashes, size mismatches, and hash/content mismatches.
3. Store object bytes in content-addressed storage using BLAKE3 fan-out paths.
4. Persist staged snapshot metadata only after all objects are verified and stored.
5. Return an ACK after persistence:

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

## Diff Semantics

TODO: Define how diffs are computed between snapshots. Specify the diff algorithm, handling of binary files, rename/move detection, and representation of changes across nested tree boundaries.

## Merge Semantics

TODO: Define the merge strategy and conflict resolution model. Specify three-way merge behavior, automatic resolution rules, conflict markers, and how merges propagate across nested trees.

## Snapshot Format

TODO: Define the snapshot (commit) format including metadata fields (author, timestamp, message, parent references). Specify how snapshots reference tree state, how snapshot chains form history, and how snapshots relate across branches.
