# Implementation Details — `worktree-git`

The `worktree-git` crate is the **bidirectional Git compatibility layer** for the Worktree version control system. It bridges the gap between Git repositories (SHA-1, commits, trees, blobs) and Worktree's native object model (BLAKE3, snapshots, manifests, blobs). It handles import, export, remote operations, and hash translation.

---

## Crate Metadata

- **Name:** `worktree-git`
- **Edition:** 2021
- **Dependencies:** `worktree-protocol` (sibling), `git2` 0.19 (libgit2 bindings), `serde` 1 (derive), `thiserror` 1, `chrono` 0.4 (serde), `tracing` 0.1, `uuid` 1 (v4)
- **Dev dependencies:** `serde_json` 1

---

## Architecture

```
src/
├── lib.rs              # Declares 6 public modules
├── error.rs            # GitCompatError (8 variants) + Result alias
├── config/
│   ├── mod.rs
│   ├── gitattributes.rs # .gitattributes parser
│   └── gitignore.rs     # .gitignore ↔ .worktreeignore converter
├── hash_index/
│   ├── mod.rs
│   └── store.rs         # InMemoryHashIndex (SHA-1 ↔ BLAKE3 bidirectional)
├── import/
│   ├── mod.rs
│   ├── repo.rs          # GitRepo wrapper around git2::Repository
│   ├── walker.rs        # CommitWalker (topological commit traversal)
│   ├── converter.rs     # GitToWorktreeConverter (Git → Worktree objects)
│   └── submodule.rs     # SubmoduleImporter (discover + extract)
├── export/
│   ├── mod.rs
│   ├── builder.rs       # GitRepoBuilder (initialize new Git repos)
│   ├── converter.rs     # WorktreeToGitConverter (Worktree → Git objects)
│   ├── squash.rs        # Snapshot squashing for clean Git history
│   └── submodule.rs     # .gitmodules generation from Worktree trees
└── remote/
    ├── mod.rs
    ├── transport.rs     # GitTransport (HTTPS/SSH detection)
    ├── auth.rs          # GitAuth (SSH key + credential helper)
    ├── push.rs          # GitPush (push to remote)
    ├── pull.rs          # GitPull (fetch + merge from remote)
    └── mirror.rs        # Mirror (continuous bidirectional sync)
```

---

## Error Handling

`GitCompatError` has 8 variants with `#[from]` auto-conversions for `git2::Error` and `std::io::Error`:

| Variant | Source |
|---------|--------|
| `GitError` | `git2::Error` (auto-conversion) |
| `ProtocolError` | Worktree protocol layer |
| `IoError` | `std::io::Error` (auto-conversion) |
| `HashIndexError` | BLAKE3 ↔ SHA-1 mapping |
| `ImportError` | Git → Worktree conversion |
| `ExportError` | Worktree → Git conversion |
| `RemoteError` | Network/remote operations |
| `ConfigError` | Configuration parsing |

---

## `config::gitattributes` — .gitattributes Parser

**Fully implemented** with comprehensive tests.

`GitAttribute` struct holds a glob `pattern` and `Vec<String>` attribute tokens. `parse_gitattributes(content)` parses line-by-line:
- Skips blank lines and `#` comments
- First whitespace-delimited token = pattern
- Remaining tokens = attributes (e.g., `text`, `-diff`, `eol=lf`)
- Pattern-only lines are valid (resets attributes to unspecified)

Tests cover: empty content, comments, single/multiple rules, pattern-only lines, negated attributes, serde roundtrips.

---

## `config::gitignore` — Ignore Format Conversion

Two identity functions for `.gitignore` ↔ `.worktreeignore` conversion:
- `gitignore_to_worktreeignore(content)` — pass-through (formats currently identical)
- `worktreeignore_to_gitignore(content)` — pass-through

Future versions may diverge as Worktree adds extended pattern syntax.

---

## `hash_index::store` — SHA-1 ↔ BLAKE3 Bidirectional Index

**Fully implemented** with tests.

`InMemoryHashIndex` uses dual `HashMap`s for O(1) lookup in both directions:
- `git_to_content: HashMap<GitHash, ContentHash>`
- `content_to_git: HashMap<ContentHash, GitHash>`

Implements the `HashIndex` trait from `worktree-protocol::compat::git_hash_map`:
- `insert(HashMapping)` — Inserts in both maps.
- `lookup_by_git(&GitHash) -> Option<ContentHash>`
- `lookup_by_content(&ContentHash) -> Option<GitHash>`

Also provides `len()`, `is_empty()`.

Tests: empty index, insert + round-trip lookup, missing → None.

---

## `import::repo` — Git Repository Wrapper

**Fully implemented.**

`GitRepo` wraps `git2::Repository` with convenience methods:
- `open(path)` — Opens bare or working directory repos.
- `inner()` — Access raw `git2::Repository`.
- `branches()` — Lists all local branch names.
- `head_branch()` — Current HEAD branch name (errors on detached/unborn).
- `commit_count()` — Total commits reachable from HEAD via topological revwalk.

---

## `import::walker` — Commit Graph Walker

**Fully implemented** with `Iterator` + `ExactSizeIterator`.

`CommitWalker` collects all commit OIDs from a `git2::Revwalk` into a `Vec`, reverses them (oldest-first, parents-before-children), and iterates sequentially:

- `new(repo)` — Walk from HEAD.
- `from_branch(repo, branch_name)` — Walk from a specific branch.
- Uses `TOPOLOGICAL | TIME` sort ordering.
- Reversing ensures parent snapshots are created before their children during import.
- `total_commits()`, `remaining()`, `reset()` for progress tracking.

---

## `import::converter` — Git → Worktree Object Converter

**API designed, bodies are `todo!()` stubs.**

`GitToWorktreeConverter<'repo>` holds a `&GitRepo` reference and a `TreeId`:
- `convert_commit(&git2::Commit) -> Result<Snapshot>` — Map author, message, parents, tree hash.
- `convert_tree(&git2::Tree) -> Result<Manifest>` — Iterate entries, map kinds and hashes.
- `convert_blob(&git2::Blob) -> Result<Blob>` — Copy content, compute BLAKE3 hash.

---

## `import::submodule` — Submodule Discovery

**Fully implemented.**

`SubmoduleImporter::import_submodules(repo)` uses `git2::Repository::submodules()` to enumerate all registered submodules. Returns `Vec<SubmoduleInfo>` with name, relative path, and remote URL.

---

## `export::builder` — Git Repository Builder

**Fully implemented.**

`GitRepoBuilder` uses the builder pattern:
- `with_initial_branch(name)` — Default: `"main"`.
- `bare(bool)` — Create bare repo.
- `build(output_path)` — Calls `git2::Repository::init()` or `init_bare()`, then sets HEAD to the configured branch.

---

## `export::converter` — Worktree → Git Object Converter

**API designed, bodies are `todo!()` stubs.**

`WorktreeToGitConverter` wraps a `git2::Repository`:
- `convert_snapshot(&Snapshot) -> Result<git2::Oid>` — Create Git commit.
- `convert_manifest(&Manifest) -> Result<git2::Oid>` — Create Git tree.
- `convert_blob(&Blob) -> Result<git2::Oid>` — Create Git blob.

---

## `export::squash` — Snapshot Squashing

**Types implemented, function is `todo!()`.**

`SquashMode` enum:
- `None` — Every snapshot → one Git commit.
- `AutoOnly` — Squash auto-generated snapshots into nearest manual snapshot.
- `All` — Squash all consecutive snapshots into one commit.

`SquashOptions` combines mode + `keep_last` count (most recent N snapshots stay unsquashed).

`squash_snapshots(snapshots, options) -> Vec<Snapshot>` — Not yet implemented.

---

## `export::submodule` — .gitmodules Generation

**Stub.** `generate_gitmodules(trees)` will generate `.gitmodules` INI content from Worktree `Tree` objects.

---

## `remote::transport` — Transport Protocol Detection

**Fully implemented** with tests.

`GitTransport` enum:
- `Https(String)` — URLs starting with `https://` or `http://`
- `Ssh(String)` — URLs starting with `ssh://` or `git@`

`from_url(url)` auto-detects transport type. Returns `None` for unrecognized schemes.

---

## `remote::auth` — Authentication

**Fully implemented.**

`GitAuth` supports two strategies via private `AuthMethod` enum:

1. **SSH Key:** Validates file existence at construction. `callbacks()` produces `git2::RemoteCallbacks` using `git2::Cred::ssh_key()`.

2. **Credential Helper:** Uses system Git credential manager. `callbacks()` tries `git2::Cred::credential_helper()` for plaintext credentials, falls back to `git2::Cred::default()`.

---

## `remote::push` / `remote::pull` — Push/Pull Operations

**Stubs.** `GitPush` and `GitPull` have builder-style constructors with optional auth. `push()`, `push_all()`, `pull()`, `fetch_only()` are all `todo!()`.

---

## `remote::mirror` — Continuous Sync

**Config implemented, loop is `todo!()`.**

`MirrorConfig` is serializable: `remote_url`, `branch`, `interval_secs`.

`Mirror::start(config)` will eventually run a blocking sync loop that periodically pushes and pulls changes.

---

## Implementation Maturity Summary

| Component | Status |
|-----------|--------|
| `error` | ✅ Complete |
| `config::gitattributes` | ✅ Complete (with tests) |
| `config::gitignore` | ✅ Complete (pass-through) |
| `hash_index::store` | ✅ Complete (with tests) |
| `import::repo` | ✅ Complete |
| `import::walker` | ✅ Complete |
| `import::converter` | 🔶 API only (todo!) |
| `import::submodule` | ✅ Complete |
| `export::builder` | ✅ Complete |
| `export::converter` | 🔶 API only (todo!) |
| `export::squash` | 🔶 Types complete (todo!) |
| `export::submodule` | 🔶 Stub (todo!) |
| `remote::transport` | ✅ Complete (with tests) |
| `remote::auth` | ✅ Complete |
| `remote::push` | 🔶 Stub (todo!) |
| `remote::pull` | 🔶 Stub (todo!) |
| `remote::mirror` | 🔶 Config complete (todo!) |

---

## TODO

- [ ] Implement `GitToWorktreeConverter::convert_commit` — map Git commit fields to Worktree `Snapshot`
- [ ] Implement `GitToWorktreeConverter::convert_tree` — map Git tree entries to Worktree `Manifest`
- [ ] Implement `GitToWorktreeConverter::convert_blob` — copy content + compute BLAKE3 hash
- [ ] Implement `WorktreeToGitConverter::convert_snapshot` — create Git commit from Worktree snapshot
- [ ] Implement `WorktreeToGitConverter::convert_manifest` — create Git tree from Worktree manifest
- [ ] Implement `WorktreeToGitConverter::convert_blob` — create Git blob from Worktree blob
- [ ] Implement `squash_snapshots` — apply squash options to collapse snapshot history
- [ ] Implement `generate_gitmodules` — create .gitmodules from Worktree trees
- [ ] Implement `GitPush::push` — open repo, resolve remote, push refspec with auth
- [ ] Implement `GitPush::push_all` — enumerate branches and push each
- [ ] Implement `GitPull::pull` — fetch + fast-forward/merge
- [ ] Implement `GitPull::fetch_only` — retrieve refs without merging
- [ ] Implement `Mirror::start` — periodic push/pull sync loop
- [ ] Add persistent `HashIndex` backed by SQLite or memory-mapped file for large repos
- [ ] Handle passphrase-protected SSH keys in `GitAuth`
- [ ] Add progress callbacks for long-running import/export operations
- [ ] Implement incremental import (only new commits since last import)
- [ ] Add tests for `GitRepo`, `CommitWalker`, and `GitRepoBuilder` against real Git repos
- [ ] Support Git LFS pointers during import (convert to Worktree large file stubs)