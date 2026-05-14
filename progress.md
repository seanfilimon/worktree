# Worktree Architecture Enhancements - PR 2

## Advanced LCA Merge Algorithm
- [x] Upgrade the BFS to a Priority Queue (`BinaryHeap`) in `merge_branch`.
- [x] Factor in `SnapshotState.timestamp` to find the most recent common ancestor (LCA) in time.

## Git Bridge Object Streaming
- [ ] Refactor the `Blob` protocol to utilize data streams instead of `Vec<u8>`.
- [ ] Switch git2 integration to use `git2::Repository::blob_stream()` in `convert_blob`.
- [ ] Fix mocked identities.

## Robust Sync WAL
- [ ] Introduce a Write-Ahead Log in `.wt/cache/` for chunking and push states.
- [ ] Update `push_staged` in `crates/worktree-sdk/src/engine/sync.rs` to use WAL.
- [x] Add TOML parser dependency (`github.com/pelletier/go-toml/v2`).
- [x] Create database migration for `tenant_policies` table.
- [x] Update `iam.ParsePolicies` to parse `.wt/access/*.toml` files instead of JSON.
- [x] Update Canonical `Service.processPolicies` to look for `.toml` files.
- [x] Implement database-backed policy storage (inserting/updating `tenant_policies`).
- [x] Refactor `Authorizer` interface implementation to evaluate rules against the `tenant_policies` table via fast SQL queries.
- [x] Add tests for the new database-backed policy evaluation.
