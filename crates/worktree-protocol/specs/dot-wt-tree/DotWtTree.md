# `.wt-tree/` Individual Tree Configuration Specification

> **Status:** Living Document
> **Scope:** Per-tree configuration folder within a W0rkTree worktree
> **Authority:** Subordinate to root `.wt/` — can restrict, never expand

---

## 1. Overview

The `.wt-tree/` folder lives inside each **child tree** in a W0rkTree worktree. It manages
tree-specific configuration: branch strategy, auto-snapshot rules, tree-level access policies,
tree-level ignore patterns, tree-level hooks, tree-level license config, registered paths,
branch protection, and reflog retention overrides.

`.wt-tree/` operates **under the authority** of the root `.wt/`. It can customize settings for
its tree and **restrict** access further, but it **CANNOT** expand permissions or negate
root-level ignores. This invariant is enforced both by the CLI and the bgprocess at sync time.

Every tree created via `wt tree add <path>` receives a `.wt-tree/` directory scaffolded with
sensible defaults inherited from the root `.wt/` configuration.

---

## 2. Folder Structure

```
services/auth-service/
├── .wt-tree/
│   ├── config.toml           # Tree config: branch strategy, auto-snapshot, large files,
│   │                         #   reflog, branch protection, registered paths, license
│   ├── ignore                # Tree-level ignore patterns (additive only, cannot negate root)
│   │
│   ├── access/
│   │   └── policies.toml    # Tree-level access policies (can restrict, not expand)
│   │
│   └── hooks/
│       ├── pre-snapshot      # Runs before snapshot creation for this tree
│       └── post-snapshot     # Runs after snapshot creation for this tree
│
├── src/
│   ├── main.rs
│   └── oauth.rs
├── Cargo.toml
└── README.md
```

### Minimal `.wt-tree/`

A tree only requires `config.toml` to exist. All other files and directories are optional:

```
.wt-tree/
└── config.toml
```

If `ignore`, `access/`, or `hooks/` are absent, the tree inherits root-level behavior
unmodified and no tree-level customization is applied.

---

## 3. `config.toml` — Full Reference

The tree-level `config.toml` controls behavior for **this tree only**. Every key is optional;
omitted keys inherit from the root `.wt/config.toml`.

```toml
# ─── Tree Identity ───────────────────────────────────────────────────────────

[tree]
name = "auth-service"                     # Human-readable tree name
                                          # Defaults to the directory name
branch_strategy = "feature-branch"        # feature-branch | trunk-based | release-train
                                          # Determines how branches are created/merged

# ─── Auto-Snapshot ───────────────────────────────────────────────────────────

[auto_snapshot]
enabled = true                            # Enable/disable auto-snapshot for this tree
                                          # Default: inherits from root
inactivity_timeout_secs = 300             # Seconds of inactivity before auto-snapshot fires
                                          # Default: inherits from root (typically 300)
max_changed_files = 25                    # Auto-snapshot triggers if changed file count exceeds this
                                          # Default: inherits from root (typically 50)
on_tree_switch = true                     # Auto-snapshot when switching away from this tree's branch
                                          # Default: inherits from root

# ─── Large Files ─────────────────────────────────────────────────────────────

[large_files]
threshold_bytes = 5242880                 # Files above this size use chunked storage
                                          # Default: inherits from root (typically 10MB)
chunk_size_bytes = 2097152                # Chunk size for large file uploads
                                          # Default: inherits from root (typically 4MB)
lazy_loading = true                       # Lazy-load large files on demand
                                          # Default: inherits from root
preload_patterns = ["*.rs"]               # Patterns of large files to eagerly preload
                                          # Default: inherits from root

# ─── Reflog ──────────────────────────────────────────────────────────────────

[reflog]
retention_days = 180                      # How long to keep reflog entries for this tree
                                          # Default: inherits from root (typically 90)
max_entries_per_branch = 20000            # Max reflog entries per branch in this tree
                                          # Default: inherits from root (typically 10000)

# ─── License ─────────────────────────────────────────────────────────────────

[license]
license = "AGPL-3.0-only"                # SPDX identifier for this tree
                                          # Overrides root default for this tree
spdx_strict = true                        # Enforce SPDX validity
                                          # Default: inherits from root

# Per-path license overrides within this tree
[[license.path]]
path = "src/crypto"
license = "proprietary"

[[license.path]]
path = "src/vendor"
license = "Apache-2.0"

# License grants scoped to this tree
[[license.grant]]
path = "src/crypto"
tenant = "security-partner"
grant = "read-only"

# ─── Registered Paths ───────────────────────────────────────────────────────

[[registered_path]]
path = "src/crypto"
description = "Cryptographic implementations — restricted access"

[[registered_path]]
path = "src/oauth.rs"
description = "OAuth2 authorization code flow"

[[registered_path]]
path = "migrations"
description = "Database migration scripts"

# ─── Branch Protection ──────────────────────────────────────────────────────

# Tree-level branch protection is ADDITIVE to root-level protection.
# If root protects "main" with no_direct_push, the tree cannot remove that rule.
# The tree CAN add additional rules on top.

[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review"]

[branch_protection.review]
min_approvals = 2
require_from_roles = ["security-reviewer", "Admin"]

[[branch_protection]]
branch = "release/*"
rules = ["no_direct_push", "no_force_push", "require_merge_review"]

[branch_protection.review]
min_approvals = 1
require_from_roles = ["Maintainer", "Admin"]
```

### Inheritance Semantics

| Key                          | Inheritance Rule                                            |
|------------------------------|-------------------------------------------------------------|
| `tree.name`                  | Defaults to directory name; no inheritance                  |
| `tree.branch_strategy`       | Defaults to root's implicit `feature-branch`                |
| `auto_snapshot.*`            | Omitted keys inherit from root `[auto_snapshot]`            |
| `large_files.*`              | Omitted keys inherit from root `[large_files]`              |
| `reflog.*`                   | Omitted keys inherit from root `[reflog]`                   |
| `license.license`            | Overrides root `[license].default` for this tree            |
| `license.path`               | Tree-scoped; does not merge with root `[[license.path]]`    |
| `registered_path`            | Tree-scoped; root registered paths still apply globally     |
| `branch_protection`          | **Additive** — merges with root rules, cannot remove        |

---

## 4. Authority Model

The authority model is the **core invariant** of `.wt-tree/`. It is enforced by bgprocess
during sync and by the CLI during local operations.

### 4.1 CAN Customize

The following settings can be freely customized per tree:

- **Auto-snapshot rules** — enable/disable, timeouts, file thresholds
- **Branch strategy** — `feature-branch`, `trunk-based`, `release-train`
- **Large file thresholds** — can lower (but not raise above root ceiling)
- **Reflog retention** — can extend or shorten for this tree
- **Branch protection** — can ADD rules; cannot remove rules set by root
- **Registered paths** — tree-scoped path registration
- **Tree-level license** — can set a different SPDX license for the tree
- **Tree-level ignore patterns** — additive patterns only
- **Tree-level access policies** — can restrict further, not expand
- **Tree-level hooks** — additional hooks that run after root hooks

### 4.2 CANNOT Override

The following are controlled exclusively by the root `.wt/` and cannot be overridden:

| Setting                         | Reason                                                     |
|---------------------------------|------------------------------------------------------------|
| Server connection               | Single source of truth in `.wt/config.toml`                |
| Tenant identity                 | Bound to the worktree, not to individual trees             |
| Worktree visibility             | Global property of the worktree                            |
| Root-level ignore patterns      | Security/policy — cannot negate root ignores               |
| Root-level access roles         | Role definitions are worktree-wide                         |
| Root-level access policies      | Cannot grant more than root allows                         |
| Root license defaults           | Server respects root for cross-tenant enforcement          |
| Sync interval                   | Controlled by bgprocess at the worktree level              |
| Shallow clone settings          | Controlled at worktree level                               |

### 4.3 Enforcement

```
┌──────────────────────────────┐
│  Root .wt/ (ceiling)         │ ← Maximum permissions / authoritative ignores
├──────────────────────────────┤
│  Tree .wt-tree/ (restrict)   │ ← Can tighten within ceiling
├──────────────────────────────┤
│  Subtree .wt-tree/ (restrict)│ ← Can tighten further within parent tree
└──────────────────────────────┘
```

At every level, the effective policy is the **intersection** (most restrictive combination)
of all levels in the chain.

---

## 5. Subtree Nesting

When a tree contains subtrees (nested `.wt-tree/` inside another `.wt-tree/`), the authority
chain extends downward:

```
services/auth-service/               ← Tree (has .wt-tree/)
├── .wt-tree/
│   └── config.toml
├── plugins/
│   └── oauth-plugin/               ← Subtree (has .wt-tree/ inside parent's .wt-tree/)
│       ├── .wt-tree/
│       │   ├── config.toml
│       │   └── ignore
│       └── src/
│           └── lib.rs
└── src/
    └── main.rs
```

### Nesting Rules

1. **Authority chain:** Root `.wt/` → Parent `.wt-tree/` → Subtree `.wt-tree/`
2. **Each level is a further restriction** — subtree cannot expand what parent restricts
3. **Ignore patterns accumulate** — subtree cannot negate parent tree's patterns
4. **Access policies intersect** — effective policy is the most restrictive
5. **Branch protection merges** — rules from all levels are combined (union)
6. **Hooks execute in order** — root hooks → parent tree hooks → subtree hooks
7. **No depth limit** — nesting can go arbitrarily deep, but each level only restricts

### Resolution Example

If root `.wt/` grants `team-backend` read+write access, and the parent tree's
`policies.toml` restricts `team-backend` to read-only, then the subtree's `policies.toml`
**cannot** restore write access. The most restrictive policy wins at every level.

---

## 6. `ignore` (Tree-Level)

The tree-level `ignore` file uses the same syntax as the root `.wt/ignore` (and `.gitignore`):

```
# Tree-specific ignores for auth-service
# These patterns are ADDITIVE to root .wt/ignore

# Test fixtures
tests/fixtures/*.snapshot
tests/fixtures/large-*.json

# Generated code
src/generated/

# Local development overrides
.env.local
docker-compose.override.yml
```

### Rules

- **Additive only** — patterns here add to the root `.wt/ignore` patterns
- **CANNOT negate root patterns** — if root ignores `*.log`, the tree cannot un-ignore `!*.log`
- If a negation of a root pattern is attempted, bgprocess logs a **warning** and the root
  pattern wins silently
- Negation (`!`) IS allowed for patterns defined within this same file or within the
  tree's own scope
- Subtree `ignore` files cannot negate parent tree's `ignore` patterns

### Pattern Resolution Order

1. Hard-coded ignores (`.wt/`, `.wt-tree/`, `.git/`) — always active, never overridable
2. Root `.wt/ignore` — authoritative
3. Parent tree `.wt-tree/ignore` — additive
4. Subtree `.wt-tree/ignore` — additive (cannot negate any above)
5. Soft defaults — active unless explicitly un-ignored in `.wt/ignore`

The final ignore set is the **union** of all levels.

---

## 7. `access/policies.toml` (Tree-Level)

Tree-level access policies define who can do what **within this tree only**. They follow the
same RBAC + ABAC model as root `.wt/access/policies.toml` but are scoped and restricted.

```toml
# ─── Tree-Level Access Policies ─────────────────────────────────────────────
# These policies apply to the auth-service tree only.
# They can RESTRICT access further but CANNOT expand beyond root policies.

[[policy]]
name = "restrict-crypto-to-security-team"
description = "Only security team can write to src/crypto"
effect = "deny"

[policy.subject]
not_team = ["security"]

[policy.action]
actions = ["write", "snapshot"]

[policy.scope]
path = "src/crypto"                      # Must be a registered_path in this tree's config.toml

# ─── Read-Only for External Contractors ──────────────────────────────────────

[[policy]]
name = "contractors-read-only"
description = "External contractors get read-only access to this tree"
effect = "allow"

[policy.subject]
role = ["Contractor"]

[policy.action]
actions = ["read"]

[policy.scope]
scope = "tree"                           # Applies to entire tree

# ─── Deny All to Specific Tenant ────────────────────────────────────────────

[[policy]]
name = "block-external-tenant"
description = "Block access from untrusted tenant"
effect = "deny"

[policy.subject]
tenant = ["external-corp"]

[policy.action]
actions = ["*"]

[policy.scope]
scope = "tree"
```

### Scope Options

| Scope                          | Meaning                                              |
|--------------------------------|------------------------------------------------------|
| `scope = "tree"`               | Entire tree                                          |
| `path = "src/crypto"`         | Specific registered path within this tree            |
| `branch = "main"`             | Specific branch within this tree                     |
| `branch = "release/*"`        | Branch pattern within this tree                      |

### Restriction-Only Invariant

The server and bgprocess enforce the restriction-only invariant:

- **Before applying** a tree policy, the effective root policy for the subject is computed
- **If the tree policy would EXPAND** permissions (e.g., grant write where root only grants
  read), the tree policy is **rejected** and a warning is logged
- **If the tree policy RESTRICTS** permissions (e.g., deny write where root grants read+write),
  the tree policy is **accepted** and the effective policy is the intersection

---

## 8. `hooks/` (Tree-Level)

Tree-level hooks run **in addition to** root-level hooks. They provide tree-specific
automation for snapshot lifecycle events.

### Execution Order

```
1. Root .wt/hooks/pre-snapshot       ← Runs first (abort = no snapshot)
2. Tree .wt-tree/hooks/pre-snapshot  ← Runs second (abort = no snapshot)
3. --- snapshot creation ---
4. Root .wt/hooks/post-snapshot      ← Runs first after snapshot
5. Tree .wt-tree/hooks/post-snapshot ← Runs second after snapshot
```

For nested subtrees, the order extends:

```
1. Root pre-snapshot
2. Parent tree pre-snapshot
3. Subtree pre-snapshot
4. --- snapshot creation ---
5. Root post-snapshot
6. Parent tree post-snapshot
7. Subtree post-snapshot
```

### Hook Interface

Hooks are **executable scripts or programs**. They receive context via environment variables:

| Variable                     | Description                                           |
|------------------------------|-------------------------------------------------------|
| `WT_TREE_NAME`              | Name of the tree being snapshotted                    |
| `WT_TREE_PATH`              | Absolute path to the tree directory                   |
| `WT_BRANCH`                 | Current branch name                                   |
| `WT_SNAPSHOT_ID`            | Snapshot ID (only in post-snapshot)                    |
| `WT_CHANGED_FILES`          | Newline-separated list of changed file paths          |
| `WT_USER`                   | Current user identity                                 |
| `WT_HOOK_PHASE`             | `pre-snapshot` or `post-snapshot`                     |

### Behavior

- **pre-snapshot:** Exit code `0` = proceed. Any non-zero exit code **aborts** the snapshot.
  Stderr output is captured and shown to the user as the abort reason.
- **post-snapshot:** Exit code is logged but does **not** affect the snapshot. Post-snapshot
  hooks are informational — useful for notifications, CI triggers, etc.
- **Timeout:** Hooks have a default timeout of 30 seconds. Configurable in root
  `.wt/config.toml` under `[hooks].timeout_secs`.
- **Permissions:** Hook files must be executable (`chmod +x` on Unix). On Windows, `.cmd`,
  `.bat`, `.ps1`, and `.exe` extensions are recognized.

---

## 9. What `.wt-tree/` Does NOT Contain

The following are explicitly **out of scope** for `.wt-tree/`:

| Concern                        | Where It Lives                                        |
|--------------------------------|-------------------------------------------------------|
| Object store (blobs, snapshots)| Managed by bgprocess externally                       |
| Full history                   | Managed by bgprocess + synced to server               |
| Identity / authentication      | Root `.wt/identity/`                                  |
| Server connection config       | Root `.wt/config.toml` `[worktree]`                   |
| Tenant configuration           | Root `.wt/config.toml` `[worktree]`                   |
| Custom role definitions        | Root `.wt/access/roles.toml`                          |
| Global reflog                  | Root `.wt/reflog/_global.log`                         |
| Conflict metadata              | Root `.wt/conflicts/`                                 |
| Cache                          | Root `.wt/cache/`                                     |

---

## 10. Creation and Lifecycle

### Automatic Scaffolding

When a tree is created via `wt tree add <path>`, the CLI scaffolds `.wt-tree/` automatically:

```
$ wt tree add services/auth-service

Created tree 'auth-service' at services/auth-service/
  .wt-tree/config.toml  — initialized with defaults from root
```

The scaffolded `config.toml` contains:

```toml
[tree]
name = "auth-service"

# All other settings inherit from root .wt/config.toml.
# Uncomment and customize as needed:

# [auto_snapshot]
# enabled = true
# inactivity_timeout_secs = 300

# [large_files]
# threshold_bytes = 10485760

# [reflog]
# retention_days = 90
```

### Removal

When a tree is removed via `wt tree remove <path>`, the `.wt-tree/` directory is deleted
along with the tree registration. History and snapshots are preserved on the server.

### Migration

If a `.wt-tree/config.toml` uses a deprecated key, bgprocess logs a deprecation warning
during sync and attempts automatic migration. The migrated file is written back to disk
with comments explaining the change.

---

## 11. Validation

bgprocess validates `.wt-tree/` contents during every sync cycle:

1. **Schema validation** — `config.toml` must conform to the expected schema
2. **Authority validation** — policies cannot expand root permissions
3. **Ignore validation** — no negation of root ignore patterns
4. **Path validation** — `[[registered_path]]` entries must exist on disk
5. **License validation** — SPDX identifiers must be valid (if `spdx_strict = true`)
6. **Branch protection merge** — tree rules are merged with root rules; conflicts logged

Validation errors are reported via:
- CLI output during `wt tree validate`
- bgprocess logs
- Server-side rejection during sync (for authority violations)

---

## 12. Version Control

The `.wt-tree/` directory is **version-controlled** — it is tracked by W0rkTree and included
in snapshots. This means:

- Changes to tree configuration are auditable in history
- Configuration can be branched and merged alongside code
- Policy changes go through the same review process as code (if branch protection requires it)
- The server has the authoritative copy and rejects invalid configurations during sync

---