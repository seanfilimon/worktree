# Git compatibility

W0rkTree is not built on Git, but it can move history in both directions so that teams can migrate
gradually and keep Git-based tooling (CI, mirrors, code hosts) working.

| Command | Direction | Implemented by |
|---|---|---|
| `wt git import GIT_PATH [DEST]` | Git → new W0rkTree worktree | `worktree_git::import_repository` |
| `wt git export GIT_PATH [--rev REV]` | W0rkTree → Git repository | `worktree_git::export_repository` |
| `git clone` / `fetch` / `push` of `wt::https://host/tenant/tree` | Both, with stock Git against a server | `git-remote-wt` (`worktree_git::remote_helper`), see [below](#git-remote-helper-git-remote-wt) |

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

`wt git import` and `wt git export` skip what W0rkTree cannot store; a `git push` through the
[remote helper](#git-remote-helper-git-remote-wt) refuses it instead.

## Git remote helper (`git-remote-wt`)

With the `git-remote-wt` binary on `PATH` (the installers and release archives ship it next to
`wt`), stock Git — and anything built on it, such as CI checkouts — can use a server tree as a
remote. Nothing changes on the server.

```bash
wt login https://wt.example.com                     # or export WT_TOKEN=... (CI)
git clone wt::https://wt.example.com/acme/web
cd web
git commit -am "fix the build"
git push origin main
git fetch origin                                    # snapshots pushed with wt arrive as commits
```

The URL is `wt::<server URL>/<tenant>/<tree>`; `wt://host/tenant/tree` is short for the `https`
form. To move an existing Git repository to a server, add the remote and push — the first push
creates the tree:

```bash
git remote add wt wt::https://wt.example.com/acme/web
git push wt main --tags
```

### How it works

Git starts `git-remote-wt <remote> <url>` and speaks the remote-helper protocol
(gitremote-helpers(7)) with it. The helper implements the `fetch` and `push` capabilities, reading
and writing the Git object database directly with the conversions described above:

| Step | What happens |
|---|---|
| Cache | `$GIT_DIR/wt/<remote>/` is a W0rkTree worktree without checked-out files. The SDK fetches the server tree into it incrementally and pushes from it. Its `.wt/git-map.json` records which commit stands for which snapshot in this repository. |
| `git fetch` / `clone` | The helper fetches into the cache, writes the commits of every server branch and tag that Git lacks (same conversion as `wt git export`) and advertises them as `refs/heads/*`, `refs/tags/*` and `HEAD` (the tree's default branch). |
| `git push` | Commits without a snapshot become snapshots (ids as in `wt git import`) on the pushed branch; the SDK then pushes the branch. Tags become W0rkTree tags (annotated tags keep message, tagger and time). |
| Authentication | The SDK's: the login saved by `wt login` in `$WT_HOME`, or `WT_TOKEN`. Public trees can be read without a login. Credentials in the URL are rejected. |

### What is refused

W0rkTree history is append-only and a push never silently loses content, so the helper rejects the
ref (Git prints `! [remote rejected]` with the reason) when:

| Request | Result |
|---|---|
| `git push --force` that would discard server snapshots | Refused: fetch, merge the remote branch, push again. A forced push that only adds snapshots is an ordinary push. |
| Deleting a branch or tag (`git push origin :name`) | Refused: use `wt branch archive` / `wt tag delete NAME --remote`. |
| Moving an existing tag | Refused. |
| A tag whose commit is on no server branch | Refused: push the branch first (or in the same `git push`). |
| A commit with symbolic links, submodules, non-UTF-8 file names or a `.wt` path | Refused before anything reaches the server; the message names the commit and the paths. |
| Refs other than `refs/heads/*` and `refs/tags/*` | Refused. |

Server-side rules (branch protection, secret scanning, permissions) apply as for `wt push`, and their
message is shown by Git. `--depth`, `--dry-run` and `--atomic` are not supported.

### Commit ids across clones

- A snapshot made with `wt` has the same commit SHA in every clone and in every `wt git export`.
- A commit pushed through the helper keeps its SHA in the clone that pushed it, and in every clone
  that already has that commit (for example a clone of the same repository on another host): the
  helper recognises the commit by the snapshot id and verifies that it converts to exactly that
  snapshot.
- Other clones get a commit rebuilt from the snapshot. It has the same SHA only when the original
  had identical author and committer, a UTC (`+0000`) time and no signature (see
  [determinism](#determinism)); otherwise files, history and messages are identical but the SHA —
  and so the SHAs of its descendants in that clone — differs from the pusher's.

W0rkTree identifies history by snapshot id; use that, not a commit SHA, to name a revision across
machines. Deleting `$GIT_DIR/wt/<remote>/` is safe: the next fetch rebuilds it.

## Roadmap

The [Git compatibility section of the core spec](../crates/worktree-protocol/specs/WorkTree.md) also
describes a live bidirectional mirror and serving trees over Git's own network protocol (so that Git
needs no helper). Neither is built; preserving Git committer identity, time zones and signatures in
snapshots would make helper pushes reproduce the same SHA everywhere and needs a protocol addition.
