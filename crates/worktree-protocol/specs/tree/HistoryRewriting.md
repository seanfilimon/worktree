# History Rewriting and Stash Specification

> **Version:** 0.6 — describes `wt cherry-pick`, `wt rebase`, `wt stash` and
> `auto_snapshot.on_branch_switch = "stash"` as built
> (`crates/worktree-sdk/src/repo/{rewrite,stash}.rs`, `docs/ENGINEERING_PLAN.md` §14.5).
> Features described but not built are listed under [Roadmap](#roadmap).

## Overview

W0rkTree history is append-only once shared: the server never accepts a
branch update that drops snapshots it already has. Locally, before
pushing, three tools reshape work in progress:

- **Cherry-pick** copies the change of existing snapshots onto the current
  branch as new snapshots.
- **Rebase** replays the current branch's own snapshots on top of another
  base and moves the branch to the result. Pushed snapshots are protected.
- **Stash** sets uncommitted working changes aside and restores them later,
  optionally automatically on every branch switch.

All three are local, per-tree operations. They reuse the three-way merge
engine (`crate::merge`, `Repository::three_way`) and, for cherry-pick and
rebase, the conflict state of `wt merge`, so a conflicting step is resolved
with the same commands as a conflicting merge.

---

## 1. Replaying a Snapshot

Cherry-pick and rebase are sequences of **steps**. A step applies one
snapshot `S`:

| Merge side | Manifest |
|---|---|
| base | `S`'s first parent (empty for a root snapshot) |
| ours | the progress so far (the branch tip, or the onto snapshot plus already replayed steps) |
| theirs | `S` |

The merge is `merge_manifests` + diff3 text merging exactly as in
`wt merge` (path-level resolution, text conflicts with markers, binary and
modify/delete conflicts keep "ours" in the working tree). For merge
snapshots only the change relative to the **first** parent is applied; the
replayed snapshot has a single parent.

Outcomes of a step:

- **Clean, with changes** → a new snapshot: new UUID, parent = progress
  tip, `author` and `message` of `S`, `auto_generated` of `S`,
  `created_at` = now, `branch` = current branch. `reverts` is kept and,
  when it names a snapshot replayed earlier in the same operation, is
  remapped to that snapshot's replacement. Rebase keeps `dependencies`
  (they move with the snapshot); cherry-pick drops them (a copy would
  create duplicate TODOs on push). The snapshot is signed when signing is
  configured (`[signing] key` + `auto = true`, see Security.md), using
  `signing::sign_snapshot` — signatures of `S` are never copied because the
  signed payload changes.
- **Clean, no changes** (the change is already present) → the step is
  **skipped** and listed in `skipped`.
- **Conflicts** → the operation **stops** (section 4).

## 2. Cherry-pick

```
wt cherry-pick REV... [-n|--no-snapshot]
wt cherry-pick --continue | --skip | --abort
```

- Revisions are resolved with the usual syntax (`HEAD~2`, branch names,
  `origin/main`, tags, id prefixes, `@{N}`) and applied in the given order.
- Requires a clean working tree (no modified, deleted or new files) and no
  merge, cherry-pick or rebase in progress.
- Each created snapshot moves the branch and writes one reflog entry
  (`action = "cherry-pick"`, message `cherry-pick <short id>: <subject>`).
- `--no-snapshot` applies the changes to the working tree only: no
  snapshots, the branch does not move, nothing is written to the reflog.
  When such a run conflicts after earlier steps were applied, the
  accumulated working state is stored as an unreferenced snapshot
  (`auto_generated`, message `cherry-pick --no-snapshot: working state
  before <id>`) and used as the "ours" side of the conflict, so
  `wt merge resolve --ours` keeps the earlier steps.

## 3. Rebase

```
wt rebase ONTO [--force]
wt rebase --continue | --skip | --abort
```

Let `T` be the current branch tip and `O` the resolved `ONTO`.

1. `O` is an ancestor of `T` → **up to date**, nothing changes.
2. `T` is an ancestor of `O` → **fast-forward**: the branch moves to `O`,
   the working tree is updated, reflog `rebase: fast-forward to <O>`.
3. Otherwise the **todo list** is the first-parent chain of `T` back to the
   first snapshot reachable from `O`, oldest first. A snapshot missing
   locally (shallow history) is an error asking for `wt fetch --deepen N`.
4. **Pushed-history guard**: for every configured remote (and `origin`),
   if the remote-tracking ref `refs/remotes/<remote>/<branch>` exists and
   its ancestry contains any todo snapshot, the rebase is refused (exit
   1) unless `--force`: those snapshots are already on the server, and the
   rewritten branch would need a force push the server refuses.
5. The steps are replayed onto `O`. On completion the branch points at
   the last replayed snapshot and one reflog entry `action = "rebase"`
   records `old` = tip before the call, `new` = the result (so
   `BRANCH@{1}` is the pre-rebase tip). A call that stops at a conflict
   also writes one `rebase` entry when the branch moved.

Requires a clean working tree and no operation in progress. The original
snapshots are not deleted; they stay reachable through the reflog until it
expires.

## 4. Conflicts, Continue, Skip, Abort

A conflicting step stops the operation:

- The working tree holds the merged result with diff3 conflict markers
  (labels `ours (<branch> <short id>)`, `base (<short parent id>)`,
  `theirs (<short id of S>)`).
- `.wt/trees/<tree>/MERGE_STATE.json` and `.wt/conflicts/<tree>.json` are
  written in the **merge format** with `source_branch` = short id of `S`,
  `source_tip` = `S`, `target_tip` = the progress tip, `base` = `S`'s first
  parent and `message` = `S`'s message.
- `.wt/trees/<tree>/REWRITE_STATE.json` (`RewriteState`) records `op`
  (`cherry-pick` / `rebase`), `branch`, `orig_tip`, `onto`, `current` (=
  `S`), `todo` (remaining steps), `picked` (original → new ids), `skipped`,
  `no_snapshot`, `started_at`.
- The branch points at the progress tip (steps completed before the
  conflict are already snapshots).
- The CLI prints the conflicts and exits with code **3**.

Resolving uses the merge commands unchanged:

| Command | Effect during a stopped cherry-pick / rebase |
|---|---|
| `wt merge status` | lists the conflicts of the stopped step |
| `wt merge resolve PATH --ours\|--theirs\|--file F`, `--all` | resolves conflicts (`ours` = progress, `theirs` = the replayed snapshot) |
| `wt merge --continue`, `wt merge abort`, `wt snapshot` | **refused** with a hint naming `wt <op> --continue/--skip/--abort` (they would create a two-parent merge snapshot) |
| `wt cherry-pick --continue` / `wt rebase --continue` | requires no conflict markers left; snapshots the working tree as the step's replacement (secret scan applies; an empty result counts as skipped) and replays the remaining steps |
| `wt cherry-pick --skip` / `wt rebase --skip` | drops the step: resets the working tree to the progress tip and replays the remaining steps |
| `wt cherry-pick --abort` / `wt rebase --abort` | moves the branch back to `orig_tip`, resets the working tree to it (files created meanwhile that no side tracks are kept), clears all state, reflog entry `<op>: aborted` |

`--continue` of one operation while the other is in progress is refused.
The background daemon does not auto-snapshot while `MERGE_STATE.json`
exists, so it never interferes with a stopped step.

## 5. Stash

```
wt stash [push] [-m MSG] [-u|--include-untracked]
wt stash list
wt stash show [N] [-p]
wt stash apply [N]
wt stash pop [N]
wt stash drop [N]
wt stash clear
```

Entries are stored per tree in `.wt/trees/<tree>/stash.jsonl`, one JSON
`StashEntry` per line, oldest first. `N` counts from the newest entry (0)
and may be written `stash@{N}`.

```json
{"id":"<uuid>","message":"WIP on main: 1a2b3c4d subject","branch":"main",
 "base":"<head snapshot id>","base_manifest":"<hash>","manifest":"<hash>",
 "include_untracked":false,"auto":false,"created_at":"...",
 "changes":[{"path":"a.txt","kind":"modified"}]}
```

- **push** compares the working tree with the head snapshot. Modified and
  deleted files are stashed; files absent from the head snapshot
  ("untracked", i.e. new files) only with `--include-untracked`. The
  stashed state is stored as a manifest (head entries with the stashed
  paths replaced); the head's manifest is recorded as `base_manifest`.
  Stashed paths are then reset to the head snapshot (new stashed files are
  deleted). Nothing to stash is an error. Refused during a merge,
  cherry-pick or rebase.
- **apply** three-way merges the entry into the working tree: base =
  `base_manifest`, ours = the current working tree (blobs are stored
  first), theirs = `manifest`. The working tree may be dirty and the head
  may have moved. Conflicting text files get markers (`ours (working
  tree)` / `theirs (stash@{N}: message)`), other conflicts keep the working
  copy. No merge state is recorded: edit the files, then snapshot.
  Exit code 3 on conflicts.
- **pop** = apply, then drop the entry **unless** the apply conflicted.
- **show** diffs `base_manifest` → `manifest` (names by default, `-p`
  patches).
- **drop** / **clear** remove entries; objects are left to garbage
  collection.

### GC roots

`Repository::stash_roots()` returns the `manifest` and `base_manifest` of
every entry of every tree; `wt gc` keeps them and the objects they
reference. Because the base manifest is stored by hash, an entry stays
applicable even after its base snapshot is rewritten by a rebase.
`Repository::rewrite_roots()` returns the snapshots a stopped
cherry-pick/rebase still needs (`orig_tip`, `onto`, `current`, `todo`).

## 6. Auto-stash on Branch Switch

With `auto_snapshot.on_branch_switch = "stash"` (worktree or tree config,
`wt config set auto_snapshot.on_branch_switch stash`), `wt switch NAME`:

1. checks that `NAME` exists and no merge is in progress;
2. if the working tree is dirty, stashes everything (new files included)
   as `auto-stash on <current branch>` with `auto = true`;
3. switches (if the switch fails, the auto-stash is popped back);
4. pops the newest `auto` entry whose `branch` is `NAME`, if any.

`--force` (discard changes), `-c` (create a branch and carry the changes)
and switching to the current branch bypass the auto-stash. The `--json`
output of `wt switch` gains `stashed` and `restored`. A conflicting pop
leaves the entry, prints the conflicts and exits 3 (the switch itself
succeeded). The other modes are unchanged: `refuse` (default) refuses a
dirty switch, `snapshot` asks the daemon to snapshot first (BgProcess.md).

---

## Implementation Status

| Feature | Status |
|---|---|
| Cherry-pick of one or more revisions, skip of already-applied changes | Built (v0.6 §14.5) |
| `--no-snapshot`, `--continue`, `--skip`, `--abort` | Built (v0.6 §14.5) |
| Rebase onto a revision, up-to-date / fast-forward detection | Built (v0.6 §14.5) |
| Pushed-history guard and `--force` | Built (v0.6 §14.5) |
| `reverts` remapping, dependencies kept, re-signing | Built (v0.6 §14.5) |
| Conflict state shared with `wt merge resolve/status` | Built (v0.6 §14.5) |
| Reflog entries `cherry-pick` / `rebase` | Built (v0.6 §14.5) |
| Stash push/list/show/apply/pop/drop/clear, `--include-untracked` | Built (v0.6 §14.5) |
| Three-way stash apply onto a dirty tree / moved head | Built (v0.6 §14.5) |
| `on_branch_switch = "stash"` in `wt switch` | Built (v0.6 §14.5) |
| `stash_roots()` / `rewrite_roots()` for local GC | Built (v0.6 §14.5) |

## Roadmap

- Interactive rebase (reorder, squash, fixup, edit, reword) and
  `--autosquash`.
- `wt rebase --onto NEWBASE UPSTREAM` (three-argument form) and replaying
  merges with their topology (`--rebase-merges`).
- Patch-id based detection of changes already upstream (today a step is
  skipped only when it produces no change).
- Cherry-pick ranges (`A..B`), `-x` provenance lines and `--allow-empty`.
- `wt stash branch NAME`, partial stashes (`wt stash push PATH...`),
  `--keep-index`-like modes, stash entries shared through the server.
- Conflict state for stash apply (resolvable with `wt merge resolve`).
- Showing a stopped cherry-pick / rebase in `wt status`.

## References

- `crates/worktree-sdk/src/repo/rewrite.rs` — cherry-pick, rebase, `RewriteState`, `rewrite_roots`
- `crates/worktree-sdk/src/repo/stash.rs` — stash, `StashEntry`, `stash_roots`, `switch_branch_autostash`
- `crates/worktree-sdk/src/repo/merging.rs` — `three_way`, conflict state, refusal hooks
- `crates/worktree-sdk/src/repo/snapshotting.rs` — `snapshot` refusal during a stopped step
- `crates/worktree-sdk/src/merge/mod.rs` — `merge_manifests`, `merge_text_with`
- `crates/worktree-sdk/src/signing.rs` — `sign_snapshot`
- `crates/worktree-sdk/src/config.rs` — `OnBranchSwitch`
- `crates/worktree-cli/src/commands/{cherry_pick,rebase,stash,branch,merge}.rs`
- `crates/worktree-sdk/tests/{rewrite,rewrite_signing,stash}.rs`, `crates/worktree-cli/tests/rewrite.rs`
