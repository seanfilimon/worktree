# `.wt/` Root Worktree Configuration Specification

## Overview

The `.wt/` folder lives at the root of every W0rkTree worktree. It is the single source of
truth for worktree-wide configuration and serves as the **root-level authority** for:

- **Identity** — authentication tokens and local user overrides.
- **Server connection** — which W0rkTree server this worktree syncs with.
- **Access control** — custom roles and RBAC/ABAC policies that define the permission ceiling.
- **Configuration** — sync behaviour, auto-snapshot rules, large-file handling, shallow clone
  settings, branch protection, registered paths, and license metadata.
- **Ignore patterns** — authoritative ignore rules that no child tree can negate.
- **Reflog** — per-branch and global operation history.
- **Hooks** — pre/post-snapshot scripts that apply across every tree.
- **Conflicts** — machine-readable merge-conflict metadata.
- **Cache** — local, bgprocess-managed ephemeral data.

### Cardinal Rule

`.wt/` defines the **ceiling**. No child tree's `.wt-tree/` can expand permissions, negate
root ignore patterns, or override server/tenant identity. Child trees may only **restrict
further** within the bounds the root establishes.

---

## Folder Structure

```
.wt/
├── config.toml              # Root config: server, sync, tenant, registered paths,
│                            #   licenses, large files, branch protection, reflog, shallow
├── ignore                   # Root ignore patterns (authoritative, cannot be negated by trees)
│
├── identity/
│   ├── token                # Auth token for server connection (JWT or API key)
│   └── identity.toml        # Local user identity (name, email override)
│
├── access/
│   ├── roles.toml           # Custom role definitions (ceiling for all trees)
│   └── policies.toml        # Root-level access policies (RBAC + ABAC)
│
├── hooks/
│   ├── pre-snapshot         # Runs before snapshot creation (exit non-zero to abort)
│   └── post-snapshot        # Runs after snapshot creation (informational)
│
├── reflog/
│   ├── main.log             # Per-branch reflog
│   ├── feature/oauth.log    # Nested branch reflog
│   └── _global.log          # Global reflog (all branches, all operations)
│
├── conflicts/               # Machine-readable conflict metadata (during merge)
│   └── *.conflict.json      # One file per conflicting path
│
└── cache/                   # Local cache (bgprocess-managed, ephemeral)
    └── ...
```

### File Ownership

| Path | Written by | Synced to server |
|------|-----------|-----------------|
| `config.toml` | User / CLI | Yes |
| `ignore` | User / CLI | Yes |
| `identity/token` | CLI (`wt auth login`) | **No** — local only |
| `identity/identity.toml` | User / CLI | Yes |
| `access/roles.toml` | User / CLI (requires PolicyManage) | Yes |
| `access/policies.toml` | User / CLI (requires PolicyManage) | Yes |
| `hooks/*` | User / CLI | Yes |
| `reflog/*` | bgprocess | Configurable |
| `conflicts/*` | bgprocess | No — local only |
| `cache/*` | bgprocess | No — local only |

---

## `config.toml` — Full Reference

Below is the **complete** `config.toml` with every supported section and key. Optional keys
show their default values in comments.

```toml
# ──────────────────────────────────────────────
#  Worktree identity
# ──────────────────────────────────────────────

[worktree]
name = "my-project"                         # Human-readable worktree name
server = "https://wt.company.com"           # W0rkTree server URL
tenant = "acme-corp"                        # Tenant (organisation) slug
visibility = "private"                      # private | shared | public
                                            #   private  — only explicit grants
                                            #   shared   — visible to tenant members
                                            #   public   — readable by all authenticated users

# ──────────────────────────────────────────────
#  Sync behaviour
# ──────────────────────────────────────────────

[sync]
auto = true                                 # Enable automatic background sync
interval_secs = 30                          # Polling / push interval in seconds
retry_backoff_max_secs = 300                # Max exponential backoff on transient failure
conflict_strategy = "mark"                  # mark | ours | theirs — default merge strategy

# ──────────────────────────────────────────────
#  Auto-snapshot
# ──────────────────────────────────────────────

[auto_snapshot]
enabled = true                              # Automatically create snapshots
inactivity_timeout_secs = 300               # Seconds of inactivity before snapshot
max_changed_files = 50                      # Max dirty files before forced snapshot
on_tree_switch = true                       # Snapshot current tree when switching trees

# ──────────────────────────────────────────────
#  Large file handling
# ──────────────────────────────────────────────

[large_files]
threshold_bytes = 10485760                  # 10 MiB — files above this use chunked storage
chunk_size_bytes = 4194304                  # 4 MiB chunk size for upload/download
lazy_loading = true                         # Only fetch large blobs on access
preload_patterns = ["*.rs", "*.ts"]         # Glob patterns to eagerly preload

# ──────────────────────────────────────────────
#  Reflog
# ──────────────────────────────────────────────

[reflog]
retention_days = 90                         # Days to retain reflog entries locally
max_entries_per_branch = 10000              # Hard cap per branch log file
sync_to_server = true                       # Replicate reflog to server for recovery
compression = "zstd"                        # none | gzip | zstd

# ──────────────────────────────────────────────
#  Shallow clone settings
# ──────────────────────────────────────────────

[shallow]
enabled = false                             # Enable shallow mode for this worktree
default_depth = 50                          # Number of snapshots to fetch initially
auto_deepen = true                          # Automatically fetch more history on demand
lazy_blobs = true                           # Defer blob download until checkout

# ──────────────────────────────────────────────
#  License metadata
# ──────────────────────────────────────────────

[license]
default = "MIT"                             # Default SPDX identifier for the worktree
spdx_strict = true                          # Reject non-SPDX identifiers

# Per-path license overrides (repeatable)
[[license.path]]
path = "services/billing-engine"
license = "proprietary"

[[license.path]]
path = "libs/shared-utils"
license = "Apache-2.0"

# Cross-tenant license grants (repeatable)
[[license.grant]]
path = "services/billing-engine"
tenant = "partner-corp"
grant = "read-only"                         # read-only | compile-only | full

[[license.grant]]
path = "libs/shared-utils"
tenant = "*"                                # Wildcard — all tenants
grant = "full"

# ──────────────────────────────────────────────
#  Registered paths
# ──────────────────────────────────────────────
#  Registered paths are paths that are explicitly tracked for
#  policy enforcement, audit, and UI surfacing. They can be
#  referenced in access policies by scope = { path = "..." }.

[[registered_path]]
path = "config/production.toml"
description = "Production configuration"
sensitivity = "high"                        # low | medium | high | critical

[[registered_path]]
path = "secrets"
description = "Secrets directory"
sensitivity = "critical"

[[registered_path]]
path = "services/billing-engine/src/pricing.rs"
description = "Core pricing logic"
sensitivity = "high"

# ──────────────────────────────────────────────
#  Tenant access (worktree-level grants)
# ──────────────────────────────────────────────

[[tenant_access]]
tenant = "alice-dev"
permissions = ["tree:read", "branch:read"]

[[tenant_access]]
tenant = "partner-corp"
permissions = ["tree:read", "branch:read", "snapshot:read"]

# ──────────────────────────────────────────────
#  Branch protection rules
# ──────────────────────────────────────────────
#  Available rules:
#    no_direct_push      — cannot push snapshots directly
#    require_merge_review — must go through merge review flow
#    no_delete            — branch cannot be deleted
#    no_force_push        — cannot overwrite history
#    require_ci_pass      — (future) require CI green before merge

[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review", "no_delete", "no_force_push"]

[branch_protection.review]
min_approvals = 1
require_from_roles = ["Maintainer", "Admin"]
dismiss_stale_on_update = true              # Dismiss approvals when new snapshots arrive

[[branch_protection]]
branch = "release/*"
rules = ["no_direct_push", "require_merge_review", "no_delete"]

[branch_protection.review]
min_approvals = 2
require_from_roles = ["Maintainer", "Admin"]
```

### Key Validation Rules

1. `worktree.name` must be 1–128 characters, alphanumeric plus `-`, `_`, `.`.
2. `worktree.server` must be a valid HTTPS URL (HTTP allowed only for `localhost`).
3. `worktree.tenant` must be a valid tenant slug (lowercase alphanumeric plus `-`).
4. `license.default` must be a valid SPDX expression when `spdx_strict = true`.
5. `registered_path.path` must be relative and must not escape the worktree root.
6. `branch_protection.branch` supports literal names and trailing `*` globs only.
7. Numeric values (`interval_secs`, `threshold_bytes`, etc.) must be positive integers.

---

## `ignore` — Root Ignore Patterns

### Format

The ignore file follows `.gitignore` syntax with the following rules:

| Syntax | Meaning |
|--------|---------|
| `# comment` | Comment line (ignored) |
| `pattern` | Match files/dirs anywhere in the worktree |
| `/pattern` | Match relative to worktree root only |
| `pattern/` | Match directories only |
| `*` | Match any sequence of non-`/` characters |
| `**` | Match any sequence of characters including `/` |
| `?` | Match any single non-`/` character |
| `[abc]` | Character class |
| `!pattern` | Negate a previous pattern |

### Authoritativeness

Root `.wt/ignore` is **authoritative**:

- Patterns defined here **cannot** be negated by any child `.wt-tree/ignore`.
- If a child tree attempts to negate a root pattern with `!pattern`, bgprocess logs a
  warning and the root pattern wins silently.
- Negation (`!`) within the root ignore file itself works normally — you can un-ignore
  something you ignored earlier in the same file.

### Hard-Coded Ignores

The following patterns are **always active** and cannot be un-ignored by any ignore file:

```
.wt/
.wt-tree/
.git/
```

These are structural directories managed by W0rkTree or Git and must never be snapshotted
as regular content.

### Soft Defaults

The following patterns are active **by default** but can be explicitly un-ignored in
`.wt/ignore` (and only in `.wt/ignore`, not in tree-level ignores):

```
node_modules/
target/
__pycache__/
.DS_Store
Thumbs.db
*.pyc
*.pyo
*.swp
*.swo
*~
.env
.venv/
dist/
build/
*.log
```

To un-ignore a soft default, add an explicit negation in `.wt/ignore`:

```
# We want to track build output
!dist/
```

### Example `.wt/ignore`

```
# Compiled output
*.o
*.a
*.so
*.dll

# IDE files
.idea/
.vscode/
*.iml

# Secrets (NEVER snapshot)
*.pem
*.key
.env.*

# But allow .env.example
!.env.example

# OS junk
.DS_Store
Thumbs.db

# Test fixtures that are too large
tests/fixtures/large/
```

---

## `identity/` — Authentication & User Identity

### `identity/token`

A single-line file containing the authentication credential for the W0rkTree server.

- Format: JWT bearer token **or** opaque API key.
- Created by `wt auth login` — should **never** be edited by hand.
- File permissions: `0600` (owner read/write only). bgprocess warns if permissions are lax.
- **Not synced to server** — local to this machine only.
- **Not committed to snapshots** — hard-coded ignore ensures this.

### `identity/identity.toml`

Local user identity overrides for this specific worktree. When present, these values
override the global user config at `~/.config/w0rktree/config.toml`.

```toml
[user]
name = "Alice Engineer"
email = "alice@company.com"
signing_key = "ssh-ed25519 AAAA..."     # Optional — for snapshot signing
```

- `name` and `email` are embedded in snapshot metadata.
- `signing_key` is used for cryptographic snapshot signing when enabled.
- This file **is** synced to server and versioned.

---

## `access/` — Declarative Access Control

### Built-in Roles

The following roles always exist and cannot be deleted or redefined:

| Role | Description |
|------|-------------|
| **Owner** | Full control. Can transfer ownership, delete worktree. |
| **Admin** | Full control except ownership transfer and worktree deletion. |
| **Maintainer** | Manage branches, merge, manage trees. Cannot modify access policies. |
| **Developer** | Read/write to allowed trees and branches. |
| **Viewer** | Read-only access. |

### `access/roles.toml` — Custom Role Definitions

Custom roles extend the built-in set. They must be a **subset** of an existing role's
permissions — you cannot create a custom role with more power than Admin.

```toml
# A security reviewer who can read everything but only write to security-related paths
[[role]]
name = "security-reviewer"
inherits = "Viewer"                         # Start with Viewer permissions
description = "Can review and approve security-sensitive changes"

[[role.grant]]
permission = "merge:approve"
scope = "worktree"

[[role.grant]]
permission = "branch:read"
scope = "worktree"

# A CI bot role with minimal permissions
[[role]]
name = "ci-bot"
inherits = "Viewer"
description = "Automated CI/CD pipeline identity"

[[role.grant]]
permission = "snapshot:read"
scope = "worktree"

[[role.grant]]
permission = "branch:read"
scope = "worktree"

# An external auditor with read-only access and audit log visibility
[[role]]
name = "external-auditor"
inherits = "Viewer"
description = "External compliance auditor — read-only with audit trail"

[[role.grant]]
permission = "reflog:read"
scope = "worktree"
```

### `access/policies.toml` — RBAC + ABAC Policies

Policies bind subjects to permissions at specific scopes. The root policies file defines
the **ceiling** — tree-level policies can only restrict further.

```toml
# Grant the backend team Developer access to the entire worktree
[[policy]]
name = "backend-team-access"
subject = { team = "backend" }
role = "Developer"
scope = "worktree"
effect = "allow"

# Grant the security team the custom security-reviewer role
[[policy]]
name = "security-review-access"
subject = { team = "security" }
role = "security-reviewer"
scope = "worktree"
effect = "allow"

# Restrict the billing-engine to specific accounts
[[policy]]
name = "billing-engine-restrict"
subject = { role = "Developer" }
scope = { path = "services/billing-engine" }
effect = "deny"
unless = { team = ["billing", "platform"] }

# Allow all authenticated users to read shared libraries
[[policy]]
name = "shared-libs-read"
subject = "all_authenticated"
role = "Viewer"
scope = { path = "libs/*" }
effect = "allow"

# Deny all access to secrets unless you are Admin or Owner
[[policy]]
name = "secrets-lockdown"
subject = "all_authenticated"
scope = { path = "secrets" }
effect = "deny"
unless = { role = ["Admin", "Owner"] }
```

#### Subject Types

| Subject | Syntax | Example |
|---------|--------|---------|
| Specific account | `{ account = "alice" }` | Single user |
| Team | `{ team = "backend" }` | All members of a team |
| Role | `{ role = "Developer" }` | All users with a role |
| Tenant | `{ tenant = "partner-corp" }` | All users in a tenant |
| All authenticated | `"all_authenticated"` | Any logged-in user |

#### Scope Types

| Scope | Syntax | Example |
|-------|--------|---------|
| Worktree-wide | `"worktree"` | Entire worktree |
| Specific tree | `{ tree = "auth-service" }` | One tree |
| Specific branch | `{ branch = "main" }` | One branch |
| Specific path | `{ path = "secrets" }` | Registered path |

#### Policy Evaluation Order

1. Explicit **deny** rules are evaluated first — deny always wins.
2. **unless** clauses can exempt subjects from a deny.
3. **allow** rules are then evaluated — at least one must match.
4. If no allow rule matches, access is **implicitly denied**.
5. Tree-level policies run **after** root policies — they can only add denies or narrow allows.

### Modification Permissions

Only users with the `PolicyManage` permission (granted to Owner, Admin, or custom roles
with explicit grant) can modify `access/roles.toml` or `access/policies.toml`. Attempts
to modify these files without the correct permission are rejected by bgprocess before sync.

---

## `hooks/` — Pre/Post Snapshot Hooks

### Overview

Hooks are executable scripts or programs that run at defined points in the snapshot
lifecycle. Root-level hooks apply to **all** snapshots across every tree.

### `hooks/pre-snapshot`

- **When**: Runs immediately before a snapshot is created (manual or auto).
- **Working directory**: Worktree root.
- **Environment variables**:
  - `WT_HOOK=pre-snapshot`
  - `WT_BRANCH=<current branch>`
  - `WT_TREE=<tree name or "root">`
  - `WT_SNAPSHOT_TYPE=manual|auto`
  - `WT_CHANGED_FILES=<newline-separated list>`
- **Exit code**: Non-zero aborts the snapshot. stderr is shown to the user.
- **Timeout**: 30 seconds (configurable via `config.toml` in future).

### `hooks/post-snapshot`

- **When**: Runs immediately after a snapshot is successfully created.
- **Working directory**: Worktree root.
- **Environment variables**:
  - `WT_HOOK=post-snapshot`
  - `WT_BRANCH=<current branch>`
  - `WT_TREE=<tree name or "root">`
  - `WT_SNAPSHOT_ID=<new snapshot hash>`
  - `WT_SNAPSHOT_TYPE=manual|auto`
- **Exit code**: Informational only — non-zero is logged but does not roll back.

### Execution Order

When both root and tree hooks exist:

1. Root `hooks/pre-snapshot` runs first.
2. Tree `.wt-tree/hooks/pre-snapshot` runs second.
3. Snapshot is created.
4. Root `hooks/post-snapshot` runs first.
5. Tree `.wt-tree/hooks/post-snapshot` runs second.

If the root pre-snapshot hook fails, the tree hook is **not** executed and the snapshot
is aborted.

---

## `reflog/` — Operation History

### Purpose

The reflog records every ref-changing operation in the worktree. It enables:

- Undo / recovery of lost snapshots.
- Cross-machine history when synced to server.
- Audit trail for compliance.

### File Layout

- **Per-branch files**: `reflog/<branch-name>.log` (e.g., `reflog/main.log`,
  `reflog/feature/oauth.log`). Nested branches use directory separators.
- **Global log**: `reflog/_global.log` contains all operations across all branches.

### Log Entry Format

Each line is a single operation, tab-separated:

```
<ISO-8601 timestamp>\t<operation>\t<before_ref>\t<after_ref>\t<user>\t<message>
```

#### Operations

| Operation | Description |
|-----------|-------------|
| `snapshot` | New snapshot created |
| `merge` | Branch merged |
| `revert` | Snapshot reverted |
| `branch:create` | New branch created |
| `branch:delete` | Branch deleted |
| `branch:rename` | Branch renamed |
| `reset` | Ref reset (e.g., `wt reset --to`) |
| `sync:pull` | Pulled from server |
| `sync:push` | Pushed to server |

#### Example

```
2025-01-15T09:32:11Z	snapshot	abc1234	def5678	alice	auto-snapshot: 3 files changed
2025-01-15T10:01:44Z	merge	def5678	789abcd	bob	merge feature/oauth into main
2025-01-15T10:05:00Z	sync:push	789abcd	789abcd	alice	pushed main to server
```

### Retention

- Controlled by `config.toml` → `[reflog].retention_days` and `max_entries_per_branch`.
- bgprocess prunes entries older than `retention_days` during periodic maintenance.
- When `sync_to_server = true`, entries are replicated before pruning for remote recovery.

---

## `conflicts/` — Merge Conflict Metadata

### Purpose

When bgprocess detects conflicts during auto-merge or an explicit `wt merge`, it writes
structured conflict metadata to `conflicts/`. This enables tooling and UIs to present
conflicts in a machine-readable way.

### File Naming

Each conflicting file gets a corresponding `.conflict.json`:

```
conflicts/<path-with-dashes>.conflict.json
```

For example, a conflict in `src/auth/handler.rs` produces:
`conflicts/src-auth-handler.rs.conflict.json`

### Conflict JSON Schema

```json
{
  "file": "src/auth/handler.rs",
  "branch_ours": "main",
  "branch_theirs": "feature/oauth",
  "ancestor_ref": "abc1234",
  "ours_ref": "def5678",
  "theirs_ref": "789abcd",
  "hunks": [
    {
      "start_line": 42,
      "end_line": 58,
      "ours": "fn authenticate(token: &str) -> Result<User> {\n    ...\n}",
      "ancestor": "fn authenticate(token: &str) -> bool {\n    ...\n}",
      "theirs": "fn authenticate(token: &str, provider: Provider) -> Result<User> {\n    ...\n}"
    }
  ],
  "auto_resolvable": false,
  "created_at": "2025-01-15T10:02:00Z"
}
```

### Lifecycle

1. bgprocess writes `.conflict.json` files when conflicts are detected.
2. User resolves conflicts (manually or via tooling).
3. User runs `wt conflicts resolve` or creates a new snapshot.
4. bgprocess cleans up resolved `.conflict.json` files.

---

## `cache/` — Local Cache

### Purpose

bgprocess uses the `cache/` directory for ephemeral, machine-local data:

- Partial blob downloads in progress.
- Decompressed object cache for faster access.
- Index acceleration structures.

### Characteristics

- **Never synced** to server.
- **Never committed** to snapshots.
- Can be safely deleted at any time — bgprocess rebuilds as needed.
- Size is bounded by bgprocess configuration (default: 500 MiB).

---

## What `.wt/` Does NOT Contain

The following are explicitly **outside** the `.wt/` directory:

| Concern | Where it lives |
|---------|---------------|
| Object store (blobs, snapshots, trees) | bgprocess-managed external storage |
| Full version history | bgprocess + server |
| Large binary data | bgprocess chunked storage layer |
| Tree-specific config | `.wt-tree/` inside each tree |
| Global user config | `~/.config/w0rktree/config.toml` |
| Server-side enforcement | Server process |

---

## Initialisation

When `wt init` creates a new worktree, it scaffolds `.wt/` with:

1. `config.toml` — populated with user-provided name, server, tenant, and sensible defaults.
2. `ignore` — populated with soft defaults (commented, for visibility).
3. `identity/` — empty until `wt auth login` is run.
4. `access/roles.toml` — empty (built-in roles are implicit).
5. `access/policies.toml` — default policy granting Owner full access.
6. `hooks/` — empty directory.
7. `reflog/` — empty directory (bgprocess populates on first operation).
8. `conflicts/` — empty directory.
9. `cache/` — empty directory.

---

## Migration & Compatibility

- The `.wt/` folder includes an implicit format version derived from the W0rkTree CLI
  version that created it.
- When a newer CLI opens an older `.wt/`, it auto-migrates forward (non-destructive).
- When an older CLI opens a newer `.wt/`, it refuses with a clear upgrade message.
- `config.toml` is forward-compatible: unknown keys are preserved but ignored by older CLIs.