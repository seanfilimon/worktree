# Implementation Notes — `worktree-cli`

The `wt` binary — the user-facing command surface. It contains **no VCS
logic**: every command talks to `worktree_sdk::Client`, which dispatches to
the `worktree-bg` daemon over IPC when one is running and falls back to the
embedded engine otherwise (the CLI never knows which).

## Architecture

```
src/
├── main.rs             # clap parser + tokio entry + error printing
├── commands/
│   ├── mod.rs          # Commands enum (20 subcommands) + execute() dispatcher
│   └── <one file per command>
└── output/
    └── format.rs       # print_header/success/error/info/warning/kv/list, styled_hash
```

Every command module follows the same shape: parse args (done by clap),
`Client::open_current()` (or `Client::init` for `wt init`), call the
matching `Client` method, format the result via `output::format`.

## Honest status (verified against code, WT-PHASE-1)

- **Working end-to-end (local)**: init, status, snapshot, log, branch
  (create/list/switch/delete), merge, diff, tag, tree, config, reflog,
  ignore, depend (config-file level), revert (as counter-snapshot),
  permission (writes `.wt/access/policies.toml`).
- **Placeholders pending later phases**: `sync`/`staged` (server lands in
  WT-PHASE-4/5), `git` subcommands (converters land in WT-PHASE-8),
  `server` (PID-file stub until daemon IPC in WT-PHASE-3), `archive`
  (writes a manifest, not a real tar/zip yet — WT-PHASE-10).
