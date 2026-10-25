# `wt` CLI reference

`wt` is the W0rkTree command-line interface (crate [`worktree-cli`](../crates/worktree-cli/README.md)).
Every command is a thin layer over [`worktree-sdk`](./sdk-guide.md); anything `wt` can do, an
embedding program can do too.

- [Conventions](#conventions) — global flags, exit codes, JSON output, revisions, environment
- [Local history](#local-history) — `init`, `status`, `snapshot`, `log`, `show`, `diff`, `reflog`
- [Branches and merging](#branches-and-merging) — `branch`, `switch`, `merge`, `revert`, `restore`, `tag`
- [Worktree structure and configuration](#worktree-structure-and-configuration) — `tree`, `ignore`, `config`
- [Servers and sync](#servers-and-sync) — `remote`, `register`, `login`, `logout`, `whoami`, `password-reset`, `push`, `fetch`, `pull`, `clone`, `stage`, `unstage`
- [Export and interop](#export-and-interop) — `archive`, `git`
- [Background daemon](#background-daemon) — `daemon`
- [Collaboration](#collaboration) — `tenant`, `trees`, `mr`, `ci`, `protect`, `license`, `todo`, `deps`, `release`, `search`, `events`
- [Administration](#administration) — `admin`, `audit`
- [Identity and access](#identity-and-access) — `team`, `role`, `policy`, `grant`, `access`, `sa`, `token`, `session`, `mfa`, `invite`, `iam`, `sso`, `identity`

Server commands accept `--server URL` (or `WT_SERVER`; default: this worktree's origin, else the
only saved login) and `--json`; tree arguments are written `tenant/tree`. See
[Identity and access management](./iam.md) for IAM concepts.

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
| `WT_TOKEN` | Bearer token (API token or service-account key) used for every server instead of saved logins, e.g. in CI. `WT_TOKEN_USER` optionally names the account for display. |
| `WT_SERVER` | Default server for server commands (same as `--server`). |
| `NO_COLOR` | Disable colors (same as `--no-color`). Colors are also off when stdout is not a terminal. |
| `WT_LOG` | Enable diagnostic logging on stderr, e.g. `WT_LOG=debug` or `WT_LOG=worktree_sdk=trace`. |

Passwords are read from the first line of stdin with `--password-stdin`, else from `WT_PASSWORD`,
else from an interactive prompt that does not echo.

---

<!-- Everything below is generated from `wt ... --help`; do not edit by hand. -->

## Local history

### `wt init`

Create a new worktree

```text
Usage: wt init [OPTIONS] [PATH]

Arguments:
  [PATH]  Directory to initialize (default: the current directory)

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --name <N>      Name of the worktree and its root tree (default: directory name)
      --branch <B>    Default branch (default: main)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --watch         Start the background daemon (if needed) and auto-snapshot the new worktree
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt status`

Show the working tree status

```text
Usage: wt status [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --team          Also show the team's staged work in progress from the remote
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt snapshot`

Record the working state as a new snapshot

```text
Usage: wt snapshot [OPTIONS] --message <MSG>

Options:
      --json           Print machine-readable JSON (one document) on stdout
  -m, --message <MSG>  Snapshot message
      --allow-empty    Create a snapshot even if nothing changed
  -C <PATH>            Run as if wt was started in PATH
      --no-scan        Skip the pre-snapshot secret scan
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --sign           Sign this snapshot with the configured key ([signing] key, else the `default` key; see `wt identity generate`)
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --no-verify      Skip the pre-snapshot and post-snapshot hooks
  -V, --version        Print version

Dependencies:
      --depend <TREE[@BRANCH]>  Declare that this snapshot needs work in TREE (TENANT/TREE also accepted); creates a TODO there on push. Repeatable; the flags below apply to the preceding --depend
      --depend-tenant <TENANT>  Tenant of the dependency's tree (default: the tenant of this tree)
      --todo <TITLE>            Title of the TODO (default: the first line of the message)
      --details <TEXT>          Longer description of the TODO
      --priority <PRIORITY>     Priority of the TODO (default: medium) [possible values: low, medium, high, critical]
      --blocking                The dependent branch may not merge until the TODO is done
      --linked                  The branches must merge together (linked merge)
```

### `wt log`

Show snapshot history

```text
Usage: wt log [OPTIONS] [REV]

Arguments:
  [REV]  Start from this revision (default: HEAD)

Options:
      --json            Print machine-readable JSON (one document) on stdout
  -n, --max-count <N>   Show at most N snapshots
  -C <PATH>             Run as if wt was started in PATH
      --oneline         One snapshot per line
      --show-signature  Verify and show each snapshot's signature
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

### `wt show`

Show a snapshot and its changes

```text
Usage: wt show [OPTIONS] <REV>

Arguments:
  <REV>  Snapshot to show

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --stat          Show a diffstat instead of the patch
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt diff`

Show changes between the working tree and snapshots

```text
Usage: wt diff [OPTIONS] [REV|PATH]... [-- <PATH>...]

Arguments:
  [REV|PATH]...  Up to two revisions followed by paths. With no revision the working tree is compared with HEAD; with one, with that revision; with two, the revisions are compared with each other
  [PATH]...      Paths to restrict the diff to (everything after `--`)

Options:
      --json                  Print machine-readable JSON (one document) on stdout
      --name-only             Show only the names of changed files
  -C <PATH>                   Run as if wt was started in PATH
      --name-status           Show each changed file's status letter and name (A, M, D, or R<similarity> OLD NEW for renames)
      --stat                  Show a diffstat instead of patches
      --tree <NAME>           Operate on this tree instead of the one containing the current directory
  -M, --find-renames[=<PCT>]  Detect renames: deleted/added files with identical content, or at least PCT percent similar lines (default 50), are shown as `renamed: OLD -> NEW`
      --no-color              Disable colored output
      --server <URL>          Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --word-diff             Show changed words inline (`[-removed-]{+added+}`) instead of whole changed lines
  -V, --version               Print version
```

### `wt reflog`

Show the reference log

```text
Usage: wt reflog [OPTIONS]
       wt reflog <COMMAND>

Commands:
  expire  Drop old reflog entries (snapshots are never deleted)
  help    Print this message or the help of the given subcommand(s)

Options:
      --json             Print machine-readable JSON (one document) on stdout
  -C <PATH>              Run as if wt was started in PATH
  -n, --max-count <N>    Show at most N entries
  -b, --branch <BRANCH>  Only entries of this branch (numbered as `BRANCH@{N}`)
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --no-color         Disable colored output
      --server <URL>     Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version          Print version
```

#### `wt reflog expire`

Drop old reflog entries (snapshots are never deleted)

```text
Usage: wt reflog expire [OPTIONS]

Options:
      --json              Print machine-readable JSON (one document) on stdout
      --older-than <AGE>  Drop entries older than this (`90d`, `12h`, `30m`, `2w`, `now`) [default: 90d]
  -C <PATH>               Run as if wt was started in PATH
      --keep <N>          Keep at most N newest entries per tree [default: 1000]
      --all-trees         Expire the reflogs of every tree, not just the current one
      --tree <NAME>       Operate on this tree instead of the one containing the current directory
      --no-color          Disable colored output
      --server <URL>      Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version           Print version
```

## Branches and merging

### `wt branch`

List, create, archive or restore branches

```text
Usage: wt branch [OPTIONS]
       wt branch <COMMAND>

Commands:
  list     List branches (the default)
  create   Create a branch
  archive  Archive (soft-delete) a branch; its history stays reachable
  restore  Restore an archived branch
  help     Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
  -a, --all           Include archived branches
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt branch list`

List branches (the default)

```text
Usage: wt branch list [OPTIONS]

Options:
  -a, --all           Include archived branches
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt branch create`

Create a branch

```text
Usage: wt branch create [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --from <REV>    Revision to start from (default: HEAD)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt branch archive`

Archive (soft-delete) a branch; its history stays reachable

```text
Usage: wt branch archive [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt branch restore`

Restore an archived branch

```text
Usage: wt branch restore [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt switch`

Switch branches

```text
Usage: wt switch [OPTIONS] <NAME>

Arguments:
  <NAME>  Branch to switch to

Options:
  -c, --create        Create the branch at the current head first
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --force         Discard uncommitted changes
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt merge`

Merge a branch into the current branch

```text
Usage: wt merge [OPTIONS] <SOURCE|--continue|--abort>
       wt merge <COMMAND>

Commands:
  resolve  Resolve conflicted paths of the merge in progress
  abort    Abandon the merge in progress and restore the pre-merge state
  status   Show the merge in progress and the state of each conflict
  help     Print this message or the help of the given subcommand(s)

Arguments:
  [SOURCE]  Branch or revision to merge into the current branch

Options:
      --json                 Print machine-readable JSON (one document) on stdout
  -C <PATH>                  Run as if wt was started in PATH
      --tree <NAME>          Operate on this tree instead of the one containing the current directory
  -m, --message <MSG>        Message for the merge snapshot
      --no-color             Disable colored output
      --no-ff                Always create a merge snapshot, even when a fast-forward is possible
      --server <URL>         Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -s, --strategy <STRATEGY>  How conflicting changes are resolved: `manual` writes conflict markers (labelled `ours (BRANCH ID)` / `theirs (SOURCE ID)`) and stops; `ours` / `theirs` resolve every conflicting hunk and file in favour of that side while still combining non-conflicting changes [default: manual] [possible values: manual, ours, theirs]
      --continue             Finish a conflicted merge after resolving the conflicts
      --abort                Abandon a conflicted merge and restore the pre-merge state
  -V, --version              Print version
```

#### `wt merge resolve`

Resolve conflicted paths of the merge in progress

```text
`--ours` / `--theirs` resolve the conflicting hunks of a text file in favour of that side (non-conflicting changes from the other side are kept) and take that side's version of binary or modify/delete conflicts; `--file F` uses the contents of F.

Usage: wt merge resolve [OPTIONS] <--ours|--theirs|--file <FILE>> [PATH]...

Arguments:
  [PATH]...
          Conflicted paths to resolve

Options:
      --all
          Resolve every unresolved conflict (with --ours or --theirs)

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

      --ours
          Resolve in favour of the current branch

      --theirs
          Resolve in favour of the merged source

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --file <FILE>
          Use the contents of this file as the resolution

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

#### `wt merge abort`

Abandon the merge in progress and restore the pre-merge state

```text
Usage: wt merge abort [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt merge status`

Show the merge in progress and the state of each conflict

```text
Usage: wt merge status [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt revert`

Create a snapshot that undoes another snapshot

```text
Usage: wt revert [OPTIONS] <REV>

Arguments:
  <REV>  Snapshot whose changes to undo

Options:
      --json           Print machine-readable JSON (one document) on stdout
  -m, --message <MSG>  Message for the revert snapshot (default: `Revert "<subject>"`)
  -C <PATH>            Run as if wt was started in PATH
      --parent <N>     For a merge snapshot: the parent (1-based) whose line of history is kept; the changes the merge brought relative to it are undone
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

### `wt restore`

Restore working files from a snapshot

```text
Usage: wt restore [OPTIONS] [PATH]...

Arguments:
  [PATH]...  Paths to restore (default: everything)

Options:
      --from <REV>    Revision to restore from (default: HEAD)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt tag`

List, create or delete tags

```text
Usage: wt tag [OPTIONS] [COMMAND]

Commands:
  list    List tags (the default)
  create  Tag a snapshot
  delete  Delete a tag
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tag list`

List tags (the default)

```text
Usage: wt tag list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tag create`

Tag a snapshot

```text
Usage: wt tag create [OPTIONS] <NAME> [REV]

Arguments:
  <NAME>  
  [REV]   Revision to tag (default: HEAD)

Options:
      --json           Print machine-readable JSON (one document) on stdout
  -m, --message <MSG>  Tag message (makes an annotated tag)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt tag delete`

Delete a tag

```text
Usage: wt tag delete [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Worktree structure and configuration

### `wt tree`

Manage nested trees

```text
Usage: wt tree [OPTIONS] <COMMAND>

Commands:
  list         List the trees of this worktree
  add          Register a directory as a nested tree
  remove       Unregister a nested tree (its files are kept)
  validate     Check tree layout: .wt-tree/ marker, config and ignore files parse, no nested .wt/ (all trees when NAME is omitted; exit 1 on errors)
  init-config  Scaffold .wt-tree/{config.toml,ignore,hooks/,access/} for a nested tree (default: the selected tree); existing files are kept
  help         Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tree list`

List the trees of this worktree

```text
Usage: wt tree list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tree add`

Register a directory as a nested tree

```text
Usage: wt tree add [OPTIONS] <NAME> <PATH>

Arguments:
  <NAME>  
  <PATH>  Directory of the tree (created if missing)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tree remove`

Unregister a nested tree (its files are kept)

```text
Usage: wt tree remove [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tree validate`

Check tree layout: .wt-tree/ marker, config and ignore files parse, no nested .wt/ (all trees when NAME is omitted; exit 1 on errors)

```text
Usage: wt tree validate [OPTIONS] [NAME]

Arguments:
  [NAME]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tree init-config`

Scaffold .wt-tree/{config.toml,ignore,hooks/,access/} for a nested tree (default: the selected tree); existing files are kept

```text
Usage: wt tree init-config [OPTIONS] [NAME]

Arguments:
  [NAME]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt ignore`

Manage ignore patterns

```text
Usage: wt ignore [OPTIONS] <COMMAND>

Commands:
  list   Show the effective ignore patterns, in evaluation order
  add    Add a pattern (gitignore syntax) to .wt/ignore
  check  Check whether a path is ignored
  help   Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt ignore list`

Show the effective ignore patterns, in evaluation order

```text
Usage: wt ignore list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --sources       Show the file each pattern comes from (hard, soft, .wt/ignore, ...)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt ignore add`

Add a pattern (gitignore syntax) to .wt/ignore

```text
Usage: wt ignore add [OPTIONS] <PATTERN>

Arguments:
  <PATTERN>  

Options:
      --in-tree       Add it to the selected tree's .wt-tree/ignore instead (additive: cannot re-include root-ignored paths)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt ignore check`

Check whether a path is ignored

```text
Usage: wt ignore check [OPTIONS] <PATH>

Arguments:
  <PATH>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt config`

Read and write worktree configuration

```text
Usage: wt config [OPTIONS] <COMMAND>

Commands:
  list   Show every effective configuration value
  get    Print one effective value (dotted key, e.g. user.name or remote.origin.url)
  set    Set one value in .wt/config.toml, the user config (--global) or a tree's .wt-tree/config.toml (global --tree NAME)
  unset  Remove one value from .wt/config.toml, the user config (--global) or a tree's .wt-tree/config.toml (global --tree NAME)
  help   Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt config list`

Show every effective configuration value

```text
Usage: wt config list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --show-origin   Show which file set each value (default, user, worktree or tree)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt config get`

Print one effective value (dotted key, e.g. user.name or remote.origin.url)

```text
Usage: wt config get [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --show-origin   Also show which file set the value
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt config set`

Set one value in .wt/config.toml, the user config (--global) or a tree's .wt-tree/config.toml (global --tree NAME)

```text
Usage: wt config set [OPTIONS] <KEY> <VALUE>

Arguments:
  <KEY>    
  <VALUE>  

Options:
      --global        Write the user config (<wt home>/config.toml): [user], [signing], [secret_scanning] only
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt config unset`

Remove one value from .wt/config.toml, the user config (--global) or a tree's .wt-tree/config.toml (global --tree NAME)

```text
Usage: wt config unset [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --global        Remove it from the user config
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Servers and sync

### `wt remote`

Manage remote servers

```text
Usage: wt remote [OPTIONS] <COMMAND>

Commands:
  list    List configured remotes
  add     Add a remote
  remove  Remove a remote
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt remote list`

List configured remotes

```text
Usage: wt remote list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt remote add`

Add a remote

```text
Usage: wt remote add [OPTIONS] --tenant <T> <NAME> <URL>

Arguments:
  <NAME>  
  <URL>   Server base URL, e.g. http://localhost:7420

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --tenant <T>    Tenant slug on the server
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt remote remove`

Remove a remote

```text
Usage: wt remote remove [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt register`

Create an account on a server

```text
Usage: wt register [OPTIONS] --username <USER> --email <EMAIL> [SERVER]

Arguments:
  [SERVER]  Server URL (default: the URL of the worktree's default remote)

Options:
      --json             Print machine-readable JSON (one document) on stdout
  -u, --username <USER>  Account name
  -C <PATH>              Run as if wt was started in PATH
  -e, --email <EMAIL>    Email address
      --password-stdin   Read the password from the first line of stdin
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --no-color         Disable colored output
  -V, --version          Print version
```

### `wt login`

Log in to a server

```text
Usage: wt login [OPTIONS] [SERVER]

Arguments:
  [SERVER]  Server URL (default: the URL of the worktree's default remote)

Options:
      --json             Print machine-readable JSON (one document) on stdout
  -u, --username <USER>  Account name (prompted for if omitted)
  -C <PATH>              Run as if wt was started in PATH
      --password-stdin   Read the password from the first line of stdin
      --mfa-code <CODE>  TOTP or recovery code for accounts with two-factor authentication (prompted for when needed) [env: WT_MFA_CODE]
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --no-color         Disable colored output
  -V, --version          Print version
```

### `wt logout`

Forget the saved credential for a server

```text
Usage: wt logout [OPTIONS] [SERVER]

Arguments:
  [SERVER]  Server URL (default: the URL of the worktree's default remote)

Options:
      --json         Print machine-readable JSON (one document) on stdout
  -C <PATH>          Run as if wt was started in PATH
      --tree <NAME>  Operate on this tree instead of the one containing the current directory
      --no-color     Disable colored output
  -V, --version      Print version
```

### `wt whoami`

Show who you are logged in as

```text
Usage: wt whoami [OPTIONS] [SERVER]

Arguments:
  [SERVER]  Server URL (default: the URL of the worktree's default remote)

Options:
      --json         Print machine-readable JSON (one document) on stdout
  -C <PATH>          Run as if wt was started in PATH
      --tree <NAME>  Operate on this tree instead of the one containing the current directory
      --no-color     Disable colored output
  -V, --version      Print version
```

### `wt password-reset`

Set a new password with a one-time reset code from an administrator

```text
Usage: wt password-reset [OPTIONS] <CODE> [SERVER]

Arguments:
  <CODE>    One-time reset code (`wtr_...`) issued by a server administrator
  [SERVER]  Server URL (default: --server, else the worktree's origin, else the only saved login)

Options:
      --json            Print machine-readable JSON (one document) on stdout
      --password-stdin  Read the new password from the first line of stdin
  -C <PATH>             Run as if wt was started in PATH
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
  -V, --version         Print version
```

### `wt push`

Upload a branch to a remote

```text
Usage: wt push [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --remote <R>    Remote to push to (default: [sync].remote, usually origin)
      --branch <B>    Branch to push (default: the current branch)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt fetch`

Download remote branches without touching local ones

```text
Usage: wt fetch [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --remote <R>    Remote to fetch from (default: [sync].remote, usually origin)
  -C <PATH>           Run as if wt was started in PATH
      --depth <N>     Download at most N new snapshots per branch (shallow history)
      --deepen <N>    Extend a shallow history by N snapshots per branch
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt pull`

Fetch and integrate a remote branch

```text
Usage: wt pull [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --remote <R>    Remote to pull from (default: [sync].remote, usually origin)
      --branch <B>    Branch to integrate (default: the current branch)
  -C <PATH>           Run as if wt was started in PATH
      --depth <N>     Fetch at most N new snapshots per branch (shallow history)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt clone`

Clone a tree from a server

```text
Usage: wt clone [OPTIONS] <URL> <TENANT/TREE> [DEST]

Arguments:
  <URL>          Server URL, e.g. http://localhost:7420
  <TENANT/TREE>  Tree to clone
  [DEST]         Destination directory (default: ./TREE)

Options:
      --depth <N>     Clone only the newest N snapshots of each branch (shallow clone)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --watch         Start the background daemon (if needed) and auto-snapshot the clone
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt stage`

Upload work in progress as a team-visible staged snapshot

```text
Usage: wt stage [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --remote <R>    Remote to stage on (default: [sync].remote, usually origin)
  -C <PATH>           Run as if wt was started in PATH
      --discard       Remove your staged snapshot of the current branch instead (same as `wt unstage`)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt unstage`

Remove your staged snapshot from the remote

```text
Usage: wt unstage [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --remote <R>    Remote to unstage from (default: [sync].remote, usually origin)
      --branch <B>    Branch whose staged snapshot to remove (default: the current branch)
  -C <PATH>           Run as if wt was started in PATH
      --all           Remove your staged snapshots of every branch of the tree
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Export and interop

### `wt archive`

Write a snapshot's files to a tar.gz or zip archive

```text
Usage: wt archive [OPTIONS] --output <FILE> [REV]

Arguments:
  [REV]  Snapshot to archive (default: HEAD)

Options:
      --json                Print machine-readable JSON (one document) on stdout
  -o, --output <FILE>       Archive file to write
  -C <PATH>                 Run as if wt was started in PATH
      --format <FORMAT>     Archive format (default: from the file extension, else tar.gz) [possible values: tar.gz, zip]
      --no-license          Ignore license rules: export every path and add no LICENSES/NOTICE
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --include-restricted  Keep paths whose license forbids redistribution
      --no-color            Disable colored output
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version             Print version
```

### `wt git`

Import from or export to Git

```text
Usage: wt git [OPTIONS] <COMMAND>

Commands:
  import  Import a Git repository (URL or path) with its branches and tags
  export  Export this tree's history into a Git repository (created if missing)
  push    Export this tree and push it to a Git remote
  remote  Manage Git remotes (stored in .wt/config.toml [git_remote.NAME])
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt git import`

Import a Git repository (URL or path) with its branches and tags

```text
A URL (https://, ssh://, user@host:path, file://) is cloned into a temporary directory first; a path is read in place. Without `--tree` a new worktree is created at DEST (default: ./<repository name>). Inside a worktree, `--tree NAME` imports into that tree instead (creating it as a nested tree at DEST or ./NAME if it does not exist); importing again adds only new commits and fast-forwards branches.

Usage: wt git import [OPTIONS] <URL|PATH> [DEST]

Arguments:
  <URL|PATH>
          Git repository URL or path

  [DEST]
          Destination directory

Options:
      --branches <A,B>
          Import only these branches (comma-separated); a name without a local branch selects the remote-tracking branch of that name

      --json
          Print machine-readable JSON (one document) on stdout

      --all-branches
          Also import remote-tracking branches (refs/remotes/*) of a local repository (default: local branches only)

  -C <PATH>
          Run as if wt was started in PATH

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

#### `wt git export`

Export this tree's history into a Git repository (created if missing)

```text
Usage: wt git export [OPTIONS] <GIT_PATH>

Arguments:
  <GIT_PATH>  Git repository to write; a missing path ending in `.git` is created bare

Options:
      --json                Print machine-readable JSON (one document) on stdout
      --rev <REV>           Branch to export (default: all branches)
  -C <PATH>                 Run as if wt was started in PATH
      --no-license          Ignore license rules: export every path and add no LICENSES/NOTICE
      --include-restricted  Keep paths whose license forbids redistribution
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --no-color            Disable colored output
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version             Print version
```

#### `wt git push`

Export this tree and push it to a Git remote

```text
TARGET is a remote added with `wt git remote add`, a URL or a path (a missing local path is created as a bare repository). The remote's history is fetched first so imported commits are reused and pushes fast-forward. Tags are pushed too.

Usage: wt git push [OPTIONS] <NAME|URL|PATH>

Arguments:
  <NAME|URL|PATH>
          Git remote name, URL or path

Options:
  -b, --branch <B>
          Branch to push (repeatable; default: all branches)

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

  -f, --force
          Overwrite remote branches even when the push is not a fast-forward

      --no-license
          Ignore license rules: export every path and add no LICENSES/NOTICE

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --include-restricted
          Keep paths whose license forbids redistribution

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

#### `wt git remote`

Manage Git remotes (stored in .wt/config.toml [git_remote.NAME])

```text
Usage: wt git remote [OPTIONS] <COMMAND>

Commands:
  add     Add a Git remote (local paths are stored absolute)
  list    List Git remotes
  remove  Remove a Git remote
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt git remote add`

Add a Git remote (local paths are stored absolute)

```text
Usage: wt git remote add [OPTIONS] <NAME> <URL>

Arguments:
  <NAME>  
  <URL>   

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt git remote list`

List Git remotes

```text
Usage: wt git remote list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt git remote remove`

Remove a Git remote

```text
Usage: wt git remote remove [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Background daemon

### `wt daemon`

Control the background daemon

```text
Usage: wt daemon [OPTIONS] <COMMAND>

Commands:
  start     Start the daemon in the background
  stop      Stop the daemon
  status    Show the daemon's state and watched worktrees
  watch     Auto-snapshot a worktree (default: the current one)
  unwatch   Stop watching a worktree (default: the current one)
  pause     Pause auto-snapshots
  resume    Resume auto-snapshots
  restart   Stop the daemon (if running) and start it again
  logs      Show the daemon's log ($WT_HOME/bg.log)
  snapshot  Snapshot every changed tree of a worktree now (default: the current one)
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon start`

Start the daemon in the background

```text
Usage: wt daemon start [OPTIONS]

Options:
      --foreground    Run attached to this terminal instead
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon stop`

Stop the daemon

```text
Usage: wt daemon stop [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon status`

Show the daemon's state and watched worktrees

```text
Usage: wt daemon status [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon watch`

Auto-snapshot a worktree (default: the current one)

```text
Usage: wt daemon watch [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon unwatch`

Stop watching a worktree (default: the current one)

```text
Usage: wt daemon unwatch [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon pause`

Pause auto-snapshots

```text
Usage: wt daemon pause [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon resume`

Resume auto-snapshots

```text
Usage: wt daemon resume [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon restart`

Stop the daemon (if running) and start it again

```text
Usage: wt daemon restart [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon logs`

Show the daemon's log ($WT_HOME/bg.log)

```text
Usage: wt daemon logs [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -n, --lines <N>     Number of trailing lines to show [default: 50]
  -C <PATH>           Run as if wt was started in PATH
  -f, --follow        Keep printing new lines as they are written (Ctrl-C to stop)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt daemon snapshot`

Snapshot every changed tree of a worktree now (default: the current one)

```text
Usage: wt daemon snapshot [OPTIONS] [PATH]

Arguments:
  [PATH]  

Options:
      --all           Snapshot every watched worktree
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Collaboration

### `wt tenant`

Manage tenants (organizations) and their members on the server

```text
Usage: wt tenant [OPTIONS] <COMMAND>

Commands:
  list           List the tenants you can see
  create         Create an organization tenant (you become its owner)
  show           Show a tenant
  rename         Change a tenant's display name
  plan           Change a tenant's plan (server admins only)
  members        List a tenant's members
  add-member     Add a member to a tenant
  set-role       Change a member's role
  remove-member  Remove a member from a tenant
  suspend        Suspend a tenant: it becomes read-only (server admins only; nothing is revoked)
  resume         Lift a suspension or deactivation (server admins only)
  deactivate     Deactivate a tenant: all access is refused (server admins only)
  help           Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant list`

List the tenants you can see

```text
Usage: wt tenant list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant create`

Create an organization tenant (you become its owner)

```text
Usage: wt tenant create [OPTIONS] <SLUG>

Arguments:
  <SLUG>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --name <N>      Display name (default: the slug)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant show`

Show a tenant

```text
Usage: wt tenant show [OPTIONS] <TENANT>

Arguments:
  <TENANT>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant rename`

Change a tenant's display name

```text
Usage: wt tenant rename [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  
  <NAME>    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant plan`

Change a tenant's plan (server admins only)

```text
Usage: wt tenant plan [OPTIONS] <TENANT> <PLAN>

Arguments:
  <TENANT>  
  <PLAN>    [possible values: free, pro, enterprise]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant members`

List a tenant's members

```text
Usage: wt tenant members [OPTIONS] <TENANT>

Arguments:
  <TENANT>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant add-member`

Add a member to a tenant

```text
Usage: wt tenant add-member [OPTIONS] <TENANT> <USERNAME>

Arguments:
  <TENANT>    
  <USERNAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --role <R>      Role of the new member [default: developer]
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant set-role`

Change a member's role

```text
Usage: wt tenant set-role [OPTIONS] <TENANT> <USERNAME> <ROLE>

Arguments:
  <TENANT>    
  <USERNAME>  
  <ROLE>      

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant remove-member`

Remove a member from a tenant

```text
Usage: wt tenant remove-member [OPTIONS] <TENANT> <USERNAME>

Arguments:
  <TENANT>    
  <USERNAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant suspend`

Suspend a tenant: it becomes read-only (server admins only; nothing is revoked)

```text
Usage: wt tenant suspend [OPTIONS] <TENANT>

Arguments:
  <TENANT>  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --reason <TEXT>  Why, shown to members whose writes are refused
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt tenant resume`

Lift a suspension or deactivation (server admins only)

```text
Usage: wt tenant resume [OPTIONS] <TENANT>

Arguments:
  <TENANT>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt tenant deactivate`

Deactivate a tenant: all access is refused (server admins only)

```text
Usage: wt tenant deactivate [OPTIONS] <TENANT>

Arguments:
  <TENANT>  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --reason <TEXT>  Why, shown to members whose requests are refused
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

### `wt trees`

Manage trees on the server: create, settings, branches, tags, compare, stats

```text
Usage: wt trees [OPTIONS] <COMMAND>

Commands:
  list           List trees (default: the origin tenant, else every tenant you can see)
  create         Create a tree
  show           Show a tree and its counters
  update         Change a tree's settings
  delete         Delete a tree and everything in it
  branches       List a tree's branches
  branch-create  Create a branch on the server
  branch-delete  Delete a branch on the server
  tags           List a tree's tags
  tag-create     Create a tag on the server
  tag-delete     Delete a tag on the server
  log            Show snapshot history of a branch (newest first)
  compare        Compare two revisions (snapshot ids, branches or tags)
  stats          Show contributors, activity and languages of a branch
  file-log       Show the snapshots that changed a file
  staged         Show the team's staged (work-in-progress) snapshots
  help           Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees list`

List trees (default: the origin tenant, else every tenant you can see)

```text
Usage: wt trees list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --tenant <T>    Only trees of this tenant
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees create`

Create a tree

```text
Usage: wt trees create [OPTIONS] <T/NAME>

Arguments:
  <T/NAME>  TENANT/NAME of the new tree

Options:
      --description <D>     
      --json                Print machine-readable JSON (one document) on stdout
  -C <PATH>                 Run as if wt was started in PATH
      --visibility <V>      Who can read the tree (server default: private) [possible values: private, internal, public]
      --default-branch <B>  Name of the default branch (server default: main)
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --no-color            Disable colored output
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version             Print version
```

#### `wt trees show`

Show a tree and its counters

```text
Usage: wt trees show [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees update`

Change a tree's settings

```text
Usage: wt trees update [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --description <D>     
      --json                Print machine-readable JSON (one document) on stdout
  -C <PATH>                 Run as if wt was started in PATH
      --visibility <V>      [possible values: private, internal, public]
      --default-branch <B>  
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --no-color            Disable colored output
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version             Print version
```

#### `wt trees delete`

Delete a tree and everything in it

```text
Usage: wt trees delete [OPTIONS] <TREE>

Arguments:
  <TREE>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees branches`

List a tree's branches

```text
Usage: wt trees branches [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees branch-create`

Create a branch on the server

```text
Usage: wt trees branch-create [OPTIONS] <TREE> <NAME>

Arguments:
  <TREE>  
  <NAME>  

Options:
      --from <REV>    Start point: snapshot id, branch or tag (default: the default branch tip)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees branch-delete`

Delete a branch on the server

```text
Usage: wt trees branch-delete [OPTIONS] <TREE> <NAME>

Arguments:
  <TREE>  
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees tags`

List a tree's tags

```text
Usage: wt trees tags [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees tag-create`

Create a tag on the server

```text
Usage: wt trees tag-create [OPTIONS] --target <REV> <TREE> <NAME>

Arguments:
  <TREE>  
  <NAME>  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --target <REV>   Snapshot id, branch or tag to tag
  -C <PATH>            Run as if wt was started in PATH
  -m, --message <MSG>  Tag message
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt trees tag-delete`

Delete a tag on the server

```text
Usage: wt trees tag-delete [OPTIONS] <TREE> <NAME>

Arguments:
  <TREE>  
  <NAME>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees log`

Show snapshot history of a branch (newest first)

```text
Usage: wt trees log [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --branch <B>    Branch (default: the default branch)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
  -n <N>              Number of snapshots (server default: 50)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees compare`

Compare two revisions (snapshot ids, branches or tags)

```text
Usage: wt trees compare [OPTIONS] --to <REV> [TREE]

Arguments:
  [TREE]  

Options:
      --from <REV>    Base revision (default: the first parent of --to)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --to <REV>      Revision to compare
      --stat          Show a diffstat
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --patch         Show the unified diff
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees stats`

Show contributors, activity and languages of a branch

```text
Usage: wt trees stats [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --days <N>      Activity window in days (1-365, server default: 30)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees file-log`

Show the snapshots that changed a file

```text
Usage: wt trees file-log [OPTIONS] <TREE> <PATH>

Arguments:
  <TREE>  
  <PATH>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt trees staged`

Show the team's staged (work-in-progress) snapshots

```text
Usage: wt trees staged [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt mr`

Merge requests: list, open, review, approve, merge, close

```text
Usage: wt mr [OPTIONS] <COMMAND>

Commands:
  list         List merge requests
  show         Show a merge request
  create       Open a merge request
  approve      Approve a merge request
  merge        Merge (fast-forward) a merge request into its target
  close        Close a merge request without merging
  review       Review a merge request: approve, request changes or comment
  comment      Comment on a merge request, optionally on a file line
  comments     List the comments of a merge request
  checks       Show whether a merge request can be merged, and what blocks it
  link         Link merge requests of several trees into a group that merges atomically
  merge-group  Merge every merge request of a linked group, all or nothing
  groups       List the linked groups of a tenant
  help         Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr list`

List merge requests

```text
Usage: wt mr list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --state <STATE>  Which merge requests to list [default: open] [possible values: open, merged, closed, all]
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt mr show`

Show a merge request

```text
Usage: wt mr show [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --diff          Also show the changes it brings (JSON: {"merge_request", "diff"})
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr create`

Open a merge request

```text
Usage: wt mr create [OPTIONS] --source <B> --target <B> --title <T> [TREE]

Arguments:
  [TREE]  

Options:
      --json             Print machine-readable JSON (one document) on stdout
      --source <B>       Branch with the changes
  -C <PATH>              Run as if wt was started in PATH
      --target <B>       Branch to merge into
      --title <T>        
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --description <D>  
      --no-color         Disable colored output
      --server <URL>     Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version          Print version
```

#### `wt mr approve`

Approve a merge request

```text
Usage: wt mr approve [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr merge`

Merge (fast-forward) a merge request into its target

```text
Usage: wt mr merge [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr close`

Close a merge request without merging

```text
Usage: wt mr close [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr review`

Review a merge request: approve, request changes or comment

```text
Approvals and change requests apply to the current source tip: when the source moves, approvals go stale and must be renewed; a change request blocks merging until the same reviewer approves.

Usage: wt mr review [OPTIONS] <--approve|--request-changes|--comment> <NUMBER> [TREE]

Arguments:
  <NUMBER>
          

  [TREE]
          

Options:
      --approve
          Approve the current source tip

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

      --request-changes
          Request changes (blocks merging until you approve)

      --comment
          Comment without approving (needs -m)

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

  -m, --message <TEXT>
          Review text

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

#### `wt mr comment`

Comment on a merge request, optionally on a file line

```text
Usage: wt mr comment [OPTIONS] --message <TEXT> <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json            Print machine-readable JSON (one document) on stdout
  -m, --message <TEXT>  Comment text
  -C <PATH>             Run as if wt was started in PATH
      --path <PATH>     File the comment refers to
      --line <N>        Line of --path the comment refers to
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

#### `wt mr comments`

List the comments of a merge request

```text
Usage: wt mr comments [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr checks`

Show whether a merge request can be merged, and what blocks it

```text
Usage: wt mr checks [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mr link`

Link merge requests of several trees into a group that merges atomically

```text
Usage: wt mr link [OPTIONS] <NAME> <TREE#N> <TREE#N>...

Arguments:
  <NAME>                Name of the group
  <TREE#N> <TREE#N>...  Members as TENANT/TREE#N (at least two)

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --tenant <SLUG>  Tenant that owns the group (default: the first member's tenant)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt mr merge-group`

Merge every merge request of a linked group, all or nothing

```text
Usage: wt mr merge-group [OPTIONS] <ID>

Arguments:
  <ID>  Group id (see `wt mr groups`)

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --tenant <SLUG>  Tenant that owns the group (default: this worktree's origin tenant)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt mr groups`

List the linked groups of a tenant

```text
Usage: wt mr groups [OPTIONS]

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --tenant <SLUG>  Tenant (default: this worktree's origin tenant)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

### `wt ci`

CI check results: report a status, show checks of a snapshot or branch

```text
Usage: wt ci [OPTIONS] <COMMAND>

Commands:
  report  Report the result of a CI check for a snapshot (needs ci:report)
  status  Show the combined CI state and checks of a snapshot or branch
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt ci report`

Report the result of a CI check for a snapshot (needs ci:report)

```text
A newer report for the same snapshot and context replaces the older one. Branch protection with --require-ci blocks merges until the required contexts report success on the source tip.

Usage: wt ci report [OPTIONS] --snapshot <ID> --context <NAME> --state <STATE> [TREE]

Arguments:
  [TREE]
          

Options:
      --json
          Print machine-readable JSON (one document) on stdout

      --snapshot <ID>
          Snapshot id (or a branch / tag, resolved to its snapshot)

  -C <PATH>
          Run as if wt was started in PATH

      --context <NAME>
          Check name, e.g. "ci/build"

      --state <STATE>
          Check state
          
          [possible values: pending, success, failure, error]

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --description <TEXT>
          Short description of the result

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

      --url <URL>
          Link to the CI run (http or https)

  -V, --version
          Print version
```

#### `wt ci status`

Show the combined CI state and checks of a snapshot or branch

```text
Usage: wt ci status [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --snapshot <ID>  Snapshot id (or a branch / tag)
      --branch <B>     Branch whose tip to show (default: the tree's default branch)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

### `wt protect`

Branch protection rules of a server tree

```text
Usage: wt protect [OPTIONS] <COMMAND>

Commands:
  list    List the protection rules of a tree
  set     Create or replace the rule for a branch pattern (unset flags are off)
  remove  Remove the rule for a branch pattern
  tag     Tag protection rules: who may create tags, and whether they can move or be deleted
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt protect list`

List the protection rules of a tree

```text
Usage: wt protect list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt protect set`

Create or replace the rule for a branch pattern (unset flags are off)

```text
Usage: wt protect set [OPTIONS] [TREE] <PATTERN>

Arguments:
  [TREE]     
  <PATTERN>  Branch name or glob, e.g. "main" or "release/*"

Options:
      --json            Print machine-readable JSON (one document) on stdout
      --no-direct-push  Changes reach matching branches only through merge requests
      --approvals <N>   Approvals required to merge a merge request [default: 0]
  -C <PATH>             Run as if wt was started in PATH
      --pusher <U>      Account allowed to push (repeatable; default: anyone with write access)
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --no-delete       Matching branches cannot be deleted
      --require-ci      Merges need passing CI on the source tip (every --check, or every reported check)
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --check <NAME>    CI context that must succeed (repeatable; implies --require-ci)
      --require-signed  Pushed snapshots must carry a verified signature
      --require-linear  No merge snapshots: direct pushes and merged sources must be linear
      --block-on-todos  Open blocking TODOs of the source branch block merging
  -V, --version         Print version
```

#### `wt protect remove`

Remove the rule for a branch pattern

```text
Usage: wt protect remove [OPTIONS] [TREE] <PATTERN>

Arguments:
  [TREE]     
  <PATTERN>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt protect tag`

Tag protection rules: who may create tags, and whether they can move or be deleted

```text
Usage: wt protect tag [OPTIONS] <COMMAND>

Commands:
  list    List the tag protection rules of a tree
  set     Create or replace the rule for a tag pattern (unset flags are off)
  remove  Remove the rule for a tag pattern
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt protect tag list`

List the tag protection rules of a tree

```text
Usage: wt protect tag list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt protect tag set`

Create or replace the rule for a tag pattern (unset flags are off)

```text
Usage: wt protect tag set [OPTIONS] [TREE] <PATTERN>

Arguments:
  [TREE]     
  <PATTERN>  Tag name or glob, e.g. "v*"

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --no-delete     Matching tags cannot be deleted
  -C <PATH>           Run as if wt was started in PATH
      --no-move       Matching tags cannot be replaced (moved to another snapshot)
      --tagger <U>    Account allowed to create, move or delete matching tags (repeatable; default: anyone with write access)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

##### `wt protect tag remove`

Remove the rule for a tag pattern

```text
Usage: wt protect tag remove [OPTIONS] [TREE] <PATTERN>

Arguments:
  [TREE]     
  <PATTERN>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt license`

Path license rules of a server tree

```text
Usage: wt license [OPTIONS] <COMMAND>

Commands:
  list    List the license rules of a tree
  set     Assign an SPDX license to the paths matching a glob
  remove  Remove the rule for a glob
  detect  Guess the SPDX licenses of the tree's files (LICENSE files and SPDX-License-Identifier headers) and propose license rules
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt license list`

List the license rules of a tree

```text
Usage: wt license list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt license set`

Assign an SPDX license to the paths matching a glob

```text
Usage: wt license set [OPTIONS] [TREE] <GLOB> <SPDX>

Arguments:
  <ARG> <ARG>...
          [TREE] GLOB SPDX: an optional TENANT/TREE, the path glob and the SPDX license id

Options:
      --grant <GRANT>
          What other tenants may do with the matching files

          Possible values:
          - read-only
          - modify
          - redistribute
          - license-default: No explicit grant: permissive licenses allow everything, proprietary ones (LicenseRef-*) nothing (sent as "")
          
          [default: read-only]

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

      --grantee <TENANT>
          Apply the rule only to this tenant (slug; its members, its service accounts or the user whose personal tenant it is). Default: every tenant that does not own the tree

      --expires <WHEN>
          Stop applying the rule after this time (RFC 3339, e.g. 2026-06-30T23:59:59Z, or a date YYYY-MM-DD meaning the end of that day in UTC)

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

#### `wt license remove`

Remove the rule for a glob

```text
Usage: wt license remove [OPTIONS] [TREE] <GLOB>

Arguments:
  [TREE]  
  <GLOB>  

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt license detect`

Guess the SPDX licenses of the tree's files (LICENSE files and SPDX-License-Identifier headers) and propose license rules

```text
A LICENSE/LICENCE/COPYING file proposes a rule for its directory (several license files in one directory combine with OR); a file whose SPDX header differs from its directory's license proposes a rule for that file. `--apply` creates the proposed rules on the tree's server (rules that already exist are skipped).

Usage: wt license detect [OPTIONS] [PATH]

Arguments:
  [PATH]
          File or directory to scan (default: the whole tree)

Options:
      --apply
          Create the proposed rules on the server

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

      --grant <GRANT>
          Grant for the created rules

          Possible values:
          - read-only
          - modify
          - redistribute
          - license-default: No explicit grant: permissive licenses allow everything, proprietary ones (LicenseRef-*) nothing (sent as "")
          
          [default: read-only]

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --yes
          Do not ask for confirmation

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

  -V, --version
          Print version
```

### `wt todo`

TODOs created from snapshot dependencies: list, show, claim, complete, cancel

```text
Usage: wt todo [OPTIONS] <COMMAND>

Commands:
  list      List TODOs of a tree
  show      Show one TODO
  claim     Assign a TODO to yourself and mark it in progress
  complete  Mark a TODO done
  cancel    Cancel a TODO
  create    Create a TODO by hand
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt todo list`

List TODOs of a tree

```text
Usage: wt todo list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json             Print machine-readable JSON (one document) on stdout
      --status <STATUS>  Which TODOs to list (active = open and in progress) [default: active] [possible values: active, open, in-progress, done, cancelled, all]
      --branch <B>       Only TODOs targeting this branch
  -C <PATH>              Run as if wt was started in PATH
      --assignee <USER>  Only TODOs assigned to this user ("me" = you)
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --all-trees        List the TODOs of every tree of the tenant
      --no-color         Disable colored output
      --server <URL>     Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version          Print version
```

#### `wt todo show`

Show one TODO

```text
Usage: wt todo show [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt todo claim`

Assign a TODO to yourself and mark it in progress

```text
Usage: wt todo claim [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt todo complete`

Mark a TODO done

```text
Usage: wt todo complete [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json            Print machine-readable JSON (one document) on stdout
      --snapshot <REV>  Snapshot (id, branch or tag) that resolves it
  -C <PATH>             Run as if wt was started in PATH
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

#### `wt todo cancel`

Cancel a TODO

```text
Usage: wt todo cancel [OPTIONS] <NUMBER> [TREE]

Arguments:
  <NUMBER>  
  [TREE]    

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt todo create`

Create a TODO by hand

```text
Usage: wt todo create [OPTIONS] <TITLE> [TREE]

Arguments:
  <TITLE>  
  [TREE]   

Options:
      --details <TEXT>       
      --json                 Print machine-readable JSON (one document) on stdout
  -C <PATH>                  Run as if wt was started in PATH
      --priority <PRIORITY>  [default: medium] [possible values: low, medium, high, critical]
      --blocking             Blocks merging of the branches that depend on it
      --tree <NAME>          Operate on this tree instead of the one containing the current directory
      --branch <B>           Branch the work is needed on (default: the tree's default branch)
      --no-color             Disable colored output
      --server <URL>         Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version              Print version
```

### `wt deps`

Dependencies declared by snapshots: status and graph

```text
Usage: wt deps [OPTIONS] <COMMAND>

Commands:
  status  Dependencies of a branch and their TODOs, plus TODOs requested from the tree
  graph   Dependency graph between trees (text or Graphviz dot)
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt deps status`

Dependencies of a branch and their TODOs, plus TODOs requested from the tree

```text
Usage: wt deps status [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --branch <B>    Branch whose snapshots declare the dependencies (default: the default branch)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt deps graph`

Dependency graph between trees (text or Graphviz dot)

```text
Usage: wt deps graph [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --branch <B>       
      --json             Print machine-readable JSON (one document) on stdout
  -C <PATH>              Run as if wt was started in PATH
      --format <FORMAT>  [default: text] [possible values: text, dot]
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --no-color         Disable colored output
      --server <URL>     Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version          Print version
```

### `wt release`

Releases of a server tree: list, create, show, edit, delete, assets

```text
Usage: wt release [OPTIONS] <COMMAND>

Commands:
  list      List releases (newest first; drafts only if you may create releases)
  create    Create a release (the tag is created at --snapshot when it does not exist)
  show      Show a release and its assets
  edit      Edit a release
  delete    Delete a release (its tag is kept)
  upload    Upload files as release assets (replacing assets of the same name)
  download  Download release assets (all of them without ASSET)
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt release list`

List releases (newest first; drafts only if you may create releases)

```text
Usage: wt release list [OPTIONS] [TREE]

Arguments:
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt release create`

Create a release (the tag is created at --snapshot when it does not exist)

```text
Usage: wt release create [OPTIONS] <TAG> [TREE]

Arguments:
  <TAG>   
  [TREE]  

Options:
      --json               Print machine-readable JSON (one document) on stdout
      --name <NAME>        Display name (default: the tag)
  -C <PATH>                Run as if wt was started in PATH
      --notes <TEXT>       Release notes
      --notes-file <FILE>  Read the release notes from a file
      --tree <NAME>        Operate on this tree instead of the one containing the current directory
      --draft              Create it as an unpublished draft
      --no-color           Disable colored output
      --prerelease         Mark it as a pre-release
      --server <URL>       Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --snapshot <REV>     Snapshot (id, branch or tag) to tag when TAG does not exist yet
  -V, --version            Print version
```

#### `wt release show`

Show a release and its assets

```text
Usage: wt release show [OPTIONS] <TAG> [TREE]

Arguments:
  <TAG>   
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt release edit`

Edit a release

```text
Usage: wt release edit [OPTIONS] <TAG> [TREE]

Arguments:
  <TAG>   
  [TREE]  

Options:
      --json               Print machine-readable JSON (one document) on stdout
      --name <NAME>        
  -C <PATH>                Run as if wt was started in PATH
      --notes <TEXT>       
      --notes-file <FILE>  
      --tree <NAME>        Operate on this tree instead of the one containing the current directory
      --draft              Turn it back into a draft
      --no-color           Disable colored output
      --publish            Publish a draft
      --server <URL>       Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --prerelease         Mark it as a pre-release
      --no-prerelease      Mark it as a regular release
  -V, --version            Print version
```

#### `wt release delete`

Delete a release (its tag is kept)

```text
Usage: wt release delete [OPTIONS] <TAG> [TREE]

Arguments:
  <TAG>   
  [TREE]  

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt release upload`

Upload files as release assets (replacing assets of the same name)

```text
Usage: wt release upload [OPTIONS] <TAG> <FILE>...

Arguments:
  <TAG>      
  <FILE>...  

Options:
      --in <TENANT/TREE>  Server tree (default: the tree of the current worktree)
      --json              Print machine-readable JSON (one document) on stdout
  -C <PATH>               Run as if wt was started in PATH
      --tree <NAME>       Operate on this tree instead of the one containing the current directory
      --no-color          Disable colored output
      --server <URL>      Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version           Print version
```

#### `wt release download`

Download release assets (all of them without ASSET)

```text
Usage: wt release download [OPTIONS] <TAG> [ASSET]

Arguments:
  <TAG>    
  [ASSET]  

Options:
      --json              Print machine-readable JSON (one document) on stdout
  -o, --output <DIR>      Directory to write to [default: .]
  -C <PATH>               Run as if wt was started in PATH
      --in <TENANT/TREE>  Server tree (default: the tree of the current worktree)
      --tree <NAME>       Operate on this tree instead of the one containing the current directory
      --no-color          Disable colored output
      --server <URL>      Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version           Print version
```

### `wt search`

Search tenants, trees, branches and accounts on the server

```text
Usage: wt search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  Text to look for (case-insensitive substring)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -n <N>              Maximum hits per kind (server default: 20)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt events`

Stream live server events

```text
Usage: wt events [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --tenant <T>    Only events of this tenant
  -C <PATH>           Run as if wt was started in PATH
      --tree <TREE>   Only events of this tree (TENANT/TREE)
      --type <TYPE>   Only events of this type, e.g. `branch.pushed` (repeatable)
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

## Administration

### `wt admin`

Server administration: accounts, system information, statistics

```text
Usage: wt admin [OPTIONS] <COMMAND>

Commands:
  accounts        List every account on the server
  account-create  Create an account (bypasses the registration setting)
  account-set     Change an account's admin or disabled flag
  password        Set an account's password (revokes its tokens)
  reset-code      Issue a one-time password reset code for an account
  system          Show server version, runtime and storage information
  stats           Show object counts across the server
  gc              Run server garbage collection now: mark unreachable objects, delete those past the grace period, expire staged snapshots and audit events
  gc-status       Show the garbage-collection configuration and recent runs
  limits          Show or change a tenant's quotas (unset limits use the server defaults; 0 unsets a limit)
  usage           Show a tenant's resource usage against its effective limits
  help            Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin accounts`

List every account on the server

```text
Usage: wt admin accounts [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin account-create`

Create an account (bypasses the registration setting)

```text
Usage: wt admin account-create [OPTIONS] --email <EMAIL> <USERNAME>

Arguments:
  <USERNAME>  

Options:
      --email <EMAIL>   Email address
      --json            Print machine-readable JSON (one document) on stdout
      --admin           Make the account a server admin
  -C <PATH>             Run as if wt was started in PATH
      --password-stdin  Read the password from the first line of stdin
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

#### `wt admin account-set`

Change an account's admin or disabled flag

```text
Usage: wt admin account-set [OPTIONS] <USERNAME>

Arguments:
  <USERNAME>  

Options:
      --admin <BOOL>     Grant or revoke server admin [possible values: true, false]
      --json             Print machine-readable JSON (one document) on stdout
  -C <PATH>              Run as if wt was started in PATH
      --disabled <BOOL>  Disable or re-enable the account [possible values: true, false]
      --tree <NAME>      Operate on this tree instead of the one containing the current directory
      --no-color         Disable colored output
      --server <URL>     Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version          Print version
```

#### `wt admin password`

Set an account's password (revokes its tokens)

```text
Usage: wt admin password [OPTIONS] <USERNAME>

Arguments:
  <USERNAME>  

Options:
      --json            Print machine-readable JSON (one document) on stdout
      --password-stdin  Read the password from the first line of stdin
  -C <PATH>             Run as if wt was started in PATH
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

#### `wt admin reset-code`

Issue a one-time password reset code for an account

```text
Usage: wt admin reset-code [OPTIONS] <USERNAME>

Arguments:
  <USERNAME>  

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --ttl-hours <N>  Hours until the code expires (server default: 1)
  -C <PATH>            Run as if wt was started in PATH
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt admin system`

Show server version, runtime and storage information

```text
Usage: wt admin system [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin stats`

Show object counts across the server

```text
Usage: wt admin stats [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin gc`

Run server garbage collection now: mark unreachable objects, delete those past the grace period, expire staged snapshots and audit events

```text
Usage: wt admin gc [OPTIONS]

Options:
      --dry-run       Report what would be marked and deleted without changing anything
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin gc-status`

Show the garbage-collection configuration and recent runs

```text
Usage: wt admin gc-status [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt admin limits`

Show or change a tenant's quotas (unset limits use the server defaults; 0 unsets a limit)

```text
Usage: wt admin limits [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json                Print machine-readable JSON (one document) on stdout
      --max-trees <N>       Maximum number of trees
  -C <PATH>                 Run as if wt was started in PATH
      --max-storage <SIZE>  Maximum stored bytes, e.g. 500M, 10GiB or 1TB (K/M/G/T and KiB/MiB/GiB/TiB are powers of 1024, KB/MB/GB/TB powers of 1000)
      --max-branches <N>    Maximum number of branches per tree
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --max-members <N>     Maximum number of members
      --no-color            Disable colored output
      --rate <N>            Requests per minute per principal of the tenant
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --clear               Remove every tenant-specific limit (back to the server defaults)
  -V, --version             Print version
```

#### `wt admin usage`

Show a tenant's resource usage against its effective limits

```text
Usage: wt admin usage [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt audit`

Query the server audit log

```text
Usage: wt audit [OPTIONS]

Options:
      --json
          Print machine-readable JSON (one document) on stdout

      --tenant <T>
          Only events of this tenant (default: every tenant you administer)

      --action <A>
          Only events with this action, e.g. `branch.push` or `tree.create`; a trailing `.*` matches a prefix, e.g. `iam.*`

  -C <PATH>
          Run as if wt was started in PATH

      --resource-type <T>
          Only events on this resource type, e.g. `tree` or `account`

      --tree <NAME>
          Operate on this tree instead of the one containing the current directory

      --no-color
          Disable colored output

      --resource-id <ID>
          Only events on this resource id

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

      --since <RFC3339>
          Only events at or after this RFC 3339 time, e.g. 2026-01-31T00:00:00Z

      --until <RFC3339>
          Only events before this RFC 3339 time (exclusive)

  -n <N>
          Maximum number of events (server default: 100; ignored with `--format ndjson`)

      --format <FORMAT>
          Output format: `table`, or `ndjson` to export every matching event as one JSON object per line (no limit)

          Possible values:
          - table:  Aligned table (the JSON array with `--json`)
          - ndjson: Newline-delimited JSON export of every match
          
          [default: table]

  -V, --version
          Print version
```

## Identity and access

### `wt team`

Teams within a tenant and their members

```text
Usage: wt team [OPTIONS] <COMMAND>

Commands:
  list    List the teams of a tenant
  create  Create a team
  show    Show a team with its members and tree grants
  update  Change a team's name or description
  delete  Delete a team (its memberships and grants go with it)
  add     Add a member to a team (or change their team role)
  remove  Remove a member from a team
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt team list`

List the teams of a tenant

```text
Usage: wt team list [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt team create`

Create a team

```text
Usage: wt team create [OPTIONS] <TENANT> <SLUG>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug ([a-z0-9-])

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --name <NAME>                Display name (default: the slug)
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  Description [default: ""]
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt team show`

Show a team with its members and tree grants

```text
Usage: wt team show [OPTIONS] <TENANT> <SLUG>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt team update`

Change a team's name or description

```text
Usage: wt team update [OPTIONS] <TENANT> <SLUG>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --name <NAME>                New display name
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  New description
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt team delete`

Delete a team (its memberships and grants go with it)

```text
Usage: wt team delete [OPTIONS] <TENANT> <SLUG>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt team add`

Add a member to a team (or change their team role)

```text
Usage: wt team add [OPTIONS] <TENANT> <SLUG> <USER>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug
  <USER>    Username

Options:
      --json          Print machine-readable JSON (one document) on stdout
      --maintainer    Make the member a team maintainer (may manage the team's membership)
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt team remove`

Remove a member from a team

```text
Usage: wt team remove [OPTIONS] <TENANT> <SLUG> <USER>

Arguments:
  <TENANT>  Tenant slug
  <SLUG>    Team slug
  <USER>    Username

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt role`

Custom roles of a tenant

```text
Usage: wt role [OPTIONS] <COMMAND>

Commands:
  list    List built-in and custom roles
  create  Create a custom role
  show    Show a role and its permissions
  update  Change a custom role's permissions or description
  delete  Delete a custom role (refused while it is assigned)
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt role list`

List built-in and custom roles

```text
Usage: wt role list [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt role create`

Create a custom role

```text
Usage: wt role create [OPTIONS] --permission <ACTION> <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Role name ([a-z0-9-], must not collide with a built-in role)

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --permission <ACTION>        Granted action (`tree:read`, `area:*`, ...); repeatable
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  Description [default: ""]
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt role show`

Show a role and its permissions

```text
Usage: wt role show [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Role name

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt role update`

Change a custom role's permissions or description

```text
Usage: wt role update [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Role name

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --permission <ACTION>        Replacement permission list; repeatable
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  New description
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt role delete`

Delete a custom role (refused while it is assigned)

```text
Usage: wt role delete [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Role name

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt policy`

Access policies (allow/deny statements) of a tenant

```text
Usage: wt policy [OPTIONS] <COMMAND>

Commands:
  list      List the policies of a tenant
  show      Show a policy and its statements
  create    Create a policy from a JSON file
  update    Replace a policy with the contents of a JSON file
  delete    Delete a policy
  validate  Check a policy file's statements without saving anything
  enable    Enable a policy
  disable   Disable a policy (kept, but not evaluated)
  simulate  Dry-run a policy change: evaluate a request with and without the policies of a file (nothing is saved)
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy list`

List the policies of a tenant

```text
Usage: wt policy list [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy show`

Show a policy and its statements

```text
Usage: wt policy show [OPTIONS] <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Policy id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy create`

Create a policy from a JSON file

```text
Usage: wt policy create [OPTIONS] --file <FILE> <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
  -f, --file <FILE>   Policy document (`-` = stdin)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy update`

Replace a policy with the contents of a JSON file

```text
Usage: wt policy update [OPTIONS] --file <FILE> <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Policy id

Options:
  -f, --file <FILE>   Policy document (`-` = stdin)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy delete`

Delete a policy

```text
Usage: wt policy delete [OPTIONS] <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Policy id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy validate`

Check a policy file's statements without saving anything

```text
Usage: wt policy validate [OPTIONS] --file <FILE> <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
  -f, --file <FILE>   Policy document (`-` = stdin)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy enable`

Enable a policy

```text
Usage: wt policy enable [OPTIONS] <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Policy id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy disable`

Disable a policy (kept, but not evaluated)

```text
Usage: wt policy disable [OPTIONS] <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Policy id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt policy simulate`

Dry-run a policy change: evaluate a request with and without the policies of a file (nothing is saved)

```text
By default the file's policies replace the tenant's policies of the same name and are added otherwise; with --only they are the only policies evaluated.

Usage: wt policy simulate [OPTIONS] --file <FILE> --principal <PRINCIPAL> --action <ACTION> <TENANT>

Arguments:
  <TENANT>
          Tenant slug

Options:
  -f, --file <FILE>
          One policy, an array of policies, or {"policies": [...]} (`-` = stdin)

      --json
          Print machine-readable JSON (one document) on stdout

  -C <PATH>
          Run as if wt was started in PATH

      --only
          Evaluate only the file's policies

      --principal <PRINCIPAL>
          `user:<name>` or `sa:<tenant>/<name>`

      --action <ACTION>
          Action, e.g. `tree:write` or `branch:push`

      --no-color
          Disable colored output

      --server <URL>
          Server for server-side commands (default: this worktree's origin, else the only saved login)
          
          [env: WT_SERVER=]

      --tree <TREE>
          Tree name (or TENANT/TREE) of the resource

      --branch <BRANCH>
          Branch of the resource

      --path <PATH>
          Path within the tree

      --ip <IP>
          Request IP address, for ip_ranges conditions

      --mfa
          Treat the session as MFA-verified, for require_mfa conditions

  -V, --version
          Print version
```

### `wt grant`

Per-tree access grants for users, teams and service accounts

```text
Usage: wt grant [OPTIONS] <COMMAND>

Commands:
  list    List the grants on a tree
  set     Grant a role on a tree (replaces the subject's existing grant)
  remove  Remove a subject's grant from a tree
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt grant list`

List the grants on a tree

```text
Usage: wt grant list [OPTIONS] <TREE>

Arguments:
  <TREE>  Server tree (TENANT/TREE)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt grant set`

Grant a role on a tree (replaces the subject's existing grant)

```text
Usage: wt grant set [OPTIONS] --role <ROLE> <--user <USER>|--team <SLUG>|--sa <NAME>|--tenant <SLUG>> <TREE>

Arguments:
  <TREE>  Server tree (TENANT/TREE)

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --user <USER>    A user, by username
  -C <PATH>            Run as if wt was started in PATH
      --team <SLUG>    A team of the tree's tenant, by slug
      --sa <NAME>      A service account, by name
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --tenant <SLUG>  Every member and service account of another tenant, by slug (cross-tenant access to this tree)
      --role <ROLE>    Built-in or custom role of the tree's tenant
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt grant remove`

Remove a subject's grant from a tree

```text
Usage: wt grant remove [OPTIONS] <--user <USER>|--team <SLUG>|--sa <NAME>|--tenant <SLUG>> <TREE>

Arguments:
  <TREE>  Server tree (TENANT/TREE)

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --user <USER>    A user, by username
  -C <PATH>            Run as if wt was started in PATH
      --team <SLUG>    A team of the tree's tenant, by slug
      --sa <NAME>      A service account, by name
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --tenant <SLUG>  Every member and service account of another tenant, by slug (cross-tenant access to this tree)
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -y, --yes            Do not ask for confirmation
  -V, --version        Print version
```

### `wt access`

Declarative access config (.wt/access): validate, plan, apply, pull

```text
Usage: wt access [OPTIONS] <COMMAND>

Commands:
  validate  Check the access files locally (syntax, actions, principals, resources)
  plan      Show what `wt access apply` would change on the server
  apply     Make the server's config-managed roles and policies match the files
  pull      Write the last applied document from the server as TOML files
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt access validate`

Check the access files locally (syntax, actions, principals, resources)

```text
Usage: wt access validate [OPTIONS]

Options:
      --dir <DIR>     Directory holding roles.toml / policies.toml (default: .wt/access at the worktree root, or the tree's .wt-tree/access with --tree-doc)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree-doc      Use the tree-level document (<tree>/.wt-tree/access/policies.toml: deny statements only) of the selected tree instead of the tenant's
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt access plan`

Show what `wt access apply` would change on the server

```text
Usage: wt access plan [OPTIONS]

Options:
      --dir <DIR>     Directory holding roles.toml / policies.toml (default: .wt/access at the worktree root, or the tree's .wt-tree/access with --tree-doc)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree-doc      Use the tree-level document (<tree>/.wt-tree/access/policies.toml: deny statements only) of the selected tree instead of the tenant's
      --tenant <T>    Tenant slug (default: the tenant of this worktree's origin remote)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt access apply`

Make the server's config-managed roles and policies match the files

```text
Usage: wt access apply [OPTIONS]

Options:
      --dir <DIR>     Directory holding roles.toml / policies.toml (default: .wt/access at the worktree root, or the tree's .wt-tree/access with --tree-doc)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree-doc      Use the tree-level document (<tree>/.wt-tree/access/policies.toml: deny statements only) of the selected tree instead of the tenant's
      --tenant <T>    Tenant slug (default: the tenant of this worktree's origin remote)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
  -y, --yes           Do not ask for confirmation
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt access pull`

Write the last applied document from the server as TOML files

```text
Usage: wt access pull [OPTIONS]

Options:
      --dir <DIR>     Directory holding roles.toml / policies.toml (default: .wt/access at the worktree root, or the tree's .wt-tree/access with --tree-doc)
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree-doc      Use the tree-level document (<tree>/.wt-tree/access/policies.toml: deny statements only) of the selected tree instead of the tenant's
      --tenant <T>    Tenant slug (default: the tenant of this worktree's origin remote)
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
  -y, --yes           Overwrite local files that differ without asking
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt sa`

Service accounts and their keys

```text
Usage: wt sa [OPTIONS] <COMMAND>

Commands:
  list        List the service accounts of a tenant
  create      Create a service account
  show        Show a service account and its keys
  update      Change a service account's role or description, or disable/enable it
  delete      Delete a service account with its keys and grants
  key-create  Create a key (bearer token) for a service account; shown once
  key-delete  Revoke a service-account key
  help        Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sa list`

List the service accounts of a tenant

```text
Usage: wt sa list [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sa create`

Create a service account

```text
Usage: wt sa create [OPTIONS] --role <ROLE> <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Name ([a-z0-9-])

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --role <ROLE>                Role the account acts with inside the tenant
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  Description [default: ""]
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt sa show`

Show a service account and its keys

```text
Usage: wt sa show [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Service account name

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sa update`

Change a service account's role or description, or disable/enable it

```text
Usage: wt sa update [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Service account name

Options:
      --json                       Print machine-readable JSON (one document) on stdout
      --role <ROLE>                New role
  -C <PATH>                        Run as if wt was started in PATH
      --description <DESCRIPTION>  New description
      --disable                    Disable the account (its keys stop working)
      --tree <NAME>                Operate on this tree instead of the one containing the current directory
      --enable                     Enable a disabled account
      --no-color                   Disable colored output
      --server <URL>               Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                    Print version
```

#### `wt sa delete`

Delete a service account with its keys and grants

```text
Usage: wt sa delete [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Service account name

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sa key-create`

Create a key (bearer token) for a service account; shown once

```text
Usage: wt sa key-create [OPTIONS] <TENANT> <NAME>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Service account name

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --name <NAME>    Key name [default: key]
  -C <PATH>            Run as if wt was started in PATH
      --ttl-hours <N>  Expire after this many hours (default: never)
      --scope <SCOPE>  Restrict the key to an action pattern (`tree:read`, `tree:*`); repeatable
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt sa key-delete`

Revoke a service-account key

```text
Usage: wt sa key-delete [OPTIONS] <TENANT> <NAME> <KEY_ID>

Arguments:
  <TENANT>  Tenant slug
  <NAME>    Service account name
  <KEY_ID>  Key id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt token`

Your personal API tokens

```text
Usage: wt token [OPTIONS] <COMMAND>

Commands:
  list    List your API tokens
  create  Create an API token; the secret is shown once
  revoke  Revoke an API token
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt token list`

List your API tokens

```text
Usage: wt token list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt token create`

Create an API token; the secret is shown once

```text
Usage: wt token create [OPTIONS] <NAME>

Arguments:
  <NAME>  Token name

Options:
      --json           Print machine-readable JSON (one document) on stdout
      --ttl-hours <N>  Expire after this many hours (default: never)
  -C <PATH>            Run as if wt was started in PATH
      --scope <SCOPE>  Restrict the token to an action pattern (`tree:read`, `tree:*`); repeatable
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt token revoke`

Revoke an API token

```text
Usage: wt token revoke [OPTIONS] <ID>

Arguments:
  <ID>  Token id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt session`

Your sign-in sessions

```text
Usage: wt session [OPTIONS] <COMMAND>

Commands:
  list           List your sessions (the one this CLI uses is marked current)
  revoke         Sign out one session
  revoke-others  Sign out every session except the current one
  help           Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt session list`

List your sessions (the one this CLI uses is marked current)

```text
Usage: wt session list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt session revoke`

Sign out one session

```text
Usage: wt session revoke [OPTIONS] <ID>

Arguments:
  <ID>  Session id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt session revoke-others`

Sign out every session except the current one

```text
Usage: wt session revoke-others [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt mfa`

Two-factor authentication (TOTP) and recovery codes

```text
Usage: wt mfa [OPTIONS] <COMMAND>

Commands:
  status          Show whether two-factor authentication is enabled
  enable          Enroll an authenticator app (shows a QR code, then asks for a code)
  disable         Turn two-factor authentication off (asks for your password)
  recovery-codes  Replace your recovery codes (asks for a current code)
  help            Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mfa status`

Show whether two-factor authentication is enabled

```text
Usage: wt mfa status [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mfa enable`

Enroll an authenticator app (shows a QR code, then asks for a code)

```text
Usage: wt mfa enable [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt mfa disable`

Turn two-factor authentication off (asks for your password)

```text
Usage: wt mfa disable [OPTIONS]

Options:
      --json            Print machine-readable JSON (one document) on stdout
      --password-stdin  Read the password from the first line of stdin
  -C <PATH>             Run as if wt was started in PATH
      --tree <NAME>     Operate on this tree instead of the one containing the current directory
      --no-color        Disable colored output
      --server <URL>    Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version         Print version
```

#### `wt mfa recovery-codes`

Replace your recovery codes (asks for a current code)

```text
Usage: wt mfa recovery-codes [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt invite`

Tenant invitations

```text
Usage: wt invite [OPTIONS] <COMMAND>

Commands:
  list    List a tenant's invitations
  create  Invite someone to a tenant; prints a one-time code
  revoke  Revoke a pending invitation
  show    Preview the invitation behind a code
  accept  Accept an invitation as the signed-in account
  help    Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt invite list`

List a tenant's invitations

```text
Usage: wt invite list [OPTIONS] <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt invite create`

Invite someone to a tenant; prints a one-time code

```text
Usage: wt invite create [OPTIONS] <--email <EMAIL>|--user <USER>> <TENANT>

Arguments:
  <TENANT>  Tenant slug

Options:
      --email <EMAIL>  Invite by email address
      --json           Print machine-readable JSON (one document) on stdout
  -C <PATH>            Run as if wt was started in PATH
      --user <USER>    Invite an existing account by username
      --role <ROLE>    Tenant role granted on acceptance [default: developer]
      --tree <NAME>    Operate on this tree instead of the one containing the current directory
      --no-color       Disable colored output
      --team <SLUG>    Also add the invitee to this team
      --server <URL>   Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version        Print version
```

#### `wt invite revoke`

Revoke a pending invitation

```text
Usage: wt invite revoke [OPTIONS] <TENANT> <ID>

Arguments:
  <TENANT>  Tenant slug
  <ID>      Invitation id

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt invite show`

Preview the invitation behind a code

```text
Usage: wt invite show [OPTIONS] <CODE>

Arguments:
  <CODE>  Invitation code (`wti_...`)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt invite accept`

Accept an invitation as the signed-in account

```text
Usage: wt invite accept [OPTIONS] <CODE>

Arguments:
  <CODE>  Invitation code (`wti_...`)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

### `wt iam`

Inspect access: simulate requests, effective permissions, action catalog

```text
Usage: wt iam [OPTIONS] <COMMAND>

Commands:
  actions    List every action roles and policies can refer to
  effective  Show the effective permissions of a principal (default: you)
  simulate   Evaluate an access request and explain the decision
  help       Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt iam actions`

List every action roles and policies can refer to

```text
Usage: wt iam actions [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt iam effective`

Show the effective permissions of a principal (default: you)

```text
Usage: wt iam effective [OPTIONS] <--tenant <TENANT>|--tree <TREE>>

Options:
      --json                   Print machine-readable JSON (one document) on stdout
      --tenant <TENANT>        Tenant slug
  -C <PATH>                    Run as if wt was started in PATH
      --tree <TREE>            Server tree (TENANT/TREE). A plain string: the `tree` id is shared with the global `--tree` flag, which this option replaces here
      --principal <PRINCIPAL>  `user:<name>` or `sa:<tenant>/<name>`
      --no-color               Disable colored output
      --server <URL>           Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version                Print version
```

#### `wt iam simulate`

Evaluate an access request and explain the decision

```text
Usage: wt iam simulate [OPTIONS] --principal <PRINCIPAL> --action <ACTION> --tenant <TENANT>

Options:
      --json                   Print machine-readable JSON (one document) on stdout
      --principal <PRINCIPAL>  `user:<name>` or `sa:<tenant>/<name>`
      --action <ACTION>        Action, e.g. `tree:write` or `branch:push`
  -C <PATH>                    Run as if wt was started in PATH
      --tenant <TENANT>        Tenant slug of the resource
      --no-color               Disable colored output
      --tree <TREE>            Tree name (or TENANT/TREE) of the resource
      --branch <BRANCH>        Branch of the resource
      --server <URL>           Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --path <PATH>            Path within the tree
      --ip <IP>                Request IP address, for ip_ranges conditions
      --mfa                    Treat the session as MFA-verified, for require_mfa conditions
  -V, --version                Print version
```

### `wt sso`

Single sign-on (OpenID Connect) configuration

```text
Usage: wt sso [OPTIONS] <COMMAND>

Commands:
  show  Show the SSO configuration
  set   Change the SSO configuration (unset options keep their value)
  help  Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sso show`

Show the SSO configuration

```text
Usage: wt sso show [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt sso set`

Change the SSO configuration (unset options keep their value)

```text
Usage: wt sso set [OPTIONS]

Options:
      --enabled <BOOL>           Turn SSO login on or off [possible values: true, false]
      --json                     Print machine-readable JSON (one document) on stdout
  -C <PATH>                      Run as if wt was started in PATH
      --name <NAME>              Provider name shown on the login button, e.g. "Okta"
      --issuer <URL>             OpenID Connect issuer URL
      --tree <NAME>              Operate on this tree instead of the one containing the current directory
      --client-id <ID>           OAuth client id
      --no-color                 Disable colored output
      --client-secret-stdin      Read the OAuth client secret from the first line of stdin
      --server <URL>             Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
      --redirect-url <URL>       Callback URL registered with the provider (…/api/v1/auth/sso/callback)
      --scope <SCOPE>            Requested scope; repeatable, replaces the list
      --allowed-domain <DOMAIN>  Email domain allowed to sign in; repeatable, replaces the list
      --auto-create <BOOL>       Create accounts on first SSO login [possible values: true, false]
      --link-by-email <BOOL>     Link first-time identities to existing accounts by verified email (only if local emails are trustworthy: they are not verified) [possible values: true, false]
      --username-claim <CLAIM>   ID-token claim used as the username
  -V, --version                  Print version
```

### `wt identity`

Snapshot signing keys: generate, show, register, list, revoke

```text
Usage: wt identity [OPTIONS] <COMMAND>

Commands:
  generate  Generate a new Ed25519 signing key in <wt home>/keys
  list      List local signing keys
  show      Show a key's fingerprint and public key
  register  Upload a key's public half to the server and record its key id
  revoke    Revoke a registered key on the server
  verify    Verify a snapshot's signature against local (and optionally an account's registered) keys
  help      Print this message or the help of the given subcommand(s)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity generate`

Generate a new Ed25519 signing key in <wt home>/keys

```text
Usage: wt identity generate [OPTIONS] [NAME]

Arguments:
  [NAME]  Key name (default: "default")

Options:
      --force         Replace an existing key of the same name
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity list`

List local signing keys

```text
Usage: wt identity list [OPTIONS]

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity show`

Show a key's fingerprint and public key

```text
Usage: wt identity show [OPTIONS] [NAME]

Arguments:
  [NAME]  Key name (default: [signing] key, else "default")

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity register`

Upload a key's public half to the server and record its key id

```text
Usage: wt identity register [OPTIONS] [NAME]

Arguments:
  [NAME]  Key name (default: [signing] key, else "default")

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity revoke`

Revoke a registered key on the server

```text
Usage: wt identity revoke [OPTIONS] <ID>

Arguments:
  <ID>  Server key id (or the name of a registered local key)

Options:
      --json          Print machine-readable JSON (one document) on stdout
  -y, --yes           Do not ask for confirmation
  -C <PATH>           Run as if wt was started in PATH
      --tree <NAME>   Operate on this tree instead of the one containing the current directory
      --no-color      Disable colored output
      --server <URL>  Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version       Print version
```

#### `wt identity verify`

Verify a snapshot's signature against local (and optionally an account's registered) keys

```text
Usage: wt identity verify [OPTIONS] [SNAPSHOT]

Arguments:
  [SNAPSHOT]  Snapshot to verify (default: HEAD)

Options:
      --account <USERNAME>  Also trust the registered keys of this server account
      --json                Print machine-readable JSON (one document) on stdout
  -C <PATH>                 Run as if wt was started in PATH
      --tree <NAME>         Operate on this tree instead of the one containing the current directory
      --no-color            Disable colored output
      --server <URL>        Server for server-side commands (default: this worktree's origin, else the only saved login) [env: WT_SERVER=]
  -V, --version             Print version
```
