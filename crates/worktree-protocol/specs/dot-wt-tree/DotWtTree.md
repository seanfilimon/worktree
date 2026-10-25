# `.wt-tree/` Nested Tree Directory Specification

> **Version:** 0.5 — describes the marker directory as built (`crates/worktree-sdk/src/repo/trees.rs`, `docs/ENGINEERING_PLAN.md` §13.7, §13.9).
> Features described but not built are listed under [Roadmap](#roadmap).

---

## 1. Overview

A `.wt-tree/` directory turns the directory containing it into a **nested
tree** of the enclosing worktree. It is written by `wt tree add NAME PATH`
and holds:

- the tree's identity (`name`, `id`) — the marker proper;
- tree-level configuration overrides (`[auto_snapshot]`,
  `[secret_scanning]`, `[hooks]`);
- tree-level ignore patterns;
- tree-level snapshot hooks;
- tree-scoped declarative access policies (deny statements only).

The root tree has no `.wt-tree/`; its equivalents are `.wt/config.toml`,
`.wt/ignore`, `.wt/hooks/` and `.wt/access/`.

Two facts shape everything below:

1. **The registry is in `.wt/`, not here.** A tree exists because
   `.wt/trees/<name>/tree.toml` exists; the marker records the same name and
   id so tools can identify the directory. Copying a directory with a
   `.wt-tree/` into a worktree does not register it.
2. **`.wt-tree/` is never snapshotted.** Manifest paths with a `.wt-tree`
   component are rejected, so the contents of this directory are local to the
   worktree; they are not pushed, fetched or cloned. Server-side effects
   happen only through explicit commands (`wt access apply`).

---

## 2. Layout

```
services/auth/
├── .wt-tree/
│   ├── config.toml          marker identity + tree-level overrides (required)
│   ├── ignore               tree-level ignore patterns (optional)
│   ├── hooks/
│   │   ├── pre-snapshot     (optional; also .cmd/.bat/.ps1 on Windows)
│   │   └── post-snapshot
│   └── access/
│       └── policies.toml    tree-scope deny policies (optional)
├── .wtignore                tracked ignore file (shared with the team)
└── src/...
```

`wt tree add` creates only `config.toml`. `wt tree init-config` scaffolds
the full set (`config.toml`, `ignore`, `hooks/`, `access/`).

---

## 3. `config.toml`

```toml
# Marker identity (written by wt tree add; rewritten when the tree adopts the server id)
name = "auth"
id = "0b7f3c9e-5d2a-4e61-8f0c-6a1d2e3b4c5d"

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
| `name`, `id` | Must match `.wt/trees/<name>/tree.toml`; maintained by the SDK |
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

Because this file is not shared, prefer a tracked `.wtignore` in the tree
directory for rules the team needs.

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

Only the hooks of the tree being snapshotted run: a nested tree's snapshot
does not run its parent tree's `.wt-tree/hooks/`. Environment: `WT_HOOK`,
`WT_TREE`, `WT_TREE_PATH`, `WT_BRANCH`, `WT_ROOT`, `WT_SNAPSHOT` (post); the
working directory is the tree root; timeout from `[hooks] timeout_secs`.

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
wt access apply [--yes]                    # applies tenant files and tree files
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
- Configuration resolves user → worktree → this tree. A parent tree's
  `.wt-tree/config.toml` does not apply to its nested trees.
- Ignore rules: the root level applies everywhere; a parent tree's
  `.wt-tree/ignore` applies to the parent tree's own files only, while
  tracked `.wtignore` files in a directory apply to every path below it.
- Tree-scope policies apply to the server tree they are applied to; server
  authorization treats every tree independently.
- There is no depth limit.

---

## 8. Validation

`wt tree validate [NAME]` (all trees when omitted) reports:

1. a missing `.wt-tree/` marker;
2. a `config.toml` that does not parse;
3. an `ignore` file that does not parse;
4. a nested `.wt/` directory inside the tree.

`wt access validate` checks `access/policies.toml`. Other commands treat an
unparsable tree config as an error for that tree.

---

## 9. Lifecycle

| Action | Effect on `.wt-tree/` |
|---|---|
| `wt tree add NAME PATH` | Creates `PATH/.wt-tree/config.toml` with `name` and a new `id` |
| First push / fetch / clone contact | `id` rewritten to the server tree id |
| `wt tree init-config` | Scaffolds `ignore`, `hooks/`, `access/` |
| `wt tree remove NAME` | Deletes `.wt-tree/` and `.wt/trees/NAME/`; files are kept and fall back to the enclosing tree |

---

## Implementation Status

| Item | Status |
|---|---|
| Marker `config.toml` with `name`/`id`, registry in `.wt/trees/` | Built |
| `.wt-tree/` excluded from scans and manifests | Built |
| Tree-level `[auto_snapshot]`, `[secret_scanning]`, `[hooks]` overrides | Built (v0.5 §13.9) |
| `.wt-tree/ignore` | Built (v0.5 §13.9) |
| `.wt-tree/hooks/` | Built (v0.5 §13.9) |
| `.wt-tree/access/policies.toml` (deny only) | Built (v0.5 §13.7) |
| `wt tree validate`, `wt tree init-config` | Built (v0.5 §13.9) |

## Roadmap

Described in earlier drafts, not built and not in v0.5:

- **Versioning `.wt-tree/` with the tree** (so configuration and policies are
  pushed, reviewed and cloned with the code), and server-side rejection of
  invalid tree configuration during sync.
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
  `WT_HOOK_PHASE`; running parent-tree hooks for nested trees.
- Warnings for attempted negation of root ignore patterns, and automatic
  migration of deprecated keys.

## References

- `crates/worktree-sdk/src/repo/trees.rs` — `add_tree`, `remove_tree`, marker writing, `set_tree_id`
- `crates/worktree-sdk/src/repo/mod.rs` — `TREE_MARKER_DIR`, layout
- `crates/worktree-sdk/src/model.rs` — `validate_manifest_path` (reserved components)
- `crates/worktree-sdk/src/config.rs` — config sections and hierarchy
- `crates/worktree-sdk/src/repo/ignore_rules.rs` — ignore evaluation
- `crates/worktree-sdk/src/hooks.rs` — hook discovery and execution
- `crates/worktree-protocol/src/access/` — declarative parser/compiler
- `crates/worktree-sdk/src/remote/access.rs` — plan/apply/pull
- `crates/worktree-cli/src/commands/{tree,config,access}.rs`
- `services/server-go/internal/accesscfg/` — server apply
