# Tree Specification

## Overview

A Tree is the fundamental unit of code organization in W0rkTree. Each tree represents a logically isolated portion of your codebase with its own snapshot history, branches, access rules, and configuration. Trees can be nested inside other trees, enabling hierarchical project structures that scale from small libraries to massive multi-service platforms.

Unlike Git's flat repository model where everything shares a single history, W0rkTree trees provide true isolation: creating a branch in one tree does not affect any other tree. Snapshots are scoped to the tree that produced them. Access control is enforced per-tree. Licensing can vary per-tree. Each tree evolves independently while remaining part of a coherent worktree.

Trees are the building blocks of W0rkTree's multi-tenant architecture. A single worktree can contain dozens or hundreds of trees, each owned by different teams, each with different branching strategies, each versioned and released on its own schedule — all without the organizational chaos that plagues Git monorepos.

## Core Concepts

### What is a Tree?

A Tree is:

- **A versioned namespace** with its own independent snapshot history
- **An isolated workspace** within a worktree, bounded by a directory
- **A container for branches** and their snapshot DAGs
- **A unit of synchronization** for distributed collaboration between bgprocess and server
- **A boundary for access control** with its own policies that restrict (never expand) root policies
- **A boundary for licensing** with per-tree and per-path license configuration
- **A dependency endpoint** that can depend on or be depended upon by other trees
- **A task management unit** with structured TODOs generated from cross-tree dependencies
- **A configuration scope** with its own `.wt-tree/` directory for tree-level settings

A tree is created explicitly with `wt tree add` or during initial worktree setup with `wt init`. Every worktree has at least one tree: the root tree. Additional trees can be added as subdirectories, each with their own `.wt-tree/` configuration.

### Key Differences from Git

#### Git's Flat Monorepo Problem

In traditional Git:

```
monorepo/
├── .git/
├── frontend/
├── backend/
├── mobile/
├── shared/
└── infrastructure/
```

**All directories share:**
- A single commit history — every commit touches the entire repo
- A single branching model — `feature-x` applies to everything
- All-or-nothing clone and checkout operations
- Tightly coupled changes — a single commit can modify frontend, backend, and infrastructure
- Merge conflicts across completely unrelated code paths
- No per-directory access control, licensing, or versioning

**Result**: As the repository grows, it becomes unwieldy. Teams step on each other. History is polluted with irrelevant changes. Performance degrades. Organizational structure is lost.

#### W0rkTree's Nested Tree Solution

In W0rkTree:

```
my-platform/
├── .wt/                     # Root worktree config
├── frontend/
│   └── .wt-tree/            # Independent tree
├── backend/
│   └── .wt-tree/            # Independent tree
├── mobile/
│   └── .wt-tree/            # Independent tree
├── shared/
│   └── .wt-tree/            # Independent tree
└── infrastructure/
    └── .wt-tree/            # Independent tree
```

**Each tree has:**
- Independent snapshot history — frontend snapshots contain only frontend changes
- Own branching strategy — backend can use trunk-based while frontend uses feature branches
- Isolated operations — `wt snapshot` in one tree does not touch another
- Decoupled changes — teams work in their own trees without interference
- Focused merge conflicts — conflicts only occur within the tree's own files
- Per-tree access control, licensing, and configuration

**Result**: Clean separation of concerns. Teams own their trees. History stays focused. Performance scales. Structure is maintained even as the organization grows.

### The Multi-Tenancy Advantage

Trees provide **true multi-tenancy** for code organization:

1. **Logical Isolation**: Each tree represents a distinct component, service, or library with clear boundaries.
2. **Independent Evolution**: Trees can be versioned, branched, and released on completely independent schedules.
3. **Selective Access**: Teams only need access to the trees they work on. The server enforces this per-tree.
4. **Team Ownership**: Different teams own different trees. Ownership is explicit in configuration, not implicit by convention.
5. **Flexible Workflows**: Each tree can use the branching strategy appropriate to its development cadence.
6. **Per-Tree Licensing**: Open-source components, proprietary code, and third-party dependencies can coexist with different licenses tracked per-tree and per-path.
7. **Structured Dependencies**: Trees, branches, and snapshots can declare explicit dependencies on other trees, with automatic task generation and status tracking.
8. **Partial Sync**: Developers can sync only the trees they need. Stub trees provide metadata without content until materialized.

## Tree Structure & Directory Layout

### Directory Layout

Each tree contains a `.wt-tree/` directory that holds all tree-level configuration. This directory is versioned and synced alongside the tree's content.

```
services/auth-service/
├── .wt-tree/
│   ├── config.toml           # Tree config: branch strategy, auto-snapshot, large file thresholds
│   ├── ignore                # Tree-level ignore patterns (additive to root .wt/ignore)
│   ├── access/
│   │   └── policies.toml    # Tree-level policies (can restrict, not expand root)
│   └── hooks/
│       ├── pre-snapshot      # Runs before a snapshot is created
│       └── post-snapshot     # Runs after a snapshot is created
├── src/
│   ├── main.rs
│   ├── auth/
│   │   ├── login.rs
│   │   └── session.rs
│   └── middleware/
│       └── jwt.rs
├── tests/
│   └── auth_test.rs
└── Cargo.toml
```

**Key distinction**: `.wt/` is the root worktree configuration directory (one per root worktree). `.wt-tree/` is the per-tree configuration directory (one per individual tree, including nested trees).

The root tree's `.wt/` directory contains:

```
my-platform/
├── .wt/
│   ├── config.toml           # Root worktree config (authoritative defaults)
│   ├── ignore                # Root ignore patterns (apply to all trees)
│   ├── access/
│   │   └── policies.toml    # Root access policies (trees can restrict, not expand)
│   ├── reflog/               # Per-branch reflog storage
│   │   ├── frontend/
│   │   │   ├── main.log
│   │   │   └── feature-auth.log
│   │   └── backend/
│   │       └── main.log
│   └── conflicts/            # Machine-readable conflict metadata
│       └── ...
├── frontend/
│   └── .wt-tree/             # Tree-level config
├── backend/
│   └── .wt-tree/             # Tree-level config
└── ...
```

### Tree Metadata

Every tree tracks the following metadata, stored in `.wt-tree/config.toml` and maintained by bgprocess:

- **name**: The tree's identifier within the worktree (e.g., `auth-service`, `frontend`, `shared-lib`)
- **parent**: The parent tree path if this is a nested tree, or `null` for top-level trees
- **created_at**: ISO 8601 timestamp of when the tree was created
- **created_by**: The tenant who created the tree
- **default_branch**: The default branch for this tree (typically `main`)
- **description**: Optional human-readable description of the tree's purpose

Example metadata in `.wt-tree/config.toml`:

```toml
[tree]
name = "auth-service"
parent = "services"
created_at = "2024-06-15T09:30:00Z"
created_by = "alice"
default_branch = "main"
description = "Authentication and authorization service"
```

## Branches (Per-Tree)

### What is a Branch?

A branch in a tree represents a named line of development. Unlike Git where branches are repository-wide, tree branches are:

- **Tree-scoped**: A branch exists only within its tree. Creating `feature-auth` in the frontend tree has no effect on the backend tree.
- **Independently managed**: Each tree can have different branching strategies, different numbers of branches, different protection rules.
- **Linkable across trees**: When features span multiple trees, branches can be linked for synchronized merging.
- **Tracked by bgprocess**: The bgprocess manages local branch state. The server maintains canonical branch state.

### Branch Structure

Each branch tracks the following:

```
Branch:
├── name: "feature-auth"
├── tree: "frontend"
├── tip_snapshot: "a3f8c2..."        # Latest snapshot on this branch
├── created_at: "2024-06-20T14:00:00Z"
├── created_by: "alice"
├── parent_branch: "main"            # Branch this was created from
├── linked_group: "feature-auth"     # Optional: linked branch group name
└── protection: [...]                # Optional: branch protection rules
```

### Branch Operations

#### Creating a Branch

```bash
wt branch create feature-auth

# Output:
# Created branch 'feature-auth' in tree 'frontend'
# Based on: main (snapshot a3f8c2...)
# Switched to branch 'feature-auth'
```

Branches are always created from the current branch's tip snapshot. The new branch starts at the same snapshot and diverges from there.

#### Switching Branches

```bash
wt branch switch main

# Output:
# Switched to branch 'main' in tree 'frontend'
# Working directory updated to snapshot a3f8c2...
```

When switching branches, bgprocess updates the working directory to reflect the target branch's tip snapshot. Any unsaved changes are auto-snapshotted on the current branch before switching.

#### Listing Branches

```bash
wt branch list

# Output:
# Tree: frontend
#
#   main               a3f8c2...  "Add rate limiting middleware"    (2 hours ago)
# * feature-auth       b7d1e9...  "Add OAuth2 provider support"    (5 minutes ago)
#   feature-redesign   c4a6f0...  "Update color palette"           (3 days ago)
```

#### Deleting a Branch

```bash
wt branch delete feature-redesign

# Output:
# Soft-deleted branch 'feature-redesign' in tree 'frontend'
# Recovery window: 30 days
# To restore: wt branch restore feature-redesign
```

Branch deletion is a soft delete. The branch and its snapshots remain recoverable for a configurable retention period. This is consistent with W0rkTree's append-only, non-destructive design.

### Branch Strategies Per Tree

Each tree can configure its branching strategy in `.wt-tree/config.toml`. This affects how bgprocess and the server enforce branch workflow rules.

```toml
[tree]
name = "auth-service"
branch_strategy = "feature-branch"
```

**Available strategies:**

| Strategy | Description |
|---|---|
| `feature-branch` | Developers create feature branches off `main`, merge back via review. Default strategy. |
| `trunk-based` | All work happens on `main` with short-lived branches. Emphasizes continuous integration. |
| `release-branch` | Long-lived release branches (`release/1.x`, `release/2.x`) with cherry-picks from `main`. |
| `custom` | No enforced strategy. Teams define their own workflow. |

The strategy is advisory for `custom` and enforced by the server for the other options. For example, `trunk-based` may reject branches older than a configurable threshold.

### Branch Protection

Branch protection rules are server-enforced. The bgprocess respects them locally but the server is the ultimate authority. Protection rules are configured per-branch in `.wt-tree/config.toml`:

```toml
[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review"]

[[branch_protection]]
branch = "release/*"
rules = ["no_direct_push", "require_merge_review", "require_ci_pass"]
```

**Available protection rules:**

| Rule | Description |
|---|---|
| `no_direct_push` | Snapshots cannot be pushed directly to this branch. Must go through merge. |
| `require_merge_review` | Merges into this branch require at least one review approval. |
| `require_ci_pass` | Merges into this branch require all CI checks to pass. |
| `require_signature` | All snapshots must be cryptographically signed. |
| `no_force_delete` | Branch cannot be deleted, even soft-deleted. |

### Linked Branches Across Trees

When a feature spans multiple trees, branches in those trees can be **linked**. Linked branches form a group that must merge together — preventing partial deployments where one tree's changes land without the others.

```toml
# In frontend/.wt-tree/config.toml
[[linked_branches]]
group = "feature-payment-flow"
branch = "feature-payment-flow"

# In backend/.wt-tree/config.toml
[[linked_branches]]
group = "feature-payment-flow"
branch = "feature-payment-api"
```

**Linked branch rules:**

1. A linked branch cannot merge independently. Attempting to merge `frontend/feature-payment-flow` into `frontend/main` will fail if the linked branches in other trees are not also ready.
2. The `wt branch merge-linked <group>` command merges all branches in the group atomically.
3. All branches in the group must pass their respective branch protection rules before the group can merge.
4. If any branch in the group has unresolved conflicts, the entire group merge is blocked.

```bash
# Attempt to merge a linked branch individually
wt branch merge feature-payment-flow

# Output:
# ✗ Cannot merge frontend/feature-payment-flow independently.
#   This branch is part of linked group 'feature-payment-flow':
#     - frontend/feature-payment-flow
#     - backend/feature-payment-api
#
#   Use: wt branch merge-linked feature-payment-flow

# Merge the entire linked group
wt branch merge-linked feature-payment-flow

# Output:
# Merging linked branch group: feature-payment-flow
#   Checking status...
#     ✓ frontend/feature-payment-flow: Ready
#     ✓ backend/feature-payment-api: Ready
#
#   Merging...
#     ✓ frontend/feature-payment-flow → frontend/main
#     ✓ backend/feature-payment-api → backend/main
#
# All linked branches merged successfully.
```

## Snapshots

### What is a Snapshot?

A Snapshot is an immutable, content-addressed record of a tree's state at a specific point in time. Snapshots are the fundamental unit of history in W0rkTree — they replace Git's commits with a clearer name and a richer structure.

**Key properties:**
- **Immutable**: Once created, a snapshot cannot be modified. There is no rebase, no amend, no history rewriting.
- **Content-addressed**: The snapshot's ID is derived from its content. Identical content always produces the same ID.
- **Tree-scoped**: A snapshot belongs to exactly one tree. It contains only changes relevant to that tree.
- **Branch-scoped**: A snapshot is created on a specific branch within a tree.
- **Append-only**: New snapshots are appended to the DAG. Old snapshots are never removed (except by retention policy).

### Snapshot Structure

```
Snapshot: a3f8c2d7e1b9...
├── metadata:
│   ├── id: "a3f8c2d7e1b9..."
│   ├── tree: "frontend"
│   ├── branch: "feature-auth"
│   ├── parent: "b7d1e9f3c2a8..."       # Previous snapshot (null for initial)
│   ├── parents: ["b7d1e9...", "c4a6f0..."]  # Multiple parents for merge snapshots
│   ├── author: "alice"
│   ├── timestamp: "2024-06-20T14:30:00Z"
│   ├── message: "Add OAuth2 provider support"
│   └── content_hash: "d4e5f6..."        # Hash of all tracked file contents
├── changes: [
│   ├── Change {
│   │   ├── path: "src/auth/oauth.rs"
│   │   ├── operation: Added
│   │   └── diff: {...}
│   │   }
│   ├── Change {
│   │   ├── path: "src/auth/login.rs"
│   │   ├── operation: Modified
│   │   └── diff: {...}
│   │   }
│   └── ...
│   ]
├── dependencies: [
│   ├── Dependency {
│   │   ├── tree: "backend"
│   │   ├── requirement: "Need POST /api/auth/oauth/callback endpoint"
│   │   ├── priority: "high"
│   │   ├── blocking: true
│   │   ├── status: "pending"
│   │   └── todo_branch: "backend/frontend-oauth-req"
│   │   }
│   └── ...
│   ]
├── revert_of: null                      # If this is a revert, references original snapshot
└── tags: ["auth-v1.0-rc1"]             # Tags pointing to this snapshot
```

### Snapshot Creation

Snapshots can be created in two ways:

**1. Automatic (bgprocess)**

The bgprocess watches the file system and creates snapshots automatically based on configurable rules. This is the default mode of operation — developers work normally, and bgprocess captures their progress without manual intervention.

```toml
# .wt-tree/config.toml
[auto_snapshot]
enabled = true
inactivity_timeout_secs = 300    # Snapshot after 5 minutes of inactivity
min_interval_secs = 60           # At least 60 seconds between auto-snapshots
```

Auto-snapshots are created with a generated message describing the changed files. They are immediately available locally and synced to the server as staged snapshots.

**2. Manual**

Developers can create explicit snapshots with descriptive messages:

```bash
wt snapshot -m "Add OAuth2 provider support"

# Output:
# Created snapshot a3f8c2d7 on frontend/feature-auth
#   M src/auth/oauth.rs
#   M src/auth/login.rs
#   A src/auth/providers/google.rs
```

Manual snapshots are also synced to the server as staged snapshots until explicitly pushed.

### Snapshot DAG

Snapshots form a directed acyclic graph (DAG) within each tree. The DAG represents the complete history of the tree.

```
Tree: frontend, Branch: main

a3f8c2 ← b7d1e9 ← c4a6f0 ← d9e2f1 (tip)
  │        │        │        │
  │        │        │        └─ "Add rate limiting"
  │        │        └─ "Implement caching layer"
  │        └─ "Update API client"
  └─ "Initial frontend structure"
```

**Branching:**

```
Tree: frontend

                      ┌─ feature-auth
                      │  e5f3a1 ← f8c2d7
                      │
main: a3f8c2 ← b7d1e9 ← c4a6f0 ← d9e2f1 (tip)
                         │
                         └─ feature-redesign
                            g1b4e8 ← h2c5f9
```

**Merge snapshots** have multiple parents, recording where two lines of development converged:

```
Merge Snapshot: j7k8l9
├── parents: ["f8c2d7", "d9e2f1"]
├── message: "Merge feature-auth into main"
└── changes: [...]  # Merged changes
```

### Snapshot Components

#### 1. Metadata

Every snapshot contains:

- **ID**: Content-addressed hash derived from the snapshot's contents
- **Tree**: Which tree this snapshot belongs to
- **Branch**: Which branch this snapshot was created on
- **Author**: The tenant who created the snapshot
- **Timestamp**: ISO 8601 timestamp of creation
- **Message**: Human-readable description of changes (auto-generated for auto-snapshots)
- **Parent(s)**: Reference(s) to the preceding snapshot(s) in the DAG
- **Content Hash**: Hash of all tracked file contents at this point in time

#### 2. Changes List

Each snapshot contains a list of file-level changes with full diffs:

```
Change:
├── path: "src/auth/oauth.rs"
├── operation: Added | Modified | Deleted | Renamed { from } | Copied { from }
└── diff: Diff { hunks }
```

Operations:

| Operation | Description |
|---|---|
| `Added` | New file created |
| `Modified` | Existing file changed |
| `Deleted` | File removed |
| `Renamed { from }` | File moved/renamed from another path |
| `Copied { from }` | File copied from another path |

#### 3. Diff Structure

Each change includes a detailed diff with hunks:

```
Diff:
├── old_content_hash: "abc123..."     # null for Added
├── new_content_hash: "def456..."     # null for Deleted
└── hunks: [
    Hunk {
        old_start: 10,
        old_lines: 5,
        new_start: 10,
        new_lines: 7,
        lines: [
            " unchanged line",
            "-removed line",
            "+added line",
            "+another added line",
            " unchanged line",
        ]
    },
    ...
]
```

#### 4. Dependencies

Optional cross-tree dependency declarations. See the Dependencies section for full details.

```
Dependency:
├── tree: "backend"
├── branch: "main"                    # Optional: specific branch
├── requirement: "Need endpoint X"    # Human-readable requirement
├── priority: "high"                  # high | medium | low
├── blocking: true                    # Whether this blocks merge
├── status: "pending"                 # pending | in_progress | completed | blocked
└── todo_branch: "backend/..."       # Auto-generated TODO branch name
```

#### 5. Revert Metadata

If this snapshot is a revert of a previous snapshot, it includes a reference to the original:

```
revert_of: "a3f8c2..."               # The snapshot being reverted
revert_reason: "Broke auth flow"      # Optional reason
```

Reverts in W0rkTree create a new snapshot with the inverse diff. History is never rewritten — the original snapshot remains in the DAG, and the revert snapshot is appended after it.

#### 6. Tag References

If any tags point to this snapshot, they are recorded:

```
tags: ["v2.3.0", "auth-service/v2.3"]
```

Tags are immutable named references. See the Tags & Releases section for details.

## Staged Snapshot Visibility (Per-Tree)

### How Staging Works

When bgprocess captures a snapshot (either automatic or manual), it syncs that snapshot to the server as **staged**. A staged snapshot is visible to the team but is **not yet part of the branch's canonical history**.

This creates a powerful collaboration primitive: team members can see what others are working on in real-time, without polluting the branch history with work-in-progress changes.

### Staged vs Pushed

| Aspect | Staged Snapshot | Pushed Snapshot |
|---|---|---|
| **Created by** | bgprocess (auto or manual) | Explicit `wt push` |
| **Visible to team** | Yes — via `wt status --team` | Yes — in branch history |
| **Part of branch history** | No | Yes |
| **Can be superseded** | Yes — next snapshot replaces it | No — immutable in DAG |
| **Server stores it** | Yes, temporarily | Yes, permanently |
| **Triggers branch protection** | No | Yes |

### Team Visibility

```bash
wt status --team

# Output:
# Tree: frontend
# Branch: main
#
# Your staged snapshots:
#   a3f8c2... "Add OAuth2 flow" (staged 5 minutes ago, not pushed)
#
# Team activity:
#   alice — feature-auth — staged 2 min ago
#     M src/auth/oauth.rs
#     A src/auth/providers/google.rs
#
#   bob — feature-redesign — staged 15 min ago
#     M src/components/Header.tsx
#     M src/styles/theme.css
#
#   carol — main — staged 1 hour ago
#     M src/api/client.rs
```

Team members can see:
- Who has staged snapshots in this tree
- Which branch they are working on
- Which files they are touching
- How recently they staged

This enables natural coordination: if you see someone else is modifying a file you plan to change, you can coordinate before conflicts arise.

### Staged Snapshot Lifecycle

1. **bgprocess creates snapshot** — Auto-snapshot or manual `wt snapshot`
2. **bgprocess syncs to server** — Snapshot appears as staged on the server
3. **Team can observe** — `wt status --team` shows staged activity
4. **Developer pushes** — `wt push` promotes staged snapshot(s) to branch history
5. **Server enforces rules** — Branch protection, access control, license compliance checked on push
6. **Snapshot becomes permanent** — Now part of the canonical branch DAG

## Tree Access Control

### Policy Files

Tree-level access control is defined in `.wt-tree/access/policies.toml`. These policies can **restrict** the root `.wt/access/policies.toml` but can **never expand** them. This is a fundamental security constraint — a tree cannot grant more access than the root worktree allows.

```toml
# .wt-tree/access/policies.toml

# Restrict tree access to specific tenants
[[policy]]
subject = { tenant = "alice" }
action = "write"
effect = "allow"

[[policy]]
subject = { tenant = "bob" }
action = "read"
effect = "allow"

# Deny all other access (implicit, but can be explicit)
[[policy]]
subject = "*"
action = "*"
effect = "deny"
```

### Policy Subjects

Policies can target different subject types:

| Subject Type | Syntax | Description |
|---|---|---|
| Tenant | `{ tenant = "alice" }` | A specific user |
| Team | `{ team = "backend-team" }` | A team within a tenant's organization |
| Role | `{ role = "reviewer" }` | Anyone with a specific role |
| Wildcard | `"*"` | Everyone |

### Cross-Tenant Policies

Trees can grant access to tenants from other organizations:

```toml
# Allow a contractor read access to this tree
[[policy]]
subject = { tenant = "contractor-jane" }
action = "read"
effect = "allow"
```

Cross-tenant policies are always explicit. There is no implicit sharing between tenants.

### Registered Paths

For fine-grained access within a tree, paths can be registered in `.wt-tree/config.toml`. Registered paths enable path-level access policies:

```toml
[[registered_path]]
path = "src/crypto"
description = "Security-sensitive cryptographic code"

[[registered_path]]
path = "src/billing"
description = "Payment processing logic"
```

Once a path is registered, policies can target it:

```toml
# .wt-tree/access/policies.toml

# Only security team can write to crypto code
[[policy]]
subject = { team = "security-team" }
path = "src/crypto"
action = "write"
effect = "allow"

[[policy]]
subject = "*"
path = "src/crypto"
action = "write"
effect = "deny"
```

### Scope Hierarchy

Access control is evaluated in order of specificity. More specific scopes take precedence:

```
RegisteredPath > Branch > Tree > Tenant > Global
```

1. **RegisteredPath**: Policies targeting a specific registered path within the tree
2. **Branch**: Policies targeting a specific branch (via branch protection rules)
3. **Tree**: Policies in `.wt-tree/access/policies.toml`
4. **Tenant**: Policies at the tenant/organization level
5. **Global**: Policies in `.wt/access/policies.toml` (root worktree)

A `deny` at any level overrides an `allow` at a less specific level. A tree-level `deny` cannot be overridden by a tree-level `allow` at a broader scope.

## Tree License Configuration

### Per-Tree Licensing

Each tree can specify its license in `.wt-tree/config.toml`. The server enforces license compliance on every sync — snapshots that violate license policy are rejected.

```toml
# .wt-tree/config.toml

[license]
spdx = "MIT"
```

### Per-Path Licensing

Within a tree, different paths can have different licenses:

```toml
# .wt-tree/config.toml

[license]
spdx = "MIT"

[[license.path_override]]
path = "vendor/openssl-wrapper"
spdx = "Apache-2.0"
description = "Vendored OpenSSL bindings under Apache license"

[[license.path_override]]
path = "src/proprietary"
spdx = "LicenseRef-Proprietary"
description = "Proprietary business logic — not open source"
```

### License Grants for Cross-Tenant Sharing

When sharing a tree with other tenants, license grants specify what the recipient is allowed to do:

```toml
# .wt-tree/config.toml

[[license.grant]]
tenant = "partner-corp"
usage = "internal"
redistribution = false
modification = true
```

### License Inheritance

Tree licenses interact with the root `.wt/config.toml` license as follows:

- If the tree specifies a license, it **overrides** the root license for that tree.
- If the tree does not specify a license, it **inherits** the root license.
- Per-path overrides within the tree always take precedence over the tree-level license.
- The server enforces that tree licenses are compatible with the root license when cross-tenant sharing is enabled.

## Ignore Patterns (Per-Tree)

### Tree-Level Ignore

Each tree can define ignore patterns in `.wt-tree/ignore`. These patterns tell bgprocess which files to exclude from snapshots.

```
# .wt-tree/ignore

# Build artifacts
target/
dist/
*.o
*.so

# IDE files
.idea/
.vscode/
*.swp

# Environment
.env
.env.local

# OS files
.DS_Store
Thumbs.db
```

### Additive Pattern Rules

Ignore patterns follow strict additive rules:

1. **Root `.wt/ignore` applies to all trees.** These patterns are the baseline.
2. **Tree `.wt-tree/ignore` adds patterns** for that tree. It can add new patterns but **cannot negate** patterns from root `.wt/ignore`.
3. **Nested subtree `.wt-tree/ignore` adds patterns** for the subtree. It can add new patterns but **cannot negate** parent tree or root patterns.

This ensures that ignore patterns can only become more restrictive as you move down the tree hierarchy. A subtree cannot "un-ignore" something that a parent tree has excluded.

### Pattern Compilation

The bgprocess compiles all applicable ignore patterns (root + tree + subtree) into an optimized matcher when the tree is loaded. This matcher is used by the file system watcher to filter events before they are processed, ensuring that ignored files never trigger snapshot creation.

## Dependencies in Trees, Branches, and Snapshots

### The Git Problem: Organizational Chaos

In Git, a developer can modify files across multiple unrelated subsections in a single commit:

```bash
# Developer touches everything in one commit:
Modified: frontend/src/auth/login.js
Modified: frontend/src/api/client.js
Modified: backend/src/auth/handlers.rs
Modified: backend/src/middleware/auth.rs
Modified: mobile/src/screens/Login.kt
Modified: shared/lib/auth-types.ts
Modified: infrastructure/nginx.conf
Modified: docs/api/authentication.md
```

**This creates organizational chaos in large teams:**
- Changes scattered across unrelated subsections
- No structured coordination between teams
- Hard to track what depends on what
- Merge conflicts across unrelated components
- No clear ownership or task management
- Manual project management in external tools (Jira, Linear, etc.)

### The W0rkTree Solution: Structured Dependencies

W0rkTree provides **explicit dependency management** at three levels: Trees, Branches, and Snapshots.

#### Level 1: Tree Dependencies

A tree can declare static dependencies on other trees in `.wt-tree/config.toml`:

```toml
# frontend/.wt-tree/config.toml

[[dependency]]
tree = "shared"
branch = "main"
version = ">=1.2.0"
required = true

[[dependency]]
tree = "backend"
branch = "main"
required = false
```

Tree dependencies establish that the frontend tree requires the shared tree at version >=1.2.0 to function. The server tracks these relationships and can notify teams when dependencies are updated.

#### Level 2: Branch Dependencies

Branches can depend on branches in other trees. This is the primary mechanism for cross-tree feature coordination:

```toml
# frontend/.wt-tree/branches/feature-dashboard.toml

[[dependency]]
tree = "backend"
branch = "feature-dashboard-api"
status = "pending"
blocking = true

[[dependency]]
tree = "design-system"
branch = "dashboard-components"
status = "completed"
blocking = false
linked = false
```

**When you create a branch dependency:**
1. W0rkTree tracks the relationship across trees
2. Can automatically create TODO branches in target trees
3. Updates status as dependencies are satisfied
4. Provides visibility into blockers
5. Optionally links branches for synchronized merging

#### Level 3: Snapshot Dependencies

Individual snapshots can declare fine-grained dependencies:

```bash
wt snapshot -m "Add dashboard UI" \
  --depend backend:"Need GET /api/dashboard endpoint" \
  --priority high \
  --blocking
```

This creates a snapshot with an explicit dependency record:

```
Snapshot: a3f8c2...
├── message: "Add dashboard UI"
└── dependencies:
    └── Dependency {
        tree: "backend",
        requirement: "Need GET /api/dashboard endpoint",
        details: [
            "Should return user stats",
            "Include last 30 days of activity",
            "Support pagination with ?page=N"
        ],
        priority: "high",
        blocking: true,
        status: "pending",
        todo_branch: "backend/frontend-dashboard-req"
    }
```

### Automatic TODO Branch Generation

When a snapshot in one tree declares a dependency on another tree, W0rkTree automatically generates a structured TODO in the target tree.

#### Example: Frontend Needs Backend Changes

**Developer in frontend tree:**

```bash
wt snapshot -m "Add analytics dashboard UI" \
  --depend backend:"Need POST /api/analytics/events endpoint" \
  --details "Accept: event_type, user_id, timestamp, metadata" \
  --details "Return: event_id, status" \
  --priority high \
  --blocking \
  --linked
```

**W0rkTree automatically creates a TODO in backend tree:**

```yaml
# Stored in server, visible via wt todo list in backend tree
todo_id: "frontend-feature-analytics-req-001"

from:
  tree: frontend
  branch: feature-analytics
  snapshot: a3f8c2d7e1b9
  author: alice
  timestamp: "2024-06-20T14:30:00Z"

requirement:
  title: "Need POST /api/analytics/events endpoint"
  description: |
    The frontend analytics dashboard needs a new endpoint
    to submit analytics events from the UI.
  details:
    - "Accept: event_type, user_id, timestamp, metadata"
    - "Return: event_id, status"
  priority: high
  blocking: true
  linked: true

context:
  related_files:
    - "frontend/src/analytics/Dashboard.tsx"
    - "frontend/src/analytics/api.ts"
  related_snapshots:
    - "a3f8c2d7: Add analytics dashboard UI"

status: pending
assigned_to: null
created: "2024-06-20T14:30:00Z"
updated: "2024-06-20T14:30:00Z"

notifications:
  - sent_to: "backend-team@example.com"
  - sent_to: "backend-tree-watchers"
```

**Backend team sees the TODO:**

```bash
wt todo list

# Output:
# PENDING TODOS (backend):
#
# 1. [HIGH][BLOCKING][LINKED] frontend/feature-analytics-req-001
#    Need POST /api/analytics/events endpoint
#    From: alice (frontend/feature-analytics)
#    Created: 5 minutes ago
#
# 2. [MEDIUM] mobile/user-avatar-req-003
#    Update user model to include avatar URL
#    From: bob (mobile/user-profile)
#    Created: 2 days ago
```

### Structured Project Management Per Tree

Each tree maintains its own task management system, integrated with version control.

#### Claiming TODOs

```bash
wt todo claim 1 --assign-to charlie

# Output:
# Claimed TODO frontend/feature-analytics-req-001
# Created branch: frontend-feature-analytics-req-001
# Switched to branch: frontend-feature-analytics-req-001
```

#### Working on TODOs

```bash
# Already on the TODO branch
# ... make changes ...
wt snapshot -m "Add analytics events endpoint"

# Link the snapshot to the TODO
wt todo link 1
```

#### Completing TODOs

```bash
wt todo complete 1 --snapshot a3f8c2

# W0rkTree automatically:
# 1. Updates the TODO status to "completed"
# 2. Notifies the frontend team
# 3. Updates the dependency status in frontend branch
# 4. Records which snapshot satisfied the requirement
```

**Frontend team receives notification:**

```bash
wt branch status feature-analytics

# Output:
# Branch: feature-analytics (frontend)
#
# Dependencies:
#   ✓ backend/feature-analytics-req-001 [COMPLETED]
#     Requirement: POST /api/analytics/events endpoint
#     Completed by: charlie
#     Snapshot: backend@a3f8c2d7
#     Completed: 10 minutes ago
#     Linked: YES
#
# All dependencies satisfied. Ready to merge.
```

### Linked Branches

When a dependency is created with `--linked`, the branches are grouped. Linked branches must merge together:

```bash
# Attempting to merge a linked branch individually fails
wt branch merge feature-analytics

# Output:
# ✗ Cannot merge frontend/feature-analytics independently.
#   Linked branches must merge together:
#     - backend/frontend-feature-analytics-req-001
#
#   Use: wt branch merge-linked feature-analytics

# Merge the entire linked group atomically
wt branch merge-linked feature-analytics

# Output:
# Merging linked branch group: feature-analytics
#   Checking status...
#     ✓ frontend/feature-analytics: Ready
#     ✓ backend/frontend-feature-analytics-req-001: Ready
#
#   Merging...
#     ✓ frontend/feature-analytics → frontend/main
#     ✓ backend/frontend-feature-analytics-req-001 → backend/main
#
# All linked branches merged successfully.
```

### Cross-Tree Coordination Workflow

Complete workflow showing how trees coordinate:

**Step 1: Frontend creates feature branch**
```bash
# In frontend tree
wt branch create feature-user-profile
wt snapshot -m "Add user profile page UI"
```

**Step 2: Frontend declares dependency with linked branch**
```bash
wt snapshot -m "Wire up profile API client" \
  --depend backend:"Need GET /api/users/:id/profile endpoint" \
  --details "Should include: name, email, avatar, bio, joined_date" \
  --priority high \
  --blocking \
  --linked
```

**Step 3: Backend receives TODO automatically**
```bash
# In backend tree
wt todo list
# Shows new TODO from frontend

wt todo claim 1
# Creates and switches to: frontend-feature-user-profile-req branch
```

**Step 4: Backend implements and completes**
```bash
# ... implement the endpoint ...
wt snapshot -m "Add user profile endpoint"
wt todo complete 1
# Notifies frontend automatically
```

**Step 5: Frontend can now merge (linked)**
```bash
# In frontend tree
wt deps check
# Output: All dependencies satisfied

wt branch merge-linked feature-user-profile
# Merges both frontend and backend branches atomically
```

### Dependency Visualization

```bash
wt deps graph

# Output:
# Tree Dependency Graph
#
# frontend
# ├── Tree Dependencies:
# │   └── shared@main (>=1.2.0) [SATISFIED]
# │
# ├── Branch: feature-analytics
# │   └─► backend: POST /api/analytics/events [PENDING][LINKED]
# │       TODO: backend/frontend-feature-analytics-req-001
# │
# └── Branch: feature-user-profile [LINKED]
#     └─► backend: GET /api/users/:id/profile [COMPLETED][LINKED]
#         Completed by: charlie (backend@a3f8c2)
#
# backend
# ├── Outstanding TODOs: 2 pending
# │   ├── frontend/feature-analytics-req-001 [PENDING][LINKED]
# │   └── mobile/push-notifications-req-005 [IN_PROGRESS]
# │
# └── Branch: feature-real-time [LINKED]
#     └─► infrastructure: WebSocket service [PENDING][LINKED]
#
# LINKED BRANCH GROUPS:
#   1: frontend/feature-analytics ⟷ backend/frontend-feature-analytics-req-001
#   2: frontend/feature-user-profile ⟷ backend/frontend-feature-user-profile-req
#   3: backend/feature-real-time ⟷ infrastructure/websocket-service
```

## Tags & Releases (Tree Context)

### Tags

Tags are immutable named references to snapshots. Since snapshots belong to specific trees and branches, tags inherit that context.

**Tag naming** supports tree-scoped names for clarity:

```bash
wt tag create v2.3.0 --snapshot a3f8c2

# Output:
# Created tag 'v2.3.0' on frontend/main at snapshot a3f8c2
```

```bash
wt tag create auth-service/v2.3 --snapshot b7d1e9

# Output:
# Created tag 'auth-service/v2.3' on auth-service/main at snapshot b7d1e9
```

**Tag structure:**

```
Tag:
├── name: "auth-service/v2.3"
├── tree: "auth-service"
├── branch: "main"
├── snapshot: "b7d1e9..."
├── created_by: "alice"
├── created_at: "2024-06-25T10:00:00Z"
└── message: "Auth service release 2.3 — adds OAuth2 support"
```

Tags cannot be moved or deleted (immutable). To correct a mistake, create a new tag.

### Releases

Releases build on tags with additional metadata and artifact storage:

```bash
wt release create auth-service/v2.3 \
  --tag auth-service/v2.3 \
  --notes "## What's New\n- OAuth2 provider support\n- Session refresh improvements" \
  --artifact dist/auth-service-2.3.tar.gz \
  --artifact dist/auth-service-2.3-checksums.txt
```

**Release structure:**

```
Release:
├── tag: "auth-service/v2.3"
├── tree: "auth-service"
├── notes: "## What's New\n..."
├── artifacts: [
│   ├── "dist/auth-service-2.3.tar.gz"
│   └── "dist/auth-service-2.3-checksums.txt"
│   ]
├── created_by: "alice"
└── created_at: "2024-06-25T10:05:00Z"
```

Releases are stored on the server and are accessible to any tenant with read access to the tree.

## Merge & Conflict Resolution (Tree Context)

### Auto-Merge by BGProcess

The bgprocess can automatically merge non-conflicting changes within a tree. When two branches have diverged but their changes do not overlap, bgprocess performs the merge without manual intervention:

```bash
wt branch merge feature-auth

# Output:
# Auto-merging feature-auth into main...
#   Analyzing changes...
#     feature-auth: 3 files changed
#     main: 1 file changed (no overlap)
#   ✓ Auto-merge successful
#   Created merge snapshot: d9e2f1...
```

### Conflict Detection

When changes overlap, bgprocess detects the conflict and halts the merge:

```bash
wt branch merge feature-auth

# Output:
# Merging feature-auth into main...
#   Analyzing changes...
#   ✗ CONFLICT in src/auth/login.rs
#     Both branches modified lines 45-60
#
# Merge paused. Resolve conflicts and run: wt merge continue
```

### Conflict Markers

W0rkTree uses improved conflict markers with clear labels:

```
<<<<<<< main (snapshot d9e2f1)
    let session = create_session(user_id, Duration::hours(24));
=======
    let session = create_session(user_id, Duration::hours(12));
    session.set_refresh(true);
>>>>>>> feature-auth (snapshot f8c2d7)
```

### Machine-Readable Conflict Metadata

In addition to in-file markers, W0rkTree stores machine-readable conflict metadata in `.wt/conflicts/`:

```toml
# .wt/conflicts/src-auth-login-rs.toml

[conflict]
file = "src/auth/login.rs"
tree = "frontend"
ours_branch = "main"
ours_snapshot = "d9e2f1..."
theirs_branch = "feature-auth"
theirs_snapshot = "f8c2d7..."

[[hunk]]
start_line = 45
end_line = 60
ours_content = "    let session = create_session(user_id, Duration::hours(24));\n"
theirs_content = "    let session = create_session(user_id, Duration::hours(12));\n    session.set_refresh(true);\n"
```

This enables IDE integrations and automated tools to parse and present conflicts programmatically.

### Merge Strategies

| Strategy | Description |
|---|---|
| `auto` | BGProcess auto-merges if no conflicts. Halts on conflict. Default. |
| `manual` | Always halt and require manual review, even if no conflicts. |
| `ours` | On conflict, keep the target branch's version. |
| `theirs` | On conflict, keep the source branch's version. |

```bash
wt branch merge feature-auth --strategy theirs
```

### Binary File Conflicts

Binary files (images, compiled assets, etc.) cannot be merged with textual diff. Binary file conflicts always require manual resolution:

```bash
# Output on binary conflict:
# ✗ CONFLICT (binary) in assets/logo.png
#   Cannot auto-merge binary files.
#   Keep ours:   wt merge resolve assets/logo.png --ours
#   Keep theirs: wt merge resolve assets/logo.png --theirs
#   Replace:     wt merge resolve assets/logo.png --file new-logo.png
```

## Large File Handling (Tree Context)

### Configurable Threshold

Each tree can configure the large file threshold in `.wt-tree/config.toml`. Files exceeding this threshold are stored using chunked storage with lazy loading — no external LFS system needed.

```toml
# .wt-tree/config.toml

[large_files]
threshold_bytes = 5242880    # 5 MB
```

### Chunked Storage

Large files are split into content-addressed chunks. Only chunks that differ between snapshots are stored and synced. This means:

- Appending to a large log file only stores the new chunks
- Modifying a section of a binary file only stores the affected chunks
- Downloading a tree does not require downloading all large file history — only the current version

### Lazy Loading

Large files are not downloaded until they are needed. When syncing a tree, bgprocess downloads metadata for large files but defers content download until the file is accessed:

```bash
wt sync

# Output:
# Syncing frontend...
#   Downloaded: 47 files (1.2 MB)
#   Lazy: 3 large files (450 MB total, download on access)
```

### Root Authority

The root `.wt/config.toml` settings for large files are authoritative. Tree-level settings can adjust thresholds within the bounds set by the root:

- If root sets `max_file_size = 100MB`, a tree cannot increase this to 200MB.
- If root sets `threshold_bytes = 10MB`, a tree can lower its threshold to 5MB but not raise it to 20MB.

## Reflog (Tree Context)

### Per-Branch Reflog

W0rkTree maintains a reflog for every branch in every tree. The reflog records every operation that changed the branch's tip snapshot:

```
# .wt/reflog/frontend/main.log

d9e2f1 → e5f3a1  merge    "Merge feature-auth into main"            2024-06-25T10:30:00Z  alice
c4a6f0 → d9e2f1  snapshot "Add rate limiting middleware"             2024-06-25T09:15:00Z  bob
b7d1e9 → c4a6f0  snapshot "Implement caching layer"                 2024-06-24T16:00:00Z  alice
a3f8c2 → b7d1e9  snapshot "Update API client"                       2024-06-24T14:30:00Z  carol
```

### Viewing the Reflog

```bash
wt reflog

# Output:
# Tree: frontend, Branch: main
#
# e5f3a1 (HEAD) merge    "Merge feature-auth into main"     (2 hours ago)
# d9e2f1         snapshot "Add rate limiting middleware"      (3 hours ago)
# c4a6f0         snapshot "Implement caching layer"          (yesterday)
# b7d1e9         snapshot "Update API client"                (yesterday)
# a3f8c2         snapshot "Initial frontend structure"       (3 days ago)
```

### Tree-Level Retention Override

Each tree can configure reflog retention in `.wt-tree/config.toml`:

```toml
# .wt-tree/config.toml

[reflog]
retention_days = 180    # Keep reflog entries for 180 days (default: 90)
```

The root `.wt/config.toml` sets the default retention. Trees can extend or shorten retention within server-configured bounds.

## Shallow History & Partial Sync (Tree Context)

### Partial Tree Sync

When initializing a worktree from a remote source, developers can choose to sync only specific trees:

```bash
wt init --from https://wt.example.com/my-platform --trees frontend,shared

# Output:
# Initializing worktree from https://wt.example.com/my-platform
#   Syncing tree: frontend (full)
#   Syncing tree: shared (full)
#   Stubbing tree: backend (metadata only)
#   Stubbing tree: mobile (metadata only)
#   Stubbing tree: infrastructure (metadata only)
#
# Worktree ready. 2 trees synced, 3 stub trees.
```

### Stub Trees

Trees that are not fully synced exist as **stub trees**. A stub tree contains:

- Tree metadata (name, branches, latest snapshot references)
- `.wt-tree/config.toml` (for dependency resolution)
- No file content

Stub trees allow the worktree to maintain a complete picture of the project structure and dependencies without downloading content the developer does not need.

```bash
wt tree list

# Output:
# Trees in my-platform:
#
#   frontend       [synced]  12 branches, 347 snapshots
#   shared         [synced]   3 branches,  89 snapshots
#   backend        [stub]    15 branches, 502 snapshots
#   mobile         [stub]     8 branches, 201 snapshots
#   infrastructure [stub]     4 branches,  56 snapshots
```

### Materializing a Stub Tree

When a developer needs to work on a stub tree, they materialize it:

```bash
wt sync --tree backend

# Output:
# Materializing stub tree: backend
#   Downloading content... 4.7 MB
#   Syncing branches... 15 branches
#   ✓ backend is now fully synced
```

### Shallow History

For large trees with long histories, developers can sync with limited history depth:

```bash
wt init --from https://wt.example.com/my-platform --trees frontend --depth 50

# Output:
# Syncing frontend with shallow history (50 snapshots)
#   Downloaded: 50 of 347 snapshots
#   Older history available on demand
```

Shallow history can be deepened later:

```bash
wt sync --tree frontend --deepen 100

# Output:
# Deepening frontend history by 100 snapshots
#   Now have: 150 of 347 snapshots
```

## Snapshot History

### Linear History

The simplest case: a sequence of snapshots on a single branch, each pointing to its parent.

```
Tree: backend, Branch: main

a3f8c2 ← b7d1e9 ← c4a6f0 ← d9e2f1 (tip)
  │        │        │        │
  │        │        │        └─ "Add rate limiting"
  │        │        └─ "Implement caching"
  │        └─ "Update API endpoints"
  └─ "Initial backend structure"
```

### Branching History

When branches diverge, the DAG splits:

```
Tree: frontend

                      ┌─ feature-auth
                      │  e5f3a1 ← f8c2d7
                      │
main: a3f8c2 ← b7d1e9 ← c4a6f0 ← d9e2f1 (tip)
                         │
                         └─ feature-redesign
                            g1b4e8 ← h2c5f9
```

Each branch has its own independent line of snapshots. The branch point is recorded in the DAG — you can always trace where a branch diverged.

### Merge Snapshots

Merge snapshots have multiple parents, recording where two branches converged:

```
                      ┌─ feature-auth ──────────────┐
                      │  e5f3a1 ← f8c2d7            │
                      │                              ▼
main: a3f8c2 ← b7d1e9 ← c4a6f0 ← d9e2f1 ← j7k8l9 (merge)
```

```
Merge Snapshot: j7k8l9
├── parents: ["f8c2d7", "d9e2f1"]
├── message: "Merge feature-auth into main"
└── changes: [...]
```

### Append-Only Guarantee

W0rkTree's snapshot history is **append-only**. There is no rebase. There is no `--force` push. There is no history rewriting.

- **No rebase**: Rebase rewrites history. W0rkTree does not rewrite history. Period.
- **No amend**: Once a snapshot is created, it is immutable. Create a new snapshot instead.
- **No force push**: The server rejects any attempt to rewrite branch history.
- **Revert, not reset**: To undo a change, create a new snapshot with the inverse diff. The original snapshot remains in the DAG.

This append-only guarantee means that the snapshot DAG is a reliable audit trail. You can always trace exactly what happened, when, and by whom.

## Cross-Tree Relationships

### Tree-Level Dependencies

Static dependencies between trees declared in `.wt-tree/config.toml`:

```toml
# frontend/.wt-tree/config.toml

[[dependency]]
tree = "shared"
branch = "main"
version = ">=1.2.0"
required = true

[[dependency]]
tree = "design-system"
branch = "v2"
snapshot = "def456"
required = false
```

Tree-level dependencies are long-lived and structural. They express that one tree fundamentally depends on another.

### Branch-Level Dependencies

Dynamic dependencies for features in development:

```toml
# frontend/.wt-tree/branches/feature-dashboard.toml

[[dependency]]
tree = "backend"
branch = "feature-dashboard-api"
status = "pending"
blocking = true
todo_created = true
linked = true
```

Branch-level dependencies are transient — they exist for the lifetime of the feature branch and are resolved when the branch merges.

### Snapshot-Level Dependencies

Fine-grained requirements declared on individual snapshots:

```
Snapshot: a3f8c2...
└── dependencies:
    └── Dependency {
        tree: "backend",
        requirement: "Need authentication endpoint",
        priority: "high",
        blocking: true,
        status: "pending",
        todo_branch: "backend/frontend-auth-req"
    }
```

Snapshot-level dependencies create the most specific and actionable cross-tree requirements.

### Change Propagation

When a depended-upon tree changes, dependent trees can:

1. **Be notified** of upstream changes via the server
2. **Sync updates** selectively for specific trees or branches
3. **Test compatibility** before merging upstream changes
4. **Maintain stable versions** until the team is ready to upgrade
5. **Receive TODO completion notifications** when dependencies are satisfied
6. **Track blockers** automatically through the dependency graph

## Operations

### Creating a Snapshot

```bash
# Manual snapshot with message
wt snapshot -m "Add login functionality"

# Snapshot with dependencies
wt snapshot -m "Add dashboard UI" \
  --depend backend:"Need GET /api/dashboard endpoint" \
  --priority high \
  --blocking

# Auto-snapshot (bgprocess creates these automatically)
# No command needed — bgprocess watches for changes
```

### Viewing Snapshot History

```bash
wt log

# Output:
# Tree: frontend, Branch: main
#
# a3f8c2d7  alice   2024-06-25 14:23   Add OAuth2 provider support
# b7d1e9f3  bob     2024-06-24 09:15   Update API client
# c4a6f0e2  alice   2024-06-23 16:00   Implement caching layer
# d9e2f1a8  carol   2024-06-22 11:30   Initial frontend structure
```

### Viewing a Specific Snapshot

```bash
wt show a3f8c2

# Output:
# Snapshot: a3f8c2d7e1b9
# Tree:     frontend
# Branch:   feature-auth
# Author:   alice
# Date:     2024-06-25 14:23:00
#
#     Add OAuth2 provider support
#
# Changes:
#   A src/auth/providers/google.rs
#   M src/auth/login.rs
#   M src/auth/oauth.rs
#
# Dependencies:
#   → backend: "Need POST /api/auth/oauth/callback endpoint" [PENDING]
#
# [diff details...]
```

### Comparing Snapshots

```bash
wt diff a3f8c2 b7d1e9

# Shows the diff between two snapshots in the same tree
```

### Reverting a Snapshot

```bash
wt revert a3f8c2

# Output:
# Created revert snapshot e5f3a1
#   Reverts: a3f8c2 "Add OAuth2 provider support"
#   Inverse diff applied to: src/auth/providers/google.rs, src/auth/login.rs, src/auth/oauth.rs
```

Revert creates a **new snapshot** with the inverse diff. The original snapshot `a3f8c2` remains in the DAG — history is never rewritten.

### Managing TODOs

```bash
# View pending TODOs for this tree
wt todo list

# Claim a TODO (creates a branch automatically)
wt todo claim 1

# Link a snapshot to a TODO
wt todo link 1

# Complete a TODO
wt todo complete 1 --snapshot a3f8c2

# View dependency status for current branch
wt deps status

# Visualize the full dependency graph
wt deps graph
```

## Benefits of Tree-Based Versioning

### 1. Clean Separation

Each tree maintains its own snapshot history. Frontend snapshots contain only frontend changes. Backend snapshots contain only backend changes. There is no pollution from unrelated changes.

### 2. Focused Diffs

When you view a snapshot's diff, you see only changes relevant to that tree. No scrolling past hundreds of unrelated file changes to find the three files you care about.

### 3. Independent Versioning

Trees can have different version numbers, release cycles, and stability guarantees. The auth-service can release v3.0 while the notification-service is still on v1.2 — and that is fine.

### 4. Parallel Development

Multiple teams work in different trees without constant merge conflicts. The frontend team's rapid iteration does not create conflicts for the backend team's careful refactoring.

### 5. Selective History

View and analyze history at the tree level. When debugging a backend issue, you see only backend snapshots — not the 500 frontend snapshots from this week.

### 6. Better Performance

Operations are scoped to a single tree. Syncing, searching history, and creating snapshots are faster because they operate on a focused subset of the worktree.

### 7. Structured Project Management

The built-in TODO system with automatic task generation from dependencies replaces ad-hoc coordination. Requirements flow from tree to tree with structured metadata, not Slack messages.

### 8. Prevents Organizational Chaos

Unlike Git where a single developer can modify files across every subsection in one commit, trees enforce boundaries. Changes stay within their tree. Cross-tree coordination is explicit and tracked.

### 9. Synchronized Multi-Tree Features

Linked branches ensure that features spanning multiple trees are deployed atomically. No more "the frontend deployed but the backend API isn't ready yet."

### 10. Coordinated Releases

Linked branches must merge together. Release coordination is built into the version control system, not managed in a spreadsheet.

## Use Case Examples

### Microservices

```
Worktree: my-platform

Tree: auth-service
├── .wt-tree/
├── Branch: main
│   └── Snapshots: Service-specific auth changes
├── Branch: add-oauth
│   └── Snapshots: OAuth2 implementation
└── Releases: auth-service/v1.0, auth-service/v2.0

Tree: payment-service
├── .wt-tree/
├── Branch: main
│   └── Snapshots: Payment processing changes
├── Branch: stripe-integration
│   └── Snapshots: Stripe API work
└── Releases: payment-service/v1.0
```

Each microservice evolves independently with its own version history, release cadence, and team ownership.

### Frontend/Backend Split

```
Worktree: webapp

Tree: frontend
├── .wt-tree/ (branch_strategy = "feature-branch")
├── Branch: main (release every 2 weeks)
├── Branch: feature-redesign
└── Snapshots: UI changes only

Tree: backend
├── .wt-tree/ (branch_strategy = "trunk-based")
├── Branch: main (release monthly)
├── Branch: feature-new-api
└── Snapshots: API changes only
```

Different release cadences, different branching strategies, no cross-pollination of changes.

### Component Libraries

```
Worktree: design-system

Tree: core-components
├── .wt-tree/ (strict branch protection on main)
├── Stable, versioned releases
└── Semantic versioning: v1.0, v1.1, v2.0

Tree: experimental
├── .wt-tree/ (branch_strategy = "trunk-based")
├── Bleeding edge features
└── Fast iteration, no stability guarantees

Tree: documentation
├── .wt-tree/
├── Docs updates independent of code
└── Continuous deployment on every snapshot
```

Clear boundaries between stability levels. Breaking changes in experimental do not affect core-components.

### Large Organization

```
Worktree: megacorp-platform

Tree: payments-service
├── Branch: feature-subscriptions
│   └── Dependency: billing-service needs invoice API [LINKED]
│       Auto-created TODO: billing-service/payments-subscriptions-req
├── TODOs: 3 pending from other teams
└── Snapshots: Payment processing only

Tree: billing-service
├── Branch: payments-subscriptions-req (TODO branch)
│   └── Working on: Invoice API endpoint
├── TODOs: 5 pending from other teams
└── Snapshots: Billing logic only

Tree: notifications-service
├── Dependencies on: payments-service, billing-service
├── TODOs: 2 pending requests
└── Snapshots: Notification logic only
```

Each tree maintains its own tasks and dependencies, preventing the organizational chaos that plagues Git monorepos with dozens of teams.

### Cross-Tree Feature with Linked Branches

```
Worktree: saas-platform

Linked Branch Group: feature-payment-flow
├── Tree: frontend
│   └── Branch: feature-payment-flow [LINKED]
│       └── Snapshots: Payment UI, checkout form
│
├── Tree: backend
│   └── Branch: feature-payment-api [LINKED]
│       └── Snapshots: Payment API, webhooks
│
├── Tree: mobile
│   └── Branch: feature-payment-flow [LINKED]
│       └── Snapshots: Mobile payment screens
│
└── Tree: notification-service
    └── Branch: payment-notifications [LINKED]
        └── Snapshots: Payment confirmation emails
```

```bash
# All branches must merge together
wt branch merge-linked feature-payment-flow

# Output:
# Merging linked branch group: feature-payment-flow
#   Checking status...
#     ✓ frontend/feature-payment-flow: Ready
#     ✓ backend/feature-payment-api: Ready
#     ✓ mobile/feature-payment-flow: Ready
#     ✓ notification-service/payment-notifications: Ready
#
#   Merging...
#     ✓ frontend/feature-payment-flow → frontend/main
#     ✓ backend/feature-payment-api → backend/main
#     ✓ mobile/feature-payment-flow → mobile/main
#     ✓ notification-service/payment-notifications → notification-service/main
#
# Complete payment feature deployed atomically across all trees.
```

This prevents the all-too-common scenario where the frontend deploys a new checkout flow but the backend payment API is still on the old version, or the mobile app updates but the notification service hasn't been updated to send the new confirmation emails.

## Configuration (Tree-Level)

### Full `.wt-tree/config.toml` Example

```toml
# =============================================================================
# Tree Configuration — auth-service
# =============================================================================

[tree]
name = "auth-service"
parent = "services"
created_at = "2024-06-15T09:30:00Z"
created_by = "alice"
default_branch = "main"
description = "Authentication and authorization service"
branch_strategy = "feature-branch"    # feature-branch | trunk-based | release-branch | custom

# -----------------------------------------------------------------------------
# Auto-Snapshot Configuration
# -----------------------------------------------------------------------------

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 300    # Snapshot after 5 min of inactivity
min_interval_secs = 60           # At least 60 seconds between auto-snapshots

# -----------------------------------------------------------------------------
# Large File Handling
# -----------------------------------------------------------------------------

[large_files]
threshold_bytes = 5242880        # 5 MB — files above this use chunked storage

# -----------------------------------------------------------------------------
# Reflog Retention
# -----------------------------------------------------------------------------

[reflog]
retention_days = 180             # Keep reflog entries for 180 days

# -----------------------------------------------------------------------------
# License
# -----------------------------------------------------------------------------

[license]
spdx = "MIT"

[[license.path_override]]
path = "vendor/openssl-wrapper"
spdx = "Apache-2.0"
description = "Vendored OpenSSL bindings"

# -----------------------------------------------------------------------------
# Branch Protection
# -----------------------------------------------------------------------------

[[branch_protection]]
branch = "main"
rules = ["no_direct_push", "require_merge_review"]

[[branch_protection]]
branch = "release/*"
rules = ["no_direct_push", "require_merge_review", "require_ci_pass"]

# -----------------------------------------------------------------------------
# Linked Branches
# -----------------------------------------------------------------------------

[[linked_branches]]
group = "feature-oauth"
branch = "feature-oauth"

# -----------------------------------------------------------------------------
# Tree Dependencies
# -----------------------------------------------------------------------------

[[dependency]]
tree = "shared"
branch = "main"
version = ">=1.2.0"
required = true

[[dependency]]
tree = "user-service"
branch = "main"
required = false

# -----------------------------------------------------------------------------
# Registered Paths (for fine-grained access control)
# -----------------------------------------------------------------------------

[[registered_path]]
path = "src/crypto"
description = "Security-sensitive cryptographic code"

[[registered_path]]
path = "src/tokens"
description = "Token generation and validation"
```

### Configuration Hierarchy

Configuration flows from global to specific, with more specific scopes overriding broader ones:

```
System defaults
  └── User config (~/.wt/config.toml)
       └── Root worktree config (.wt/config.toml)
            └── Tree config (.wt-tree/config.toml)
                 └── Subtree config (nested .wt-tree/config.toml)
```

**Override rules:**
- More specific config overrides less specific config for the same key.
- Trees can restrict root policies but cannot expand them (access control, ignore patterns).
- Trees can lower large file thresholds but cannot raise them above root limits.
- Trees can extend reflog retention but cannot exceed server-configured maximums.

## Conclusion

Trees are the architectural foundation of W0rkTree. They provide the isolation, structure, and coordination primitives that make large-scale, multi-team software development manageable.

By making snapshots and branches tree-scoped rather than repository-scoped, and by adding explicit dependency management at tree, branch, and snapshot levels, W0rkTree enables:

- **Clean isolation** without physical separation — all trees coexist in one worktree
- **Independent histories** without losing coordination — trees evolve separately but coordinate explicitly
- **Flexible workflows** without forcing uniformity — each tree uses the branching strategy that fits
- **Scalable structure** without performance degradation — operations are scoped to individual trees
- **Automatic task generation** for cross-tree coordination — dependencies create actionable TODOs
- **Linked branches** for synchronized multi-tree features — no partial deployments
- **Append-only history** with full auditability — no rebase, no force push, no history rewriting
- **Per-tree access control and licensing** — security and compliance at the right granularity
- **Staged snapshot visibility** — team awareness without polluting branch history
- **Partial sync and stub trees** — developers work with only what they need

The tree model transforms version control from a flat file-tracking system into a structured, multi-tenant platform for collaborative software development.