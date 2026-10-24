<div align="center">

# W0rkTree

### **The version control system that comes after Git.**

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org)
[![Go](https://img.shields.io/badge/go-1.25-00ADD8.svg)](https://go.dev)
[![TypeScript](https://img.shields.io/badge/typescript-React%2019-blue.svg)](https://www.typescriptlang.org)
[![License](https://img.shields.io/badge/license-W0rkTree%20Public%20License%20v1.0-blue.svg)](#license)
[![Status](https://img.shields.io/badge/status-alpha%20(v2)-yellow.svg)](#implementation-status)

W0rkTree is an independent version control system — not a Git wrapper, not a hosting layer.
It has its own object model, sync protocol, multi-tenant server, access control, license
enforcement and real-time "staged" visibility of work in progress.

It speaks Git only for migration and interoperability (`wt git import` / `wt git export`).

[Architecture](#architecture) · [Quickstart](#quickstart) · [Docs](./docs/README.md) · [CLI Reference](./docs/cli-reference.md) · [Status](#implementation-status) · [Contributing](./CONTRIBUTING.md)

</div>

---

## Why W0rkTree exists

Git was designed in 2005 to track the Linux kernel. Two decades later almost every team uses it —
not because it solves their problems, but because nothing better existed. Many of Git's pain points
are design decisions, not bugs:

| Problem | Git | W0rkTree |
|---|---|---|
| **No organization model** | A repo is a bag of files; teams and ownership live on a hosting platform. | Tenants (users and organizations), memberships and roles are part of the server. |
| **Work is invisible until push** | Nobody sees what anyone is doing until it lands. | **Staged snapshots**: work in progress is uploaded and visible to the team without entering branch history. |
| **Destructive by design** | `rebase`, `reset --hard`, `push --force`. | Append-only history. No rebase, no force-push. Branches are archived, not deleted. `wt revert` adds a snapshot. |
| **No access control** | None built in. | Role-based authorization per tenant, tree visibility (`public`/`internal`/`private`), branch protection and merge requests — enforced by the server. |
| **No license enforcement** | Honor system. | Per-path SPDX rules with grants (`read-only`, `modify`, `redistribute`) enforced on push. |
| **Overloaded commands** | `checkout` does three things; `reset` has several modes. | One job per command: `snapshot`, `switch`, `restore`, `revert`, `merge`. |
| **Submodules** | A separate, fragile system. | **Nested trees**: independently versioned sub-projects inside one worktree. |
| **Secrets slip in** | Only external hooks. | Built-in secret scanning before every snapshot. |

### Git vs W0rkTree

Rows marked *Roadmap* are specified in [`crates/worktree-protocol/specs/`](./crates/worktree-protocol/specs/) but not part of the current v2 implementation.

| Aspect | Git | W0rkTree (v2) |
|---|---|---|
| Architecture | Local tool + separate hosting | CLI/SDK + optional background daemon + W0rkTree server |
| Unit of history | Repository | Tree (a worktree can hold several nested trees) |
| Terminology | commit, checkout, stash | snapshot, switch, restore, stage |
| Staging area | `git add` index | None — a snapshot records the working state of the tree |
| History | Rewritable | Append-only; snapshots are immutable, ids are UUIDs, content is BLAKE3-addressed |
| Merge | merge, rebase, cherry-pick, squash | Three-way merge only; diff3 markers plus machine-readable `.wt/conflicts/<tree>.json` |
| Collaboration | Invisible until push | Staged snapshots (`wt stage`, `wt status --team`), live server events over SSE |
| Identity | Unverified name + email | Server accounts, argon2id passwords, `wt_` bearer tokens |
| Access control | None | Built-in role matrix (viewer → owner), visibility, branch protection, merge requests |
| Declarative policies (RBAC + ABAC in TOML) | None | *Roadmap* (access-config documents are stored but not evaluated) |
| License tracking | None | Per-path SPDX rules enforced on push |
| Large files | Git LFS | Stored as whole blobs today; FastCDC chunking and lazy loading are *Roadmap* |
| Auto-snapshots | None | `worktree-bg` daemon snapshots on inactivity and stages work automatically |
| Submodules | Separate system | Nested trees with independent branches |
| Recovery | Local reflog that expires | Local append-only reflog; server-synced reflog is *Roadmap* |
| Monitoring | None | `/healthz`, Prometheus `/metrics`, append-only audit log, admin panel |
| Transport security | SSH/HTTPS | Cleartext h2c on one port — terminate TLS in front of the server; native TLS/QUIC and Ed25519 signing are *Roadmap* |

---

## Architecture

```
                      ┌──────────────────────── developer machine ───────────────────────┐
                      │                                                                  │
                      │   wt (worktree-cli)        worktree-bg (daemon)                  │
                      │        │   │  IPC: JSON over 127.0.0.1 TCP  │                    │
                      │        │   └───────────────────────────────►│                    │
                      │        ▼                                    ▼                    │
                      │   ┌──────────────────── worktree-sdk ─────────────────────┐      │
                      │   │ Repository (.wt/ on disk) · diff · merge · scan       │      │
                      │   │ remote (gRPC h2c + REST client) · ipc                  │      │
                      │   └───────────────────────────┬────────────────────────────┘      │
                      │   worktree-git (Git import/export over the SDK)                   │
                      └───────────────────────────────┼──────────────────────────────────┘
                                                      │ http://host:7420
                                                      │ gRPC worktree.sync.v1.SyncService
                                                      │ + REST /api/v1 + SSE
                      ┌───────────────────────────────▼──────────────────────────────────┐
                      │  worktree-server (Go, services/server-go) — one port, default :7420│
                      │  iam · syncsvc · httpapi · staged · review · license · audit ·    │
                      │  events · meta (bbolt: meta.db) · objstore (CAS: objects/)        │
                      └───────────────────────────────▲──────────────────────────────────┘
                                                      │ REST + SSE, served at /admin/
                                              apps/admin (React admin panel)
```

* **`wt`** is the command-line interface. All logic lives in **`worktree-sdk`**, which other Rust
  programs can embed.
* **`worktree-bg`** is an optional per-user daemon: it watches worktrees, takes auto-snapshots
  and uploads them as staged snapshots.
* **`worktree-server`** (Go) is the source of truth for shared history. gRPC (HTTP/2 cleartext),
  REST, Server-Sent Events, `/metrics` and the static admin panel share a single listener.
* **`apps/admin`** is the web admin panel; **`apps/web`** is the marketing and docs site.

Hard boundaries:

- The daemon never enforces access control; the server does.
- The server never touches a working directory or creates snapshots on its own (merge requests
  fast-forward an existing branch tip).
- Objects are byte-identical on disk, on the wire and on the server, so push and pull are byte copies.

More detail: [server architecture](./docs/server-architecture.md), [sync protocol](./docs/protocol-spec.md),
[daemon](./docs/daemon.md), [storage format](./docs/storage-format.md).

---

## Repository layout

```
.
├── crates/                    Rust workspace
│   ├── worktree-protocol/     Wire contract (proto/sync.proto), hashing, ids, specs/
│   ├── worktree-sdk/          Local engine: Repository, object store, diff, merge, scan, remote, ipc
│   ├── worktree-cli/          The `wt` binary
│   ├── worktree-bg/           The `worktree-bg` background daemon
│   ├── worktree-git/          Git import/export bridge (libgit2)
│   └── worktree-e2e/          End-to-end tests driving real wt / worktree-bg / server binaries
├── services/
│   └── server-go/             The W0rkTree server (Go 1.25): gRPC + REST + SSE + admin static files
├── apps/
│   ├── admin/                 Admin panel (Vite + React 19 + TypeScript + Tailwind v4)
│   └── web/                   Marketing + documentation site (Next.js)
├── docs/                      User and contributor documentation
├── scripts/                   ci.sh, e2e.sh, gen-proto.sh, installers
├── Cargo.toml                 Rust workspace root
└── package.json / turbo.json  npm workspaces + Turborepo
```

The previous Rust server crate (`crates/worktree-server`) was removed in favour of the Go server, and
the Yew admin crate (`crates/worktree-admin`) is replaced by `apps/admin`.

---

## Quickstart

Build from source. Requirements: Rust (stable), Go 1.25+, Node.js 22+ (only for the admin panel).
`protoc` is vendored by the build; you do not need to install it. Full setup per OS is in
[CONTRIBUTING.md](./CONTRIBUTING.md#development-setup).

### 1. Build

```bash
git clone https://github.com/seanfilimon/worktree.git
cd worktree

# Rust: wt, worktree-bg
cargo build --release

# Go: the server
(cd services/server-go && go build -o ../../target/release/worktree-server .)

# Optional: the admin panel (served by the server at /admin/)
(cd apps/admin && npm install && npm run build)

export PATH="$PWD/target/release:$PATH"
```

On Windows the binaries are `wt.exe`, `worktree-bg.exe` and `worktree-server.exe` (pass
`-o ../../target/release/worktree-server.exe` to `go build`).

### 2. Run the server

```bash
worktree-server --data ./wt-data --admin-password 'change-me' --admin-dir apps/admin/dist
# gRPC + REST + SSE on http://localhost:7420, admin panel on http://localhost:7420/admin/
```

`--admin-password` creates (or updates) the server admin account `admin`.

### 3. Create an account and log in

```bash
wt register http://localhost:7420 -u alice -e alice@example.com     # prompts for a password
# later, on another machine:
wt login http://localhost:7420 -u alice
wt whoami http://localhost:7420
```

For scripts, pass the password on stdin with `--password-stdin` or via `WT_PASSWORD`.
Registering creates a personal tenant whose slug is the username (`alice`).

### 4. Init → snapshot → push → clone

```bash
wt init demo && cd demo
echo "hello" > hello.txt
wt status
wt snapshot -m "first snapshot"
wt log --oneline

wt remote add origin http://localhost:7420 --tenant alice
wt push                                   # creates tree alice/demo on the server if missing

cd ..
wt clone http://localhost:7420 alice/demo demo-copy
cat demo-copy/hello.txt
```

Next: the [getting started guide](./docs/getting-started.md) covers branches, merges, nested trees,
staged snapshots and the daemon.

---

## Core concepts

**Worktree and trees.** A *worktree* is a directory with a `.wt/` folder. It contains a root tree and
any number of *nested trees* — subdirectories with a `.wt-tree/` marker. Each tree has its own
branches, snapshots, tags and reflog; a tree's snapshot excludes the paths of trees nested inside it.

**Snapshots, not commits.** A snapshot is an immutable record of a tree's working state: a manifest
hash, parents, author, message and timestamp. There is no staging area — `wt snapshot` captures the
tree as it is on disk, minus ignored paths. Snapshot ids are UUID v4 strings; file contents
(blobs) and manifests are content-addressed by BLAKE3-256.

**Append-only history.** Nothing rewrites history. `wt revert` creates a new snapshot that undoes an
old one, `wt branch archive` soft-deletes a branch (restorable), the server only accepts fast-forward
pushes, and merges create two-parent snapshots.

**Staged snapshots.** `wt stage` (or the daemon, automatically) uploads the current working state
to the server as a *staged snapshot*: visible to the team with `wt status --team` and in the admin
panel, but not part of any branch. There is at most one per (tree, account, branch); pushing the branch
clears it.

**Tenants, roles and visibility.** Every tree on the server belongs to a tenant (a user or an
organization). Members hold one of five roles — `viewer`, `developer`, `maintainer`, `admin`, `owner` —
and trees are `private`, `internal` (any signed-in account can read) or `public` (anonymous reads).

**Branch protection and merge requests.** Protection rules match branch names by glob and can forbid
direct pushes, require approvals and forbid deletion. A merge request is merged on the server by
fast-forwarding the target branch to the source branch's tip.

**License compliance.** Trees can assign SPDX licenses to path globs with a grant level that limits
what members of other tenants may do with those paths.

See the [glossary](./docs/README.md#glossary) for terminology.

---

## Implementation status

The v2 rebuild is **alpha**. The table reflects the v2 contract in
[docs/ENGINEERING_PLAN.md](./docs/ENGINEERING_PLAN.md); the end-to-end suite (`scripts/e2e.sh`) is the
acceptance gate for integration. Items under *Roadmap* exist only as specifications.

| Component | Path | Status | Scope |
|---|---|---|---|
| Wire protocol | `crates/worktree-protocol/proto/sync.proto` | v2 | `SyncService` (13 RPCs), object/snapshot records, push rejections |
| SDK — local engine | `crates/worktree-sdk` | v2 | `.wt/` layout, object store, snapshots, branches, tags, merge (diff3), revert, restore, reflog, ignore, nested trees |
| SDK — remote client | `crates/worktree-sdk/src/remote` | v2 | login/register, push, fetch, pull, clone, stage, list staged |
| SDK — scanning | `crates/worktree-sdk/src/scan.rs` | v2 | Secret scanning (pre-snapshot), SPDX detection, FastCDC boundaries (not yet used for storage) |
| CLI `wt` | `crates/worktree-cli` | v2 | 29 top-level commands, `--json` output, exit codes 0/1/2/3 |
| Daemon `worktree-bg` | `crates/worktree-bg` | v2 | File watching, debounced auto-snapshots, auto-stage, JSON/TCP IPC |
| Git bridge | `crates/worktree-git` | v2 | `wt git import` / `wt git export` with deterministic, incremental mapping |
| Server | `services/server-go` | v2 | bbolt metadata, CAS objects, IAM, sync, REST, SSE, staged, branch protection, merge requests, license rules, audit, metrics |
| Admin panel | `apps/admin` | v2 | Dashboard, tenants, trees, file browser, merge requests, audit log, accounts, tokens |
| End-to-end tests | `crates/worktree-e2e`, `scripts/e2e.sh` | v2 | Drives the real binaries against a live server |
| Marketing / docs site | `apps/web` | Existing | Next.js site |

### Roadmap (specified, not implemented)

- Declarative TOML access policies (RBAC + ABAC), custom roles and `.wt-tree/` permission ceilings
  ([IAM spec](./crates/worktree-protocol/specs/iam/IAM.md)).
- Large-file chunked storage and lazy loading via FUSE/ProjFS ([Storage spec](./crates/worktree-protocol/specs/storage/Storage.md)).
- QUIC transport, native TLS, Ed25519 snapshot signing, encryption at rest ([Security spec](./crates/worktree-protocol/specs/security/Security.md)).
- Tree/branch/snapshot dependencies and auto-generated TODO branches ([Tree spec](./crates/worktree-protocol/specs/tree/Tree.md)).
- Releases, CI check gates and tenant quotas (the `CiChecksFailed` / `QuotaExceeded` rejections are
  reserved in the protocol).
- Live Git mirroring and a Git remote helper.
- Server-synced reflog and offline sync queue.

---

## Development workflow

```bash
bash scripts/ci.sh           # full CI pipeline: Rust fmt/clippy/test, Go gofmt/vet/test, frontend builds
bash scripts/e2e.sh          # builds everything, starts the Go server, drives wt end-to-end
bash scripts/gen-proto.sh    # regenerate Go bindings after editing sync.proto
```

Individual checks:

```bash
cargo build --workspace && cargo test --workspace
(cd services/server-go && go build ./... && go vet ./... && go test ./...)
(cd apps/admin && npm run build && npm test)
(cd apps/web && npm run build)
```

Rust code is generated from `sync.proto` by `build.rs` with a vendored `protoc`. See
[CONTRIBUTING.md](./CONTRIBUTING.md) for coding standards, test conventions and the PR process.

---

## Documentation

| Document | Contents |
|---|---|
| [Getting started](./docs/getting-started.md) | Install, first worktree, branches, merges, remotes, team workflow |
| [CLI reference](./docs/cli-reference.md) | Every `wt` command, flag and exit code |
| [Configuration](./docs/configuration.md) | `.wt/config.toml`, `$WT_HOME`, `bg.toml`, server flags |
| [Storage format](./docs/storage-format.md) | `.wt/` layout, objects, refs, reflog |
| [Sync protocol](./docs/protocol-spec.md) | `SyncService`, hashing, push/pull/stage flows, rejections |
| [Server architecture](./docs/server-architecture.md) | Go packages, storage, IAM, REST API, events, audit, licenses |
| [Security](./docs/security.md) | Secret scanning, passwords, tokens, history guarantees |
| [Daemon](./docs/daemon.md) | `worktree-bg`, auto-snapshots, IPC |
| [SDK guide](./docs/sdk-guide.md) | Embedding `worktree-sdk` in Rust programs |
| [Admin panel](./docs/admin-panel.md) | Running and building `apps/admin` |
| [Git compatibility](./docs/git-compatibility.md) | Import/export semantics and limits |
| [Specifications](./crates/worktree-protocol/specs/README.md) | Long-form design specs (includes roadmap material) |

---

## Contributing

See **[CONTRIBUTING.md](./CONTRIBUTING.md)**. In short: CI must pass (`bash scripts/ci.sh`), every
change needs tests, and history is append-only — we merge with merge commits, not squash or rebase.

| Role | Name |
|---|---|
| **Founding Engineer** | **Sean Filimon** — [@seanfilimon](https://github.com/seanfilimon) |

---

## License

W0rkTree is licensed under the **W0rkTree Public License v1.0** — a copyleft license based on the
GNU GPL v2 with one additional clause: **Brand Protection** (Section 11).

| Can I... | Answer |
|---|---|
| Use commercially? | **Yes** |
| Modify the source? | **Yes** |
| Distribute modified versions? | **Yes** — with attribution and source code |
| Use privately without limits? | **Yes** |
| Create plugins, extensions, integrations? | **Yes** — under any name |
| Fork and contribute back? | **Yes** |
| Strip the W0rkTree name and rebrand? | **No** |
| Remove attribution? | **No** |
| Distribute without source code? | **No** |

The full text is in [`LICENSE`](./LICENSE).

```
SPDX-License-Identifier: LicenseRef-W0rkTree-Public-License-1.0
```

---

<div align="center">

**W0rkTree is not the next version of Git. It is what comes after Git.**

</div>
