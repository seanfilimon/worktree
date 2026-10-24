# worktree-bg

**Local daemon** for the W0rkTree VCS. Runs on every developer's machine.

This is **not** the remote server — that's `worktree-server`, slated to be
reimplemented in Go (see the current `crates/worktree-server/` for the
Rust shell that will be stripped down in WT-EXTRACT-3).

## Role

Watches the local filesystem, hashes content with BLAKE3, builds local
snapshots, and syncs them to the remote `worktree-server`. Owns all `.wt/`
and `.wt-tree/` local state.

## Spec

`../worktree-protocol/specs/bgprocess/BgProcess.md`

## Status

Skeleton only. Module structure is committed but no implementation yet.
Code moves from the current `worktree-server` crate into this one happen
in WT-EXTRACT-2.
