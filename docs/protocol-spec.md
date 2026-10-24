# Worktree Protocol Specification

The Worktree protocol defines the core data model, object formats, and semantics used by the Worktree version control system. This specification covers how objects are represented, how trees are structured, how data is serialized on the wire, and how diffs, merges, and snapshots operate.

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