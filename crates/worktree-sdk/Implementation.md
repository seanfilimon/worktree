# Implementation Details — `worktree-sdk`

The `worktree-sdk` crate is the **local engine** for the Worktree version control system. It is the core library that performs all local repository operations — initializing repos, creating snapshots (commits), managing branches, computing diffs, merging, and syncing. It operates entirely on the local filesystem using a `.wt/` metadata directory (analogous to Git's `.git/`).

---

## Crate Metadata

- **Name:** `worktree-sdk`
- **Edition:** 2021
- **Dependencies:** `worktree-protocol` (sibling), `tokio` 1 (full), `serde` 1 (derive), `serde_json` 1, `toml` 0.8, `thiserror` 1, `tracing` 0.1, `chrono` 0.4 (serde), `uuid` 1 (v4), `blake3` 1, `walkdir` 2
- **Exports:** `WorktreeEngine`, `SdkError`

---

## Architecture Overview

```
src/
├── lib.rs          # Re-exports WorktreeEngine and SdkError
├── error.rs        # SdkError enum (13 variants)
└── engine/
    ├── mod.rs      # WorktreeEngine struct + open/init
    ├── init.rs     # Repository initialization (.wt/ creation)
    ├── status.rs   # State management + working tree status (BLAKE3 diffing)
    ├── snapshot.rs # Snapshot creation (file collection + hashing)
    ├── branch.rs   # Branch CRUD (create/list/switch/delete)
    ├── tree.rs     # Tree CRUD (add/list/remove sub-projects)
    ├── merge.rs    # Branch merging (hash-based conflict detection)
    ├── diff.rs     # Working tree + snapshot-to-snapshot diffs
    ├── tag.rs      # Tag CRUD
    ├── log.rs      # Snapshot log display
    ├── sync.rs     # Push/pull stubs
    ├── config.rs   # Config + ignore file reading
    ├── reflog.rs   # Reflog generation from snapshot history
    ├── dependency.rs # Dependency listing
    └── ignore.rs   # Ignore pattern parsing
```

---

## Core Struct: `WorktreeEngine`

The single entry point for all SDK operations. Holds a `root: PathBuf` pointing to the project root containing `.wt/`.

### Construction

- **`WorktreeEngine::init(path)`** — Creates a new worktree repository. Calls `init::initialize()` to build the `.wt/` directory structure, write default `config.toml`, default `ignore` file, and initial `state.json` with one `root` tree on the `main` branch.

- **`WorktreeEngine::open(path)`** — Opens an existing repository. Resolves relative paths via `std::env::current_dir()`, then walks up parent directories looking for a `.wt/` directory. Returns `SdkError::NotAWorktree` if none found.

### Path Accessors

| Method | Returns |
|--------|---------|
| `root()` | Project root directory |
| `wt_dir()` | `.wt/` metadata directory |
| `state_file()` | `.wt/state.json` |
| `objects_dir()` | `.wt/objects/` |
| `refs_dir()` | `.wt/refs/` |
| `reflog_dir()` | `.wt/reflog/` |

---

## Data Model — `state.json`

The entire repository state lives in a single JSON file (`.wt/state.json`). This is the central source of truth:

```
WorktreeState
├── name: String
├── created_at: String (RFC 3339)
├── current_tree: Option<String>
└── trees: Vec<TreeState>
    ├── name, path, current_branch
    ├── branches: Vec<BranchState>
    │   ├── name: String
    │   ├── tip: Option<String>  (snapshot ID)
    │   └── created_at: String
    ├── snapshots: Vec<SnapshotState>
    │   ├── id (UUID), message, author, timestamp
    │   ├── parents: Vec<String>
    │   ├── tree_name, branch_name
    │   ├── files: Vec<FileEntry> { path, hash (BLAKE3 hex), size }
    │   └── auto_generated: bool
    └── tags: Vec<TagState>
        ├── name, target_snapshot, message, tagger
        └── created_at
```

### State Persistence Pattern

Every mutating operation follows the same pattern:
1. `load_state(engine)` — Read + deserialize `.wt/state.json`
2. Mutate `WorktreeState` in memory
3. `save_state(engine, &state)` — Atomic write via temp file (`.json.tmp`) + `fs::rename()`

The atomic rename prevents corruption if the process crashes mid-write.

---

## Module: `init` — Repository Initialization

`initialize(root)` creates the full `.wt/` directory structure:

**Directories created:**
- `.wt/objects/` — Content-addressable object store (reserved, not yet used)
- `.wt/refs/branches/`, `.wt/refs/tags/` — Reference storage (reserved)
- `.wt/reflog/` — Operation history
- `.wt/identity/` — Authentication credentials
- `.wt/access/` — Access control policies
- `.wt/hooks/` — Pre/post operation hooks
- `.wt/cache/` — Temporary data (PID files, staged snapshots, remotes)
- `.wt/conflicts/` — Conflict resolution workspace

**Files created:**
- `.wt/config.toml` — Default configuration with sections: `[worktree]`, `[sync]`, `[auto_snapshot]`, `[large_files]`, `[reflog]`
- `.wt/ignore` — Default ignore patterns (`.wt/`, `.git/`, `node_modules/`, `target/`, etc.)
- `.wt/state.json` — Initial state with one `root` tree on `main` branch

---

## Module: `status` — State Management & Working Tree Diffing

### State Types

All state types implement `Serialize`/`Deserialize` for JSON persistence:

- `WorktreeState` — Top-level state with `find_tree()`, `find_tree_mut()`, `current_tree()`, `current_tree_mut()`
- `TreeState` — Per-tree state with `current_branch()`, `find_branch()`, `snapshots_on_branch()`
- `BranchState` — Branch name + tip snapshot ID
- `SnapshotState` — Full snapshot data including file entries
- `FileEntry` — File path + BLAKE3 hash + size
- `TagState` — Tag metadata
- `ReflogEntryState` — Reflog entry data

### Working Tree Status (`compute_status`)

The `compute_status(engine)` function performs real-time change detection:

1. Load state, find current tree and branch.
2. Get the last snapshot's file list as a `HashMap<path, hash>`.
3. Walk the working directory recursively using `walkdir`.
4. For each file: compute BLAKE3 hash, compare against known hash.
   - New file (not in known) → **added**
   - Known file with different hash → **modified**
5. Any remaining known files not found on disk → **deleted**

**Ignored directories:** `.wt/`, `.git/`, `node_modules/` are filtered out during the walk using `is_ignored_dir()` and `contains_ignored_dir()` helper functions that handle both forward and backslash path separators (Windows compatibility).

---

## Module: `snapshot` — Snapshot Creation

`create_snapshot(engine, tree_name, message)`:

1. Load state, resolve tree name (falls back to `current_tree`).
2. Collect all files in the tree's directory via `collect_files()`:
   - Uses `walkdir` for recursive traversal
   - Reads each file, computes BLAKE3 hash
   - Skips `.wt/`, `.git/`, `node_modules/`
   - Returns `Vec<FileEntry>` with path, hash, size
3. **No-change detection:** If the previous snapshot exists and has identical file hashes (after sorting both sets), returns `SdkError::NoChanges`.
4. Generate UUID v4 for snapshot ID.
5. Determine author from env vars: `WT_AUTHOR` → `USER` → `USERNAME` → `"unknown"`.
6. Create `SnapshotState` with parents = previous branch tip (if any).
7. Update branch tip to new snapshot ID.
8. Append snapshot to tree's snapshot list.
9. Save state atomically.

---

## Module: `branch` — Branch Management

Four operations, all following the load-mutate-save pattern:

- **`create_branch(engine, name, tree_name)`** — Creates new branch forked from current branch's tip. Rejects duplicate names.
- **`list_branches(engine, tree_name)`** — Returns `(Vec<BranchState>, current_branch_name)`.
- **`switch_branch(engine, name, tree_name)`** — Updates `tree.current_branch`. Rejects nonexistent branches.
- **`delete_branch(engine, name, tree_name)`** — Cannot delete current branch or `main` branch (protected). Uses `retain()` for removal.

---

## Module: `tree` — Tree (Sub-Project) Management

- **`add_tree(engine, path)`** — Validates path (no `..`, no absolute), creates directory + `.wt-tree/config.toml`, adds to state with `main` branch.
- **`list_trees(engine)`** — Returns all `TreeState`s.
- **`remove_tree(engine, name)`** — Cannot remove `root`. Removes `.wt-tree/` directory. If removing current tree, falls back to `root`.

---

## Module: `merge` — Branch Merging

`merge_branch(engine, source_branch)`:

1. Cannot merge branch into itself.
2. Gets latest snapshot's file list from both source and target branches.
3. Builds merged file set using HashMap:
   - Start with target files.
   - For each source file: if path exists in target with different hash → **conflict**.
   - If no conflicts exist, add source-only files.
4. On conflict → returns `SdkError::MergeConflict` listing conflicted paths.
5. On success → creates merge snapshot with two parents (target tip + source tip).
6. Returns `MergeResult { snapshot, files_merged, conflicts }`.

---

## Module: `diff` — Change Detection

Two modes:

- **`diff_working_tree(engine)`** — Wraps `compute_status()`, converts added/modified/deleted lists to `Vec<DiffEntry>`.
- **`diff_snapshots(engine, from_id, to_id)`** — HashMap comparison of two snapshots' file entries. Produces full `DiffEntry` structs with old/new hashes and sizes.

`DiffStatus` enum: `Added`, `Modified`, `Deleted`, `Renamed(String)`.

---

## Module: `tag` — Tag Management

- **`create_tag(engine, name, message, tree_name)`** — Tags current branch tip. Rejects duplicates. Author from env vars.
- **`list_tags(engine, tree_name)`** — Returns all tags in the tree.
- **`delete_tag(engine, name, tree_name)`** — Removes by name, errors if not found.

---

## Module: `sync` — Push/Pull (Stubs)

- **`push(engine)`** — Returns `PushResult` with branch name, snapshot count, and server name. Currently a local-only stub.
- **`pull(engine)`** — Returns `PullResult` with `up_to_date: true`. Placeholder for future server sync.

---

## Module: `config` — Configuration Reading

- **`read_config(engine)`** — Reads `.wt/config.toml` as raw string.
- **`read_ignore(engine)`** — Reads `.wt/ignore`, returns empty string if missing.

---

## Module: `reflog` — Operation History

`show_reflog(engine, count)` generates reflog entries from snapshot history, formatted as:
```
main@{0}: abcd1234 — message (2024-01-01T00:00:00Z)
```

---

## Module: `dependency` / `ignore`

- `list_dependencies(engine)` — Lists non-root trees as dependencies.
- `list_ignored(engine)` — Parses `.wt/ignore`, strips comments and blank lines.

---

## Error Handling

`SdkError` enum with 13 variants:

| Variant | Meaning |
|---------|---------|
| `NotAWorktree` | No `.wt/` directory found |
| `AlreadyInitialized` | `.wt/` already exists |
| `TreeNotFound(name)` | Tree doesn't exist |
| `BranchNotFound(name)` | Branch doesn't exist |
| `SnapshotNotFound(id)` | Snapshot doesn't exist |
| `NoChanges` | Nothing to snapshot |
| `MergeConflict(msg)` | Conflicting file changes |
| `BranchProtection(msg)` | Protected branch violation |
| `PermissionDenied(msg)` | Access denied |
| `InvalidConfig(msg)` | Bad configuration |
| `Io(std::io::Error)` | Filesystem error |
| `Serialization(msg)` | JSON serialization error |
| `TagExists(name)` / `TagNotFound(name)` | Tag conflicts |

---

## Key Design Decisions

1. **Single JSON state file** — All trees, branches, snapshots, and tags in one `state.json`. Simple and atomic but won't scale to very large histories. The `objects/` directory exists but is not yet used.

2. **BLAKE3 for hashing** — Fast cryptographic hash (~3x faster than SHA-256) for content-addressable file identification and change detection.

3. **Atomic saves** — `save_state` writes to `.json.tmp` then renames, preventing corruption on crash.

4. **No traits/interfaces** — Everything is concrete functions on `WorktreeEngine`. No dependency injection or abstraction layers.

5. **Author detection** — Uses `WT_AUTHOR` → `USER` → `USERNAME` env vars, falling back to `"unknown"`.

6. **Cross-platform path handling** — All path comparisons normalize backslashes to forward slashes. Ignore checks handle both separator styles.

---

## TODO

- [ ] Implement actual content-addressable object storage in `.wt/objects/` instead of inline file data in `state.json`
- [ ] Add real server sync protocol integration (push/pull currently return stubs)
- [ ] Implement auto-snapshot engine (detect inactivity, trigger snapshot automatically)
- [ ] Add shallow clone support (partial history loading)
- [ ] Implement large file chunking with lazy blob loading
- [ ] Add file-level locking for concurrent access to `state.json`
- [ ] Implement proper merge conflict resolution (currently fails on any conflict)
- [ ] Wire up `worktree-protocol` types instead of local `SnapshotState`/`BranchState`
- [ ] Add reflog persistence to disk (currently generated on-the-fly from snapshots)
- [ ] Implement ignore pattern glob matching beyond simple directory filtering
- [ ] Add `tokio` async variants of all operations (dependency exists but is unused)
- [ ] Implement snapshot garbage collection for unreachable snapshots
- [ ] Add hooks system (pre-snapshot, post-snapshot, pre-merge, etc.)
- [ ] Implement rename detection in diff (currently only tracks add/modify/delete)
- [ ] Add integrity verification command that re-hashes all files and validates state