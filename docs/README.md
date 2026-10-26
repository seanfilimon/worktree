# W0rkTree documentation

Documentation for the v2 implementation of W0rkTree. Start with **Getting started**; use the
references when you need exact behavior. Long-form design specifications (including features that
are not implemented yet) live in [`crates/worktree-protocol/specs/`](../crates/worktree-protocol/specs/README.md).

## Guides

| Document | For | Contents |
|---|---|---|
| [Getting started](./getting-started.md) | Everyone | Build, run a server, first worktree, branches, nested trees, sharing, team workflow |
| [Git compatibility](./git-compatibility.md) | Teams migrating from Git | `wt git import` / `export`, mapping, determinism, limits |
| [Admin panel](./admin-panel.md) | Server operators | Running, building and using `apps/admin` |
| [Deploying the website](./deploy-docs-site.md) | Maintainers | Deploying `apps/web` (website and docs) to Railway: setup, `scripts/deploy-docs.sh`, GitHub Actions |

## Reference

| Document | Contents |
|---|---|
| [CLI reference](./cli-reference.md) | Every `wt` command and flag, exit codes, `--json`, revision syntax |
| [Configuration](./configuration.md) | `.wt/config.toml` keys, `$WT_HOME` files, `bg.toml`, server flags and environment |
| [Storage format](./storage-format.md) | The `.wt/` directory: objects, manifests, snapshots, refs, index, reflog, merge state |
| [Sync protocol](./protocol-spec.md) | `worktree.sync.v1.SyncService`, hashing, push / pull / stage flows, rejections |
| [Server architecture](./server-architecture.md) | Go packages, request flow, storage, IAM and role matrix, staged snapshots, branch protection, merge requests, licenses, events, audit, REST API |
| [REST API (OpenAPI 3.1)](../services/server-go/internal/httpapi/openapi.yaml) | Every `/api/v1` route: parameters, bodies, responses, errors, auth and the `x-wt-action` each checks. Served by every server at `GET /api/v1/openapi.yaml`; call routes with `wt api METHOD PATH`. Conventions: [`specs/server/RestApi.md`](../crates/worktree-protocol/specs/server/RestApi.md) |
| [Identity and access](./iam.md) | Roles, custom roles, teams, grants, policies and the simulator, service accounts, tokens, sessions, MFA, invitations, password resets, SSO |
| [Security](./security.md) | Secret scanning, passwords and tokens, history guarantees, deployment notes |
| [Daemon](./daemon.md) | `worktree-bg`: lifecycle, auto-snapshot pipeline, IPC protocol |
| [SDK guide](./sdk-guide.md) | Embedding `worktree-sdk` in Rust programs |

## Component READMEs

| Component | Docs |
|---|---|
| `crates/worktree-sdk` | [README](../crates/worktree-sdk/README.md) · [Implementation notes](../crates/worktree-sdk/Implementation.md) |
| `crates/worktree-cli` | [README](../crates/worktree-cli/README.md) · [Implementation notes](../crates/worktree-cli/Implementation.md) |
| `crates/worktree-bg` | [README](../crates/worktree-bg/README.md) |
| `crates/worktree-git` | [Implementation notes](../crates/worktree-git/Implementation.md) |
| `crates/worktree-protocol` | [README](../crates/worktree-protocol/README.md) · [`sync.proto`](../crates/worktree-protocol/proto/sync.proto) |
| `services/server-go` | [README](../services/server-go/README.md) |

## Contributors

| Document | Contents |
|---|---|
| [CONTRIBUTING.md](../CONTRIBUTING.md) | Development setup, workspace layout, coding standards, tests, proto regeneration, commits and PRs |
| [Engineering plan](./ENGINEERING_PLAN.md) | The binding v2 contract between components |

## Glossary

| Term | Meaning |
|---|---|
| **Worktree** | A directory with a `.wt/` folder; contains one root tree and any number of nested trees. |
| **Tree** | An independently versioned unit with its own branches, snapshots, tags and reflog. On the server, a tree belongs to a tenant. |
| **Nested tree** | A subdirectory with a `.wt-tree/` marker; excluded from its parent tree's snapshots. |
| **Snapshot** | Immutable record of a tree's files at a point in time (manifest hash, parents, author, message). Id = UUID. |
| **Manifest** | Sorted list of `(path, blob hash, size, executable)` for one snapshot; stored as an object. |
| **Blob** | The bytes of one file, addressed by BLAKE3-256. |
| **Branch** | Named pointer to a snapshot within one tree. Archived instead of deleted. |
| **Remote-tracking branch** | `origin/<branch>`: the last tip seen on a remote. |
| **Tag** | Immutable named pointer to a snapshot, optionally with a message. |
| **Staged snapshot** | A team-visible upload of someone's working state; not part of branch history. One per (tree, account, branch). |
| **Auto-snapshot** | A snapshot taken by the daemon after a quiet period (`auto: N files changed`). |
| **Tenant** | A user (personal, slug = username) or organization on the server that owns trees. |
| **Role** | `viewer`, `developer`, `maintainer`, `admin` or `owner` — a member's permissions in a tenant. |
| **Visibility** | `private`, `internal` (any signed-in account can read) or `public` (anonymous reads). |
| **Branch protection** | Server rule matching branch names: no direct push, required approvals, allowed pushers, no delete. |
| **Merge request** | Server-side request to fast-forward a target branch to a source branch after approvals. |
| **License rule** | SPDX license + grant (`read-only`, `modify`, `redistribute`) on a path glob, enforced for other tenants. |
| **Reflog** | Append-only local log of operations that moved branches or rewrote working files. |
| **Daemon** | `worktree-bg`, the optional per-user background process. |
| **Roadmap** | Specified in `crates/worktree-protocol/specs/` but not implemented in v2. |
