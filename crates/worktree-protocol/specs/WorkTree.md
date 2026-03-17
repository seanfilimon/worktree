# W0rkTree Specification

> **Version:** 0.2.0 — Complete Rewrite
> **Status:** Authoritative
> **Terminology:** "W0rkTree" (marketing, with zero), `worktree` (code, all lowercase)

---

## Table of Contents

1. [Overview](#overview)
2. [Core Concepts](#core-concepts)
3. [Architecture — The Two Runtimes](#architecture--the-two-runtimes)
4. [Worktree Structure](#worktree-structure)
5. [Configuration Hierarchy](#configuration-hierarchy)
6. [Tenants & Cross-Tenant Access](#tenants--cross-tenant-access)
7. [Dependency System](#dependency-system)
8. [Tags & Releases](#tags--releases)
9. [Merge & Conflict Resolution](#merge--conflict-resolution)
10. [License Compliance](#license-compliance)
11. [Ignore Patterns](#ignore-patterns)
12. [Large File Handling](#large-file-handling)
13. [Shallow History & Partial Sync](#shallow-history--partial-sync)
14. [Branch Protection](#branch-protection)
15. [Reflog](#reflog)
16. [Revert](#revert)
17. [Archiving](#archiving)
18. [Diff System](#diff-system)
19. [Git Compatibility](#git-compatibility)
20. [Comparison with Git](#comparison-with-git)
21. [Use Cases](#use-cases)
22. [Design Philosophy](#design-philosophy)
23. [Benefits](#benefits)

---

## 1. Overview

W0rkTree is a next-generation distributed version control system designed to **replace Git entirely**. It introduces multi-tenant code organization through Trees, with built-in structured project management, real-time team visibility via staged snapshots, declarative access control, file-level license compliance, and cross-tree coordination.

W0rkTree is **not** a Git wrapper. It is **not** a Git hosting platform. It is **not** a layer on top of Git. W0rkTree is its own system with its own protocol, its own storage model, its own identity system, and its own history model. It speaks Git when needed — for migration and interop — but Git is a compatibility target, not a dependency.

**What W0rkTree provides that Git does not:**

- **Multi-tenant architecture** — Users and organizations are first-class entities with verified identity, cross-tenant access grants, and per-tree visibility modes.
- **Two-runtime model** — A local background process (`worktree-bgprocess`) handles file watching and auto-snapshots. A remote server (`worktree-server`) handles canonical history, access control, and compliance enforcement.
- **Staged snapshot visibility** — Your team can see what you are working on in real-time, without you pushing incomplete work to a branch.
- **Declarative access control** — TOML-based, version-controlled, Terraform-style access policies at every level of the tree hierarchy.
- **File-level license compliance** — Per-path SPDX license tracking, server-enforced on every sync.
- **Nested trees** — Trees inside trees, each with independent versioning, branches, and access rules. No submodules. No subtree merges.
- **Append-only history** — No rebase. No force-push. No history rewriting. Ever.
- **Native large file handling** — Chunked storage with lazy loading. No LFS. No configuration. It just works.

---

## 2. Core Concepts

### What is a W0rkTree?

A W0rkTree is the **top-level organizational unit**. It contains one or more Trees, each with independent versioning. A W0rkTree is owned by a Tenant on the W0rkTree server.

Think of a W0rkTree as the outer container — the boundary that defines a project, a product, or an organization's codebase. Inside that container, individual Trees hold the actual code, each with their own snapshot history, branches, and access rules.

Every W0rkTree has a single `.wt/` directory at its root. This directory contains configuration, access rules, identity, hooks, and the reflog for the root tree.

### Trees

Trees are the **fundamental unit of code organization** in W0rkTree. Each tree:

- Has its own snapshot history, independent of sibling trees
- Has its own branches, independent of sibling trees
- Has its own access rules via `.wt-tree/access/`
- Has its own ignore patterns via `.wt-tree/ignore`
- Can be synced independently of other trees in the same W0rkTree
- Can contain nested trees (subtrees), creating a hierarchy

Trees replace Git repositories, Git submodules, and monorepo directory conventions with a single, consistent model. A microservices project might have one tree per service. A multi-platform app might have trees for frontend, backend, mobile, and shared libraries. Each tree gets its own `.wt-tree/` folder for configuration and access control.

### Snapshots (not commits)

Snapshots are **immutable, content-addressed records** of the complete state of a tree at a point in time. They are created automatically by the bgprocess as the developer works, or manually via `wt snapshot`.

Key properties of snapshots:

- **Immutable** — Once created, a snapshot cannot be changed.
- **Content-addressed** — The snapshot's identity is derived from its content. Two identical states produce the same snapshot hash.
- **Complete state** — A snapshot captures the full tree state, not a delta. (Storage uses deltas internally, but the logical model is complete state.)
- **Append-only** — Snapshots are only ever added to history, never removed or reordered.

There is no staging area. There is no index. There is no `add` command. The bgprocess watches the filesystem and creates snapshots of the working state. The developer's job is to write code, not to curate a staging area.

### Branches

Branches are **named pointers to snapshot chains** within a tree. Each tree has independent branches — a branch in one tree has no relationship to a branch of the same name in another tree unless explicitly linked.

Every tree has a default branch (typically `main`). Branches are presented in the UX as "workstreams" — visible streams of work with clear history, not confusing pointers that move around.

Branch operations:

- `wt branch create <name>` — Create a new branch from the current snapshot
- `wt branch switch <name>` — Switch the working directory to a branch
- `wt branch list` — Show all branches in the current tree
- `wt branch delete <name>` — Soft-delete a branch (recoverable via reflog)

Branches can be **linked** across trees. Linked branches must be merged together — if `frontend/feature-oauth` is merged to `main`, then `backend/feature-oauth` must also be merged to `main`. This prevents partial feature deployments across trees.

### Tenants

A **tenant** is a user or organization on the W0rkTree server with a verified identity:

- **Username (slug)** — A unique, URL-safe identifier. Used in paths and access grants.
- **Email** — A verified email address. Used for notifications and identity verification.
- **Type** — Either `user` (individual) or `org` (organization with multiple members).

Tenants own W0rkTrees. Tenants can grant cross-tenant access to specific trees. Tenants are the unit of authentication and authorization throughout the system.

### Staged Snapshots — The Key Innovation

This is the feature that changes how teams work together.

In Git, your work is invisible to your team until you push. There is no middle ground — either you push (polluting branch history with WIP) or you stay silent (leaving your team in the dark).

W0rkTree introduces **staged snapshots** as a visibility layer between local work and branch history:

1. The **bgprocess** creates snapshots automatically as you work.
2. These snapshots are synced to the server as **staged snapshots**.
3. Staged snapshots are **visible to the team** — your colleagues can see what files you have changed, how far along you are, and what branch you are working on.
4. Staged snapshots are **NOT part of branch history**. They do not pollute the branch. They are a separate visibility layer.
5. When you are ready, you explicitly run `wt push` to **finalize** your work into the branch history.
6. Alternatively, you can discard staged snapshots without any trace in branch history.

**Staged ≠ Pushed.** This distinction is fundamental. Staged snapshots solve the "what is everyone working on?" problem without requiring standups, status messages, or invasive monitoring. The server aggregates staged snapshots across all team members and presents a real-time view of active work.

---

## 3. Architecture — The Two Runtimes

W0rkTree is split into two cooperating runtimes. Neither is optional. The local process handles everything on the developer's machine. The server handles everything shared.

```
┌──────────────────────────────────────────────────────┐
│                  Developer Machine                    │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │            worktree-bgprocess                   │  │
│  │                                                 │  │
│  │  • Filesystem watcher (real-time)               │  │
│  │  • Auto-snapshot engine                         │  │
│  │  • Local snapshot history & DAG                 │  │
│  │  • Branch management (create, switch, merge)    │  │
│  │  • .wt/ and .wt-tree/ folder management         │  │
│  │  • Staged snapshot sync (upload to server)      │  │
│  │  • Remote branch sync (download from server)    │  │
│  │  • Large file chunking & lazy loading           │  │
│  │  • Auto-merge for non-conflicting changes       │  │
│  │  • Platform-native data storage                 │  │
│  └──────────────────────┬──────────────────────────┘  │
│                         │                             │
└─────────────────────────┼─────────────────────────────┘
                          │  W0rkTree Sync Protocol
                          │  (native + Git compat layer)
┌─────────────────────────┼─────────────────────────────┐
│                         │                             │
│  ┌──────────────────────┴──────────────────────────┐  │
│  │              worktree-server                    │  │
│  │                                                 │  │
│  │  • Canonical history storage (source of truth)  │  │
│  │  • Multi-tenant management                      │  │
│  │  • IAM: tenants, teams, roles, policies         │  │
│  │  • Cross-tenant access enforcement              │  │
│  │  • Staged snapshot aggregation & visibility      │  │
│  │  • Branch protection enforcement                │  │
│  │  • License compliance enforcement               │  │
│  │  • Merge request system                         │  │
│  │  • Tag & release management                     │  │
│  │  • CI/CD gate integration                       │  │
│  │  • API for admin panel, CLI, SDK                │  │
│  └─────────────────────────────────────────────────┘  │
│                                                       │
│                    Remote Server                       │
└───────────────────────────────────────────────────────┘
```

### BGProcess Responsibilities

The background process (`worktree-bgprocess`) runs continuously on the developer's machine. It is the **only process that touches the working directory**. The server never reads or writes files on the developer's machine.

| Responsibility | Details |
|---|---|
| **Filesystem watching** | Monitors the working directory for changes in real-time using OS-native APIs (inotify, FSEvents, ReadDirectoryChangesW). |
| **Auto-snapshot engine** | Automatically creates snapshots as the developer works. Configurable interval and change threshold. Manual snapshots via `wt snapshot` are also supported. |
| **Local history & DAG** | Maintains the full local snapshot history and directed acyclic graph. Branch pointers, parent relationships, and merge records are all local-first. |
| **Branch management** | Creates, switches, lists, deletes, and merges branches. All branch operations happen locally first, then sync to server. |
| **`.wt/` folder management** | Owns the root `.wt/` configuration directory. Reads config, applies settings, manages identity, hooks, and reflog. |
| **`.wt-tree/` folder management** | Manages per-tree `.wt-tree/` directories for all trees in the worktree. |
| **Staged snapshot sync** | Uploads staged snapshots to the server. These appear in the team visibility layer but do not become part of branch history until the developer runs `wt push`. |
| **Remote branch sync** | Downloads branch updates from the server. Keeps local branch pointers in sync with canonical server state. |
| **Large file chunking** | Splits large files into content-addressable chunks using FastCDC. Deduplication is automatic. |
| **Lazy loading** | Downloads file content on demand via FUSE (Linux/macOS) or ProjFS (Windows). Cloning a tree does not require downloading every file. |
| **Auto-merge** | Handles automatic merges for non-conflicting changes. Conflicts are surfaced to the developer with machine-readable metadata. |
| **Platform-native storage** | Stores all internal data in the platform-appropriate location: |
| | — Windows: `%APPDATA%\W0rkTree\` |
| | — Linux: `~/.local/share/w0rktree/` |
| | — macOS: `~/Library/Application Support/W0rkTree/` |

### Server Responsibilities

The server (`worktree-server`) is the **source of truth**. It is multi-tenant by design. The bgprocess syncs with the server but cannot bypass its enforcement.

| Responsibility | Details |
|---|---|
| **Canonical history** | The server's snapshot history is authoritative. If a local history diverges, the server wins. |
| **Multi-tenant hosting** | Users and organizations are first-class tenants with isolated data, configurable visibility, and cross-tenant access grants. |
| **IAM enforcement** | Tenants, teams, roles, and policies. All access rules defined in `.wt/access/` and `.wt-tree/access/` are enforced server-side. The bgprocess cannot bypass them. |
| **Cross-tenant access** | Enforces cross-tenant sharing rules. A tenant can grant read, write, or admin access to specific trees for other tenants. |
| **Staged snapshot aggregation** | Stores staged snapshots uploaded by all team members. Provides a unified view of active work across the worktree. |
| **Branch protection** | Enforces protection rules: required reviews, required CI, no direct push, snapshot signature requirements. Server-side enforcement — the bgprocess cannot override it. |
| **License compliance** | Enforces per-path SPDX license rules on every sync. Rejects snapshots that violate license policy. Controls export and redistribution. |
| **Merge request system** | Built-in merge requests with review, approval, and CI gate integration. No external platform required. |
| **Tag & release management** | Immutable tags and releases with artifact storage. Server enforces naming and uniqueness. |
| **API surface** | Exposes API for the admin panel, CLI tools, SDKs, and third-party integrations. |

### Separation of Concerns

This separation is a **core architectural constraint**, not an implementation detail:

- The bgprocess **never** enforces access control. It reads access config for local display purposes only.
- The server **never** watches files or touches the working directory.
- The bgprocess **never** stores canonical history. Its local history is a cache that syncs to the server.
- The server **never** creates auto-snapshots. It only stores what the bgprocess sends.

---

## 4. Worktree Structure

A W0rkTree has a root directory containing the `.wt/` folder and one or more trees, each with their own `.wt-tree/` folder.

### Directory Layout

```
my-project/                          ← Root W0rkTree
├── .wt/                             ← Root configuration (one per worktree)
│   ├── config.toml                  ← Root configuration file
│   ├── ignore                       ← Root ignore patterns (authoritative)
│   ├── identity/                    ← Tenant identity and credentials
│   │   ├── tenant.toml              ← Current tenant identity
│   │   └── keys/                    ← Signing keys
│   ├── access/                      ← Root access control (ceiling)
│   │   ├── roles.toml               ← Role definitions
│   │   └── policies.toml            ← Access policies
│   ├── hooks/                       ← Lifecycle hooks
│   │   ├── pre-snapshot.sh          ← Runs before snapshot creation
│   │   ├── post-snapshot.sh         ← Runs after snapshot creation
│   │   └── pre-push.sh             ← Runs before push to server
│   ├── conflicts/                   ← Machine-readable conflict metadata
│   └── reflog/                      ← Operation log for recovery
│       └── HEAD                     ← Reflog entries
├── services/
│   ├── auth-service/                ← Tree
│   │   ├── .wt-tree/               ← Tree-level configuration
│   │   │   ├── config.toml         ← Tree-specific config
│   │   │   ├── ignore              ← Tree-specific ignore patterns (additive)
│   │   │   └── access/             ← Tree-level access (can restrict, not expand)
│   │   │       ├── roles.toml
│   │   │       └── policies.toml
│   │   ├── src/
│   │   ├── tests/
│   │   └── Cargo.toml
│   └── api-gateway/                 ← Tree
│       ├── .wt-tree/
│       │   ├── config.toml
│       │   ├── ignore
│       │   └── access/
│       ├── src/
│       └── Cargo.toml
├── libs/
│   └── shared-models/               ← Tree
│       ├── .wt-tree/
│       │   ├── config.toml
│       │   └── access/
│       ├── src/
│       └── Cargo.toml
├── frontend/                        ← Tree
│   ├── .wt-tree/
│   │   ├── config.toml
│   │   ├── ignore
│   │   └── access/
│   ├── src/
│   └── package.json
└── README.md
```

### Key Structural Rules

- **One `.wt/` per worktree** — The `.wt/` directory exists only at the root. It defines the worktree boundary.
- **One `.wt-tree/` per tree** — Every tree (including nested subtrees) gets its own `.wt-tree/` directory.
- **No `.wt-tree/` at root** — The root worktree uses `.wt/`, not `.wt-tree/`. The root is implicitly a tree.
- **Nesting is recursive** — A tree can contain subtrees, which can contain subtrees. Each level gets its own `.wt-tree/`.
- **Nothing else in `.wt/` or `.wt-tree/`** — These directories are owned by the bgprocess. Do not put application files in them.

---

## 5. Configuration Hierarchy

W0rkTree configuration follows a strict hierarchy. Each level can override values from the level above, with one critical exception: **access permissions follow a ceiling model**.

### Precedence Order (lowest to highest)

```
1. System config         (machine-wide defaults, platform-specific location)
2. User global           (~/.config/w0rktree/config.toml)
3. Root .wt/config.toml  (worktree-level settings)
4. Tree .wt-tree/config.toml (tree-level overrides)
5. Subtree .wt-tree/config.toml (nested tree overrides)
6. Environment variables (highest priority, always wins)
```

Environment variables use the prefix `WT_` and replace dots with underscores. For example, `snapshot.auto_interval` becomes `WT_SNAPSHOT_AUTO_INTERVAL`.

### The Permission Ceiling Model

Access permissions do **not** follow the standard override hierarchy. They follow a **strict ceiling model**:

- **Root `.wt/access/`** = the ceiling. This defines the **maximum** permissions any entity can have across the entire worktree.
- **Tree `.wt-tree/access/`** = can **restrict** permissions below the root ceiling, but can **never expand** them beyond what the root allows.
- **Subtree `.wt-tree/access/`** = can restrict further below the parent tree's permissions, but can never expand beyond the parent.

This means:

- If the root denies write access to a tenant, no tree can grant it.
- If a tree restricts a role to read-only, no subtree can grant write.
- The server enforces the ceiling. A misconfigured `.wt-tree/access/` that attempts to grant more than the parent allows is rejected.

### Example Configuration

```toml
# .wt/config.toml — Root worktree configuration

[worktree]
name = "my-project"
tenant = "acme-corp"
visibility = "private"        # private | shared | public

[snapshot]
auto = true                   # bgprocess creates snapshots automatically
auto_interval = "30s"         # minimum interval between auto-snapshots
auto_threshold = 5            # minimum changed files to trigger auto-snapshot

[sync]
auto = true                   # bgprocess syncs staged snapshots automatically
server = "https://wt.acme.dev"

[branches]
default = "main"
```

```toml
# services/auth-service/.wt-tree/config.toml — Tree-level configuration

[tree]
name = "auth-service"

[snapshot]
auto_interval = "15s"         # More frequent snapshots for this tree

[branches]
default = "main"

[dependencies]
[dependencies.shared-models]
  path = "../../libs/shared-models"
  branch = "main"
  required = true
```

---

## 6. Tenants & Cross-Tenant Access

### Tenant Model

A **tenant** is the identity unit in W0rkTree. Every action — creating a snapshot, pushing to a branch, granting access — is performed by a tenant.

| Property | Description |
|---|---|
| `username` | Unique, URL-safe slug. Used in paths and access grants. Example: `acme-corp`, `alice`. |
| `email` | Verified email address. Used for notifications and identity verification. |
| `type` | `user` (individual) or `org` (organization with team members). |
| `created` | Timestamp of tenant registration. |
| `status` | `active`, `suspended`, or `deactivated`. |

### Worktree Visibility

Every worktree has a visibility mode that controls its default access:

| Mode | Behavior |
|---|---|
| **Private** (default) | Only the owning tenant and explicitly granted tenants can see it. |
| **Shared** | Visible to a defined set of tenants (e.g., all members of an organization). |
| **Public** | Visible to everyone. Snapshots can be synced by anyone. Write access still requires explicit grants. |

### Simple Tenant Access

For straightforward access grants, use the `tenant_access` table in `config.toml`:

```toml
# .wt/config.toml

[tenant_access]

[tenant_access.bob]
  role = "contributor"          # Can push to non-protected branches

[tenant_access.external-team]
  role = "reader"               # Read-only access
  trees = ["frontend", "docs"]  # Limited to specific trees

[tenant_access.devops-org]
  role = "admin"                # Full administrative access
```

### Full IAM Policies

For complex access scenarios, use the full IAM policy system in `.wt/access/policies.toml`:

```toml
# .wt/access/policies.toml

[[policy]]
name = "frontend-team-access"
description = "Frontend team has full access to frontend tree only"
effect = "allow"
principals = ["team:frontend-devs"]
actions = ["snapshot:create", "branch:create", "branch:push", "branch:merge"]
resources = ["tree:frontend/*"]

[[policy]]
name = "readonly-for-contractors"
description = "Contractors can read but not write"
effect = "allow"
principals = ["team:contractors"]
actions = ["snapshot:read", "branch:list", "tree:sync"]
resources = ["tree:*"]

[[policy]]
name = "deny-contractor-writes"
effect = "deny"
principals = ["team:contractors"]
actions = ["snapshot:create", "branch:push", "branch:merge"]
resources = ["tree:*"]
```

### Role Definitions

```toml
# .wt/access/roles.toml

[roles.reader]
description = "Read-only access"
permissions = ["snapshot:read", "branch:list", "tree:sync"]

[roles.contributor]
description = "Can create snapshots and push to non-protected branches"
permissions = ["snapshot:read", "snapshot:create", "branch:list", "branch:create", "branch:push"]

[roles.maintainer]
description = "Can merge to protected branches and manage tags"
permissions = ["snapshot:*", "branch:*", "tag:create", "tag:delete", "merge-request:merge"]

[roles.admin]
description = "Full access including access control management"
permissions = ["*"]
```

### Cross-Tenant Access

Tenants can grant access to other tenants — including tenants in other organizations:

```toml
# .wt/config.toml

[tenant_access.partner-org]
  role = "reader"
  trees = ["shared-api"]
  expires = "2025-12-31T23:59:59Z"  # Time-limited access
```

Cross-tenant access is enforced by the server. The bgprocess includes tenant identity in every sync request. The server validates every operation against the access policies before allowing it.

---

## 7. Dependency System

### The Problem with Git

In Git, a feature that spans multiple parts of the codebase produces a single branch with scattered changes:

```
# Git approach — chaotic in large organizations
git checkout -b big-feature

# Developer modifies:
frontend/src/app.js
backend/src/api.rs
mobile/src/main.kt
shared/lib/auth.js
docs/api.md

git commit -m "WIP: Big feature touching everything"
```

**Problems:**
- Changes scattered across unrelated subsystems
- In large organizations, this creates organizational chaos
- No way to track what depends on what
- No structured coordination between teams
- Manual project management required via external tools

### The W0rkTree Solution

W0rkTree provides **explicit dependency management** at three levels. The bgprocess manages dependency tracking locally. The server coordinates dependencies across tenants and teams.

#### Level 1: Tree Dependencies

A tree can depend on other trees. These are declared in the tree's `.wt-tree/config.toml`:

```toml
# frontend/.wt-tree/config.toml

[dependencies]
[dependencies.shared-models]
  path = "../../libs/shared-models"
  branch = "main"
  required = true

[dependencies.design-system]
  path = "../../libs/design-system"
  branch = "v2"
  required = false
```

Tree dependencies are resolved by the bgprocess at sync time. If a required dependency is not available, the bgprocess warns the developer. The server tracks dependency relationships across all trees in a worktree.

#### Level 2: Branch Dependencies

A branch can depend on branches in other trees:

```toml
# frontend branch: feature-new-ui
# Stored in branch metadata

[dependencies]
[dependencies."backend/feature-new-api"]
  status = "pending"
  blocking = true
  created_todo = true
  linked = true

[dependencies."design-system/tokens-v3"]
  status = "completed"
  blocking = false
  linked = false
```

#### Level 2.5: Linked Branches Across Trees

Branches can be **linked** across trees, creating synchronized merge requirements:

```toml
# Linked branches working together on the same feature
[linked_branches]
group = "oauth-implementation"
branches = [
  "frontend/feature-oauth",
  "backend/feature-oauth",
  "mobile/feature-oauth",
]
```

**When branches are linked:**
- All linked branches must be at the same hierarchical level.
- When one branch is merged to `main`, all linked branches must also merge to their respective `main` branches.
- If `frontend/feature-oauth` is merged to `frontend/main`, then `backend/feature-oauth` must merge to `backend/main`.
- The server enforces this. Attempting to merge one without the others produces an error.
- This prevents partial feature deployment across trees.

#### Level 3: Snapshot Dependencies

Individual snapshots can declare dependencies on work in other trees:

```toml
# Snapshot abc123 in frontend tree

[[dependencies]]
tree = "backend"
branch = "main"
requirement = "Need POST /api/v2/auth endpoint"
status = "pending"
todo_branch = "frontend/feature-auth-integration"

[[dependencies]]
tree = "design-system"
requirement = "Need updated color tokens"
priority = "medium"
status = "completed"
completed_by = "charlie"
```

### Automatic TODO Branch Generation

When a developer creates a feature that depends on changes in another tree, W0rkTree automatically generates structured tasks.

**Developer working in frontend tree:**

```bash
cd frontend
wt branch create feature-dashboard

# Make changes...
wt snapshot -m "Add dashboard UI"

# Realize backend changes are needed
wt depend add backend \
  --message "Need GET /api/dashboard endpoint" \
  --priority high \
  --linked true
```

**W0rkTree automatically:**

1. Creates a TODO branch in the backend tree: `frontend/feature-dashboard-req`
2. Links the branches together for synchronized merging
3. Adds structured task information:

```toml
# Stored in backend tree's TODO system

[todo]
id = "todo-2024-0115-001"

[todo.from]
tree = "frontend"
branch = "feature-dashboard"
snapshot = "abc123def456"
author = "Alice <alice@example.com>"
timestamp = "2024-01-15T14:30:00Z"

[todo.requirement]
title = "Need GET /api/dashboard endpoint"
description = "Frontend dashboard requires a backend endpoint for user dashboard data"
priority = "high"
blocking = true
linked = true
details = [
  "Return user dashboard data",
  "Include recent activity",
  "Support pagination",
]

[todo.status]
state = "pending"
assigned_to = ""
created = "2024-01-15T14:30:00Z"
updated = "2024-01-15T14:30:00Z"
```

4. Notifies the backend team via server-side notifications
5. Tracks the dependency in the frontend branch metadata
6. Updates status automatically when backend work completes

### Structured Project Management

Each tree maintains its own task list:

```bash
# View TODOs for current tree
wt todo list

# Output:
# PENDING TODOS:
#   1. [HIGH] frontend/feature-dashboard-req
#      Need GET /api/dashboard endpoint
#      Requested by: Alice (frontend)
#      Created: 2 hours ago
#
#   2. [MED] mobile/user-profile-req
#      Update user model to include avatar
#      Requested by: Bob (mobile)
#      Created: 1 day ago

# Claim a TODO
wt todo claim 1

# Mark TODO as complete
wt todo complete 1 --snapshot abc123
```

### Cross-Tree Workflow Example

**Frontend Team:** Needs new API endpoint.

```bash
cd frontend
wt branch create feature-analytics

# Work on frontend code...
wt snapshot -m "Add analytics UI"

# Create dependency on backend with linked branches
wt depend add backend \
  --message "Need POST /api/analytics/events endpoint" \
  --details "Should accept: user_id, event_type, metadata" \
  --priority high \
  --blocking true \
  --linked true
```

**Backend Team:** Receives structured task.

```bash
cd backend
wt todo list
# Shows: [HIGH] frontend/feature-analytics-req

wt todo claim 1
wt branch create api-analytics-events

# Implement the endpoint...
wt snapshot -m "Add analytics events endpoint"

# Mark the TODO complete
wt todo complete 1
# This automatically notifies frontend team
# and updates the dependency status
```

**Frontend Team:** Receives notification.

```bash
cd frontend
wt branch status feature-analytics

# Output:
# Branch: feature-analytics
# Dependencies:
#   ✓ backend/api-analytics-events (completed by Charlie)
#     Snapshot: def456
#     Completed: 10 minutes ago
#     Linked: YES
#
# Status: Ready to merge!
```

**Merging Linked Branches:**

```bash
# Linked branches must move together
cd frontend
wt merge-request create feature-analytics --target main

# Output:
# Merge request created for frontend/feature-analytics → frontend/main
#
# LINKED BRANCHES DETECTED:
#   backend/api-analytics-events → backend/main
#
# All linked branches must be merged to their respective main branches.
# Use: wt merge-request create-linked to create merge requests for all linked branches

wt merge-request create-linked

# Output:
# Created Merge Requests:
#   ✓ frontend/feature-analytics → frontend/main
#   ✓ backend/api-analytics-events → backend/main
#
# These merge requests are linked. Merging one will require all to be ready.
```

### Dependency Visualization

```bash
wt deps graph

# Output:
# W0rkTree Dependency Graph
#
# frontend/feature-dashboard
#   ├─► backend/api-dashboard-endpoint [PENDING]
#   └─► design-system/dashboard-components [COMPLETED]
#
# frontend/feature-analytics [LINKED]
#   ├─► backend/api-analytics-events [COMPLETED] [LINKED]
#   └─► shared/analytics-lib [IN_PROGRESS]
#
# backend/feature-real-time
#   └─► infrastructure/websocket-service [PENDING]
#
# LINKED BRANCH GROUPS:
#   Group 1: frontend/feature-analytics ⟷ backend/api-analytics-events
```

---

## 8. Tags & Releases

### Tags

Tags are **immutable named references** to specific snapshots. Once created, a tag cannot be moved or modified — only soft-deleted with a recovery window.

**Lightweight tags** — A name pointing to a snapshot:

```bash
wt tag create v1.0.0
wt tag create v1.0.0 --snapshot abc123   # Tag a specific snapshot
```

**Annotated tags** — A name with a message, author, and optional signature:

```bash
wt tag create v1.0.0 --annotate --message "First stable release"
wt tag create v1.0.0 --annotate --message "Signed release" --sign
```

**Tag properties:**
- Tags are **global to the worktree** — they are not scoped to a single tree.
- Tags sync between bgprocess and server. The server is the source of truth.
- Tag names must be unique within a worktree.
- Tags cannot be moved. To correct a mistag, delete the old tag and create a new one.

### Releases

Releases are a **first-class concept** built on top of tags. A release is a tag plus:

| Property | Description |
|---|---|
| `notes` | Markdown-formatted release notes. |
| `artifacts` | Attached binary files (build outputs, documentation, etc.). |
| `status` | `draft`, `pre-release`, or `stable`. |
| `created_by` | Tenant who created the release. |
| `created_at` | Timestamp. |

```bash
# Create a release from an existing tag
wt release create v1.0.0 \
  --notes "## What's New\n- Feature X\n- Bug fix Y" \
  --status stable \
  --artifact ./dist/app-v1.0.0.tar.gz \
  --artifact ./dist/app-v1.0.0.zip

# List releases
wt release list

# Update release status
wt release update v1.0.0 --status stable
```

Releases are managed by the server. The server enforces that release tags cannot be deleted while the release exists. Artifacts are stored server-side with content-addressing and deduplication.

---

## 9. Merge & Conflict Resolution

### Merge Model

W0rkTree has **one merge model**: merge. There is no rebase. There is no squash merge. There is no cherry-pick that rewrites history. History is append-only, and merges create a new snapshot that records the combination of two branches.

### Auto-Merge

The bgprocess handles automatic merges for **non-conflicting changes**:

- Changes to different files merge automatically.
- Changes to different sections of the same file merge automatically.
- The bgprocess uses a three-way merge algorithm with the common ancestor as the base.
- If auto-merge succeeds, a merge snapshot is created with both parent branches recorded.

### Conflict Resolution

When changes conflict, the bgprocess surfaces the conflict with **improved conflict markers** and **machine-readable metadata**:

**Conflict markers in the file:**

```
<<<<<<< current-branch (your changes)
function handleAuth(user) {
  return validateToken(user.token);
}
||||||| common-ancestor (original)
function handleAuth(user) {
  return checkAuth(user);
}
=======
function handleAuth(user) {
  return verifyCredentials(user.email, user.password);
}
>>>>>>> incoming-branch (their changes)
```

Key differences from Git:

- **Clear labels**: Each section identifies the branch name and role (your changes, original, their changes).
- **Three-way display**: The common ancestor is always shown (not optional like in Git).
- **Machine-readable metadata**: Conflicts are also recorded in `.wt/conflicts/`.

**Machine-readable conflict metadata:**

```toml
# .wt/conflicts/src-auth-handler.toml

[conflict]
file = "src/auth/handler.rs"
current_branch = "feature-new-auth"
incoming_branch = "main"
ancestor_snapshot = "abc123"
current_snapshot = "def456"
incoming_snapshot = "789abc"

[[conflict.hunks]]
start_line = 42
end_line = 48
type = "content"    # content | delete-modify | rename
```

### Merge Strategies

| Strategy | Description |
|---|---|
| **Auto** (default) | Three-way merge. Auto-resolve non-conflicts. Surface conflicts for manual resolution. |
| **Manual** | No auto-resolution. Every changed file requires explicit review. |
| **Ours** | In case of conflict, keep the current branch's version. |
| **Theirs** | In case of conflict, keep the incoming branch's version. |

```bash
wt merge feature-branch                    # Auto strategy (default)
wt merge feature-branch --strategy manual  # Manual review of all changes
wt merge feature-branch --strategy ours    # Prefer current branch on conflicts
wt merge feature-branch --strategy theirs  # Prefer incoming branch on conflicts
```

### No Rebase. Ever.

W0rkTree does not support rebase. History is a record of what happened, not a narrative to be edited. If you want a clean history, write clean snapshots. If you made a mistake, create a new snapshot that fixes it. The original mistake remains in history because that is what happened.

This is not a limitation. This is a design decision. Append-only history is:

- **Safer** — No accidental data loss from `rebase -i` or `reset --hard`.
- **Simpler** — One merge model, not four (merge, rebase, squash, cherry-pick).
- **Auditable** — The full history is always available for compliance and debugging.
- **Collaborative** — No force-push means no overwriting your teammate's work.

---

## 10. License Compliance

W0rkTree tracks licenses at the **file path level**, not the repository level. This enables fine-grained control over proprietary code, open-source modules, and mixed-license projects.

### Per-Path License Assignment

```toml
# .wt/config.toml

[licenses]
default = "MIT"                  # Default license for all files

[licenses.paths]
"src/core/**" = "MIT"
"src/enterprise/**" = "LicenseRef-Proprietary"
"vendor/openssl/**" = "Apache-2.0"
"docs/**" = "CC-BY-4.0"
```

License identifiers follow the [SPDX specification](https://spdx.org/licenses/). Complex expressions are supported:

```toml
[licenses.paths]
"src/hybrid/**" = "MIT OR Apache-2.0"
"src/combined/**" = "MIT AND CC-BY-4.0"
```

### Server Enforcement

The server enforces license compliance on every sync, export, fork, and copy operation:

- **Sync**: The server rejects snapshots that introduce files violating the license policy.
- **Export**: Proprietary paths are excluded from public exports unless explicitly allowed.
- **Fork**: Cross-tenant forks respect license boundaries. A public fork cannot include proprietary-licensed paths.
- **Copy**: Copying files between trees enforces license compatibility.

### License Grant Model

| Grant | Description |
|---|---|
| **Read-only** | Tenant can view the file but not modify or redistribute. |
| **Modify** | Tenant can view and modify the file. |
| **Redistribute** | Tenant can include the file in their own distributions. |

```toml
# .wt/access/policies.toml

[[policy]]
name = "enterprise-license"
effect = "allow"
principals = ["team:enterprise-customers"]
actions = ["file:read"]
resources = ["path:src/enterprise/**"]
# Note: no modify or redistribute — read-only license
```

### Dual Enforcement

Both **IAM** and **License** must pass for an operation to succeed:

1. IAM check: Does this tenant have permission to perform this action on this resource?
2. License check: Does the license for this path allow this action?

If either check fails, the operation is denied. This means a tenant with admin IAM permissions still cannot redistribute a file with a read-only license grant.

---

## 11. Ignore Patterns

W0rkTree uses a hierarchical ignore system with gitignore-compatible syntax.

### Ignore File Locations

| File | Scope | Authority |
|---|---|---|
| `.wt/ignore` | Root worktree | **Authoritative** — patterns here cannot be negated by any tree |
| `.wt-tree/ignore` | Specific tree | **Additive** — adds patterns for this tree, cannot negate root |
| Subtree `.wt-tree/ignore` | Nested tree | **Additive** — adds patterns for the subtree, cannot negate parent |

### Pattern Syntax

Patterns follow gitignore syntax:

```
# Comments start with #
*.log                  # Ignore all .log files
build/                 # Ignore build directories
!important.log         # Negation (re-include) — only works within same level
/secret.key            # Anchored to directory root
**/temp                # Match in any subdirectory
```

### Hierarchy Rules

- **Root patterns are absolute.** If `.wt/ignore` ignores `*.env`, no tree can un-ignore `*.env`.
- **Tree patterns are additive.** A `.wt-tree/ignore` can add new patterns but cannot negate root patterns.
- **Subtree patterns are additive.** A subtree's `.wt-tree/ignore` can add patterns but cannot negate its parent tree's patterns.
- **Negation (`!`) only works within the same level.** A tree can negate its own patterns but not its parent's.

### Built-in Ignores

**Hard ignores** — Always ignored, cannot be overridden:

```
.wt/
.wt-tree/
.git/
```

**Soft defaults** — Ignored by default, can be overridden in config:

```
node_modules/
target/
__pycache__/
*.pyc
.DS_Store
Thumbs.db
*.swp
*.swo
*~
```

To include a soft-default path, add a negation in `.wt/ignore` or `.wt-tree/ignore`:

```
# .wt-tree/ignore
!node_modules/important-package/
```

---

## 12. Large File Handling

W0rkTree handles large files **natively**. There is no separate LFS system, no tracking configuration, no extensions to install. Every file goes through the same pipeline regardless of size.

### Automatic Detection

Files exceeding a configurable size threshold (default: 10 MB) are automatically treated as large files:

```toml
# .wt/config.toml

[storage]
large_file_threshold = "10MB"   # Files above this size get chunked storage
```

### Chunked Storage

Large files are split into content-addressable chunks using the **FastCDC** (Fast Content-Defined Chunking) algorithm:

- **Content-defined boundaries** — Chunk boundaries are determined by file content, not fixed offsets. This means small edits to a large file only produce new chunks for the changed regions.
- **Automatic deduplication** — Identical chunks across different files or snapshots are stored only once.
- **Configurable chunk size** — Default target is 1 MB chunks, configurable per-worktree.

### Lazy Loading

The bgprocess downloads file content **on demand**, not eagerly:

- **FUSE (Linux/macOS)** — The bgprocess mounts a FUSE filesystem that intercepts file reads and downloads chunks as needed.
- **ProjFS (Windows)** — The bgprocess uses Windows Projected File System to provide on-demand materialization.
- **Transparent** — Applications see regular files. The lazy loading is invisible to tools and editors.

### No Separate LFS

| Git + LFS | W0rkTree |
|---|---|
| Install LFS extension | Nothing to install |
| Configure `.gitattributes` tracking patterns | Nothing to configure |
| `git lfs track "*.psd"` | Just add the file |
| Separate LFS server/storage | Same storage pipeline |
| LFS pointer files in repo | Real files everywhere |
| LFS bandwidth limits/quotas | Unified storage management |

---

## 13. Shallow History & Partial Sync

### Shallow Init

When initializing from a remote worktree, you can limit the history depth:

```bash
wt init --from https://wt.acme.dev/acme-corp/my-project --depth 10
```

This downloads only the last 10 snapshots per branch. Older history is available on demand — the bgprocess fetches it from the server when needed (e.g., when running `wt log` past the shallow boundary).

### Lazy History Loading

```bash
# View recent history (available locally)
wt log --limit 10

# View older history (fetched from server on demand)
wt log --limit 100

# View full history (fetched incrementally)
wt log --all
```

### Partial Tree Sync

You can sync only specific trees from a worktree:

```bash
# Init with only specific trees
wt init --from https://wt.acme.dev/acme-corp/my-project \
  --trees frontend,shared-models

# Add more trees later
wt tree sync api-gateway
```

Trees that are not synced appear as **stub trees** — empty directories with a `.wt-tree/` folder containing only metadata. The bgprocess materializes them on demand when you run `wt tree sync`.

### Server Always Has Complete History

Shallow and partial sync are **client-side optimizations only**. The server always maintains the complete history for every tree, every branch, and every snapshot. Nothing is lost. The developer chooses how much to download locally.

---

## 14. Branch Protection

Branch protection rules are defined declaratively in configuration and enforced by the server. The bgprocess cannot bypass them.

### Protection Rules

```toml
# .wt/config.toml

[branch_protection.main]
no_direct_push = true              # Must go through merge request
require_merge_review = true        # At least one approval required
required_reviewers = 2             # Number of required approvals
no_delete = true                   # Cannot delete this branch
require_ci_pass = true             # CI must pass before merge
required_ci_checks = ["build", "test", "lint"]
require_snapshot_signature = false # Snapshots must be signed

[branch_protection."release/*"]
no_direct_push = true
require_merge_review = true
required_reviewers = 1
no_delete = false                  # Release branches can be deleted after merge
require_ci_pass = true
```

### Built-in Merge Request System

W0rkTree has a **built-in merge request system**. No external platform required.

```bash
# Create a merge request
wt merge-request create feature-auth --target main \
  --title "Add authentication system" \
  --description "Implements OAuth2 + JWT"

# List merge requests
wt merge-request list

# Review a merge request
wt merge-request review 42 --approve
wt merge-request review 42 --request-changes --comment "Need tests"

# Merge (server enforces all protection rules)
wt merge-request merge 42
```

### Protection Inheritance

Trees can **add to** root protections but cannot remove them:

```toml
# services/auth-service/.wt-tree/config.toml

[branch_protection.main]
# Inherits all root protections, plus:
required_reviewers = 3             # Stricter than root's 2
require_snapshot_signature = true  # Additional requirement
```

If the root requires 2 reviewers, a tree can require 3 but cannot require 1. The ceiling model applies to branch protection just as it does to access control.

---

## 15. Reflog

The reflog is a **chronological log of all operations** that change branch tips, snapshot pointers, or tree state. It is the safety net for recovery.

### What Gets Logged

Every operation that moves a branch pointer or modifies tree state:

- Snapshot creation (auto and manual)
- Branch create, switch, delete
- Merge operations
- Push and sync operations
- Tag creation and deletion
- Revert operations

### Storage

```
.wt/reflog/
├── HEAD                    ← Current branch reflog
├── refs/
│   ├── main               ← Reflog for main branch
│   ├── feature-auth       ← Reflog for feature-auth branch
│   └── ...
```

Each reflog entry contains:

```
<snapshot-hash> <previous-hash> <operation> <timestamp> <tenant> <message>
```

### Sync to Server

The reflog is synced to the server. The server maintains a **complete reflog** for every branch across all tenants. This means:

- Local reflog entries are backed up to the server.
- If a developer loses their local data, the reflog can be recovered from the server.
- Administrators can audit the full history of operations on any branch.

### Recovery

```bash
# View reflog for current branch
wt reflog

# Output:
# reflog@{0}: push — Push feature-auth to server (abc123)
# reflog@{1}: snapshot — Auto-snapshot (def456)
# reflog@{2}: merge — Merge main into feature-auth (789abc)
# reflog@{3}: branch switch — Switch to feature-auth (from main)

# Restore to a previous state
wt snapshot restore reflog@{2}

# View reflog for a specific branch
wt reflog --branch main
```

### Configurable Retention

```toml
# .wt/config.toml

[reflog]
retention = "90d"              # Keep reflog entries for 90 days
max_entries = 10000            # Maximum entries per branch
sync_to_server = true          # Sync reflog to server (default: true)
```

---

## 16. Revert

Reverting creates an **inverse diff** as a new snapshot. History stays append-only — the original snapshot is not modified or removed.

### Basic Revert

```bash
# Revert a specific snapshot
wt revert abc123

# This creates a new snapshot that undoes all changes from abc123
# The original abc123 remains in history
```

### Revert a Merge Snapshot

Merge snapshots have multiple parents. You must specify which parent to revert relative to:

```bash
# Revert merge snapshot, keeping parent 1's side
wt revert abc123 --parent 1

# Revert merge snapshot, keeping parent 2's side
wt revert abc123 --parent 2
```

### Revert Behavior

- The revert operation computes the inverse of the target snapshot's changes.
- A new snapshot is created with this inverse diff.
- The new snapshot's message defaults to `"Revert: <original message>"`.
- The revert snapshot records a reference to the original snapshot for auditability.
- History remains append-only. The reverted snapshot is still visible in history.

```bash
wt revert abc123 --message "Revert auth changes due to security issue"
```

---

## 17. Archiving

Archiving exports a tree's contents as a standalone archive file, without VCS metadata.

### Archive Formats

```bash
# Export as tar.gz (default)
wt archive tar.gz

# Export as zip
wt archive zip

# Export a specific tree
wt archive tar.gz --tree auth-service

# Export a specific snapshot
wt archive tar.gz --snapshot abc123

# Export to a specific output path
wt archive tar.gz --output ./dist/auth-service-v1.0.0.tar.gz
```

### What Gets Archived

- All tracked files in the tree (or worktree root).
- Respects ignore patterns — ignored files are excluded.
- **No VCS metadata** — `.wt/`, `.wt-tree/`, and internal storage are excluded.

### License Compliance in Archives

Archives respect license compliance rules:

- Paths with `LicenseRef-Proprietary` or similar restrictive licenses are **excluded** from archives by default.
- To include proprietary paths, the archiving tenant must have the `redistribute` license grant.
- The server can be configured to enforce archive compliance even for local archives.

```bash
# Archive with license compliance check
wt archive tar.gz --check-licenses

# Force include all paths (requires redistribute grant)
wt archive tar.gz --include-all
```

### Integration with Releases

Archives are the natural artifact type for releases:

```bash
wt tag create v1.0.0 --annotate --message "Stable release"
wt archive tar.gz --output ./dist/project-v1.0.0.tar.gz
wt release create v1.0.0 \
  --artifact ./dist/project-v1.0.0.tar.gz \
  --status stable
```

---

## 18. Diff System

W0rkTree provides a comprehensive diff system with multiple targets, modes, and output formats.

### Diff Targets

```bash
# Working tree vs. last snapshot
wt diff

# Working tree vs. specific snapshot
wt diff abc123

# Between two snapshots
wt diff abc123 def456

# Between two branches
wt diff main feature-auth

# Between a branch and a snapshot
wt diff main abc123
```

### Diff Modes

| Mode | Flag | Description |
|---|---|---|
| **Full** (default) | (none) | Show full diff with context lines |
| **Stat** | `--stat` | Show file-level summary (insertions, deletions, changes) |
| **Name only** | `--name-only` | Show only the names of changed files |
| **Word-level** | `--word-diff` | Show word-level changes instead of line-level |

```bash
wt diff --stat
# Output:
#  src/auth/handler.rs  | 42 +++++++++------
#  src/auth/token.rs    | 18 +++++++
#  tests/auth_test.rs   |  7 +++
#  3 files changed, 47 insertions(+), 20 deletions(-)

wt diff --name-only
# Output:
#  src/auth/handler.rs
#  src/auth/token.rs
#  tests/auth_test.rs

wt diff --word-diff
# Shows inline word-level changes with [-removed-] and {+added+} markers
```

### Rename and Copy Detection

W0rkTree automatically detects file renames and copies:

```bash
wt diff --stat
# Output:
#  src/auth/handler.rs → src/auth/oauth_handler.rs | 12 +++---
#  src/utils/hash.rs (copied from src/auth/hash.rs) |  3 ++-
```

The rename/copy detection threshold is configurable:

```toml
# .wt/config.toml

[diff]
rename_threshold = 50    # % similarity required to detect rename (default: 50)
copy_detection = true    # Enable copy detection (default: true)
context_lines = 3        # Lines of context around changes (default: 3)
```

### Output Formats

| Format | Flag | Description |
|---|---|---|
| **Colored** (default) | (none) | Terminal-colored diff output |
| **Patch** | `--patch` | Standard unified diff format (compatible with `patch` utility) |
| **JSON** | `--json` | Machine-readable JSON diff output |

```bash
# Colored terminal output (default)
wt diff

# Patch format for piping to other tools
wt diff --patch > changes.patch

# JSON for programmatic processing
wt diff --json | jq '.files[].path'
```

---

## 19. Git Compatibility

W0rkTree is not Git, but it speaks Git when needed. The Git compatibility layer is a **bridge for migration and interop**, not a core dependency.

### Import from Git

```bash
# Import a Git repository as a W0rkTree
wt init --from-git https://github.com/org/repo.git

# Import with history mapping
wt init --from-git ./local-repo --full-history

# Import specific branches only
wt init --from-git https://github.com/org/repo.git --branches main,develop
```

The importer maps Git concepts to W0rkTree concepts:

| Git | W0rkTree |
|---|---|
| Commits | Snapshots |
| Tags | Tags |
| Branches | Branches |
| `.git/` | `.wt/` |
| `.gitignore` | `.wt/ignore` (imported and converted) |

### Export to Git

```bash
# Export current tree as a Git repository
wt export-git ./output-repo

# Export with full history
wt export-git ./output-repo --full-history

# Export specific branch only
wt export-git ./output-repo --branch main
```

### Git Remote Bridge

W0rkTree can push to and pull from Git remotes (e.g., GitHub, GitLab):

```bash
# Add a Git remote
wt remote add github --git https://github.com/org/repo.git

# Push to Git remote (converts snapshots to commits)
wt remote push github main

# Pull from Git remote (converts commits to snapshots)
wt remote pull github main
```

### Live Mirror Mode

For teams migrating gradually, W0rkTree supports **bidirectional sync** with a Git remote:

```bash
# Enable live mirror to a Git remote
wt remote mirror github --bidirectional

# While mirroring:
# - Snapshots created in W0rkTree are automatically pushed to GitHub as commits
# - Commits pushed to GitHub are automatically pulled into W0rkTree as snapshots
# - Tags round-trip in both directions
# - Branches round-trip in both directions
```

Live mirror mode is a migration tool. It allows teams to adopt W0rkTree incrementally while keeping their existing Git workflows operational. Once migration is complete, the mirror can be disabled and the Git remote removed.

### Round-Trip Guarantees

| Entity | Git → W0rkTree → Git |
|---|---|
| Commits/Snapshots | ✓ Content preserved. Hashes will differ (different content-addressing). |
| Tags | ✓ Lightweight and annotated tags round-trip. |
| Branches | ✓ Branch names and targets preserved. |
| File content | ✓ Byte-for-byte identical. |
| Author/Date | ✓ Preserved in snapshot metadata. |
| Merge history | ✓ Merge structure preserved. Parent relationships maintained. |

---

## 20. Comparison with Git

| Aspect | Git | W0rkTree |
|---|---|---|
| **Architecture** | Monolithic local tool + separate hosting | Two-runtime: local bgprocess + remote server |
| **Organization** | Single flat repository per project | Multi-tenant trees with nested subtrees |
| **Identity** | Name + email in config (no verification) | Verified tenant identity: username + email |
| **Terminology** | commit, repository, checkout, stash | snapshot, tree, switch, (no stash needed) |
| **Staging** | Explicit staging area (`git add`) required | No staging area. Snapshot captures working state. |
| **Commands** | 150+ commands, many overloaded | One job per command, no overloading |
| **Branches** | Global namespace, naming conflicts | Tree-scoped with independent strategies |
| **Access Control** | None built-in — relies on hosting platform | Declarative `.wt/access/` with TOML, ceiling model |
| **Merge** | Merge, rebase, cherry-pick, squash | Merge only. No rebase. Append-only history. |
| **History** | Rewritable (rebase, reset, force-push) | Append-only. Non-destructive. Soft deletes. |
| **Large Files** | Requires Git LFS (separate system) | Native chunked storage with lazy loading |
| **Protocol** | Git protocol (smart/dumb HTTP, SSH) | Native W0rkTree + Git compatibility layer |
| **Collaboration** | No visibility until push | Staged snapshots — team sees WIP in real-time |
| **License Tracking** | None | Per-path SPDX, server-enforced compliance |
| **Automation** | Manual everything | Auto-snapshot, auto-sync by default |
| **Recovery** | `git reflog` (local only, expires) | Full reflog, server-synced, configurable retention |
| **Configuration** | `.git/config` + global config | Hierarchical: system → user → `.wt/` → `.wt-tree/` |
| **Multi-tenancy** | Not supported | First-class tenants, cross-tenant sharing |
| **Monitoring** | None built-in | Server-side telemetry, sync health, activity |
| **Deployment** | External CI/CD reads Git events | Server-enforced branch protection, CI gates |
| **Submodules** | Separate system, notoriously painful | Nested trees — native, consistent, reliable |
| **Dependencies** | None built-in | Three-level dependency system with TODO generation |
| **Project Management** | External tools only | Built-in per-tree task management |
| **Conflict Resolution** | Basic markers, no machine-readable format | Three-way markers + machine-readable `.wt/conflicts/` |

### Why Not Just Improve Git?

Git's architecture makes certain improvements impossible:

- **No identity system** — Git has no concept of verified users. Adding one would break every existing workflow.
- **No access control** — Git delegates access control to hosting platforms. Adding native access control would require a server component, which Git architecturally avoids.
- **Rewritable history** — Git's `rebase`, `reset`, and `force-push` are deeply embedded in the culture and tooling. Making history append-only would break existing workflows.
- **No staging visibility** — Git has no mechanism for sharing work-in-progress without pushing to a branch. Adding one would require a server component.
- **Monolithic repository model** — Git's one-repo-per-project model cannot support nested independent versioning without submodules (which are a separate, painful system).

W0rkTree does not try to improve Git. It replaces it.

---

## 21. Use Cases

### Microservices Architecture

Each microservice lives in its own tree with independent versioning, branches, and release cycles.

```
payment-platform/                 ← W0rkTree
├── .wt/
├── services/
│   ├── payment-api/              ← Tree (Backend team)
│   ├── fraud-detection/          ← Tree (ML team)
│   ├── notification-service/     ← Tree (Platform team)
│   └── merchant-portal/          ← Tree (Frontend team)
├── libs/
│   ├── shared-types/             ← Tree (Shared)
│   └── grpc-protos/              ← Tree (Shared)
└── infra/
    └── terraform/                ← Tree (DevOps team)
```

- Teams own their trees without stepping on each other.
- Shared libraries are nested trees with their own versioning.
- Cross-service dependencies are explicit and tracked.
- Each service can be released independently while linked branches coordinate cross-service features.

### Multi-Platform Applications

Frontend, backend, mobile, and shared code each occupy separate trees.

```
social-app/                       ← W0rkTree
├── .wt/
├── web/                          ← Tree (Web team)
├── ios/                          ← Tree (iOS team)
├── android/                      ← Tree (Android team)
├── backend/                      ← Tree (Backend team)
└── shared/
    ├── api-types/                ← Tree (Shared)
    └── design-tokens/            ← Tree (Design team)
```

- Linked branches coordinate cross-platform features.
- A release in one tree can require matching releases in dependent trees.
- The dependency system prevents partial deployments (e.g., deploying a new API without the client update).

### Enterprise Codebases

Thousands of developers across hundreds of teams.

- Declarative access control defines who can touch what — at the file path level, version-controlled alongside the code.
- License compliance prevents unauthorized use of proprietary modules.
- The server enforces every rule without relying on developer discipline.
- Per-tree branch protection rules allow different review requirements for different components.
- The staged snapshot visibility layer gives management real-time insight into active work without requiring status meetings.

### Open-Source with Proprietary Modules

Public trees for open-source code. Private nested trees for proprietary modules.

```
analytics-engine/                 ← W0rkTree (visibility: public)
├── .wt/
├── core/                         ← Tree (MIT license, public)
├── connectors/
│   ├── postgres/                 ← Tree (MIT license, public)
│   ├── mysql/                    ← Tree (MIT license, public)
│   └── snowflake/                ← Tree (Proprietary, private)
├── enterprise/
│   ├── sso-integration/          ← Tree (Proprietary, private)
│   └── audit-logging/            ← Tree (Proprietary, private)
└── docs/                         ← Tree (CC-BY-4.0, public)
```

- Per-path SPDX licensing ensures open-source files stay open-source and proprietary files stay proprietary.
- The server blocks any sync that would violate license boundaries.
- External contributors can fork and contribute to public trees without accessing proprietary code.
- Archives automatically exclude proprietary paths.

---

## 22. Design Philosophy

These are not aspirations. They are constraints enforced by the protocol.

### 1. One Job Per Command

Every W0rkTree command does exactly one thing. There is no `checkout` that creates branches, switches branches, and restores files depending on the flags you pass. If a command name describes the action, that is the only action it performs.

| Git (overloaded) | W0rkTree (one job) |
|---|---|
| `git checkout -b feature` | `wt branch create feature` |
| `git checkout main` | `wt branch switch main` |
| `git checkout -- file.txt` | `wt restore file.txt` |
| `git reset --soft HEAD~1` | `wt snapshot undo` |
| `git reset --hard HEAD~1` | (not supported — history is append-only) |

### 2. Plain Terminology

If you have to explain what a word means, it is the wrong word. W0rkTree uses words that mean what they say.

| Git term | Problem | W0rkTree term |
|---|---|---|
| Commit | Not intuitive for non-developers | **Snapshot** — a picture of the state |
| Repository | Overloaded meaning | **Tree** — a collection of files |
| Push/Pull | Two words for sync | **Sync** / **Push** — clear direction |
| Checkout | Does three different things | **Switch** (branch) / **Restore** (file) |
| Stash | Unexplained metaphor | (Not needed — auto-snapshots capture everything) |
| HEAD | Cryptic pointer name | **Current snapshot** |
| Index/staging area | Confusing indirection | (Does not exist — snapshot captures working state) |

### 3. Automatic by Default

The bgprocess creates snapshots automatically as you work. It syncs staged snapshots automatically when connected. You can override this — manual mode exists — but the default is automation. The developer's job is to write code, not to babysit version control.

### 4. Append-Only History

There is no rebase. There is no `reset --hard`. There is no `force-push`. History is append-only. If you make a mistake, you create a new snapshot that fixes it. The original mistake remains in history, because that is what happened. History is a record, not a narrative to be edited.

### 5. Non-Destructive Operations

Nothing is permanently deleted immediately. Branches, snapshots, tags — when "deleted," they enter a **soft-delete state** with a configurable recovery window. A full reflog is maintained server-side. Accidental data loss requires active effort to achieve.

### 6. Real-Time Collaboration

Staged snapshot visibility means your team knows what you are working on without asking. This is not a chat feature or a status page — it is a core protocol feature. The server maintains real-time visibility into active work across all tenants with appropriate access.

### 7. Multi-Protocol Support

W0rkTree speaks its own native protocol for full functionality. It also speaks Git protocol for migration and interop. A Git client can clone a W0rkTree tree (with reduced functionality). A W0rkTree bgprocess can import from a Git repository. The Git compatibility layer is a bridge, not a dependency.

---

## 23. Benefits

### For Developers

- **Focused context** — Work only on relevant trees. Clone only what you need.
- **Faster operations** — Smaller scope means faster snapshots, faster diffs, faster merges.
- **Clear boundaries** — Understand system architecture through tree structure.
- **Independent experimentation** — Try changes in one tree without affecting others.
- **No babysitting** — Auto-snapshots and auto-sync mean you focus on code, not version control.
- **Real-time visibility** — See what your teammates are working on via staged snapshots.
- **Better conflict resolution** — Three-way markers with clear labels and machine-readable metadata.
- **Native large files** — No LFS configuration, no tracking patterns, no extensions.
- **Safe history** — Append-only history means no accidental data loss.

### For Teams

- **Reduced coordination** — Teams work in their own trees with independent branches and release cycles.
- **Structured dependencies** — The dependency system replaces ad-hoc coordination with explicit, tracked relationships.
- **Automatic task generation** — Cross-tree dependencies automatically create structured TODOs.
- **Linked branches** — Multi-tree features are atomic units that merge together.
- **Built-in merge requests** — No external platform required for code review.
- **Flexible processes** — Each tree can have its own branch strategy and protection rules.
- **Clear ownership** — Trees map to team responsibilities.
- **Easier onboarding** — New members init only the trees they need.

### For Organizations

- **Scalable architecture** — Structure grows with the organization. Add trees as teams form.
- **Security by default** — Declarative access control, enforced server-side, version-controlled.
- **License compliance** — Per-path SPDX tracking prevents license violations before they happen.
- **Audit trail** — Full reflog, server-synced, with configurable retention.
- **Multi-tenancy** — Cross-organization collaboration with explicit access grants and visibility modes.
- **Resource efficiency** — Partial sync and lazy loading mean developers only download what they use.
- **Integrated project management** — Dependencies, TODOs, and linked branches replace external PM tools.
- **Migration path** — Git compatibility layer enables gradual adoption without disrupting existing workflows.
- **Prevents chaos** — Structured dependencies and tree boundaries prevent the organizational mess that occurs in large Git codebases.

---

## Appendix A: Command Reference Summary

| Command | Description |
|---|---|
| `wt init` | Initialize a new worktree |
| `wt init --from <url>` | Initialize from a remote worktree |
| `wt init --from-git <url>` | Import from a Git repository |
| `wt snapshot` | Create a manual snapshot |
| `wt snapshot -m <msg>` | Create a snapshot with a message |
| `wt push` | Finalize staged snapshots into branch history |
| `wt sync` | Bidirectional sync with server |
| `wt branch create <name>` | Create a new branch |
| `wt branch switch <name>` | Switch to a branch |
| `wt branch list` | List branches in current tree |
| `wt branch delete <name>` | Soft-delete a branch |
| `wt merge <branch>` | Merge a branch into current branch |
| `wt diff` | Show changes |
| `wt log` | Show snapshot history |
| `wt tag create <name>` | Create a tag |
| `wt release create <tag>` | Create a release from a tag |
| `wt revert <snapshot>` | Revert a snapshot |
| `wt archive <format>` | Export tree as archive |
| `wt reflog` | Show operation log |
| `wt todo list` | Show pending TODOs for current tree |
| `wt todo claim <id>` | Claim a TODO |
| `wt todo complete <id>` | Mark a TODO as complete |
| `wt depend add <tree>` | Add a dependency on another tree |
| `wt deps graph` | Visualize dependency graph |
| `wt merge-request create` | Create a merge request |
| `wt remote add <name>` | Add a remote (W0rkTree or Git) |
| `wt tree sync <name>` | Sync a specific tree |
| `wt restore <file>` | Restore a file to its last snapshot state |

---

## Appendix B: Glossary

| Term | Definition |
|---|---|
| **W0rkTree** | The top-level organizational unit containing one or more trees. Marketing name uses zero. Code uses `worktree`. |
| **Tree** | The fundamental unit of code organization. Has its own snapshot history, branches, and access rules. |
| **Snapshot** | An immutable, content-addressed record of the complete state of a tree at a point in time. |
| **Staged Snapshot** | A snapshot synced to the server for team visibility but not yet part of branch history. |
| **Branch** | A named pointer to a snapshot chain within a tree. |
| **Linked Branch** | Branches across different trees that must be merged together. |
| **Tenant** | A user or organization with verified identity on the W0rkTree server. |
| **BGProcess** | The local background process (`worktree-bgprocess`) that runs on the developer's machine. |
| **Server** | The remote server (`worktree-server`) that is the source of truth. |
| **Tag** | An immutable named reference to a specific snapshot. |
| **Release** | A tag with attached artifacts, notes, and status. |
| **Merge Request** | A request to merge one branch into another, with review and CI gate support. |
| **Reflog** | Chronological log of all operations that change branch tips. |
| **Ceiling Model** | Access control model where parent levels set maximum permissions that children cannot exceed. |
| **Stub Tree** | A tree that exists in metadata but whose files have not been synced locally. |
| **FastCDC** | Content-defined chunking algorithm used for large file storage. |
| **SPDX** | Software Package Data Exchange — standard for license identifiers. |

---

**W0rkTree is not the next version of Git. It is what comes after Git.**