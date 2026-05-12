# Worktree SDK Guide

The Worktree SDK provides programmatic access to Worktree's version control capabilities. Use the SDK to integrate Worktree into your own tools, editors, CI/CD pipelines, and automation workflows.

The SDK is available as the `worktree-sdk` Rust crate and exposes a high-level API for all core operations including tree management, snapshots, branches, and permissions.

## Installation

TODO: Document how to add the SDK as a dependency, minimum supported Rust version, and feature flags.

## Connecting

The current SDK prototype targets the local Rust HTTP server at `http://127.0.0.1:9876`. This is a
temporary compatibility path while the production Go server contract is defined.

## Tree Operations

TODO: Document how to initialize trees, open existing trees, list files, read/write content, and manage nested trees programmatically.

## Snapshot Operations

The SDK can create local snapshots through `worktree_sdk::engine::snapshot::create_snapshot`. Recent
sync work adds staged upload support:

- `worktree_sdk::engine::sync::push(engine)` stages the latest snapshot on the current branch.
- `worktree_sdk::engine::sync::push_latest_staged(engine)` is the explicit latest-snapshot helper.
- `worktree_sdk::engine::sync::push_staged(engine, snapshot_id)` uploads a specific snapshot.

The staged upload computes added, modified, and deleted paths relative to the parent snapshot. It
uploads added/modified file bytes to `POST /staged` after locally verifying each file's BLAKE3 hash
still matches the snapshot metadata.

## Branch Operations

TODO: Document how to create branches, list branches, switch branches, delete branches, and merge branches through the SDK.

## Permission Operations

TODO: Document how to set permissions on trees and subtrees, query current permissions, list permission rules, and manage access control programmatically.
