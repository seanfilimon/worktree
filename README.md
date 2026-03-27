<div align="center">

# W0rkTree

### **The version control system that comes after Git.**

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org)
[![TypeScript](https://img.shields.io/badge/typescript-5.3-blue.svg)](https://www.typescriptlang.org)
[![License](https://img.shields.io/badge/license-W0rkTree%20Public%20License%20v1.0-blue.svg)](#license)
[![Status](https://img.shields.io/badge/status-active%20development-brightgreen.svg)](#implementation-status)

W0rkTree is a **complete Git replacement** — not a wrapper, not an extension, not a hosting layer.
It is an independent version control system with its own protocol, storage model, identity system,
access control engine, license compliance framework, and real-time collaboration primitives.

It speaks Git only when necessary — for migration and interoperability — and nothing more.

[Architecture](#architecture) · [Getting Started](#getting-started) · [CLI Reference](#command-reference) · [Specifications](#specifications) · [Contributing](./CONTRIBUTING.md)

</div>

---

## Why W0rkTree Exists

Git was designed in 2005 to track the Linux kernel. Two decades later, every engineering team uses it — not because Git solves their problems, but because nothing better existed.

Git's problems are not bugs. They are fundamental design decisions that cannot be fixed without replacing the system:

| Problem | Git's Reality | W0rkTree's Solution |
|---|---|---|
| **No organization model** | A repo is a bag of files. Teams, ownership, tenants — all bolted on externally. | First-class multi-tenant architecture with verified identity. |
| **No collaboration visibility** | Work is invisible until push. No one knows what anyone else is doing. | Staged snapshots — team sees WIP in real-time without polluting history. |
| **Destructive by design** | `rebase`, `reset --hard`, `push --force` — data loss is one command away. | Append-only history. No rebase. No force-push. Ever. |
| **No access control** | Zero built-in permissions. File-level, branch-level, path-level — all external. | Declarative TOML policies with RBAC + ABAC, version-controlled alongside code. |
| **LFS as an afterthought** | Large files need a separate system with its own server and failure modes. | Native chunked storage with lazy loading. Zero configuration. |
| **No license enforcement** | Compliance is entirely honor-system. No mechanism to prevent code theft. | Per-path SPDX licensing, server-enforced on every operation. |
| **Cryptic command surface** | 150+ commands. `checkout` does three things. `reset` has five modes. | One job per command. Plain terminology. Snapshot, not commit. |
| **Painful submodules** | A separate, notoriously broken system for nested repositories. | Nested trees — native, consistent, independently versioned. |

---

## Git vs W0rkTree — Full Comparison

| Aspect | Git | W0rkTree |
|---|---|---|
| **Architecture** | Monolithic local tool + separate hosting | Two-runtime: local bgprocess + remote server |
| **Organization** | Single flat repo per project | Multi-tenant trees with nested subtrees |
| **Identity** | Name + email (no verification) | Verified tenant: username + email + type |
| **Terminology** | commit, repository, checkout, stash | snapshot, tree, switch — plain language |
| **Staging** | Explicit `git add` required | No staging area — snapshot captures working state |
| **Commands** | 150+ commands, many overloaded | One job per command, no overloading |
| **Branches** | Global namespace | Tree-scoped with independent strategies |
| **Access control** | None built-in | Declarative TOML policies, RBAC + ABAC, ceiling model |
| **Merge** | Merge, rebase, cherry-pick, squash | Merge only. No rebase. Append-only history. |
| **History** | Rewritable (rebase, reset, force-push) | Append-only. Non-destructive. Soft deletes. |
| **Large files** | Requires Git LFS (separate system) | Native chunked storage with lazy loading |
| **Collaboration** | Invisible until push | Staged snapshots — team sees WIP in real-time |
| **License tracking** | None | Per-path SPDX, server-enforced compliance |
| **Dependencies** | None | Three-level system with auto-TODO generation |
| **Project management** | External tools only | Built-in per-tree structured task management |
| **Submodules** | Separate, notoriously painful system | Nested trees — native, consistent, reliable |
| **Recovery** | `git reflog` (local, expires) | Full reflog, server-synced, configurable retention |
| **Multi-tenancy** | Not supported | First-class tenants, cross-tenant sharing |
| **Conflict resolution** | Basic markers | Three-way markers + machine-readable `.wt/conflicts/` |
| **Monitoring** | None | Server-side telemetry, sync health, audit logs |
| **Security** | No auth, no encryption in native protocol | TLS 1.3, mandatory auth, Ed25519 signing, secret scanning |

---

## Architecture

W0rkTree operates as a **two-runtime system**. Neither runtime is optional.

```
┌──────────────────────────────────────────────────────┐
│                  Developer Machine                    │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │            worktree-bgprocess                   │  │
│  │                                                 │  │
│  │  • Filesystem watcher (OS-native APIs)          │  │
│  │  • Auto-snapshot engine                         │  │
│  │  • Local snapshot history & DAG                 │  │
│  │  • Branch management                            │  │
│  │  • .wt/ and .wt-tree/ folder management         │  │
│  │  • Staged snapshot sync → server                │  │
│  │  • Large file chunking (FastCDC)                │  │
│  │  • Lazy loading (FUSE / ProjFS)                 │  │
│  │  • Auto-merge for non-conflicting changes       │  │
│  │  • Ignore pattern engine                        │  │
│  │  • Secret scanning (pre-snapshot)               │  │
│  └──────────────────────┬──────────────────────────┘  │
│                         │                             │
└─────────────────────────┼─────────────────────────────┘
                          │  W0rkTree Sync Protocol
                          │  QUIC (TLS 1.3) / gRPC (HTTP/2 fallback)
┌─────────────────────────┼─────────────────────────────┐
│                         │                             │
│  ┌──────────────────────┴──────────────────────────┐  │
│  │              worktree-server                    │  │
│  │                                                 │  │
│  │  • Canonical history (source of truth)          │  │
│  │  • Multi-tenant isolation                       │  │
│  │  • IAM: tenants, teams, roles, RBAC + ABAC      │  │
│  │  • Staged snapshot aggregation & visibility      │  │
│  │  • Branch protection enforcement                │  │
│  │  • License compliance enforcement               │  │
│  │  • Merge request system (review + CI gates)     │  │
│  │  • Tag & release management                     │  │
│  │  • Audit logging (immutable, append-only)       │  │
│  │  • API: gRPC + REST + WebSocket                 │  │
│  └─────────────────────────────────────────────────┘  │
│                    Remote Server                       │
└───────────────────────────────────────────────────────┘
```

### Separation of Concerns (Hard Constraints)

- The **bgprocess never enforces access control** — it reads policies for local display only.
- The **server never touches the working directory** or creates snapshots.
- The **bgprocess never stores canonical history** — its local history is a sync cache.
- The **server never bypasses its own enforcement**, even for admin operations.

---

## Project Structure

W0rkTree is a polyglot monorepo managed with **Cargo workspaces** (Rust) and **Turborepo + pnpm** (TypeScript/web).

```
worktree/
├── crates/                              # ── Rust Workspace ──
│   ├── worktree-protocol/               # Protocol definitions — the heart of everything
│   │   ├── src/
│   │   │   ├── core/                    #   BLAKE3 hashing, typed UUIDs, error types
│   │   │   ├── object/                  #   Blob, tree, snapshot, branch, manifest, delta,
│   │   │   │                            #   tag, release, reflog, dependency, staged, merge request
│   │   │   ├── iam/                     #   Account, tenant, team, role, permission, scope,
│   │   │   │                            #   policy, access decision engine
│   │   │   ├── access/                  #   Resource targeting, tree/branch ACLs
│   │   │   ├── config/                  #   Worktree config, tree config, hierarchy model
│   │   │   └── feature/                 #   Diff, merge, wire protocol, Git compat, ignore,
│   │   │                                #   licensing, large file, sync, archive, audit
│   │   └── specs/                       # Authoritative specification documents (14 specs)
│   │
│   ├── worktree-sdk/                    # Local engine — snapshots, branches, diffs, merges
│   ├── worktree-server/                 # Background daemon — watcher, auto-snapshot, sync, gRPC
│   ├── worktree-cli/                    # CLI binary (`wt`) — 20 subcommands, colored output
│   ├── worktree-git/                    # Git compatibility — import, export, SHA-1↔BLAKE3 bridge
│   └── worktree-admin/                  # Admin panel — Yew WASM SPA + Axum HTTP API
│
├── apps/                                # ── TypeScript Workspace ──
│   └── web/                             # Marketing & docs site (Next.js 16, Fumadocs, shadcn)
│
├── docs/                                # User-facing documentation
│   ├── cli-reference.md
│   ├── server-architecture.md
│   ├── protocol-spec.md
│   ├── git-compatibility.md
│   ├── sdk-guide.md
│   └── admin-panel.md
│
├── tests/                               # Cross-crate test suites
│   ├── protocol_tests/
│   ├── server_tests/
│   ├── git_compat_tests/
│   └── e2e_tests/
│
├── scripts/                             # Build & install scripts
│   ├── ci.sh                            # CI pipeline (fmt → clippy → test → build)
│   ├── install.sh                       # Unix installer
│   └── install.ps1                      # Windows installer
│
├── Cargo.toml                           # Rust workspace root
├── package.json                         # Node/pnpm workspace root (Turborepo)
├── turbo.json                           # Turborepo pipeline config
├── LICENSE                              # W0rkTree Public License v1.0
├── CONTRIBUTING.md                      # Contributor guide
└── README.md                            # ← You are here
```

### Crate Dependency Graph

```
worktree-protocol          ← Foundation: every crate depends on this
    ↑
    ├── worktree-sdk       ← Core local engine (snapshots, branches, diffs, merges)
    │       ↑
    │       └── worktree-cli       ← CLI binary (`wt`)
    │
    ├── worktree-git       ← Git bridge (libgit2, SHA-1↔BLAKE3 index)
    │       ↑
    │       └── worktree-server    ← Background daemon (watcher, sync, gRPC, RBAC)
    │
    └── worktree-admin     ← Admin panel (Yew WASM + Axum SSR)
```

---

## Crate Overview

### `worktree-protocol` — The Foundation

Every type, wire format, access control primitive, and protocol message in the system. The single source of truth for the binary protocol spoken between the two runtimes.

| Module | What It Defines |
|---|---|
| `core::hash` | BLAKE3 content-addressable hashing (faster than SHA-256) |
| `core::id` | Typed UUID v4 identifiers (SnapshotId, TreeId, BranchId, TenantId, ...) |
| `object::*` | 12 object types: blob, tree, snapshot, branch, manifest, delta, tag, release, reflog, dependency, staged, merge_request |
| `iam::*` | Accounts, tenants, teams, 5 built-in roles, 20 atomic permissions, RBAC + ABAC engine |
| `config::*` | Root config, tree config, permission ceiling hierarchy |
| `feature::*` | Diff, merge, wire protocol, Git compat, ignore, licensing, large file, sync, archive, audit |

**[Full protocol README →](./crates/worktree-protocol/README.md)**

### `worktree-sdk` — The Local Engine

The core library that performs all local repository operations. Operates entirely on the local filesystem using a `.wt/` metadata directory.

| Component | Status | Description |
|---|---|---|
| `WorktreeEngine` | ✅ Complete | Central entry point — init, open, path resolution |
| `init` | ✅ Complete | Full `.wt/` directory scaffolding with default config |
| `status` | ✅ Complete | BLAKE3-based change detection (added/modified/deleted) |
| `snapshot` | ✅ Complete | File collection, hashing, no-change detection, atomic save |
| `branch` | ✅ Complete | Create, list, switch, delete (with protection) |
| `tree` | ✅ Complete | Add, list, remove sub-projects |
| `merge` | ✅ Complete | Hash-based conflict detection, merge snapshots |
| `diff` | ✅ Complete | Working tree + snapshot-to-snapshot comparison |
| `tag` | ✅ Complete | Create, list, delete |
| `sync` | 🔶 Stub | Push/pull placeholders for future server integration |

### `worktree-server` — The Background Daemon

Long-running daemon that watches the filesystem, auto-snapshots, syncs with the remote server, enforces RBAC, and exposes a gRPC API.

| Component | Status | Description |
|---|---|---|
| `watcher::fs` | ✅ Complete | Platform-native filesystem watcher (notify crate) |
| `watcher::debounce` | ✅ Complete | Event deduplication with configurable time window |
| `auth::session` | ✅ Complete | JWT-style session tokens with expiry |
| `auth::enforcer` | ✅ Complete | Permission enforcement with hierarchical scope matching |
| `storage::index` | ✅ Complete | In-memory object index (hash → kind) |
| `storage::disk` | 🔶 Partial | Git-style fan-out paths (store/retrieve stubs) |
| `engine::rules` | ✅ Complete | Declarative condition/action automation rules |
| `engine::event` | 🔶 Types only | Semantic event classification (CodeChange, ConfigChange, ...) |
| `engine::auto_commit` | 🔶 Structure | Threshold-based auto-snapshot engine |
| `sync::transport` | ✅ Complete | QUIC/TCP transport abstraction |
| `api::handlers` | 🔶 Stubs | Init, status, snapshot, branch request handlers |
| `service::health` | ✅ Complete | Health tracking (uptime, trees watched, snapshots created) |

### `worktree-cli` — The CLI (`wt`)

**20 top-level commands** with **10 nested sub-enums**. Delegates all business logic to `worktree-sdk`. Colored output with semantic formatting.

| Command | Sub-actions | Description |
|---|---|---|
| `wt init` | — | Initialize a new worktree |
| `wt status` | `--team` | Show working tree status (or team-wide staged activity) |
| `wt snapshot` | `-m`, `-t` | Create a snapshot |
| `wt log` | `-n count` | Show snapshot history |
| `wt branch` | `create / list / switch / delete` | Branch management |
| `wt merge` | `--strategy` | Merge branches |
| `wt sync` | `push / pull / pause / resume` | Remote synchronization |
| `wt tree` | `add / list / remove / status` | Sub-project management |
| `wt diff` | `--name-only`, `--stat` | Show differences |
| `wt tag` | `create / list / delete` | Tag management |
| `wt config` | `show / get / set` | Configuration management |
| `wt reflog` | `-n count` | Operation history |
| `wt revert` | — | Revert a snapshot |
| `wt archive` | `--format`, `--tree` | Create archive |
| `wt depend` | `add / list / todo` | Dependency management |
| `wt staged` | `list / clear` | Team visibility — staged snapshots |
| `wt ignore` | `list / add` | Ignore pattern management |
| `wt permission` | `set / get / list` | Access control management |
| `wt git` | `import / export / clone / remote / push / pull / mirror` | Git interoperability |
| `wt server` | `start / stop / status` | Background process management |

### `worktree-git` — Git Compatibility Layer

Bidirectional bridge between Git (SHA-1, commits, trees, blobs) and W0rkTree (BLAKE3, snapshots, manifests, blobs).

| Component | Status | Description |
|---|---|---|
| `hash_index::store` | ✅ Complete | SHA-1 ↔ BLAKE3 bidirectional O(1) lookup |
| `config::gitattributes` | ✅ Complete | `.gitattributes` parser with full test coverage |
| `config::gitignore` | ✅ Complete | `.gitignore` ↔ `.wt/ignore` converter |
| `import::repo` | ✅ Complete | Git repository wrapper (open, branches, head, commit count) |
| `import::walker` | ✅ Complete | Topological commit graph traversal (oldest-first) |
| `import::submodule` | ✅ Complete | Submodule discovery and extraction |
| `export::builder` | ✅ Complete | Git repository initialization (bare, initial branch) |
| `remote::transport` | ✅ Complete | HTTPS/SSH auto-detection |
| `remote::auth` | ✅ Complete | SSH key + credential helper authentication |
| `import::converter` | 🔶 API only | Git commit/tree/blob → W0rkTree snapshot/manifest/blob |
| `export::converter` | 🔶 API only | W0rkTree → Git object conversion |
| `remote::push/pull` | 🔶 Stubs | Network push/pull operations |

### `worktree-admin` — Admin Panel

Dual-mode web interface: **Yew WASM SPA** (client-side) + **Axum HTTP API** (server-side, `--features ssr`).

| Component | Status | Description |
|---|---|---|
| **8 Yew components** | ✅ Complete | Navbar, Card, Badge, Button, StatCard, RepoCard, Loading, Footer |
| **Routing** | ✅ Complete | 6 routes: Dashboard, Repositories, Detail, Statistics, Settings, 404 |
| **CSS system** | ✅ Complete | shadcn/ui-inspired CSS variables + inline style helpers |
| **Axum API** | ✅ Complete | 10 endpoints: health, status, metrics, server control, repos, stats, GC |
| **Auth middleware** | ✅ Complete | Bearer token validation with bypass option |
| **Error handling** | ✅ Complete | 11 error variants → HTTP status codes with JSON bodies |
| **Page components** | 🔶 Planned | Dashboard, Repositories, Statistics, Settings pages |
| **Real server integration** | 🔶 Planned | Currently returns mock data |

### `@worktree/web` — Marketing & Documentation Site

Next.js 16 application with Fumadocs for documentation, Radix UI primitives, shadcn components, and Tailwind CSS v4.

---

## Core Concepts

### Snapshots — Not Commits

Snapshots are **immutable, content-addressed records** of the complete state of a tree at a point in time.

- **No staging area.** No `add` command. No index. The bgprocess watches the filesystem and snapshots the working state.
- **Auto-created.** The bgprocess creates snapshots automatically as you work.
- **Append-only.** Snapshots are only ever added to history, never removed or reordered.
- **Content-addressed.** Identical states produce the same BLAKE3 hash → automatic deduplication.

### Trees — Not Repositories

Trees are the **fundamental unit of code organization**. Each tree has independent snapshot history, independent branches, independent access rules, independent license config, and can contain **nested subtrees** (no submodules).

```
my-worktree/
├── .wt/                    # Root worktree configuration
├── frontend/               # ← Tree (independent history & branches)
│   └── .wt-tree/
├── backend/                # ← Tree
│   └── .wt-tree/
├── shared/                 # ← Tree
│   ├── .wt-tree/
│   └── models/             # ← Nested subtree
│       └── .wt-tree/
└── mobile/                 # ← Tree
    └── .wt-tree/
```

### Staged Snapshots — Real-Time Team Visibility

The feature that changes how teams work:

```
Git:       edit → (invisible) → push → team sees work
W0rkTree:  edit → auto-snapshot → staged (team sees WIP) → push (permanent)
```

- Staged snapshots are **visible to the team** — colleagues see what files you changed, on which branch, in which tree.
- Staged snapshots are **NOT part of branch history** — they don't pollute the branch with WIP.
- When ready, `wt push` finalizes staged snapshots into branch history.
- Answers "what is everyone working on right now?" without standups, Slack messages, or ticket systems.

### Multi-Tenant Architecture

Every W0rkTree is owned by a **tenant** — a verified user or organization with a unique slug, verified email, and configurable plan (Free, Pro, Enterprise). Cross-tenant access is granted via simple TOML config or full IAM policies.

### Three-Level Dependency System

| Level | What It Tracks | Example |
|---|---|---|
| **Tree dependencies** | Tree A depends on Tree B | `frontend` requires `shared-models >= 1.0.0` |
| **Branch dependencies** | Feature branch in Tree A depends on feature branch in Tree B | `frontend/feature-oauth` blocks on `backend/feature-oauth` |
| **Snapshot dependencies** | Individual snapshot declares requirements on other trees | Alice's snapshot auto-generates a TODO branch in `backend` |

Dependencies can be **linked** (must merge together) and **blocking** (prevent merge until resolved). Auto-generated TODO branches include structured metadata: title, description, priority, assignee, linked files.

### License Compliance

Per-path SPDX licensing with server-enforced compliance:

```
1. IAM check:     Does this tenant have permission?    → YES/NO
2. License check:  Does this file's license allow it?   → YES/NO
3. Final:          BOTH must pass.
```

Grant levels: `read-only` (view only), `modify` (edit, no export), `redistribute` (full rights). Proprietary code cannot be exported, forked, or synced without an explicit grant.

---

## Getting Started

### Prerequisites

| Tool | Version | Purpose |
|---|---|---|
| **Rust** | stable (2021 edition) | Crate compilation |
| **Cargo** | latest stable | Rust package manager |
| **Node.js** | ≥ 18.0.0 | Web frontend, docs site |
| **pnpm** | ≥ 8.0.0 | Node package manager |

### Quick Start

```bash
# Clone the repository
git clone https://github.com/seanfilimon/worktree.git
cd worktree

# Build all Rust crates
cargo build --workspace

# Install Node dependencies & build web
pnpm install
pnpm build

# Run all tests
cargo test --workspace

# Run the full CI pipeline locally
bash scripts/ci.sh
```

### Running the CLI

```bash
# Build the CLI binary
cargo build -p worktree-cli

# Initialize a new worktree
./target/debug/wt init

# Create a snapshot
./target/debug/wt snapshot -m "Initial snapshot"

# Check status
./target/debug/wt status

# View history
./target/debug/wt log
```

### Running the Admin Panel

```bash
# Install Trunk (WASM build tool)
cargo install trunk
rustup target add wasm32-unknown-unknown

# Start the admin panel dev server
cd crates/worktree-admin
trunk serve
# → Open http://127.0.0.1:3000
```

### Running the Docs Site

```bash
# Start the Next.js dev server
cd apps/web
pnpm dev
# → Open http://localhost:3000
```

---

## Configuration

W0rkTree uses a **four-level configuration hierarchy** with strict precedence:

```
System defaults (lowest priority)
  └── User global config
        └── .wt/config.toml (root worktree)
              └── .wt-tree/config.toml (per-tree override — highest priority)
```

### `.wt/` — Root Worktree Directory

| Path | Purpose |
|---|---|
| `config.toml` | Root config: sync, auto-snapshot, storage, licensing, tenant access, branch protection |
| `ignore` | Root-level ignore patterns (authoritative) |
| `identity/` | Auth tokens, user identity, Ed25519 signing keys |
| `access/roles.toml` | Custom role definitions |
| `access/policies.toml` | Root-level RBAC + ABAC policies |
| `hooks/` | Pre/post-snapshot hooks |
| `reflog/` | Operation history per branch |
| `conflicts/` | Machine-readable merge conflict metadata (JSON) |
| `cache/` | Local computation cache (deletable, not synced) |

### `.wt-tree/` — Per-Tree Configuration

Each tree can override root settings with the **restriction-only invariant**: tree-level config can restrict but never expand what the root allows.

| Path | Purpose |
|---|---|
| `config.toml` | Tree overrides: snapshot intervals, large file thresholds, license, branch protection |
| `ignore` | Tree-level ignore patterns (additive to root) |
| `access/policies.toml` | Tree-scoped access policies (can only restrict) |
| `hooks/` | Tree-level hooks (run after root hooks) |

### Example Root Config

```toml
[worktree]
name = "my-project"
tenant = "acme-corp"
visibility = "shared"

[sync]
auto = true
interval_secs = 30

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 30
max_changed_files = 50

[large_files]
threshold_bytes = 10485760    # 10 MB

[license]
default = "MIT"
spdx_strict = true

[[license.path]]
path = "services/billing-engine"
license = "proprietary"

[[tenant_access]]
tenant = "partner-corp"
permissions = ["tree:read", "sync:pull"]

[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review", "no_delete"]

[branch_protection.review]
min_approvals = 2
require_from_roles = ["maintainer", "admin"]
```

---

## IAM System

W0rkTree includes a full Identity and Access Management system — no external platform required.

### Built-in Roles

```
owner ⊃ admin ⊃ maintainer ⊃ contributor ⊃ reader
```

### 20 Atomic Permissions

| Category | Permissions |
|---|---|
| **Tree** | `tree:read`, `tree:write`, `tree:create`, `tree:delete` |
| **Branch** | `branch:create`, `branch:delete`, `branch:protect` |
| **Snapshot** | `snapshot:create`, `snapshot:revert`, `snapshot:sign` |
| **Sync** | `sync:push`, `sync:pull`, `sync:force_push` |
| **Management** | `manage:roles`, `manage:teams`, `manage:policies` |
| **Admin** | `admin:tenant`, `admin:audit_read`, `admin:bypass_protection` |
| **Tags & Releases** | `tag:create`, `release:create` |
| **Merge Requests** | `mr:create`, `mr:review`, `mr:merge` |

### Scope Hierarchy & Ceiling Model

```
Global → Tenant → Tree → Branch → RegisteredPath
```

- **Deny always beats allow** at the same scope level.
- Parent levels set **maximum** permissions — children **cannot** expand beyond the ceiling.
- `.wt/access/` defines the root ceiling; `.wt-tree/access/` can only restrict.

### ABAC Conditions

Policies support attribute-based conditions: `time.hour`, `time.day_of_week`, `source.ip`, `user.department`, custom tenant-defined attributes.

---

## Security

| Layer | Mechanism |
|---|---|
| **Transport** | TLS 1.3 (QUIC native) / mTLS (enterprise). No unencrypted transport. |
| **Authentication** | OAuth2 device flow, API keys, JWT tokens with auto-refresh |
| **Access control** | 20 atomic permissions, RBAC + ABAC, scope hierarchy, deny-beats-allow |
| **Integrity** | BLAKE3 hash verification on every object read |
| **Signing** | Ed25519 snapshot signatures. Branch protection can require signed snapshots. |
| **Secret scanning** | Pre-snapshot regex scanning (AWS keys, Stripe keys, private keys, custom). Block-on-match. |
| **Audit logging** | Immutable, append-only log of every access decision, policy change, sync operation |
| **License enforcement** | Server blocks unauthorized export/fork/sync of proprietary code |
| **Data at rest** | Optional per-tenant encryption with envelope encryption and key rotation |
| **IPC** | Unix sockets (owner-only) / Windows named pipes (ACLs). No network exposure. |

---

## Storage Model

### Content-Addressable Objects

All objects addressed by BLAKE3 hash — faster than SHA-256, with tree-hashing mode for large inputs:

| Object | Description |
|---|---|
| **Blob** | Raw file content |
| **Tree** | Directory listing: `[(name, type, hash), ...]` |
| **Snapshot** | Complete tree state at a point in time |
| **Manifest** | Large file chunk map: `[(offset, size, chunk_hash), ...]` |
| **Delta** | Compressed diff between two versions |
| **Tag** | Named reference to a snapshot |
| **Branch** | Mutable pointer to tip snapshot |

### Large File Handling — No LFS

Files above threshold (default: 10 MB) are automatically chunked using **FastCDC**:

- Content-defined boundaries → inserting data only affects nearby chunks
- Independent content-addressing → automatic cross-file/cross-version deduplication
- Lazy loading via FUSE (Linux/macOS) or ProjFS (Windows)
- LRU chunk cache (default: 2 GB)
- **Zero configuration required**

### Wire Format

```
┌─────────┬─────────┬───────┬──────────┬────────────────┬──────────┬──────────┐
│ Magic   │ Version │ Flags │ Msg Type │ Payload Length │ Payload  │ Checksum │
│ "WT01"  │ u8      │ u8    │ u16      │ u32            │ bincode  │ BLAKE3   │
└─────────┴─────────┴───────┴──────────┴────────────────┴──────────┴──────────┘
```

Serialization: Bincode (sync protocol), JSON (REST API). Compression: zstd level 3 for payloads > 1 KB.

---

## Command Reference

| Command | Description |
|---|---|
| `wt init` | Initialize a new worktree |
| `wt init --from <url>` | Initialize from a remote worktree |
| `wt init --from-git <url>` | Import from a Git repository |
| `wt snapshot` | Create a manual snapshot |
| `wt snapshot -m <msg>` | Create a snapshot with a message |
| `wt push` | Finalize staged snapshots into branch history |
| `wt sync` | Bidirectional sync with server |
| `wt sync pause` / `resume` | Pause/resume staged snapshot sync |
| `wt branch create <name>` | Create a new branch |
| `wt branch switch <name>` | Switch to a branch |
| `wt branch list` | List branches in current tree |
| `wt branch delete <name>` | Soft-delete a branch (recoverable) |
| `wt merge <branch>` | Merge a branch into current branch |
| `wt diff` | Show changes |
| `wt log` | Show snapshot history |
| `wt status` | Show working tree status |
| `wt status --team` | Show staged activity from all team members |
| `wt staged` | List all staged (unpushed) snapshots |
| `wt staged clear` | Clear staged snapshots |
| `wt tag create <name>` | Create a tag |
| `wt release create <tag>` | Create a release from a tag |
| `wt revert <snapshot>` | Revert a snapshot (creates new corrective snapshot) |
| `wt archive <format>` | Export tree as archive (license-aware) |
| `wt reflog` | Show operation log |
| `wt tree add <path>` | Add a nested tree |
| `wt tree list` | List all trees |
| `wt tree remove <path>` | Remove a tree |
| `wt todo list` | Show pending TODOs for current tree |
| `wt todo claim <id>` | Claim a TODO |
| `wt todo complete <id>` | Mark a TODO as complete |
| `wt depend add <tree>` | Add a dependency on another tree |
| `wt deps graph` | Visualize dependency graph |
| `wt merge-request create` | Create a merge request |
| `wt permission set` | Set access control policies |
| `wt permission list` | List all policies |
| `wt config show` | Display current configuration |
| `wt ignore list` / `add` | Manage ignore patterns |
| `wt restore <file>` | Restore a file to its last snapshot state |
| `wt git import <repo>` | Import from Git |
| `wt git export <path>` | Export to Git (with license filtering) |
| `wt git mirror <url>` | Live mirror to/from a Git remote |
| `wt server start` / `stop` / `status` | Manage background process |

---

## Specifications

The protocol is defined by 14 authoritative specification documents in [`crates/worktree-protocol/specs/`](./crates/worktree-protocol/specs/):

| Specification | Document | Covers |
|---|---|---|
| **Protocol Overview** | [`specs/README.md`](./crates/worktree-protocol/specs/README.md) | Architecture, terminology, Git comparison, innovation summary |
| **W0rkTree Core** | [`specs/WorkTree.md`](./crates/worktree-protocol/specs/WorkTree.md) | Full system: trees, snapshots, tenants, dependencies, merge, tags, licensing, diff, Git compat |
| **Tree Spec** | [`specs/tree/Tree.md`](./crates/worktree-protocol/specs/tree/Tree.md) | Trees, branches, snapshots, nesting, dependencies, linked branches, cross-tree coordination |
| **BGProcess** | [`specs/bgprocess/BgProcess.md`](./crates/worktree-protocol/specs/bgprocess/BgProcess.md) | Local daemon: auto-snapshot, watcher, staged sync, chunking, IPC, platform support |
| **Server** | [`specs/server/Server.md`](./crates/worktree-protocol/specs/server/Server.md) | Remote server: tenant isolation, IAM, branch protection, merge requests, API surface |
| **IAM** | [`specs/iam/IAM.md`](./crates/worktree-protocol/specs/iam/IAM.md) | Roles, permissions, scopes, RBAC + ABAC policies, access decision engine |
| **Declarative Access** | [`specs/iam/DeclarativeAccess.md`](./crates/worktree-protocol/specs/iam/DeclarativeAccess.md) | Path registration, custom roles, policy authoring, tree overrides, full examples |
| **Tenant Model** | [`specs/iam/TenantModel.md`](./crates/worktree-protocol/specs/iam/TenantModel.md) | Tenant types, lifecycle, cross-tenant access, visibility, orgs, ABAC attributes |
| **Staged Visibility** | [`specs/visibility/StagedVisibility.md`](./crates/worktree-protocol/specs/visibility/StagedVisibility.md) | Staged snapshot pipeline, visibility surfaces, privacy controls, license interaction |
| **Sync Protocol** | [`specs/sync/Sync.md`](./crates/worktree-protocol/specs/sync/Sync.md) | Staged upload, push/pull, delta sync, offline mode, have/want, transport, wire format |
| **Storage** | [`specs/storage/Storage.md`](./crates/worktree-protocol/specs/storage/Storage.md) | Objects, BLAKE3, FastCDC chunking, pack files, GC, quotas, shallow/partial sync |
| **`.wt/` Directory** | [`specs/dot-wt/DotWt.md`](./crates/worktree-protocol/specs/dot-wt/DotWt.md) | Root config, ignore, identity, access, hooks, reflog, conflicts, cache |
| **`.wt-tree/` Directory** | [`specs/dot-wt-tree/DotWtTree.md`](./crates/worktree-protocol/specs/dot-wt-tree/DotWtTree.md) | Per-tree config, authority model, nesting rules, tree-level policies |
| **License Compliance** | [`specs/licensing/LicenseCompliance.md`](./crates/worktree-protocol/specs/licensing/LicenseCompliance.md) | SPDX assignment, grant model, server enforcement, Git export handling |
| **Security** | [`specs/security/Security.md`](./crates/worktree-protocol/specs/security/Security.md) | Transport, auth, signing, secret scanning, encryption, audit, threat model |

### Recommended Reading Order

1. `specs/README.md` — Architecture and terminology
2. `specs/WorkTree.md` — Full system design
3. `specs/tree/Tree.md` — Trees and cross-tree coordination
4. `specs/dot-wt/DotWt.md` + `specs/dot-wt-tree/DotWtTree.md` — Configuration
5. `specs/bgprocess/BgProcess.md` — Local runtime
6. `specs/server/Server.md` — Remote runtime
7. `specs/iam/IAM.md` → `DeclarativeAccess.md` → `TenantModel.md` — Access control
8. `specs/visibility/StagedVisibility.md` — Real-time collaboration
9. `specs/sync/Sync.md` → `specs/storage/Storage.md` — Protocol and storage
10. `specs/licensing/LicenseCompliance.md` → `specs/security/Security.md` — Compliance and security

---

## Implementation Status

### ✅ Complete

| Component | What's Done |
|---|---|
| **worktree-protocol** | All object types, IAM system, config hierarchy, wire format, diff/merge types |
| **worktree-sdk** | Init, snapshot, branch CRUD, tree CRUD, diff, merge, tag, status, reflog |
| **worktree-cli** | 20 commands with colored output, config management, TOML read/write |
| **worktree-git** | Hash index, gitattributes parser, repo wrapper, commit walker, submodule import, repo builder, transport, auth |
| **worktree-server** | Filesystem watcher, debouncer, session auth, permission enforcer, object index, health tracker, transport, rules engine |
| **worktree-admin** | 8 Yew components, routing, CSS system, Axum API (10 endpoints), auth middleware, error handling |
| **@worktree/web** | Next.js site with Fumadocs, shadcn, Tailwind v4 |
| **Specifications** | 14 detailed specifications covering the complete system |

### 🔶 In Progress

- Sync protocol messages and delta negotiation
- Server gRPC service definitions
- Git import/export object conversion
- Content-addressable object store (disk backend)
- Large file chunking integration
- License compliance types and SPDX validation

### 📋 Planned

- Full QUIC transport implementation
- Offline queue and reconnection logic
- Admin panel page components with real server integration
- WebSocket streaming for real-time staged visibility
- Snapshot signing (Ed25519) and verification
- Secret scanning engine
- Audit logging pipeline
- Archive/export with license compliance filtering
- Shell completions (bash, zsh, fish, PowerShell)

---

## Use Cases

### Microservices Architecture

One tree per service. Shared libraries as nested trees. Cross-service dependencies tracked with the three-level dependency system. Teams own their trees with independent branches, snapshots, and release cycles.

### Multi-Platform Applications

`frontend/`, `backend/`, `mobile/`, `shared/` as separate trees. Linked branches coordinate cross-platform features. The dependency system prevents partial deployments.

### Enterprise Codebases

Declarative access control at the path level, version-controlled alongside code. License compliance prevents unauthorized use of proprietary modules. Server enforces every rule without relying on developer discipline.

### Open Source with Proprietary Modules

Public trees for open-source code. Private nested trees for proprietary modules. Per-path SPDX licensing ensures boundaries are enforced at the protocol level — not by convention, by the server.

### Monorepo Migration from Git

`wt init --from-git <url>` imports a Git repository as a W0rkTree tree. Split into nested trees at your own pace. Git compatibility bridge keeps CI/CD pipelines working during migration.

---

## Design Principles

| # | Principle | Enforcement |
|---|---|---|
| 1 | **One job per command** | Every CLI command does exactly one thing. No overloaded flags. |
| 2 | **Plain terminology** | Snapshot, not commit. Tree, not repository. Sync, not push/pull/fetch. |
| 3 | **Automatic by default** | Auto-snapshot, auto-sync. Manual mode is opt-in. |
| 4 | **Append-only history** | No rebase. No `reset --hard`. No force-push. Ever. |
| 5 | **Non-destructive operations** | Soft deletes with configurable recovery windows. Server-synced reflog. |
| 6 | **Real-time collaboration** | Staged snapshot visibility is a core protocol feature. |
| 7 | **Security by default** | Auth, encryption, access control, licensing, audit — built into the protocol. |

---

## Tech Stack

| Layer | Technology |
|---|---|
| **Protocol & Core** | Rust 2021, BLAKE3, Bincode, Serde, UUID v4, Chrono |
| **Server / Daemon** | Tokio, Notify (fs watcher), Tracing, TOML config |
| **CLI** | Clap 4 (derive), Colored, WalkDir |
| **Git Bridge** | libgit2 (via git2 crate), SHA-1 ↔ BLAKE3 translation |
| **Admin Panel** | Yew 0.21 (WASM), Axum 0.7 (SSR), shadcn-style CSS variables |
| **Docs Site** | Next.js 16, Fumadocs, React 19, Radix UI, Tailwind CSS v4 |
| **Build** | Cargo workspaces, Turborepo, pnpm, Trunk (WASM) |
| **Transport** | QUIC (TLS 1.3 native), gRPC over HTTP/2 (fallback) |
| **Hashing** | BLAKE3 (content addressing), Ed25519 (snapshot signing) |

---

## Contributing

See **[CONTRIBUTING.md](./CONTRIBUTING.md)** for the full guide. Key points:

- **Spec-first development** — features have specs before code.
- **CI must pass** — `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `cargo build --release`.
- **Every PR needs tests** — no exceptions.
- **Conventional commits** — `feat(protocol): add StagedSnapshot type`.
- **Fork-and-branch** workflow — merge commits (no squash, no rebase — we practice what we preach).

### Founding Team

| Role | Name |
|---|---|
| **Founding Engineer** | **Sean Filimon** — [@seanfilimon](https://github.com/seanfilimon) |

---

## Glossary

| Term | Definition |
|---|---|
| **W0rkTree** | Top-level organizational unit containing one or more trees. Marketing name uses zero; code uses `worktree`. |
| **Tree** | Fundamental unit of code organization with independent history, branches, and access rules. |
| **Snapshot** | Immutable, content-addressed record of complete tree state at a point in time. |
| **Staged Snapshot** | Snapshot synced to server for team visibility but not yet part of branch history. |
| **Branch** | Named pointer to a snapshot chain within a tree. |
| **Linked Branch** | Branches across different trees that must be merged together. |
| **Tenant** | Verified user or organization on the W0rkTree server. |
| **BGProcess** | Local background daemon running on the developer's machine. |
| **Server** | Remote server — the canonical source of truth. |
| **Tag** | Immutable named reference to a specific snapshot. |
| **Release** | Tag with attached artifacts, notes, and status. |
| **Merge Request** | Request to merge one branch into another, with review and CI gates. |
| **Reflog** | Chronological log of all operations that change branch tips. |
| **Ceiling Model** | Access control model where parent levels set maximum permissions children cannot exceed. |
| **Stub Tree** | Tree that exists in metadata but whose files haven't been synced locally. |
| **FastCDC** | Content-defined chunking algorithm used for large file storage. |
| **SPDX** | Software Package Data Exchange — standard for license identifiers. |
| **Registered Path** | Explicitly declared path that can be targeted by access policies (no glob guessing). |

---

## License

This project is licensed under the **W0rkTree Public License v1.0** — a copyleft license based on the GNU GPL v2 with a single additional clause: **Brand Protection** (Section 11).

| Can I... | Answer |
|---|---|
| Use commercially? | **Yes** |
| Modify the source? | **Yes** |
| Distribute modified versions? | **Yes** — with attribution and source code |
| Use privately without limits? | **Yes** |
| Create plugins, extensions, integrations? | **Yes** — under any name |
| Build competing products from scratch? | **Yes** |
| Fork and contribute back? | **Yes** |
| Strip the W0rkTree name and rebrand? | **No** |
| Remove attribution? | **No** |
| Distribute without source code? | **No** |

The full license text is in [`LICENSE`](./LICENSE).

```
SPDX-License-Identifier: LicenseRef-W0rkTree-Public-License-1.0
```

---

<div align="center">

**W0rkTree is not the next version of Git. It is what comes after Git.**

*Built with Rust. Designed from first principles. Open source forever.*

</div>