# worktree-bg

**Local background daemon** for W0rkTree. Runs on each developer machine,
watches registered worktrees and takes auto-snapshots (see
`docs/ENGINEERING_PLAN.md` §6).

## Usage

```
worktree-bg run [--foreground]     # serve until Ctrl-C / SIGTERM / IPC Shutdown
worktree-bg service [--platform systemd|launchd|windows]
                                   # print a login-service definition
```

Normally started and controlled through `wt daemon …`. `run` does not
detach; `--foreground` additionally logs to stderr. Log level comes from
`WT_LOG` (tracing `EnvFilter` syntax, default `info`).

## Per-user state (`$WT_HOME`, default `~/.wt`)

| File      | Purpose |
|-----------|---------|
| `bg.json` | `{pid, port, version}` — written on startup, removed on clean shutdown |
| `bg.toml` | `watched = ["<root>", …]` — persisted on watch / unwatch |
| `bg.log`  | daemon log (rotated to `bg.log.old` at startup past 10 MiB) |

A second `run` against the same `$WT_HOME` exits immediately if the daemon
in `bg.json` answers `Status`; a stale `bg.json` is replaced.

## How it works

* **IPC** — `worktree_sdk::ipc`: one JSON line each way over TCP on
  `127.0.0.1:<port>`.
* **Watching** — one recursive `notify` watcher per worktree. Events under
  `.wt/` and `.wt-tree/`, reads, and paths matched by
  `Repository::is_ignored` are dropped on the watcher thread; edits to
  `.wt/config.toml`, `.wt/ignore` or `.wtignore` reload the settings.
* **Debouncing** — per the worktree's `[auto_snapshot]` section: a snapshot
  is due `inactivity_secs` after the last change, or immediately once
  `max_changed_files` distinct paths changed. Paused worktrees keep counting
  changes but do not snapshot.
* **Snapshot pass** — every tree with changes gets
  `Repository::snapshot(auto_generated = true, "auto: N files changed")`;
  trees with a merge in progress are skipped. If `[sync].auto_stage` is set,
  the configured remote exists and a credential is saved, each snapshotted
  tree is staged with `remote::stage`. Failures are reported as
  `last_error` in `Status`, never fatal.
