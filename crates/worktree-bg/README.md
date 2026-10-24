# worktree-bg

The optional per-user **background daemon** for W0rkTree. It watches registered worktrees, takes
auto-snapshots after a quiet period, and uploads them as staged snapshots so the team can see work in
progress. It is not the server — that is the Go service in
[`services/server-go`](../../services/server-go/README.md).

```bash
cargo build -p worktree-bg --release
wt daemon start                # or: worktree-bg run --foreground
wt daemon watch ~/src/demo
wt daemon status
```

## Binary

| Command | Purpose |
|---|---|
| `worktree-bg run [--foreground]` | Run until Ctrl-C or an IPC `Shutdown`. Logs to `$WT_HOME/bg.log`; `--foreground` also logs to stderr. Level from `WT_LOG` (default `info`). |
| `worktree-bg service [--platform P]` | Print a login-service definition (systemd user unit, launchd agent, Task Scheduler command) that starts the daemon automatically. Nothing is installed. |

Normally you control it through `wt daemon …`.

## Behavior

- Advertises itself in `$WT_HOME/bg.json` (`pid`, `port`, `version`) and listens on `127.0.0.1` for
  newline-delimited JSON requests (`worktree_sdk::ipc`).
- Persists the watch list in `$WT_HOME/bg.toml` (`watched = [...]`).
- Per worktree: `notify` watcher → filter (`.wt/` and ignored paths dropped) → debounce
  (`auto_snapshot.inactivity_secs`, `auto_snapshot.max_changed_files`) → `Repository::snapshot`
  with `auto_generated = true`, message `auto: N files changed`, for every tree with changes → if
  `sync.auto_stage` and a remote + credential exist, `remote::stage`.
- Never snapshots a tree with a merge in progress. Never pushes.

## Modules

| Module | Responsibility |
|---|---|
| `daemon` | Watch manager, IPC request handling, process lifecycle |
| `watcher` | `notify` integration, event filtering, debouncing |
| `worker` | Per-worktree event loop and status counters |
| `snapshot` | The auto-snapshot + auto-stage pass |
| `config`, `info` | `bg.toml` and `bg.json` |
| `paths` | Canonical worktree-root paths (handles `\\?\` prefixes on Windows) |
| `service` | Login-service definitions |

## Documentation

- [Daemon guide](../../docs/daemon.md) — lifecycle, pipeline, IPC protocol.
- [Configuration](../../docs/configuration.md) — `[auto_snapshot]`, `[sync]`, `$WT_HOME` files.
- Spec (includes roadmap items): [BgProcess.md](../worktree-protocol/specs/bgprocess/BgProcess.md).
