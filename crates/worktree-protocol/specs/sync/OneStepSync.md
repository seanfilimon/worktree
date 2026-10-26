# One-Step Sync and Offline Queue Specification

> **Version:** 0.7 — describes `wt sync`, the offline outbox (`wt push
> --queue`, `wt sync queue|flush|drop`) and the daemon's queue flushing and
> auto-sync as built (`crates/worktree-sdk/src/remote/sync_all.rs`,
> `crates/worktree-sdk/src/repo/outbox.rs`, `crates/worktree-bg/src/sync.rs`,
> `docs/ENGINEERING_PLAN.md` §15.5). Features described but not built are
> listed under [Roadmap](#roadmap).

## Overview

Keeping a tree in step with its server tree used to take three commands
(`wt fetch`, `wt pull` or `wt rebase origin/BRANCH`, `wt push`) and failed
outright when the server was unreachable. v0.7 adds:

- **`wt sync`** — fetch, integrate the remote copy of the current branch
  (merge or rebase) and push, in one command with one summary.
- **The offline outbox** — when the server cannot be reached, unpushed
  snapshots are recorded as a *queued push* instead of failing; queued
  pushes are delivered later by `wt sync flush` or automatically by the
  daemon.
- **Daemon auto-sync** — optionally a full `wt sync` of every clean tree
  on a schedule.

Everything is built from the existing client flows: `remote::fetch`
(Sync.md), `Repository::merge` (Tree.md) and `Repository::rebase`
(HistoryRewriting.md), and `remote::push`. No protocol messages or server
endpoints were added: the server sees ordinary `Pull`/`Push` calls.

---

## 1. `wt sync`

```
wt sync [--merge | --rebase] [--no-push] [--all-trees] [--remote R]
wt sync --continue | --abort
```

For the selected tree (`--tree`, else the tree containing the current
directory) and its current branch `B`, with remote `R` (`--remote`, else
`sync.remote`, default `origin`):

1. **Fetch** every remote branch into `refs/remotes/R/*`
   (`remote::fetch` semantics, including tags). A tree that does not exist
   on the server yet is not an error: the fetch is skipped and step 3
   creates it.
2. **Integrate** `R/B` into `B`, decided by `plan_integration`:

   | Local `B` vs `R/B` | Action |
   |---|---|
   | equal, `R/B` unknown, or `B` contains `R/B` | nothing (`up_to_date`) |
   | `B` empty or an ancestor of `R/B` | fast-forward + checkout (`fast_forward`) |
   | diverged, strategy `merge` | `Repository::merge(B, "R/B")` with message `Merge R/B into B` (`merged`) |
   | diverged, strategy `rebase` | `Repository::rebase(B, onto "R/B")` (`rebased`; `replayed` = snapshots replayed) |

   The strategy is `--merge` / `--rebase`, else `sync.strategy`
   (`"merge"` by default). Rebase replays only unpushed snapshots (the
   pushed-history guard of `wt rebase` applies unchanged). Integrating needs
   a clean working tree (`DirtyWorkingTree` otherwise); when there is
   nothing to integrate a dirty tree is fine and the push still happens.
3. **Push** `B` when it has snapshots and its tip differs from `R/B`
   (`remote::push`: negotiation, object upload, `Push` with `old_tip`,
   tags). `--no-push` stops after step 2.

### Conflicts

A conflicting merge or rebase step stops the sync exactly like `wt merge` /
`wt rebase`: conflict markers in the working tree, `MERGE_STATE.json` and
`.wt/conflicts/<tree>.json` (plus `REWRITE_STATE.json` for a rebase). The
summary lists the conflicts and the command exits with code **3**. Nothing
is pushed.

While the merge or rebase is pending, `wt sync` refuses and points at:

- `wt sync --continue` — finishes the integration (`continue_merge`, or
  `rebase_continue`, which may stop again at a later snapshot) once the
  markers are resolved (`wt merge status`, `wt merge resolve PATH
  --ours|--theirs|--file F`), then pushes. Remaining markers fail with
  exit 3 (`MergeConflict`).
- `wt sync --abort` — `abort_merge` / `rebase_abort`: the branch and the
  working tree return to their state before the integration.

The pending state is the ordinary merge/rebase state, so `wt merge
--continue` or `wt rebase --continue` followed by `wt push` is equivalent.

### `--all-trees`

Syncs every tree of the worktree on its own current branch, in tree order.
An error in one tree (not logged in, permission denied, ...) is reported for
that tree and the others still sync. Exit code 3 when any tree stopped with
conflicts, else 1 when any tree failed or was offline with nothing to queue,
else 0.

### Output

Human output is a per-tree summary:

```
Sync proj (main) with origin [merge]
  fetch   1 branch updated; received 2 snapshots and 5 objects
  merge   merged origin/main (merge snapshot 1a2b3c4d)
  push    9f8e7d6c..1a2b3c4d -> origin/main; sent 2 snapshots and 3 objects
Synced.
```

`--json` prints the `SyncReport` (an array of `TreeSync {tree, report,
error}` with `--all-trees`):

| Field | Meaning |
|---|---|
| `tree`, `branch`, `remote` | what was synced |
| `strategy` | `merge` / `rebase` |
| `status` | `up_to_date`, `synced`, `conflicts`, `aborted`, `queued`, `offline` |
| `fetch` | the `FetchReport`, `null` when not fetched |
| `integration` | `none`, `up_to_date`, `fast_forward`, `merged`, `rebased`, `conflicts`, `aborted` |
| `replayed` | snapshots replayed by a rebase |
| `old_tip`, `new_tip` | branch tip before / after |
| `push` | the `PushReport`, `null` when nothing was pushed |
| `queued` | the `OutboxEntry` when the push was queued |
| `conflicts` | `ConflictInfo` list when stopped |
| `offline_error` | why the server could not be reached |

Exit codes: 0 (including `queued`), 1 (`offline` with nothing queued, or
any error), 3 (`conflicts`).

---

## 2. The Offline Outbox

### Unreachable vs. refused

`remote::is_unreachable(err)` classifies a failure as "the server could not
be reached": a connect failure (`cannot connect to ...`), gRPC
`Unavailable`, `DeadlineExceeded`, or a transport error (connection
refused / reset, broken pipe). Every answer from the server — a push
rejection, `Unauthenticated`, `PermissionDenied`, quota — is *not*
unreachable and fails normally.

### Storage

`.wt/trees/<tree>/outbox.jsonl`, one `OutboxEntry` per line, oldest first;
the file is removed when the queue is empty.

| Field | Meaning |
|---|---|
| `id` | UUID |
| `remote`, `branch` | what to push |
| `tip` | the branch tip when it was (last) queued |
| `queued_at` | when first queued |
| `attempts` | failed delivery attempts |
| `last_attempt_at`, `last_error` | the last failed attempt |

There is at most one entry per `(remote, branch)`: queueing the branch again
moves `tip` forward and keeps the position and `queued_at`. Read-modify-write
operations hold the tree lock (`trees/<tree>/LOCK`) briefly; pushes run
without it. `Repository::outbox_roots()` lists queued tips for garbage
collection (they are normally branch tips anyway).

### Queueing

- `wt push --queue` — push; when the server is unreachable, queue instead
  (`remote::push_or_queue`) and exit 0. Without `--queue` an unreachable
  push fails with a hint naming `--queue`.
- `wt sync` — when the fetch finds the server unreachable and the branch is
  ahead of the last known `R/B`, the push is queued (`status: queued`, exit
  0); with nothing to push (or `--no-push`) the status is `offline` (exit
  1). A server that disappears between fetch and push is handled the same
  way.

A successful push of a branch (by `wt sync`, `wt push --queue` or a flush)
removes that branch's entries.

### Managing the queue

| Command | Effect |
|---|---|
| `wt sync queue [--all-trees]` (alias `wt sync status`, or `wt sync --status`) | list queued pushes, numbered from 1 (oldest), with attempts and the last error |
| `wt sync flush [--all-trees]` | retry every entry (`remote::flush_outbox`); exit 1 while any remain |
| `wt sync drop N` | forget entry N without pushing it |

A flush pushes each entry's branch at its **current** tip (snapshots made
after queueing go out too). A delivered entry is removed; a failed one
stays with `attempts`/`last_error` updated. After one entry of a remote is
found unreachable, that remote's other entries are not tried again in the
same pass. A queued push the server refuses (typically because someone else
pushed meanwhile) stays queued; `wt sync` integrates the remote changes,
pushes, and clears it.

---

## 3. Daemon: Auto-Flush and Auto-Sync

Each watched worktree runs, next to the fetch loop (BgProcess.md), an
**outbox loop** (`worktree_bg::sync::run_outbox`) that re-reads the
worktree config every 10 s (`SyncSettings::outbox_poll`):

- **Auto-flush** (`sync.auto_flush = true`, the default): when any tree has
  queued pushes the loop runs `flush_outbox` for those trees. A pass that
  leaves entries queued backs off exponentially (1 s doubling to 5 min,
  the same `Backoff` as the fetch loop); a pass that delivers everything
  resets it. A change of the queue (new entry or newer tip) is tried at once
  regardless of the backoff.
- **Auto-sync** (`sync.auto_sync_secs = N`, default 0 = off; only when the
  sync remote is configured): every N seconds, for every tree that is clean
  (`status` reports no changes) and has no merge or rebase pending, run
  `remote::sync` with `abort_on_conflict`: a conflicting integration is
  undone at once (`status: aborted`, logged as a warning) so the working
  tree never receives conflict markers unattended; pushes that cannot reach
  the server are queued and then flushed by auto-flush.

Results are logged to `<wt home>/bg.log`.

---

## 4. Configuration

| Key | Type | Default | Meaning |
|---|---|---|---|
| `sync.strategy` | `"merge"` \| `"rebase"` | `"merge"` | how `wt sync` integrates diverged history |
| `sync.auto_flush` | bool | `true` | the daemon delivers queued pushes |
| `sync.auto_sync_secs` | integer | `0` | daemon auto-sync period (0 = off) |

The keys live in `[sync]` of `.wt/config.toml`, are understood by `wt config
get/set/list`, and default values are not written to the file.
`sync.remote` (default `origin`) names the remote for `wt sync` and the
daemon.

---

## Implementation Status

| Feature | Status |
|---|---|
| `wt sync` fetch → fast-forward / merge / rebase → push, summary and `--json` | Built (v0.7 §15.5) |
| `--merge`, `--rebase`, `--no-push`, `--remote`, `--all-trees` | Built (v0.7 §15.5) |
| Conflicts exit 3, `wt sync --continue` / `--abort` | Built (v0.7 §15.5) |
| Trees not yet on the server are created by the push | Built (v0.7 §15.5) |
| Outbox (`outbox.jsonl`), one entry per remote/branch | Built (v0.7 §15.5) |
| `wt push --queue`, auto-queue in `wt sync` | Built (v0.7 §15.5) |
| `wt sync queue` (`status`, `--status`), `flush`, `drop N` | Built (v0.7 §15.5) |
| Daemon auto-flush with backoff, immediate retry on queue change | Built (v0.7 §15.5) |
| Daemon auto-sync of clean trees with conflict abort | Built (v0.7 §15.5) |
| `sync.strategy`, `sync.auto_flush`, `sync.auto_sync_secs` config keys | Built (v0.7 §15.5) |

## Roadmap

- Syncing branches other than the current one (`wt sync --branch B`, all
  tracking branches).
- Queue state in `wt status` and `wt daemon status` (queued count, last
  flush, last auto-sync).
- Queueing staged snapshots (`wt stage`) and tag pushes while offline.
- Auto-stash of a dirty working tree around the integration
  (`wt sync --autostash`).
- A desktop notification when an unattended auto-sync is aborted by
  conflicts.

## References

- `crates/worktree-sdk/src/remote/sync_all.rs` — `sync`, `sync_continue`,
  `sync_abort`, `sync_all`, `push_or_queue`, `flush_outbox`,
  `is_unreachable`, `SyncReport`
- `crates/worktree-sdk/src/repo/outbox.rs` — `OutboxEntry`, `outbox`,
  `outbox_queue`, `outbox_drop`, `outbox_remove`, `outbox_record_attempt`,
  `outbox_roots`
- `crates/worktree-sdk/src/remote/fetch.rs` — `fetch_with`,
  `plan_integration`
- `crates/worktree-sdk/src/config.rs` — `SyncSection`, `SyncStrategy`
- `crates/worktree-bg/src/sync.rs` — `run_outbox`, `Backoff`, `SyncSettings`
- `crates/worktree-cli/src/commands/synchronize.rs` — `wt sync`
- `crates/worktree-cli/src/commands/sync.rs` — `wt push --queue`
- Tests: `crates/worktree-sdk/tests/sync_outbox.rs`,
  `crates/worktree-bg/tests/outbox_loop.rs`,
  `crates/worktree-e2e/tests/e2e/one_step_sync.rs`
