# Background daemon (`worktree-bg`)

`worktree-bg` is an optional per-user process that makes W0rkTree automatic: it watches your
worktrees, takes **auto-snapshots** when you pause, and uploads your work in progress as **staged
snapshots** so your team can see it. Everything it does can also be done by hand with
`wt snapshot` and `wt stage`; the CLI never requires the daemon.

Crate: [`crates/worktree-bg`](../crates/worktree-bg/README.md). IPC types:
[`worktree_sdk::ipc`](../crates/worktree-sdk/src/ipc.rs).

## Quick start

```bash
wt daemon start            # spawn worktree-bg in the background
cd ~/src/demo && wt daemon watch
# ...edit files, stop typing for 30 s...
wt log -n 1                # "auto: 3 files changed"
wt status --team           # your staged work, as teammates see it
wt daemon status
```

## Lifecycle

| Action | How |
|---|---|
| Start | `wt daemon start` spawns `worktree-bg run` detached. `wt daemon start --foreground` (or `worktree-bg run --foreground`) runs it in the terminal and also logs to stderr. |
| Discovery | On startup the daemon listens on a free port on `127.0.0.1` and writes `$WT_HOME/bg.json` = `{"pid": …, "port": …, "version": "…"}`. |
| Stop | `wt daemon stop` sends `Shutdown`; Ctrl-C works in the foreground. In-flight snapshots finish first, then `bg.json` is removed. |
| Stale state | If `bg.json` exists but the port refuses connections, clients report the daemon as not running. |
| Logs | `$WT_HOME/bg.log` (rotated to `bg.log.old` at startup once it exceeds 10 MiB). Level from `WT_LOG` (default `info`). |
| Autostart | `worktree-bg service` prints a login-service definition for this platform (systemd user unit, launchd agent, or Task Scheduler command) for you to install. |

The watch list survives restarts: it is persisted in `$WT_HOME/bg.toml` (`watched = ["…"]`) on every
`watch` / `unwatch`, and re-read on startup.

## Auto-snapshot pipeline

For each watched worktree:

```
notify watcher ──► filter ──► debouncer ──► auto-snapshot pass ──► auto-stage
 (recursive)       drop .wt/,    inactivity_secs     per tree with      if sync.auto_stage,
                   ignored paths or max_changed_files changes           remote + login exist
```

1. **Watch.** A recursive `notify` watcher reports file events. Events under `.wt/` and ignored
   paths are dropped. Changes to ignore files or to `.wt/config.toml` reload the rules.
2. **Debounce.** The worker collects distinct changed paths. A pass is due after
   `auto_snapshot.inactivity_secs` (default 30) seconds without new events, or immediately once
   `auto_snapshot.max_changed_files` (default 50) distinct paths have changed.
3. **Snapshot.** For every tree of the worktree that has changes, the daemon calls
   `Repository::snapshot` with `auto_generated = true` and message `auto: N files changed`. The
   secret scan runs as usual; a finding skips that snapshot and is reported as `last_error`.
   The repository is reopened on each pass, so config edits take effect immediately.
4. **Stage.** If `sync.auto_stage` is `true` (default), the remote named by `sync.remote` (default
   `origin`) is configured and a credential for its server exists, the daemon calls
   `remote::stage` for each snapshotted tree.

Rules:

- Nothing is snapshotted while a merge is in progress in that tree.
- `auto_snapshot.enabled = false` turns auto-snapshots off for a worktree without unwatching it.
- `wt daemon pause` / `resume` suspend and resume auto-snapshots (all worktrees, or one via the
  IPC `path`).
- Auto-snapshots go onto the tree's current branch like manual ones and appear in the reflog as
  `auto-snapshot`. They are never pushed automatically — `wt push` remains explicit.

## IPC protocol

Transport: newline-delimited JSON over TCP on `127.0.0.1:<port from bg.json>`. One request line and
one response line per connection.

Requests (`IpcRequest`, tagged by `cmd`):

```json
{"cmd":"status"}
{"cmd":"watch","path":"/home/ada/src/demo"}
{"cmd":"unwatch","path":"/home/ada/src/demo"}
{"cmd":"pause","path":null}
{"cmd":"resume","path":"/home/ada/src/demo"}
{"cmd":"snapshot_now","path":"/home/ada/src/demo"}
{"cmd":"shutdown"}
```

Responses (`IpcResponse`, tagged by `result`):

```json
{"result":"ok","message":"watching /home/ada/src/demo"}
{"result":"error","message":"not a worktree repository (no .wt directory found)"}
{"result":"status","pid":4242,"version":"0.1.0","started_at":"2026-10-24T09:00:00Z",
 "watched":[{"root":"/home/ada/src/demo","paused":false,"pending_changes":2,
             "snapshots_created":17,"last_snapshot":"5d0c2f7e-…",
             "last_snapshot_at":"2026-10-24T11:58:02Z","last_stage_at":"2026-10-24T11:58:03Z",
             "last_error":null}]}
```

Try it by hand (Unix):

```bash
PORT=$(jq .port "${WT_HOME:-$HOME/.wt}/bg.json")
echo '{"cmd":"status"}' | nc 127.0.0.1 "$PORT"
```

From Rust, use `worktree_sdk::ipc::send(&IpcRequest::Status).await`; it returns
`SdkError::DaemonNotRunning` when `bg.json` is missing or stale.

The IPC endpoint is unauthenticated and bound to loopback; see [security](./security.md#local-files).

## Configuration summary

| Setting | Where | Default |
|---|---|---|
| Watched worktrees | `$WT_HOME/bg.toml` | empty |
| Enable auto-snapshots | `.wt/config.toml` `auto_snapshot.enabled` | `true` |
| Quiet period | `auto_snapshot.inactivity_secs` | `30` |
| Burst threshold | `auto_snapshot.max_changed_files` | `50` |
| Auto-stage | `sync.auto_stage` | `true` |
| Remote for staging | `sync.remote` | `origin` |
| Log level | `WT_LOG` environment variable | `info` |

See [configuration](./configuration.md) for the full reference.

## Roadmap

The [BgProcess spec](../crates/worktree-protocol/specs/bgprocess/BgProcess.md) also describes
features that v2 does not implement: owner-only Unix sockets / named pipes for IPC, an offline sync
queue, large-file chunking, lazy loading (FUSE/ProjFS) and automatic merging of non-conflicting
remote changes.
