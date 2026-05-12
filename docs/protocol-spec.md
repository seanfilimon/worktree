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

## Diff Semantics

TODO: Define how diffs are computed between snapshots. Specify the diff algorithm, handling of binary files, rename/move detection, and representation of changes across nested tree boundaries.

## Merge Semantics

TODO: Define the merge strategy and conflict resolution model. Specify three-way merge behavior, automatic resolution rules, conflict markers, and how merges propagate across nested trees.

## Snapshot Format

TODO: Define the snapshot (commit) format including metadata fields (author, timestamp, message, parent references). Specify how snapshots reference tree state, how snapshot chains form history, and how snapshots relate across branches.
