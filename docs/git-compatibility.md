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
| Git LFS pointer file (`filter=lfs`) | The file's real content (see [Git LFS](#git-lfs)) |
| Submodule (gitlink, mode `160000`) | Pin in the tracked `.wtsubmodules` file; optionally a nested tree (see [Submodules](#submodules)) |

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

The report (`ImportReport`) lists branches, snapshot, blob and tag counts, the checked-out branch, what
happened to Git LFS files (`ImportReport::lfs`) and the submodules of the checked-out commit
(`ImportReport::submodules`). `wt git import` prints a warning on stderr for every LFS file imported
as a pointer, every pointer-shaped file without `filter=lfs`, and every submodule whose content was
not imported. History stays in the root tree; split it into nested trees afterwards with
`wt tree add` if needed.

If an import of a **new** worktree fails while converting history (for example `--lfs require` with
missing content), the half-created `.wt/` directory is removed again. An import into an existing
tree that fails leaves the tree's branches untouched.

## Git LFS

W0rkTree needs no LFS: files above 8 MiB are stored as content-defined chunks. Import therefore
replaces each Git LFS pointer with the content it names.

```bash
wt git import ~/src/game game                       # content from the local LFS store
wt git import ~/src/game game --lfs-fetch           # also download what is missing (needs git-lfs)
wt git import ~/src/game game --lfs require         # CI: fail if any content is missing
wt git import ~/src/game game --lfs skip            # keep pointer files as they are
```

**Detection.** A blob is treated as an LFS file only if

1. it parses as a pointer exactly as the `git-lfs` client parses one — smaller than 1024 bytes,
   keys `version` (`https://git-lfs.github.com/spec/v1` or a legacy spec URL), `oid`
   (`sha256:` + 64 lowercase hex digits) and `size` (decimal) in that order, optional
   `ext-N-name` lines, surrounding whitespace and `\r\n` tolerated, nothing else; **and**
2. its path has `filter=lfs` according to the commit's `.gitattributes` files (the file in the path's
   directory and every parent directory; the deepest matching line wins; `-filter`, `!filter` or
   another `filter=` value turn it off).

Pointer-shaped files without the attribute are what a Git checkout shows too, so they are imported as
they are and listed in `ImportReport::lfs.untracked` (with a warning). Macro attributes (`[attr]`),
quoted patterns, `.git/info/attributes` and global attribute files are not consulted.

**Content sources**, in order:

1. the source repository's local LFS store: `<git dir>/lfs/objects/<aa>/<bb>/<oid>` (the common Git
   directory for linked worktrees; `lfs.storage` is honoured);
2. only with `--lfs-fetch` (`ImportOptions::lfs_fetch`) and if `git lfs version` succeeds:
   `git lfs fetch --all`, run once in the source repository, then `git lfs smudge` per object still
   missing.

Content is used only if its size and SHA-256 match the pointer. Pointers with LFS extensions
(`ext-N-name` lines) are never substituted, since their content needs the extension programs.

**Modes** (`--lfs`, `ImportOptions::lfs`):

| Mode | Content available | Content missing |
|---|---|---|
| `auto` (default) | Imported | Pointer file imported; listed in `ImportReport::lfs.missing` with the reason; warning per file plus a summary |
| `require` | Imported | The import fails (nothing is left behind for a new worktree) |
| `skip` | Pointer file imported, no lookup (`ImportReport::lfs.skipped`) | Pointer file imported |

`--lfs-fetch` cannot be combined with `--lfs skip`. The whole content of an LFS object is read into
memory while it is imported.

**Determinism rule.** A snapshot id depends only on the commit SHA, so the same commit always becomes
the same snapshot id, whether its LFS content was available or not. The snapshot's *content* is
fixed by the first import that writes it, because snapshot records are immutable (append-only
history):

- Importing again into the same worktree (incremental import) skips commits that already have a
  snapshot. A commit imported with a pointer file keeps it; only new commits pick up content that has
  become available since. `.wt/git-map.json` is identical either way.
- To get the content for commits first imported as pointers, fetch it and import into a **new**
  worktree. Snapshot ids are the same as before; the manifests now hold the content.
- Two worktrees that imported the same commit with and without its LFS content therefore hold the same
  snapshot id with different content. Do not push both to the same server tree; use
  `--lfs require` where that matters (e.g. CI) to guarantee complete content.

**Export.** `wt git export` writes the imported content as **ordinary Git blobs**; it creates no LFS
pointers and no LFS objects. Consequences:

- Exporting into a new repository rebuilds the LFS commits with the real content, so their SHAs differ
  from the original commits, and the repository's size grows by the size of every version of every
  large file (some hosts reject blobs above 100 MB).
- Exporting into the source repository (or `wt git push` to it) reuses the original commits through
  `.wt/git-map.json`; only snapshots created in W0rkTree become new commits, and their large files are
  plain blobs. While `.gitattributes` still says `filter=lfs`, `git lfs` reports those files as
  modified until they are migrated (`git lfs migrate import`) or the attribute is removed.
- Files that were imported as pointers (content missing, `--lfs skip`, or no `filter=lfs`) are
  exported as the original pointer blobs.

## Submodules

W0rkTree has no gitlink entry. Its counterpart to submodules are
[nested trees](../README.md#core-concepts) — independently versioned sub-projects inside one
worktree — but a root snapshot cannot pin a nested-tree snapshot without a protocol change. Import
therefore records every gitlink in a **tracked file**, `.wtsubmodules`, at the root of the imported
tree:

```toml
# Git submodule pins recorded by `wt git import` (one per gitlink).
# `wt git export` writes each entry back as a gitlink; edit `commit` to move a pin.
version = 1

[[submodule]]
path = "vendor/lib"
commit = "0123456789abcdef0123456789abcdef01234567"
name = "vendor/lib"
url = "https://example.com/lib.git"
```

- One entry per gitlink of the commit, at any depth, sorted by path. `name` and `url` come from the
  commit's `.gitmodules` (which is imported as an ordinary file too) and are informational.
- The file is part of every snapshot whose commit has gitlinks, so each snapshot keeps its own pins
  and nothing is lost. The file is generated deterministically from the commit.
- A Git tree that has gitlinks *and* its own `.wtsubmodules` file cannot be recorded; the import
  fails with an error naming the tree.

`--submodules` (`ImportOptions::submodules`) chooses how much more is imported:

| Mode | Behavior |
|---|---|
| `record` (default) | Pins only; submodule content is not imported (a warning per submodule of the checked-out commit). |
| `nested` | Pins, plus — for a **new** worktree — each submodule of the checked-out commit whose repository is available locally (`<git dir>/modules/<name>`, or an initialised checkout at its path) is imported with all its branches and tags into a nested tree at its path (tree name derived from the path, e.g. `vendor-lib`). The nested tree switches to a branch whose tip is the pinned commit when there is one; otherwise the report says it is not at the pin. Submodules of submodules are recorded as pins in the nested tree. A submodule that cannot be imported is reported and the import continues. |

The pinned snapshot of a nested tree is `UUID v5(pinned commit)`, like every imported commit, and the
mapping is in `.wt/git-map.json`. Importing into an existing tree (`--tree NAME`) always records pins
only.

**Export.** `wt git export` removes `.wtsubmodules` from the Git tree and writes one gitlink per
entry. Together with the imported `.gitmodules` this reproduces the original Git tree, so a commit
with submodules round-trips to the same tree SHA (and the same commit SHA under the conditions in
[Determinism](#determinism)). Nested trees are not part of the root tree's export; export them
separately (`wt --tree vendor-lib git export ../lib.git`). A pin whose path is now occupied by files,
or a `.wtsubmodules` file that does not parse, is not turned into a gitlink: the export reports it
(`ExportReport::warnings`, printed by the CLI) and, for an unparsable file, exports it as an ordinary
file. Pins are exported unchanged by license-aware exports (a gitlink carries no content).

What is **not** preserved: submodule content in `record` mode; history of submodules not available
locally; a nested tree's link to its pin beyond the `.wtsubmodules` entry (moving the nested tree does
not move the pin — edit `commit` to do that).

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
newline, no unsupported entries and no Git LFS files whose content was imported (see below).
Submodule gitlinks round-trip. Otherwise content, structure and history shape are preserved but SHAs
differ.

## Limits

| Git feature | Behavior |
|---|---|
| Submodules (gitlinks) | Pins recorded in `.wtsubmodules` and exported back as gitlinks; content imported only with `--submodules nested` for locally available submodules ([details](#submodules)). |
| Symlinks | Skipped with a warning. |
| Non-UTF-8 file names | Skipped with a warning. |
| Committer distinct from author | Import keeps the author; export writes the author as committer. |
| Commit and tag signatures (GPG/SSH) | Not preserved. |
| Time-zone offsets | Converted to UTC. |
| Empty directories | Not represented (neither Git nor W0rkTree tracks them). |
| `.gitattributes` | Imported as an ordinary file (used to detect Git LFS files). |
| Git LFS | Content imported when available locally (or with `--lfs-fetch`), else the pointer with a warning; export writes plain blobs ([details](#git-lfs)). |
| Notes, reflogs, stashes, remotes, hooks, config | Not imported. |
| Nested W0rkTree trees on export | One tree per export; export each tree into its own Git repository. |
| Remote URLs | Not supported directly; clone/push with `git`. |

## Roadmap

The [Git compatibility section of the core spec](../crates/worktree-protocol/specs/WorkTree.md) also
describes a live bidirectional mirror and a Git remote helper (so that `git clone` can talk to a
W0rkTree server). Neither is part of v2.
