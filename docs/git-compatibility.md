# Git compatibility

W0rkTree is not built on Git, but it can move history in both directions so that teams can migrate
gradually and keep Git-based tooling (CI, mirrors, code hosts) working.

| Command | Direction | Implemented by |
|---|---|---|
| `wt git import GIT_PATH [DEST]` | Git → new W0rkTree worktree | `worktree_git::import_repository` |
| `wt git export GIT_PATH [--rev REV]` | W0rkTree → Git repository | `worktree_git::export_repository` |

Both work on local Git repositories via libgit2 (crate [`worktree-git`](../crates/worktree-git/Implementation.md)).
To import from a hosted repository, `git clone` it first; to publish an export, `git push` from the
exported repository.

## Object mapping

| Git | W0rkTree |
|---|---|
| Commit | Snapshot (parents, author, timestamp, message preserved) |
| Commit SHA-1 | Snapshot id (UUID v5 of the SHA, see [determinism](#determinism)) |
| Tree (recursive) | Manifest (flat, sorted list of files) |
| Blob | Blob (re-hashed with BLAKE3) |
| Mode `100644` / `100755` | `executable = false` / `true` |
| Branch (`refs/heads/*`) | Branch of the root tree |
| Lightweight / annotated tag | Tag (annotated tag message → tag message) |
| `.gitignore` | `.wt/ignore` |
| Author `Name <email>` | Snapshot author `Name <email>` |

## Import

```bash
wt git import ~/src/legacy legacy
cd legacy
wt log --oneline
wt branch
```

`import_repository(git_path, dest, opts)`:

1. Creates a new worktree at `DEST` (default: the Git repository's directory name; must not already
   be a worktree).
2. Walks every local branch's commit DAG (or only `ImportOptions::branches`) in topological order,
   oldest first, and converts each commit into a snapshot **on the root tree**: the Git tree becomes
   a manifest, each distinct blob is stored once, parents are mapped to the snapshots of the parent
   commits.
3. Attributes each commit to the first branch containing it — the checked-out branch first, then the
   others by name — and sets each branch to its converted tip.
4. Converts tags (`ImportOptions::include_tags`; the CLI imports them).
5. Converts `.gitignore` into `.wt/ignore`.
6. Records every commit in `.wt/git-map.json` and checks out the Git repository's current branch.

The report (`ImportReport`) lists branches, snapshot, blob and tag counts and the checked-out branch.
History stays in the root tree; split it into nested trees afterwards with `wt tree add` if needed.

## Export

```bash
wt git export ../legacy-mirror.git          # all branches and tags
wt git export ../mirror --rev main          # only the history of one revision
cd ../legacy-mirror.git && git log --oneline main
```

`export_repository(repo, git_path, opts)`:

1. Opens the Git repository at `GIT_PATH`, creating it if missing (bare when `ExportOptions::bare`).
2. For each branch of the exported tree (default: the root tree; `ExportOptions::tree`) — or only the
   requested branches / revision — converts snapshots to commits, parents first.
3. Commits are built **only from snapshot data**: manifest → Git tree, snapshot author → author and
   committer, snapshot time in UTC, message terminated by a newline.
4. Snapshots already listed in `.wt/git-map.json` whose commit exists in the target are reused
   instead of rewritten, so repeated exports only write new commits (`ExportReport::commits`).
5. Updates branch and tag refs in the target. **W0rkTree is the source of truth**: existing refs with
   the same names are overwritten. In a non-bare target, the checked-out files follow `HEAD` only if
   the target had no local modifications.

## `.wt/git-map.json`

The mapping between snapshot ids and commit SHAs, written by both import and export:

```json
{
  "version": 1,
  "snapshots": {
    "5d0c2f7e-3b1a-5d55-9a43-0f6f6f2b7a10": "3b18e512dba79e4c8300dd08aeb37f8e728b8dad"
  }
}
```

Keys are sorted so the file is stable across runs. Deleting it is safe: export then rebuilds every
commit, and because commits are deterministic it produces the same SHAs.

## Determinism

Both directions are deterministic:

- **Import**: the snapshot id of an imported commit is a UUID v5 derived from the commit's lowercase
  hex SHA in a fixed namespace. Re-importing the same Git history — on any machine — yields the same
  snapshot ids, so independently imported copies agree.
- **Export**: a commit depends only on the snapshot's manifest, parents, author, timestamp (UTC) and
  message. Exporting the same snapshots always yields the same commit SHAs.

A Git → W0rkTree → Git round trip therefore reproduces commit SHAs only when the original commits
already had identical author and committer, a UTC (`+0000`) timestamp, a message ending in exactly one
newline, and no unsupported entries (see below). Otherwise content, structure and history shape are
preserved but SHAs differ.

## Limits

| Git feature | Behavior |
|---|---|
| Submodules (gitlinks) | Skipped with a warning; use a nested tree instead. |
| Symlinks | Skipped with a warning. |
| Non-UTF-8 file names | Skipped with a warning. |
| Committer distinct from author | Import keeps the author; export writes the author as committer. |
| Commit and tag signatures (GPG/SSH) | Not preserved. |
| Time-zone offsets | Converted to UTC. |
| Empty directories | Not represented (neither Git nor W0rkTree tracks them). |
| `.gitattributes`, Git LFS pointers | Imported as ordinary files; LFS content is not fetched. |
| Notes, reflogs, stashes, remotes, hooks, config | Not imported. |
| Nested W0rkTree trees on export | One tree per export; export each tree into its own Git repository. |
| Remote URLs | Not supported directly; clone/push with `git`. |

## Roadmap

The [Git compatibility section of the core spec](../crates/worktree-protocol/specs/WorkTree.md) also
describes a live bidirectional mirror and a Git remote helper (so that `git clone` can talk to a
W0rkTree server). Neither is part of v2.
