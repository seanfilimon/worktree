# Background Daemon (`worktree-bg`) Specification

> **Version:** 0.5 — describes the daemon as built (`crates/worktree-bg`, `crates/worktree-sdk/src/ipc.rs`, `docs/ENGINEERING_PLAN.md` §6 and §13.11).
> Features described in earlier drafts but not built are listed under [Roadmap](#roadmap).

---

## Table of Contents

1. [Overview](#1-overview)
2. [What the Daemon Is and Is Not](#2-what-the-daemon-is-and-is-not)
3. [Architecture](#3-architecture)
4. [Lifecycle](#4-lifecycle)
5. [Auto-Snapshot Pipeline](#5-auto-snapshot-pipeline)
6. [Auto-Staging](#6-auto-staging)
7. [Background Fetch](#7-background-fetch)
8. [Maintenance](#8-maintenance)
9. [IPC Protocol](#9-ipc-protocol)
10. [Configuration](#10-configuration)
11. [CLI Commands](#11-cli-commands)
12. [Errors and Recovery](#12-errors-and-recovery)
13. [Security](#13-security)
14. [Platform Support](#14-platform-support)
15. [Implementation Status](#implementation-status)
16. [Roadmap](#roadmap)
17. [References](#references)

---

## 1. Overview

`worktree-bg` is an optional per-user process that makes W0rkTree automatic.
For every worktree it watches, it:

- takes **auto-snapshots** of trees with changes after a quiet period;
- uploads them as **staged snapshots** so the team sees work in progress;
- keeps **remote-tracking branches** fresh by subscribing to server branch
  events;
- runs periodic **maintenance** (reflog expiry, stale lock cleanup).

Everything it does can be done by hand (`wt snapshot`, `wt stage`, `wt fetch`,
`wt reflog expire`). The `wt` CLI performs all version-control operations
in-process through `worktree-sdk` and never requires the daemon.

One daemon serves all worktrees of a user. It is identified by its `$WT_HOME`
(default `~/.wt`): two daemons with different `WT_HOME` values are
independent.

---

## 2. What the Daemon Is and Is Not

It **is**:

- a long-running Tokio process (`worktree-bg run`) started by `wt daemon
  start`, a login service, or by hand;
- a client of the same SDK the CLI uses: it calls `Repository::snapshot`,
  `remote::stage` and `remote::fetch`, and takes the same per-tree locks
  (`.wt/trees/<tree>/LOCK`), so it never interleaves writes with the CLI;
- controlled over a loopback TCP socket with newline-delimited JSON.

It is **not**:

- required: no command routes through it;
- a server: it listens only on `127.0.0.1` and answers only IPC requests that
  carry its token;
- an enforcement point: access, protection and license checks happen on the
  server;
- a merger: it never merges, fast-forwards or checks out local branches; it
  only updates `refs/remotes/<remote>/*`;
- a pusher: auto-snapshots never become branch history on the server until
  you run `wt push`.

---

## 3. Architecture

```
                         $WT_HOME/bg.json  {pid, port, version, token}
   wt CLI ──NDJSON/TCP 127.0.0.1:port──►  Daemon (daemon.rs)
                                            │  watch list: $WT_HOME/bg.toml
                                            │
                  ┌─────────────────────────┼──────────────────────────┐
                  ▼                         ▼                          ▼
        per worktree: notify watcher   per worktree: worker task   per worktree: sync task
        (watcher/mod.rs)               (worker.rs)                  (WatchBranches stream)
          │ EventForwarder:              │ Debouncer                   │
          │  drop .wt/, reads,           │  (watcher/debounce.rs)      │ fetch on event,
          │  ignored paths;              │ RepoSnapshotter             │ periodic fallback,
          │  reload on config/ignore     │  (snapshot.rs)              │ backoff when offline
          ▼                              ▼                             ▼
        WatchEvent::{Changed,         Repository::snapshot        remote::fetch
         Settings, Error}             remote::stage
```

| Module | Responsibility |
|---|---|
| `main.rs` | CLI (`run [--foreground]`, `service [--platform]`), logging to `bg.log`, signal handling |
| `daemon.rs` | `Daemon`: watch manager, IPC dispatch, `run()` lifecycle, single-instance check |
| `info.rs` | `bg.json` writer; removes it on drop if it still belongs to this instance |
| `config.rs` | `bg.toml` (`BgConfig { watched }`), atomic writes |
| `paths.rs` | Canonical root paths (symlinks resolved, Windows `\\?\` stripped) |
| `watcher/` | `notify` integration, path classification (`filter.rs`), debouncer |
| `worker.rs` | Per-worktree event loop, `WatchStats`, `TriggerReport` |
| `snapshot.rs` | One auto-snapshot pass: snapshot dirty trees, then stage |
| `service.rs` | Login-service definitions (systemd, launchd, Task Scheduler) |

---

## 4. Lifecycle

### 4.1 Startup

`worktree-bg run [--foreground]`:

1. Create `$WT_HOME`, open `bg.log` (rotated to `bg.log.old` when larger than
   10 MiB); log level from `WT_LOG` (default `info`); `--foreground` also logs
   to stderr.
2. If `bg.json` exists and its daemon answers `Status`, exit successfully
   with "already running" (one daemon per `WT_HOME`). A stale or unreadable
   `bg.json` is replaced.
3. Load `bg.toml`, bind `127.0.0.1:0` (an ephemeral port), generate a random
   token and write `bg.json = {"pid", "port", "version", "token"}`
   atomically.
4. Serve IPC; in parallel, restore every watch listed in `bg.toml`. A
   worktree that cannot be opened stays listed and is reported with
   `last_error = "not watching: ..."`.

`wt daemon start` locates `worktree-bg` next to `wt` or on `PATH`, spawns
`worktree-bg run` detached and waits up to 15 s for it to answer `Status`
(pointing at `bg.log` if it exits early). `wt daemon start --foreground` runs
it attached to the terminal. `wt init --watch` and `wt clone --watch` start
the daemon if needed and watch the new worktree.

### 4.2 Running

| Activity | Trigger |
|---|---|
| File watching | Continuous, one recursive `notify` watcher per worktree |
| Auto-snapshot pass | Debouncer deadline (§5), or `SnapshotNow` |
| Auto-stage | After a pass that created snapshots (§6) |
| Background fetch | `BranchEvent` from `WatchBranches`, or every `sync.interval_secs` (§7) |
| Config/ignore reload | File events on `.wt/config.toml`, `.wt/ignore`, `.wtignore`, `.wt-tree/*` |
| Maintenance | Every 6 h (§8) |
| IPC | Continuous; one task per connection |

### 4.3 Shutdown

Triggered by `wt daemon stop` (`Shutdown` request), Ctrl-C, or SIGTERM on Unix:

1. Stop accepting IPC connections; in-flight requests get up to 5 s (so the
   `Shutdown` response is delivered).
2. For every watched worktree with pending changes, take a final
   auto-snapshot.
3. Drop the watchers and wait up to 10 s for workers to finish an in-flight
   pass.
4. Remove `bg.json` (only if it still names this instance) and exit 0.

`wt daemon restart` = stop, wait for exit, start.

### 4.4 Crash behaviour

There is no journal: every SDK write is atomic (temp file + rename) and object
writes are idempotent, so a crash leaves at worst an unused object or a torn
last reflog line (skipped on read). Per-tree locks are OS locks and are
released when the process dies. After a crash `bg.json` is stale; clients
treat a refused connection as "daemon not running" (`SdkError::DaemonNotRunning`)
and the next `wt daemon start` replaces the file. On restart, watches are
restored from `bg.toml`; changes made while the daemon was down are picked up
by the next pass (snapshots compare the full working state, not an event log).

---

## 5. Auto-Snapshot Pipeline

```
notify event
   │  is_change? (pure reads and access-time updates are dropped)
   ▼
classify(root, path)
   ├─ outside the worktree / the root itself  → dropped
   ├─ .wt/…                                   → Internal (reload if .wt/config.toml or .wt/ignore)
   ├─ …/.wt-tree/…                            → Internal (always reload)
   └─ anything else                           → Change{rel, reload = (file is .wtignore)}
   ▼
is_ignored(rel)?  yes → dropped     (errors fail open: the change is kept)
   ▼
worker: Debouncer.record(rel)       pending = set of distinct paths
   ▼
due when: enabled && !paused && pending non-empty, and
          (now ≥ last_event + inactivity_secs  ||  pending ≥ max_changed_files  ||  changed bytes ≥ max_changed_bytes)
   ▼
pass (RepoSnapshotter): reopen Repository (fresh config), for each tree:
   skip if a merge is in progress or the tree is clean,
   else Repository::snapshot(auto_generated = true, message = message_format)
   ▼
stage the snapshotted trees (§6), update WatchStats
```

Details:

- **Granularity.** Debouncing is per worktree; a pass checks every tree of
  the worktree and snapshots each one that has changes (one snapshot per
  dirty tree, on its current branch).
- **Settings** come from the effective `[auto_snapshot]` section and are
  reloaded when config files change. `enabled = false` turns auto-snapshots
  off without unwatching; `max_changed_files = 0` disables the burst limit.
- **Message.** `auto_snapshot.message_format` with placeholders `{count}`,
  `{files}`, `{branch}`; default `auto: {count} files changed`.
- **Pause.** `wt daemon pause` keeps collecting changes but never fires;
  `resume` re-arms. `SnapshotNow` (`wt daemon snapshot`) fires immediately,
  ignoring pause and `enabled`.
- **Same pipeline as manual snapshots:** hooks run, the secret scan runs (a
  finding skips that tree and is reported in `last_error`), signing applies
  when `[signing] auto = true`, the reflog records `auto-snapshot`.
- **Merges.** A tree with a merge in progress is never auto-snapshotted.
- **Branch switches.** With `auto_snapshot.on_branch_switch = "snapshot"`,
  `wt switch` takes an auto-snapshot of a dirty tree before switching (in the
  CLI process; the daemon is not involved).

---

## 6. Auto-Staging

After a pass that created snapshots, if

- `sync.auto_stage = true` (default), and
- `[remote.<sync.remote>]` exists (default remote `origin`), and
- `$WT_HOME/credentials.toml` has a credential for that server URL,

the daemon calls `remote::stage(repo, tree, remote)` for each snapshotted
tree. Because the working tree equals the new snapshot, the staged id is the
auto-snapshot's id. Staging sends `StageUploadRequest {tenant_id, tree_id,
branch_id, branch_name, snapshot_id, files_changed, files_added,
files_modified, files_deleted, message}` after uploading missing objects; the
server keeps one staged entry per (tree, account, branch) and publishes
`snapshot.staged`.

Not logged in → staging is skipped silently (debug log). Failures are
recorded per tree in `last_error`; nothing is queued or retried until the
next pass. A successful `wt push` clears the pusher's staged entry for that
branch; `wt stage --discard` / `wt unstage` remove it via `Unstage`.

---

## 7. Background Fetch

When `sync.auto_fetch = true` (default) and the worktree has a remote with a
saved credential, the daemon keeps a sync task per watched worktree:

1. Open `WatchBranches{tree_ids}` for the worktree's linked trees (trees that
   already have a server id). The server streams a `BranchEvent {tree_id,
   branch_name, old_tip, new_tip, actor, at}` for every `branch.pushed`,
   `branch.created` and `branch.deleted` event on trees the caller can read.
2. On each event, run `remote::fetch` for that tree: new snapshots and
   objects are downloaded and `refs/remotes/<remote>/<branch>` is updated.
   Local branches and working files are never touched.
3. Every `sync.interval_secs` (default 300) fetch every linked tree as a
   fallback, even without events.
4. When the server is unreachable or the stream breaks, reconnect with
   exponential backoff from 1 s to 5 min.

Users integrate fetched work explicitly with `wt pull` or `wt merge
origin/<branch>`.

---

## 8. Maintenance

Every 6 hours, for each watched worktree:

- expire reflog entries older than the retention (default 90 days) and beyond
  the per-tree cap (default 1000 entries), through the same SDK API as
  `wt reflog expire`;
- remove stale lock files left by dead processes.

---

## 9. IPC Protocol

### 9.1 Transport

- TCP on `127.0.0.1:<port>`; the port and token are read from
  `$WT_HOME/bg.json`.
- One request line and one response line per connection, each a JSON object
  terminated by `\n`, at most 1 MiB.
- Timeouts: 5 s to connect, send and (for quick requests) receive; 300 s for
  `watch` and `snapshot_now`.
- Every request carries the daemon's `token`; requests with a missing or
  wrong token get an `error` response and no effect.

### 9.2 Requests (`IpcRequest`, tagged by `cmd`)

```json
{"cmd":"status"}
{"cmd":"watch","path":"/home/ada/src/demo"}
{"cmd":"unwatch","path":"/home/ada/src/demo"}
{"cmd":"pause","path":null}
{"cmd":"resume","path":"/home/ada/src/demo"}
{"cmd":"snapshot_now","path":"/home/ada/src/demo"}
{"cmd":"shutdown"}
```

Paths must be absolute. `watch` accepts any directory inside a worktree and
resolves it to the canonical worktree root; `unwatch`, `pause`, `resume` and
`snapshot_now` find the deepest watched root containing the path. `pause` /
`resume` with `path: null` apply to every watched worktree. `watch` and
`unwatch` persist the change to `bg.toml`.

### 9.3 Responses (`IpcResponse`, tagged by `result`)

```json
{"result":"ok","message":"watching /home/ada/src/demo"}
{"result":"error","message":"not a worktree repository (no .wt directory found)"}
{"result":"status","pid":4242,"version":"0.1.0","started_at":"2026-10-25T09:00:00Z",
 "watched":[{"root":"/home/ada/src/demo","paused":false,"pending_changes":2,
             "snapshots_created":17,"last_snapshot":"5d0c2f7e-…",
             "last_snapshot_at":"2026-10-25T11:58:02Z","last_stage_at":"2026-10-25T11:58:03Z",
             "last_error":null}]}
```

`snapshot_now` answers `ok` with a summary ("created 2 snapshots, staged 2
tree(s)") or `error` with the per-tree failures. Malformed requests get
`{"result":"error","message":"invalid request: ..."}`.

### 9.4 Clients

Rust: `worktree_sdk::ipc::send(&IpcRequest::Status).await` (reads `bg.json`,
returns `SdkError::DaemonNotRunning` when it is missing or the port refuses
connections), or `send_to(port, ...)`. Servers: `ipc::serve(listener,
handler, shutdown)`.

---

## 10. Configuration

| Setting | Where | Default |
|---|---|---|
| Watched worktrees | `$WT_HOME/bg.toml` `watched = ["..."]` | empty |
| Daemon discovery | `$WT_HOME/bg.json` (written by the daemon) | — |
| Log file / level | `$WT_HOME/bg.log`, env `WT_LOG` | `info` |
| Auto-snapshots on/off | `auto_snapshot.enabled` | `true` |
| Quiet period | `auto_snapshot.inactivity_secs` (alias `inactivity_timeout_secs`) | `30` |
| Burst limits | `auto_snapshot.max_changed_files`, `auto_snapshot.max_changed_bytes` | `50`, unset |
| Message | `auto_snapshot.message_format` | `auto: {count} files changed` |
| Auto-stage | `sync.auto_stage` | `true` |
| Remote | `sync.remote` | `origin` |
| Background fetch | `sync.auto_fetch` | `true` |
| Fetch fallback interval | `sync.interval_secs` | `300` |

The `[auto_snapshot]` and `[sync]` values are read from the worktree's
effective configuration (`$WT_HOME/config.toml` < `.wt/config.toml` <
`.wt-tree/config.toml`, see [DotWt.md](../dot-wt/DotWt.md)) on every pass, so
edits apply without restarting.

```toml
# $WT_HOME/bg.toml
watched = [
    "/home/ada/src/demo",
    "/home/ada/src/platform",
]
```

---

## 11. CLI Commands

| Command | Effect |
|---|---|
| `wt daemon start [--foreground]` | Start (or report "already running") |
| `wt daemon stop` | `Shutdown` |
| `wt daemon restart` | Stop and start |
| `wt daemon status` | `Status` rendered as a table (`--json` for the raw object) |
| `wt daemon watch [PATH]` | Watch the worktree containing `PATH` (default: current) |
| `wt daemon unwatch [PATH]` | Stop watching (a no-longer-existing path is passed through so stale entries can be removed) |
| `wt daemon pause` / `resume` | Pause/resume every watched worktree |
| `wt daemon snapshot` | `SnapshotNow` for the current worktree |
| `wt daemon logs [-n N] [--follow]` | Print / follow `$WT_HOME/bg.log` |
| `wt init --watch`, `wt clone --watch` | Start the daemon if needed and watch the new worktree |
| `worktree-bg service [--platform systemd|launchd|windows]` | Print a login-service definition with install instructions |

`wt status` prints a hint when auto-snapshots are enabled for the worktree but
the daemon is not running.

---

## 12. Errors and Recovery

| Situation | Behaviour |
|---|---|
| Worktree missing or not a worktree at startup | Listed with `last_error`, not watched |
| Watcher error | Logged, `last_error` set; watching continues |
| Config reload fails | `last_error` set; previous settings kept |
| Ignore check fails | Change kept (fail open) |
| Snapshot fails (secrets, hook, I/O) | Error per tree in `last_error`; other trees continue; retried on the next change |
| Stage fails | Error in `last_error`; retried with the next pass |
| Server unreachable | Background fetch backs off 1 s → 5 min; snapshots continue locally |
| Merge in progress | Tree skipped |

`last_error` is cleared by the next pass that completes without errors.

---

## 13. Security

- The IPC socket is bound to loopback only and requires the random token from
  `bg.json`; on a shared machine, protect `$WT_HOME` with user-only
  permissions.
- `credentials.toml` (mode 0600 on Unix) holds bearer tokens; the daemon reads
  it to stage and fetch as the user.
- The daemon runs as the user and has exactly the user's file access; it
  stores no data outside `$WT_HOME` and the watched worktrees' `.wt/`.
- Server transport security is that of the configured remote URL
  (`https://` → TLS with ALPN h2; `http://` → cleartext h2c, intended for
  localhost).

---

## 14. Platform Support

| Capability | Linux | macOS | Windows |
|---|---|---|---|
| File watcher (`notify` 7) | inotify | FSEvents | ReadDirectoryChangesW |
| IPC | TCP loopback | TCP loopback | TCP loopback |
| Autostart (`worktree-bg service`) | systemd user unit (`~/.config/systemd/user/worktree-bg.service`) | launchd agent `dev.w0rktree.bg` | Task Scheduler logon task `W0rkTree\worktree-bg` |
| Signals | Ctrl-C, SIGTERM | Ctrl-C, SIGTERM | Ctrl-C |

The service definitions run `<exe> run` at login and restart on failure;
`worktree-bg service` only prints them, it never installs anything.

```ini
# worktree-bg service --platform systemd
[Unit]
Description=W0rkTree background daemon (auto-snapshots)

[Service]
ExecStart="/usr/local/bin/worktree-bg" run
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

---

## Implementation Status

| Feature | Status |
|---|---|
| `worktree-bg run`, single instance per `WT_HOME`, `bg.json`, `bg.toml`, `bg.log` | Built |
| Recursive watching, path classification, ignore filtering, config hot reload | Built |
| Debounced auto-snapshots (inactivity, file-count limit), pause/resume, snapshot now | Built |
| Auto-staging | Built |
| IPC over NDJSON/TCP | Built |
| Login-service definitions | Built |
| IPC token | Built (v0.5 §13.11) |
| Background fetch via `WatchBranches`, periodic fallback, backoff | Built (v0.5 §13.11) |
| Shutdown snapshot, maintenance (reflog expiry, stale locks) | Built (v0.5 §13.11) |
| `wt daemon restart/logs/snapshot`, `--watch` on init/clone, status hint | Built (v0.5 §13.11) |
| `max_changed_bytes`, `message_format`, `on_branch_switch` | Built (v0.5 §13.9) |

## Roadmap

Described in earlier drafts, not built and not in v0.5:

- **Virtual filesystem and lazy loading** of large files (FUSE, FUSE-T,
  ProjFS), a chunk cache with LRU eviction and pinning.
- **Automatic merging** of non-conflicting remote changes into local branches,
  "auto-merged" notices in `wt status`, pausing sync on conflicts.
- **Semantic event classifier** (code / config / dependency / cross-tree
  changes) and a pending changeset with byte deltas per event.
- **Routing CLI commands through the daemon** (IPC `snapshot.create`,
  `branch.switch`, `diff.compute`, ...) with a single-writer lock in the
  daemon; today the CLI does the work and file locks coordinate.
- **Per-worktree IPC endpoints** on Unix domain sockets / named pipes with
  owner-only permissions, length-prefixed framing.
- **Offline queue** of staged snapshots and a sync cursor; reflog upload to
  the server.
- **Operation journal** replayed after crashes; PID file in `.wt/`.
- **Platform-native storage directories** for objects and caches.
- **Token refresh** and certificate pinning.
- **Metrics** (`wt_watcher_events_total`, ...) and a Prometheus endpoint in the
  daemon; daily log rotation.
- **Windows Service** registration (the daemon uses a logon task instead).
- `[watcher]`, `[worker]`, `[storage]` config sections; env overrides
  `WT_SYNC_AUTO`, `WT_SNAPSHOT_AUTO`, `WT_SYNC_INTERVAL`, `WT_WORKER_SOCKET`,
  `WT_STORAGE_DIR`; `wt worker ...` command names (the commands are
  `wt daemon ...`).

## References

- `crates/worktree-bg/src/main.rs` — binary, logging, signals
- `crates/worktree-bg/src/daemon.rs` — `Daemon`, `run`, IPC handling
- `crates/worktree-bg/src/watcher/{mod,filter,debounce}.rs`
- `crates/worktree-bg/src/worker.rs`, `src/snapshot.rs`
- `crates/worktree-bg/src/{config,info,paths,service}.rs`
- `crates/worktree-sdk/src/ipc.rs` — `IpcRequest`, `IpcResponse`, `DaemonStatus`, `DaemonInfo`, `send`, `serve`
- `crates/worktree-sdk/src/remote/{stage,fetch,watch}.rs`
- `crates/worktree-sdk/src/config.rs` — `[auto_snapshot]`, `[sync]`
- `crates/worktree-cli/src/commands/daemon.rs`
- `crates/worktree-protocol/proto/sync.proto` — `WatchBranches`, `BranchEvent`, `StageUploadRequest`, `Unstage`
- `services/server-go/internal/syncsvc/watch.go`, `internal/staged/`
- `docs/daemon.md` — user guide
