# `.wt/` Worktree Metadata Directory Specification

> **Version:** 0.5 — describes the directory as built. The authoritative layout is the doc comment in `crates/worktree-sdk/src/repo/mod.rs`; v0.5 additions follow `docs/ENGINEERING_PLAN.md` §13.

## Overview

`.wt/` sits at the root of every worktree and marks its boundary: commands
find the worktree by walking up from the current directory to the nearest
`.wt/`. Unlike earlier drafts of this spec, `.wt/` holds **all** repository
data — configuration, the object store, snapshot records and per-tree state.
There is no external, platform-native data directory; per-user state lives
in `$WT_HOME` (see [§9](#9-per-user-state-wt_home)).

Nothing under `.wt/` is ever part of a snapshot (manifest paths containing a
`.wt` component are rejected), so its contents are local to this worktree and
are not shared by `wt push` / `wt clone`.

---

## 1. Layout

```
.wt/
├── config.toml                          WorktreeConfig (§2)
├── ignore                               worktree-wide ignore patterns (§3)
├── objects/<hh>/<62 hex>                content-addressed objects (§4)
├── snapshots/<id>.json                  snapshot records (§5)
├── trees/<tree>/                        per-tree state (§6)
│   ├── tree.toml                        TreeRecord: name, id, path, default_branch, remote_name
│   ├── HEAD                             current branch name
│   ├── refs/branches/<branch>           tip snapshot id (empty = no snapshots yet)
│   ├── refs/branches.meta.json          created_at / archived per branch
│   ├── refs/remotes/<remote>/<branch>   last fetched server tip
│   ├── refs/tags/<tag>.json             TagInfo
│   ├── index.json                       stat cache: path -> (size, mtime_ns, hash)
│   ├── reflog.jsonl                     ReflogEntry per line, append-only
│   ├── MERGE_STATE.json                 present while a merge awaits resolution
│   ├── LOCK                             OS file lock held by mutating operations
│   ├── shallow                          shallow-boundary snapshot ids (shallow clones)
│   └── stubbed                          paths withheld by the server (not checked out)
├── conflicts/<tree>.json                conflict list of the merge in progress (§7)
├── hooks/<phase>                        worktree-wide snapshot hooks (§8)
├── access/roles.toml                    declarative custom roles (tenant scope)
├── access/policies.toml                 declarative policies (tenant scope)
└── git-map.json                         snapshot id <-> Git commit sha (only after wt git import/export)
```

| Path | Written by | Notes |
|---|---|---|
| `config.toml` | `wt init`, `wt config set`, `wt remote add/remove`, `wt git remote` | Hand-editable; unknown keys preserved |
| `ignore` | `wt init` (header), `wt ignore add`, user | Gitignore syntax |
| `objects/`, `snapshots/` | SDK (snapshot, merge, fetch, clone, import) | Immutable files |
| `trees/` | SDK | Do not edit by hand |
| `conflicts/` | `wt merge`, removed when the merge finishes or aborts | |
| `hooks/` | User | Executables |
| `access/` | User, `wt access pull` | Applied with `wt access apply` |
| `git-map.json` | `worktree-git` | |

---

## 2. `config.toml`

`config.toml` deserializes into `worktree_sdk::config::WorktreeConfig`.
Sections and keys:

```toml
[worktree]
name = "my-project"                 # worktree / root tree name
id = "3f6c1d2a-8f0e-4c55-9d7b-2a1e5b9c0d44"   # UUID, read-only
created_at = "2026-10-25T09:00:00+00:00"      # RFC 3339, read-only
default_branch = "main"             # default branch for new trees (default "main")

[user]
name = "Ada Lovelace"               # snapshot author "Name <email>"; empty → OS user name
email = "ada@example.com"

[auto_snapshot]                     # used by the daemon
enabled = true                      # default true
inactivity_secs = 30                # quiet period before a snapshot (alias: inactivity_timeout_secs)
max_changed_files = 50              # snapshot at once when this many distinct paths changed (0 = no limit)
max_changed_bytes = 52428800        # snapshot at once when this many bytes changed
message_format = "auto: {count} files changed"   # placeholders {count} {files} {branch}
on_branch_switch = "refuse"         # snapshot | stash | refuse — what wt switch does with a dirty tree

[sync]
auto_stage = true                   # daemon stages auto-snapshots (default true)
remote = "origin"                   # remote used by the daemon and as default for push/fetch/pull/stage
auto_fetch = true                   # daemon keeps remote-tracking branches fresh (default true)
interval_secs = 300                 # fallback periodic fetch (default 300)

[remote.origin]                     # one table per remote
url = "http://localhost:7420"       # http:// or https://; trailing '/' stripped
tenant = "acme"                     # tenant slug on the server

[secret_scanning]
enabled = true                      # default true
block_on_match = true               # false = warn only
patterns = [{ name = "internal-token", regex = "itk_[A-Za-z0-9]{32}" }]
exclude = ["tests/fixtures/**"]

[hooks]
enabled = true                      # default true
timeout_secs = 60                   # default 60

[signing]
key = "laptop"                      # key name under $WT_HOME/keys/ ("" = no signing)
auto = false                        # sign every snapshot (else only wt snapshot --sign)

[git_remote.github]                 # Git remotes for wt git push (wt git remote add)
url = "https://example.com/acme/web.git"
```

`[secret_scanning]`, `[hooks]` and `[signing]` are omitted from the file while
they hold their defaults. The same `[user]`, `[signing]` and
`[secret_scanning]` sections can be set per user in `$WT_HOME/config.toml`;
`[auto_snapshot]`, `[secret_scanning]` and `[hooks]` can be overridden per
tree in `<tree>/.wt-tree/config.toml`. Precedence: user < `.wt/` < tree.

### 2.1 Command access

```bash
wt config list
wt config get user.email [--show-origin]
wt config set user.name "Ada Lovelace"
wt config set auto_snapshot.inactivity_secs 10 --tree frontend
wt config set user.email ada@example.com --global
wt config set remote.origin.url https://wt.example.com     # creates [remote.origin] if needed
wt remote add origin http://localhost:7420 --tenant acme
```

Dotted keys accepted by `get`/`set`: `worktree.name`, `worktree.default_branch`,
`worktree.id` and `worktree.created_at` (read-only), `user.name`, `user.email`,
`auto_snapshot.enabled`, `auto_snapshot.inactivity_secs`,
`auto_snapshot.max_changed_files`, `sync.auto_stage`, `sync.remote`,
`remote.<name>.url`, `remote.<name>.tenant`, plus the v0.5 keys of the
sections above.

### 2.2 Validation

| Key | Rule |
|---|---|
| `worktree.name` | Tree-name rules: 1-100 chars, one path component, no `/ \ : * ? " < > |` or control chars, not starting with `.`, not ending with `.` or space |
| `worktree.default_branch` | Branch-name rules (see Tree.md §2.1) |
| `remote.<name>` | Name: letters, digits, `-`, `_` (≤ 64); `url` must be `http://` or `https://` with a host |
| `sync.remote` | Remote-name rules |
| booleans | `true/false/yes/no/on/off/1/0` |
| numbers | Non-negative integers |

An unparsable `config.toml` makes every command fail with `InvalidConfig`
naming the file.

---

## 3. `ignore`

Gitignore syntax, patterns relative to the worktree root. `wt init` writes a
comment header; `wt ignore add PATTERN` appends; `wt git import` copies the
Git repository's root `.gitignore` patterns here.

Evaluation (all trees):

1. Hard-coded `.wt/`, `.wt-tree/`, `.git/` — always ignored, never
   re-includable.
2. Soft defaults `__pycache__/`, `*.pyc`, `.env`, `*.log`, `node_modules/`,
   `.venv/`, `dist/`, `build/`, `target/` — ignored unless re-included with
   `!pattern`.
3. `.wt/ignore` and the root `.wtignore` form the root level. Patterns here
   cannot be re-included by `!pattern` in a deeper `.wtignore` or a
   `.wt-tree/ignore`; inside the root level the last matching line wins, so
   `!` works normally.
4. `<tree>/.wt-tree/ignore`, then `.wtignore` files in subdirectories (deeper
   files take precedence over shallower ones, except over the root level).

Because `.wt/ignore` is not shared, rules the team needs should go in a
tracked `.wtignore`. Invalid pattern lines are skipped with a warning.

```
# .wt/ignore
*.pem
.env.*
!.env.example
tests/fixtures/large/
```

---

## 4. `objects/`

Content-addressed store (`worktree_sdk::store::ObjectStore`):

- Path: `objects/<first 2 hex>/<remaining 62 hex>` of the BLAKE3-256 hash.
- Stored raw, uncompressed — the same bytes the server stores and the wire
  carries.
- Kinds: blobs (file content or one chunk), manifests (protobuf
  `ManifestRecord`), chunk lists (protobuf `ChunkList`, files > 8 MiB).
- Writes go to a temp file in the fan-out directory and are renamed into
  place; losing a rename race to another writer of the same object counts as
  success (Windows "Access is denied" included).
- Objects are never deleted locally.

## 5. `snapshots/`

One JSON file per snapshot, `snapshots/<uuid>.json`, the serde form of
`worktree_sdk::Snapshot` (hashes as hex). Records are immutable. Lookups by
prefix need at least 4 characters.

## 6. `trees/`

One directory per registered tree (root tree included); see the layout
table above and [Tree.md](../tree/Tree.md). Notes:

- `refs/branches/` may contain `/`-separated subdirectories for branch names
  like `feature/login`.
- `index.json` is only a cache; losing it costs a re-hash.
- `reflog.jsonl` lines that do not parse (torn writes) are skipped. Retention:
  90 days / 1000 entries per tree by default, applied by `wt reflog expire`
  and the daemon's maintenance pass.
- `LOCK` is an OS-level advisory lock taken by every mutating operation so the
  CLI and the daemon never interleave writes to one tree; it is released
  automatically when the process exits. The daemon's maintenance pass removes
  stale lock files.

## 7. `conflicts/`

`conflicts/<tree>.json` exists while a merge in that tree has unresolved
conflicts. It is a JSON array of `ConflictInfo`:

```json
[
  {"path": "src/auth/handler.rs", "kind": "content",
   "hunks": [{"ours_start": 42, "ours_len": 6, "theirs_start": 42, "theirs_len": 9}]},
  {"path": "docs/old.md", "kind": "modify_delete", "hunks": []}
]
```

`kind` is one of `content`, `modify_delete`, `add_add`, `binary`. The merge
itself (source branch, tips, base, message) is in
`trees/<tree>/MERGE_STATE.json`. Both files are removed by `wt merge
--continue`, a completing `wt snapshot`, or `wt merge abort`.

## 8. `hooks/`

Worktree-wide hooks run for every tree; tree hooks live in
`<tree>/.wt-tree/hooks/`.

| File | When | Failure |
|---|---|---|
| `hooks/pre-snapshot` | Before the working state is read | Non-zero exit aborts the snapshot (`pre-snapshot hook failed: ...`) |
| `hooks/post-snapshot` | After the snapshot is written | Printed to stderr; snapshot kept |

On Windows `<phase>.cmd`, `<phase>.bat` and `<phase>.ps1` are also found.
Order: `.wt/hooks/` first, then the tree's. Environment:

| Variable | Value |
|---|---|
| `WT_HOOK` | `pre-snapshot` or `post-snapshot` |
| `WT_TREE` | Tree name |
| `WT_TREE_PATH` | Absolute tree directory (also the working directory) |
| `WT_BRANCH` | Current branch |
| `WT_ROOT` | Worktree root |
| `WT_SNAPSHOT` | New snapshot id (post only) |

`[hooks] timeout_secs` kills slow hooks; `[hooks] enabled = false` or
`wt snapshot --no-verify` skips them. Hooks run for daemon auto-snapshots too.

## 9. Per-user state (`$WT_HOME`)

`$WT_HOME` (default `~/.wt`, from `HOME` or `USERPROFILE`) is outside every
worktree:

| File | Content |
|---|---|
| `credentials.toml` | Saved bearer tokens per normalized server URL (mode 0600 on Unix) |
| `config.toml` | User-level `[user]`, `[signing]`, `[secret_scanning]` |
| `keys/<name>.key`, `keys/<name>.pub` | Ed25519 signing keys (hex seed, 0600) and public key + registered key id |
| `bg.json` | Running daemon: `{pid, port, version, token}` |
| `bg.toml` | Daemon watch list `watched = [...]` |
| `bg.log` | Daemon log (rotated to `bg.log.old` above 10 MiB) |

```toml
# $WT_HOME/credentials.toml
[servers."http://localhost:7420"]
username = "ada"
token = "wt_..."
```

Tests and isolated environments must set `WT_HOME`.

---

## 10. Initialization

`wt init [PATH] [--name N] [--branch B] [--watch]` creates:

1. `config.toml` with `[worktree]` (name, new UUID, timestamp, default
   branch) and default `[user]`, `[auto_snapshot]`, `[sync]` sections;
2. `objects/`, `snapshots/`, `trees/`;
3. `ignore` with a comment header;
4. `trees/<name>/` for the root tree: `tree.toml`, `HEAD`, an empty default
   branch ref, `refs/tags/`, `refs/remotes/`, and an `init` reflog entry.

If any step fails, `.wt/` is removed. `AlreadyInitialized` if `.wt/` exists.
`--watch` also starts the daemon (if needed) and watches the new worktree.
`hooks/`, `access/`, `conflicts/` and the per-tree `shallow`/`stubbed` files
are created only when used. `wt clone` performs the same initialization, then
configures `[remote.origin]`, adopts the server tree id and fetches.

## 11. Compatibility

- Readers ignore unknown keys and `wt config set` preserves them, so newer
  sections do not break older files.
- New fields of stored JSON records (`Snapshot.dependencies`, `reverts`,
  `signature`; `ManifestEntry.chunked`) default when absent, so v0.5 reads
  pre-v0.5 data unchanged.
- There is no explicit format version and no migration step.

---

## Implementation Status

| Item | Status |
|---|---|
| Layout of `config.toml`, `ignore`, `objects/`, `snapshots/`, `trees/`, `conflicts/` | Built |
| `$WT_HOME` credentials, daemon files | Built |
| `[secret_scanning]`, `[hooks]`, `[signing]`, `sync.auto_fetch/interval_secs` | Built (v0.5) |
| Config hierarchy with `--show-origin`, `--tree`, `--global` | Built (v0.5 §13.9) |
| `hooks/`, `access/`, `shallow`, `stubbed`, conflict hunks, `[git_remote.*]` | Built (v0.5) |
| `git-map.json` | Built |

## Roadmap

Described in earlier drafts, not built and not in v0.5:

- `[worktree] server / tenant / visibility` keys — the server URL and tenant
  are per remote (`[remote.<name>]`); visibility is a server tree setting.
- `[sync] auto / retry_backoff_max_secs / conflict_strategy`, `[shallow]`,
  `[large_files]` / `[storage]` (threshold, chunk size, lazy loading,
  preload patterns), `[reflog]` (retention, per-branch caps, server sync,
  compression), `[license]`, `[[license.path]]`, `[[license.grant]]`,
  `[[registered_path]]`, `[[tenant_access]]`, `[[branch_protection]]` — the
  corresponding features are server rules or fixed behaviour today.
- `identity/token` and `identity/identity.toml` — tokens live in
  `$WT_HOME/credentials.toml`; identity is `[user]`.
- Per-branch and global reflog files (`reflog/<branch>.log`,
  `_global.log`) and tab-separated reflog lines — the reflog is one JSONL file
  per tree.
- One `.conflict.json` per conflicting path with stored ours/base/theirs
  text.
- `cache/` directory and a bounded object cache.
- A format version with automatic forward migration.
- Hook variables `WT_SNAPSHOT_TYPE` and `WT_CHANGED_FILES`.
- Client-side rejection of `access/` edits without a `PolicyManage`
  permission — authorization happens when the server applies the files.

## References

- `crates/worktree-sdk/src/repo/mod.rs` — layout doc comment, `init`, `open`
- `crates/worktree-sdk/src/config.rs` — `WorktreeConfig`, sections, `get`/`set`, `wt_home()`
- `crates/worktree-sdk/src/store.rs` — `ObjectStore`
- `crates/worktree-sdk/src/repo/{trees,refs,reflog,working,merging,ignore_rules,lock,fsutil}.rs`
- `crates/worktree-sdk/src/hooks.rs`, `src/signing.rs`, `src/scan/secrets.rs`
- `crates/worktree-sdk/src/remote/credentials.rs` — `credentials.toml`
- `crates/worktree-sdk/src/ipc.rs`, `crates/worktree-bg/src/{info,config}.rs` — `bg.json`, `bg.toml`
- `crates/worktree-git/src/{map,import}.rs` — `git-map.json`, `.gitignore` import
- `crates/worktree-cli/src/commands/{init,config,ignore,remote,access}.rs`
