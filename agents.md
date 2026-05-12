# W0rktree — Claude Code Context

## What This Project Is

W0rktree is a from-scratch Git replacement written in Rust. Not a wrapper — an independent VCS
with its own binary protocol, content-addressable storage, IAM system, and real-time collaboration
primitives. It speaks Git only for migration and interoperability.

The blog posts in the repo explain the *why*. The specs in `crates/worktree-protocol/specs/` are
the authoritative *what* and *how*. When in doubt, read the spec.

---

## Repository Layout

```
worktree/
├── crates/                        # Rust workspace
│   ├── worktree-protocol/         # Foundation — every other crate depends on this
│   │   ├── src/
│   │   │   ├── core/              # BLAKE3 hashing, typed UUIDs, error types
│   │   │   ├── object/            # 12 object types (blob, tree, snapshot, branch, ...)
│   │   │   ├── iam/               # Roles, permissions, scopes, RBAC + ABAC engine
│   │   │   ├── access/            # Resource targeting, tree/branch ACLs
│   │   │   ├── config/            # Config hierarchy model
│   │   │   └── feature/           # Diff, merge, wire protocol, Git compat, licensing, sync
│   │   └── specs/                 # 14 authoritative specification documents
│   ├── worktree-sdk/              # Local engine: snapshots, branches, diffs, merges
│   ├── worktree-server/           # Background daemon: watcher, auto-snapshot, sync, gRPC
│   ├── worktree-cli/              # `wt` binary — 20 subcommands
│   ├── worktree-git/              # Git bridge: SHA-1 ↔ BLAKE3, import, export
│   └── worktree-admin/            # Admin panel: Yew WASM SPA + Axum HTTP API
├── apps/
│   └── web/                       # Marketing/docs site (Next.js 16, Fumadocs)
├── docs/                          # User-facing documentation
├── tests/                         # Cross-crate integration + e2e tests
└── scripts/
    ├── ci.sh                      # Full CI pipeline (fmt → clippy → test → build)
    ├── install.sh                 # Unix installer
    └── install.ps1                # Windows installer
```

### Crate Dependency Order

```
worktree-protocol          ← touch this first when adding new types
    ↑
    ├── worktree-sdk       ← local engine, no network
    │       ↑
    │       └── worktree-cli
    │
    ├── worktree-git       ← Git bridge (libgit2)
    │       ↑
    │       └── worktree-server
    │
    └── worktree-admin     ← Yew WASM + Axum
```

New types always go in `worktree-protocol` first. Other crates import from there.

---

## Core Concepts

### Snapshots, not commits
Immutable, content-addressed (BLAKE3) records of complete tree state. No staging area. No `add`
command. The bgprocess watches the filesystem and creates snapshots automatically. History is
**append-only** — no rebase, no force-push, no reset --hard, ever.

### Trees, not repositories
Fundamental unit of code organization. Each tree has independent snapshot history, branches, and
access rules. Trees can be nested (like subdirectories, unlike submodules).

### Staged snapshots
The team visibility layer. Snapshots sync to the server as "staged" — visible to teammates (file
paths, branch, timestamp) but NOT part of branch history. `wt push` finalizes them into history.
This is a protocol-level feature, not a platform add-on.

### Two-runtime architecture (hard constraint)
- **bgprocess** (local): watches files, creates snapshots, manages local DAG, syncs to server
  - **NEVER enforces access control** — reads policies for display only
  - **NEVER stores canonical history** — local store is a sync cache
- **server** (remote): canonical history, IAM, staged visibility, branch protection
  - **NEVER touches working directory**
  - **NEVER creates snapshots from filesystem observation**

This separation is an architectural invariant enforced throughout the codebase. Do not blur it.

### IAM and the ceiling model
Access control is defined in TOML files alongside code (`.wt/access/`, `.wt-tree/access/`).
Scope hierarchy: `Global → Tenant → Tree → Branch → RegisteredPath`.
Parent levels set **maximum** permissions — children can only restrict, never expand.
Deny beats Allow at the same scope level. Default is Deny.

---

## Product Goal (current work)

**Goal:** Fully working product in this workspace. Not a demo, not a PR — a shippable system
that real users can run. No artificial constraints on scope.

### How the tool works when a user activates it (from lead founder engineer)

1. **Creates `.wt/` folder** in the target directory (tree initialization)
2. **Spawns bgprocess** — watches directory for file changes
3. **Validates permissions** — IAM write-access check against policies
4. **Begins syncing** — bgprocess auto-snapshots and syncs to server synchronously

Auto-snapshots must sync to server immediately after creation (not fire-and-forget).
Staged snapshots are visible to teammates before `wt push` finalizes them into branch history.

### Snapshot-based conflict resolution (PR merges)

When merge requests arrive with conflicts, W0rktree does NOT replay commits one by one.
Instead:
1. Load all snapshots on source branch
2. Group related snapshots by session (time window + author)
3. Diff each group against target branch tip
4. Surface conflicts at group level
5. Resolve conflicts → push combined snapshot chain

This is the core differentiator from Git. Implementation requires spec update to
`specs/server/Server.md` merge request section before coding.

### Production targets (no longer out of scope)

- Real IAM enforcement on all server endpoints
- TLS on server (rustls)
- Separate server-side canonical storage (not shared SDK state.json)
- WebSocket push for staged snapshot visibility
- Snapshot signing (Ed25519)
- Offline queue + reconnect in bgprocess
- QUIC transport (wire protocol)
- Multi-tenant routing
- Audit logging pipeline

### Current implementation status

| Component | Status |
|-----------|--------|
| Object storage (`disk.rs`) | ✅ Working |
| Event classification (`event.rs`) | ✅ Working |
| Auto-commit engine (`auto_commit.rs`) | ✅ Working |
| API handlers (`handlers.rs`) | ✅ Working |
| HTTP server (`lib.rs`) | ✅ Working — Axum on 9876, watcher loop calls `create_snapshot()` |
| CLI server spawn/kill (`server.rs`) | ✅ Working |
| SDK HTTP push/pull (`sync.rs`) | ✅ Working |
| Auto-snapshot → immediate server sync | ✅ Working — bgprocess pushes staged snapshots |
| Server-side canonical storage | ✅ Working — independent ServerStateStore used |
| IAM enforcement on endpoints | ❌ Missing — all endpoints open |
| TLS | ❌ Missing |
| Staged snapshot visibility | ❌ Missing |
| Snapshot-based merge/conflict resolution | ❌ Missing — spec needed first |
| WebSocket streaming | ❌ Missing |

---

## Implementation Status

### ✅ Complete — do not rewrite unless fixing a bug
- `worktree-protocol`: all object types, IAM engine, config hierarchy, wire format
- `worktree-sdk`: init, snapshot, branch CRUD, tree CRUD, diff, merge, tag, status, reflog
- `worktree-cli`: 20 commands, colored output, config management
- `worktree-git`: hash index, gitattributes parser, repo wrapper, commit walker, submodule
  discovery, repo builder, transport, auth
- `worktree-server`: filesystem watcher, debouncer, session auth, permission enforcer, object
  index, health tracker, transport layer, rules engine
- `worktree-admin`: 8 Yew components, routing, CSS system, Axum API (10 endpoints), auth
  middleware, error handling

### 🔶 Stubbed or partial — next implementation targets
- `worktree-server/src/lib.rs` — bgprocess creates snapshot locally but doesn't push to server synchronously
- `worktree-server/` — no canonical server-side storage; handlers call SDK which reads/writes local state.json
- `worktree-server/src/auth/` — session auth exists but endpoints not gated by IAM
- `worktree-git/src/import/converter.rs` — API defined, Git→W0rktree conversion not wired
- `worktree-git/src/export/converter.rs` — API defined, W0rktree→Git conversion not wired
- `worktree-git/src/remote/push.rs` / `pull.rs` — network operations are stubs
- License compliance types defined, SPDX validation not implemented

### 📋 Not started — production requirements
- Synchronous auto-snapshot → server push (bgprocess uploads staged snapshot immediately after creation)
- Server-side canonical object store independent of local SDK
- IAM enforcement on all server endpoints (use existing `worktree-server/src/auth/enforcer.rs`)
- TLS via rustls
- Snapshot-based merge conflict resolution (spec `specs/server/Server.md` first)
- WebSocket push for staged snapshot visibility (`specs/visibility/StagedVisibility.md`)
- Snapshot signing (Ed25519) and verification
- QUIC transport (`specs/sync/Sync.md`)
- Offline queue and reconnection logic in bgprocess
- Multi-tenant routing on server
- Secret scanning engine (pre-snapshot regex hook)
- Audit logging pipeline
- Admin panel wired to real server data (currently mock)
- Archive/export with license compliance filtering
- Shell completions (bash, zsh, fish, PowerShell)

---

## Build and Test

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Full CI pipeline (run this before every PR)
bash scripts/ci.sh

# Build just the CLI
cargo build -p worktree-cli

# Run the admin panel (requires trunk)
cargo install trunk
rustup target add wasm32-unknown-unknown
cd crates/worktree-admin && trunk serve

# Run the docs site
cd apps/web && npm install && npm run dev
```

CI runs: `cargo fmt --check` → `cargo clippy -D warnings` → `cargo test --workspace` →
`cargo build --release`. All four must pass.

---

## Specifications

The specs in `crates/worktree-protocol/specs/` are the ground truth. Before implementing
anything non-trivial, read the relevant spec. Before adding a new feature, the spec comes first.

| Spec | Path | What It Covers |
|---|---|---|
| Protocol Overview | `specs/README.md` | Architecture, terminology, Git comparison |
| Full system | `specs/WorkTree.md` | Trees, snapshots, tenants, dependencies, everything |
| Tree | `specs/tree/Tree.md` | Trees, branches, nesting, linked branches |
| BGProcess | `specs/bgprocess/BgProcess.md` | Local daemon: watcher, auto-snapshot, IPC |
| Server | `specs/server/Server.md` | Remote: IAM, branch protection, merge requests |
| IAM | `specs/iam/IAM.md` | Roles, permissions, RBAC + ABAC |
| Declarative Access | `specs/iam/DeclarativeAccess.md` | Policy authoring, ceiling model, examples |
| Tenant Model | `specs/iam/TenantModel.md` | Tenant types, cross-tenant access |
| Staged Visibility | `specs/visibility/StagedVisibility.md` | Staged snapshot pipeline, privacy controls |
| Sync Protocol | `specs/sync/Sync.md` | Delta sync, push/pull, wire format |
| Storage | `specs/storage/Storage.md` | Objects, BLAKE3, FastCDC, pack files, GC |
| `.wt/` Directory | `specs/dot-wt/DotWt.md` | Root config layout |
| `.wt-tree/` Directory | `specs/dot-wt-tree/DotWtTree.md` | Per-tree config, authority model |
| License Compliance | `specs/licensing/LicenseCompliance.md` | SPDX, grant model, enforcement |
| Security | `specs/security/Security.md` | Transport, auth, signing, audit, threat model |

---

## Code Conventions

### Rust
- Rust 2021 edition throughout
- `cargo fmt` and `cargo clippy -D warnings` must pass — no exceptions
- Error types live in `worktree-protocol::core::error` — use those, don't invent local ones
- All content addressing uses BLAKE3 via `worktree-protocol::core::hash`
- All IDs are typed UUIDs from `worktree-protocol::core::id` — never raw strings or u64s
- Async runtime is Tokio; the bgprocess runs a single Tokio runtime with cooperating tasks
- Serialization: Bincode for wire protocol, Serde+JSON for REST API, TOML for config
- `worktree-git` contains local re-implementations of some protocol types (e.g. `InMemoryHashIndex`
  in `hash_index/store.rs`). When `worktree-protocol` traits or structs change, check that these
  local impls stay in sync — the compiler will flag mismatches but won't auto-fix them.

### Tests
- Every feature needs tests — unit tests alongside the code, integration tests in `tests/`
- Cross-crate tests go in `tests/protocol_tests/`, `tests/server_tests/`, etc.
- `tests/e2e_tests/` for full CLI → server round trips
- Production code: test error paths and edge cases, not just happy path

### Commits
- Conventional commits: `feat(protocol): add StagedSnapshot expiry field`
- Scope is the crate name: `protocol`, `sdk`, `server`, `cli`, `git`, `admin`, `web`
- Merge commits only — no squash, no rebase (the project practices what it preaches)

### Production standards
- No `unwrap()` / `expect()` in non-test code — propagate errors properly
- No shared mutable state without explicit locking
- All network-facing code must handle timeouts and partial reads
- Secrets (tokens, keys) never logged, never in error messages
- `cargo clippy -D warnings` must pass before any feature is considered done

---

## Config File Locations (Runtime)

| Platform | Path |
|---|---|
| Linux | `~/.local/share/w0rktree/` |
| macOS | `~/Library/Application Support/W0rkTree/` |
| Windows | `%APPDATA%\W0rkTree\` |

The storage directory is never inside the working directory (unlike `.git/`).

IPC: Unix domain sockets on Linux/macOS (`$XDG_RUNTIME_DIR/w0rktree/worker.sock`),
named pipes on Windows (`\\.\pipe\w0rktree-worker`).

---

## Key Design Rules (Never Violate)

1. **History is append-only.** No operation should delete or rewrite existing snapshots.
2. **bgprocess never enforces access control.** All enforcement is server-side.
3. **Server never touches the working directory.** All file operations are bgprocess-only.
4. **New types go in `worktree-protocol` first.** Other crates import; they don't define protocol types.
5. **Specs before code.** Non-trivial features need a spec update before implementation.
6. **One command, one job.** CLI commands do exactly one thing. No overloaded flags.
7. **Deny beats allow at the same scope.** The IAM engine must maintain this invariant.
8. **Ceiling model is inviolable.** Tree-level policies can only restrict, never expand root.
9. **Auto-snapshots sync synchronously.** bgprocess must push staged snapshot to server immediately after local creation. Fire-and-forget is not acceptable.
10. **Server storage is canonical.** The server's object store is the source of truth. Local `.wt/` is a cache. They must never be the same file.

---

## Glossary

| Term | Meaning |
|---|---|
| **Tree** | Fundamental code unit with independent history, branches, access rules |
| **Snapshot** | Immutable content-addressed record of complete tree state |
| **Staged snapshot** | Snapshot visible to team but not yet in branch history |
| **Branch** | Named pointer to a snapshot chain within a tree |
| **Linked branch** | Branches across trees that must merge together |
| **Tenant** | Verified user or organization on the server |
| **BGProcess** | Local background daemon (`worktree-bgprocess` / `worktree-worker`) |
| **Ceiling model** | Parent permission levels are maximums; children only restrict |
| **Registered path** | Explicitly declared path targetable by access policies |
| **FastCDC** | Content-defined chunking algorithm used for large files |
| **SPDX** | Software Package Data Exchange — license identifier standard |