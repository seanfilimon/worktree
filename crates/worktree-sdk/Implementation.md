# Implementation Notes — `worktree-sdk`

`worktree-sdk` is the **client facade** for operating on a worktree. It no
longer contains VCS logic — that moved to the `worktree-engine` crate in
WT-PHASE-1.

## Architecture

```
src/
├── lib.rs      # Re-exports Client, SdkError, and engine result types
├── client.rs   # Client: the single entry point (open / init / all ops)
├── remote.rs   # IPC client to worktree-bg (stub until WT-PHASE-3)
└── error.rs    # SdkError: Engine(EngineError) | DaemonUnavailable
```

## Dispatch model (BgProcess.md §14)

`Client::open` prefers a running `worktree-bg` daemon over IPC and falls
back to an **embedded `worktree-engine`** when no daemon is listening
(degraded mode). The IPC path is a stub until the daemon lands in
WT-PHASE-3; today every call goes through the embedded engine.

Callers (the `wt` CLI, integrations) only ever see `Client` methods —
`status()`, `snapshot_create()`, `branch_*()`, `merge()`, `diff_*()`,
`tag_*()`, `tree_*()`, `sync_*()` — and never construct engines directly.

## Status

Accurate as of WT-PHASE-1. Local operations work end-to-end through the
embedded engine; server sync and daemon dispatch are not implemented yet
(see the phase plan in the repository root README).
