# worktree-git — implementation notes

Git ⇄ W0rkTree conversion over [`worktree-sdk`](../worktree-sdk/README.md) and libgit2 (`git2`).
It backs `wt git import` and `wt git export`. User-facing semantics and limits are in
[docs/git-compatibility.md](../../docs/git-compatibility.md).

## Public API (`bridge.rs`)

```rust
pub fn import_repository(git_path: &Path, dest: &Path, opts: &ImportOptions) -> Result<ImportReport>;
pub fn export_repository(repo: &Repository, git_path: &Path, opts: &ExportOptions) -> Result<ExportReport>;
```

| Type | Fields |
|---|---|
| `ImportOptions` | `branches` (default: all local branches), `name` (default: destination dir name), `include_tags` |
| `ImportReport` | `dest`, `branches`, `snapshots`, `blobs`, `tags`, `checked_out` |
| `ExportOptions` | `tree` (default: root tree), `branches` (default: all), `include_tags`, `bare` |
| `ExportReport` | `git_path`, `branches`, `commits` (newly written), `tags` |

## Modules

| Module | Responsibility |
|---|---|
| `bridge` | The two entry points above and their option/report types |
| `import` | Git → W0rkTree: commit walk, tree → manifest conversion, blob storage, tags, `.gitignore` |
| `export` | W0rkTree → Git: manifest → Git tree, snapshot → commit, refs, optional working-tree update |
| `map` | `.wt/git-map.json`: `{ "version": 1, "snapshots": { "<snapshot id>": "<commit sha>" } }`, keys sorted |
| `ident` | Deterministic snapshot ids (UUID v5 of the commit SHA in a fixed namespace) and author ⇄ signature conversion |
| `hash_index` | BLAKE3 ⇄ Git SHA-1 blob index so export writes each blob once |
| `config` | `.gitignore` → `.wt/ignore` conversion, `.gitattributes` parsing |
| `remote` | Helpers to push an exported repository to a URL (not exposed by the CLI) |
| `error` | `GitCompatError`, `Result<T>` |

## Import algorithm

1. Open the Git repository; `Repository::init` the destination (name from `ImportOptions::name`).
2. Walk commits of the selected branches topologically, parents first. Each commit is converted once,
   even if reachable from several branches; it is attributed to the first branch containing it (the
   checked-out branch first, then the others by name).
3. Convert the commit tree recursively into a sorted `Manifest`: regular files (`100644`) and
   executables (`100755`) become entries; symlinks, submodule gitlinks and non-UTF-8 names are skipped
   with a warning. Each distinct blob is stored once in the object store.
4. Write a `Snapshot` with id `ident::snapshot_id_for_commit(sha)` (UUID v5), parents mapped through
   the same function, author `"Name <email>"`, the author timestamp in UTC and the message with
   trailing whitespace trimmed.
5. Set branch refs to the converted tips, convert tags (annotated tag messages are kept), convert
   `.gitignore` into `.wt/ignore`, record every commit in `git-map.json`, and check out the Git
   repository's current branch.

## Export algorithm

1. Open or create the target repository (bare if requested).
2. For each selected branch, walk `ancestry(tip)` parents-first. A snapshot already in
   `git-map.json` whose commit exists in the target is reused.
3. Otherwise build the Git tree from the manifest (blobs via `hash_index`), and the commit from the
   snapshot only: author = committer = parsed snapshot author (fallback name/email when missing),
   time = snapshot `created_at` in UTC (offset 0), message trimmed and terminated by one newline.
   This makes commits — and therefore SHAs — a pure function of the snapshot.
4. Update `refs/heads/*` and `refs/tags/*` in the target, overwriting existing refs (W0rkTree is the
   source of truth). In a non-bare target, update the working files to `HEAD` only if they have no
   local modifications.
5. Save the updated `git-map.json`.

## Invariants

- Re-importing the same Git history produces identical snapshot ids.
- Exporting the same snapshots produces identical commit SHAs; repeated exports are incremental.
- Import never modifies the source Git repository. Export never modifies the worktree except
  `.wt/git-map.json`.

## Testing

Tests build small Git repositories with `git2` in temporary directories (no `git` binary needed) and
check round trips, determinism, executable bits, skipped entries, tags and incremental export.
