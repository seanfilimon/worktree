# Visibility Specification: Trees and Staged Snapshots

## Table of Contents

1. [Overview](#1-overview)
2. [Tree visibility](#2-tree-visibility)
3. [Staged snapshots: the idea](#3-staged-snapshots-the-idea)
4. [Producing a staged snapshot](#4-producing-a-staged-snapshot)
5. [Server model](#5-server-model)
6. [Who can see staged work](#6-who-can-see-staged-work)
7. [Surfaces](#7-surfaces)
8. [Activity summary and overlap detection](#8-activity-summary-and-overlap-detection)
9. [Clearing and retention](#9-clearing-and-retention)
10. [Privacy controls](#10-privacy-controls)
11. [Interaction with licenses and path policies](#11-interaction-with-licenses-and-path-policies)
12. [Implementation status](#12-implementation-status)
13. [Roadmap](#13-roadmap)
14. [References](#14-references)

---

## 1. Overview

This spec covers two kinds of visibility on a W0rkTree server:

- **Tree visibility** (`private`, `internal`, `public`). It decides who may
  read a tree beyond the roles and grants of [IAM](../iam/IAM.md).
- **Staged snapshot visibility.** Teammates can see who is working on which
  files, on which branch, before anything is pushed.

The guiding rule for staged snapshots is *staged ≠ pushed, visible ≠ merged*.
Staged snapshots are team-visible metadata about work in progress. They never
enter branch history.

## 2. Tree visibility

| `Tree.Visibility` | Anonymous | Any signed-in principal | Tenant members, grantees |
|---|---|---|---|
| `private` (default) | — | — | per role or grant |
| `internal` | — | `tree:read` | per role or grant |
| `public` | `tree:read` | `tree:read` | per role or grant |

Visibility follows these rules:

- **Where it is set.** On `POST /tenants/{t}/trees {visibility}`, or with
  `PATCH /trees/{t}/{tree}` / `wt trees update TREE --visibility V`
  (`tree:admin`).
- **Read only.** Visibility adds `tree:read` and nothing else: no
  `tenant:read`, never any write. Unknown values count as `private`.
- **Denies still win.** An explicit policy deny overrides visibility, for
  example a `tree:read` deny on `wt:path/<t>/<tree>/internal/**` for
  `anonymous`.
- **Licenses still apply.** License rules still govern content for non-member
  tenants. A public tree can therefore expose some paths only as stubs. See
  [License Compliance](../licensing/LicenseCompliance.md).

`tree:read` covers browsing, cloning, pulling, downloading objects, listing
branches, tags and staged work, and subscribing to the tree's events.

## 3. Staged snapshots: the idea

In Git, work is invisible until it is pushed. Two people can edit the same
file for days and only find out at merge time. W0rkTree closes that gap at
snapshot granularity:

- **What is published.** The background daemon (or `wt stage`) publishes a
  **staged snapshot**: the list of files the author has changed relative to
  the shared branch head, plus counts and an optional message.
- **Who sees it.** Teammates see it through `wt status --team`, the admin
  panel, SSE events and the API.
- **What is never published.** Keystrokes, cursor positions and line-level
  diffs. Granularity is a snapshot, not a character.
- **Not history.** Staged snapshots are not part of history. A push replaces
  them with real branch snapshots.

## 4. Producing a staged snapshot

`worktree_sdk::remote::stage` runs these steps, in `wt stage [--remote R]`
and in `worktree-bg`:

1. **Pick the base.** The base is the server's tip of the current branch, or
   the last fetched `refs/remotes/<remote>/<branch>` when the server tip is
   not known locally.
2. **Diff.** Build the working-tree manifest and compare it with the base by
   path. The result is the changed files plus added, modified and deleted
   counts, and covers everything not yet pushed, committed or not.
3. **Upload.** Send the manifest and the blobs the server lacks (`Negotiate`
   then `UploadObjects`).
4. **Stage.** Call `SyncService.Stage(StageUploadRequest)` with these fields:
   - `tree_id`, `branch_name`;
   - `snapshot_id`: the head snapshot when the working tree is clean,
     otherwise a fresh UUID;
   - `files_changed`, `files_added`, `files_modified`, `files_deleted`;
   - optional `message`: the head snapshot's message when it is staged as is.

The daemon stages automatically after each auto-snapshot when `[sync]
auto_stage = true` (the default), provided the worktree has a remote and a
saved login.

The request's `tenant_id` and `account_id` are ignored. The server derives
both from the tree and the authenticated principal.

## 5. Server model

`domain.StagedSnapshot`:

| Field | Notes |
|---|---|
| `snapshot_id` | As sent |
| `tree_id`, `tenant_id` | From the tree |
| `account_id`, `author` | From the principal |
| `branch_name` | Defaults to the tree's default branch |
| `files_changed` | Normalized ('/'-separated, cleaned, de-duplicated, sorted). At most 10 000 may be sent; the first 1 000 are stored. |
| `files_added`, `files_modified`, `files_deleted` | Kept as reported, not truncated |
| `message` | Truncated to 4 KiB on a UTF-8 boundary |
| `staged_at` | Server time |

The model follows these rules:

- **One per author and branch.** There is **at most one staged snapshot per
  (tree, account, branch)**. A newer stage replaces the older one.
- **Write access required.** Staging requires `tree:write`. Validation
  problems (missing snapshot id, too many files) return `accepted: false`
  with an error.
- **Event.** A successful stage publishes `snapshot.staged` with the stored
  record as payload.
- **Metadata only.** The server stores the staged record. It does not create
  a `Snapshot` or move any branch.

## 6. Who can see staged work

Listing staged snapshots requires `tree:read` on the tree
(`staged.Service.List` / `Summary`). The audience is therefore:

| Tree | Can see staged metadata |
|---|---|
| `private` | members of the tenant and principals with a grant or allow policy |
| `internal` | every signed-in principal |
| `public` | everyone, **including anonymous clients** |

Staged visibility is all-or-nothing per tree. Branches have no read
permission of their own, so every reader of a tree sees staged work on every
branch.

## 7. Surfaces

| Surface | What it shows |
|---|---|
| `wt status --team` | The team's staged work on the current tree, from the remote |
| `wt trees staged TREE` | Staged snapshots of a server tree |
| `wt stage` / `wt stage --discard`, `wt unstage` (v0.5) | Publish, or withdraw, your own staged snapshot |
| `wt events --tenant T --tree TREE --type snapshot.staged --type staged.cleared` | Live stream |
| `GET /api/v1/trees/{t}/{tree}/staged?branch=` | `[StagedSnapshot]`, newest first |
| `GET /api/v1/trees/{t}/{tree}/staged/summary` | `[BranchActivity]` (§8) |
| `DELETE /api/v1/trees/{t}/{tree}/staged?branch=` (v0.5) | Remove your own entry. `tree:admin` holders may pass `account=`. |
| gRPC `ListStaged`, `Stage`, `Unstage` (v0.5) | Same, for the SDK |
| `GET /api/v1/events` (SSE), `GET /events/recent` | `snapshot.staged` and `staged.cleared` events, with the usual tenant or tree scoping |
| Admin panel | Dashboard live activity; tree detail **Staged** tab |

Example of `wt status --team` output:

```
Staged work on origin:
  alice  feature/oauth   3 files    2 minutes ago     WIP: token refresh
      + 1  ~ 2  - 0
  bob    fix/auth-panic  1 file     15 minutes ago
      + 0  ~ 1  - 0
```

The exact relative-time wording comes from the CLI's `ago` helper.

## 8. Activity summary and overlap detection

`GET {tree}/staged/summary` aggregates the live entries per branch, most
recent activity first (`staged.BranchActivity`):

| Field | Contents |
|---|---|
| `branch` | Branch name |
| `authors` | One `AuthorActivity` per account: snapshot id, message, counters, `files_listed`, `staged_at` |
| `files_added`, `files_modified`, `files_deleted` | Summed counters |
| `distinct_files` | Distinct listed paths across authors |
| `latest_at` | Most recent `staged_at` |
| `overlaps` | Paths staged on this branch that a **different** account has also staged, on this or any other branch. Each overlap lists its contributors (`account_id`, `author`, `branch`). |

Overlaps are **advisory early conflict warnings**. They do not block
anything. They compare the stored path lists, so paths beyond the first 1 000
of a very large staged snapshot are not considered.

## 9. Clearing and retention

| Event | Effect | `staged.cleared` reason |
|---|---|---|
| The author pushes the branch | The pusher's entry for that branch is removed (`ClearOnPush`) | `pushed` |
| Entry older than the retention window | Hidden from list and summary, deleted lazily on read and by GC | `expired` |
| `wt unstage`, `wt stage --discard`, `Unstage`, `DELETE {tree}/staged` (v0.5) | Entry removed | event `staged.cleared` |

The retention window is set by `--staged-retention` / `WT_STAGED_RETENTION`.
The default is `168h` (7 days), and `0` keeps entries forever. The staged
service applies the same 7-day window when reading.

## 10. Privacy controls

- **Turn off automatic staging** for a worktree with `wt config set
  sync.auto_stage false`. Work then becomes visible only when you run
  `wt stage` or push.
- **Pause the daemon** with `wt daemon pause` (and later `wt daemon resume`).
  This stops auto-snapshots, and with them auto-staging.
- **Withdraw** a published staged snapshot with `wt unstage` (v0.5).
- **Choose the audience.** Put work on a tree whose visibility matches the
  people who should see it. Staged metadata goes to every reader of the tree
  (§6).

## 11. Interaction with licenses and path policies

- **Paths are metadata.** Staged entries list paths, never contents. Paths are
  not filtered by license rules or `wt:path` read denies. A reader of the tree
  can see "alice changed `billing/pricing.rs`" even when they would receive
  that file only as a stub on pull.
- **No staged content.** The blobs uploaded with a staged snapshot are not
  served through any API. Content becomes readable only after a push, and then
  under the normal read checks: `tree:read`, path policies and license
  `CheckRead`.

## 12. Implementation status

| Area | Status |
|---|---|
| Tree visibility `private` / `internal` / `public` | Built |
| `wt stage`, daemon auto-stage, `Stage` / `ListStaged` RPCs | Built |
| Staged store (one entry per tree, account and branch), normalization, limits | Built (`internal/staged`) |
| `snapshot.staged` / `staged.cleared` events over SSE | Built |
| REST list and summary with overlap detection; `wt status --team`, `wt trees staged` | Built |
| Clear on push; lazy expiry; GC retention | Built |
| `Unstage` RPC, `wt unstage`, `wt stage --discard`, `DELETE {tree}/staged` | Built (v0.5) |

## 13. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **Multiple staged snapshots per user and branch**, with a status lifecycle
  (`Staged`, `Pushed`, `Cleared`, `Expired`) and per-user history. One entry
  per (tree, account, branch) is kept, and removed entries are gone.
- **Reading staged file contents**, with license checks on that content.
- **A push-time advisory prompt** ("alice has staged changes to the same
  file, proceed?"). Overlaps are available from the summary endpoint and the
  admin panel.
- **Per-branch staged privacy** (a `branch:read` permission), and hiding
  staged metadata from anonymous readers of public trees.
- **A per-tree opt-out** in `.wt-tree/config.toml`, and the `wt sync
  pause|resume` commands. Use `sync.auto_stage` and `wt daemon pause`.
- **The `wt staged [--user] [--branch]` and `wt staged clear [--all]`
  commands.** Use `wt trees staged` and `wt unstage`.
- **WebSocket endpoints** (`/api/repositories/:id/staged/live`). SSE covers
  real-time delivery.
- **Line-level or keystroke presence** (deliberately not planned).

## 14. References

- `services/server-go/internal/staged/{staged.go,summary.go}`
- `services/server-go/internal/syncsvc/trees.go` (`Stage`, `ListStaged`),
  `syncsvc/watch.go` (`Unstage`)
- `services/server-go/internal/httpapi/{trees.go,insights.go,staged_routes.go,events.go}`
- `services/server-go/internal/authz/authorizer.go` (visibility)
- `services/server-go/internal/domain/types.go` (`StagedSnapshot`,
  `Tree.Visibility`)
- `crates/worktree-sdk/src/remote/stage.rs`, `crates/worktree-bg/`
- `crates/worktree-cli/src/commands/{sync,status,trees,events}.rs`
- `crates/worktree-protocol/proto/sync.proto` (`StageUploadRequest`,
  `ListStaged`, `Unstage`)
- `docs/ENGINEERING_PLAN.md` §2, §6, §11, §13.3, §13.11
- Related specs: [IAM](../iam/IAM.md), [Sync](../sync/Sync.md),
  [BgProcess](../bgprocess/BgProcess.md),
  [License Compliance](../licensing/LicenseCompliance.md)
