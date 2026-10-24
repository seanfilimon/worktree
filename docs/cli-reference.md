# `wt` CLI reference

`wt` is the W0rkTree command-line interface (crate [`worktree-cli`](../crates/worktree-cli/README.md)).
Every command is a thin layer over [`worktree-sdk`](./sdk-guide.md); anything `wt` can do, an
embedding program can do too.

- [Conventions](#conventions) — global flags, exit codes, JSON output, revisions, environment
- [Local history](#local-history) — `init`, `status`, `snapshot`, `log`, `show`, `diff`, `reflog`
- [Branches and merging](#branches-and-merging) — `branch`, `switch`, `merge`, `revert`, `restore`, `tag`
- [Worktree structure](#worktree-structure) — `tree`, `ignore`, `config`
- [Servers and sync](#servers-and-sync) — `remote`, `register`, `login`, `logout`, `whoami`, `push`, `fetch`, `pull`, `clone`, `stage`
- [Export and interop](#export-and-interop) — `archive`, `git`
- [Background daemon](#background-daemon) — `daemon`

---

## Conventions

### Global flags

Global flags may appear anywhere on the command line.

| Flag | Meaning |
|---|---|
| `--json` | Print one machine-readable JSON document on stdout instead of human output. |
| `-C <path>` | Run as if `wt` had been started in `<path>`. |
| `--tree <name>` | Operate on the named tree instead of the tree containing the current directory. |
| `--no-color` | Disable colored output. |
| `-h`, `--help` | Show help for the command. |
| `-V`, `--version` | Print the version. |

Without `--tree`, commands use the deepest tree whose directory contains the current directory
(falling back to the root tree). See [nested trees](./getting-started.md#nested-trees).

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | The command failed (for example: not a worktree, branch not found, push rejected, secrets detected, server unreachable). |
| `2` | Usage error: unknown command or flag, missing or conflicting arguments. |
| `3` | A merge stopped with conflicts (`wt merge`, `wt pull`), or `wt merge --continue` found unresolved conflict markers. |

### JSON output

With `--json`, stdout carries exactly one JSON document and nothing else, so it can be piped into
`jq` or parsed by scripts. Field names follow the SDK data model (for example `StatusReport`,
`Snapshot`, `BranchInfo`, `PushReport` — see [the SDK guide](./sdk-guide.md#data-model)). Error
messages are written to stderr and the exit code tells success from failure. Secrets such as tokens
are never printed.

```bash
wt status --json | jq -r '.changes[] | "\(.kind) \(.path)"'
wt log -n 1 --json | jq -r '.[0].id'
```

### Revisions

Commands that take `REV` accept:

| Syntax | Resolves to |
|---|---|
| `HEAD` | Tip of the current branch |
| `HEAD~N` | N-th first-parent ancestor of `HEAD` |
| `<branch>` | Tip of a local branch |
| `<branch>~N` | N-th first-parent ancestor of that tip |
| `origin/<branch>` | Last known tip of the branch on remote `origin` (any remote name works) |
| `<tag>` | The snapshot a tag points at |
| `<id-prefix>` | A snapshot id or a unique prefix of at least 4 characters |

### Environment

| Variable | Effect |
|---|---|
| `WT_HOME` | Per-user state directory (default `~/.wt`): credentials, daemon files. See [configuration](./configuration.md#per-user-state-wt_home). |
| `WT_PASSWORD` | Password used by `wt login` / `wt register` instead of prompting. |
| `NO_COLOR` | Disable colors (same as `--no-color`). Colors are also off when stdout is not a terminal. |
| `WT_LOG` | Enable diagnostic logging on stderr, e.g. `WT_LOG=debug` or `WT_LOG=worktree_sdk=trace`. |

Passwords are read from the first line of stdin with `--password-stdin`, else from `WT_PASSWORD`,
else from an interactive prompt that does not echo.

---

## Local history

### `wt init`

```
wt init [path] [--name N] [--branch B]
```

Create a new worktree: a `.wt/` directory at `path` (default: the current directory, created if
missing) with a root tree, an empty default branch and an `init` reflog entry.

| Option | Default | Meaning |
|---|---|---|
| `path` | `.` | Directory to initialize. |
| `--name N` | directory name | Name of the worktree and of its root tree. |
| `--branch B` | `main` | Default branch. |

Fails if `.wt/` already exists.

```bash
wt init                       # current directory
wt init my-project --branch trunk
```

### `wt status`

```
wt status [--team]
```

Compare the working directory with the head snapshot of the current tree and list added,
modified and deleted files (ignored paths and nested trees excluded). Shows the current branch,
head snapshot and, if one is in progress, the merge state and its conflicted paths.

| Option | Meaning |
|---|---|
| `--team` | Instead, list the team's [staged snapshots](./server-architecture.md#staged-snapshots) for this tree from the server (requires a remote and login). |

```bash
wt status
wt status --team
wt status --json        # StatusReport: {tree, branch, head, changes:[{path, kind}], merge}
```

### `wt snapshot`

```
wt snapshot -m MSG [--allow-empty] [--no-scan]
```

Record the working state of the current tree as a new snapshot on its current branch. There is no
staging step: every non-ignored file in the tree is captured.

| Option | Meaning |
|---|---|
| `-m MSG` | Snapshot message (required). |
| `--allow-empty` | Create the snapshot even if nothing changed since `HEAD`. |
| `--no-scan` | Skip the pre-snapshot [secret scan](./security.md#secret-scanning). |

Fails (exit 1) when nothing changed (without `--allow-empty`) or when the secret scanner reports a
finding. During a conflicted merge, `wt snapshot` succeeds only once all conflict markers are gone;
it then completes the merge with a two-parent snapshot (equivalent to `wt merge --continue`).

```bash
wt snapshot -m "Add parser"
wt snapshot -m "Checkpoint" --allow-empty
```

### `wt log`

```
wt log [-n N] [REV] [--oneline]
```

Show snapshots reachable from `REV` (default `HEAD`), newest first.

| Option | Meaning |
|---|---|
| `-n N` | Show at most N snapshots. |
| `REV` | Start from this revision. |
| `--oneline` | One line per snapshot: short id (8 characters) and message. |

```bash
wt log -n 10
wt log feature --oneline
wt log origin/main
```

### `wt show`

```
wt show REV [--stat]
```

Show a snapshot's metadata (id, parents, author, date, message) and its changes relative to its
first parent as a unified diff.

| Option | Meaning |
|---|---|
| `--stat` | Show per-file added/removed line counts instead of patches. |

```bash
wt show HEAD
wt show 3f2a9c1e --stat
```

### `wt diff`

```
wt diff [REV [REV2]] [--name-only] [--stat] [PATH...]
```

| Form | Compares |
|---|---|
| `wt diff` | Working directory vs `HEAD` |
| `wt diff REV` | Working directory vs `REV` |
| `wt diff REV REV2` | `REV` vs `REV2` |

| Option | Meaning |
|---|---|
| `--name-only` | Print only changed paths. |
| `--stat` | Print per-file added/removed line counts. Conflicts with `--name-only`. |
| `PATH...` | Restrict to these paths (prefix match). Put `--` before paths that could be mistaken for revisions. |

Patches are unified diffs with 3 lines of context and `a/` / `b/` path prefixes. Binary files
(NUL byte in the first 8 KiB or invalid UTF-8) are reported without a patch.

```bash
wt diff
wt diff HEAD~3 HEAD --stat
wt diff main feature -- src/
```

### `wt reflog`

```
wt reflog [-n N]
```

Show the current tree's reference log, newest first: every operation that moved a branch or
changed the working state (`init`, `snapshot`, `auto-snapshot`, `switch`, `branch.create`,
`branch.archive`, `merge`, `fast-forward`, `revert`, `restore`, `pull`, `push`, `tag.create`,
`tag.delete`) with old and new snapshot ids. The reflog is append-only.

```bash
wt reflog -n 20
```

---

## Branches and merging

### `wt branch`

```
wt branch [list] [-a]
wt branch create NAME [--from REV]
wt branch archive NAME
wt branch restore NAME
```

| Subcommand | Meaning |
|---|---|
| `list` (default) | List branches of the current tree, marking the current one. `-a` includes archived branches. |
| `create NAME` | Create a branch at `HEAD`, or at `--from REV`. Does not switch to it. |
| `archive NAME` | Soft-delete a branch: hidden from listings, history kept. The current and the default branch cannot be archived. |
| `restore NAME` | Un-archive a branch. An archived name cannot be reused until it is restored. |

There is no hard delete.

```bash
wt branch
wt branch create feature/login --from HEAD~2
wt branch archive spike
wt branch -a
```

### `wt switch`

```
wt switch NAME [-c] [--force]
```

Switch the current tree to branch `NAME`, rewriting the working directory to that branch's tip
(tracked files absent there are removed; ignored files and nested trees are never touched).

| Option | Meaning |
|---|---|
| `-c` | Create `NAME` at the current `HEAD` first. |
| `--force` | Discard uncommitted changes instead of refusing. |

Fails if there are uncommitted changes and `--force` is not given.

```bash
wt switch -c feature/login
wt switch main
```

### `wt merge`

```
wt merge SOURCE [-m MSG] [--no-ff]
wt merge --continue [-m MSG]
wt merge --abort
```

Merge `SOURCE` (a branch or any revision) into the current branch. Requires a clean working tree.

| Outcome | Result |
|---|---|
| Up to date | Nothing to do. |
| Fast-forward | The branch moves to `SOURCE`'s tip (unless `--no-ff`). |
| Merged | A two-parent merge snapshot is created. |
| Conflicts | Conflict markers are written into files, the merge state is saved, exit code `3`. |

Text conflicts use diff3-style markers:

```
<<<<<<< main
our line
||||||| base
original line
=======
their line
>>>>>>> feature
```

Other conflict kinds: `modify_delete` (one side modified, the other deleted), `add_add` (both added
different content), `binary` (binary file changed on both sides — "ours" is kept in the working
tree). A machine-readable list is written to `.wt/conflicts/<tree>.json`.

| Option | Meaning |
|---|---|
| `-m MSG` | Message for the merge snapshot. |
| `--no-ff` | Always create a merge snapshot, even when a fast-forward is possible. |
| `--continue` | Finish a conflicted merge after removing all markers. Fails with exit `3` if markers remain. |
| `--abort` | Restore the pre-merge state and clear the merge. |

While a merge is in progress, the daemon does not auto-snapshot the tree.

```bash
wt merge feature/login
wt merge feature/login --no-ff -m "Merge login"
# ...resolve conflicts in the files...
wt merge --continue
```

### `wt revert`

```
wt revert REV
```

Create a new snapshot on the current branch that undoes the changes `REV` introduced relative to its
first parent. History is not rewritten. Requires a clean working tree; fails if later snapshots
changed the same lines or if there is nothing left to undo.

```bash
wt revert HEAD~1
```

### `wt restore`

```
wt restore [PATH...] [--from REV]
```

Overwrite working files at `PATH...` (all files if none given) with their content at `REV` (default
`HEAD`). Files absent at `REV` are deleted. Prints the restored paths. Does not create a snapshot.

```bash
wt restore src/main.rs
wt restore --from HEAD~2 docs/
wt restore                    # discard all uncommitted changes in the tree
```

### `wt tag`

```
wt tag [list]
wt tag create NAME [REV] [-m MSG]
wt tag delete NAME
```

Tags are named, immutable pointers to snapshots, scoped to a tree. `create` tags `REV` (default
`HEAD`); `-m` adds a message (annotated tag). `wt push` uploads local tags the server does not have.

```bash
wt tag create v1.0.0 -m "First release"
wt tag
wt tag delete v1.0.0-rc1
```

---

## Worktree structure

### `wt tree`

```
wt tree list
wt tree add NAME PATH
wt tree remove NAME
```

| Subcommand | Meaning |
|---|---|
| `list` | List trees with their path, current branch and default branch. |
| `add NAME PATH` | Register `PATH` (relative to the worktree root; created if missing) as a nested tree. Writes `PATH/.wt-tree/config.toml`. |
| `remove NAME` | Unregister a nested tree. Files are kept. The root tree cannot be removed. |

A tree's snapshots exclude the paths of trees nested inside it.

```bash
wt tree add web apps/web
wt --tree web snapshot -m "Web skeleton"
wt tree list
```

### `wt ignore`

```
wt ignore list
wt ignore add PATTERN
wt ignore check PATH
```

| Subcommand | Meaning |
|---|---|
| `list` | Effective patterns: built-ins, `.wt/ignore`, and `.wtignore` files. |
| `add PATTERN` | Append a gitignore-syntax pattern to `.wt/ignore`. |
| `check PATH` | Report whether `PATH` is ignored. |

Built-in ignores: `.wt/`, `.wt-tree/`, `.git/`, `target/`, `node_modules/`, `.DS_Store`, `*.swp`.

```bash
wt ignore add "*.log"
wt ignore check build/out.o
```

### `wt config`

```
wt config list
wt config get KEY
wt config set KEY VALUE
```

Read and write `.wt/config.toml` using dotted keys such as `user.name`,
`auto_snapshot.inactivity_secs` or `remote.origin.url`. Unknown keys and invalid values are rejected.
All keys are listed in [configuration](./configuration.md#wtconfigtoml).

```bash
wt config set user.name "Ada Lovelace"
wt config set user.email ada@example.com
wt config set auto_snapshot.enabled false
wt config get remote.origin.url
```

---

## Servers and sync

Server URLs are base URLs such as `http://localhost:7420`; gRPC and REST share the same address.
Credentials are stored per server in `$WT_HOME/credentials.toml`. Where `SERVER` is optional it
defaults to the URL of the worktree's default remote.

### `wt remote`

```
wt remote list
wt remote add NAME URL --tenant T
wt remote remove NAME
```

A remote names a server URL and the tenant whose trees this worktree pushes to. Each tree is stored
on the server under that tenant with the tree's remote name (by default the tree name).

```bash
wt remote add origin http://localhost:7420 --tenant acme
wt remote list
```

### `wt register`

```
wt register [SERVER] -u USER -e EMAIL [--password-stdin]
```

Create an account on the server and save the returned token. The server also creates a personal
tenant whose slug is the username. Self-registration can be disabled on the server
(`--allow-registration=false`).

```bash
wt register http://localhost:7420 -u ada -e ada@example.com
printf '%s\n' "$PASS" | wt register http://localhost:7420 -u ci -e ci@example.com --password-stdin
```

### `wt login`, `wt logout`, `wt whoami`

```
wt login [SERVER] [-u USER] [--password-stdin]
wt logout [SERVER]
wt whoami [SERVER]
```

| Command | Meaning |
|---|---|
| `login` | Exchange username and password for a session token and save it. Prompts for the username if `-u` is omitted. |
| `logout` | Forget the saved credential for the server. |
| `whoami` | Ping the server with the saved credential and print the server version, username and personal tenant. |

```bash
wt login http://localhost:7420 -u ada
WT_PASSWORD=secret wt login -u ada          # uses the default remote's URL
wt whoami
```

### `wt push`

```
wt push [--remote R] [--branch B]
```

Upload a branch of the current tree (default: the current branch) to remote `R` (default `origin`).
Creates the tree on the server if it does not exist yet, uploads only the snapshots and objects the
server is missing, then advances the server branch. Also uploads local tags the server does not have,
and updates `origin/<branch>`.

Pushes are fast-forward only. A push is rejected (exit 1) when:

| Rejection | Meaning / fix |
|---|---|
| Conflict | The server branch has snapshots you do not have. Run `wt pull`, then push again. |
| Branch protection | A protection rule forbids direct pushes. Open a merge request instead. |
| Access denied | You lack write access (`developer` role or higher) to the tree. |
| License violation | A changed path is covered by a license rule that does not let your tenant modify it. |
| Review required | The branch requires approvals. |

Pushing a branch clears your staged snapshot for it.

```bash
wt push
wt push --branch feature/login
wt --tree web push --remote upstream
```

### `wt fetch`

```
wt fetch [--remote R]
```

Download new snapshots, objects and tags for every branch of the current tree and update the
remote-tracking refs (`origin/<branch>`). Never touches local branches or files.

```bash
wt fetch
wt log origin/main --oneline
```

### `wt pull`

```
wt pull [--remote R] [--branch B]
```

Fetch, then integrate `<remote>/<branch>` into the local branch (default: current): a fast-forward
plus checkout when possible, otherwise a merge as with `wt merge` (exit `3` on conflicts).

```bash
wt pull
```

### `wt clone`

```
wt clone URL TENANT/TREE [DEST]
```

Create a new worktree at `DEST` (default: the tree name; must not exist or must be empty) from tree
`TREE` of tenant `TENANT`: initializes `.wt/`, configures `origin`, links the local tree id to the
server's, fetches everything, creates a local branch for every remote branch and checks out the
default branch. Public trees can be cloned without logging in.

```bash
wt clone http://localhost:7420 acme/platform
wt clone http://localhost:7420 acme/platform ~/src/platform
```

### `wt stage`

```
wt stage [--remote R]
```

Upload the current working state of the tree as a **staged snapshot**: a team-visible
work-in-progress snapshot that is not part of branch history. Each (tree, account, branch) has at
most one staged snapshot; staging again replaces it, and pushing the branch clears it. The daemon
does this automatically after auto-snapshots when `sync.auto_stage` is enabled.

```bash
wt stage
wt status --team
```

---

## Export and interop

### `wt archive`

```
wt archive [REV] -o FILE [--format tar.gz|zip]
```

Write the files of snapshot `REV` (default `HEAD`) of the current tree to an archive.

| Option | Meaning |
|---|---|
| `-o FILE` | Output file (required). |
| `--format` | `tar.gz` (default) or `zip`. |

```bash
wt archive v1.0.0 -o release.tar.gz
wt archive -o snapshot.zip --format zip
```

### `wt git`

```
wt git import GIT_PATH [DEST]
wt git export GIT_PATH [--rev REV]
```

| Subcommand | Meaning |
|---|---|
| `import` | Create a new worktree at `DEST` (default: the Git repository's directory name) from the Git repository at `GIT_PATH`: every branch's commits become snapshots, tags become tags, `.gitignore` becomes `.wt/ignore`. |
| `export` | Write snapshots to the Git repository at `GIT_PATH` (created if missing) as commits. `--rev` limits the export to the history of one revision. Incremental and deterministic via `.wt/git-map.json`. |

See [Git compatibility](./git-compatibility.md) for the exact semantics and limits.

```bash
wt git import ~/src/legacy-repo legacy
wt -C legacy git export ../legacy-mirror.git
```

---

## Background daemon

### `wt daemon`

```
wt daemon start [--foreground]
wt daemon stop
wt daemon status
wt daemon watch [PATH]
wt daemon unwatch [PATH]
wt daemon pause
wt daemon resume
```

Control the per-user [`worktree-bg`](./daemon.md) daemon. `PATH` defaults to the current worktree.

| Subcommand | Meaning |
|---|---|
| `start` | Start the daemon in the background (`--foreground` runs it attached to the terminal). |
| `stop` | Ask the daemon to shut down. |
| `status` | PID, version, start time and per-worktree state: paused, pending changes, snapshots created, last snapshot/stage, last error. |
| `watch` | Start watching a worktree for auto-snapshots. Persisted in `$WT_HOME/bg.toml`. |
| `unwatch` | Stop watching a worktree. |
| `pause` / `resume` | Suspend or resume auto-snapshots. |

Commands other than `start` fail with exit 1 if the daemon is not running.

```bash
wt daemon start
wt daemon watch
wt daemon status --json
```
