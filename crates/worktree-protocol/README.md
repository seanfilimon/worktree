# `worktree-protocol`

> **The protocol definition crate for W0rkTree — a complete Git replacement built from first principles.**

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-W0rkTree%20Public%20License%20v1.0-blue.svg)](#license)

---

## Overview

`worktree-protocol` is the foundational crate that defines every data type, wire format, access control primitive, and protocol message used by the W0rkTree version control system. It is the **single source of truth** for the binary protocol spoken between the two W0rkTree runtimes — the local background process (`worktree-bgprocess`) and the remote server (`worktree-server`).

W0rkTree is **not** a Git wrapper, extension, or hosting layer. It is an independent version control system with its own protocol, storage model, identity system, and history model. It speaks Git only for migration and interoperability — nothing more.

### What This Crate Provides

| Domain              | What's Defined                                                                                                                                                                             |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Core primitives** | BLAKE3 content-addressable hashing, typed UUID identifiers, protocol error types                                                                                                           |
| **Object model**    | Blobs, tree objects, snapshots, branches, manifests, deltas, tags, releases, reflogs, dependencies, staged snapshots, merge requests                                                       |
| **IAM**             | Accounts, tenants, teams, roles (RBAC), permissions (20 atomic), scopes, policies (ABAC), access decision engine                                                                           |
| **Access control**  | Resource targeting, per-tree ACLs, per-branch ACLs, branch protection rules                                                                                                                |
| **Configuration**   | Root worktree config, per-tree config overrides, permission ceiling hierarchy                                                                                                              |
| **Features**        | Diff computation, merge strategies, binary wire protocol, Git compatibility, ignore engine, license compliance, large file chunking, sync protocol messages, archive/export, audit logging |

---

## Architecture Context

W0rkTree operates as a **two-runtime system**. This crate defines the contract between them.

```text
┌─────────────────────────────────────────────────┐
│              Developer Machine                   │
│                                                  │
│   ┌──────────────────────────────────────────┐   │
│   │         worktree-bgprocess               │   │
│   │                                          │   │
│   │  • Filesystem watcher (OS-native)        │   │
│   │  • Auto-snapshot engine                  │   │
│   │  • Local history & DAG                   │   │
│   │  • Branch management                     │   │
│   │  • .wt/ and .wt-tree/ management         │   │
│   │  • Staged snapshot sync                  │   │
│   │  • Large file chunking (FastCDC)         │   │
│   │  • Lazy loading (FUSE/ProjFS)            │   │
│   │  • Auto-merge engine                     │   │
│   └─────────────────┬────────────────────────┘   │
│                     │                            │
└─────────────────────┼────────────────────────────┘
                      │  ← worktree-protocol →
                      │  (QUIC/TLS 1.3 + gRPC fallback)
┌─────────────────────┼────────────────────────────┐
│                     │                            │
│   ┌─────────────────┴────────────────────────┐   │
│   │           worktree-server                │   │
│   │                                          │   │
│   │  • Canonical history (source of truth)   │   │
│   │  • Multi-tenant isolation                │   │
│   │  • IAM enforcement (RBAC + ABAC)         │   │
│   │  • Staged snapshot aggregation           │   │
│   │  • Branch protection enforcement         │   │
│   │  • License compliance enforcement        │   │
│   │  • Merge request system                  │   │
│   │  • Tag & release management              │   │
│   │  • Audit logging                         │   │
│   └──────────────────────────────────────────┘   │
│                  Remote Server                    │
└──────────────────────────────────────────────────┘
```

**Separation of concerns is a hard constraint:**

- The bgprocess **never** enforces access control — it reads policies for display only.
- The server **never** touches the working directory or creates snapshots.
- The bgprocess **never** stores canonical history — its local history is a sync cache.
- The server **never** bypasses its own enforcement, even for admin operations.

---

## Module Architecture

```text
worktree-protocol/src/
├── lib.rs                      # Crate root — re-exports all public API
├── core/
│   ├── hash.rs                 # BLAKE3 content-addressable hashing
│   ├── id.rs                   # Typed UUID identifiers (SnapshotId, TreeId, BranchId, ...)
│   └── error.rs                # Protocol-level error types
├── object/
│   ├── blob.rs                 # Content blobs (raw file data)
│   ├── tree.rs                 # Worktree/tree objects (directory manifests)
│   ├── snapshot.rs             # Snapshots (immutable state records — replaces "commits")
│   ├── branch.rs               # Branch pointers (named refs to snapshot chains)
│   ├── manifest.rs             # Large file manifests (chunk maps)
│   ├── delta.rs                # Change deltas (compressed diffs between versions)
│   ├── tag.rs                  # Tags (lightweight and annotated)
│   ├── release.rs              # Releases (tags + artifacts + notes)
│   ├── reflog.rs               # Reference log entries
│   ├── dependency.rs           # Dependencies & auto-generated TODOs
│   ├── staged.rs               # Staged snapshots (team visibility layer)
│   └── merge_request.rs        # Merge requests (review + CI gates)
├── iam/
│   ├── account.rs              # User accounts (verified identity)
│   ├── tenant.rs               # Multi-tenant isolation (Personal / Organization)
│   ├── team.rs                 # Team groupings within tenants
│   ├── role.rs                 # RBAC roles (5 built-in + custom)
│   ├── permission.rs           # 20 atomic permissions across 8 categories
│   ├── scope.rs                # Hierarchical scope (Global → Tenant → Tree → Branch → Path)
│   ├── policy.rs               # ABAC policies (effect + subjects + scope + conditions)
│   └── engine.rs               # Access decision engine (evaluate policies → allow/deny)
├── access/
│   ├── resource.rs             # Resource targeting for policies
│   ├── tree_access.rs          # Per-tree access control lists
│   └── branch_access.rs        # Per-branch ACL + branch protection rules
├── config/
│   ├── worktree_config.rs      # Root .wt/config.toml definitions
│   ├── tree_config.rs          # Per-tree .wt-tree/config.toml overrides
│   └── hierarchy.rs            # Permission ceiling model (parent caps child)
└── feature/
    ├── diff.rs                 # Diff computation (snapshot, branch, tree, working-dir)
    ├── merge.rs                # Merge strategies (recursive three-way, ours, theirs)
    ├── wire.rs                 # Binary wire protocol (magic bytes, versioned envelope)
    ├── compat.rs               # Git compatibility layer (import/export/mirror)
    ├── ignore.rs               # Ignore engine (hierarchical .wt/ignore + .wt-tree/ignore)
    ├── licensing.rs            # License compliance (SPDX, grants, enforcement)
    ├── large_file.rs           # Large file chunking (FastCDC, manifests, lazy loading)
    ├── sync_protocol.rs        # Sync messages (staged upload, push, pull, delta)
    ├── archive.rs              # Archive/export (tar.gz, zip, with license compliance)
    └── audit.rs                # Audit logging (immutable, append-only event trail)
```

---

## Core Concepts

### Content-Addressable Hashing (`core::hash`)

All objects in W0rkTree are addressed by their **BLAKE3** hash — faster than SHA-256/SHA-1, with tree-hashing mode for large inputs. Identical content always produces the same hash, enabling automatic deduplication across branches, trees, and tenants.

```rust
use worktree_protocol::core::hash::ContentHash;

let hash = ContentHash::from_bytes(b"file content");
// BLAKE3 → 32-byte digest → hex-encoded for display
```

### Typed Identifiers (`core::id`)

Every entity has a strongly-typed UUID v4 identifier. This prevents accidental ID mixing at compile time:

```rust
use worktree_protocol::core::id::{SnapshotId, TreeId, BranchId, TenantId};

let snap_id = SnapshotId::new();   // UUID v4
let tree_id = TreeId::new();       // Cannot be used where SnapshotId is expected
```

### Snapshots — Not Commits (`object::snapshot`)

Snapshots are **immutable, content-addressed records** of the complete state of a tree at a point in time. They replace Git's "commits" with clearer semantics:

- **Immutable** — once created, never modified.
- **Content-addressed** — identity derived from content.
- **Append-only** — only ever added to history, never removed or reordered.
- **No staging area** — no `add` command, no index. The bgprocess watches the filesystem and snapshots the working state.
- **Auto-created** — the bgprocess creates snapshots automatically as you work.

A snapshot contains:

| Field        | Description                                                |
| ------------ | ---------------------------------------------------------- |
| `id`         | BLAKE3 hash of the snapshot content                        |
| `parent_ids` | One parent (normal) or two+ parents (merge snapshot)       |
| `tree_hash`  | Root tree object hash (complete directory state)           |
| `author`     | Verified tenant email                                      |
| `timestamp`  | UTC timestamp                                              |
| `message`    | Human-readable description (auto-generated or manual)      |
| `metadata`   | Revert info, auto-snapshot flag, dependency refs, tag refs |

### Trees — Not Repositories (`object::tree`)

Trees are the **fundamental unit of code organization**. Each tree has:

- Its own independent snapshot history
- Its own independent branches
- Its own access rules (`.wt-tree/access/`)
- Its own ignore patterns (`.wt-tree/ignore`)
- Its own license configuration
- The ability to contain **nested subtrees** (no submodules)

A microservices project might have one tree per service. A multi-platform app might have trees for `frontend/`, `backend/`, `mobile/`, and `shared/`. Each tree versions independently while living inside a single W0rkTree.

### Staged Snapshots — The Key Innovation (`object::staged`)

Staged snapshots are W0rkTree's answer to Git's collaboration visibility gap:

```text
Git workflow:     edit → (invisible) → push → team sees work
W0rkTree workflow: edit → auto-snapshot → staged (team sees WIP) → push (permanent)
```

| Property                 | Staged Snapshot     | Pushed Snapshot |
| ------------------------ | ------------------- | --------------- |
| Visible to team          | ✓                   | ✓               |
| Part of branch history   | ✗                   | ✓               |
| Permanent                | ✗ (ephemeral, GC'd) | ✓ (append-only) |
| Requires explicit action | ✗ (automatic)       | ✓ (`wt push`)   |

Staged snapshots answer "what is everyone working on right now?" without polluting branch history with WIP commits.

---

## IAM System

W0rkTree includes a full **Identity and Access Management** system — no external platform required.

### Built-in Roles (Superset Hierarchy)

```text
owner ⊃ admin ⊃ maintainer ⊃ contributor ⊃ reader
```

Each higher role includes all permissions of the roles below it.

### 20 Atomic Permissions

| Category            | Permissions                                                   |
| ------------------- | ------------------------------------------------------------- |
| **Tree**            | `tree:read`, `tree:write`, `tree:create`, `tree:delete`       |
| **Branch**          | `branch:create`, `branch:delete`, `branch:protect`            |
| **Snapshot**        | `snapshot:create`, `snapshot:revert`, `snapshot:sign`         |
| **Sync**            | `sync:push`, `sync:pull`, `sync:force_push`                   |
| **Management**      | `manage:roles`, `manage:teams`, `manage:policies`             |
| **Admin**           | `admin:tenant`, `admin:audit_read`, `admin:bypass_protection` |
| **Tags & Releases** | `tag:create`, `release:create`                                |
| **Merge Requests**  | `mr:create`, `mr:review`, `mr:merge`                          |

### Scope Hierarchy

Policies are scoped to a hierarchy, and **deny always beats allow** at the same level:

```text
Global → Tenant → Tree → Branch → RegisteredPath
```

### Permission Ceiling Model

Parent levels set **maximum** permissions. Children **cannot** expand beyond the ceiling:

- `.wt/access/` defines the root ceiling
- `.wt-tree/access/` can only **restrict**, never expand
- A tree-level policy granting `tree:write` has no effect if the root denies it

### ABAC Conditions

Policies support attribute-based conditions for fine-grained control:

```toml
[[policy]]
name = "business-hours-only"
effect = "deny"
subjects = { role = "contractor" }
scope = "global"
permissions = ["sync:push"]

[[policy.conditions]]
attribute = "time.hour"
operator = "not_between"
value = [9, 17]
```

Supported attributes: `time.hour`, `time.day_of_week`, `source.ip`, `user.department`, `user.clearance_level`, plus custom tenant-defined attributes.

---

## Multi-Tenant Architecture

### Tenant Model

A **tenant** is a verified user or organization:

| Property | Description                                                   |
| -------- | ------------------------------------------------------------- |
| `id`     | UUID v4                                                       |
| `name`   | Display name                                                  |
| `slug`   | URL-safe unique identifier (3-39 chars, `[a-z0-9][-a-z0-9]*`) |
| `email`  | Verified email address                                        |
| `type`   | `Personal` or `Organization`                                  |
| `status` | `Active` or `Suspended`                                       |
| `plan`   | `Free`, `Pro`, `Enterprise`, `Custom`                         |
| `limits` | `max_accounts`, `max_trees`, `max_storage_bytes`, ...         |

### Worktree Visibility Modes

| Mode                  | Behavior                                                |
| --------------------- | ------------------------------------------------------- |
| **Private** (default) | Only the owning tenant can access                       |
| **Shared**            | Owning tenant + explicitly granted tenants              |
| **Public**            | All authenticated users can read; write requires grants |

### Cross-Tenant Access

Two methods, from simple to full-featured:

**Simple grants** in `.wt/config.toml`:

```toml
[[tenant_access]]
tenant = "partner-corp"
permissions = ["tree:read", "branch:create"]
```

**Full IAM policies** in `.wt/access/policies.toml`:

```toml
[[policy]]
name = "partner-read-access"
effect = "allow"
subjects = { tenant = "partner-corp" }
scope = "global"
permissions = ["tree:read", "sync:pull"]
```

---

## Dependency System

W0rkTree provides a **three-level dependency model** that replaces the ad-hoc coordination tools teams bolt onto Git:

### Level 1: Tree Dependencies

Declare that one tree depends on another (e.g., `frontend` depends on `shared-models`):

```toml
[[dependency]]
tree = "shared-models"
branch = "main"
version = ">=1.0.0"
required = true
```

### Level 2: Branch Dependencies

Declare that a feature branch in one tree depends on a feature branch in another:

```toml
[[dependency]]
tree = "backend"
branch = "feature-new-api"
status = "in-progress"
blocking = true
linked = true
```

### Level 3: Snapshot Dependencies

Individual snapshots can declare requirements on other trees, triggering **automatic TODO branch generation** in the target tree:

```text
Alice (frontend/feature-analytics):
  "I need a new endpoint GET /api/analytics/events from backend"
    ↓ auto-generates
  backend/todo/frontend-feature-analytics-a3f8c2d7
    ↓ with structured metadata
  { title, description, priority, blocking, linked, assigned_to }
```

### Linked Branches

Branches across trees can be **linked** — if `frontend/feature-oauth` is linked to `backend/feature-oauth`, they must be merged together. This prevents partial feature deployments.

---

## License Compliance

W0rkTree enforces license compliance at the **file level** as a first-class protocol concept — not a convention, not an honor system.

```text
Enforcement stack:
  1. IAM check:     Does this tenant have permission?    → YES/NO
  2. License check:  Does this file's license allow it?   → YES/NO
  3. Final:          BOTH must pass.
```

### Per-Path SPDX Licensing

```toml
[license]
default = "MIT"
spdx_strict = true

[[license.path]]
path = "services/billing-engine"
license = "proprietary"

[[license.path]]
path = "vendor/third-party-sdk"
license = "Apache-2.0"
```

### License Grants

```toml
[[license.grant]]
path = "services/billing-engine"
tenant = "partner-corp"
grant = "read-only"           # Can view, cannot copy/export

[[license.grant]]
path = "services/billing-engine"
tenant = "contractor@dev.io"
grant = "modify"              # Can modify, cannot export
```

| Grant Level    | Read | Modify | Export/Copy/Fork |
| -------------- | ---- | ------ | ---------------- |
| `read-only`    | ✓    | ✗      | ✗                |
| `modify`       | ✓    | ✓      | ✗                |
| `redistribute` | ✓    | ✓      | ✓                |

The server blocks unauthorized export, fork, sync, and archive operations at the protocol level. A tenant can have full IAM permissions but still be blocked from exporting proprietary code without a license grant.

---

## Storage Model

### Content-Addressable Object Store

| Object Type  | Description                                                   |
| ------------ | ------------------------------------------------------------- |
| **Blob**     | Raw file content, BLAKE3-addressed                            |
| **Tree**     | Directory listing: `[(name, type, hash), ...]`                |
| **Snapshot** | Complete tree state at a point in time (replaces Git commits) |
| **Manifest** | Large file chunk map: `[(offset, size, chunk_hash), ...]`     |
| **Delta**    | Compressed diff between two object versions                   |
| **Tag**      | Named reference to a snapshot (lightweight or annotated)      |
| **Branch**   | Mutable pointer to the tip snapshot of a branch               |

### Large File Handling — No LFS

Files above a configurable threshold (default: 10 MB) are automatically chunked using **FastCDC** (Fast Content-Defined Chunking):

- Content-defined boundaries → inserting data only affects nearby chunks
- Independent content-addressing → automatic cross-file/cross-version deduplication
- Lazy loading via FUSE (Linux/macOS) or ProjFS (Windows)
- LRU chunk cache with configurable size (default: 2 GB)
- **Zero configuration required** — it just works

### Wire Format

```text
┌─────────┬─────────┬───────┬──────────┬────────────────┬────────────┬──────────┐
│ Magic   │ Version │ Flags │ Msg Type │ Payload Length │ Payload    │ Checksum │
│ "WT01"  │ u8      │ u8    │ u16      │ u32            │ bincode    │ BLAKE3   │
│ 4 bytes │ 1 byte  │ 1 byte│ 2 bytes  │ 4 bytes        │ variable   │ 32 bytes │
└─────────┴─────────┴───────┴──────────┴────────────────┴────────────┴──────────┘
```

- **Serialization**: Bincode (compact, fast) for sync protocol; JSON for REST/admin API
- **Compression**: zstd level 3 (payloads > 1 KB)
- **Transport**: QUIC (TLS 1.3 built-in) primary, gRPC over HTTP/2 fallback
- **Authentication**: JWT tokens in metadata headers

---

## Sync Protocol

The sync protocol defines three distinct operations — they are **not** interchangeable:

| Operation       | Trigger                           | What Happens                                                 |
| --------------- | --------------------------------- | ------------------------------------------------------------ |
| **Staged sync** | Automatic (configurable interval) | Snapshots uploaded to server as "staged" for team visibility |
| **Branch push** | Explicit (`wt push`)              | Staged snapshots finalized into branch history               |
| **Branch pull** | Automatic (server notification)   | Remote branch updates downloaded to local                    |

### Delta Sync

Only new objects are transferred. Have/want negotiation eliminates redundant data:

```text
BGProcess → Server:  "I have objects [A, B, C]. I want objects [D, E, F]."
Server → BGProcess:  "You need [D, F]. You already have [E] (dedup)."
```

Objects are sent in dependency order (blobs → trees → snapshots) with streaming backpressure.

### Offline Mode

When the server is unreachable:

- All local operations continue (snapshots, branches, merges, tags)
- Staged snapshots accumulate locally
- On reconnect: exponential backoff (1s → 2s → 4s → ... → 30s cap)
- Delta sync catches up — no full re-sync needed
- Push operations queued until reconnection

---

## Configuration Hierarchy

W0rkTree uses a **four-level configuration hierarchy** with strict precedence:

```text
System defaults (lowest priority)
  └── User global config
        └── .wt/config.toml (root worktree)
              └── .wt-tree/config.toml (per-tree override — highest priority)
```

### `.wt/` — Root Worktree Configuration

The `.wt/` directory at the worktree root contains:

| Path                   | Purpose                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `config.toml`          | Root configuration (sync, auto-snapshot, storage, licensing, tenant access, branch protection) |
| `ignore`               | Root-level ignore patterns (authoritative)                                                     |
| `identity/`            | Authentication tokens and user identity (signing keys)                                         |
| `access/roles.toml`    | Custom role definitions                                                                        |
| `access/policies.toml` | Root-level RBAC + ABAC policies                                                                |
| `hooks/`               | Pre/post-snapshot hooks                                                                        |
| `reflog/`              | Operation history per branch                                                                   |
| `conflicts/`           | Machine-readable merge conflict metadata                                                       |
| `cache/`               | Local computation cache (deletable, not synced)                                                |

### `.wt-tree/` — Per-Tree Configuration

Each tree's `.wt-tree/` directory can override root settings with the **restriction-only invariant**: tree-level config can restrict but never expand what the root allows.

| Path                   | Purpose                                                                                         |
| ---------------------- | ----------------------------------------------------------------------------------------------- |
| `config.toml`          | Tree-specific overrides (snapshot intervals, large file thresholds, license, branch protection) |
| `ignore`               | Tree-level ignore patterns (additive to root)                                                   |
| `access/policies.toml` | Tree-scoped access policies (can only restrict)                                                 |
| `hooks/`               | Tree-level hooks (run after root hooks)                                                         |

---

## Security

Security is built into every layer of the protocol — not bolted on as an afterthought.

| Layer                   | Mechanism                                                                                                        |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------- |
| **Transport**           | TLS 1.3 (QUIC native) / mTLS (enterprise). No unencrypted transport.                                             |
| **Authentication**      | OAuth2 device flow, API keys, JWT tokens with configurable expiry and auto-refresh                               |
| **Access control**      | 20 atomic permissions, 5 built-in roles, RBAC + ABAC engine, scope hierarchy, deny-beats-allow                   |
| **Integrity**           | BLAKE3 hash verification on every object read. Corrupt objects auto-re-fetched.                                  |
| **Signing**             | Ed25519 snapshot signatures. Branch protection can require signed snapshots.                                     |
| **Secret scanning**     | Pre-snapshot regex scanning (AWS keys, Stripe keys, private keys, custom patterns). Configurable block-on-match. |
| **Audit logging**       | Immutable, append-only log of every access decision, policy change, license grant, and sync operation            |
| **License enforcement** | Server blocks unauthorized export/fork/sync of proprietary code at the protocol level                            |
| **Data at rest**        | Optional per-tenant encryption with envelope encryption and key rotation (enterprise)                            |
| **IPC**                 | Unix sockets (owner-only perms) / Windows named pipes (ACLs). No network exposure.                               |

### Threat Model

| Threat               | Mitigation                                                         |
| -------------------- | ------------------------------------------------------------------ |
| Eavesdropping        | TLS 1.3 / QUIC encryption on all traffic                           |
| Unauthorized access  | Mandatory authentication, full IAM                                 |
| Code theft           | License compliance enforcement at protocol level                   |
| Secret leakage       | Pre-snapshot scanning with configurable blocking                   |
| History tampering    | BLAKE3 integrity, append-only history, no rebase, snapshot signing |
| Privilege escalation | Scope hierarchy, deny-beats-allow, permission ceiling model        |
| Supply chain attack  | Snapshot signatures, server-side signature verification            |

---

## Branch Protection

Branch protection rules are **server-enforced** — the bgprocess cannot bypass them:

```toml
[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review", "require_ci_pass", "no_delete"]

[branch_protection.review]
min_approvals = 2
require_from_roles = ["maintainer", "admin"]
dismiss_stale_on_update = true

[[branch_protection]]
branch = "release/*"
rules = ["no_direct_push", "require_merge_review", "require_snapshot_signature"]
```

Built-in merge request system with review tracking, stale review dismissal, and CI gate integration — no external platform required.

---

## Git Compatibility

W0rkTree speaks Git when necessary — for migration, interop, and backward compatibility:

| Operation                  | Description                                                    |
| -------------------------- | -------------------------------------------------------------- |
| `wt init --from-git <url>` | Import from a Git repository (full history conversion)         |
| `wt git export`            | Export a tree as a Git repository (with license filtering)     |
| `wt git mirror`            | Live mirror to/from a Git remote                               |
| Git remote bridge          | A Git client can clone a W0rkTree tree (reduced functionality) |

### Round-Trip Guarantees

- Git commits → W0rkTree snapshots (lossless)
- W0rkTree snapshots → Git commits (metadata loss — no IAM, no license, no staged visibility)
- Proprietary-licensed paths are **blocked** from Git export to public remotes

---

## Key Design Principles

| #   | Principle                      | Enforcement                                                                     |
| --- | ------------------------------ | ------------------------------------------------------------------------------- |
| 1   | **One job per command**        | Every CLI command does exactly one thing. No `checkout` that does three things. |
| 2   | **Plain terminology**          | Snapshot, not commit. Tree, not repository. Sync, not push/pull/fetch.          |
| 3   | **Automatic by default**       | Auto-snapshot, auto-sync. Manual mode exists but is opt-in.                     |
| 4   | **Append-only history**        | No rebase. No `reset --hard`. No force-push. Ever.                              |
| 5   | **Non-destructive operations** | Soft deletes with configurable recovery windows. Reflog server-synced.          |
| 6   | **Real-time collaboration**    | Staged snapshot visibility is a core protocol feature, not an add-on.           |
| 7   | **Multi-protocol support**     | Native W0rkTree protocol + Git compatibility bridge.                            |

---

## Dependencies

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
blake3 = "1"
bincode = "1"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"
uuid = { version = "1", features = ["v4", "serde"] }

[dev-dependencies]
serde_json = "1"
```

| Dependency  | Purpose                                              |
| ----------- | ---------------------------------------------------- |
| `serde`     | Serialization/deserialization for all protocol types |
| `blake3`    | Content-addressable hashing (faster than SHA-256)    |
| `bincode`   | Compact binary wire format for sync protocol         |
| `chrono`    | UTC timestamps on snapshots, reflogs, audit entries  |
| `thiserror` | Ergonomic protocol error types                       |
| `uuid`      | Typed v4 identifiers for all entities                |

---

## Specification Documents

This crate is the Rust implementation of the protocol defined in [`specs/`](./specs/). The specifications are the authoritative source:

| Specification             | Path                                                                             | Covers                                                                                                    |
| ------------------------- | -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| **Protocol Overview**     | [`specs/README.md`](./specs/README.md)                                           | Architecture, terminology, Git comparison, innovation summary                                             |
| **W0rkTree Core**         | [`specs/WorkTree.md`](./specs/WorkTree.md)                                       | Full system spec: trees, snapshots, tenants, dependencies, merge, tags, licensing, diff, Git compat       |
| **Tree Spec**             | [`specs/tree/Tree.md`](./specs/tree/Tree.md)                                     | Trees, branches, snapshots, nested trees, dependencies, linked branches, cross-tree coordination          |
| **BGProcess**             | [`specs/bgprocess/BgProcess.md`](./specs/bgprocess/BgProcess.md)                 | Local daemon: auto-snapshot, filesystem watching, staged sync, large file chunking, IPC, platform support |
| **Server**                | [`specs/server/Server.md`](./specs/server/Server.md)                             | Remote server: tenant isolation, IAM enforcement, branch protection, merge requests, API surface          |
| **IAM**                   | [`specs/iam/IAM.md`](./specs/iam/IAM.md)                                         | Roles, permissions, scopes, RBAC + ABAC policies, access decision engine                                  |
| **Declarative Access**    | [`specs/iam/DeclarativeAccess.md`](./specs/iam/DeclarativeAccess.md)             | Path registration, custom roles, policy authoring, tree-level overrides, full examples                    |
| **Tenant Model**          | [`specs/iam/TenantModel.md`](./specs/iam/TenantModel.md)                         | Tenant types, lifecycle, cross-tenant access, visibility modes, organization structure, ABAC attributes   |
| **Staged Visibility**     | [`specs/visibility/StagedVisibility.md`](./specs/visibility/StagedVisibility.md) | Staged snapshot pipeline, visibility surfaces, privacy controls, license interaction, retention           |
| **Sync Protocol**         | [`specs/sync/Sync.md`](./specs/sync/Sync.md)                                     | Staged upload, push/pull, delta sync, offline mode, have/want negotiation, transport, wire format         |
| **Storage**               | [`specs/storage/Storage.md`](./specs/storage/Storage.md)                         | Object types, BLAKE3 addressing, FastCDC chunking, pack files, GC, quotas, shallow/partial sync           |
| **`.wt/` Directory**      | [`specs/dot-wt/DotWt.md`](./specs/dot-wt/DotWt.md)                               | Root config, ignore, identity, access, hooks, reflog, conflicts, cache                                    |
| **`.wt-tree/` Directory** | [`specs/dot-wt-tree/DotWtTree.md`](./specs/dot-wt-tree/DotWtTree.md)             | Per-tree config, authority model, nesting rules, tree-level policies and hooks                            |
| **License Compliance**    | [`specs/licensing/LicenseCompliance.md`](./specs/licensing/LicenseCompliance.md) | SPDX assignment, grant model, server enforcement, Git export handling, audit trail                        |
| **Security**              | [`specs/security/Security.md`](./specs/security/Security.md)                     | Transport, auth, signing, secret scanning, encryption, audit logging, threat model                        |

### Recommended Reading Order

```text
1. specs/README.md                       ← Start here: architecture and terminology
2. specs/WorkTree.md                     ← Full system design
3. specs/tree/Tree.md                    ← Deep dive into trees and coordination
4. specs/dot-wt/DotWt.md                ← Root configuration
5. specs/dot-wt-tree/DotWtTree.md       ← Per-tree configuration
6. specs/bgprocess/BgProcess.md         ← Local runtime
7. specs/server/Server.md               ← Remote runtime
8. specs/iam/IAM.md                     ← Access control system
9. specs/iam/DeclarativeAccess.md       ← Policy authoring guide
10. specs/iam/TenantModel.md            ← Multi-tenancy
11. specs/visibility/StagedVisibility.md ← Real-time collaboration
12. specs/sync/Sync.md                  ← Sync protocol
13. specs/storage/Storage.md            ← Storage architecture
14. specs/licensing/LicenseCompliance.md ← License enforcement
15. specs/security/Security.md          ← Security model
```

---

## Git vs W0rkTree — Full Comparison

| Aspect                  | Git                                      | W0rkTree                                              |
| ----------------------- | ---------------------------------------- | ----------------------------------------------------- |
| **Architecture**        | Monolithic local tool + separate hosting | Two-runtime: local bgprocess + remote server          |
| **Organization**        | Single flat repo per project             | Multi-tenant trees with nested subtrees               |
| **Identity**            | Name + email (no verification)           | Verified tenant: username + email + type              |
| **Terminology**         | commit, repository, checkout, stash      | snapshot, tree, switch — plain language               |
| **Staging**             | Explicit `git add` required              | No staging area — snapshot captures working state     |
| **Commands**            | 150+ commands, many overloaded           | One job per command, no overloading                   |
| **Branches**            | Global namespace                         | Tree-scoped with independent strategies               |
| **Access control**      | None built-in                            | Declarative TOML policies, RBAC + ABAC, ceiling model |
| **Merge**               | Merge, rebase, cherry-pick, squash       | Merge only. No rebase. Append-only history.           |
| **History**             | Rewritable (rebase, reset, force-push)   | Append-only. Non-destructive. Soft deletes.           |
| **Large files**         | Requires Git LFS (separate system)       | Native chunked storage with lazy loading              |
| **Collaboration**       | Invisible until push                     | Staged snapshots — team sees WIP in real-time         |
| **License tracking**    | None                                     | Per-path SPDX, server-enforced compliance             |
| **Dependencies**        | None                                     | Three-level system with auto-TODO generation          |
| **Project management**  | External tools only                      | Built-in per-tree structured task management          |
| **Submodules**          | Separate, notoriously painful system     | Nested trees — native, consistent, reliable           |
| **Recovery**            | `git reflog` (local, expires)            | Full reflog, server-synced, configurable retention    |
| **Multi-tenancy**       | Not supported                            | First-class tenants, cross-tenant sharing             |
| **Conflict resolution** | Basic markers                            | Three-way markers + machine-readable `.wt/conflicts/` |
| **Monitoring**          | None                                     | Server-side telemetry, sync health, audit logs        |

---

## Use Cases

### Microservices Architecture

One tree per service. Shared libraries as nested trees. Cross-service dependencies tracked with the three-level dependency system. Teams own their trees with independent branches, snapshots, and release cycles.

### Multi-Platform Applications

`frontend/`, `backend/`, `mobile/`, `shared/` as separate trees. Linked branches coordinate cross-platform features. The dependency system prevents partial deployments.

### Enterprise Codebases

Declarative access control at the path level, version-controlled alongside code. License compliance prevents unauthorized use of proprietary modules. Server enforces every rule without relying on developer discipline.

### Open Source with Proprietary Modules

Public trees for open-source code. Private nested trees for proprietary modules. Per-path SPDX licensing ensures boundaries are enforced at the protocol level.

### Monorepo Migration from Git

`wt init --from-git <url>` imports a Git repository as a W0rkTree tree. Split into nested trees at your own pace. Git compatibility bridge keeps CI/CD pipelines working during migration.

---

## Command Reference

| Command                    | Description                                   |
| -------------------------- | --------------------------------------------- |
| `wt init`                  | Initialize a new worktree                     |
| `wt init --from <url>`     | Initialize from a remote worktree             |
| `wt init --from-git <url>` | Import from a Git repository                  |
| `wt snapshot`              | Create a manual snapshot                      |
| `wt snapshot -m <msg>`     | Create a snapshot with a message              |
| `wt push`                  | Finalize staged snapshots into branch history |
| `wt sync`                  | Bidirectional sync with server                |
| `wt branch create <name>`  | Create a new branch                           |
| `wt branch switch <name>`  | Switch to a branch                            |
| `wt branch list`           | List branches in current tree                 |
| `wt branch delete <name>`  | Soft-delete a branch (recoverable)            |
| `wt merge <branch>`        | Merge a branch into current branch            |
| `wt diff`                  | Show changes                                  |
| `wt log`                   | Show snapshot history                         |
| `wt status --team`         | Show staged activity from all team members    |
| `wt staged`                | List all staged (unpushed) snapshots          |
| `wt tag create <name>`     | Create a tag                                  |
| `wt release create <tag>`  | Create a release from a tag                   |
| `wt revert <snapshot>`     | Revert a snapshot (creates new snapshot)      |
| `wt archive <format>`      | Export tree as archive (license-aware)        |
| `wt reflog`                | Show operation log                            |
| `wt todo list`             | Show pending TODOs for current tree           |
| `wt todo claim <id>`       | Claim a TODO                                  |
| `wt todo complete <id>`    | Mark a TODO as complete                       |
| `wt depend add <tree>`     | Add a dependency on another tree              |
| `wt deps graph`            | Visualize dependency graph                    |
| `wt merge-request create`  | Create a merge request                        |
| `wt restore <file>`        | Restore a file to its last snapshot state     |
| `wt sync pause` / `resume` | Pause/resume staged snapshot sync             |
| `wt git export`            | Export tree as a Git repository               |
| `wt git mirror`            | Live mirror to/from a Git remote              |

---

## Glossary

| Term                | Definition                                                                                                  |
| ------------------- | ----------------------------------------------------------------------------------------------------------- |
| **W0rkTree**        | Top-level organizational unit containing one or more trees. Marketing name uses zero; code uses `worktree`. |
| **Tree**            | Fundamental unit of code organization with independent history, branches, and access rules.                 |
| **Snapshot**        | Immutable, content-addressed record of complete tree state at a point in time.                              |
| **Staged Snapshot** | Snapshot synced to server for team visibility but not yet part of branch history.                           |
| **Branch**          | Named pointer to a snapshot chain within a tree.                                                            |
| **Linked Branch**   | Branches across different trees that must be merged together.                                               |
| **Tenant**          | Verified user or organization on the W0rkTree server.                                                       |
| **BGProcess**       | Local background daemon running on the developer's machine.                                                 |
| **Server**          | Remote server — the canonical source of truth.                                                              |
| **Tag**             | Immutable named reference to a specific snapshot.                                                           |
| **Release**         | Tag with attached artifacts, notes, and status.                                                             |
| **Merge Request**   | Request to merge one branch into another, with review and CI gates.                                         |
| **Reflog**          | Chronological log of all operations that change branch tips.                                                |
| **Ceiling Model**   | Access control model where parent levels set maximum permissions children cannot exceed.                    |
| **Stub Tree**       | Tree that exists in metadata but whose files haven't been synced locally.                                   |
| **FastCDC**         | Content-defined chunking algorithm used for large file storage.                                             |
| **SPDX**            | Software Package Data Exchange — standard for license identifiers.                                          |
| **Registered Path** | Explicitly declared path that can be targeted by access policies (no glob guessing).                        |

---

## Implementation Status

### Implemented

- `core::hash` — BLAKE3 content-addressable hashing
- `core::id` — Typed UUID identifiers
- `core::error` — Protocol error types
- `object::*` — All object types (blob, tree, snapshot, branch, manifest, delta, tag, release, reflog, dependency, staged, merge_request)
- `iam::*` — Account, tenant, team, role, permission, scope, policy, engine types
- `access::*` — Resource targeting, tree/branch ACLs
- `config::*` — Worktree config, tree config, hierarchy model
- `feature::wire` — Binary wire protocol format
- `feature::diff` — Diff computation types
- `feature::merge` — Merge strategy types

### In Progress

- `feature::sync_protocol` — Sync message types
- `feature::ignore` — Ignore pattern engine
- `feature::licensing` — License compliance types
- `feature::large_file` — Large file chunking types

### Planned

- `feature::compat` — Git compatibility layer
- `feature::archive` — Archive/export
- `feature::audit` — Audit logging
- Full gRPC service definitions
- QUIC transport implementation

---

## License

This crate is licensed under the **W0rkTree Public License v1.0** — a copyleft license based on the GNU GPL v2 with a single additional clause: **Brand Protection** (Section 11).

### What This Means

| Can I...                                               | Answer                                     |
| ------------------------------------------------------ | ------------------------------------------ |
| Use commercially?                                      | **Yes**                                    |
| Modify the source?                                     | **Yes**                                    |
| Distribute modified versions?                          | **Yes** — with attribution and source code |
| Use privately without limits?                          | **Yes**                                    |
| Create plugins, extensions, and integrations?          | **Yes** — under any name you choose        |
| Build competing products from scratch?                 | **Yes**                                    |
| Fork and contribute back?                              | **Yes**                                    |
| Strip the W0rkTree name and rebrand as my own product? | **No**                                     |
| Remove attribution to W0rkTree?                        | **No**                                     |
| Distribute without source code?                        | **No**                                     |

### The Brand Protection Clause

The sole addition beyond GPL v2 is Section 11, which requires:

1. **Attribution preservation** — modified versions must retain clear attribution to the W0rkTree project in README files, about screens, and license headers.
2. **No rebranding** — you cannot remove the W0rkTree identity and present a modified version as an independent, original product unrelated to W0rkTree.
3. **Distinguishing modified versions** — public forks must either include "W0rkTree" with a modifier (e.g., "W0rkTree Community Edition") or use a different name with the required attribution (e.g., "MyProject — based on W0rkTree").

This does **not** restrict private use, internal modifications, functional changes, plugins, interoperable tools, clean-room implementations of the protocol, or any of the four freedoms guaranteed by the GPL.

The full license text is at [`LICENSE`](../../LICENSE) in the repository root.

```text
SPDX-License-Identifier: LicenseRef-W0rkTree-Public-License-1.0
```

---

<div align="center">

**W0rkTree is not the next version of Git. It is what comes after Git.**

</div>
