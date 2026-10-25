# W0rkTree Protocol Specifications

W0rkTree is a version control system with its own object model, wire protocol,
multi-tenant server and identity system. Git is supported through an
import/export bridge, not as a foundation. These specifications describe the
system **as built** after v0.5 (`docs/ENGINEERING_PLAN.md` §1-§13). Anything a
spec describes that is not built is listed in that spec's **Roadmap**
section.

---

## Status Legend

Every spec ends with three sections:

- **Implementation status** — a table of features with one of:
  - **Built** — implemented on `main` and covered by tests; the spec text
    describes the behaviour exactly. "Built (v0.5 §13.x)" marks features
    delivered by the v0.5 workstreams.
  - **Partial** — implemented with documented gaps (the gaps are listed in
    the row or in the Roadmap).
  - **Roadmap** — described but not built.
- **Roadmap** — features described in earlier drafts (or intended) that are
  neither built nor part of v0.5.
- **References** — source files that implement the spec.

When a spec and the code disagree, the code (and `docs/ENGINEERING_PLAN.md`)
is authoritative; the spec is a bug.

---

## Specification Index

| Spec | Path | Summary |
|---|---|---|
| **Core** | [`WorkTree.md`](./WorkTree.md) | Concepts (worktree, tree, snapshot, manifest, staged snapshot, tenant), architecture, config hierarchy, and every user-facing feature in one place |
| **Trees** | [`tree/Tree.md`](./tree/Tree.md) | Tree registry and nesting, per-tree branches, snapshots and their creation, staging, merges, dependencies/TODOs, tags, reflog |
| **`.wt/` directory** | [`dot-wt/DotWt.md`](./dot-wt/DotWt.md) | On-disk layout of a worktree, full `config.toml` reference, ignore rules, objects, hooks, `$WT_HOME` files |
| **`.wt-tree/` directory** | [`dot-wt-tree/DotWtTree.md`](./dot-wt-tree/DotWtTree.md) | Nested-tree marker, tree-level config overrides, ignore, hooks, tree-scope deny policies |
| **Background daemon** | [`bgprocess/BgProcess.md`](./bgprocess/BgProcess.md) | `worktree-bg`: watching, debounced auto-snapshots, auto-staging, background fetch, maintenance, NDJSON/TCP IPC |
| **Server** | [`server/Server.md`](./server/Server.md) | Go server: one port for gRPC and REST, storage, services, operations (TLS, GC, quotas, rate limits, metrics) |
| **Sync protocol** | [`sync/Sync.md`](./sync/Sync.md) | `worktree.sync.v1.SyncService`: push, pull, negotiate, object transfer, staging, tags, branch watch, access config |
| **Storage** | [`storage/Storage.md`](./storage/Storage.md) | BLAKE3 content addressing, blobs, flat manifests, FastCDC chunk lists, local and server object stores, GC |
| **IAM** | [`iam/IAM.md`](./iam/IAM.md) | Authentication (sessions, tokens, MFA, SSO, service accounts) and authorization (roles, grants, policies, evaluation order) |
| **Declarative access** | [`iam/DeclarativeAccess.md`](./iam/DeclarativeAccess.md) | `.wt/access/*.toml` and `.wt-tree/access/policies.toml`: format, validation, `wt access plan/apply/pull` |
| **Tenant model** | [`iam/TenantModel.md`](./iam/TenantModel.md) | Accounts, personal and organization tenants, members, teams, invitations, tenant status and limits, cross-tenant access |
| **Staged visibility** | [`visibility/StagedVisibility.md`](./visibility/StagedVisibility.md) | Staged snapshots: lifecycle, who sees them, `wt status --team`, events, retention |
| **License compliance** | [`licensing/LicenseCompliance.md`](./licensing/LicenseCompliance.md) | Per-path SPDX rules and grants, enforcement on push/pull/read/export, NOTICE generation, detection |
| **Security** | [`security/Security.md`](./security/Security.md) | Threat model, transport, credentials, snapshot signing, secret scanning, audit, tenant isolation |
| **Blame** | [`tree/Blame.md`](./tree/Blame.md) | Line attribution: first-parent-first walk with merge handling, `wt blame`, `GET {tree}/blame/{rev}/{path...}` with read checks |
| **Policy history** | [`iam/PolicyHistory.md`](./iam/PolicyHistory.md) | Policy versions on every REST and access-apply change, statement-level diff, rollback and restore, `wt policy history/diff/rollback` |
| **Webhooks** | [`server/Webhooks.md`](./server/Webhooks.md) | Tenant and tree webhooks: event patterns, signed JSON deliveries, retries, deactivation, delivery log, SSRF guard, `wt webhook` |
| **Code search** | [`server/CodeSearch.md`](./server/CodeSearch.md) | Trigram index of default branches, literal/regex search, permission filtering, `GET /search/code`, `wt search code` |
| **CLI ergonomics** | [`cli/CliErgonomics.md`](./cli/CliErgonomics.md) | Shell completions, `[alias]` expansion rules, `wt doctor` checks, `wt open` admin-panel URLs, grouped help |
| **History rewriting** | [`tree/HistoryRewriting.md`](./tree/HistoryRewriting.md) | `wt cherry-pick`, `wt rebase` (pushed-history guard, conflicts via `wt merge resolve`), `wt stash`, auto-stash on branch switch |
| **REST API** | [`server/RestApi.md`](./server/RestApi.md) | `/api/v1` conventions: auth, errors, pagination, versioning, the OpenAPI document (`openapi.yaml`) and `wt api` |
| **Server-side merges** | [`server/Merging.md`](./server/Merging.md) | Merge methods (fast-forward, merge, squash), protection restrictions, three-way merge, mergeability, conflicts |
| **Compression and integrity** | [`storage/Compression.md`](./storage/Compression.md) | WTZ1 zstd object encoding, gRPC gzip, `wt gc`, `wt fsck`, server fsck |
| **Deployment and release** | [`server/Deployment.md`](./server/Deployment.md) | Release archives and SHA256SUMS, installers, container images, Compose profiles, Helm chart, systemd/launchd units, proxies, CI and release workflows |
| **Sparse checkout** | [`tree/SparseCheckout.md`](./tree/SparseCheckout.md) | `wt sparse` patterns, working-tree semantics, partial clones (`wt clone --sparse`), on-demand downloads, gc/fsck |

Suggested reading order: this README → `WorkTree.md` → `tree/Tree.md` →
`dot-wt/DotWt.md` → `dot-wt-tree/DotWtTree.md` → `bgprocess/BgProcess.md` →
`sync/Sync.md` → `storage/Storage.md` → `server/Server.md` → the IAM,
visibility, licensing and security specs.

---

## Why W0rkTree Exists

Git's limits are design decisions, not bugs:

- **No organization model.** Repositories have no tenants, teams or owners;
  hosting platforms bolt them on.
- **No built-in access control.** Path- and branch-level rules require
  external tooling.
- **Rewritable history.** Rebase, reset and force push make data loss one
  command away.
- **Invisible work in progress.** Nothing is shared until someone pushes.
- **Large files need a second system** (Git LFS).
- **No license tracking** per file.

W0rkTree addresses these in the protocol: a multi-tenant server with IAM,
nested independently versioned trees, append-only shared history, staged
snapshots, automatic chunking of large files and server-enforced per-path
license rules.

---

## Core Terminology

| W0rkTree | Closest Git term | Meaning |
|---|---|---|
| **Worktree** | working copy + `.git` | A directory with a `.wt/` metadata directory; holds one or more trees |
| **Tree** | repository | An independently versioned set of files with its own branches, tags and reflog |
| **Snapshot** | commit | Immutable record of a tree's full file state (UUID id, flat manifest) |
| **Manifest** | tree object | Sorted list of `{path, hash, size, executable, chunked}` |
| **Branch** | branch | Per-tree pointer to a snapshot; deleted by archiving (soft) |
| **Tag / Release** | tag / (hosting feature) | Named snapshot / server object with notes and assets |
| **Staged snapshot** | — | Work in progress uploaded for team visibility, not branch history |
| **Tenant** | — | Owner namespace on the server: a user's personal tenant or an organization |
| **Push / Fetch / Pull** | push / fetch / pull | Transfer between a local tree and its server tree; push must fast-forward |

Paths: `.wt/` — worktree metadata (one per worktree, at the root);
`.wt-tree/` — marker and local config of a nested tree. Neither is ever
snapshotted.

There is no staging area or `add` command, no rebase, no reset and no force
push.

---

## Architecture

```
 wt (CLI, Rust) ──► worktree-sdk (Rust) ──► .wt/ on disk
      │                  │  remote: gRPC (h2c or TLS) + REST on one port (default 7420)
      │ IPC: NDJSON/TCP  ▼
 worktree-bg (optional daemon) ──► worktree-server (Go, services/server-go) ──► bbolt meta.db + objects/
                                        ▲ REST + SSE
                                  apps/admin (React admin panel at /admin/)
 worktree-git (Rust) — Git import/export/push over worktree-sdk
```

- **CLI + SDK** do all local version control in-process; the daemon is
  optional automation (auto-snapshot, auto-stage, background fetch).
- **The server** is the authority for shared branch tips and enforces IAM,
  branch protection, license rules, path policies, signatures and quotas on
  every call. It never touches working directories.
- **Wire contract:** `crates/worktree-protocol/proto/sync.proto`
  (`worktree.sync.v1.SyncService`); objects are BLAKE3-addressed byte strings
  identical locally, on the wire and on the server.

---

## Principles

1. **One job per command** — `wt switch` switches, `wt restore` restores,
   `wt branch create` creates.
2. **Plain terminology** — snapshot, tree, stage.
3. **Automatic when wanted** — the daemon snapshots and stages; everything
   also works by hand.
4. **Append-only shared history** — the server only accepts pushes that
   descend from its tip; fixes are new snapshots (`wt revert`).
5. **Soft deletes** — branches are archived; unreachable server objects are
   collected only after a grace period.
6. **Server-side enforcement** — clients are never trusted for access,
   protection, license or quota decisions.
7. **Git as a bridge** — import, export and push to Git remotes.

---

## Getting Started

```bash
wt init demo && cd demo
echo hello > README.md
wt snapshot -m "First snapshot"
wt remote add origin http://localhost:7420 --tenant acme
wt login -u ada
wt push
wt daemon start && wt daemon watch      # optional: auto-snapshots and auto-staging
```

User documentation lives in `docs/` (`getting-started.md`,
`cli-reference.md`, `configuration.md`, `daemon.md`, `server-architecture.md`).

---

## Implementation Status

| Item | Status |
|---|---|
| All specs in the index exist and are maintained against the code | Built |
| Per-spec feature status | See each spec's Implementation status table |

## Roadmap

Cross-cutting items that appear in several specs' Roadmap sections: lazy
loading through a virtual filesystem, partial multi-tree clone and sharing of
the nested-tree layout, automatic merging by the daemon, branch strategies,
config-file-declared protections/licenses/dependencies, registered paths,
semantic change classification, server-synced reflog, live Git mirroring and
serving the Git protocol, verified e-mail addresses.

## References

- `docs/ENGINEERING_PLAN.md` — binding contracts (§1-§13)
- `crates/worktree-protocol/proto/sync.proto` — wire protocol
- `crates/worktree-sdk/src/repo/mod.rs` — `.wt/` layout
- `crates/worktree-cli/src/cli.rs` — command surface
- `crates/worktree-bg/` — daemon
- `services/server-go/internal/domain/` — server types and interfaces
- `docs/` — user-facing documentation
