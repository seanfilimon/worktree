# Implementation Details — `worktree-cli`

The `worktree-cli` crate provides the **command-line interface** for the Worktree version control system. The binary is named `wt` and serves as the primary user-facing tool. It delegates all core operations to the `worktree-sdk` crate and uses `worktree-protocol` for type definitions.

---

## Crate Metadata

- **Name:** `worktree-cli`
- **Binary:** `wt`
- **Edition:** 2021
- **Dependencies:** `worktree-sdk` (sibling), `worktree-protocol` (sibling), `clap` 4 (derive), `tokio` 1 (full), `colored` 2, `tracing` 0.1, `tracing-subscriber` 0.3, `chrono` 0.4, `toml` 0.8, `walkdir` 2

---

## Architecture

```
src/
├── main.rs             # Entry point: Cli struct + tokio::main
├── commands/
│   ├── mod.rs          # Commands enum (20 subcommands) + execute() dispatcher
│   ├── init.rs         # wt init [path]
│   ├── status.rs       # wt status [--team]
│   ├── snapshot.rs     # wt snapshot -m "message" [-t tree]
│   ├── log.rs          # wt log [-n count]
│   ├── branch.rs       # wt branch {create|list|switch|delete}
│   ├── merge.rs        # wt merge <branch> [--strategy]
│   ├── sync.rs         # wt sync {push|pull|pause|resume}
│   ├── tree.rs         # wt tree {add|list|remove|status}
│   ├── diff.rs         # wt diff [from] [to] [--name-only] [--stat]
│   ├── tag.rs          # wt tag {create|list|delete}
│   ├── config.rs       # wt config {show|get|set}
│   ├── reflog.rs       # wt reflog [-n count]
│   ├── revert.rs       # wt revert <snapshot>
│   ├── archive.rs      # wt archive <output> [--format] [--tree]
│   ├── depend.rs       # wt depend {add|list|todo}
│   ├── staged.rs       # wt staged [list|clear]
│   ├── ignore.rs       # wt ignore {list|add}
│   ├── permission.rs   # wt permission {set|get|list}
│   ├── git.rs          # wt git {import|export|clone|remote|push|pull|mirror}
│   └── server.rs       # wt server {start|stop|status}
└── output/
    ├── mod.rs
    ├── format.rs       # 8 print helper functions (colored output)
    └── color.rs        # Theme struct + 12 color constants
```

---

## Entry Point (`main.rs`)

1. Initializes `tracing_subscriber::fmt` for structured logging.
2. Parses CLI arguments via `Cli::parse()` (clap 4 derive).
3. Dispatches to `commands::execute(cli.command).await`.
4. On error, prints via `output::format::print_error()` and exits with code 1.

The `Cli` struct has a single field: `#[command(subcommand)] command: Commands`.

---

## Command Dispatch (`commands/mod.rs`)

The `Commands` enum defines **20 top-level subcommands** with **10 nested sub-enums** for multi-level commands:

### Top-Level Commands

| Command | Sub-actions | Description |
|---------|-------------|-------------|
| `Init` | — | Initialize a new worktree |
| `Status` | `--team` flag | Show working tree status |
| `Snapshot` | `-m`, `-t` | Create a snapshot |
| `Log` | `-n count` | Show snapshot history |
| `Branch` | `Create/List/Switch/Delete` | Branch management |
| `Merge` | `--strategy` | Merge branches |
| `Sync` | `Push/Pull/Pause/Resume` | Remote synchronization |
| `Tree` | `Add/List/Remove/Status` | Sub-project management |
| `Diff` | `--name-only`, `--stat` | Show differences |
| `Tag` | `Create/List/Delete` | Tag management |
| `Config` | `Show/Get/Set` | Configuration management |
| `Reflog` | `-n count` | Operation history |
| `Revert` | — | Revert a snapshot |
| `Archive` | `--format`, `--tree` | Create archive |
| `Depend` | `Add/List/Todo` | Dependency management |
| `Staged` | `List/Clear` | Team activity |
| `Ignore` | `List/Add` | Ignore patterns |
| `Permission` | `Set/Get/List` | Access control |
| `Git` | `Import/Export/Clone/Remote/Push/Pull/Mirror` | Git interop |
| `Server` | `Start/Stop/Status` | Background process |

The `execute()` function pattern-matches on `Commands` and delegates to the appropriate module's `execute()` function.

---

## Output System (`output/`)

### `format.rs` — Colored Print Helpers

| Function | Visual | Purpose |
|----------|--------|---------|
| `print_header(text)` | **bold underline** | Section titles |
| `print_success(text)` | ✔ green | Success messages |
| `print_error(text)` | ✖ red | Error messages |
| `print_info(text)` | ℹ cyan | Informational |
| `print_warning(text)` | ⚠ yellow | Warnings |
| `print_kv(key, value)` | `  key: value` | Key-value pairs (dimmed key) |
| `print_list_item(text)` | `  • text` | Bulleted list items |
| `styled_hash(hash)` | yellow bold | Snapshot ID formatting |

### `color.rs` — Theme System

`Theme` struct defines 12 semantic colors: success, error, warning, info, header, muted, accent, branch, hash, added, removed, modified. Currently `format.rs` hardcodes colors directly — the `Theme` struct is infrastructure for future theming.

---

## Command Implementations

### `init.rs` — Repository Initialization

Calls `WorktreeEngine::init(path)` with the provided path (defaults to `.`). Displays location, default branch (`main`), and default tree (`root`).

### `status.rs` — Working Tree Status

Calls `worktree_sdk::engine::status::compute_status()`. Displays tree name, branch, snapshot count. Shows changes categorized as:
- `+ path` — added (green)
- `~ path` — modified (yellow)  
- `- path` — deleted (red)

With `--team` flag, shows staged snapshots from other team members.

### `snapshot.rs` — Snapshot Creation

Calls `worktree_sdk::engine::snapshot::create_snapshot()`. Displays: short ID (first 8 chars), message, author, timestamp, tree, branch, file count, parent IDs.

### `branch.rs` — Branch Management

- **Create:** Forks from current tip, shows base snapshot.
- **List:** Shows all branches with `*` marking current. Current branch displayed in green bold.
- **Switch:** Updates current branch pointer.
- **Delete:** Removes branch (SDK enforces protection).

### `log.rs` — Snapshot History

Displays git-log-style output: yellow short IDs, auto-generated markers, author, date, branch, tree, parents, file count. Defaults to 20 entries.

### `diff.rs` — Difference Display

Four modes based on `(from, to)` argument combinations:
- `(None, None)` or `("working", None)` → working tree diff
- `(Some(from), Some(to))` → snapshot-to-snapshot diff
- Other combinations → fall back to working tree diff

Three output modes:
- **Default:** Status prefix + path + hash details
- **`--name-only`:** Just prefix + path
- **`--stat`:** Prefix + path + byte-level size delta

Status prefixes: `+` (added), `~` (modified), `-` (deleted), `→` (renamed).

### `merge.rs` — Branch Merging

Calls `worktree_sdk::engine::merge::merge_branch()`. Shows resulting snapshot ID, files merged count, and any conflict list.

### `sync.rs` — Synchronization

- **Push/Pull:** Delegate to SDK, show results.
- **Pause:** Creates `.wt/cache/sync_paused` sentinel file.
- **Resume:** Removes the sentinel file.

### `config.rs` — Configuration Management

The most complex standalone module with three TOML helper functions:

- **`resolve_toml_key(table, key)`** — Traverses nested TOML tables by dotted key (e.g., `sync.auto`).
- **`set_toml_key(table, key, value)`** — Type-preserving insertion. If a key already exists as `Boolean`, `"yes"` → `true`. Creates intermediate tables if needed.
- **`format_toml_value(value)`** — Recursive TOML value → display string conversion.

### `permission.rs` — Access Control

- **Set:** Writes TOML policy entries to `.wt/access/policies.toml` with subjects like `tenant:<name>` or `account:<name>`.
- **Get:** Parses policies.toml line-by-line to filter by tree.
- **List:** Dumps all policies + custom roles.

### `git.rs` — Git Interoperability

The largest command module. Most operations are **scaffolding stubs** that print progress messages:

- **Import/Export/Clone/Push/Pull:** Initialize worktree, print conversion messages, note that full functionality requires `worktree-git` crate.
- **Remote:** Stores URLs as individual `.url` files in `.wt/cache/remotes/`.
- **Mirror:** Stores configs as `.toml` files in `.wt/cache/mirrors/`.

### `server.rs` — Background Process

Manages via PID file (`.wt/cache/bgprocess.pid`):
- **Start:** Writes current PID, warns if already running.
- **Stop:** Removes PID file.
- **Status:** Reads PID file, shows auto-sync state and snapshot counts.

### `archive.rs` — Archive Creation

Currently writes a **manifest file** (not actual tar.gz/zip) listing files from the latest snapshot. Uses `walkdir` to count files when no snapshot exists.

### `depend.rs` — Dependency Management

- **Add:** Appends `[[dependencies]]` TOML entries to `.wt-tree/config.toml`.
- **List:** Combines SDK dependencies with per-tree config parsing.
- **Todo:** Scans for `required = true` blocking dependencies.

---

## Execution Pattern

Every command module follows the same pattern:

1. `pub async fn execute(args...) -> Result<(), Box<dyn std::error::Error>>`
2. Open or init `WorktreeEngine` from current directory (`.`)
3. Call SDK functions for business logic
4. Use `output::format::*` functions for colorized terminal output
5. Return `Ok(())` or propagate errors

---

## File System Layout (`.wt/` directory)

Based on what the CLI reads/writes:

| Path | Purpose |
|------|---------|
| `.wt/config.toml` | Repository configuration |
| `.wt/ignore` | Ignore patterns |
| `.wt/state.json` | Full repository state |
| `.wt/access/policies.toml` | Permission policies |
| `.wt/access/roles.toml` | Custom roles |
| `.wt/cache/sync_paused` | Sync pause sentinel |
| `.wt/cache/bgprocess.pid` | Background process PID |
| `.wt/cache/staged/` | Staged snapshot data |
| `.wt/cache/staged_index.json` | Staged snapshot index |
| `.wt/cache/remotes/<name>.url` | Git remote URLs |
| `.wt/cache/mirrors/<tree>.toml` | Git mirror configs |
| `<tree>/.wt-tree/config.toml` | Per-tree config + dependencies |

---

## TODO

- [ ] Implement actual archive creation (tar.gz/zip) instead of manifest files
- [ ] Complete git import/export/push/pull with full `worktree-git` integration
- [ ] Add interactive conflict resolution UI for merges
- [ ] Implement `wt blame` command for per-line attribution
- [ ] Add `wt cherry-pick` for selective snapshot application
- [ ] Implement `wt stash` for temporary change storage
- [ ] Add progress bars for long-running operations (snapshot, sync, archive)
- [ ] Implement shell completions generation (bash, zsh, fish, PowerShell)
- [ ] Add `--json` output flag for machine-readable output
- [ ] Implement `wt bisect` for binary search through snapshot history
- [ ] Add colored diff output with `+`/`-` line prefixes for file content
- [ ] Use the `Theme` system from `color.rs` instead of hardcoded colors
- [ ] Implement `wt remote` as a top-level command (currently nested under `wt git`)
- [ ] Add `wt clean` command for removing untracked files
- [ ] Implement `wt graph` for visual branch/snapshot DAG display