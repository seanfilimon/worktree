# CLI Ergonomics Specification: Completions, Doctor, Aliases, Open

## Table of Contents

1. [Overview](#1-overview)
2. [Shell completions](#2-shell-completions)
3. [Aliases](#3-aliases)
4. [`wt doctor`](#4-wt-doctor)
5. [`wt open`](#5-wt-open)
6. [Grouped help](#6-grouped-help)
7. [Implementation status](#7-implementation-status)
8. [Roadmap](#8-roadmap)
9. [References](#9-references)

---

## 1. Overview

v0.6 (`docs/ENGINEERING_PLAN.md` §14.8) adds four conveniences to `wt` that
change nothing about the data model or the wire protocol:

| Feature | Command | Network |
|---|---|---|
| Shell completion scripts | `wt completions SHELL` | none |
| Command aliases | `[alias]` in config, `wt NAME args...` | none |
| Setup diagnostics | `wt doctor [--offline] [--timeout SECS] [--json]` | optional, bounded |
| Open the admin panel | `wt open [tree \| mr N \| snapshot REV \| file PATH] [--print] [--remote R]` | none (launches a browser) |

## 2. Shell completions

`wt completions bash|zsh|fish|powershell|elvish` prints a static completion
script generated from the clap command definition (`clap_complete`). It
completes every command, subcommand, flag and enumerated value. Aliases
configured when the script is generated (§3) are included as extra commands
described as ``Alias for `wt ...` ``; an alias named like a built-in command
is left out. Regenerate the script after changing aliases.

Installation (also in `wt completions --help`):

| Shell | Install |
|---|---|
| bash | `wt completions bash > ~/.local/share/bash-completion/completions/wt` |
| zsh | `wt completions zsh > "${fpath[1]}/_wt"`, then restart zsh |
| fish | `wt completions fish > ~/.config/fish/completions/wt.fish` |
| PowerShell | `wt completions powershell >> $PROFILE` (or dot-source a saved copy) |
| elvish | `wt completions elvish >> ~/.config/elvish/rc.elv` |

An unknown shell name is a usage error (exit 2).

## 3. Aliases

### 3.1 Configuration

Aliases live in an `[alias]` table of string values:

```toml
[alias]
st  = "status"
lg  = "log --oneline -n 20"
wip = "snapshot --allow-empty -m 'work in progress'"
```

- Layers: the user config (`<wt home>/config.toml`) and the worktree config
  (`.wt/config.toml`). A worktree alias replaces a user alias of the same
  name. `[alias]` in a tree's `.wt-tree/config.toml` is not read (and
  `wt config set --tree T alias.x ...` is refused for a nested tree).
- `wt config set [--global] alias.NAME "COMMAND LINE"` adds or replaces an
  alias; an empty value removes it, as does `wt config unset [--global]
  alias.NAME`. With `--global` both work outside a worktree.
- `wt config get alias.NAME` and `wt config list [--show-origin]` show
  aliases like every other key.
- Names: 1-64 characters of `[A-Za-z0-9_-]`, starting with a letter or
  digit. Values are validated when set: they must split into at least one
  word (§3.2).

### 3.2 Expansion

Expansion happens on the raw command line before argument parsing:

1. The command word is the first argument after the leading global options
   (`--json`, `--no-color`, `-C PATH` / `-CPATH`, `--tree NAME` /
   `--tree=NAME`, `--server URL` / `--server=URL`). Help and version flags,
   `--`, any other option, or a non-UTF-8 argument before the command word
   leave the command line unchanged.
2. If the command word is a built-in command, one of its clap aliases, or
   `help`, nothing is expanded: **built-in commands cannot be shadowed**
   (`wt doctor` warns about such aliases).
3. Otherwise, if the word is an alias, it is replaced by the words of its
   value, split shell-style: whitespace separates words, `'...'` quotes
   literally, `"..."` quotes with `\"` and `\\` escapes, and outside quotes
   a backslash escapes whitespace, a quote or a backslash (other
   backslashes are kept, so Windows paths need no doubling). The remaining
   arguments follow the expansion: `wt lg main` runs
   `wt log --oneline -n 20 main`.
4. The expansion may itself start with an alias; steps 1-3 repeat. At most
   **5** expansions happen; a 6th (a chain that is too long, or a cycle such
   as `a = "b"`, `b = "a"`) is a usage error (exit 2) naming the chain.
5. A word that is neither a command nor an alias is passed to clap
   unchanged, which reports it with its usual "a similar subcommand exists"
   suggestion (exit 2).

Aliases are read from the `-C` directory when given (else the current
directory) without opening the repository; unreadable or invalid config
files contribute no aliases. Built-in commands never read alias tables.

Shell aliases (git's `!command`) are not supported (see Roadmap).

## 4. `wt doctor`

`wt doctor` runs the checks below and prints one line per check:

```
✓ worktree            /home/ada/web (tree web, branch main)
✓ config.user         /home/ada/.wt/config.toml
✓ config.worktree     /home/ada/web/.wt/config.toml
! user                user.email not set (snapshot authorship)
                      fix: wt config set --global user.email "you@example.com"
✓ origin: server      http://localhost:7420 is reachable
✓ origin: sync        gRPC Ping ok (server 0.6.0)
✓ origin: version     wt 0.6.0, server 0.6.0
✓ origin: clock       in sync (within 30s)
✓ origin: credential  logged in as ada
✓ origin: token       the session expires in 29 days
✓ daemon              running (pid 4242, version 0.6.0), watching this worktree
- signing             no signing key configured ([signing] key)
- hooks               no hooks

0 errors, 1 warning
```

Marks: `✓` ok, `!` warning, `✗` error, `-` skipped. A check with a known
remedy prints a `fix:` line. The exit code is 1 when any check is an error
(after the report is printed; stderr then says how many failed), else 0.
With `--json` the report is one document
`{"ok","errors","warnings","checks":[{"name","status":"ok|warn|error|skip","message","fix"?}]}`.

| Check | Error | Warning | Notes |
|---|---|---|---|
| `worktree` | `.wt/` exists but does not open (e.g. invalid `config.toml`), or `--tree` names no tree | — | skipped outside a worktree; later worktree checks are omitted |
| `config.user`, `config.worktree`, `config.tree.NAME` | the file does not parse or a value is invalid for that layer | — | each layer is validated exactly as it is read (allowed sections only); the user file is reported even when absent |
| `user` | — | `user.name` / `user.email` empty after layering | |
| `aliases` | an alias cannot be expanded (bad quoting, too deep, cycle) | an alias has a built-in command's name | omitted when no aliases exist |
| `R: server` | `GET /healthz` gets no HTTP answer | non-2xx answer | R = the remote names sharing that server URL (outside a worktree: every server with a saved login); an unreachable server skips its other checks |
| `R: sync` | gRPC `Ping` fails or times out | — | |
| `R: version` | — | `wt` and server differ in **major** version | unversioned server builds are reported, not flagged |
| `R: clock` | skew ≥ 5 min | skew ≥ 30 s | skew = server `Date` header minus the local time halfway through the `/healthz` request |
| `R: credential` | the saved credential (or `WT_TOKEN`) is rejected by `GET /api/v1/auth/me` | not logged in | |
| `R: token` | the current session has expired | it expires within 7 days | from `GET /api/v1/auth/sessions` (`current: true`); API tokens and service-account keys are skipped |
| `daemon` | — | not running while auto-snapshots are enabled for this worktree; stale `bg.json`; version differs from `wt`; not watching this worktree; paused; last snapshot or sync error | IPC `Status` with the token from `bg.json` |
| `signing` | `[signing] key` is set but the key is missing in `<wt home>/keys`; the registration no longer exists or was revoked on its server | the key is not registered | the server part (`GET /api/v1/auth/keys`) runs only online and when logged in to the key's server |
| `hooks` | — | a file in `.wt/hooks/` or `<tree>/.wt-tree/hooks/` that never runs (unknown name, not executable on Unix, a lower-priority variant of the same phase on Windows), an extensionless hook on Windows without `sh` on PATH, hooks present but `[hooks] enabled = false` | `*.sample` and dot files are ignored |

Every network operation is bounded by `--timeout SECS` (default 5).
`--offline` skips every server, credential and clock check (the daemon IPC
on 127.0.0.1 still runs), so the command never waits on the network.

## 5. `wt open`

`wt open` builds the admin-panel URL of the current tree on the server of a
remote (`--remote NAME`, default `origin`) and opens it in a browser:

| Target | URL |
|---|---|
| (none) / `tree` | `<server>/admin/trees/<tenant>/<tree>` |
| `mr N` | `<server>/admin/trees/<tenant>/<tree>/merge-requests/N` |
| `snapshot REV` | `<server>/admin/trees/<tenant>/<tree>/snapshots/<id>` |
| `file PATH` | `<server>/admin/trees/<tenant>/<tree>/files?path=<tree-relative path>&branch=<current branch>` |

- `<server>` is the remote URL (normalized: `http://` added when no scheme
  is given, no trailing slash); `<tenant>` is the remote's tenant and
  `<tree>` the tree's remote name (default: its local name). Tenant, tree
  and snapshot id are percent-encoded path segments; `path` and `branch`
  keep their `/` separators.
- `REV` is any revision (`HEAD`, `HEAD~2`, a branch, a tag, an id prefix)
  and is resolved locally to the full snapshot id; a hex id of at least 8
  characters that is not known locally is used verbatim.
- `PATH` is relative to the current directory and converted to a path
  relative to the tree; a path naming the tree directory opens the Files
  tab at its root.
- Browser: `$BROWSER` when set, else `cmd /C start "" "<url>"` on Windows
  (the URL is quoted so `&` stays literal), `open` on macOS and `xdg-open`
  elsewhere. The launcher is started detached; failing to start it is an
  error whose hint repeats the URL.
- `--print` prints only the URL (scriptable: `curl "$(wt open --print)"`).
  `--json` prints `{"url","opened"}`.
- A missing remote is an error (exit 1) with a hint to add one; outside a
  worktree the command fails with the usual "not a worktree" hint.

## 6. Grouped help

`wt --help` and `wt -h` list the commands grouped by topic (Local history,
Branches and merging, Worktree structure and configuration, Servers and
sync, Export and interop, Background daemon, Collaboration, Administration,
Identity and access, Maintenance and setup), mirroring
`docs/cli-reference.md`. A command not assigned to a group is listed under
"Other commands", so new commands are never hidden. Help of individual
commands (`wt CMD --help`) is unchanged.

## 7. Implementation status

| Feature | Status |
|---|---|
| `wt completions` for bash, zsh, fish, PowerShell, elvish, with configured aliases | Built (v0.6 §14.8) |
| `[alias]` in user and worktree config; `wt config set/unset [--global] alias.NAME` (also outside a worktree with `--global`) | Built (v0.6 §14.8) |
| Alias expansion before parsing: no shadowing of built-ins, 5-level limit, cycle report, clap suggestions kept | Built (v0.6 §14.8) |
| `wt doctor` with every check in §4, `--offline`, `--timeout`, `--json`, exit 1 on errors | Built (v0.6 §14.8) |
| `wt open` for tree, merge request, snapshot and file pages; `--print`, `--remote`, `$BROWSER` | Built (v0.6 §14.8) |
| Grouped root help | Built (v0.6 §14.8) |

## 8. Roadmap

- Shell aliases (`!cmd`) and parameter substitution in aliases.
- Dynamic (runtime) completion of branch, tag, tree and remote names.
- `wt doctor --fix` applying the safe fixes (start/watch the daemon, set
  the executable bit).
- `wt open` for releases, CI checks and staged snapshots, and outside a
  worktree with `TENANT/TREE`.
- Version-skew rules beyond the major version once releases follow a
  shared version scheme.

## 9. References

- `crates/worktree-cli/src/alias.rs` — alias expansion
- `crates/worktree-cli/src/help.rs` — grouped root help
- `crates/worktree-cli/src/main.rs` — expansion and parsing entry point
- `crates/worktree-cli/src/commands/completions.rs`
- `crates/worktree-cli/src/commands/doctor.rs`
- `crates/worktree-cli/src/commands/open.rs`
- `crates/worktree-sdk/src/config.rs` — `[alias]` section, `split_alias_command`,
  `load_aliases`, `validate_config_file`, user-config writes
- `crates/worktree-cli/tests/cli_ux.rs` — end-to-end tests
- `apps/admin/src/app/routes.tsx` — admin-panel routes `wt open` links to
