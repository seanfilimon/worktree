# worktree-bg

**Local daemon** for the W0rkTree VCS. Runs on every developer's machine.
The remote multi-tenant server is `worktree-server`.

## Role

Watches the working directory, creates auto-snapshots after inactivity (or
when enough files change), serves the `wt` CLI over IPC (named pipe / Unix
socket, one endpoint per worktree), and — from WT-PHASE-5 — syncs staged
snapshots to the remote server.

## Binary

```
worktree-bg run        # foreground daemon (blocks)
worktree-bg start      # detached start + readiness wait
worktree-bg stop       # graceful shutdown over IPC
worktree-bg status     # daemon info over IPC
worktree-bg install    # register with the OS service manager (login start)
worktree-bg uninstall
```

Users normally go through `wt server start|stop|status` instead.

## Status (verified against code)

- **Working**: daemon lifecycle, IPC server + full command dispatch
  (status, snapshot, log, branch, merge, diff, reflog), watcher +
  debouncer, auto-snapshot engine (inactivity + file-count triggers),
  event classification, service install (schtasks / systemd user unit /
  launchd agent).
- **Pending**: `sync/` is a stub until the server lands (WT-PHASE-4/5);
  lazy-loading VFS arrives in WT-PHASE-11.

## Spec

`../worktree-protocol/specs/bgprocess/BgProcess.md`
