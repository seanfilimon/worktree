# Policy History Specification

## Table of Contents

1. [Overview](#1-overview)
2. [What records a version](#2-what-records-a-version)
3. [The version record](#3-the-version-record)
4. [Version numbers and the synthetic version 1](#4-version-numbers-and-the-synthetic-version-1)
5. [Statement-level diff](#5-statement-level-diff)
6. [Rollback and restore](#6-rollback-and-restore)
7. [REST API](#7-rest-api)
8. [CLI](#8-cli)
9. [Events and audit](#9-events-and-audit)
10. [Implementation status](#10-implementation-status)
11. [Roadmap](#11-roadmap)
12. [References](#12-references)

## 1. Overview

Every tenant policy (see [IAM §8](./IAM.md)) keeps a version history on
the server. Each change appends an immutable `PolicyVersion` holding the
whole policy as it was after the change, who made it, when and why. The
history answers "who changed this deny statement and when", shows the
statement-level difference between any two versions, and lets an admin
roll a policy back to an earlier version — including bringing back a
deleted policy.

History is kept for policies changed through the REST API and through
declarative access documents ([DeclarativeAccess](./DeclarativeAccess.md))
alike, so a policy that moves between the two keeps one continuous
history.

## 2. What records a version

| Write path | `change` | Policy stored in the version |
|---|---|---|
| `POST /tenants/{t}/policies` | `create` | the new policy |
| `PUT /tenants/{t}/policies/{id}` (also `wt policy update/enable/disable`) | `update` | the policy after the replace |
| `DELETE /tenants/{t}/policies/{id}` | `delete` | the policy as it was before the delete |
| `POST /tenants/{t}/policies/{id}/rollback` | `rollback` | the policy after the rollback |
| `wt access apply` / `SyncAccessConfig` / `POST .../access-config/apply` creating, updating (including adopting a REST policy) or deleting a config-managed policy | `create` / `update` / `delete` | as above; `comment` = `applied from the access document of tenant <slug>` (or `tree <tenant>/<tree>`) |

An apply that leaves a policy unchanged records nothing for it. Dry runs
(`plan`) record nothing. Forced REST edits of config-managed policies
(`?force=true`) are recorded like any other update.

Recording happens after the policy write succeeded. If storing the
version fails, the write is not undone: the failure is logged by the
server and the audit record carries `history_version: "0"`.

## 3. The version record

```json
{
  "tenant_id": "6f0c…",
  "policy_id": "d082…",
  "version": 3,
  "change": "rollback",
  "policy": { "id": "d082…", "name": "protect-main", "enabled": true, "statements": [ … ], "version": 3, … },
  "changed_by": "olga",
  "changed_at": "2026-10-25T12:00:00Z",
  "comment": "rollback to version 1: undo the rename"
}
```

Versions are stored in the server document collection `policy_versions`
under the key `DocKey(tenant_id, policy_id, "%08d" version)`, so a policy's
versions list in order. They are never modified or deleted, and they stay
after the policy itself is deleted.

## 4. Version numbers and the synthetic version 1

`version` is the history's own sequence, `1, 2, 3, …` per policy with no
gaps. For a policy created while history recording exists, version *n* of
a create, update or rollback is also the policy's `Policy.version` counter
at that point; deletes take the next history number. (A policy restored by
rollback starts its `Policy.version` counter at 1 again, while the history
numbering continues.)

Policies that existed before history recording have no stored versions.
For them the server materializes a **synthetic version 1** — once, and
persistently — either the first time their history is read (`versions`,
`diff`, `rollback`) or just before their first recorded change, whichever
comes first. The synthetic version holds the policy as it was at that
moment and has `change: "create"`, `changed_by` = the policy's
`created_by`, `changed_at` = its `updated_at`, and the comment
`synthetic version: the policy existed before version history was recorded`.
Its `policy.version` keeps the policy's own counter, which may be larger
than 1.

## 5. Statement-level diff

`GET …/diff?from=A&to=B` compares the policies of two versions.

- `to` defaults to the latest version, `from` to `to - 1`. `from=0` is the
  empty policy (everything added). `from` may be greater than `to`
  (reverse diff). A `delete` version stands for the empty policy on
  either side.
- **Matching.** Statements with a `sid` are matched by sid; when a sid
  occurs more than once, its *n*-th occurrence in `from` is matched with
  its *n*-th occurrence in `to`. Statements without a sid are matched by
  position among the statements without a sid: the *n*-th sid-less
  statement of `from` with the *n*-th sid-less statement of `to`, so
  inserting or removing sid'd statements does not shift them.
- **Comparison** is canonical: empty and missing lists are equal, effects
  compare case-insensitively, timestamps compare in UTC.
- **Changes** list `add` and `change` entries in the order of the `to`
  version's statements, followed by `remove` entries in the order of the
  `from` version. `index` is the 0-based position in `to` (add, change)
  or `from` (remove). Add and remove carry `statement`; change carries
  `before` and `after`.
- **Fields** list differences of `name`, `description` and `enabled`
  (`before`/`after` are `null` for the empty policy).

```json
{
  "from": 1, "to": 2,
  "changes": [
    {"op": "add",    "sid": "new",  "index": 0, "statement": { … }},
    {"op": "change", "sid": "",     "index": 2, "before": { … }, "after": { … }},
    {"op": "remove", "sid": "gone", "index": 1, "statement": { … }}
  ],
  "fields": [{"field": "enabled", "before": true, "after": false}]
}
```

## 6. Rollback and restore

`POST …/rollback {"version": N, "comment"?}` copies the name,
description, enabled flag and statements of version *N* onto the policy
and records a new `rollback` version with the comment
`rollback to version N` (followed by `: <comment>` when given). The
policy's id, `managed_by`, `tree_id`, `created_by` and `created_at` are
kept; `Policy.version` is incremented as by `PUT`.

- The statements are re-validated against the current action catalog
  (`400` when invalid) and the name must still be free (`409`).
- Rolling back to the content the policy already has is `409 conflict`.
- **Restore.** When the policy is deleted, the rollback re-creates it with
  its original id from version *N* (any version, including the `delete`
  version, which holds the pre-delete state). The restored policy leaves
  the deleted list.
- **Config-managed policies** (`managed_by: "config"`, of the live policy
  or, for a restore, of version *N*) are refused with
  `409 {"code":"managed_by_config"}` unless `?force=true` is passed by a
  tenant owner or server admin (`403` otherwise), as for other REST edits
  ([DeclarativeAccess §8](./DeclarativeAccess.md)). The next
  `wt access apply` reconciles a forced rollback.

## 7. REST API

All under `/api/v1`; reads need `iam:read` on the tenant, rollback
`iam:admin`. Anonymous callers get `401`.

```
GET  /tenants/{t}/policies/deleted                    -> [PolicyVersion]   latest (delete) version of each deleted policy, newest first
GET  /tenants/{t}/policies/{id}/versions              -> [PolicyVersion]   oldest first; works for deleted policies; 404 when unknown
GET  /tenants/{t}/policies/{id}/versions/{v}          -> PolicyVersion     400 bad number, 404 unknown
GET  /tenants/{t}/policies/{id}/diff?from=&to=        -> {from, to, changes, fields}
POST /tenants/{t}/policies/{id}/rollback[?force=true] {version, comment?} -> PolicyVersion (the new version)
```

The existing policy write endpoints add `history_version` to their audit
details.

## 8. CLI

```
wt policy history ID [--tenant T]
wt policy show-version ID VERSION [--tenant T]
wt policy diff ID [--from V] [--to V] [--tenant T]
wt policy rollback ID VERSION [-m MSG] [--force] [--tenant T]
wt policy deleted [--tenant T]
```

`--tenant` defaults to the tenant of the worktree's origin remote. All
commands honour `--json` (the server JSON) and `--server`. `diff` prints
`~ field: before -> after`, `+ statement N (sid): …`, `- statement N
(sid): …` and `~ statement N (sid)` followed by the old and new
statement, with 1-based statement numbers as in `wt policy show`.

## 9. Events and audit

Every recorded version (including those of access applies) publishes
`policy.changed` on the event bus (and so to webhooks and `wt events`):

```json
{"type": "policy.changed", "tenant_id": "…", "tree_id": "<for tree-document policies>", "actor": "olga",
 "payload": {"policy_id": "…", "name": "protect-main", "version": 3, "change": "rollback", "comment": "…"}}
```

The synthetic version 1 publishes nothing. Rollbacks are audited as
`policy.rollback` with details `name`, `version` (the policy counter),
`to_version`, `history_version`, `restored` and `comment`; creates,
updates and deletes keep their `policy.create|update|delete` audit records
with an added `history_version`; access applies keep `access.applied`.

## 10. Implementation status

| Item | Status |
|---|---|
| Versions on REST create, update, delete; on access applies | Built (v0.6) |
| Synthetic version 1 for older policies | Built (v0.6) |
| `versions`, `versions/{v}`, `diff`, `deleted` endpoints | Built (v0.6) |
| Rollback, restore of deleted policies, managed-by-config guard | Built (v0.6) |
| `policy.changed` event, `policy.rollback` audit | Built (v0.6) |
| `wt policy history / show-version / diff / rollback / deleted` | Built (v0.6) |
| Admin panel history drawer and diff view | Owned by the admin workstream (§14.9) |

## 11. Roadmap

- **Version history of custom roles** and of access documents themselves
  (only the last applied `AccessConfigDoc` is kept).
- **Retention.** Versions are kept forever; pruning old versions (by age
  or count) is not built.
- **Policies of deleted trees.** Config-managed tree policies removed
  because their tree was deleted are not recorded as `delete` versions.
- **Rollback of a whole access apply** as one operation; rollbacks are per
  policy.

## 12. References

- `services/server-go/internal/policyhist/` (recorder, synthetic version,
  diff, apply tracking)
- `services/server-go/internal/httpapi/iam_policy_history.go`,
  `iam_policies.go`
- `services/server-go/internal/accesscfg/accesscfg.go` (the
  `policyhist.Track` call in `Apply`)
- `services/server-go/internal/domain/v06.go` (`PolicyVersion`,
  `EventPolicyChanged`)
- `crates/worktree-cli/src/commands/policy.rs`
- `docs/ENGINEERING_PLAN.md` §14.7
