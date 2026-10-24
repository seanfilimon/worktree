# worktree-cli — implementation notes

`wt` is a thin client: every command maps onto one or a few `worktree-sdk` calls. The command
surface is fixed by [docs/ENGINEERING_PLAN.md](../../docs/ENGINEERING_PLAN.md) §5; the user-facing
reference is [docs/cli-reference.md](../../docs/cli-reference.md).

## Structure

```
src/
  main.rs        entry point: parse, run, map errors to exit codes, configure colors and logging
  cli.rs         clap derive definition (global flags + Command enum) and parser tests
  context.rs     per-invocation state: -C, --tree, --json; opens the Repository, selects the tree,
                 runs async SDK calls on a Tokio runtime
  paths.rs       user paths (relative to the cwd) -> '/'-separated tree-relative paths
  error.rs       CliError: message, optional hint, exit-code class
  output/        human formatting and the single-document JSON writer
  commands/      one module per command group (init, status, snapshot, history, diff, branch,
                 merge, tag, tree, ignore, config, remote, auth, sync, archive, git, daemon)
```

## Conventions

- **One job per command.** Commands do not chain unrelated side effects; for example `wt push` never
  pulls, and `wt fetch` never touches local branches.
- **Tree selection.** `--tree NAME` wins; otherwise `Repository::tree_for_path(cwd)` picks the deepest
  tree containing the current directory.
- **Output.** Human output goes to stdout, diagnostics and errors to stderr. With `--json` exactly one
  JSON document is written to stdout (serialized SDK types or small command-specific structs).
  Secrets (tokens, passwords) are never printed.
- **Colors** only when stdout is a terminal and neither `--no-color` nor `NO_COLOR` is set.
- **Logging** is off unless `WT_LOG` holds a `tracing` filter directive (e.g. `debug`).
- **Passwords** come from `--password-stdin` (first line), else `WT_PASSWORD`, else a no-echo prompt.
- **Server default.** Commands with an optional `SERVER` use the URL of the worktree's default remote.

## Exit codes

| Code | Source |
|---|---|
| 0 | Success |
| 1 | Any `SdkError` or I/O failure (including rejected pushes and detected secrets) |
| 2 | clap usage errors and invalid argument combinations |
| 3 | `MergeOutcome::Conflicts` from `merge`/`pull`, and `SdkError::MergeConflict` (e.g. `merge --continue` with markers left) |

## Command → SDK mapping

| Command | SDK |
|---|---|
| `init` | `Repository::init` |
| `status` / `status --team` | `Repository::status` / `remote::list_staged` |
| `snapshot` | `Repository::snapshot` |
| `log`, `show` | `Repository::log`, `resolve`, `diff_revs` |
| `diff` | `Repository::diff_working`, `diff_revs` |
| `branch`, `switch` | `branches`, `create_branch`, `archive_branch`, `restore_branch`, `switch_branch` |
| `merge` | `merge`, `continue_merge`, `abort_merge` |
| `revert`, `restore` | `revert`, `restore` |
| `tag` | `tags`, `create_tag`, `delete_tag` |
| `tree` | `trees`, `add_tree`, `remove_tree` |
| `reflog`, `ignore` | `reflog`, `ignore_patterns`, `add_ignore_pattern`, `is_ignored` |
| `config`, `remote` | `WorktreeConfig::{entries, get, set}` + `set_config` |
| `register`, `login`, `logout`, `whoami` | `remote::{register, login, remove_credential, ping}` |
| `push`, `fetch`, `pull`, `clone`, `stage` | `remote::{push, fetch, pull, clone, stage}` |
| `archive` | `resolve` + `manifest_of` + `read_file_at`, written with `tar`/`flate2` or `zip` |
| `git import` / `git export` | `worktree_git::{import_repository, export_repository}` |
| `daemon start` | spawns `worktree-bg run` detached |
| `daemon stop/status/watch/unwatch/pause/resume` | `ipc::send(IpcRequest::…)` |

## Testing

- `cli.rs` contains parser tests for every command and flag combination.
- Command tests run the built binary (`assert_cmd`) against `tempfile` worktrees, with `WT_HOME`
  set to a temporary directory.
- Server-backed flows are covered by `crates/worktree-e2e`.
