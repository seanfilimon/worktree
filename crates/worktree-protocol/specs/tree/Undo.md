# Undo, Force Push and Remote Tag Deletion Specification

> **Version:** 0.8. This spec describes the local operation log with `wt undo`
> and `wt redo`, force pushes with lease semantics, push idempotency, the
> server reflog, protection bypass and the `DeleteTag` RPC, as built
> (`crates/worktree-sdk/src/repo/oplog.rs`, `crates/worktree-sdk/src/remote/force.rs`,
> `services/server-go/internal/syncsvc/{push_force,push_force_ops,tags_delete}.go`,
> `docs/ENGINEERING_PLAN.md` §16.10). Features that are described but not built
> are listed under [Roadmap](#roadmap).

## Overview

Two kinds of mistakes are hard to fix in a version control system:

- a **local** operation that did something unexpected, such as a merge into
  the wrong branch, a rebase onto the wrong base, a deleted branch or a
  dropped stash entry;
- a **shared** branch that holds history which should not be there, such as
  a pushed secret or a bad merge.

For the first, every local mutation is recorded in an append-only operation
log. `wt undo` restores the refs an operation changed, and `wt redo`
re-applies it. For the second, an administrator can **force-push** a
rewritten branch. The lease makes sure nobody's work is lost by accident,
the protection rules decide who may do it, and the server keeps the rewound
tip in its reflog.

Snapshots are never deleted by any of these operations. Undo and force push
only move refs, so every state stays reachable through the oplog, the local
reflog or the server reflog until garbage collection.

---

## 1. The Operation Log

The log is stored in `.wt/oplog.jsonl`, one JSON object per line, oldest
first. The file is shared by every tree of the worktree, and each entry
names its tree.

```json
{"id":"7f0c…","at":"2026-10-26T10:00:00Z","tree":"web","kind":"op",
 "op":"snapshot","description":"fix login",
 "changes":[{"ref":"branch","name":"main",
             "old":{"tip":"1a2b…","archived":false},
             "new":{"tip":"3c4d…","archived":false}}]}
```

| Field | Meaning |
|---|---|
| `id` | UUID of the entry. |
| `at`, `tree` | When the entry was recorded and which tree it belongs to. |
| `kind` | `op` for an operation, `undo` or `redo` for an undo or redo of the operation named by `target`. |
| `op`, `description` | Operation name and summary. The name comes from the reflog entry written during the operation (`snapshot`, `merge`, `fast-forward`, `switch`, `rebase`, `cherry-pick`, `revert`, `branch.create`, `branch.archive`, `tag.create`, …), from the stash (`stash.push`, `stash.pop`, `stash.drop`, `stash.clear`), or is `update` for plumbing ref writes. |
| `changes` | Every ref the operation changed, with its `old` and `new` value (`null` means the ref did not exist). |

A ref change has one of these forms:

| `ref` | Value |
|---|---|
| `head` | The current branch name (`HEAD`). |
| `branch` | `{tip, archived}` of a local branch. |
| `tag` | The full local tag record. |
| `stash` | The raw content of `trees/<tree>/stash.jsonl`. |

Remote-tracking refs, the reflog, the index and the working tree are not
recorded.

### 1.1 Recording

Every mutating operation takes the per-tree lock (`trees/<tree>/LOCK`). The
lock guard records the operation. When the lock is taken, the guard
captures the tree's refs: `HEAD`, every branch with its archived flag,
every tag and the stash. When the lock is released, the guard captures the
refs again. If anything changed, it appends one `op` entry with the changed
refs. Operations that change nothing are not logged, for example `wt
status`, fetches, sparse checkout changes and reflog expiry. This covers
snapshot, merge and merge continue, revert, cherry-pick, rebase (and their
`--continue` / `--abort`), branch switch, create, archive and restore,
plumbing tip writes (`set_branch_tip`, used by reset-like flows and by
pull's fast-forward), tag create and delete, and stash push, pop, drop and
clear. No per-operation code is needed.

Appends take `.wt/oplog.lock`. When the file grows beyond 4 MiB, it is
trimmed to its newer half. Undo and redo hold the tree lock without
recording and then append their own entries.

### 1.2 Stacks

Undo and redo work on two stacks per tree, computed by replaying the
tree's entries in order:

- an `op` entry is pushed on the **done** stack and clears the **undone**
  stack;
- an `undo` entry moves its target from done to undone;
- a `redo` entry moves its target back from undone to done.

`wt undo --list` shows the done stack, newest first. The position of an
entry is the `N` of `wt undo N`. `wt redo --list` shows the undone stack,
next first.

## 2. Undo and Redo

`wt undo [N] [--force]` undoes the newest `N` operations of the tree
(default 1), newest first. `wt redo [N] [--force]` re-applies the `N` most
recently undone operations. Both commands follow the same procedure:

1. Refuse while a merge, cherry-pick or rebase is stopped (`MergeInProgress`
   or the rewrite error). Refuse when the stack has fewer than `N` entries
   (`nothing to undo`, `only K operations can be undone`).
2. Unless `--force` is given, every ref an operation touches must still
   hold the value that operation left (`new` for undo, `old` for redo,
   checked in order). Otherwise the command fails with `cannot undo "…":
   <ref> changed since (use --force)`.
3. Unless `--force` is given, the working tree must be clean (see §2.1). If
   it is not, the command fails with `DirtyWorkingTree`, the same error as
   `wt switch`.
4. Each ref is set to its `old` value (undo) or `new` value (redo). Branches
   are recreated or removed with their archived flag, and tags are rewritten
   or removed.
5. The tip of the (possibly restored) current branch is checked out. Paths
   of the old head and of the working tree that the target lacks are
   removed, so with `--force` uncommitted changes are discarded.
6. One `undo` or `redo` entry is appended per operation, with the applied
   changes and `target` = the operation's id.

The result names the processed operations, the current branch and its
tip.

### 2.1 Stash semantics

The stash holds working changes, so undoing stash operations also moves
those changes:

| Operation | Undo | Redo |
|---|---|---|
| `stash.push` | Removes the entry and re-applies its changes to the working tree (three-way, like `wt stash apply`). | Puts the entry back and resets the working tree to the head. |
| `stash.pop` | Puts the entry back and resets the working tree to the head. | Removes the entry and re-applies its changes. |
| `stash.drop`, `stash.clear` | Puts the entries back. | Removes them again. Nothing is applied. |

When an undo or redo puts an entry back, the working tree may hold exactly
that entry's changes: its working manifest equals the entry's stashed
manifest. In that case the clean-tree check of step 3 passes. This makes
`push → undo → redo` and `pop → undo` work without `--force`.

`wt stash apply` changes only the working tree, so it is not recorded.

## 3. Force Push

### 3.1 Client

`wt push --force` (alias `--force-with-lease`) pushes a branch whose server
tip is not an ancestor of the local tip:

1. The server tip must equal the last known remote-tracking tip
   (`refs/remotes/<remote>/<branch>`, updated by fetch and push). If it does
   not, the push is refused before anything is uploaded: `refusing to
   force-push origin/main: the server tip … is not the last tip you fetched
   (…); run wt fetch and review the remote history before forcing`.
2. The chain and objects are collected as for a normal push. The request
   carries `force = true` and `old_tip` = that tip (the lease).
3. On success the report has `forced = true`, the remote-tracking ref moves,
   and the local reflog records `force-push`. `wt push` prints
   `old...new main -> origin/main (forced update)`.

`wt push --force` without divergence is an ordinary push.

### 3.2 Server

The force-push decision is step 6 of the push pipeline (Sync.md), when
`new_tip` does not descend from the server tip:

1. Without `force`, the push is rejected with `ConflictDetected{server_tip}`,
   as before.
2. **Lease**: when `old_tip` is set, it must equal the server tip
   (`ConflictDetected` otherwise). The branch then moves with
   compare-and-swap from that tip, so a concurrent push also loses the
   lease. A force push without `old_tip` is unconditional.
3. `branch:force_push` is required (admin role and up, tree-scoped).
   Otherwise the result is `AccessDenied{"force pushing to B requires
   branch:force_push"}`.
4. A matching protection with `no_force_push` refuses the force push with
   `BranchProtection{"branch 'B' is protected against force pushes
   (no_force_push)"}`. This rule is absolute, even for owners who bypass
   protection.
5. Matching protections with `require_linear` or `required_approvals > 0`
   refuse the force push unless the pusher bypasses branch protection (§5).
   Rewriting such a branch would otherwise bring in unreviewed or
   non-linear history.

A successful force push publishes `branch.pushed` (payload `forced: true`)
and `branch.force_pushed` `{branch, old_tip, new_tip, bypass}`. It is
audited as `branch.push` with `forced = true`, and the reflog entry keeps
the rewound tip (§4).

`wt protect set PATTERN --no-force-push` and the admin "Prevent force
pushes" toggle set the rule (`BranchProtection.no_force_push`).

## 4. Server Reflog

Every accepted push that moves a branch appends a `ReflogEntry` to the
`branch_reflog` collection, keyed `DocKey(treeID, branch, %020d unix-nano,
random)`:

`{tree_id, branch, old_tip, new_tip, kind: push|force_push, forced, bypass,
bypassed[], actor, actor_id, count, at}`

`GET {tree}/reflog?branch=&limit=50` (`tree:read`, at most 500 entries)
returns the entries newest first. The admin History tab lists the force
pushes of the selected branch with who rewound it and from which tip, and
marks the snapshots they made the tip. Entries are kept for 90 days and
are deleted with their tree. Merge-request merges are not recorded yet.

## 5. Protection Bypass

`branch:bypass_protection` (owner role, tree-scoped) lets a caller push or
merge past branch protection. The bypass is never implicit, so owners see
protection refusals like everyone else unless they ask for it:

- **Push**: the gRPC metadata `x-wt-bypass-protection: true` (`wt push
  --bypass-protection`) lifts the review service's branch-protection
  refusals (`no_direct_push`, `allowed_pushers`, `require_linear` on merge
  snapshots, and the force-push rules of §3.2.5). It does not lift
  `no_force_push`, path ACLs, quotas, license rules, secret scanning or
  signatures.
- **Merge**: `POST {tree}/merge-requests/{n}/merge` with `{"bypass": true}`
  lifts required approvals, change requests, owner approvals, required CI
  and blocking TODOs. It does not lift drafts, linked groups, merge-method
  restrictions or conflicts. Asking for a bypass without the permission
  answers `403 forbidden`.

Every bypass is audited: `branch.push` / `mr.merge` get `bypass = true` and
`bypassed` (the lifted rules), and a separate `branch.protection_bypassed`
record names the operation (`push`, `force_push`, `merge`).

## 6. Push Idempotency

`PushRequest.operation_id` is a client-chosen key of at most 128 letters,
digits and `- _ . :` characters. The first response to a push with a given
id is stored in `push_ops` (key `DocKey(treeID, operationID)`) for 24
hours. A retry by the same account returns that response without running
the push again. This covers acceptances and rejections, but not status
errors. Ids of other accounts are not shared. The SDK sends a fresh UUID
with every push and retries once, with the same id, when the connection
fails (`transport error`, `connection reset`, `broken pipe`). Expired
entries are pruned hourly and deleted with their tree.

## 7. Remote Tag Deletion

`DeleteTag{tree_id, name}` deletes a server tag. It needs `tree:write`.
Tag protections apply: `no_delete` refuses, and `allowed_taggers` limits
who may delete. Refusals and unknown tags are reported in the response
(`deleted = false, error`) and audited as `tag.delete_rejected`. A deletion
publishes `tag.deleted` and is audited as `tag.delete`. The REST endpoint
`DELETE {tree}/tags/{name}` stays as well (`tree:admin`).

`wt tag delete NAME --remote [--from R]` deletes the tag on the server and
then deletes the local tag, if there is one, so the next `wt push` does not
create it again.

## CLI Summary

| Command | Effect |
|---|---|
| `wt undo [N] [--force]` | Undo the newest N operations of the tree. |
| `wt undo --list` | Operations that can be undone, newest first. |
| `wt redo [N] [--force]` / `wt redo --list` | Re-apply undone operations / list them. |
| `wt push --force` (`--force-with-lease`) | Force push with lease. |
| `wt push --bypass-protection` | Ask the server to bypass branch protection. |
| `wt protect set PATTERN --no-force-push` | Refuse force pushes to matching branches. |
| `wt tag delete NAME --remote [--from R]` | Delete the tag on the server (and locally). |

## Implementation Status

| Feature | Status |
|---|---|
| `.wt/oplog.jsonl` recorded by the tree lock guard (HEAD, branches, tags, stash) | Built (v0.8 §16.10) |
| `wt undo [N]`, `wt undo --list`, `wt redo [N]`, `--force` | Built (v0.8 §16.10) |
| Stash-aware undo/redo (re-apply / re-stash) | Built (v0.8 §16.10) |
| `wt push --force` with client-side lease, server-side `old_tip` lease and CAS | Built (v0.8 §16.10) |
| `branch:force_push`, `no_force_push`, require_linear/approvals need bypass | Built (v0.8 §16.10) |
| `branch.force_pushed` event, audit flags, `branch_reflog` + `GET {tree}/reflog` | Built (v0.8 §16.10) |
| Protection bypass on push (metadata) and merge (`bypass`), always audited | Built (v0.8 §16.10) |
| `operation_id` idempotency (`push_ops`, 24 h), one SDK retry | Built (v0.8 §16.10) |
| `DeleteTag` RPC, `wt tag delete --remote` | Built (v0.8 §16.10) |
| Admin: `no_force_push` toggle, force-push markers in branch history | Built (v0.8 §16.10) |

## Roadmap

- Undo of working-tree-only operations (`wt restore`, `wt stash apply`,
  `wt checkout`), which would need working-tree snapshots in the log.
- Grouping composite operations into one entry (today `wt switch` with
  auto-stash records stash push, switch and stash pop separately).
- An oplog-aware `wt gc` that keeps the objects of dropped stash entries
  for as long as the log mentions them.
- `wt undo` of a push (a force push back to the previous server tip).
- Merge-request merges and REST branch moves in the server reflog, and a
  REST endpoint to restore a branch from a reflog entry.
- A CLI flag for bypassing protection on `wt mr merge` (the REST body
  supports it).

## References

- `crates/worktree-sdk/src/repo/oplog.rs` — `OpGuard`, `OpLogEntry`, `undo`, `redo`
- `crates/worktree-sdk/src/repo/{trees,refs,stash}.rs` — lock hook, ref restoration, stash hooks
- `crates/worktree-sdk/src/remote/{force,push,client}.rs` — lease, operation ids, bypass metadata, `delete_tag`
- `crates/worktree-cli/src/commands/{undo,tag,protect,sync}.rs`
- `services/server-go/internal/syncsvc/push_force.go`, `push_force_ops.go`, `tags_delete.go`, `push.go`
- `services/server-go/internal/review/protection.go` (`CheckForcePush`), `bypass.go`
- `services/server-go/internal/httpapi/reflog.go`, `openapi.yaml`
- `apps/admin/src/pages/tree/ProtectionsTab.tsx`, `HistoryTab.tsx`, `history/ForcePushes.tsx`, `api/historyV2.ts`
- `crates/worktree-e2e/tests/e2e/force_push_undo.rs`
