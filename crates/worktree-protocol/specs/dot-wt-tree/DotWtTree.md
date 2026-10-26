# `.wt-tree/` Nested Tree Directory Specification

> **Version:** 0.7 — describes the marker directory as built (`crates/worktree-sdk/src/repo/trees.rs`, `docs/ENGINEERING_PLAN.md` §13.7, §13.9, §15.10).
> Features described but not built are listed under [Roadmap](#roadmap).

---

## 1. Overview

A `.wt-tree/` directory turns the directory containing it into a **nested
tree** of the enclosing worktree. It is written by `wt tree add NAME PATH`
and holds:

- the tree's identity (`name`, `id`) in `local.toml` — the marker proper,
  **local only**;
- tree-level configuration overrides (`config.toml`: `[auto_snapshot]`,
  `[secret_scanning]`, `[hooks]`) — **shared**;
- tree-level ignore patterns (`ignore`) — **shared**;
- tree-level snapshot hooks (`hooks/`) — **shared**;
- tree-scoped declarative access policies (`access/`, deny statements
  only) — **shared**.

The root tree of a worktree created with `wt init` has no `.wt-tree/`; its
equivalents are `.wt/config.toml`, `.wt/ignore`, `.wt/hooks/` and
`.wt/access/`. A worktree created by `wt clone` of a tree that shares
`.wt-tree/` files has them at its root (see [§10](#10-sharing)).

Three facts shape everything below:

1. **The registry is in `.wt/`, not here.** A tree exists because
   `.wt/trees/<name>/tree.toml` exists; the marker records the same name and
   id so tools can identify the directory. Copying a directory with a
   `.wt-tree/` into a worktree does not register it.
2. **The shared files travel with the tree** (v0.7, §15.10).
   `.wt-tree/config.toml`, `.wt-tree/ignore`, `.wt-tree/hooks/**` and
   `.wt-tree/access/**` of a tree are snapshotted, pushed, fetched and
   cloned like any other file of that tree, so configuration, hooks and
   policies are reviewed and versioned with the code.
3. **Everything else in `.wt-tree/` stays local** — in particular
   `local.toml`. Manifest paths inside `.wt/`, a `.wt-tree/` path that is
   not one of the shared files, and any `.wt-tree/` below the tree root (a
   nested tree's marker) are rejected by the SDK and by the server. Applying
   access policies to the server still needs an explicit `wt access apply`.

---

## 2. Layout

```
services/auth/
├── .wt-tree/
│   ├── local.toml           marker identity: name + id (local only, required)
│   ├── config.toml          tree-level overrides (shared, optional)
│   ├── ignore               tree-level ignore patterns (shared, optional)
│   ├── hooks/               (shared)
│   │   ├── pre-snapshot     (optional; also .cmd/.bat/.ps1 on Windows)
│   │   └── post-snapshot
│   └── access/              (shared)
│       └── policies.toml    tree-scope deny policies (optional)
├── .wtignore                tracked ignore file
└── src/...
```

`wt tree add` creates only `local.toml`. `wt tree init-config` scaffolds
the shared set (`config.toml`, `ignore`, `hooks/`, `access/`).

### `local.toml`

```toml
# Local identity of this nested tree (written by wt; never snapshotted).
name = "auth"
id = "0b7f3c9e-5d2a-4e61-8f0c-6a1d2e3b4c5d"
```

`name` and `id` must match `.wt/trees/<name>/tree.toml`; the SDK maintains
them (the id is rewritten when the tree adopts the server id on first
contact).

---

## 3. `config.toml`

```toml
# Overrides of .wt/config.toml for this tree (all optional)
[auto_snapshot]
enabled = true
inactivity_secs = 60
max_changed_files = 20
max_changed_bytes = 10485760
message_format = "auto({branch}): {count} files changed"
on_branch_switch = "snapshot"     # snapshot | stash | refuse

[secret_scanning]
enabled = true
block_on_match = false            # warn only in this tree
patterns = [{ name = "auth-signing-key", regex = "ask_[0-9a-f]{40}" }]
exclude = ["testdata/**"]

[hooks]
enabled = true
timeout_secs = 120
```

| Key / section | Effect |
|---|---|
| `[auto_snapshot]` | Overrides the worktree values for this tree |
| `[secret_scanning]` | Overrides the worktree/user values for snapshots of this tree |
| `[hooks]` | Hook settings for this tree's snapshots |

Resolution: `$WT_HOME/config.toml` < `.wt/config.toml` < this file, key by
key. Inspect and edit with:

```bash
wt config get auto_snapshot.max_changed_files --tree auth --show-origin
wt config set secret_scanning.block_on_match false --tree auth
```

Unknown keys are preserved on save. Server URL, tenant, remotes, `[user]`,
`[signing]` and `[sync]` are worktree- or user-level only.

---

## 4. `ignore`

Gitignore syntax, relative to the tree directory, applied only to this tree.
Precedence (low → high): hard-coded `.wt/ .wt-tree/ .git/`, soft defaults,
root level (`.wt/ignore`, root `.wtignore`), `.wt-tree/ignore`, then
`.wtignore` files below. Patterns from the root level cannot be re-included
here: a `!pattern` that would re-include a path ignored by the root level has
no effect. A `!pattern` can re-include a soft default or this file's own
earlier patterns.

```
# services/auth/.wt-tree/ignore
coverage/
*.snap
!keep.snap
```

This file is shared with the tree (v0.7). Ignore rules never apply to the
tree's own shared `.wt-tree/` files themselves.

---

## 5. `hooks/`

Same contract as `.wt/hooks/` ([DotWt.md §8](../dot-wt/DotWt.md#8-hooks)):

```
1. .wt/hooks/pre-snapshot          (abort on failure)
2. <tree>/.wt-tree/hooks/pre-snapshot
3. --- snapshot written ---
4. .wt/hooks/post-snapshot         (failures reported)
5. <tree>/.wt-tree/hooks/post-snapshot
```

Hooks run outermost first: the worktree's `.wt/hooks/`, then each enclosing
parent tree's `.wt-tree/hooks/`, then the tree's own. Environment: `WT_HOOK`,
`WT_TREE`, `WT_TREE_PATH`, `WT_BRANCH`, `WT_ROOT`, `WT_SNAPSHOT` (post); the
working directory is the tree root; timeout from `[hooks] timeout_secs`.

Hook scripts are tracked with their executable bit (manifest
`executable`), which checkout, clone and pull restore on Unix. On Windows,
where files have no executable bit, a newly added file below
`.wt-tree/hooks/` is recorded executable so that it runs on Unix clones.

---

## 6. `access/policies.toml`

Tree-scoped policies in the declarative access format, restricted to **deny
statements** (an `allow` statement is a validation error, `E2001..` series):

```toml
[[policy]]
name = "auth-hardening"
description = "Contractors may not change crypto code or push main"
enabled = true

[[policy.statement]]
sid = "no-contractor-crypto"
effect = "deny"
principals = ["team:contractors", "tenant:partner"]
actions = ["tree:write"]
resources = ["wt:path/acme/auth/src/crypto/**"]

[[policy.statement]]
sid = "no-contractor-main"
effect = "deny"
principals = ["team:contractors"]
actions = ["branch:push"]
resources = ["wt:branch/acme/auth/main"]
```

Workflow:

```bash
wt access validate [--dir services/auth]   # local parse + validation
wt access plan                             # dry run against the server (create/update/delete)
wt access apply [--yes]                    # applies the tenant files (.wt/access/)
wt access apply --tree-doc NAME [--yes]    # applies one tree's .wt-tree/access/ file
wt access pull                             # writes the server's current documents back to TOML
```

The SDK compiles the TOML to canonical JSON and calls `SyncAccessConfig`
with `tree_id` set to the tree's server id (`dry_run` for `plan`). The server
validates statements, references and privilege escalation, then replaces the
config-managed policies of that tree atomically, stores the document
(`access_docs`), audits `access.applied` and publishes `access.applied`.
Applied policies are `managed_by = "config"` and read-only in the REST API
(`409 {"code":"managed_by_config"}`) unless an owner passes `?force=true`.

Because deny statements can only remove permissions, a tree document cannot
grant more than the tenant's roles, grants and policies allow. Custom roles
are tenant-level only (`.wt/access/roles.toml`).

---

## 7. Nesting

```
services/auth/                    tree "auth"
├── .wt-tree/
└── plugins/oauth/                tree "oauth" (nested inside auth)
    ├── .wt-tree/
    └── src/lib.rs                belongs to "oauth" only
```

- Each level is an independent tree with its own branches, history and
  server tree; the parent's snapshots exclude `plugins/oauth/`.
- Configuration resolves user → worktree → each enclosing parent tree →
  this tree: a parent tree's `.wt-tree/config.toml` applies to its nested
  trees unless they override the key.
- Ignore rules: the root level applies everywhere; a parent tree's
  `.wt-tree/ignore` applies to the parent tree's own files only, while
  tracked `.wtignore` files in a directory apply to every path below it.
- Tree-scope policies apply to the server tree they are applied to; server
  authorization treats every tree independently.
- There is no depth limit.

---

## 8. Validation

`wt tree validate [NAME]` (all trees when omitted) reports:

1. a missing `.wt-tree/` marker (nested trees);
2. a missing or mismatching identity in `local.toml` (warning; it is
   rewritten on the next scan), and legacy `name`/`id` keys still in
   `config.toml` (warning);
3. a `config.toml` that does not parse, or sections not read at tree level;
4. an `ignore` file that does not parse;
5. a nested `.wt/` directory inside the tree.

Manifest path rules (SDK `validate_manifest_path`, server
`validateManifestPath` in `syncsvc`): no `.wt` component anywhere; a
`.wt-tree` component only as the first component and only for
`.wt-tree/config.toml`, `.wt-tree/ignore`, `.wt-tree/hooks/<path>` and
`.wt-tree/access/<path>` (with no further reserved component). A push whose
manifest breaks these rules is refused (`FailedPrecondition`, invalid
manifest).

`wt access validate` checks `access/policies.toml`. Other commands treat an
unparsable tree config as an error for that tree.

---

## 9. Lifecycle

| Action | Effect on `.wt-tree/` |
|---|---|
| `wt tree add NAME PATH` | Creates `PATH/.wt-tree/local.toml` with `name` and a new `id` |
| First push / fetch / clone contact | `id` in `local.toml` rewritten to the server tree id |
| `wt tree init-config` | Scaffolds `config.toml`, `ignore`, `hooks/`, `access/` |
| `wt status` / `wt snapshot` | Shared files show up as changes and are snapshotted; legacy identity lines are migrated first |
| `wt checkout` / `switch` / `pull` / `clone` | Shared files are written (and removed) like any tracked file; `local.toml` is never touched |
| `wt tree remove NAME` | Deletes `.wt-tree/` and `.wt/trees/NAME/`; files are kept and fall back to the enclosing tree (the shared files stay in the tree's history) |

---

## 10. Sharing

Since v0.7 (§15.10) the shared files are ordinary tracked files of the tree
they configure, with paths relative to the tree root
(`.wt-tree/config.toml`, …):

- **Parents never see them.** A parent tree's scans stop at the nested
  tree's directory, so a nested tree's `.wt-tree/` is never part of the
  parent's manifest (and is rejected there).
- **Clone.** `wt clone TENANT/TREE` makes the tree the root tree of a new
  worktree, so its shared files land in `<root>/.wt-tree/`. A root
  `.wt-tree/` applies like an enclosing tree's: `config.toml` is a `tree`
  level of the configuration stack (above `.wt/config.toml`), `ignore` is an
  additive tree ignore file for the worktree, and `hooks/` run after
  `.wt/hooks/`. Such a root has no `local.toml` (its identity is in
  `.wt/trees/`).
- **Daemon.** Changes to shared files are working changes (they can trigger
  auto-snapshots) and also reload the configuration.

### Migration

Before v0.7 the identity lived in `config.toml` (`name`/`id` keys) and the
whole directory was local. On the first scan of a tree (any `wt status`,
`snapshot`, `diff`, `checkout`, …) after upgrading, the SDK:

1. writes `local.toml` with the identity (unless it exists);
2. removes the top-level `name = …` / `id = …` lines (and the old
   "Marker identity" comment) from `config.toml` textually, keeping
   comments and layout, and deletes the file when nothing else is left.

Existing worktrees therefore start tracking the shared files on their next
snapshot; nothing else changes. Older clients ignore (and never write)
`local.toml`.

---

## Implementation Status

| Item | Status |
|---|---|
| Marker identity in `local.toml`, registry in `.wt/trees/` | Built (v0.7 §15.10; `config.toml` before) |
| Shared `.wt-tree/{config.toml,ignore,hooks/**,access/**}` snapshotted, pushed, cloned | Built (v0.7 §15.10) |
| Manifest validation (SDK + server): `.wt-tree/` only at the tree root, never `.wt/` | Built (v0.7 §15.10) |
| Migration of legacy identity lines | Built (v0.7 §15.10) |
| Root `.wt-tree/` (from a clone) applies to the root tree | Built (v0.7 §15.10) |
| Tree-level `[auto_snapshot]`, `[secret_scanning]`, `[hooks]` overrides | Built (v0.5 §13.9) |
| `.wt-tree/ignore` | Built (v0.5 §13.9) |
| `.wt-tree/hooks/` | Built (v0.5 §13.9) |
| `.wt-tree/access/policies.toml` (deny only) | Built (v0.5 §13.7) |
| `wt tree validate`, `wt tree init-config` | Built (v0.5 §13.9) |

## Roadmap

Described in earlier drafts, not built:

- Server-side rejection of invalid tree configuration (unparsable
  `config.toml`, invalid `access/policies.toml`) during sync, and applying
  pushed `access/` documents automatically.
- A multi-tree clone that registers a tree's nested trees.
- `[tree]` section keys `branch_strategy`, `parent`, `created_by`,
  `description`.
- `[large_files]` (threshold, chunk size, lazy loading, preload patterns) and
  `[reflog]` (retention, per-branch caps) overrides.
- `[license]`, `[[license.path]]`, `[[license.grant]]`, `spdx_strict` — license
  rules are server rules (`wt license`).
- `[[registered_path]]` and policy scopes `{ path = ... }` limited to
  registered paths — path rules use glob resources.
- `[[branch_protection]]` with `[branch_protection.review]` merged additively
  with root rules — protections are server rules (`wt protect`).
- Policy syntax with `subject`/`not_team`/`unless`/`scope` tables and
  tree-level `allow` statements checked against a computed root ceiling.
- Hook variables `WT_TREE_NAME`, `WT_CHANGED_FILES`, `WT_USER`,
  `WT_HOOK_PHASE`.
- Warnings for attempted negation of root ignore patterns, and automatic
  migration of deprecated keys.

## References

- `crates/worktree-sdk/src/repo/trees.rs` — `add_tree`, `remove_tree`, marker writing (`local.toml`), `set_tree_id`, identity migration
- `crates/worktree-sdk/src/repo/working.rs` — scanning the tree's own shared `.wt-tree/` files
- `crates/worktree-sdk/src/repo/mod.rs` — `TREE_MARKER_DIR`, layout
- `crates/worktree-sdk/src/model.rs` — `validate_manifest_path`, `is_shared_tree_config_path`
- `services/server-go/internal/syncsvc/{objects,convert}.go` — server manifest path validation
- `crates/worktree-sdk/src/config.rs` — config sections and hierarchy
- `crates/worktree-sdk/src/repo/ignore_rules.rs` — ignore evaluation
- `crates/worktree-sdk/src/hooks.rs` — hook discovery and execution
- `crates/worktree-protocol/src/access/` — declarative parser/compiler
- `crates/worktree-sdk/src/remote/access.rs` — plan/apply/pull
- `crates/worktree-cli/src/commands/{tree,config,access}.rs`
- `services/server-go/internal/accesscfg/` — server apply
