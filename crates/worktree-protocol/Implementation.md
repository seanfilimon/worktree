# Implementation.md for worktree-protocol

The crate is the core type/protocol library for the Worktree version control system. It defines every domain primitive, object type, IAM model, access control system, configuration hierarchy, and wire format used across the entire workspace.

## Crate Metadata

- **Name:** `worktree-protocol`
- **Edition:** 2026
- **Dependencies:** `serde` 1 (derive), `blake3` 1, `bincode` 1, `chrono` 0.4 (serde), `thiserror` 1, `uuid` 1 (v4, serde)
- **No binary targets** — this is a pure library crate consumed by `worktree-sdk`, `worktree-git`, `worktree-server`, and `worktree-admin`.

---

## Module Architecture

The crate is organized into three foundation modules and a `feature` meta-module:

```
src/
├── lib.rs              # Root: declares modules, re-exports feature sub-modules
├── core/               # Foundational primitives
│   ├── mod.rs
│   ├── hash.rs         # BLAKE3 content-addressable hashing (ContentHash)
│   ├── id.rs           # UUID-based typed identifiers (macro-generated)
│   └── error.rs        # Protocol-level error enum
├── object/             # Version control domain objects
│   ├── mod.rs
│   ├── blob.rs         # Content-addressable blobs
│   ├── tree.rs         # Worktree (tracked directory)
│   ├── snapshot.rs     # Immutable snapshots (commits)
│   ├── branch.rs       # Named mutable pointers to snapshots
│   ├── manifest.rs     # File/directory manifests
│   ├── delta.rs        # Change deltas (add/modify/delete/rename/copy)
│   ├── tag.rs          # Immutable tags (lightweight/annotated/signed)
│   ├── release.rs      # Release management with artifacts
│   ├── reflog.rs       # Reference log (operation history)
│   ├── dependency.rs   # Cross-tree/branch dependencies + TODOs
│   ├── staged.rs       # Staged (pre-push) snapshots for team visibility
│   └── merge_request.rs # Built-in merge requests with reviews + CI
├── iam/                # Identity & Access Management
│   ├── mod.rs
│   ├── account.rs      # User accounts with ABAC attributes
│   ├── tenant.rs       # Multi-tenant isolation + billing plans
│   ├── team.rs         # Team grouping of accounts
│   ├── role.rs         # RBAC roles (5 built-in + custom)
│   ├── permission.rs   # 20 atomic permissions
│   ├── scope.rs        # Hierarchical scope (Global→Tenant→Tree→Branch)
│   ├── policy.rs       # ABAC policies with conditions
│   ├── session.rs      # Authentication sessions
│   └── engine.rs       # Central access decision engine (RBAC + ABAC)
├── access/             # Resource-level access control
│   ├── mod.rs
│   ├── resource.rs     # Resource targeting (Tree/Branch/Subtree)
│   ├── tree_access.rs  # Per-tree ACL (deny-overrides-allow)
│   └── branch_access.rs # Per-branch ACL + branch protection
├── config/             # Configuration management
│   ├── mod.rs
│   ├── worktree_config.rs # Root .wt/config.toml structure
│   ├── tree_config.rs     # Per-tree .wt-tree/config.toml overrides
│   └── hierarchy.rs       # Permission Ceiling Model resolution
└── feature/            # Protocol feature domains
    ├── mod.rs
    ├── diff/
    │   ├── mod.rs
    │   ├── compute.rs  # Manifest-to-manifest diff with rename/copy detection
    │   └── patch.rs    # Delta application to manifests
    ├── merge/
    │   ├── mod.rs
    │   ├── conflict.rs # 9 conflict kinds + resolution
    │   └── strategy.rs # 5 merge strategies + merge results
    ├── wire/
    │   ├── mod.rs
    │   ├── format.rs   # 13-byte wire header (WKTR magic)
    │   ├── encode.rs   # Serialization to wire format
    │   └── decode.rs   # Deserialization from wire format
    ├── compat/
    │   ├── mod.rs
    │   ├── git_hash_map.rs   # BLAKE3 ↔ SHA-1 bidirectional hash bridge
    │   ├── git_object_map.rs # Worktree ↔ Git object type mapping
    │   └── git_ref_map.rs    # Worktree branch/tag ↔ Git ref mapping
    ├── ignore.rs       # Hierarchical ignore pattern engine
    ├── licensing.rs    # SPDX license compliance engine
    ├── large_file.rs   # Large file chunking (FastCDC/fixed-size)
    ├── sync_protocol.rs # Push/pull/staged sync message types
    ├── archive.rs      # Archive/export options + results
    └── audit.rs        # Append-only audit log
```

---

## `core::hash` — Content-Addressable Hashing

The `ContentHash` struct wraps a `[u8; 32]` BLAKE3 digest and serves as the universal content-addressing primitive. Every blob, manifest, snapshot, and delta references content through this hash.

**Key type:**

- `ContentHash` — 32-byte BLAKE3 hash with `Copy`, `Eq`, `Hash`, `Ord`, `Serialize`/`Deserialize`.
- `ContentHash::ZERO` — The all-zeros hash constant.
- `to_hex()` / `FromStr` — Round-trip to/from 64-char lowercase hex.
- Custom `Serialize`/`Deserialize` — hex for human-readable formats, raw bytes for binary (bincode).

**Free functions:**

- `hash_bytes(data: &[u8]) -> ContentHash` — BLAKE3 hash arbitrary data.
- `hash_file(path: &Path) -> io::Result<ContentHash>` — BLAKE3 hash a file on disk.

This module is the foundation of the entire content-addressable storage model.

---

## `core::id` — Typed UUID Identifiers

A `define_id!` macro generates 9 strongly-typed UUID wrapper types to prevent accidental mixing:

| Type         | Purpose                      |
| ------------ | ---------------------------- |
| `TreeId`     | Worktree (tracked directory) |
| `SnapshotId` | Snapshot (immutable commit)  |
| `BranchId`   | Branch (mutable pointer)     |
| `TenantId`   | Multi-tenant organization    |
| `AccountId`  | User account                 |
| `TeamId`     | Team grouping                |
| `RoleId`     | RBAC role                    |
| `PolicyId`   | ABAC policy                  |
| `SessionId`  | Auth session                 |

Each ID type provides: `new()` (random UUIDv4), `from_uuid()`, `as_uuid()`, `nil()`, and full `Display`/`FromStr`/`Serialize`/`Deserialize` support.

---

## `core::error` — Protocol Errors

`ProtocolError` is a `thiserror`-derived enum with 8 variants covering serialization, hash mismatches, invalid IDs, access denial, policy violations, invalid scopes, and not-found errors.

---

## `object::blob` — Content Blobs

A `Blob` holds raw file content along with its pre-computed `ContentHash` and size. Methods:

- `from_bytes(data)` — Creates blob, auto-computes BLAKE3 hash.
- `from_file(path)` — Reads file from disk, creates blob.
- `verify()` — Recomputes hash and checks integrity.

---

## `object::tree` — Worktrees

A `Tree` represents a tracked directory with:

- `id: TreeId`, `name: String`, `parent: Option<TreeId>`, `root_path: PathBuf`
- `config: TreeConfig` (auto_snapshot, ignore_patterns)
- `with_parent()` for nested/linked tree hierarchies.

---

## `object::snapshot` — Snapshots (Commits)

A `Snapshot` is an immutable point-in-time capture forming a DAG:

- `id: SnapshotId`, `tree_id: TreeId`, `manifest_hash: ContentHash`
- `parents: Vec<SnapshotId>` — DAG linkage for history + merge tracking.
- `message`, `author: AccountId`, `timestamp`, `auto_generated: bool`
- `is_root()` — no parents. `is_merge()` — 2+ parents.

---

## `object::branch` — Branches

A `Branch` is a named mutable pointer:

- `id: BranchId`, `tree_id: TreeId`, `name`, `tip: SnapshotId`
- `advance(new_tip)` — Moves tip forward, returns old tip.

---

## `object::manifest` — File Manifests

A `Manifest` lists every tracked file/directory in a tree:

- `ManifestEntry` — `path`, `kind` (File/Directory/Symlink), `hash`, `size`, `executable`
- `compute_hash()` — Deterministic BLAKE3 of sorted entries (the hash stored in snapshots).
- `find_entry(path)` — Lookup by path.

---

## `object::delta` — Change Deltas

A `Delta` represents a single file change:

- `DeltaKind` — `Add`, `Modify`, `Delete`, `Rename { from }`, `Copy { from }`
- `old_hash`/`new_hash`, `old_size`/`new_size`
- Convenience constructors: `add()`, `modify()`, `delete()`, `rename()`, `copy()`

---

## `object::tag` — Tags

Three variants: `Lightweight`, `Annotated`, `Signed` — each pointing at a `SnapshotId` with optional message, tagger, and cryptographic signature.

---

## `object::release` — Releases

A `Release` bundles a tag with notes and `ReleaseArtifact`s (downloadable files). Lifecycle: `Draft → Published → Archived`.

---

## `object::reflog` — Reference Log

`ReflogEntry` records every tip-changing operation (10 action types: Snapshot, Merge, BranchCreate, BranchSwitch, BranchDelete, Revert, TagCreate, TagDelete, Sync, Restore). `Reflog` provides `recent(count)`, `prune(retention_days)`, and `prune_to_max(max_entries)`.

---

## `object::dependency` — Dependencies & TODOs

A comprehensive cross-tree dependency system:

- `TreeDependency` — declared in `.wt-tree/config.toml`.
- `BranchDependency` — runtime branch-to-branch links with `Active/Completed/Blocked/Stale` lifecycle.
- `LinkedBranchGroup` — coordinated branch groups across trees.
- `SnapshotDependency` — snapshot-level requirements with priority.
- `TodoItem` — auto-generated from blocking dependencies, with `Open → Claimed → InProgress → Completed/Cancelled` lifecycle.
- `DependencyRegistry` — central registry with query methods (`blocking_deps_for_branch`, `open_todos`, `todos_for_tree`).

---

## `object::staged` — Staged Snapshots

`StagedSnapshot` enables team visibility of in-progress work before push:

- `StagedStatus` — `Staged → Pushed/Cleared/Expired`
- `StagedIndex` — server-side collection with `check_conflicts(user, files)` for overlap detection and `gc(retention_days)` for cleanup.

---

## `object::merge_request` — Merge Requests

First-class protocol object with:

- `MergeRequestStatus` — `Open → InReview → Approved/ChangesRequested → Merged/Closed`
- `Review` with stale detection (new snapshots invalidate old reviews).
- `CiCheck` with `Pending/Running/Passed/Failed/Skipped`.
- `can_merge(required_reviewers)` — checks approval count, no outstanding changes-requested, all CI passed.
- `link_merge_request()` — cross-MR linking.

---

## `iam::account` — User Accounts

`Account` with `Active/Suspended/Deactivated` lifecycle, belonging to a `TenantId`, with arbitrary `HashMap<String, String>` attributes for ABAC evaluation.

---

## `iam::tenant` — Multi-Tenant Isolation

`Tenant` with `Active/Suspended` status, billing plans (`Free/Pro/Enterprise/Custom`), resource limits (`max_accounts`, `max_trees`), and `can_add_account()`/`can_add_tree()` limit enforcement.

---

## `iam::team` — Teams

`Team` groups `Vec<AccountId>` members and `Vec<RoleId>` roles within a tenant. Deduplication on add.

---

## `iam::role` — RBAC Roles

`Role` holds `HashSet<Permission>`. Five built-in roles form a superset hierarchy:

- **Owner** — all 20 permissions
- **Admin** — tree/branch/management (no GlobalAdmin/TenantAdmin)
- **Maintainer** — write + branch lifecycle + snapshot + sync
- **Developer** — read/write + branch create + snapshot + sync
- **Viewer** — read-only (3 permissions)

Custom roles are tenant-defined with arbitrary permission sets.

---

## `iam::permission` — 20 Atomic Permissions

Organized into 6 categories:

- **Tree:** `TreeRead`, `TreeWrite`, `TreeCreate`, `TreeDelete`, `TreeAdmin`
- **Branch:** `BranchRead`, `BranchCreate`, `BranchDelete`, `BranchMerge`, `BranchProtect`
- **Snapshot:** `SnapshotCreate`, `SnapshotRead`
- **Sync:** `SyncPush`, `SyncPull`
- **Management:** `AccountManage`, `TeamManage`, `RoleManage`, `PolicyManage`
- **Administrative:** `TenantAdmin`, `GlobalAdmin`

Each has a stable `as_str()` representation (e.g., `"tree:read"`).

---

## `iam::scope` — Hierarchical Scope

`Scope` enum: `Global → Tenant(TenantId) → Tree(TenantId, TreeId) → Branch(TenantId, TreeId, BranchId)`.

The critical `covers(&self, other)` method implements scope inheritance: broader scopes cover narrower ones (e.g., `Tenant` covers all `Tree` and `Branch` scopes within it).

---

## `iam::policy` — ABAC Policies

`Policy` combines:

- `effect: PolicyEffect` (`Allow`/`Deny`)
- `subjects: Vec<PolicySubject>` (`Account`, `Team`, `Role`, `AllAuthenticated`, `Everyone`)
- `scope: Scope`
- `permissions: HashSet<Permission>`
- `conditions: Vec<AttributeCondition>` — evaluated against account/request attributes
- `priority: i32` — for ordering

`AttributeCondition::evaluate()` supports 8 operators: `Equals`, `NotEquals`, `Contains`, `StartsWith`, `EndsWith`, `GreaterThan`, `LessThan`, `In`.

---

## `iam::engine` — Access Decision Engine

The heart of authorization. The `AccessEngine::evaluate()` function takes an `AccessRequest` plus all context (account, teams, roles, policies) and returns `AccessDecision::Allow` or `AccessDecision::Deny { reason }`.

**Algorithm:**

1. Check account is `Active` (inactive → deny).
2. Collect all roles from all teams the account belongs to.
3. **RBAC check:** If any role has the requested permission AND its tenant scope covers the request scope → tentatively allow.
4. **GlobalAdmin shortcut:** If any role has `GlobalAdmin` → allow everything.
5. **ABAC evaluation:** Filter enabled policies by scope coverage, subject match, permission match, and condition evaluation. Sort by priority.
6. **Deny wins:** If ANY matching Deny policy exists → deny.
7. **Allow:** If RBAC allowed OR any matching Allow policy exists → allow.
8. **Default deny:** No matching authorization.

---

## `access::resource` — Resource Targeting

`Resource` enum: `Tree`, `Branch`, `Subtree`. Each provides `tenant_id()`, `tree_id()`, and `to_scope()` for conversion to IAM scopes.

---

## `access::tree_access` — Per-Tree ACL

`TreeAccessList` holds `Vec<TreeAccessRule>`. Each rule binds a `TreeAccessSubject` (Account/Team/Role/AllAuthenticated/Public) to `PolicyEffect` + `HashSet<Permission>`. The `check()` method implements **deny-overrides-allow**: any matching deny takes precedence, then any matching allow, then implicit deny.

---

## `access::branch_access` — Per-Branch ACL + Protection

`BranchAccessList` mirrors tree access at branch granularity. `BranchProtection` adds:

- `require_snapshot_review`, `require_passing_checks`
- `restrict_push`/`restrict_merge` to specific subjects
- `allow_force_push`, `allow_deletion`

---

## `config::worktree_config` — Root Configuration

`WorktreeConfig` represents `.wt/config.toml` with sections:

- `worktree` — name, server, tenant, visibility (`Private/Shared/Public`)
- `sync` — auto, interval, retry, conflict strategy (`Auto/Manual/Ours/Theirs`)
- `auto_snapshot` — enabled, timeout, max files/bytes, on_branch_switch
- `large_files` — threshold, chunk size, lazy loading, preload patterns
- `reflog` — retention, max entries, server sync, compression
- `shallow` — enabled, depth, auto-deepen, lazy blobs
- `license` — default SPDX, strict mode, per-path assignments, grants
- Plus: `registered_paths`, `tenant_access` grants, `branch_protection` rules.
- `validate()` enforces constraint invariants.

---

## `config::tree_config` — Per-Tree Overrides

`TreeLevelConfig` has all fields as `Option<_>` — omitted fields inherit from root. Adds `branch_strategy` (`FeatureBranch/TrunkBased/ReleaseTrain`) and `dependencies`.

---

## `config::hierarchy` — Permission Ceiling Model

`ResolvedConfig::resolve(root, tree_config)` merges root + tree using the ceiling model:

- **Numeric values:** tree can only go lower (`.min()`)
- **Booleans:** tree can only make more restrictive (set to `true`)
- **Branch protection:** tree can raise required reviewers, add CI checks, but never relax
- **Registered paths:** additive (tree adds more)
- **Visibility:** cannot be overridden by tree

---

## `feature::diff` — Diff Computation

`compute_diff(old_manifest, new_manifest, options)` produces `Vec<Delta>`:

1. Build HashMap of old/new entries by path.
2. Detect modifications (same path, different hash).
3. Collect deletions and additions.
4. **Rename detection:** match deleted file hashes to added file hashes (same content, different path).
5. **Copy detection:** match existing file hashes to new additions.
6. Sort results by path.

`Patch::apply(manifest, verify_hashes)` applies deltas in order: Renames → Copies → Deletes → Adds → Modifies → sort.

---

## `feature::merge` — Merge Strategies

5 strategies: `ThreeWay`, `Ours`, `Theirs`, `FastForward`, `Union`.

9 conflict kinds: `ContentConflict`, `ModifyDelete`, `DeleteModify`, `AddAdd`, `RenameRename`, `RenameModify`, `RenameDelete`, `DirectoryFileConflict`, `ModeConflict`.

`MergeResult` tracks outcome (`Clean/AutoResolved/Conflicted/FastForwarded/NotPossible`), conflicts, deltas, and auto-resolved paths.

---

## `feature::wire` — Binary Wire Protocol

**Header format (13 bytes):**

- Magic: `0x57 0x4B 0x54 0x52` ("WKTR")
- Version: `u32` (currently 1)
- Payload length: `u32`
- Flags: `u8` (bits: compressed, checksummed, encrypted)

**Encoding:** `encode<T: Serialize>()` serializes via bincode, prepends WKTR header.
**Decoding:** `decode<T: DeserializeOwned>()` validates magic + version, extracts payload, deserializes.
**Length-prefixed:** Additional `u32` total-length prefix for stream framing.
**Message splitting:** `split_messages()` for batch processing.

---

## `feature::compat` — Git Compatibility

Three bidirectional mapping systems:

1. **`git_hash_map`** — `GitHash` (20-byte SHA-1) ↔ `ContentHash` (32-byte BLAKE3) via `HashIndex` trait + `InMemoryHashIndex`.
2. **`git_object_map`** — `ObjectMapping` maps worktree content hashes to Git SHA hex strings with object kind (Blob/Tree/Commit/Tag).
3. **`git_ref_map`** — `RefMapping` maps Worktree `BranchId`/`SnapshotId` to Git refs (`refs/heads/*`, `refs/tags/*`).

---

## `feature::ignore` — Ignore Engine

Hierarchical pattern matching: `BuiltIn` → `RootIgnore` → `TreeIgnore`.

`IgnorePattern::parse()` handles negation (`!`), directory-only (`/`), anchoring, and glob matching. `IgnoreEngine::is_ignored()` evaluates all pattern layers with the ceiling model (tree patterns cannot negate root patterns).

Built-in ignores: `.wt/`, `.git/`. Default ignores: `node_modules/`, `target/`, `__pycache__/`, `.DS_Store`, etc.

---

## `feature::licensing` — License Compliance

`LicenseEngine` enforces per-path SPDX licenses with grant levels (`ReadOnly < Modify < Redistribute`). Categories: `Permissive`, `Copyleft`, `Attribution`, `Proprietary`, `PublicDomain`.

`check(path, tenant, operation)` returns `Allowed` or `Denied { reason }`. Permissive/public-domain always allowed. Proprietary/copyleft requires explicit grant at the appropriate level.

---

## `feature::large_file` — Large File Chunking

Content-defined chunking via `FastCDC` or `FixedSize` algorithms. `chunk_data(data, config)` produces a `ChunkManifest` with `Vec<Chunk>` (each: hash, offset, size). `LargeFileStub` provides lazy-loading references.

---

## `feature::sync_protocol` — Sync Messages

12 message types for BGProcess ↔ Server communication: `StageUpload/Ack`, `PushRequest/Response`, `PullRequest/Response`, `HaveWant`, `ObjectTransfer`, `AccessConfigSync`, `TagSync`, `ChunkUpload/Download`, `Ping/Pong`.

`SyncState` tracks local/remote tips, pending counts, and sync/offline flags. `SyncEnvelope` wraps messages for wire transport.

---

## `feature::archive` — Archive/Export

`ArchiveOptions` configures tar.gz/zip export with include/exclude paths, license/ignore respect. `ArchiveResult` reports file counts, sizes, and exclusion stats.

---

## `feature::audit` — Audit Logging

14 event types (`AccessDecision`, `PolicyChange`, `TenantAccess`, `LicenseChange`, etc.). `AuditLog` is append-only with filtering by type, actor, tenant, outcome, and time range.

---

## TODO

- [ ] Implement `feature::diff::compute_diff` rename detection for partial content matches (currently exact-hash only)
- [ ] Implement `feature::merge::strategy` — actual three-way merge algorithm (currently types only, no merge execution)
- [ ] Add `feature::wire` compression support (ZSTD) when `COMPRESSED` flag is set
- [ ] Add `feature::wire` checksum support (CRC32/BLAKE3 trailer) when `CHECKSUMMED` flag is set
- [ ] Add `feature::wire` encryption support when `ENCRYPTED` flag is set
- [ ] Implement `feature::large_file::chunk_data` with actual FastCDC algorithm (currently placeholder)
- [ ] Add persistent `HashIndex` implementation backed by SQLite or RocksDB for large repos
- [ ] Implement `feature::compat` full Git-to-Worktree and Worktree-to-Git object conversion pipelines
- [ ] Add `feature::sync_protocol` message validation and schema versioning
- [ ] Implement `feature::ignore::glob_match` with full gitignore glob semantics (character classes, `**` double-star)
- [ ] Add property-based tests for `ContentHash` serialization round-trips
- [ ] Add fuzzing targets for `wire::decode` to ensure robustness against malformed input
- [ ] Implement `iam::engine` integration tests with complex multi-tenant, multi-team scenarios
- [ ] Add `config::hierarchy` tests for edge cases in the Permission Ceiling Model
- [ ] Implement `object::merge_request` auto-close when source branch is deleted
- [ ] Add `object::staged` conflict detection for file-level (not just path-level) overlaps
- [ ] Document the complete wire protocol specification in a machine-readable schema (protobuf or similar)
