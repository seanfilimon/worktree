# W0rktree — AI Agent Context & Rules

## Agent Rules and Directives

1. **Onboarding & Explanations:** The developer is new to this team and needs to quickly onboard, become familiar with the codebase, and contribute to all parts. **You must briefly explain to the developer all kinds of new concepts, design decisions, and architectural patterns that take part in this system as we come across them.** Be helpful, educational, and proactive in sharing knowledge about *how* and *why* W0rktree works the way it does.
2. **Consult the Specs:** The specs in `crates/worktree-protocol/specs/` are the authoritative ground truth for *what* and *how*. When in doubt, read the relevant spec before writing code or proposing solutions.
3. **Specs Before Code:** Non-trivial features require a spec update before implementation. Do not invent new protocol types or mechanisms without verifying they are specified.
4. **Adhere to Key Design Rules:** Enforce the architectural invariants of the system in all suggestions and implementations.
5. **Automatic Commit & Push:** Once a task is completed, confirmed functional, and documentation is updated, you must automatically commit and push the changes using descriptive commit messages.
6. **Dedicated Testing Directory (`.temp/`):** If testing is needed, always use a dedicated `.temp/` folder in the root of the workspace. Store all demo-related temporary files, test runners, scripts, databases, scenarios, and log files here. Always ensure that `.temp/` is added to `.gitignore`. **Crucially, automatically delete these temporary files as soon as testing/demoing is finished if they won't be needed in the future.**
7. **Continuous Documentation (`progress.md` & `demo.md`):** Automatically update `progress.md` to reflect the current state as we continuously develop the production-ready system. Additionally, always update `demo.md` to include new working scenarios that can be presented at a demo.

## What This Project Is

W0rktree is a from-scratch Git replacement written in Rust. Not a wrapper — an independent VCS with its own binary protocol, content-addressable storage, IAM system, and real-time collaboration primitives. It speaks Git only for migration and interoperability.

## Repository Layout

```text
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

```text
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

## Core Concepts & Architecture

### Snapshots, not commits
Immutable, content-addressed (BLAKE3) records of complete tree state. No staging area. No `add` command. History is **append-only** — no rebase, no force-push, no reset --hard, ever.

### Trees, not repositories
The fundamental unit of code organization. Each tree has independent snapshot history, branches, and access rules. Trees can be nested (like subdirectories, unlike submodules).

### Two-runtime architecture (hard constraint)
- **bgprocess** (local): watches files, creates snapshots, manages local DAG, syncs to server.
  - **NEVER enforces access control** — reads policies for display only.
  - **NEVER stores canonical history** — local store is a sync cache.
- **server** (remote): canonical history, IAM, staged visibility, branch protection.
  - **NEVER touches working directory**.
  - **NEVER creates snapshots from filesystem observation**.

### Staged snapshots & Syncing
The bgprocess auto-snapshots and syncs to the server synchronously. These are "staged" snapshots — visible to teammates (file paths, branch, timestamp) but NOT part of branch history. `wt push` finalizes them into history. This allows team visibility before final commits.

### IAM and the ceiling model
Access control is defined in TOML files alongside code (`.wt/access/`, `.wt-tree/access/`).
Scope hierarchy: `Global → Tenant → Tree → Branch → RegisteredPath`.
Parent levels set **maximum** permissions — children can only restrict, never expand.
Deny beats Allow at the same scope level. Default is Deny.

### Snapshot-based conflict resolution (PR merges)
Instead of replaying commits one by one, W0rktree groups related snapshots by session (time window + author), diffs each group against the target branch tip, and surfaces conflicts at the group level.

## Key Design Rules (Never Violate)

1. **History is append-only.** No operation should delete or rewrite existing snapshots.
2. **bgprocess never enforces access control.** All enforcement is server-side.
3. **Server never touches the working directory.** All file operations are bgprocess-only.
4. **New types go in `worktree-protocol` first.** Other crates import; they don't define protocol types.
5. **One command, one job.** CLI commands do exactly one thing. No overloaded flags.
6. **Deny beats allow at the same scope.** The IAM engine must maintain this invariant.
7. **Ceiling model is inviolable.** Tree-level policies can only restrict, never expand root.
8. **Auto-snapshots sync synchronously.** bgprocess must push staged snapshot to server immediately after local creation. Fire-and-forget is not acceptable.
9. **Server storage is canonical.** The server's object store is the source of truth. Local `.wt/` is a cache. They must never be the same file.

## Code Conventions

### Rust
- Rust 2021 edition throughout.
- `cargo fmt` and `cargo clippy -D warnings` must pass — no exceptions.
- Error types live in `worktree-protocol::core::error` — use those, don't invent local ones.
- All content addressing uses BLAKE3 via `worktree-protocol::core::hash`.
- All IDs are typed UUIDs from `worktree-protocol::core::id` — never raw strings or u64s.
- Async runtime is Tokio; the bgprocess runs a single Tokio runtime with cooperating tasks.
- Serialization: Bincode for wire protocol, Serde+JSON for REST API, TOML for config.

### Production standards
- No `unwrap()` / `expect()` in non-test code — propagate errors properly.
- No shared mutable state without explicit locking.
- All network-facing code must handle timeouts and partial reads.
- Secrets (tokens, keys) never logged, never in error messages.

## Build and Test

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Full CI pipeline
bash scripts/ci.sh
```

## Config File Locations (Runtime)

| Platform | Path |
|---|---|
| Linux | `~/.local/share/w0rktree/` |
| macOS | `~/Library/Application Support/W0rkTree/` |
| Windows | `%APPDATA%\W0rkTree\` |

IPC: Unix domain sockets on Linux/macOS (`$XDG_RUNTIME_DIR/w0rktree/worker.sock`), named pipes on Windows (`\\.\pipe\w0rktree-worker`).

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