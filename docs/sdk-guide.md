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

All sync operations utilize a robust Write-Ahead Log (`sync_wal.log`) that persists push chunk offsets dynamically, seamlessly tolerating process or network interruptions and compacting itself locally once chunks succeed.

## Branch Operations

The SDK manages branch data effectively. Recently upgraded merges (`merge_branch`) use an advanced Time-based Priority Queue algorithm for detecting the absolute latest common ancestor, preventing "criss-cross" merge conflict false positives correctly natively.

## Permission Operations

TODO: Document how to set permissions on trees and subtrees, query current permissions, list permission rules, and manage access control programmatically.
