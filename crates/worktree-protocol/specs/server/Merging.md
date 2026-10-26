# Server-side Merges

How the server merges a merge request into its target branch: the four
merge methods, how one is chosen, how branch protection restricts them, the
three-way merge the server runs, conflict reports and conflict resolution on
the server (from the CLI or the admin panel), and the REST and CLI surface
(`docs/ENGINEERING_PLAN.md` §14.1, §16.9). Reviews, CI and the readiness
gates that apply before any merge are described in [Server](./Server.md) §9.

## 1. Merge methods

| Method | Result on the target branch | Snapshots written by the server |
|---|---|---|
| `fast-forward` | The tip moves to the source tip | None |
| `merge` | The tip moves to a new merge snapshot | Parents `[target tip, source tip]`, manifest = three-way merge |
| `squash` | The tip moves to a new snapshot | Parent `[target tip]`, manifest = three-way merge (the combined changes) |
| `rebase` (v0.8) | The tip moves to the last replayed snapshot | One new snapshot per source snapshot the target lacks, replayed in order onto the target tip (§1.1) |

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
  the target. `rebase` on a fast-forwardable request is a fast-forward.
- `merge`, `squash` and a diverged `rebase` need a non-empty target branch and
  an object store on the server (always configured by `worktree-server`);
  otherwise only `fast-forward` is offered.
- `merge`/`squash` snapshots get a new UUID id, `branch_name` = target,
  `author` = the merger's username, `pushed_by` = the merger's account,
  `created_at` = `pushed_at` = the merge time, no signature. The merged blobs
  and the manifest (deterministic protobuf encoding) are stored before the
  snapshot; clients fetch them like any pushed snapshot.
- Messages: `{"message"}` when given (trimmed, at most 64 KiB of UTF-8, no
  NUL); otherwise `Merge branch '<source>' into <target> (!N)` for `merge` and
  the merge request title for `squash`. Fast-forwards and rebases ignore the
  message.

### 1.1 Rebase merges

`rebase` replays the snapshots reachable from the source tip but not from the
target tip (oldest first, parents before children) onto the target tip:

- Each snapshot's change against its **first parent** is applied to the
  current tip with the three-way merge of §4 (base = the parent's manifest,
  ours = the tip so far, theirs = the snapshot). A snapshot whose change does
  not apply cleanly stops the rebase: `409 merge_conflict` with that
  snapshot's conflicts ("snapshot X does not apply on 'main'"); nothing is
  written.
- Replayed snapshots get new UUID ids, `branch_name` = target, parents =
  `[previous replayed snapshot]` (the first: `[target tip]`), and keep the
  original `author`, `message` and `created_at`; `pushed_by` = the merger,
  `pushed_at` = the merge time, no signature. A snapshot whose changes the
  target already has (the replayed manifest equals the tip's) is dropped.
- The source must be linear: a merge snapshot among the replayed ones (for
  example a conflict resolution, §5) makes `rebase` impossible ("rebase
  replays linear histories only"; use `squash` or `merge`). At most 1000
  snapshots are replayed.
- When the target is an ancestor of the source (and the source is linear) a
  rebase is a fast-forward: no snapshot is written, `merged_snapshot` = the
  source tip.
- The outcome (incoming count, merge snapshots, conflicts) is cached per
  (tree, source tip, target tip) like the three-way outcome.
- `MergeRequest.merge_method` = `rebase`, `merged_snapshot` = the last
  replayed snapshot.

### 1.2 Deleting the source branch

`{"delete_source_branch": true}` on a merge deletes the source branch after a
successful merge. It is checked **before** merging, so a refusal leaves the
merge request open and the target unchanged:

- the caller needs `branch:delete`; the default branch and branches protected
  with `no_delete` are refused (`403 branch_protection`, as `DELETE
  {tree}/branches/{name}`);
- a source branch that is also the source or target of another open merge
  request is refused (`409 conflict`, naming it);
- a source branch in a fork (`source_tree_id`) is refused (`400`).

The deletion publishes `branch.deleted` (payload: the branch), is audited as
`branch.delete` (`{branch, tip, merge_request}`) and adds
`source_branch_deleted: true` to the `mr.merge` audit record. A failure after
the merge (the branch vanished meanwhile, storage errors) is logged and does
not fail the merge.

## 2. Choosing the method

The request body is optional: `{"method"?: "fast-forward"|"merge"|"squash"|"rebase",
"message"?, "delete_source_branch"?}`. An empty body or `{}` selects the
**default method: the first of `fast-forward`, `merge`, `squash`, `rebase`
that is allowed and possible** — fast-forward when the target is an ancestor
of the source, else a merge snapshot.

A method is **allowed** when branch protection admits it (§3). It is
**possible** when, additionally:

| Method | Possible when |
|---|---|
| `fast-forward` | the target is an ancestor of the source (or empty), and with `require_linear` no incoming snapshot is a merge snapshot |
| `merge`, `squash` | the target is non-empty and the three-way merge has no conflicts |
| `rebase` | no incoming snapshot is a merge snapshot, at most 1000 of them, and the target is an ancestor of the source or every snapshot replays without conflicts |

The readiness gates of [Server](./Server.md) §9 (approvals, change requests,
CI, TODOs, linked groups) are checked first and do not depend on the method.

## 3. Protection restrictions

The effective protection of the target (every matching rule, most restrictive
combination) restricts the methods:

- `merge_methods` (on `BranchProtection`, `PUT {tree}/protections`): the
  methods allowed into matching branches; empty = all. Lists of several
  matching rules intersect; an empty intersection allows nothing. Values are
  validated on write (`400` for unknown methods) and stored deduplicated in
  preference order (fast-forward, merge, squash, rebase).
- `require_linear` forbids `merge` (squash, rebase and fast-forward keep
  history linear; a fast-forward that would bring merge snapshots is refused,
  see §2).
- `require_signed` forbids `merge`, `squash` and `rebase`: the server cannot
  sign the snapshots it writes.

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
- A merge with any conflict is refused. The server never writes conflict
  markers into a branch; it shows them in conflict reports (§5) and the
  conflicts are resolved locally (`wt merge` the target into the source,
  resolve, push) or on the server with a merge-resolution snapshot (§5).

## 5. Conflict reports and resolution on the server (v0.8)

Both look at the merge the way `wt merge <target>` run on the source branch
would: **ours is the source branch** (where the resolution is written),
**theirs is the target branch**. The conflict set is the three-way merge of
§4 of the target into the source.

### 5.1 Conflict report

`GET {tree}/merge-requests/{n}/conflicts` (`tree:read`) answers:

```json
{
  "number": 3, "state": "open",
  "source_branch": "feature", "target_branch": "main",
  "source_tip": "…", "target_tip": "…", "merge_base": "…",
  "resolvable": true, "reason": "",
  "total": 1, "truncated": false,
  "files": [{
    "path": "a.txt", "kind": "content",
    "binary": false, "too_large": false, "restricted": false,
    "base":   {"exists": true, "size": 6, "executable": false, "hash": "…", "text": "1\n2\n3\n"},
    "ours":   {"exists": true, "size": 8, "executable": false, "hash": "…", "text": "uno\n2\n3\n"},
    "theirs": {"exists": true, "size": 8, "executable": false, "hash": "…", "text": "ONE\n2\n3\n"},
    "markers": "<<<<<<< feature\nuno\n||||||| base\n1\n=======\nONE\n>>>>>>> main\n2\n3\n",
    "regions": [{"start_line": 1, "ours_start": 1, "theirs_start": 1, "ours": ["uno"], "base": ["1"], "theirs": ["ONE"]}]
  }]
}
```

- Each side carries `exists`, `size`, `executable`, `hash` and its `text`.
  The text is omitted for absent sides, binary or chunked files (`binary`),
  sides over 1 MiB or once the report's 8 MiB text budget is spent
  (`too_large`), and paths the caller may not read — path policies and
  license read rules, as for file reads (`restricted`: no text, markers or
  regions).
- `markers` is the diff3-style preview (`<<<<<<< <source>`, `||||||| base`,
  `=======`, `>>>>>>> <target>`, the merge3 text merge of base, ours and
  theirs, §4) when both sides are text (`content`, `add_add`); `regions` are
  its conflicting regions.
- At most 200 paths are detailed; `total` counts all of them and `truncated`
  is set when more exist (resolve those locally).
- `resolvable` is false, with a `reason`, when the merge request is not open,
  the target can be fast-forwarded or already contains the source ("no
  conflicts"), the clean three-way merge can merge it on the server, the
  source lives in a fork, a branch is missing, or the server has no object
  store.

### 5.2 Resolution snapshot

`POST {tree}/merge-requests/{n}/resolve` with

```json
{
  "files": [
    {"path": "a.txt", "content": "uno/ONE\n2\n3\n"},
    {"path": "logo.png", "take": "theirs"},
    {"path": "old.txt", "delete": true}
  ],
  "message": "optional",
  "source_tip": "…", "target_tip": "…",
  "allow_markers": false
}
```

writes a **merge-resolution snapshot on the source branch** whose manifest is
the clean part of the three-way merge plus the resolutions:

- Every conflicting path is resolved exactly once with exactly one of
  `content` (UTF-8 text, at most 8 MiB; `executable` optional, default ours'
  bit, else theirs'), `take` (`ours`, `theirs` or `base`; taking a side that
  deleted the path deletes it; `base` of a path the base lacks is refused) or
  `delete`. Unresolved, unknown (non-conflicting) or doubly resolved paths,
  more than 1000 resolutions and resolutions that leave a file where a
  directory is (`a` next to `a/b`) answer `400`. Content that still holds a
  conflict block (`<<<<<<<` … `=======` … `>>>>>>>`) is refused unless
  `allow_markers`. The body may be up to 32 MiB.
- `source_tip` / `target_tip` (the tips the report was computed at) must
  still be the branch tips, otherwise `409 resolve_outdated`; a source that
  moves while the snapshot is written answers the same.
- The snapshot: new UUID id, `branch_name` = source, parents `[source tip,
  target tip]`, `author` = the caller's username, `pushed_by` = the caller,
  `created_at` = `pushed_at` = now, no signature; message default `Merge
  branch '<target>' into <source> (!N, conflicts resolved)`. Blobs and the
  manifest are stored first, then the snapshot, then the source moves by CAS.
- Authorization and checks as for a push of that snapshot: `tree:write`, the
  source's protection must allow a direct push by the caller (`403
  branch_protection`), and the server's push checks run on the plan (changed
  paths = every path that differs from the source tip, including the target's
  changes): path ACLs (`403 path_denied`), the tenant storage quota (`409
  quota_exceeded`) and signing rules (`require_signed` on the source: `403
  signature_required`).
- Afterwards the target is an ancestor of the source: the merge request
  fast-forwards (or merges/squashes) without conflicts. Approvals of the old
  source tip become stale like after any push. Because the resolution is a
  merge snapshot, `rebase` is no longer possible for the request (§1.1).
- Merge requests that are not open or have no conflicts answer `409
  conflict`; fork sources `409 conflict` with the report's reason.
- The answer: `{"merge_request", "snapshot", "paths"}` (`paths` = the
  resolved conflicting paths).

## 6. REST

All under `/api/v1/trees/{tenant}/{tree}`.

| Endpoint | Description |
|---|---|
| `POST merge-requests/{n}/merge` | Body `{"method"?, "message"?, "delete_source_branch"?}` (optional). Requires `mr:merge`. `200` with the merged `MergeRequest` (`merge_method`, `merged_snapshot` = the new snapshot for merge/squash, the last replayed snapshot for rebase, the source tip otherwise) |
| `GET merge-requests/{n}/mergeability` | Requires `tree:read`. `{"fast_forward", "up_to_date", "methods", "default", "allowed", "merge_base", "conflicts":[{"path","kind"}], "reasons":{method: why}}` |
| `GET merge-requests/{n}/checks` | Readiness ([Server](./Server.md) §9), plus `methods` (allowed and possible) and `conflicts` |
| `GET merge-requests/{n}/conflicts` | v0.8: the conflict report (§5.1). Requires `tree:read` |
| `POST merge-requests/{n}/resolve` | v0.8: the merge-resolution snapshot (§5.2). Requires `tree:write` |
| `POST /api/v1/tenants/{t}/linked-groups/{id}/merge` | Body `{"method"?}` (optional): every member merges with that method (or its own default) |

- `fast_forward` is structural (the target can be fast-forwarded or already
  contains the source), independent of protection. `methods` lists the allowed
  and possible methods in preference order; `default` is the first ("" =
  none). `allowed` lists the methods protection admits. Mergeability does not
  evaluate approvals, CI or TODOs: `checks` does, and reports `mergeable =
  true` for a diverged request without conflicts when `merge`, `squash` or
  `rebase` is allowed.
- The three-way outcome (merge base and conflicts) and the rebase outcome are
  cached in memory per (tree, source tip, target tip), bounded to 512 entries
  each; tips are immutable, so entries never go stale.
- Errors:

| Status / code | When |
|---|---|
| `400 bad_request` | unknown `method`, invalid `message`, unknown body fields; invalid resolutions (§5.2) |
| `403 review_required` | missing approvals |
| `403 branch_protection` | `delete_source_branch` of the default or a `no_delete` branch; a resolution on a source that refuses direct pushes |
| `409 merge_blocked` | change requests, CI, TODOs; a fast-forward that would bring merge snapshots under `require_linear` |
| `409 linked_group` | member of an open linked group merged alone |
| `409 merge_method_not_allowed` | the method is refused by protection |
| `409 merge_conflict` | the three-way merge (or a rebased snapshot) has conflicts |
| `409 resolve_outdated` | a resolution computed against tips that moved |
| `409 conflict` | fast-forward requested (or the only option) on diverged branches; target moved during the merge; merge/squash into an empty target; rebase of a source with merge snapshots; `delete_source_branch` of a branch another open merge request uses; resolving a request without conflicts |
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
- Linked groups: all members are evaluated with the method first; merge,
  squash and rebase snapshots of every member are written next; then the tips
  move with CAS in member order and roll back (to the old tips) on failure.
  Conflicts as the only problem answer `409 merge_conflict`, other problems
  `409 merge_blocked`.

## 7. Recording, events and audit

- `MergeRequest.merge_method` records the method (`""` on requests merged
  before v0.6 = fast-forward).
- Events: `mr.merged`, and `branch.pushed` with
  `{branch, old_tip, new_tip, merge_request, merge_method}` when the target
  moved (so watchers, code search and webhooks see server-made snapshots);
  `branch.deleted` for `delete_source_branch`.
- Resolutions publish `branch.pushed` `{branch, old_tip, new_tip,
  merge_request, resolution: true}` for the source and
  `mr.conflicts_resolved` `{number, source_branch, target_branch, old_tip,
  target_tip, snapshot_id, paths, resolved_by}`.
- Audit `mr.merge` details: `old_tip`, `new_tip`, `method`, `approvals`,
  `up_to_date` (plus `linked_group` for group merges, `source_branch_deleted`
  with `delete_source_branch`); `branch.delete` for the deleted source;
  `mr.resolve_conflicts` `{snapshot, old_tip, target_tip, paths}`.

## 8. CLI

| Command | Description |
|---|---|
| `wt mr merge N [TREE] [--method fast-forward\|merge\|squash\|rebase] [--rebase] [--delete-branch] [-m MSG]` | Merge on the server; conflicts exit with code 3 and a hint naming `wt mr conflicts` / `wt mr resolve`; `--rebase` = `--method rebase` |
| `wt mr mergeability N [TREE]` | Methods, default, history state, merge base, reasons, conflicts |
| `wt mr conflicts N [TREE] [--markers] [--write [--dir DIR]]` | The conflict report; `--markers` prints the diff3 previews; `--write` writes them into the local tree directory (or DIR) for editing (JSON: `{"conflicts", "written"}`) |
| `wt mr resolve N [TREE] [--take ours\|theirs\|base PATH]... [--delete PATH]... [--dir DIR] [-m MSG] [--allow-markers]` | Resolve on the server: every conflicting path not given to `--take`/`--delete` is read from the local tree directory (or DIR); leftover markers exit with code 3; tips are sent so a stale report answers `resolve_outdated` |
| `wt mr merge-group ID [--method M]` | Merge a linked group with one method |
| `wt protect set [TREE] PATTERN ... --merge-methods a,b` | Restrict the merge methods of matching branches |

`wt mr show` prints the method of non-fast-forward merges (`merged as X (squash)`).

## 9. Admin panel

- Merge request detail: an open request with conflicts shows a banner (count,
  first paths) linking to **Resolve conflicts**; the merge dialog offers
  `rebase` and a "Delete source branch" checkbox.
- `merge-requests/:number/resolve` (`ResolveConflictsPage`): a file list
  with per-file resolved state and "N of M resolved"; per file three panes —
  ours (source, read-only), the editable result (prefilled with the marker
  preview, else ours), theirs (target, read-only), the base collapsible —
  with Take ours / Take theirs / Take base / Delete file, previous / next
  conflict navigation and a count of the marker blocks left. Binary, too large
  and restricted files offer only take/delete. Submit is enabled once every
  file is resolved and sends the tips of the loaded report; `409
  resolve_outdated` offers a reload.

## Implementation Status

| Item | Status |
|---|---|
| `internal/merge3`: manifest merge, diff3 text merge, merge base | Built |
| Merge methods fast-forward / merge / squash, default selection | Built |
| Rebase merges (replay onto the target, linear sources, outcome cache) | Built (v0.8) |
| `delete_source_branch` on merge | Built (v0.8) |
| `merge_methods`, `require_linear`, `require_signed` restrictions | Built |
| `409 merge_conflict` with conflict list, `409 merge_method_not_allowed` | Built |
| `GET .../mergeability` with per-(source, target) cache | Built |
| Method-aware readiness (`checks`): `methods`, `conflicts`, method blockers | Built |
| Linked-group merges with a method | Built |
| `MergeRequest.merge_method` recorded | Built |
| `GET .../conflicts` (texts, binary marking, size caps, marker previews, restricted paths) | Built (v0.8) |
| `POST .../resolve` merge-resolution snapshots (push checks, events, audit) | Built (v0.8) |
| CLI `--method`, `-m`, `mergeability`, `--merge-methods` | Built |
| CLI `conflicts`, `resolve`, `merge --rebase --delete-branch` | Built (v0.8) |
| Admin conflict banner and `ResolveConflictsPage` | Built (v0.8) |

## Roadmap

- Signing server-made snapshots with a server key so `require_signed` can
  allow merge, squash, rebase and resolution snapshots.
- Content merges of chunked files (and text previews of chunked conflicts).
- Rebasing sources that contain merge snapshots (re-resolving recorded
  resolutions) and interactive rebase merges (squashing or dropping
  snapshots).
- Conflict resolution for merge requests from forks (writing the resolution
  into the fork) and for more than 200 conflicting paths in the browser.
- Binary uploads as resolutions (only text content, take and delete today).

## References

- `services/server-go/internal/merge3/` — `merge3.go` (manifests), `text.go`
  (diff3), `myers.go` (line matching), `base.go` (merge base)
- `services/server-go/internal/review/merge.go` — methods, selection,
  mergeability, snapshot writing; `rebase.go` (rebase merges,
  `delete_source_branch`); `resolve.go` (conflict reports, resolution
  snapshots); `checks.go` (readiness), `groups.go` (linked groups),
  `protection.go` (`merge_methods` resolution)
- `services/server-go/internal/httpapi/mergerequests.go`, `reviews.go`,
  `resolve.go`
- `services/server-go/internal/domain/v06.go` — `MergeFastForward`,
  `MergeCommit`, `MergeSquash`; `review.MergeRebase`; `types.go` —
  `MergeRequest.MergeMethod`, `BranchProtection.MergeMethods`; `v08.go` —
  `EventConflictsSolved`
- `crates/worktree-sdk/src/merge/` — the local merge engine these semantics
  follow
- `crates/worktree-cli/src/commands/{mr,mr_resolve,protect}.rs`
- `apps/admin/src/pages/tree/{MergeRequestDetail,ResolveConflictsPage}.tsx`,
  `apps/admin/src/api/conflicts.ts`
- `crates/worktree-e2e/tests/e2e/{server_merges,conflict_resolution}.rs`
- `docs/ENGINEERING_PLAN.md` §13.4, §14.1, §16.9
- Related specs: [Server](./Server.md), [ReviewWorkflow](./ReviewWorkflow.md),
  [Tree](../tree/Tree.md)
