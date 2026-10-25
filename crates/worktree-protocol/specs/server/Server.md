# W0rkTree Server Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Architecture](#2-architecture)
- [3. Transport: one port](#3-transport-one-port)
- [4. Configuration](#4-configuration)
- [5. Lifecycle](#5-lifecycle)
- [6. Data model](#6-data-model)
- [7. Authentication and authorization](#7-authentication-and-authorization)
- [8. Enforcement points](#8-enforcement-points)
- [9. Merge requests, reviews and CI](#9-merge-requests-reviews-and-ci)
- [10. Staged snapshots](#10-staged-snapshots)
- [11. Tags, releases, dependencies and TODOs](#11-tags-releases-dependencies-and-todos)
- [12. Storage and garbage collection](#12-storage-and-garbage-collection)
- [13. Events and SSE](#13-events-and-sse)
- [14. Audit log](#14-audit-log)
- [15. Health and metrics](#15-health-and-metrics)
- [16. Quotas and rate limiting](#16-quotas-and-rate-limiting)
- [17. Error model](#17-error-model)
- [18. REST API catalogue](#18-rest-api-catalogue)
- [19. Design constraints](#19-design-constraints)
- [20. Implementation status](#20-implementation-status)
- [21. Roadmap](#21-roadmap)
- [22. References](#22-references)

---

## 1. Overview

`worktree-server` is the remote multi-tenant W0rkTree server. It is written in
Go (`services/server-go`, module
`github.com/seanfilimon/worktree/services/server-go`, pure Go, no cgo) and
is the **central authority** for:

- canonical history;
- access control and license compliance;
- branch and tag protection;
- merge requests, CI status and releases;
- team visibility of work in progress.

```
 wt (CLI) / worktree-bg ──► worktree-sdk ── gRPC SyncService (h2c|TLS) ──┐
 apps/admin (React SPA) ───────────────────── REST /api/v1 + SSE ────────┤
                                                                          ▼
                                       worktree-server :7420 (one listener)
                                          ├── meta.db   (bbolt metadata)
                                          └── objects/  (BLAKE3 CAS)
```

The server is the source of truth. When local and server history diverge, the
server branch wins: clients must fetch and merge, and pushes are
fast-forward only. Clients are untrusted, and every claim they make (authors,
taggers, tenants, object hashes, ancestry) is checked or overwritten on the
server.

What the server does **not** do:

- It never watches files or creates auto-snapshots; that is `worktree-bg`.
- It never has a working directory and never touches the developer's
  filesystem.
- It never performs content merges. Merge requests merge only by fast-forward,
  and three-way merges happen on the client.

The Rust `crates/worktree-server` crate was removed and superseded by this Go
service.

## 2. Architecture

The binary is `main.go` → `config.Load(args)` → `server.Run(ctx, cfg)`. Shared
types and interfaces live in `internal/domain` (`types.go`, `interfaces.go`,
`iam.go`, `v05.go`).

| Package | Role |
|---|---|
| `internal/config` | Flags and `WT_*` environment variables; validation. |
| `internal/server` | Wiring, the single listener (gRPC + HTTP mux), TLS, interceptors, background tickers, graceful shutdown. |
| `internal/syncsvc` | gRPC `worktree.sync.v1.SyncService` (see [Sync](../sync/Sync.md)). |
| `internal/httpapi` | REST `/api/v1`, SSE, `/healthz`, `/metrics`, static admin SPA. |
| `internal/meta` | `MetaStore` + `DocStore` on bbolt (`NewBolt`) or in memory (`NewMemory`); `NewHistory` (ancestry, log, missing). |
| `internal/objstore` | Content-addressed object store on disk (`objects/<2>/<62>`). |
| `internal/content` | Reads (chunked) file content out of the object store. |
| `internal/iam` | Authentication: accounts, passwords, tokens, sessions, MFA, invitations, password resets; gRPC/HTTP auth interceptors. |
| `internal/authz` | Authorization: roles, teams, grants, service accounts, policies, simulator. |
| `internal/sso` | OIDC single sign-on. |
| `internal/review` | Branch protection, merge requests, approvals, reviews, linked groups. |
| `internal/license` | Per-path SPDX license rules (push and read checks). |
| `internal/pathacl` | Path-level write denies as a push check. |
| `internal/signing` | Ed25519 signing keys and snapshot signature verification (push check). |
| `internal/limits` | Tenant quotas (push check and REST) and request rate limiting. |
| `internal/staged` | Staged (WIP) snapshots, summaries, overlap detection. |
| `internal/events` | In-process pub/sub event bus with a recent-events ring. |
| `internal/audit` | Audit log writer and queries. |
| `internal/ci` | CI check status per snapshot. |
| `internal/todo` | Cross-tree dependencies → TODOs. |
| `internal/release` | Releases and release assets. |
| `internal/gc` | Object garbage collection; staged and audit retention. |
| `internal/accesscfg` | Declarative access documents (plan/apply). |
| `internal/textdiff`, `internal/hash` | Unified diffs; BLAKE3 hash type. |

Services depend only on `domain` interfaces, so each package is tested with
small in-package fakes and `meta.NewMemory()`.

## 3. Transport: one port

One TCP listener (default `:7420`) serves everything:

- **gRPC** handles requests with `ProtoMajor == 2` and `Content-Type:
  application/grpc*` (`SyncService` plus `grpc.health.v1.Health`). The message
  limit is 64 MiB in both directions. Interceptors handle panic recovery,
  bearer authentication (`iam.UnaryInterceptor` / `StreamInterceptor`) and
  rate limiting. Calls without a token run anonymously, and an invalid
  token fails with `Unauthenticated`.
- **HTTP** serves everything else: `/healthz`, `/metrics`, `/api/v1/...`
  (JSON and SSE), and `/admin/` (the static admin SPA from `--admin-dir`, with
  an `index.html` fallback). `/admin` redirects to `/admin/`, and `/`
  redirects to `/admin/`.
- **Protocols.** In cleartext the listener speaks HTTP/1.1 plus HTTP/2 with prior
  knowledge (h2c, standard library `http.Protocols`), with up to 250 concurrent
  streams per connection. Server-wide settings are `ReadHeaderTimeout` 10 s,
  `IdleTimeout` 2 min and 1 MiB maximum headers. There are no read or write
  timeouts, because uploads, downloads and SSE are long-lived.
- **TLS.** With `--tls-cert`/`--tls-key` the same port serves HTTPS and gRPC
  over TLS (ALPN `h2`), and `--tls-client-ca` adds optional client-certificate
  verification (mTLS). The server logs a warning when it listens in cleartext
  on a non-loopback address.
- **CORS.** `http://localhost:5173` (Vite dev server) is always allowed, plus
  the origins in `--cors-origins` / `WT_CORS_ORIGINS`. `*` allows any origin.
- **Request bodies.** JSON bodies are limited to 1 MiB (`413
  payload_too_large`) and must be `application/json` (`415
  unsupported_media_type`). Release asset uploads accept raw bodies of up to
  64 MiB.

There is no separate gRPC port, no WebSocket endpoint and no QUIC listener.

## 4. Configuration

Configuration comes from command-line flags and `WT_*` environment variables
only; there is no configuration file. Precedence is flag > environment
variable > default.

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--addr` | `WT_ADDR` | `:7420` | Listen address for gRPC and HTTP. |
| `--data` | `WT_DATA` | `./wt-data` | Data directory (`meta.db`, `objects/`). |
| `--admin-user` | `WT_ADMIN_USER` | `admin` | Server admin username. |
| `--admin-password` | `WT_ADMIN_PASSWORD` | — | If set, ensures the admin account exists with this password and server-admin rights. |
| `--admin-dir` | `WT_ADMIN_DIR` | — | Static admin build (`apps/admin/dist`) served at `/admin/`. |
| `--allow-registration` | `WT_ALLOW_REGISTRATION` | `true` | Self-service `POST /auth/register`. |
| `--log-level` | `WT_LOG_LEVEL` | `info` | `debug`, `info`, `warn`, `error`. |
| `--log-format` | `WT_LOG_FORMAT` | `text` | `text` or `json` (slog to stderr). |
| `--cors-origins` | `WT_CORS_ORIGINS` | — | Comma-separated extra origins. |
| `--trusted-proxies` | `WT_TRUSTED_PROXIES` | — | Comma-separated proxy IPs/CIDRs whose `X-Forwarded-For` names the client (IP conditions, rate limits, audit). |
| `--shutdown-timeout` | `WT_SHUTDOWN_TIMEOUT` | `15s` | Graceful shutdown bound. |
| `--public-url` | `WT_PUBLIC_URL` | — | External base URL (default SSO redirect URL). |
| `--tls-cert`, `--tls-key` | `WT_TLS_CERT`, `WT_TLS_KEY` | — | Serve TLS. |
| `--tls-client-ca` | `WT_TLS_CLIENT_CA` | — | Optional mTLS client CA bundle. |
| `--gc-interval` | `WT_GC_INTERVAL` | `24h` | GC ticker; `0` disables it. |
| `--gc-grace` | `WT_GC_GRACE` | `720h` | Unreachable objects are deleted after this long. |
| `--staged-retention` | `WT_STAGED_RETENTION` | `168h` | Staged snapshot lifetime; `0` keeps them. |
| `--audit-retention` | `WT_AUDIT_RETENTION` | `0` | Audit event lifetime; `0` keeps them. |
| `--rate-limit` | `WT_RATE_LIMIT` | `1200` | Requests per minute per principal or anonymous IP; `0` disables it. |
| `--default-max-trees` | `WT_DEFAULT_MAX_TREES` | `0` | Default tenant tree quota (`0` = unlimited). |
| `--default-max-storage` | `WT_DEFAULT_MAX_STORAGE` | `0` | Default tenant storage quota in bytes (`0` = unlimited). |
| `--version` | — | — | Print the version and exit. |

Exit codes are 0 (clean shutdown), 1 (runtime error) and 2 (invalid usage or
configuration).

## 5. Lifecycle

**Startup** (`server.Run`):

1. Create the data directory and open `meta.db`. The file is locked
   exclusively, so a second process on the same directory fails within 5 s.
   Open the object store and the history walker.
2. Build the services: iam, authz, events bus, audit, license, review, staged,
   and the v0.5 components (todo, release, ci, signing, gc, limits, accesscfg,
   pathacl).
3. With `--admin-password`, ensure the admin account (`iam.EnsureAdmin`).
4. Build the sync service with `PushChecks` (path ACL first, then the quota and signature
   checkers), then
   the gRPC server with its health service and the HTTP API.
5. Listen, start background tasks (GC ticker; the TODO subscriber on
   `branch.pushed`; tree-deletion subscribers that clean document
   collections), audit `server.start`, then print `worktree-server listening
   on <addr>`.

**Shutdown** happens on SIGINT, SIGTERM or context cancellation, within
`--shutdown-timeout`:

1. The gRPC health service reports `NOT_SERVING`.
2. `http.Server.Shutdown` drains every connection, including the gRPC streams
   carried through `ServeHTTP` (GOAWAY), and the SSE streams end.
3. Only after that, `grpc.Server.GracefulStop` runs, because it must not run
   while `ServeHTTP` streams are active.
4. If the timeout expires, everything is closed forcibly.
5. The server audits `server.stop` and closes the metadata store.

Every write is one bbolt transaction, so no separate flush step is needed.

## 6. Data model

Field names are the `json` tags in `internal/domain` (authoritative). See
[Tenant Model](../iam/TenantModel.md) and [IAM](../iam/IAM.md) for detail.

| Entity | Key fields |
|---|---|
| **Tenant** | `id`, `slug`, `name`, `kind` (`user` for personal tenants, `org`), `plan` (`free`, `pro`, `enterprise`; a label set by a server admin), `status` (`active`, `suspended`: writes denied, `deactivated`: all access denied, server admins excepted), `status_reason`, `limits`. Registering an account creates its personal tenant. |
| **Account** | `id`, `username`, `email`, `is_admin` (server admin), `disabled`. |
| **Membership** | tenant + account + role (`owner`, `admin`, `maintainer`, `developer`, `viewer` or a custom role). |
| **Team, Grant, ServiceAccount, CustomRole, Policy** | IAM (see [IAM](../iam/IAM.md)). |
| **Tree** | `id`, `tenant_id`, `name` (unique per tenant), `description`, `visibility` (`private`, `internal`, `public`), `default_branch`. Trees created by `ResolveTree` are private with default branch `main`. |
| **Branch** | `name`, `tip` (snapshot id), `updated_at`. Moves only by fast-forward (CAS). |
| **Snapshot** | See [Storage](../storage/Storage.md#object-model); the server adds `pushed_by`, `pushed_at`, `verified`. |
| **Tag** | `name`, `target`, `message`, `tagger`, `created_at`; immutable. |
| **StagedSnapshot** | One per (tree, account, branch); see §10. |
| **BranchProtection** | `pattern` (`path.Match` glob), `no_direct_push`, `required_approvals`, `allowed_pushers`, `no_delete`, `require_ci_pass`, `required_checks`, `require_signed`, `require_linear`, `block_on_todos`. |
| **TagProtection** | `pattern`, `no_delete`, `no_move`, `allowed_taggers`. |
| **LicenseRule** | `path_glob`, `spdx`, `grant` (`read-only`, `modify`, `redistribute`), `grantee` (tenant slug; `""` = every non-owning tenant), `expires_at`. |
| **MergeRequest** | `number` (per tree), `title`, `description`, `source_branch`, `target_branch`, `state` (`open`, `merged`, `closed`), `approvals`, `linked_group`. |

## 7. Authentication and authorization

- **Tokens.** `POST /api/v1/auth/login` returns a session token, or `{mfa_required,
  mfa_token}` followed by `/auth/login/mfa`. API tokens and service-account
  keys can carry scopes. Tokens are stored as hashes. REST and gRPC both use
  `Authorization: Bearer <token>`. SSO is OIDC with PKCE, state and nonce.
- **Authorization** (`internal/authz`):
  1. A disabled principal is denied.
  2. A server admin is allowed (token scopes still apply).
  3. Token scopes are checked.
  4. Roles are collected from membership, teams, tree grants (users, teams,
     service accounts, `tenant:` subjects) and visibility (public read for
     everyone, internal read for any signed-in principal).
  5. Enabled tenant policies apply: an explicit deny wins and an allow grants.
  6. Otherwise the request is denied implicitly.

  Policy denies never lock tenant owners out of `iam:*` and `tenant:admin`.
  Suspended tenants deny writes and deactivated tenants deny everything.
- Anonymous callers may `Ping` and read public trees. Every 403 answered by the
  REST API and every gRPC `PermissionDenied` is audited as `access.denied`.

See [IAM](../iam/IAM.md), [Declarative Access](../iam/DeclarativeAccess.md) and
[Security](../security/Security.md).

## 8. Enforcement points

All enforcement is on the server. The client reads protections and policies
only to display them.

| Operation | Checks |
|---|---|
| **Push** (gRPC) | `tree:write` → `branch:push` (per-branch policies) → branch protection (`no_direct_push`, `allowed_pushers`, `require_linear`) → snapshot records and ancestry → objects present (including chunks) → fast-forward → license `CheckPush` on changed paths → PushChecks (path ACL deny, quotas, signatures / `require_signed`) → CAS. Full pipeline: [Sync § Push](../sync/Sync.md#push). |
| **Pull / DownloadObjects** | `tree:read`; tree-scoped reachability; restricted paths (license `CheckRead`, `tree:read` path denies) are stubbed, and their content is refused. |
| **REST content** (`blob`, `files`, `compare`, file history, snapshot file lists, archive) | Same path rules: `403 path_denied`, `451 license_restricted`; listings omit restricted paths and add `restricted_count`. |
| **Tags** | `PushTag` / REST create need `tree:write`; tag protections `no_move` and `allowed_taggers`; delete needs `tree:admin` and honours `no_delete`. |
| **Branch create / delete** (REST) | `branch:create`; quotas; delete via `review.CheckDeleteBranch` (not the default branch, not `no_delete`). |
| **Merge-request merge** | See §9. |
| **Archive / export** | License-aware: restricted and non-redistributable paths are omitted and a `LICENSES/NOTICE` file is added (audit `tree.export`). |

Every operation that crosses a trust boundary needs both checks: the IAM
decision **and** the license rule must allow it. A tenant admin still cannot
read a path whose license rule withholds it from their tenant.

## 9. Merge requests, reviews and CI

- **Lifecycle.** A merge request is `open` → `merged` | `closed`. Numbers are
  allocated per tree. Creating one needs `mr:create`, and the source and target
  must be branches of the same tree.
- **Reviews.** `POST {tree}/merge-requests/{n}/review {decision, body}` takes
  `approved`, `changes_requested` or `commented` (`commented` needs only
  `tree:read`). `.../approve` is shorthand for `approved`. Authors cannot
  approve their own merge request. An approval records `snapshot_id`, the
  source tip at approval time, and counts only while that is still the source
  tip; a stale approval does not count. A `changes_requested` review from a
  principal with `mr:approve` blocks the merge until that principal approves
  the current tip.
- **Comments.** `GET|POST {tree}/merge-requests/{n}/comments {body, path?,
  line?, snapshot_id?}`; `PATCH|DELETE .../comments/{id}` by the author or
  `tree:admin` (PATCH may set `resolved`).
- **CI.** `POST {tree}/ci/status {snapshot, context, state, description?,
  target_url?}` requires `ci:report`, and a newer report for the same context
  replaces the old one. `GET {tree}/ci/status?snapshot=|branch=` returns the
  combined state: `failure` if any check is failure or error, else `pending` if
  any is pending, else `success` (`""` when there are no checks). Each report
  publishes event `ci.status`.
- **Merge readiness.** `GET {tree}/merge-requests/{n}/checks` returns
  `{mergeable, blockers[{kind, message}], approvals{required, current, stale},
  ci{state, required, missing}, todos{blocking_open}}`. A merge enforces:
  - the required approvals and changes-requested reviews;
  - `require_ci_pass`: every context in `required_checks` is `success` on the
    source tip (an empty list means every reported check);
  - `block_on_todos`: no open blocking TODO targets the source branch;
  - `require_linear`.

  A failed check answers `409 merge_blocked`, except missing approvals
  (`403 review_required`, kept for compatibility), a member of an open linked
  group merged alone (`409 linked_group`) and a target that moved so the
  merge is no longer a fast-forward (`409 conflict`).
- **Merge execution (v0.6).** A merge uses one of three methods (see
  [Merging.md](./Merging.md)): `fast-forward` (the target tip moves to the
  source tip), `merge` (the server writes a three-way merge snapshot with
  parents `[target, source]`) or `squash` (one snapshot with parent
  `[target]`). The default is the first allowed and possible method;
  `BranchProtection.MergeMethods` and `RequireLinear` restrict them, and
  conflicts answer `409 merge_conflict`. The target moves with CAS. A merge
  publishes `mr.merged` and `branch.pushed` (so watchers fetch) and
  increments `worktree_mr_merges_total`.
- **Linked groups.** `POST /tenants/{t}/linked-groups {name,
  members[{tree, number}]}` requires `mr:merge` on every member tree.
  `POST .../{id}/merge` merges all members atomically: every member must be
  mergeable, tips move by CAS in order and roll back on failure, and the merge
  publishes `group.merged`. Merging a member alone answers `409
  linked_group`.

## 10. Staged snapshots

Staged snapshots give a team visibility into work in progress without touching
branch history:

- There is one entry per (tree, account, branch), and a newer stage replaces
  the older one.
- An entry is cleared when its author pushes the branch, when the author
  discards it (`Unstage`, or REST `DELETE {tree}/staged?branch=`; `tree:admin`
  may pass `account=`), or when it is older than `--staged-retention`.
- REST `GET {tree}/staged?branch=` lists entries, and `GET
  {tree}/staged/summary` returns per-branch activity with **file overlaps**:
  the same path staged by two or more accounts, an early conflict warning.
- Events are `snapshot.staged` and `staged.cleared` (`reason` `pushed` or
  `expired`).

Protocol details are in [Sync § Staged snapshots](../sync/Sync.md#staged-snapshots)
and the product view in [Staged Visibility](../visibility/StagedVisibility.md).

## 11. Tags, releases, dependencies and TODOs

- **Tags** are immutable names pointing at a snapshot of the tree. They are
  created by `PushTag` or `POST {tree}/tags {name, target, message?}` (`target`
  = a REV: snapshot id or unique prefix of at least 4 characters, a branch or a
  tag) and deleted by `DELETE {tree}/tags/{name}`. Tag protections apply. Events
  are `tag.pushed` and `tag.deleted`.
- **Releases.** `GET|POST {tree}/releases {tag, name, notes, draft, prerelease,
  snapshot?}` creates the tag at `snapshot` when it does not exist.
  `GET|PATCH|DELETE {tree}/releases/{tag}` reads, edits and deletes a release.
  Assets use `PUT|GET|DELETE .../assets/{name}`: the upload is a raw body of at
  most 64 MiB, stored as a blob in the object store, and the download carries
  Content-Type and Content-Disposition. Actions are `release:create` and
  `release:delete`, and events are `release.created` and `release.deleted`.
- **Dependencies and TODOs.** A snapshot may declare `dependencies` (`wt
  snapshot --depend TREE[@BRANCH] --todo TITLE ...`). When it is pushed, the
  TODO service, which subscribes to `branch.pushed`:
  - resolves each target tree and requires `tree:read` on it for the pusher
    (otherwise it audits `todo.skipped`);
  - deduplicates by (source snapshot, target tree, title);
  - creates a numbered `Todo` in the target tree (event `todo.created`).

  A pushed message line `Completes: #N` or `Completes: tenant/tree#N` marks that
  TODO `done`. The REST endpoints are `GET|POST {tree}/todos`, `GET|PATCH
  {tree}/todos/{n}`, `POST .../claim|complete|cancel`, and `GET
  {tree}/dependencies?branch=` (outgoing and incoming).
- **Signing keys.** `GET|POST /auth/keys {name, public_key}` (Ed25519),
  `DELETE /auth/keys/{id}` (revokes the key), and `GET
  /accounts/{username}/keys` (public keys). Invalid signatures are audited as
  `snapshot.signature_invalid`.

## 12. Storage and garbage collection

The data directory holds `meta.db` (bbolt, every piece of metadata, with v0.5
document collections in the `docs` bucket) and `objects/<2 hex>/<62 hex>`
(a shared BLAKE3 content-addressed store, deduplicated across tenants).
Isolation is in the reference graph: objects are reachable, and therefore
readable, only through a tree's branches and tags.

`gc.Collector` marks everything reachable from branch tips, tags, staged
snapshots, merge-request tips and releases. Unreachable objects are deleted
after `--gc-grace`. The same run applies staged and audit retention. GC runs
every `--gc-interval` and on demand: `POST /api/v1/admin/gc?dry_run=` returns a
`GCReport`, and `GET /api/v1/admin/gc` returns the configuration and the last 20
runs. Full layout and algorithm: [Storage](../storage/Storage.md).

## 13. Events and SSE

`internal/events.Bus` is an in-process, non-blocking pub/sub bus:

- each subscriber has a buffer of 64 events, and events are dropped for a
  subscriber whose buffer is full (counted in bus stats);
- filters match on tenant, tree and types;
- a ring buffer keeps the 200 most recent events.

An event is `{type, tenant_id, tree_id?, actor?, payload?, at}`.

- **`GET /api/v1/events?tenant=&tree=&types=`** returns `text/event-stream`.
  The stream starts with `: connected` and `retry: 5000`, then sends one `event:
  <type>\ndata: <json Event>\n\n` per event and a `: heartbeat` comment every
  15 s. The scope depends on the parameters:
  - with `tree`, the caller needs `tree:read`;
  - with only `tenant`, the caller needs `tenant:read`;
  - with neither, server admins get every event and other accounts get the
    events of their member tenants.

  The stream ends when the server shuts down. `wt events [--tenant] [--tree]
  [--type]...` streams it from the CLI.
- **`GET /api/v1/events/recent?tenant=&tree=&limit=50`** returns recent events,
  oldest first, with the same scoping.
- **gRPC `WatchBranches`** streams branch events to clients
  ([Sync](../sync/Sync.md#branch-watching)).

| Event | Published when | Payload (main fields) |
|---|---|---|
| `tree.created` | tree created (REST or `ResolveTree`) | `name`, `tenant` |
| `tree.deleted` | tree deleted | `name` |
| `branch.pushed` | push accepted or merge request merged | `branch`, `old_tip`, `new_tip`, `count` |
| `branch.created` | REST branch create | branch |
| `branch.deleted` | REST branch delete | branch |
| `tag.pushed` | tag created (gRPC or REST) | `tag`, `target` |
| `tag.deleted` | tag deleted | tag |
| `snapshot.staged` | stage | `StagedSnapshot` |
| `staged.cleared` | push, unstage or expiry | `tree_id`, `account_id`, `author`, `branch_name`, `snapshot_id`, `reason` |
| `mr.opened` / `mr.approved` / `mr.reviewed` / `mr.commented` / `mr.merged` / `mr.closed` | merge-request activity | merge request summary |
| `group.merged` | linked group merged | group |
| `ci.status` | CI report | `CIStatus` |
| `todo.created` / `todo.updated` | TODO created or changed (in the target tenant) | `Todo` |
| `release.created` / `release.deleted` | release activity | release |
| `access.applied` | declarative access document applied | plan |

## 14. Audit log

Audit events (`{id, tenant_id, actor_id, actor, action, resource_type,
resource_id, details, remote_addr, at}`) are stored in the bbolt `audit`
bucket in time order. Actions include:

| Area | Actions |
|---|---|
| Authentication | `auth.login`, `auth.login_failed`, `auth.logout`, `auth.register`, `auth.mfa_challenge`, `auth.password_change`, `sso.login` |
| Sync | `branch.push`, `branch.push_rejected` (with reason), `tag.push`, `tree.create` (`via: grpc`), `license.denied`, `snapshot.signature_invalid` |
| Management | `tree.*`, `branch.*`, `tag.*`, `protection.*`, `license.*`, `mr.*`, `member.*`, `tenant.*`, `account.*` |
| IAM | `team.*`, `role.*`, `policy.*`, `grant.*`, `sa.*`, `token.*`, `session.*`, `mfa.*`, `invitation.*`, `password.*`, `access.applied` |
| Server and access | `access.denied` (every REST 403 and gRPC `PermissionDenied`), `todo.skipped`, `tree.export`, `server.start`, `server.stop` |

`GET /api/v1/audit?tenant=&action=&resource_type=&resource_id=&since=&until=&limit=`
queries the log. `action` accepts a prefix such as `iam.*`, and
`format=ndjson` streams every match as `application/x-ndjson`. Tenant admins
see their tenants; server admins see everything when no tenant is given. The
CLI is `wt audit`. Retention is `--audit-retention`.

## 15. Health and metrics

- **`GET /healthz`** (no prefix, no auth, never rate limited) returns
  `{"status": "ok"|"degraded", "version", "uptime_seconds", "checks":
  {"meta": "ok", "objects": "ok"}}`, with HTTP `503` when a probe fails.
  `grpc.health.v1.Health` is also served.
- **`GET /metrics`** (Prometheus text, exempt from rate limits) exposes:
  - `worktree_http_requests_total{route,code}`,
    `worktree_http_request_duration_seconds` (histogram by route),
    `worktree_http_requests_in_flight`;
  - gauges `worktree_tenants`, `worktree_accounts`, `worktree_trees`,
    `worktree_branches`, `worktree_snapshots`, `worktree_staged_snapshots`,
    `worktree_objects`, `worktree_storage_bytes`;
  - `worktree_build_info{version,goversion}`, `worktree_uptime_seconds`;
  - `worktree_sync_pushes_total{result}`, `worktree_sync_pulls_total`,
    `worktree_authz_decisions_total{decision}`, `worktree_mr_merges_total`,
    `worktree_gc_runs_total`, `worktree_gc_deleted_objects_total`,
    `worktree_rate_limited_total`.
- **`GET /api/v1/admin/stats`** returns `ServerStats`, and **`GET
  /api/v1/admin/system`** returns `SystemInfo` (both server admin only).

## 16. Quotas and rate limiting

- **Quotas.** Tenant limits (`max_trees`, `max_storage_bytes`,
  `max_branches_per_tree`, `max_members`, `requests_per_minute`) override the
  server defaults (`--default-max-trees`, `--default-max-storage`). They are
  enforced as follows:
  - storage and branches by the `limits.Limiter` push check (`QuotaExceeded`);
  - trees (REST create and a first push that creates the tree), members and
    REST branch creation with `409 quota_exceeded` (gRPC:
    `PushDenied{Kind:"quota_exceeded"}`).

  The endpoints are `GET|PUT /api/v1/admin/tenants/{t}/limits` (server admin)
  and `GET /api/v1/tenants/{t}/usage`. Existing data is never deleted when a
  tenant is over quota.
- **Rate limiting.** A token bucket runs per principal (account or service
  account id), or per remote IP for anonymous calls. The default is
  `--rate-limit` = 1,200 requests per minute, and a tenant's
  `requests_per_minute` overrides it.
  - HTTP answers `429 {"error": {"code": "rate_limited"}}` with `Retry-After`,
    `X-RateLimit-Limit` and `X-RateLimit-Remaining`.
  - gRPC answers `ResourceExhausted`.

  `/healthz` and `/metrics` are exempt.

## 17. Error model

**REST** errors use the envelope `{"error": {"code": "...", "message": "..."}}`:

| Code | HTTP | Meaning |
|---|---|---|
| `bad_request` | 400 | Invalid input. |
| `unauthenticated` | 401 | Missing or invalid credentials. |
| `forbidden` | 403 | Permission denied. |
| `path_denied` | 403 | A policy denies reading the path. |
| `branch_protection`, `license_violation`, `access_denied`, `review_required`, `signature_required`, `ci_checks_failed` | 403 | `PushDenied` kinds surfaced by REST operations. |
| `not_found` | 404 | Unknown resource or API path. |
| `method_not_allowed` | 405 | With an `Allow` header. |
| `conflict` | 409 | Already exists, or a concurrent update. |
| `quota_exceeded`, `merge_blocked`, `linked_group`, `managed_by_config`, `last_owner` | 409 | Specific conflicts. |
| `payload_too_large` | 413 | Body over the limit. |
| `unsupported_media_type` | 415 | Body is not JSON. |
| `rate_limited` | 429 | With `Retry-After`. |
| `license_restricted` | 451 | License withholds the content. |
| `internal` | 500 | Logged; no detail is exposed. |
| `not_implemented` | 501 | The feature's service is not configured. |

**gRPC** uses canonical status codes (`Unauthenticated`, `PermissionDenied`,
`InvalidArgument`, `NotFound`, `FailedPrecondition`, `ResourceExhausted`,
`Unimplemented`, `Internal`). Push rejections are response values
(`PushRejection` oneof), not errors. See [Sync § Errors](../sync/Sync.md#errors).

## 18. REST API catalogue

Every path below is under `/api/v1`, except `/healthz`, `/metrics` and
`/admin/*`. `{tree}` = `/trees/{tenant}/{tree}`, and `{t}` = `/tenants/{t}`.
`REV` = a snapshot id (full, or a unique prefix of at least 4 characters), a
branch or a tag. JSON field names are the Go `json` tags. The admin panel's
`apps/admin/src/api/*.ts` types mirror them.

**Health and server administration**

```
GET  /healthz | /metrics
GET  /admin/stats | /admin/system
GET  /admin/accounts | POST /admin/accounts | PATCH /admin/accounts/{username}
POST /admin/accounts/{username}/password | /admin/accounts/{username}/reset-code
GET|PUT /admin/sso
POST /admin/gc?dry_run= | GET /admin/gc
GET|PUT /admin/tenants/{t}/limits | PATCH /admin/tenants/{t} {status, status_reason}
GET  /search?q=&limit=20
```

**Authentication and the caller's account**

```
POST /auth/register | /auth/login | /auth/login/mfa | /auth/logout | /auth/password | /auth/password-reset
GET  /auth/me | /auth/providers | /auth/sso/start | /auth/sso/callback
GET|POST /auth/tokens | DELETE /auth/tokens/{id}
GET  /auth/sessions | DELETE /auth/sessions/{id} | POST /auth/sessions/revoke-others
GET  /auth/mfa | POST /auth/mfa/totp/enroll | /auth/mfa/totp/confirm | /auth/mfa/recovery-codes | DELETE /auth/mfa/totp
GET|POST /auth/keys | DELETE /auth/keys/{id} | GET /accounts/{username}/keys
GET  /invitations/{code} | POST /invitations/{code}/accept
```

**Tenants and IAM** (details in [IAM](../iam/IAM.md))

```
GET|POST /tenants | GET|PATCH {t} | GET {t}/usage
GET {t}/members | PUT|DELETE {t}/members/{username}
GET|POST {t}/trees
{t}/teams[/{team}[/members/{username}]] | {t}/roles[/{name}] | {t}/policies[/{id}] | POST {t}/policies/validate
{t}/service-accounts[/{name}[/keys[/{id}]]] | {t}/invitations[/{id}]
GET {t}/access-config | POST {t}/access-config/plan | /apply
GET|POST {t}/linked-groups | GET|DELETE {t}/linked-groups/{id} | POST {t}/linked-groups/{id}/merge
GET /iam/actions | POST /iam/simulate | GET /iam/effective
```

**Trees and content**

```
GET|PATCH|DELETE {tree}
GET  {tree}/branches | POST {tree}/branches {name, from?} | DELETE {tree}/branches/{name...}
GET  {tree}/snapshots?branch=&limit= | GET {tree}/snapshots/{id}
GET  {tree}/blob/{snapshot}/{path...}            raw bytes (chunked files reassembled)
GET  {tree}/compare?to=&from=&context= | {tree}/history?path=&branch=&limit= | {tree}/stats?branch=&days=
GET  {tree}/archive/{snapshot}?format=tar.gz|zip
GET  {tree}/tags | POST {tree}/tags | DELETE {tree}/tags/{name...}
GET|PUT|DELETE {tree}/protections (?pattern=) | {tree}/tag-protections (?pattern=)
GET|PUT|DELETE {tree}/licenses (?path_glob=) | {tree}/grants (?subject_type=&subject=)
GET  {tree}/access-config | POST {tree}/access-config/plan | /apply
```

**Collaboration**

```
GET  {tree}/staged?branch= | DELETE {tree}/staged?branch=&account= | GET {tree}/staged/summary
GET|POST {tree}/merge-requests?state= | GET {tree}/merge-requests/{n}
POST {tree}/merge-requests/{n}/approve | /review | /merge | /close | GET .../checks
GET|POST {tree}/merge-requests/{n}/comments | PATCH|DELETE .../comments/{id}
POST {tree}/ci/status | GET {tree}/ci/status?snapshot=|branch=
GET|POST {tree}/todos | GET|PATCH {tree}/todos/{n} | POST {tree}/todos/{n}/claim|complete|cancel
GET  {tree}/dependencies?branch=
GET|POST {tree}/releases | GET|PATCH|DELETE {tree}/releases/{tag}
PUT|GET|DELETE {tree}/releases/{tag}/assets/{name}
```

**Activity**

```
GET /events?tenant=&tree=&types= (SSE) | GET /events/recent
GET /audit?tenant=&action=&resource_type=&resource_id=&since=&until=&limit=&format=ndjson
```

Unknown `/api/` paths answer with the JSON envelope (`404 not_found`, or `405
method_not_allowed` with `Allow`).

## 19. Design constraints

1. **The server is the source of truth.** Branch tips move only on the server,
   with compare-and-swap: pushes are fast-forward only; merge requests may add
   server-written merge or squash snapshots.
2. **Append-only history.** There is no force push, rebase or rewrite of pushed
   snapshots.
3. **The server never touches working directories.** It stores snapshots and
   objects that clients upload.
4. **The client is untrusted.** Hashes, ancestry and ownership are verified, and
   identity fields are set from the token.
5. **Deny by default.** No role and no allowing policy means denied, and an
   explicit deny beats every allow.
6. **Dual enforcement.** IAM and license rules must both allow access to
   content.
7. **Tree rules only restrict.** Tree-level declarative documents may contain
   only deny statements.
8. **Logical tenant isolation.** Storage is a shared, deduplicated object store,
   and access is mediated by the reference graph and IAM.
9. **Structured errors.** Every refusal carries a machine-readable code (REST
   envelope code, gRPC status, or `PushRejection` variant) and a
   human-readable message.
10. **One port, one binary, embedded storage.** There are no external
    dependencies to run a server.

## 20. Implementation status

| Area | Status |
|---|---|
| Single-port gRPC (h2c or TLS) + REST + SSE + static admin; graceful shutdown | Built |
| `SyncService` (16 RPCs), push validation pipeline with PushChecks | Built |
| Tenants, accounts, teams, roles, policies, grants, service accounts, MFA, SSO, invitations | Built ([IAM](../iam/IAM.md)) |
| Tenant status (suspend / deactivate) | Built |
| Branch protection (including CI, signed, linear, TODO gates) and tag protection | Built |
| Per-path license rules (grantee, expiry), path ACLs, stubbed paths, license-aware archives | Built |
| Merge requests (fast-forward), reviews with stale approvals, comments, CI status, readiness, linked groups | Built |
| Staged snapshots with summaries and overlap detection | Built |
| Tags, releases with assets, dependencies → TODOs, signing keys and signature verification | Built |
| Event bus, SSE, recent events, `WatchBranches` | Built |
| Audit log with filters, retention and NDJSON export | Built |
| GC with grace period; quotas; rate limiting | Built |
| Health probes, Prometheus metrics | Built |
| `PushRejection.ci_checks_failed` / `review_required.current` | Partial (wire variants defined; denials travel as `access_denied` / `current = 0`) |

## 21. Roadmap

Described in earlier versions of this spec but not built:

- **QUIC transport**, a separate gRPC port, and WebSocket event endpoints (SSE
  and `WatchBranches` cover real-time needs).
- **Additional gRPC services** (`BranchService`, `TagService`,
  `MergeRequestService`, `AuthService`). These are served by REST plus the
  single `SyncService`.
- **Interactive conflict resolution on the server** and automatically deleting
  the source branch after a merge (merge and squash snapshots are built, see
  Merging.md).
- **Force push** with a `no_force` protection and an elevated permission.
- **Physical tenant isolation**: dedicated storage, workers or endpoints per
  tenant.
- **Plan-based limits and retention tiers** (fixed quotas, staged retention or
  rate limits per plan). Limits are server defaults plus per-tenant overrides.
- **Forking** across tenants.
- **Partial tree sync** with on-demand stub hydration, delta and pack transfer.
  See the [Sync](../sync/Sync.md#roadmap) roadmap.
- **A server config file** (TOML), `worker_threads`, and file-based audit
  logs with rotation. Configuration is flags and environment variables, and
  audit lives in `meta.db`.
- **Deactivated-tenant data purge** after a grace period.
- **Request ids** in audit records, and per-tenant or per-tree metric labels.
- **Pluggable storage backends** (S3, external database) and replication.

## 22. References

- `services/server-go/main.go`, `README.md`
- `internal/server/server.go`, `internal/config/config.go`
- `internal/syncsvc/*`, `internal/httpapi/*` (`httpapi.go` routes,
  `respond.go` errors, `middleware.go` CORS and rate limits, `events.go` SSE,
  `metrics.go`, `trees.go`, `snapshots.go`, `insights.go`, `manage.go`,
  `mergerequests.go`, `reviews.go`, `ci.go`, `todos.go`, `releases.go`,
  `keys.go`, `ops.go`, `access_routes.go`, `exports.go`,
  `staged_routes.go`, `iam*.go`, `sso.go`, `static.go`)
- `internal/domain/{types,interfaces,iam,v05}.go`
- `internal/{meta,objstore,content,iam,authz,sso,review,license,pathacl,signing,limits,staged,events,audit,ci,todo,release,gc,accesscfg}`
- `crates/worktree-protocol/proto/sync.proto`
- `docs/ENGINEERING_PLAN.md` §1, §2, §7, §11, §12, §13
- Related specs: [Sync](../sync/Sync.md), [Storage](../storage/Storage.md),
  [IAM](../iam/IAM.md), [Tenant Model](../iam/TenantModel.md),
  [Declarative Access](../iam/DeclarativeAccess.md),
  [License Compliance](../licensing/LicenseCompliance.md),
  [Staged Visibility](../visibility/StagedVisibility.md),
  [Security](../security/Security.md), [BgProcess](../bgprocess/BgProcess.md)
