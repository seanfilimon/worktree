# Implementation Details — `worktree-cli`

The `wt` binary: a thin layer over `worktree-sdk` (`Repository`, `remote`,
`ipc`) and `worktree-git::bridge`. The command surface is specified in
`docs/ENGINEERING_PLAN.md` §5.

## Layout

```
src/
├── main.rs          parse → apply -C → run → exit code
├── cli.rs           clap derive: global flags + Command enum (parsing tests)
├── context.rs       cwd, repository opening, tree selection, block_on
├── error.rs         CliError: message + hint + exit class (1/2/3)
├── paths.rs         cwd-relative user paths → '/'-separated SDK paths
├── output/
│   ├── mod.rs       Output::emit — JSON document or human Text
│   └── format.rs    short ids, relative times, diffstat bars
└── commands/        one module per command group (args + implementation)
    init status snapshot history(log/show/reflog) diff branch(+switch)
    merge(+revert/restore) tag tree ignore config remote auth sync
    archive git daemon
```

## Conventions

* **Output.** Every command builds one result value and calls
  `ctx.out.emit(&value, |text| ...)`. With `--json` the value is printed as
  a single pretty JSON document (SDK structs are serialized as-is; small
  CLI-local structs cover unit results). Otherwise the closure renders
  colored, aligned text. Colors are off for `--no-color`, `NO_COLOR`, or a
  non-terminal stdout. A closed stdout pipe is not an error.
* **Errors** go to stderr as `error: ...` plus an optional `hint: ...`.
  Exit codes: 0 ok, 1 failure, 2 usage (clap and CLI-detected), 3 merge
  conflicts (`wt merge`/`wt pull` outcomes and `SdkError::MergeConflict`).
* **Tree selection:** `--tree NAME`, else `Repository::tree_for_path(cwd)`.
* **Async** SDK calls (remote, ipc) run via `context::block_on` on a small
  multi-threaded tokio runtime created only for those commands.
* **Passwords:** `--password-stdin` (first stdin line), else `WT_PASSWORD`,
  else a no-echo prompt (`rpassword`). Tokens are never printed.
* **Daemon:** `wt daemon start` locates `worktree-bg` next to `wt`, then on
  `PATH`, spawns `worktree-bg run` detached (cwd `$WT_HOME`) and waits for
  `bg.json` plus a successful `Status` reply; `--foreground` runs
  `worktree-bg run --foreground` attached.
* **`wt diff` positionals:** leading arguments that resolve as revisions
  (at most two) are revisions, the rest must be existing paths; `--`
  introduces paths unconditionally.

## Tests

* Unit tests (`cargo test -p worktree-cli`) cover parsing of every
  command, error mapping, formatting, path conversion, archive writers,
  daemon binary lookup and renderers.
* `tests/cli.rs` drives the real binary with `assert_cmd` in temporary
  directories with a private `WT_HOME`. Tests that need SDK pieces owned by
  other workstreams are `#[ignore = "integration: needs <workstream>"]`;
  run them with `cargo test -p worktree-cli -- --include-ignored`.
