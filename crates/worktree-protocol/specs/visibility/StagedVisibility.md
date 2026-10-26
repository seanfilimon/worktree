# Visibility Specification: Trees and Staged Snapshots

## Table of Contents

1. [Overview](#1-overview)
2. [Tree visibility](#2-tree-visibility)
3. [Staged snapshots: the idea](#3-staged-snapshots-the-idea)
4. [Producing a staged snapshot](#4-producing-a-staged-snapshot)
5. [Server model: slots and labels](#5-server-model-slots-and-labels)
6. [Lifecycle and history](#6-lifecycle-and-history)
7. [Who can see staged work](#7-who-can-see-staged-work)
8. [Browsing and checking out staged work](#8-browsing-and-checking-out-staged-work)
9. [Activity summary, overlaps and the push-time advisory](#9-activity-summary-overlaps-and-the-push-time-advisory)
10. [Surfaces](#10-surfaces)
11. [Privacy controls](#11-privacy-controls)
12. [Interaction with licenses and path policies](#12-interaction-with-licenses-and-path-policies)
13. [Implementation status](#13-implementation-status)
14. [Roadmap](#14-roadmap)
15. [References](#15-references)

---

## 1. Overview

This spec covers two kinds of visibility on a W0rkTree server:

- **Tree visibility** (`private`, `internal`, `public`). It decides who may
  read a tree beyond the roles and grants of [IAM](../iam/IAM.md).
- **Staged snapshot visibility.** Signed-in teammates can see who is working
  on which files, on which branch, before anything is pushed, browse that
  work and check it out locally.

The guiding rule for staged snapshots is *staged ≠ pushed, visible ≠ merged*.
Staged snapshots are team-visible work in progress. They never enter branch
history on the server.

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
branches and tags and subscribing to the tree's events. Staged work needs
`tree:read` **and** a signed-in principal (§7).

## 3. Staged snapshots: the idea

In Git, work is invisible until it is pushed. Two people can edit the same
file for days and only find out at merge time. W0rkTree closes that gap at
snapshot granularity:

- **What is published.** The background daemon (or `wt stage`) publishes a
  **staged snapshot**: the files the author has changed relative to the
  shared branch head, counts, an optional message and the uploaded working
  manifest (so the work can be browsed and checked out).
- **Who sees it.** Signed-in readers of the tree, through `wt stage list`,
  `wt status --team`, the admin panel, SSE events and the API.
- **What is never published.** Keystrokes, cursor positions and line-level
  presence. Granularity is a snapshot, not a character.
- **Not history.** Staged snapshots are not part of branch history. A push
  replaces them with real branch snapshots.

## 4. Producing a staged snapshot

`worktree_sdk::remote::stage` (default slot) and `remote::stage_label`
(named slot) run these steps, in `wt stage [--label L] [--remote R]` and in
`worktree-bg`:

1. **Pick the base.** The base is the server's tip of the current branch, or
   the last fetched `refs/remotes/<remote>/<branch>` when the server tip is
   not known locally.
2. **Diff.** Build the working-tree manifest and compare it with the base by
   path. The result is the changed files plus added, modified and deleted
   counts, and covers everything not yet pushed, committed or not.
3. **Upload.** Send the manifest and the blobs the server lacks
   (`MissingObjects` then `UploadObjects`).
4. **Stage.** Call `SyncService.Stage(StageUploadRequest)` with:
   - `tree_id`, `branch_name`, `label` (v0.8; empty = the default slot);
   - `snapshot_id`: the head snapshot when the working tree is clean,
     otherwise a fresh UUID;
   - `files_changed`, `files_added`, `files_modified`, `files_deleted`;
   - optional `message`: the head snapshot's message when it is staged as is;
   - `manifest_hash`: the uploaded working manifest.

The daemon stages the default slot automatically after each auto-snapshot
when `[sync] auto_stage = true` (the default), provided the worktree has a
remote and a saved login. Labels are only set explicitly (`wt stage --label`).

The request's `tenant_id` and `account_id` are ignored. The server derives
both from the tree and the authenticated principal.

## 5. Server model: slots and labels

`domain.StagedSnapshot` (live entries, MetaStore `staged` bucket):

| Field | Notes |
|---|---|
| `snapshot_id` | As sent |
| `tree_id`, `tenant_id` | From the tree |
| `account_id`, `author` | From the principal |
| `branch_name` | Defaults to the tree's default branch |
| `label` | The slot (v0.8): `""` (default) or `[a-z0-9][a-z0-9_.-]{0,63}` |
| `files_changed` | Normalized ('/'-separated, cleaned, de-duplicated, sorted). At most 100 000 may be sent; the first 1 000 are stored. |
| `files_added`, `files_modified`, `files_deleted` | Kept as reported, not truncated |
| `message` | Truncated to 4 KiB on a UTF-8 boundary |
| `staged_at` | Server time |
| `manifest_hash` | Hex hash of the working manifest; must already be stored (`FailedPrecondition` otherwise) |

The model follows these rules:

- **Slots.** Every (tree, account, branch) has **up to 10 slots**, keyed by
  `label`. A newer stage of the same slot replaces the older one; other
  slots are untouched. Staging an eleventh label is refused
  (`accepted: false`, "max 10 per branch; unstage one first"); expired
  entries do not count.
- **Storage key.** `tree\0account\0branch` for the default slot (the pre-v0.8
  key) and `tree\0account\0branch\0label` for named slots. Entries stored
  before labels existed are therefore the default slot without any data
  migration.
- **Write access required.** Staging requires `tree:write`. Validation
  problems (missing snapshot id, invalid label, too many files or labels)
  return `accepted: false` with an error.
- **Event.** A successful stage publishes `snapshot.staged` with the stored
  record (including `label`) as payload.
- **Metadata only.** The server stores the staged record. It does not create
  a `Snapshot` or move any branch.

## 6. Lifecycle and history

Status lifecycle: `staged` → `pushed` | `cleared` | `expired`.

| Transition | Trigger | Who |
|---|---|---|
| `staged` → `pushed` | The author pushes the branch (`ClearOnPush`). The **default slot** always ends; a **labeled slot** ends when its snapshot is the new tip or its working manifest is the new tip's manifest (that work was pushed). Other labels stay live. | the pusher |
| `staged` → `cleared` | `wt unstage`, `wt stage --discard`, gRPC `Unstage`, `DELETE {tree}/staged` (own entries; another account's with `tree:admin`) | the caller |
| `staged` → `expired` | Older than the time-to-live: server flag `--staged-ttl` / env `WT_STAGED_TTL` (alias `--staged-retention` / `WT_STAGED_RETENTION`), default `720h` (30 days), `0` disables expiry | `system` |

- **History.** Every ended entry is copied into the DocStore collection
  `staged_history` (key `DocKey(treeID, ended_at, id)`) with `status`,
  `ended_at`, `ended_by` and, for pushes, `pushed_tip`. History is kept 30
  days, then pruned.
- **Expiry.** The staged service runs a sweeper (`staged.Service.Start`,
  hourly) that expires stale entries and prunes history; reads also hide and
  lazily expire stale entries. GC retention (`gc.Config.StagedRetention`,
  the same value) is a backstop that deletes without a history record.
- **Events.** Each transition publishes `staged.cleared` with payload
  `{tree_id, tenant_id, account_id, author, branch_name, label, snapshot_id,
  reason: pushed|unstaged|expired, status: pushed|cleared|expired}`.
- **Tree deletion.** `tree.deleted` removes the tree's history; live entries
  go with the tree's metadata.

## 7. Who can see staged work

Listing, browsing and checking out staged snapshots require `tree:read` on
the tree **and a signed-in principal**:

| Tree | Can see staged metadata |
|---|---|
| `private` | members of the tenant and principals with a grant or allow policy |
| `internal` | every signed-in principal |
| `public` | every signed-in principal; **anonymous readers see none** |

For anonymous callers `GET {tree}/staged`, `.../summary` and
`.../history` answer empty lists, `.../{id}/files` answers 404,
`.../overlaps` answers 401, gRPC `ListStaged` answers an empty list, the
tree summary reports `staged_count: 0` and staged objects are not
downloadable. Branches have no read permission of their own, so every
signed-in reader of a tree sees staged work on every branch.

## 8. Browsing and checking out staged work

- **Files.** `GET {tree}/staged/{snapshot}/files` lists the working manifest
  of a live entry (any author, any branch) with `restricted_count`;
  `GET {tree}/staged/{snapshot}/files/{path}` serves one file with the blob
  endpoint's checks (403 `path_denied`, 451 `license_restricted`, `Range`,
  `ETag`). Entries without an uploaded manifest answer 404.
- **`wt stage show [LABEL] [--user U] [--branch B] [--files]`** prints one
  staged snapshot (yours by default) with its changed files, and with
  `--files` every file of the working state.
- **`wt checkout --staged USER[/LABEL] [-b NAME] [--branch B] [--force]`**
  downloads the staged working state (manifest, blobs, chunk lists, chunks)
  with `DownloadObjects`, records it as a **local** snapshot (author = the
  teammate, created at the staging time, parent = the staged branch's tip
  when it is known locally, unsigned) on a new branch
  (default `staged/USER[/LABEL]`) and switches to it. It refuses a dirty
  working tree (unless `--force`), an existing branch name and ambiguous
  matches (the same user/label staged on several branches: pass `--branch`).
  Nothing changes on the server.
- **Admin panel.** The tree **Staged** tab groups live entries by user and
  label with a file browser (a sheet over the files endpoints) and shows the
  history with a status filter.

## 9. Activity summary, overlaps and the push-time advisory

`GET {tree}/staged/summary` aggregates the live entries per branch, most
recent activity first (`staged.BranchActivity`):

| Field | Contents |
|---|---|
| `branch` | Branch name |
| `authors` | One `AuthorActivity` per (account, label): snapshot id, `label`, message, counters, `files_listed`, `staged_at` |
| `files_added`, `files_modified`, `files_deleted` | Summed counters |
| `distinct_files` | Distinct listed paths across authors |
| `latest_at` | Most recent `staged_at` |
| `overlaps` | Paths staged on this branch that a **different** account has also staged, on this or any other branch, with contributors (`account_id`, `author`, `branch`, `label`) |

**Push-time advisory.** `GET {tree}/staged/overlaps?paths=a,b&path=c`
(sign-in required, at most 5 000 paths) returns the live staged snapshots of
**other** accounts, on any branch, whose changed files include some of the
given paths: `{overlaps: [{account_id, author, branch, label, snapshot_id,
message, staged_at, paths}]}`. After a successful push, `wt push` computes
the pushed paths (old remote tip → new tip), asks the server in batches of
200 and prints for example:

```
note: alice has staged changes to src/auth.rs, src/token.rs (on main, label oauth, 2 minutes ago)
```

The advisory never blocks or fails a push: an unreachable server, an older
server or an anonymous remote simply print nothing. `wt push --json` adds
`staged_overlaps`. `PushResponse` carries no warnings field; the REST call
is the only channel.

Overlaps compare the stored path lists, so paths beyond the first 1 000 of a
very large staged snapshot are not considered.

## 10. Surfaces

| Surface | What it does |
|---|---|
| `wt stage [--label L]` | Publish your working state into the default slot or slot `L` |
| `wt stage list [--branch B] [--mine] [--history]` | The team's staged snapshots grouped by user, with labels; `--history` adds the last 30 days of ended entries |
| `wt stage show [LABEL] [--user U] [--branch B] [--files]` | One staged snapshot and its files (§8) |
| `wt unstage [--branch B] [--label L \| --all-labels \| --all]`, `wt stage --discard [--label L]` | Remove the default slot of the branch, slot `L`, every slot of the branch, or everything |
| `wt checkout --staged USER[/LABEL]` | Check a teammate's staged work out into a new local branch (§8) |
| `wt push` | Prints the staged-overlap advisory (§9) |
| `wt status --team`, `wt trees staged TREE` | The team's staged work |
| `wt events --tree TREE --type snapshot.staged --type staged.cleared` | Live stream |
| `GET /api/v1/trees/{t}/{tree}/staged?branch=` | Live `[StagedSnapshot]`, newest first |
| `GET .../staged/summary` | `[BranchActivity]` (§9) |
| `GET .../staged/history?branch=&account=&status=&limit=` | `{history: [StagedHistoryEntry]}`, newest first, 30 days |
| `GET .../staged/overlaps?paths=&path=` | `{overlaps: [StagedOverlap]}` (§9) |
| `GET .../staged/{snapshot}/files[/{path}]` | Browse a staged working state (§8) |
| `DELETE .../staged?branch=&label=&account=` | Remove entries: `label` (with `branch`; `label=` = default slot) one slot, otherwise every label of the branch (or every branch). `account=` needs `tree:admin`. Audited `staged.unstage`. |
| gRPC `Stage` (`label`), `ListStaged` (`StagedInfo.label`, `manifest_hash`), `Unstage` | `Unstage` has no label field: it removes every label of `branch_name` (every branch when empty) |
| Admin panel | Dashboard live activity; tree **Staged** tab (§8) |

## 11. Privacy controls

- **Turn off automatic staging** for a worktree with `wt config set
  sync.auto_stage false`. Work then becomes visible only when you run
  `wt stage` or push.
- **Pause the daemon** with `wt daemon pause` (and later `wt daemon resume`).
  This stops auto-snapshots, and with them auto-staging.
- **Withdraw** a published staged snapshot with `wt unstage` (the removal is
  recorded in the history as `cleared`).
- **Choose the audience.** Put work on a tree whose visibility matches the
  people who should see it. Staged metadata goes to every signed-in reader
  of the tree (§7), never to anonymous readers.

## 12. Interaction with licenses and path policies

- **Paths are filtered.** Paths the caller may not read (`wt:path` read
  denies, license rules) are left out of `files_changed` in the list, the
  history and the files response, and are never reported by the overlaps
  endpoint. The summary endpoint does not filter paths yet (§14).
- **Content.** The files endpoints apply the same checks as blob reads, and
  `DownloadObjects` applies license `CheckRead` per path, so `wt checkout
  --staged` fails clearly when the staged state contains content the caller
  may not download.
- **GC.** Live entries' manifests are GC roots; ended entries (history) are
  metadata only and do not keep content alive.

## 13. Implementation status

| Area | Status |
|---|---|
| Tree visibility `private` / `internal` / `public` | Built |
| `wt stage`, daemon auto-stage, `Stage` / `ListStaged` RPCs | Built |
| Labeled slots (≤ 10 per account and branch), default slot compatible with pre-v0.8 data | Built (v0.8) |
| Status lifecycle and 30-day history (`staged_history`), TTL sweeper (`--staged-ttl`, default 30 days) | Built (v0.8) |
| Staged metadata hidden from anonymous readers; restricted paths filtered from listings | Built (v0.8) |
| Browsing staged files (REST, `wt stage show`, admin file browser) and `wt checkout --staged` | Built (v0.8) |
| Push-time advisory (`GET {tree}/staged/overlaps`, printed by `wt push`) | Built (v0.8) |
| `wt stage list`, `wt unstage --label/--all-labels`, admin Staged tab grouped by user and label with history | Built (v0.8) |
| `snapshot.staged` / `staged.cleared` events over SSE | Built |
| REST list and summary with overlap detection; `wt status --team`, `wt trees staged` | Built |
| `Unstage` RPC, `wt unstage`, `wt stage --discard`, `DELETE {tree}/staged` | Built (v0.5) |

## 14. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **Labels on the gRPC `Unstage` request** (it removes every label of a
  branch; the CLI uses `DELETE {tree}/staged?label=` for one slot) and
  **warnings on `PushResponse`** (the advisory uses the REST endpoint).
- **Path filtering in `GET {tree}/staged/summary`** (the list, history and
  overlaps endpoints filter restricted paths).
- **Browsing ended (history) entries' contents**: history is metadata only.
- **Per-branch staged privacy** (a `branch:read` permission).
- **A per-tree opt-out** in `.wt-tree/config.toml`, and the `wt sync
  pause|resume` commands. Use `sync.auto_stage` and `wt daemon pause`.
- **WebSocket endpoints** (`/api/repositories/:id/staged/live`). SSE covers
  real-time delivery.
- **Line-level or keystroke presence** (deliberately not planned).

## 15. References

- `services/server-go/internal/staged/{staged.go,labels.go,history.go,sweep.go,overlaps.go,summary.go,unstage.go}`
- `services/server-go/internal/syncsvc/trees.go` (`Stage`, `ListStaged`),
  `syncsvc/watch.go` (`Unstage`), `syncsvc/convert.go` (`stagedToPB`)
- `services/server-go/internal/httpapi/{staged_routes.go,staged_v2.go,trees.go,insights.go,events.go}`,
  `httpapi/openapi.yaml`
- `services/server-go/internal/meta/{bolt.go,memory.go,meta.go}` (staged
  keys), `internal/gc/gc.go` (retention backstop), `internal/config`
  (`--staged-ttl`)
- `services/server-go/internal/domain/types.go` (`StagedSnapshot`,
  `Tree.Visibility`)
- `crates/worktree-sdk/src/remote/{stage.rs,stage_v2.rs,convert.rs}`,
  `crates/worktree-bg/`
- `crates/worktree-cli/src/commands/{sync,staged,status,trees,events}.rs`
- `apps/admin/src/pages/tree/StagedTab.tsx`, `apps/admin/src/pages/tree/v08/stagedApi.ts`,
  `apps/admin/src/pages/tree/v06/StagedFilesSheet.tsx`
- `crates/worktree-protocol/proto/sync.proto` (`StageUploadRequest`,
  `StagedInfo`, `ListStaged`, `Unstage`)
- `crates/worktree-e2e/tests/e2e/{staged_visibility,staged_v2}.rs`
- `docs/ENGINEERING_PLAN.md` §2, §6, §11, §13.3, §13.11, §16.8
- Related specs: [IAM](../iam/IAM.md), [Sync](../sync/Sync.md),
  [BgProcess](../bgprocess/BgProcess.md),
  [License Compliance](../licensing/LicenseCompliance.md)
