# W0rkTree Forks Specification

> Status: built in v0.8 (`docs/ENGINEERING_PLAN.md` §16.2).

## Table of Contents

- [1. Overview](#1-overview)
- [2. Forking a tree](#2-forking-a-tree)
- [3. Shared objects and copied snapshots](#3-shared-objects-and-copied-snapshots)
- [4. Branch relations and fork sync](#4-branch-relations-and-fork-sync)
- [5. Merge requests from a fork](#5-merge-requests-from-a-fork)
- [6. Licenses](#6-licenses)
- [7. Deletion](#7-deletion)
- [8. Events, audit and storage](#8-events-audit-and-storage)
- [9. REST API](#9-rest-api)
- [10. CLI](#10-cli)
- [11. Admin panel](#11-admin-panel)
- [12. Implementation status](#12-implementation-status)
- [13. Roadmap](#13-roadmap)
- [14. References](#14-references)

---

## 1. Overview

A **fork** is a server tree created as a copy of another tree, its
**upstream**, usually in another tenant. It has the upstream's branches (all
or a chosen set) and tags, and its own history from then on. The fork
records where it came from in `Tree.fork_of` (the upstream's id).

People who may not push to a tree can still work on it: they fork it into
their own tenant, push there, and propose their changes back with a merge
request in the upstream (§5). The upstream's maintainers review and merge it
with their usual permissions. A fork follows its upstream with **fork
sync** (§4).

A fork is an ordinary tree in every other way: it has its own members,
protections, license rules, webhooks, mirrors and settings, none of which
are copied from the upstream. Clients clone, pull and push a fork like any
other tree.

The implementation is the `internal/fork` package of the Go server
(`fork.Service`, with `fork/snapcopy` for snapshot copies), the cross-tree
merge-request support in `internal/review/forks.go`, the REST routes in
`internal/httpapi/forks.go`, `wt fork` and `wt mr create --from` in the CLI
and the tree's **Forks** tab in the admin panel.

## 2. Forking a tree

`POST {tree}/forks` (`wt fork TENANT/TREE`) creates the fork:

| Input | Default | Rule |
|---|---|---|
| `tenant` | the caller's personal tenant | the caller needs `tree:create` there |
| `name` | the upstream's name | tree-name rules; unique in the tenant (409 otherwise) |
| `visibility` | the upstream's | never wider than the upstream's (`private` < `internal` < `public`) |
| `description` | the upstream's | at most 4096 bytes |
| `branches` | every branch | each must exist upstream |

Authorization: `tree:fork` on the upstream (role minimum **viewer**, tree
scoped) **and** `tree:create` in the destination tenant. Visibility does not
grant `tree:fork`: to fork a public tree, a caller needs a role or grant on
it (a policy can allow `tree:fork` broadly).

The destination tenant's `max_trees` quota applies (409 `quota_exceeded`).
When the fork leaves the upstream's tenant, the license redistribution rule
applies (§6).

What is copied:

- the chosen branches with their tips (empty branches stay empty) and
  descriptions; branch owners and protection flags are not copied;
- every tag (target, message, tagger, date);
- the snapshot records of their history (§3).

The fork's default branch is the upstream's when it is copied, else the
first copied branch. Protections, license rules, grants, policies, path
owners, webhooks, mirrors, releases, TODOs, CI statuses, merge requests and
staged snapshots are **not** copied.

A fork of a fork is allowed; its `fork_of` names its direct upstream.

## 3. Shared objects and copied snapshots

Objects (manifests, blobs, chunk lists, chunks) are content-addressed and
stored once for the whole server, so forking copies **no objects**: the
fork's snapshots reference the same manifests.

Snapshot *records* are tree-scoped (a snapshot belongs to one tree: pushes,
object downloads and the REST snapshot routes only accept records of the
tree they address), so the fork gets its own records. The copy of snapshot
`S` of tree `U` into tree `F`:

- keeps the manifest hash, author, message, creation and push times,
  auto-generated flag, branch name and dependencies;
- gets tree id `F` and parents (and `reverts`) mapped the same way;
- gets the id `S` itself had in `F` if `S` was copied from `F` (directly or
  through a chain of copies), else the deterministic UUID v5
  `DerivedID(F, S)`;
- carries no signature: signatures cover the snapshot id and parents, which
  changed. Tag copies are unsigned for the same reason.

Each copy records its origin in the `fork_snapshots` collection (key
`DocKey(F, copyID)` → `{origin_tree_id, origin_snapshot_id}`). Copies are
idempotent and only missing ancestors are written. Because origins are
followed back, history that travels upstream → fork → upstream keeps its
original ids: after a merge request from a fork is merged and the fork
syncs, the fork's branch contains the fork's own snapshots again, and
clones of the fork fast-forward.

Copying is done lazily where history crosses trees: on fork (branch and tag
history), on fork sync (new upstream snapshots) and when a merge request
from a fork is evaluated (§5).

## 4. Branch relations and fork sync

`GET {tree}/fork` (`wt fork upstream`) reports, for a fork, the upstream
(tenant, name, visibility, default branch), who forked it and when, the last
sync, and the relation of every branch to the upstream branch of the same
name:

| Status | Meaning |
|---|---|
| `up_to_date` | same content |
| `ahead` | the fork contains the upstream tip |
| `behind` | the fork can fast-forward to the upstream tip |
| `diverged` | each side has snapshots the other lacks |
| `fork_only` | the branch exists only in the fork |
| `upstream_only` | the branch exists only upstream |

Relations are computed without copying: through the copy origins of §3 and
ancestry. For a tree that is not a fork the answer is `{"fork": false}`.
`upstream_missing` means the upstream was deleted; `upstream_hidden` that
the caller may not read it (no details are shown then).

`POST {tree}/fork/sync` (`wt fork sync`) fast-forwards fork branches:

- without `branches`: every branch both trees have; upstream-only branches
  are reported as `upstream_only` and not created;
- with `branches`: those branches; one that exists only upstream is
  **created** in the fork.

Per branch the result is `fast_forwarded`, `created`, `up_to_date`, `ahead`
(nothing to do), `diverged` (listed in `diverged`; merge the upstream
locally and push) or `moved` (the fork branch moved during the sync; retry).
Branches move with a compare-and-swap. The caller needs `tree:write` on the
fork and `tree:read` on the upstream. The fork's branch protections do not
apply: a sync only fast-forwards to content the upstream already has. The
sync time is recorded (`last_sync_at`). A fork whose upstream was deleted
cannot sync (409).

## 5. Merge requests from a fork

A merge request lives in the tree it merges **into**. Opened with
`source_tenant` + `source_tree` (`wt mr create UPSTREAM --from
TENANT/TREE:BRANCH`), its source branch lives in another tree, recorded as
`MergeRequest.source_tree_id`. The source tree must be in the same **fork
network**: one tree is forked (directly or transitively) from the other, or
both from a common upstream (a deleted upstream still links its forks). The
caller must be able to read the source tree.

Who may open one: `mr:create` on the target tree, **or** `tree:fork` on the
target plus `tree:write` on the source. The second rule lets outside
contributors (viewers of the upstream) propose changes from their forks.
The source and target branch names may be equal (`main` of the fork into
`main` of the upstream). Duplicate detection includes the source tree.

The review service reads the source tip from the fork and copies its
history into the target tree (§3) before it evaluates anything, so
ancestry, mergeability, the three merge methods, conflicts, approvals
(which record the copied tip), path owners, CI checks and diffs all work on
snapshots of the target tree. `GET {tree}/merge-requests/{n}/source`
reports the source tree, the tip in the fork (`tip`) and the copied tip in
the target tree (`target_tip`); the CLI (`wt mr show --diff`) and the admin
panel diff `target_tip` against the target branch.

Merging needs the usual permissions **in the target tree** (`mr:merge`,
protections, approvals). A fast-forward moves the target branch to the
copied tip; merge and squash snapshots are written in the target tree.

Not supported on merge requests from forks: applying suggested changes from
the upstream (the source branch lives in the fork; apply them there and
push; 409) and automatic owner review requests on pushes to the fork.

When the source tree or branch is deleted, the merge request reports a
missing source and cannot be merged.

## 6. Licenses

Forking out of the upstream's tenant redistributes its files, so the same
rule as archives applies (`specs/licensing/LicenseCompliance.md` §8): every
path of every copied branch tip and tag target must be exportable both by
the caller and by the **destination tenant** evaluated as a non-member of
the upstream's tenant (grantee rules naming the destination tenant apply;
the caller's membership of the upstream's tenant does not lift the rule).
Otherwise the fork is refused with 403 `license_violation` naming the first
offending path and license. Forks within the upstream's tenant are not
restricted. Only the tips are checked, not every historical snapshot.

License rules are not copied to the fork.

## 7. Deletion

Deleting the upstream leaves its forks intact: the fork's snapshot records,
branches and tags are its own, and the shared objects stay referenced by
them (object garbage collection marks from every tree). `fork_of` keeps the
deleted upstream's id and `GET {tree}/fork` reports `upstream_missing`.

On `tree.deleted` the fork service drops the `forks` records of the deleted
tree (as upstream and as fork) and its `fork_snapshots` origins.

## 8. Events, audit and storage

| Event | When | Tree |
|---|---|---|
| `tree.forked` | a fork was created; payload `fork_tree_id`, `fork_tenant`, `fork_name`, `upstream_tenant`, `upstream_name`, `branches` | upstream |
| `tree.created` | the fork tree; payload `name`, `tenant`, `fork_of` | fork |
| `branch.pushed` / `branch.created` | fork sync moved / created a branch (`via: fork_sync`) | fork |

Audit: `tree.fork` in the destination tenant (resource the fork) and, for
cross-tenant forks, in the upstream's tenant (resource the upstream);
`fork.sync` in the fork's tenant.

| Collection | Key | Value |
|---|---|---|
| `forks` | `DocKey(upstreamTreeID, forkTreeID)` | `{upstream_tree_id, upstream_tenant_id, tree_id, tenant_id, branches, created_by, created_at, last_sync_at}` |
| `fork_snapshots` | `DocKey(treeID, snapshotID)` | `{origin_tree_id, origin_snapshot_id}` of a copied snapshot |

## 9. REST API

| Method and path | Action | Description |
|---|---|---|
| `POST {tree}/forks` | `tree:fork` + `tree:create` | Fork (§2); 201 with the new tree |
| `GET {tree}/forks` | `tree:read` | Direct forks the caller may read |
| `GET {tree}/fork` | `tree:read` | Upstream and branch relations (§4) |
| `POST {tree}/fork/sync` | `tree:write` (+ `tree:read` upstream) | Fork sync (§4) |
| `POST {tree}/merge-requests` | `mr:create` or `tree:fork` + source `tree:write` | With `source_tenant`, `source_tree` (§5) |
| `GET {tree}/merge-requests/{n}/source` | `tree:read` | Where the source lives, `tip`, `target_tip` |

`{tree}` is `/api/v1/trees/{tenant}/{tree}`. Servers without forks answer
501 `not_implemented` after the read check. The operations are documented
in `openapi.yaml`.

## 10. CLI

```text
wt fork acme/web                        # into your personal tenant, same name
wt fork acme/web --into team --name web-team --branch main --visibility private
wt fork list [TENANT/TREE]              # forks you can see
wt fork upstream [TENANT/TREE]          # upstream and branch relations
wt fork sync [TENANT/TREE] [--branch B]...   # exit 1 when a branch diverged
wt mr create acme/web --from bob/web:feature --target main --title "..."
```

`TENANT/TREE` defaults to the tree of the current worktree (its `origin`
remote). Every command supports `--json`.

## 11. Admin panel

The tree's **Forks** tab shows, for a fork, an upstream card (link, who
forked it and when, last sync, a branch relation table, **Sync with
upstream** and **Open merge request upstream** with source and target
branch pickers), a note when the upstream was deleted, the list of forks
the caller can see and a **Fork** dialog (destination tenant, name,
visibility limited to the upstream's, branches, description). Tree headers
of forks carry a "fork of TENANT/TREE" badge. A merge request from a fork
shows `TENANT/TREE:BRANCH` as its source and diffs the copied tip.

## 12. Implementation status

| Item | Status |
|---|---|
| Fork across tenants (`tree:fork` + `tree:create`), branch selection, tags, visibility rule, quota, rollback on failure | Built |
| Shared objects; snapshot copies with deterministic ids and origin tracking; round trips keep ids | Built |
| License redistribution rule for cross-tenant forks | Built |
| Branch relations, fork sync (fast-forward, create named branches, diverged report) | Built |
| Merge requests from forks: fork network check, contributor rule, copied source tips, all merge methods | Built |
| `upstream_missing`; cleanup of fork records and origins on `tree.deleted` | Built |
| Events, audit, OpenAPI operations, `wt fork`, `wt mr create --from`, admin Forks tab and fork badge | Built |

## 13. Roadmap

- Forking public trees without a role (today `tree:fork` needs a role,
  grant or policy).
- Network-wide views: all forks of a root, a fork graph, "compare across
  forks" without a merge request.
- Automatic fork sync (on upstream pushes or on a schedule) and rebase of
  diverged fork branches on the server.
- Re-signing copied snapshots and tags; verifying signatures across forks.
- Applying suggested changes to a fork's branch from the upstream's merge
  request (with `tree:write` on the fork).
- Owner review requests and notifications on pushes to the source fork of
  a cross-tree merge request.
- License checks over the full copied history instead of the tips.

## 14. References

- `docs/ENGINEERING_PLAN.md` §16.2 — binding contract
- `services/server-go/internal/fork/` — `fork.go` (fork), `sync.go` (relations, sync, cleanup), `snapcopy/` (snapshot copies)
- `services/server-go/internal/review/forks.go` — merge requests from forks
- `services/server-go/internal/httpapi/forks.go` — REST routes; `openapi.yaml` — operations
- `services/server-go/internal/domain/{types.go,v08.go}` — `Tree.ForkOf`, `MergeRequest.SourceTreeID`, `tree:fork`, `tree.forked`
- `crates/worktree-cli/src/commands/{fork.rs,mr.rs}` — CLI
- `apps/admin/src/pages/tree/{ForksTab.tsx,forksApi.ts}` — admin UI
- `crates/worktree-e2e/tests/e2e/forks.rs` — end-to-end scenario
- Related specs: [Tree](./Tree.md), [Server-side merges](../server/Merging.md),
  [Review workflow](../server/ReviewWorkflow.md),
  [License compliance](../licensing/LicenseCompliance.md), [IAM](../iam/IAM.md)
