# Worktree Architecture Enhancements - PR 2

## 1. Compiled Policy Evaluation (Go Server)
- [x] Add TOML parser dependency (`github.com/pelletier/go-toml/v2`).
- [x] Create database migration for `tenant_permissions` table.
- [x] Update `iam.ParsePolicies` to parse `.wt/access/*.toml` files.
- [x] Update Canonical `Service.processPolicies` to look for `.toml` files.
- [x] Pre-compile TOML policies into relational DB schemas (`tenant_permissions`).
- [x] Refactor `Authorizer` to evaluate rules against DB via fast single SQL query (`SELECT 1 FROM tenant_permissions ...`).
- [x] Add tests for the new database-backed policy evaluation.

## 2. Advanced LCA Merge Algorithm (Rust SDK)
- [x] Upgrade the BFS to a Priority Queue (`BinaryHeap`) in `merge_branch`.
- [x] Factor in `SnapshotState.timestamp` to find the most recent common ancestor (LCA) in time.

## 3. Git Bridge Object Streaming
- [x] Refactor the `Blob` protocol to utilize data streams instead of `Vec<u8>`.
- [x] Switch git2 integration to use `git2::Repository::blob_writer()` in `convert_blob` (streaming approach).
- [x] Replace mocked identities with authenticated default Git Signatures (`repo.signature()`).

## 4. Robust Sync WAL (Rust SDK)
- [x] Introduce a Write-Ahead Log (`sync_wal.log`) in `.wt/cache/` to journal push operations and chunking.
- [x] Update `push_staged` in `crates/worktree-sdk/src/engine/sync.rs` to track and recover from chunk offsets.
- [x] Ensure file flushing to disk via `file.sync_all()` upon append.
- [x] Implement WAL compaction cycle (`clean_wal()`) to truncate completed active states and prevent O(N) latency growth.
