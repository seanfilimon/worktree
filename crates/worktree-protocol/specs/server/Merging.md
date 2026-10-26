# Server-side Merges

How the server merges a merge request into its target branch: the three
merge methods, how one is chosen, how branch protection restricts them, the
three-way merge the server runs, and the REST and CLI surface
(`docs/ENGINEERING_PLAN.md` §14.1). Reviews, CI and the readiness gates that
apply before any merge are described in [Server](./Server.md) §9.

## 1. Merge methods

| Method | Result on the target branch | Snapshot written by the server |
|---|---|---|
| `fast-forward` | The tip moves to the source tip | None |
| `merge` | The tip moves to a new merge snapshot | Parents `[target tip, source tip]`, manifest = three-way merge |
| `squash` | The tip moves to a new snapshot | Parent `[target tip]`, manifest = three-way merge (the combined changes) |

- The target always moves with compare-and-swap from the tip the merge was
  computed against. A target that moved meanwhile fails with `409 conflict`
  ("moved during the merge; retry"); a retry recomputes the merge.
- A target that already contains the source (same tip, or the source tip is an
  ancestor of the target tip) is up to date: every allowed method succeeds
  without moving the branch and the merge request is marked merged at the
  source tip.
- `merge` on a fast-forwardable request writes a merge snapshot anyway (a
  "no-ff" merge); its manifest is the source tree. `squash` on a
  fast-forwardable request writes one snapshot with the source tree on top of
  the target.
- `merge` and `squash` need a non-empty target branch and an object store on
  the server (always configured by `worktree-server`); otherwise only
  `fast-forward` is offered.
- `merge`/`squash` snapshots get a new UUID id, `branch_name` = target,
  `author` = the merger's username, `pushed_by` = the merger's account,
  `created_at` = `pushed_at` = the merge time, no signature. The merged blobs
  and the manifest (deterministic protobuf encoding) are stored before the
  snapshot; clients fetch them like any pushed snapshot.
- Messages: `{"message"}` when given (trimmed, at most 64 KiB of UTF-8, no
  NUL); otherwise `Merge branch '<source>' into <target> (!N)` for `merge` and
  the merge request title for `squash`. Fast-forwards ignore the message.

## 2. Choosing the method

The request body is optional: `{"method"?: "fast-forward"|"merge"|"squash",
"message"?}`. An empty body or `{}` selects the **default method: the first of
`fast-forward`, `merge`, `squash` that is allowed and possible** — fast-forward
when the target is an ancestor of the source, else a merge snapshot.

A method is **allowed** when branch protection admits it (§3). It is
**possible** when, additionally:

| Method | Possible when |
|---|---|
| `fast-forward` | the target is an ancestor of the source (or empty), and with `require_linear` no incoming snapshot is a merge snapshot |
| `merge`, `squash` | the target is non-empty and the three-way merge has no conflicts |

The readiness gates of [Server](./Server.md) §9 (approvals, change requests,
CI, TODOs, linked groups) are checked first and do not depend on the method.

## 3. Protection restrictions

The effective protection of the target (every matching rule, most restrictive
combination) restricts the methods:

- `merge_methods` (on `BranchProtection`, `PUT {tree}/protections`): the
  methods allowed into matching branches; empty = all. Lists of several
  matching rules intersect; an empty intersection allows nothing. Values are
  validated on write (`400` for unknown methods) and stored deduplicated in
  preference order.
- `require_linear` forbids `merge` (squash and fast-forward keep history
  linear; a fast-forward that would bring merge snapshots is refused, see §2).
- `require_signed` forbids `merge` and `squash`: the server cannot sign the
  snapshots it writes.

Requesting a method protection refuses answers `409 merge_method_not_allowed`.
When no method is allowed and possible, the default selection fails with the
most useful reason: conflicts (`409 merge_conflict`), divergence with no
three-way method available (`409 conflict`), linear history (`409
merge_blocked`), or `409 merge_method_not_allowed`.

## 4. Three-way merge (`internal/merge3`)

The merge of source ("theirs") into target ("ours") against their merge base.
The semantics match the Rust SDK's local merge (`crates/worktree-sdk/src/merge`)
so a server merge equals the one `wt merge` would produce.

- **Merge base**: the best common ancestor — reachable from both tips and not
  an ancestor of another common ancestor; ties broken by the shortest
  breadth-first distance from the target tip, then by id. Unrelated histories
  merge against an empty manifest.
- **Per path** (`b` base, `o` ours, `t` theirs; entries compare path, hash,
  size, executable bit and chunked flag):

| Case | Result |
|---|---|
| `o == t` | keep |
| added on one side only | take it |
| added on both sides, same content, different executable bit | conflict `executable` |
| added on both sides, different content | conflict `add_add` |
| deleted on one side, unchanged on the other | delete |
| deleted on one side, changed on the other | conflict `modify_delete` |
| changed on one side only | take that side |
| content changed on one side, executable bit on the other | changed content with the toggled bit |
| content changed on both sides | merge the content (below) |

- **Content merge**: chunked (large, > 8 MiB) files conflict as `chunked`;
  binary files (a NUL byte in the first 8 KiB, or invalid UTF-8) conflict as
  `binary`; text files merge line-wise with diff3: the base is aligned with
  each side by a longest common subsequence (Myers, linear space, a cost limit
  for pathological inputs), base lines matched on both sides are stable, each
  stretch between stable lines takes the changed side, identical changes once,
  and otherwise conflicts (`content`); edits on adjacent lines conflict. The
  executable bit follows the side that changed it.
- **File/directory**: a merged file at a path the other side uses as a
  directory (`a` next to `a/b`) is a `file_directory` conflict.
- The server never writes conflict markers: a merge with any conflict is
  refused and resolved locally (`wt merge` the target into the source, resolve,
  push).

## 5. REST

All under `/api/v1/trees/{tenant}/{tree}`.

| Endpoint | Description |
|---|---|
| `POST merge-requests/{n}/merge` | Body `{"method"?, "message"?}` (optional). Requires `mr:merge`. `200` with the merged `MergeRequest` (`merge_method`, `merged_snapshot` = the new snapshot for merge/squash, the source tip otherwise) |
| `GET merge-requests/{n}/mergeability` | Requires `tree:read`. `{"fast_forward", "up_to_date", "methods", "default", "allowed", "merge_base", "conflicts":[{"path","kind"}], "reasons":{method: why}}` |
| `GET merge-requests/{n}/checks` | Readiness ([Server](./Server.md) §9), plus `methods` (allowed and possible) and `conflicts` |
| `POST /api/v1/tenants/{t}/linked-groups/{id}/merge` | Body `{"method"?}` (optional): every member merges with that method (or its own default) |

- `fast_forward` is structural (the target can be fast-forwarded or already
  contains the source), independent of protection. `methods` lists the allowed
  and possible methods in preference order; `default` is the first ("" =
  none). `allowed` lists the methods protection admits. Mergeability does not
  evaluate approvals, CI or TODOs: `checks` does, and reports `mergeable =
  true` for a diverged request without conflicts when `merge` or `squash` is
  allowed.
- The three-way outcome (merge base and conflicts) is cached in memory per
  (tree, source tip, target tip), bounded to 512 entries; tips are immutable,
  so entries never go stale.
- Errors:

| Status / code | When |
|---|---|
| `400 bad_request` | unknown `method`, invalid `message`, unknown body fields |
| `403 review_required` | missing approvals |
| `409 merge_blocked` | change requests, CI, TODOs; a fast-forward that would bring merge snapshots under `require_linear` |
| `409 linked_group` | member of an open linked group merged alone |
| `409 merge_method_not_allowed` | the method is refused by protection |
| `409 merge_conflict` | the three-way merge has conflicts |
| `409 conflict` | fast-forward requested (or the only option) on diverged branches; target moved during the merge; merge/squash into an empty target |
| `501 not_implemented` | a review service without server-side merges and a method other than fast-forward |

- `409 merge_conflict` keeps the standard error envelope and adds the
  conflicts, both at the top level and inside `error` (the message also lists
  up to ten paths):

```json
{
  "error": {
    "code": "merge_conflict",
    "message": "merge request !3 cannot be merged automatically: 1 conflicting path: a.txt (content); merge 'main' into 'feature' locally, resolve and push",
    "conflicts": [{"path": "a.txt", "kind": "content"}]
  },
  "conflicts": [{"path": "a.txt", "kind": "content"}]
}
```

  Linked-group conflicts carry `"tree": "tenant/tree"`.
- Linked groups: all members are evaluated with the method first; merge and
  squash snapshots of every member are written next; then the tips move with
  CAS in member order and roll back (to the old tips) on failure. Conflicts
  as the only problem answer `409 merge_conflict`, other problems `409
  merge_blocked`.

## 6. Recording, events and audit

- `MergeRequest.merge_method` records the method (`""` on requests merged
  before v0.6 = fast-forward).
- Events: `mr.merged`, and `branch.pushed` with
  `{branch, old_tip, new_tip, merge_request, merge_method}` when the target
  moved (so watchers, code search and webhooks see server-made snapshots).
- Audit `mr.merge` details: `old_tip`, `new_tip`, `method`, `approvals`,
  `up_to_date` (plus `linked_group` for group merges).

## 7. CLI

| Command | Description |
|---|---|
| `wt mr merge N [TREE] [--method fast-forward\|merge\|squash] [-m MSG]` | Merge on the server; conflicts exit with code 3 and a hint |
| `wt mr mergeability N [TREE]` | Methods, default, history state, merge base, reasons, conflicts |
| `wt mr merge-group ID [--method M]` | Merge a linked group with one method |
| `wt protect set [TREE] PATTERN ... --merge-methods a,b` | Restrict the merge methods of matching branches |

`wt mr show` prints the method of non-fast-forward merges (`merged as X (squash)`).

## Implementation Status

| Item | Status |
|---|---|
| `internal/merge3`: manifest merge, diff3 text merge, merge base | Built |
| Merge methods fast-forward / merge / squash, default selection | Built |
| `merge_methods`, `require_linear`, `require_signed` restrictions | Built |
| `409 merge_conflict` with conflict list, `409 merge_method_not_allowed` | Built |
| `GET .../mergeability` with per-(source, target) cache | Built |
| Method-aware readiness (`checks`): `methods`, `conflicts`, method blockers | Built |
| Linked-group merges with a method | Built |
| `MergeRequest.merge_method` recorded | Built |
| CLI `--method`, `-m`, `mergeability`, `--merge-methods` | Built |

## Roadmap

- Rebase merges (replaying the source snapshots onto the target).
- Conflict resolution in the browser (writing a resolved manifest from the
  admin panel) and server-side conflict-marker previews.
- Signing server-made snapshots with a server key so `require_signed` can
  allow merge and squash.
- Deleting the source branch after a merge.
- Content merges of chunked files.

## References

- `services/server-go/internal/merge3/` — `merge3.go` (manifests), `text.go`
  (diff3), `myers.go` (line matching), `base.go` (merge base)
- `services/server-go/internal/review/merge.go` — methods, selection,
  mergeability, snapshot writing; `checks.go` (readiness), `groups.go`
  (linked groups), `protection.go` (`merge_methods` resolution)
- `services/server-go/internal/httpapi/mergerequests.go`, `reviews.go`
- `services/server-go/internal/domain/v06.go` — `MergeFastForward`,
  `MergeCommit`, `MergeSquash`; `types.go` — `MergeRequest.MergeMethod`,
  `BranchProtection.MergeMethods`
- `crates/worktree-sdk/src/merge/` — the local merge engine these semantics
  follow
- `crates/worktree-cli/src/commands/{mr,protect}.rs`
- `docs/ENGINEERING_PLAN.md` §13.4, §14.1
- Related specs: [Server](./Server.md), [Tree](../tree/Tree.md)
