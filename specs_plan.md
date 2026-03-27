# W0rkTree — Specification Update Plan

> **Goal**: Align all existing specs, docs, and architecture documents with the W0rkTree vision — a system built to fix and replace Git entirely.

---

## Table of Contents

1. [Why This Plan Exists](#1-why-this-plan-exists)
2. [The Problems With Git (That W0rkTree Fixes)](#2-the-problems-with-git-that-w0rktree-fixes)
3. [Core Architecture Corrections](#3-core-architecture-corrections)
   - [3.8 Worktree-Level Ignore Patterns](#38-worktree-level-ignore-patterns)
   - [3.9 Revert (Undo a Specific Snapshot)](#39-revert-undo-a-specific-snapshot)
   - [3.10 Reflog (Operation History & Safety Net)](#310-reflog-operation-history--safety-net)
   - [3.11 Shallow History & Partial Sync](#311-shallow-history--partial-sync)
   - [3.12 Tags & Releases](#312-tags--releases)
   - [3.13 Archiving & Export](#313-archiving--export)
   - [3.14 Merge Strategy & Conflict Resolution](#314-merge-strategy--conflict-resolution)
   - [3.15 Large File & Binary Handling](#315-large-file--binary-handling)
   - [3.16 Diff Configuration & Capabilities](#316-diff-configuration--capabilities)
   - [3.17 Configuration Hierarchy](#317-configuration-hierarchy)
   - [3.18 Branch Protection Rules](#318-branch-protection-rules)
4. [Complete CLI Command Reference](#4-complete-cli-command-reference)
5. [Spec Files To Update](#5-spec-files-to-update)
6. [New Specs To Create](#6-new-specs-to-create)
7. [Detailed Change Plan Per File](#7-detailed-change-plan-per-file)
8. [Naming & Terminology Standardization](#8-naming--terminology-standardization)
9. [Implementation Alignment](#9-implementation-alignment)
10. [Execution Order & Dependencies](#10-execution-order--dependencies)
11. [Open Items & Decisions Needed](#11-open-items--decisions-needed)

---

## 1. Why This Plan Exists

The current specs and docs were written at different stages of the project. They contain:

- **Inconsistent naming**: "Worktree", "WorkTree", "W0rkTree", "worktree" used interchangeably
- **Missing architecture**: The `worktree-bgprocess` / `worktree-worker` concept (the local background process) is not properly distinguished from the `worktree-server` (the remote multi-tenant server)
- **Git framing instead of replacement framing**: Many docs still frame W0rkTree as "Git but better" rather than a clean replacement with Git compatibility as a migration bridge
- **Incomplete specs**: `docs/protocol-spec.md`, `docs/server-architecture.md`, `docs/git-compatibility.md`, and `docs/sdk-guide.md` are all TODO stubs
- **Missing concepts**: Staged snapshot visibility, the `.wt` folder (vs `.worktree`), native multi-tenancy on the server, and the bgprocess ↔ server split are undocumented
- **Stale architecture diagrams**: The `WORKTREE_PLAN.md` architecture diagram does not reflect the bgprocess/server separation

This plan defines every change needed to bring all specs into alignment with the true W0rkTree architecture.

---

## 2. The Problems With Git (That W0rkTree Fixes)

These problems must be **explicitly called out** across all spec documents as the foundational motivation. Every spec should trace back to which Git problem it solves.

### 2.1 UX & Conceptual Problems

| Git Problem | Category | W0rkTree Answer |
|---|---|---|
| Too many ways to do the same thing ("the Perl of VCS") | UX | One clear way to do each operation. No aliases, no overloaded commands. |
| Confusing jargon (ref, refspec, HEAD, origin, staging, stash, rebase, index) | UX | Plain terminology: tree, snapshot, branch, sync. No staging area. No index. |
| Inconsistent commands (`git checkout` does 5 different things) | UX | Each command does one thing. `wt branch switch`, `wt snapshot restore` — no overloading. |
| Misleading error messages (`src refspec does not match any`) | UX | Human-readable errors with suggested fixes. Every error code is documented. |
| The staging area / index is confusing and unnecessary | Conceptual | No staging area. Changes are tracked automatically. Snapshots capture working state directly. |
| Branches are pointers (users think they're containers) | Conceptual | Branches are still pointers internally, but the UX presents them as workstreams with visible history. |
| Local vs remote confusion ("I committed but can't see my changes") | Conceptual | The bgprocess handles sync automatically. Local and remote are always converging. No manual push/pull required (with option to disable auto-sync). |

### 2.2 Destructive Mistakes Git Enables

| Git Problem | W0rkTree Answer |
|---|---|
| Accidental file/branch deletion (`git reset --hard`) | Snapshots are immutable. Soft-delete with recovery window. No destructive reset by default. |
| Rebasing disasters (history rewriting) | No rebase. History is append-only. Divergence is handled by the merge engine, not by rewriting. |
| Force pushing (`--force`) wiping colleagues' work | No force push. Server rejects overwrites. Conflict resolution is mandatory. |
| Committing secrets (API keys, passwords) | Built-in secret scanning on snapshot creation. Pre-snapshot hooks enabled by default. Configurable patterns in `.wt/config.toml`. |

### 2.3 Workflow & Performance Problems

| Git Problem | W0rkTree Answer |
|---|---|
| Merge conflicts from long-lived branches | The bgprocess keeps trees synced continuously. Divergence is detected early and flagged. |
| Large binary handling (repo bloat) | Native chunked storage for large files. No separate LFS system needed. Lazy loading by default. |
| Not pulling often enough | The bgprocess auto-syncs. There is no "pull" — your local tree converges with the server continuously. |

### 2.4 Protocol & Security Problems

| Git Problem | W0rkTree Answer |
|---|---|
| Native `git://` protocol has no auth or encryption | W0rkTree protocol is encrypted (TLS/QUIC) with mandatory authentication. No anonymous mode. |
| Port 9418 blocked in corporate environments | W0rkTree uses standard HTTPS (443) or QUIC (443/UDP) — firewall-friendly by default. |
| No built-in access control (relies on GitHub/GitLab) | Native IAM: tenants, teams, users, roles, policies. Enforced at protocol level on the server. Granular down to individual files. |
| SSH/HTTPS are bolted-on, each with tradeoffs | One protocol. One transport. Encrypted, authenticated, multiplexed (QUIC with TCP fallback). |

---

## 3. Core Architecture Corrections

The biggest gap in current specs is the **bgprocess vs. server distinction**. This must be corrected everywhere.

### 3.1 The Two Runtimes

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          W0RKTREE ARCHITECTURE                          │
│                                                                         │
│  LOCAL MACHINE                          REMOTE / HOSTED                 │
│  ──────────────                         ───────────────                 │
│                                                                         │
│  ┌──────────────────────┐               ┌────────────────────────────┐  │
│  │  worktree-bgprocess  │◄─── QUIC ───►│     worktree-server        │  │
│  │  (worktree-worker)   │    (sync)     │                            │  │
│  │                      │               │  • Multi-tenant             │  │
│  │  • Filesystem watcher│               │  • IAM / Access Control    │  │
│  │  • Auto-snapshots    │               │  • Stores all history      │  │
│  │  • Local change      │               │  • Manages worktrees       │  │
│  │    tracking          │               │  • Serves API              │  │
│  │  • Background sync   │               │  • Tenant isolation        │  │
│  │    to server         │               │                            │  │
│  │  • Manages .wt/      │               └────────────────────────────┘  │
│  └──────────┬───────────┘                                               │
│             │                                                           │
│  ┌──────────▼───────────┐                                               │
│  │   Local Worktree     │               ┌────────────────────────────┐  │
│  │                      │               │     worktree-admin         │  │
│  │  project/            │               │  (Web UI for server mgmt)  │  │
│  │  ├── .wt/            │               └────────────────────────────┘  │
│  │  │   ├── config.toml │                                               │
│  │  │   ├── access/     │               ┌────────────────────────────┐  │
│  │  │   └── cache/      │               │     worktree-cli (wt)      │  │
│  │  ├── src/            │               │  CLI interface for users    │  │
│  │  └── ...             │               └────────────────────────────┘  │
│  └──────────────────────┘                                               │
└─────────────────────────────────────────────────────────────────────────┘
```

### 3.2 `worktree-bgprocess` (a.k.a. `worktree-worker`)

This is the **local background process** that runs on a developer's machine. Current specs call this "the server" — that is **wrong** and must be corrected.

The bgprocess **is a full local VCS**. It tracks changes, manages snapshots, handles branching — all locally. But critically, it also syncs **staged snapshots** to the W0rkTree server so the rest of the team has visibility into what's in flight.

**Core VCS Responsibilities (Local):**
- Watches the local filesystem for changes (file create/modify/delete/rename)
- Creates auto-snapshots based on configured rules (or manual `wt snapshot`)
- Manages local snapshot history, branches, and the DAG
- Handles all local storage — history and objects are stored locally but managed by the bgprocess, NOT inside the working directory alongside user code
- The bgprocess stores its data in a platform-appropriate location (`%APPDATA%\W0rkTree\`, `~/.local/share/w0rktree/`, `~/Library/Application Support/W0rkTree/`), keeping the working directory clean
- Manages the `.wt/` folder (config, access tokens, local state)

**Staged Snapshot Visibility (The Key Innovation):**

This is what makes W0rkTree fundamentally different from Git. In Git, all work is invisible to the team until someone pushes. In W0rkTree:

- When the bgprocess captures a local snapshot (auto or manual), that snapshot is synced to the server as a **staged snapshot**
- **Staged ≠ Pushed.** A staged snapshot is visible on the root W0rkTree so the team can see what's being worked on and by whom, but it is **NOT pushed to the branch**. It does not become part of the branch's snapshot history.
- The developer must **explicitly push their staged work to the branch** when they're ready — only then does it become part of the branch's actual history
- This gives teams **visibility without pollution**: you can see "Alice has 3 staged snapshots touching `auth-service/src/oauth.rs` on `feature/oauth`" without those changes being in the branch yet
- The server aggregates staged snapshots from all connected bgprocess clients and presents them on the root W0rkTree, giving the whole organization a real picture of what's in flight at any moment

**Think of it like this:**
- Git: work is invisible → push → visible + merged (one step, all or nothing)
- W0rkTree: work is captured locally → staged snapshots sync to server (visible to team, NOT on branch) → explicit push to branch (merged into branch history)

**Sync Responsibilities:**
- Syncs staged snapshots **to** the W0rkTree server (for team visibility)
- Syncs remote branch updates **from** the W0rkTree server (so local stays current)
- Handles all file uploads to the server (blobs, objects)
- Feature to disable auto-sync for offline/manual workflows
- When auto-sync is disabled, staged snapshots accumulate locally and sync when re-enabled or manually triggered

### 3.3 `worktree-server` & the Tenant Model

This is the **remote server** that hosts worktrees for teams/organizations. Current specs partially describe this but conflate it with the local daemon.

#### What is a Tenant?

A **Tenant** is a user or organization registered on the worktree-server. Every tenant:

- Has a **username** (slug) and **email address** — these are the primary identifiers used to grant or deny access
- Can own their own worktrees — each tenant has their own set of worktrees with their own IAM
- Can be granted access to **other tenants' worktrees** — you scope which tenants can access your worktree by referencing their username or email in your access config
- Can be an individual developer (personal account) or an organization (team/company account)

This is fundamentally different from Git, where access control lives entirely outside the VCS on platforms like GitHub/GitLab. In W0rkTree, tenants and their access are **first-class protocol concepts** managed directly in the worktree's configuration.

#### Worktree Visibility Modes

Every worktree has a visibility mode that determines its default tenant access:

- **Private** (default) — only the owning tenant has access. Other tenants must be explicitly granted access via IAM config or by tenant username/email.
- **Shared** — specific tenants are granted access (listed by username or email in `.wt/access/policies.toml`). Everyone else is denied.
- **Public** — open-source / open-access. All tenants on the server can read the worktree. Write access, branch creation, and other operations still require explicit grants. License compliance rules (§3.7) govern what can be copied, forked, or exported.

#### Cross-Tenant Access

You scope which tenants can access your worktree in two ways:

1. **IAM config** (`.wt/access/policies.toml`) — full RBAC/ABAC policies referencing tenants, teams, roles, and registered paths. This is the powerful, fine-grained approach.
2. **Simple tenant grants** (`.wt/config.toml`) — just list tenant usernames or email addresses with a permission level. This is the quick approach for simple sharing.

```toml
# .wt/config.toml — simple cross-tenant access grants

[[tenant_access]]
tenant = "alice-dev"                          # Tenant username
permissions = ["tree:read", "branch:read", "snapshot:read"]

[[tenant_access]]
tenant = "bob@company.com"                    # Tenant email
permissions = ["tree:read", "tree:write", "snapshot:create", "branch:create"]

[[tenant_access]]
tenant = "acme-corp"                          # Organization tenant
permissions = ["tree:read"]

# Or use the IAM config in .wt/access/policies.toml for full RBAC/ABAC control
```

Both approaches are valid. Simple `tenant_access` grants in `config.toml` are syntactic sugar — the server resolves them into full IAM policies internally. For complex scenarios (path-level access, branch restrictions, role-based rules), use `.wt/access/policies.toml`.

#### Server Responsibilities

- Native multi-tenancy: multiple tenants (users/orgs), each with their own worktrees
- IAM per tenant: accounts, teams, roles, policies
- Cross-tenant access: tenants can grant other tenants access to their worktrees by username/email or full IAM config
- Worktree visibility modes: private, shared, public
- Access control at root worktree level, individual tree level, individual registered path/file level
- License compliance enforcement (§3.7) — prevents unauthorized copying/export of licensed code
- Stores canonical branch history for all worktrees (the source of truth for pushed snapshots)
- Receives and stores staged snapshots from bgprocess clients — these are visible on the root W0rkTree but NOT part of any branch until explicitly pushed
- Serves the sync protocol to bgprocess clients
- Provides API for admin panel, CLI, and SDK
- Handles merge conflict detection when staged snapshots are pushed to branches
- Aggregates staged snapshot visibility across all connected bgprocess clients so the team sees the full picture of in-flight work

### 3.4 `.wt/` and `.wt-tree/` — Root vs. Tree Configuration

Current specs reference `.worktree/` — this must be changed. But the replacement is **two distinct folders** with different purposes:

- **`.wt/`** — lives at the **root worktree** level. Manages the overall worktree: identity, connection, root-level access, and configuration that applies across the entire project.
- **`.wt-tree/`** — lives inside **each individual tree** (child trees within the worktree). Manages tree-specific configuration, tree-specific access policies, and tree-specific hooks.

This separation mirrors the architecture: the root worktree is the organizational unit, while each tree is an independent unit of code with its own versioning and access.

#### Root Worktree: `.wt/`

```
my-project/                          ← Root Worktree
├── .wt/                             ← ROOT worktree config
│   ├── config.toml                  # Root config: server connection, sync settings, registered paths
│   ├── ignore                       # W0rkTree ignore patterns (replaces .gitignore)
│   │
│   ├── identity/
│   │   ├── token                    # Auth token for server connection
│   │   └── identity.toml            # Local user identity
│   │
│   ├── access/                      # ── ROOT-LEVEL ACCESS MANAGEMENT ──
│   │   ├── roles.toml              # Custom role definitions (apply across all trees unless overridden)
│   │   └── policies.toml           # Root-level access policies (tenant-wide, tree-wide, etc.)
│   │
│   └── hooks/                       # Root-level pre/post snapshot hooks
│       ├── pre-snapshot
│       └── post-snapshot
│
├── services/
│   ├── auth-service/                ← Child Tree
│   │   ├── .wt-tree/               ← TREE-SPECIFIC config for auth-service
│   │   │   ├── config.toml
│   │   │   ├── access/
│   │   │   │   └── policies.toml
│   │   │   └── hooks/
│   │   └── src/
│   └── api-gateway/                 ← Child Tree
│       ├── .wt-tree/               ← TREE-SPECIFIC config for api-gateway
│       │   ├── config.toml
│       │   └── access/
│       │       └── policies.toml
│       └── src/
├── libs/
│   └── shared-models/               ← Child Tree
│       ├── .wt-tree/
│       │   └── config.toml
│       └── src/
└── README.md
```

#### Individual Tree: `.wt-tree/`

Each child tree gets its own `.wt-tree/` folder. This is where tree-specific configuration lives — independent from the root `.wt/` and independent from sibling trees.

```
services/auth-service/
├── .wt-tree/
│   ├── config.toml              # Tree-specific config: branch strategy, auto-snapshot rules, etc.
│   │
│   ├── access/                  # ── TREE-LEVEL ACCESS MANAGEMENT ──
│   │   └── policies.toml       # Access policies scoped to THIS tree (overrides root if needed)
│   │
│   └── hooks/                   # Tree-specific hooks
│       ├── pre-snapshot
│       └── post-snapshot
│
├── src/
│   ├── oauth.rs
│   └── tokens.rs
└── Cargo.toml
```

**Key rule: `.wt-tree/` policies override `.wt/` policies for that tree.** Root-level access cascades down by default, but any `.wt-tree/access/policies.toml` takes precedence for its own tree. This lets teams own their tree's access without touching the root config.

#### Declarative Access via `config.toml` — Explicit Registration, No Globs

Access control in W0rkTree uses **explicit path registration** — not glob patterns. You register the exact files and directories you want to control access on in `config.toml`. There is no `**` or `*` wildcard matching. You declare precisely what you intend to protect.

This is like Terraform: you explicitly declare every resource. Nothing is implicit, nothing is pattern-matched.

**Root `.wt/config.toml`** — Root-level configuration including registered paths and access:

```toml
[worktree]
name = "my-project"
server = "https://wt.company.com"
tenant = "acme-corp"

[sync]
auto = true                               # bgprocess auto-syncs staged snapshots
interval_secs = 30

# ── REGISTERED PATHS ──────────────────────────────────────────────
# Paths you want to manage access on must be explicitly registered.
# No globs. No patterns. You register what you need.

[[registered_path]]
path = "config/production.toml"
description = "Production configuration — restricted write access"

[[registered_path]]
path = "config/staging.toml"
description = "Staging configuration"

[[registered_path]]
path = ".wt/access"
description = "Root access configuration — admin only"

[[registered_path]]
path = "scripts/deploy"
description = "Deployment scripts directory"

[[registered_path]]
path = "secrets"
description = "Secrets directory — highly restricted"
```

**Root `.wt/access/roles.toml`** — Custom roles (apply across all trees unless overridden by `.wt-tree/`):

```toml
# Custom roles beyond the built-in Owner/Admin/Maintainer/Developer/Viewer

[[role]]
name = "security-reviewer"
description = "Can read all code and approve security-sensitive merges"
permissions = ["tree:read", "branch:read", "snapshot:read", "branch:merge"]

[[role]]
name = "ci-bot"
description = "Automated CI/CD — can read, snapshot, and sync but not branch or merge"
permissions = ["tree:read", "snapshot:create", "snapshot:read", "sync:push", "sync:pull"]

[[role]]
name = "contractor"
description = "Limited write access, no branch deletion or admin"
permissions = ["tree:read", "tree:write", "branch:read", "branch:create", "snapshot:create", "snapshot:read"]
```

**Root `.wt/access/policies.toml`** — Who gets what access, at what scope:

```toml
# ── Scope: everything (whole worktree) ──────────────────────────

[[policy]]
name = "backend-team-full-access"
effect = "allow"
subjects = [{ team = "backend-team" }]
scope = "worktree"                            # Applies to entire root worktree and all trees
permissions = ["tree:read", "tree:write", "snapshot:create", "branch:create", "branch:merge", "sync:push", "sync:pull"]

[[policy]]
name = "frontend-readonly"
effect = "allow"
subjects = [{ team = "frontend-team" }]
scope = "worktree"
permissions = ["tree:read", "snapshot:read", "branch:read"]

# ── Cross-tenant access (by username or email) ──────────────────

[[policy]]
name = "partner-company-readonly"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]      # Grant by tenant username
scope = "worktree"
permissions = ["tree:read", "snapshot:read"]

[[policy]]
name = "contractor-scoped"
effect = "allow"
subjects = [{ tenant = "jane@contractor.io" }]  # Grant by tenant email
scope = { branch = "feature/contractor-work" }
permissions = ["tree:read", "tree:write", "snapshot:create"]

# ── Scope: specific branch ──────────────────────────────────────

[[policy]]
name = "intern-feature-branch"
effect = "allow"
subjects = [{ account = "intern@company.com" }]
scope = { branch = "feature/intern-task" }
permissions = ["tree:read", "tree:write", "snapshot:create"]

# ── Scope: registered path ──────────────────────────────────────
# Paths referenced here MUST be registered in config.toml first.

[[policy]]
name = "lock-production-config"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "config/production.toml" }   # Must be registered in config.toml
permissions = ["tree:write"]

[[policy]]
name = "ops-lead-production-config"
effect = "allow"
subjects = [{ account = "ops-lead@company.com" }]
scope = { path = "config/production.toml" }
permissions = ["tree:write"]                  # Explicit override for ops lead

[[policy]]
name = "secrets-restricted"
effect = "deny"
subjects = [{ role = "Developer" }, { role = "Contractor" }]
scope = { path = "secrets" }                  # Registered directory
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "no-force-delete"
effect = "deny"
subjects = [{ role = "Developer" }, { role = "Maintainer" }]
scope = "worktree"
permissions = ["tree:delete", "branch:protect"]
```

**Tree-specific `.wt-tree/access/policies.toml`** — Overrides for a single tree:

```toml
# services/auth-service/.wt-tree/access/policies.toml
# These policies apply ONLY to the auth-service tree.
# They override root .wt/access/ policies for this tree.

[[policy]]
name = "auth-team-owners"
effect = "allow"
subjects = [{ team = "auth-team" }]
scope = "tree"                                # "tree" here means THIS tree (auth-service)
permissions = ["tree:read", "tree:write", "tree:admin", "branch:create", "branch:merge", "branch:delete"]

# Register tree-specific paths in .wt-tree/config.toml, then reference here:

[[policy]]
name = "crypto-restricted"
effect = "allow"
subjects = [{ role = "security-reviewer" }]
scope = { path = "src/crypto" }               # Registered in .wt-tree/config.toml
permissions = ["tree:read", "branch:merge"]

[[policy]]
name = "crypto-deny-write-others"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "src/crypto" }
permissions = ["tree:write"]                  # Nobody writes crypto code except via merge review
```

**Tree-specific `.wt-tree/config.toml`**:

```toml
[tree]
name = "auth-service"
branch_strategy = "feature-branch"

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 300

# ── REGISTERED PATHS (tree-specific) ─────────────────────────────
# Paths registered here are relative to this tree's root.

[[registered_path]]
path = "src/crypto"
description = "Cryptographic implementations — security review required"

[[registered_path]]
path = "src/oauth.rs"
description = "OAuth2 flow — core auth logic"
```

#### Why Explicit Registration, Not Globs

1. **Predictability** — You know exactly what's controlled. No surprise matches, no forgotten exclusions. If it's not registered, it's not access-controlled at the path level (tree/branch/tenant scopes still apply).
2. **Auditability** — `config.toml` is a single source of truth for what paths have special access rules. You can read it and know instantly what's protected.
3. **Performance** — No pattern matching on every file operation. The server looks up registered paths directly. O(1) not O(n).
4. **Intentionality** — Like Terraform, you declare what you manage. You don't get a policy accidentally applying to a file you didn't intend because a glob matched too broadly.
5. **Simplicity** — New team members read the config and immediately understand the access model. No regex/glob expertise required.

**The rule**: If you want path-level access control on something, register it in `config.toml` first, then reference it in `policies.toml`. Unregistered paths fall through to the tree/branch/tenant scope — they're not unprotected, they just don't have path-specific rules.

#### How Declarative Access Works

1. User edits `.wt/access/*.toml` or `.wt-tree/access/*.toml` files (if they have `PolicyManage` or `TreeAdmin` permission)
2. For path-scoped policies, the path must first be registered in the corresponding `config.toml`
3. The bgprocess detects the change (like any other file change)
4. The bgprocess validates the access config locally (syntax, role references, permission names, registered path verification)
5. The bgprocess syncs the validated config to the server
6. The server applies the policies — they become **enforced immediately**
7. Other bgprocess clients sync the updated access config so everyone's local `.wt/` and `.wt-tree/` stay current
8. If a user without access management permission tries to edit access files, the server rejects the sync

#### Scope Hierarchy for Permissions

Permissions are evaluated in this order (most specific wins):

```
Registered Path (explicit file/dir)  ← Most specific, highest priority
  └── Branch (specific branch)
       └── Tree (entire tree)
            └── Tenant (all trees in the tenant)
                 └── Global (server-wide)    ← Broadest, lowest priority
```

Deny always beats Allow at the same scope level. A more specific scope overrides a broader one. This means a path-level Allow overrides a tree-level Deny, and a path-level Deny overrides a tree-level Allow.

**`.wt-tree/` overrides `.wt/` for that tree.** If the root `.wt/access/policies.toml` says the frontend team has read-only access to everything, but `services/auth-service/.wt-tree/access/policies.toml` says the frontend team has write access within auth-service, the tree-level override wins for auth-service.

**What `.wt/` and `.wt-tree/` do NOT contain:**
- Object store (blobs, snapshots, manifests) — managed by bgprocess externally
- Full history — managed by bgprocess and synced to server
- Large binary data — managed by bgprocess storage layer

### 3.5 Tenants, Permissions & IAM Model

The current protocol crate implements IAM with scopes at `Global → Tenant → Tree → Branch`, roles (Owner/Admin/Maintainer/Developer/Viewer), ABAC policies with conditions, and a `Resource::Subtree` with `path_prefix`. However, several critical gaps must be addressed in the spec updates:

#### What Exists in Code (Implemented)

- **Tenant model** (`iam::tenant`) — `Tenant` struct with id, name, slug, status (Active/Suspended), plan (Free/Pro/Enterprise/Custom), resource limits (max_accounts, max_trees), and ABAC attributes. However, the tenant model does not yet include email, cross-tenant access grants, or worktree visibility modes.
- **20 atomic permissions** across tree, branch, snapshot, sync, management, and admin categories
- **5 built-in roles** with superset hierarchy: Owner ⊃ Admin ⊃ Maintainer ⊃ Developer ⊃ Viewer
- **RBAC + ABAC engine** (`iam::engine`, 871 lines) with `AccessDecision`, `AccessRequest`, scope matching
- **Policy model** with Allow/Deny effects, subjects (Account/Team/Role/AllAuthenticated/Everyone), conditions with operators — but no `Tenant` subject type for cross-tenant policies
- **Scope hierarchy** with `covers()` semantics — Global covers Tenant covers Tree covers Branch
- **Resource::Subtree** with `path_prefix` field — but it falls back to `Scope::Tree` (path-level scoping is incomplete)

#### What Must Be Added to Specs (Gaps)

1. **Tenant model enrichment**: The `Tenant` struct needs an `email` field, and the `PolicySubject` enum needs a `Tenant(TenantId)` variant so policies can reference external tenants by username or email. The `Tenant` struct also needs a `visibility` field for worktree visibility modes (Private/Shared/Public). The simple `tenant_access` grants in `config.toml` must be specced as syntactic sugar that the server resolves into full IAM policies.

2. **Cross-tenant access**: The spec must define how a worktree owner grants access to other tenants. Two paths: simple grants in `config.toml` (by username/email with permission list), and full IAM policies in `policies.toml` (using `{ tenant = "..." }` subject type). The server must resolve tenant usernames and emails to `TenantId`s and evaluate cross-tenant policies during access checks.

3. **Worktree visibility modes**: The spec must define `Private` (default, owner-only), `Shared` (explicit tenant grants), and `Public` (all tenants can read, license compliance governs copying/export). Visibility mode is set in `.wt/config.toml` and enforced by the server.

4. **Registered path scope**: The `Scope` enum needs a `Path(TenantId, TreeId, RegisteredPath)` variant so that `Resource::Subtree` doesn't have to fall back to `Scope::Tree`. Paths are NOT globs — they are explicitly registered strings from `config.toml`. The server does an exact-match lookup, not pattern matching.

5. **`.wt/` vs `.wt-tree/` split**: The spec must define the two-folder model — `.wt/` for root worktree config and access, `.wt-tree/` for individual tree config and access. This replaces the single `.worktree/` folder in current specs. The inheritance rule (`.wt-tree/` overrides `.wt/` for that tree) must be formalized.

6. **Declarative file-based configuration**: The entire `.wt/access/` and `.wt-tree/access/` model described in §3.4 must be specced — how TOML files map to `Policy`, `Role`, and `Scope` structs, how the bgprocess validates and syncs them, and how the server applies them.

7. **Explicit path registration model**: The `path_prefix` on `Resource::Subtree` must be replaced with a `RegisteredPath` concept. Paths must be explicitly listed in `config.toml` before they can be referenced in `policies.toml`. No glob patterns, no wildcards — you register `src/crypto`, not `src/crypto/**`. The spec must define: how paths are registered, how policies reference them, and how the server validates that a policy's path scope matches a registered path.

8. **Scope resolution order**: The spec must define the evaluation order when multiple scopes apply. Currently the engine does scope matching but the priority rules (registered path > branch > tree > tenant > global, deny-wins-at-same-level, `.wt-tree/` overrides `.wt/`) are not formalized.

9. **Access config as version-controlled files**: Unlike traditional IAM where admins configure policies through an API, W0rkTree's access files are part of the worktree. They are versioned, snapshotted, and synced like any other file — but with special handling: only users with access management permission can modify them, and the server validates + enforces them on sync.

10. **Inheritance model**: The spec must define how nested child trees inherit access from the root:
    - Root `.wt/access/` policies cascade to all trees by default
    - Child trees override with their own `.wt-tree/access/` files
    - The `inherited` flag on `TreeAccessRule` (already in code) must be connected to this model
    - No `overrides/` subdirectory needed — each tree manages itself via its own `.wt-tree/`

11. **Condition-based access (ABAC extensions)**: The existing `AttributeCondition` with operators (Equals, Contains, StartsWith, GreaterThan, In, etc.) must be specced for use in `policies.toml` — e.g., time-based conditions, IP-based conditions, attribute-based conditions on user metadata.

12. **License compliance integration**: Path-level and file-level permissions must integrate with the license compliance system (§3.7). The access engine must evaluate license restrictions alongside IAM policies — even if a tenant has `tree:read` on a file, the license compliance layer can block export/copy if the license doesn't permit it.

#### Permission Scope Examples for Specs

The specs must include clear examples showing how the same permission behaves differently at each scope level:

```
tree:write at Scope::Global              → Can write to any file in any tree in any tenant (superadmin)
tree:write at Scope::Tenant("acme")      → Can write to any file in any tree owned by "acme"
tree:write at Scope::Tree("auth")        → Can write to any file in the auth-service tree
tree:write at Scope::Branch("feat")      → Can write to files only on the "feat" branch
tree:write at Scope::Path("src/crypto")  → Can write only to files under src/crypto (registered path)
```

And deny examples:

```
DENY tree:write at Path("src/crypto") for all_authenticated
  + ALLOW tree:write at Tree for team:auth-team
  = Auth team can write everywhere, but src/crypto is blocked for everyone (deny wins at path level)

ALLOW tree:write at Path("src/crypto") for role:security-reviewer (explicit override)
  = Security reviewers CAN write to src/crypto (more specific allow for specific subject)
```

And `.wt-tree/` override examples:

```
Root .wt/access/policies.toml:  ALLOW tree:read for team:frontend at scope:worktree
Tree .wt-tree/access/policies.toml (auth-service):  DENY tree:read for team:frontend at scope:tree
  = Frontend team can read all trees EXCEPT auth-service (tree-level override wins)
```

### 3.6 License Compliance

This is a **new first-class concept** not in any current spec. Git has no equivalent. Because W0rkTree natively supports multiple tenants accessing the same worktree — and supports public/open-source worktrees where any tenant can read — **license compliance must be enforced at the file level by the system itself**.

In Git, license compliance is a legal/honor-system concern. You put a `LICENSE` file in the repo and hope people respect it. There's nothing in Git that prevents someone from copying proprietary code out of a monorepo they have read access to.

In W0rkTree, **the server enforces license compliance at the file permission level**. If someone tries to export, copy, fork, or sync code that their license doesn't permit, the W0rkTree system blocks it.

#### How It Works

License compliance is configured in `.wt/config.toml` (root level) and `.wt-tree/config.toml` (tree level). Licenses are assigned to registered paths and files. The server enforces these licenses on every operation that moves code between tenants or outside the worktree.

**Root `.wt/config.toml`** — license configuration:

```toml
[worktree]
name = "my-project"
visibility = "public"                         # Public worktree — all tenants can read

[license]
default = "MIT"                               # Default license for the entire worktree
spdx_strict = true                            # Only allow valid SPDX identifiers

# ── Per-path license assignments ────────────────────────────────
# Registered paths can have their own license, overriding the default.

[[license.path]]
path = "libs/shared-models"
license = "MIT"

[[license.path]]
path = "services/auth-service"
license = "AGPL-3.0-only"                    # Copyleft — export/fork must preserve license

[[license.path]]
path = "services/billing-engine"
license = "proprietary"                       # Cannot be exported, copied, or forked by other tenants

[[license.path]]
path = "vendor/third-party-sdk"
license = "Apache-2.0"
attribution_required = true                   # Must include NOTICE file on export

[[license.path]]
path = "docs"
license = "CC-BY-4.0"                        # Creative Commons for documentation
```

**Tree-specific `.wt-tree/config.toml`** can also set licenses:

```toml
[tree]
name = "auth-service"

[license]
license = "AGPL-3.0-only"                    # Applies to this entire tree
```

#### What the Server Enforces

The server evaluates license compliance on operations that move code across tenant boundaries or outside the worktree:

| Operation | License Check |
|---|---|
| **Tenant reads file** | Allowed if tenant has `tree:read` AND the file's license permits read access for that tenant's license grant level |
| **Tenant forks/copies worktree** | Server checks every file's license. Proprietary files are excluded. Copyleft files carry their license. Attribution files include NOTICE. |
| **Tenant exports to Git** (`wt git export`) | License headers are injected/validated. Proprietary paths are blocked from export. A LICENSE file is auto-generated from the license config. |
| **Tenant syncs tree to their own worktree** | If a tenant tries to sync a tree with `license = "proprietary"` into their own worktree, the server blocks it unless the owning tenant has granted an explicit license grant. |
| **Public worktree browsing** | All tenants can read public worktrees, but the license on each path governs what they can DO with that code (copy, modify, redistribute). |
| **Cross-tenant staged visibility** | Staged snapshots are visible (who's working on what), but the actual file contents respect license restrictions — a tenant can see "Alice is working on billing-engine/src/pricing.rs" but cannot read the file contents if it's proprietary. |

#### License Grant Model

For proprietary code that a tenant wants to selectively share, license grants provide explicit permission:

```toml
# .wt/config.toml

[[license.grant]]
path = "services/billing-engine"
tenant = "partner-corp"                       # Grant by tenant username
grant = "read-only"                           # Can read but not copy/fork/export

[[license.grant]]
path = "services/billing-engine"
tenant = "contractor@dev.io"                  # Grant by tenant email
grant = "modify"                              # Can read and modify within this worktree, but not export

[[license.grant]]
path = "services/billing-engine/src/api.rs"
tenant = "partner-corp"
grant = "redistribute"                        # Full license to use, modify, and redistribute this specific file
```

Grant levels:
- **`read-only`** — can read the file within this worktree, cannot copy/export/fork
- **`modify`** — can read and modify within this worktree, cannot export/fork
- **`redistribute`** — full permission to use, modify, copy, export, and redistribute (equivalent to a permissive license for that tenant)

#### License Compliance in the Access Stack

License compliance sits **above** IAM in the enforcement stack:

```
1. IAM check:     Does this tenant have tree:read on this path?       → YES/NO
2. License check: Does this file's license permit this operation       → YES/NO
                   for this tenant (considering any license grants)?
3. Final decision: BOTH must pass. IAM YES + License YES = ALLOWED.
                   Either NO = DENIED.
```

This means a tenant can have full `tree:read` + `tree:write` IAM permissions on a path, but if the file is `license = "proprietary"` and they don't have a license grant, they **still cannot export or copy it**. IAM controls what you can do within the worktree; licensing controls what you can take out of it.

#### Why This Matters

1. **Open-source projects with proprietary modules** — A company publishes their framework as MIT but keeps their billing engine proprietary. In Git, nothing stops someone with repo access from copying the billing code. In W0rkTree, the server blocks it.
2. **Multi-tenant collaboration** — When multiple organizations share a worktree, each path can have different license terms. The server ensures each tenant only gets code they're licensed for.
3. **Compliance automation** — No more manual license audits. The W0rkTree server knows exactly what license every file is under and enforces it automatically on every sync, export, and fork operation.
4. **Code theft prevention** — If a contractor or partner has access to parts of your worktree, the license system ensures they cannot exfiltrate proprietary code even if their IAM grants are broad.

### 3.7 Staged Snapshot Visibility

This is a **new concept** not in any current spec. Git has no equivalent.

In Git, all work is invisible until a developer pushes to a remote. Nobody knows what anyone else is working on. Teams resort to standups, Slack messages, and ticket systems to communicate what's in flight. W0rkTree solves this at the VCS level.

**How it works:**

1. Developer edits files locally
2. The bgprocess captures snapshots (auto or manual) — this is local VCS, standard version control
3. The bgprocess syncs these snapshots to the server as **staged snapshots**, attributed to the user and their current branch
4. The server stores these staged snapshots on the **root W0rkTree** — they are visible to the team but **NOT part of any branch's history**
5. Other developers and staff can see: "Alice has staged snapshots touching `auth-service/src/oauth.rs` and `shared/models/user.rs` on `feature/oauth`"
6. When Alice is ready, she **explicitly pushes to the branch** — only then do her snapshots become part of the branch's actual snapshot history

**Key principle: Staged ≠ Pushed. Visible ≠ Merged.**

- Staged snapshots give the team **visibility into what's in flight** without polluting branch history
- A staged snapshot on the server means "this developer is working on these files" — it does NOT mean the branch has changed
- Pushing to a branch is still an explicit action — the developer decides when their work is ready
- This eliminates the Git problem of "I had no idea you were working on that file too" — in W0rkTree, you always know

**Visibility surfaces:**
- CLI: `wt status --team` shows staged snapshots from all team members
- CLI: `wt staged` shows all staged (unpushed) snapshots on the current tree
- Admin panel: real-time dashboard of staged activity per tenant/tree
- SDK: subscribe to staged snapshot events for tooling integration

**What this is NOT:**
- This is NOT real-time keystroke/line-level presence (like Google Docs)
- This is NOT broadcasting raw file edits as they happen
- This IS snapshot-level visibility — the bgprocess captures a snapshot, and that snapshot's metadata (which files changed, by whom, on which branch) becomes visible on the server
- The granularity is at the snapshot level, not the character level

### 3.8 Worktree-Level Ignore Patterns

This is the **full ignore system** that replaces `.gitignore`. In Git, `.gitignore` files are scattered throughout the repo, overlap with `.git/info/exclude` and global gitignore, and have confusing precedence rules. W0rkTree replaces this with a clean, hierarchical ignore system that follows the same root-authoritative model as the rest of the configuration.

#### Ignore File Locations

| File | Scope | Authority |
|---|---|---|
| `.wt/ignore` | Entire worktree (all trees) | **Root-level, authoritative** — cannot be overridden or negated by any tree |
| `.wt-tree/ignore` | The specific tree containing this file | **Tree-level, additive only** — can add patterns but CANNOT remove root-level ignores |
| Nested `.wt-tree/ignore` (subtree) | The specific subtree | **Subtree-level, additive only** — can add patterns but CANNOT negate parent tree or root ignores |

#### Pattern Layering

Ignore patterns are evaluated in layers, from broadest to most specific. Each layer can only ADD patterns — no layer can negate a pattern from a layer above it.

```
Root .wt/ignore                    ← Always wins. These patterns are absolute.
  └── Tree .wt-tree/ignore         ← Adds patterns for this tree only.
       └── Subtree .wt-tree/ignore ← Adds patterns for this subtree only.
```

**Example:**

```
# Root .wt/ignore — applies to ALL trees
.env
*.secret
build/
dist/
```

```
# services/auth-service/.wt-tree/ignore — adds patterns for auth-service only
coverage/
*.test.snap
```

```
# services/auth-service/plugins/oauth-plugin/.wt-tree/ignore — adds patterns for oauth-plugin subtree
fixtures/
```

The effective ignore list for `services/auth-service/plugins/oauth-plugin/` is the union of all three: `.env`, `*.secret`, `build/`, `dist/`, `coverage/`, `*.test.snap`, `fixtures/`.

#### Hierarchy Enforcement

**Root `.wt/ignore` is authoritative.** Tree-level `.wt-tree/ignore` can only ADD patterns, never negate root patterns. This prevents a child tree from accidentally including files the root project has explicitly excluded (e.g., secrets, build artifacts).

If a `.wt-tree/ignore` tries to un-ignore something the root ignores (using `!` negation against a root pattern), the bgprocess:
1. Logs a warning: `"WARN: .wt-tree/ignore in 'auth-service' attempted to negate root ignore pattern '*.secret' — root pattern takes precedence"`
2. The root pattern wins — the negation is silently discarded
3. The warning is visible via `wt ignore show` so the developer can see the conflict

#### Pattern Syntax

W0rkTree uses the same pattern syntax as `.gitignore` for easy migration:

| Pattern | Meaning |
|---|---|
| `*.log` | Ignore all files ending in `.log` in any directory |
| `build/` | Ignore the `build` directory and everything inside it (trailing `/` = directory only) |
| `/config.local` | Ignore `config.local` only at the root of the tree (leading `/` = relative to ignore file location) |
| `!important.log` | Un-ignore `important.log` (negation — only effective within the same level or lower, cannot negate a parent level) |
| `#` | Comment line |
| `doc/**/*.pdf` | Ignore all PDFs in the `doc` directory tree (doublestar glob) |
| `**/logs` | Ignore `logs` directory at any depth |

#### Built-in Defaults

The bgprocess **always** ignores the following, regardless of any configuration. These cannot be un-ignored:

```
# Hard-coded ignores — always active, cannot be overridden
.wt/
.wt-tree/
.git/
```

Additionally, the bgprocess ships with **soft defaults** that are applied unless explicitly un-ignored in `.wt/ignore`:

```
# Soft defaults — active by default, can be un-ignored in .wt/ignore if needed
node_modules/
target/
__pycache__/
.DS_Store
Thumbs.db
*.pyc
*.pyo
.tox/
.venv/
venv/
.gradle/
.idea/
.vscode/
*.swp
*.swo
*~
```

To un-ignore a soft default (e.g., if you actually want to track `.vscode/` settings), add `!.vscode/` to your `.wt/ignore`. This only works at the root level — tree-level `.wt-tree/ignore` cannot un-ignore soft defaults.

#### BGProcess Watcher Integration

The bgprocess filesystem watcher uses ignore patterns for **performance filtering at the event level**:

1. On startup (or when ignore files change), the bgprocess **compiles all ignore patterns** into a single optimized matcher (using a glob-set or similar structure)
2. Every filesystem event (create, modify, delete, rename) is checked against the compiled matcher **before** any further processing
3. Events matching an ignore pattern are **dropped immediately** — they do not trigger snapshot consideration, diff computation, or any other work
4. This means ignored files have **zero performance cost** — they don't slow down the bgprocess regardless of how many there are (e.g., `node_modules/` with 100,000 files generates zero events)

When an ignore file is modified:
1. The bgprocess detects the change to `.wt/ignore` or `.wt-tree/ignore`
2. The bgprocess recompiles the ignore pattern matcher
3. Any files that are NOW ignored but were previously tracked are **not retroactively removed from history** — they simply stop being tracked going forward
4. Any files that are NOW un-ignored begin appearing as new untracked files

#### Interaction with Registered Paths

A registered path in `config.toml` can **NEVER** be ignored — registration implies the path matters. If a file matches both an ignore pattern and a registered path, the registered path wins:

```
# .wt/ignore
*.log

# .wt/config.toml
[[registered_paths]]
path = "services/auth-service/audit.log"    # This file is tracked despite *.log ignore
```

The bgprocess logs a notice when this conflict is detected: `"NOTICE: 'services/auth-service/audit.log' matches ignore pattern '*.log' but is a registered path — tracking anyway"`

#### CLI Commands

| Command | What it does |
|---|---|
| `wt ignore show` | Display the effective ignore patterns for the current tree, merged from all levels. Shows which level each pattern comes from (root, tree, subtree, built-in). |
| `wt ignore show --tree <path>` | Display effective ignore patterns for a specific tree. |
| `wt ignore check <file>` | Check whether a specific file would be ignored, and explain why (which pattern from which level matched). |

**Example `wt ignore show` output:**

```
Effective ignore patterns for: services/auth-service

  [built-in/hard]    .wt/
  [built-in/hard]    .wt-tree/
  [built-in/hard]    .git/
  [built-in/soft]    node_modules/
  [built-in/soft]    target/
  [built-in/soft]    __pycache__/
  [built-in/soft]    .DS_Store
  [built-in/soft]    Thumbs.db
  [root .wt/ignore]  .env
  [root .wt/ignore]  *.secret
  [root .wt/ignore]  build/
  [root .wt/ignore]  dist/
  [tree .wt-tree/ignore]  coverage/
  [tree .wt-tree/ignore]  *.test.snap

  ⚠ 1 warning:
    .wt-tree/ignore line 5: '!*.secret' negates root pattern — ignored (root wins)
```

#### Migration from Git

`wt git import` automatically converts `.gitignore` files:

1. The root `.gitignore` is converted to `.wt/ignore`
2. `.gitignore` files in subdirectories are converted to `.wt-tree/ignore` in the corresponding tree (if the directory is a tree boundary) or merged into the nearest parent tree's `.wt-tree/ignore`
3. `.git/info/exclude` patterns are merged into `.wt/ignore`
4. Global gitignore (`~/.config/git/ignore`) patterns are noted in the migration log but NOT automatically imported (they belong in the user-level W0rkTree config)
5. Pattern syntax is preserved as-is (W0rkTree uses the same syntax)
6. The original `.gitignore` files are removed after conversion

### 3.9 Revert (Undo a Specific Snapshot)

This is W0rkTree's equivalent of `git revert` — creating a **NEW** snapshot that reverses the changes introduced by a specific previous snapshot. This is fundamentally different from `wt snapshot restore`, which goes back to a point in time. Revert undoes ONE snapshot's changes while keeping everything that came after.

**Key principle: History remains append-only.** Revert creates a new snapshot; it never modifies or removes existing ones. The revert snapshot is a first-class snapshot in the branch's history that happens to contain the inverse diff of the target snapshot.

#### How It Works

1. Developer identifies a snapshot that introduced a bug or unwanted change
2. Developer runs `wt revert <snapshot-id>`
3. The bgprocess computes the **inverse diff** of the target snapshot — every addition becomes a deletion, every deletion becomes an addition, every modification is reversed
4. The bgprocess applies the inverse diff to the current working tree
5. If the inverse diff applies cleanly (no conflicts), a new snapshot is created automatically with revert metadata
6. If the inverse diff conflicts with changes made since the target snapshot, the merge engine handles conflicts (same as a normal merge conflict — see §3.14)
7. The revert snapshot is synced to the server following the normal staged → push flow

#### CLI Commands

| Command | What it does |
|---|---|
| `wt revert <snapshot-id>` | Create a new snapshot that reverses the changes from the specified snapshot. |
| `wt revert <id1> <id2> <id3>` | Revert multiple snapshots in sequence. Each revert is applied in order, and a single combined revert snapshot is created. |
| `wt revert <snapshot-id> --no-snapshot` | Apply the revert to the working tree but do NOT create a snapshot yet. Lets the developer inspect the result before committing to it. |
| `wt revert --abort` | Abort an in-progress revert (if conflicts are pending). Restores the working tree to its pre-revert state. |

#### Revert of a Merge Snapshot

When reverting a merge snapshot, you must specify which parent to revert against, because a merge snapshot has two (or more) parents and the "changes introduced" depend on which parent you compare to:

```
wt revert <merge-snapshot-id> --parent 1    # Revert changes from the first parent's perspective
wt revert <merge-snapshot-id> --parent 2    # Revert changes from the second parent's perspective
```

If `--parent` is not specified for a merge snapshot, the bgprocess errors with a clear message:

```
Error: Snapshot abc123 is a merge snapshot with 2 parents.
       You must specify which parent to revert against:
         wt revert abc123 --parent 1   (revert changes relative to 'main')
         wt revert abc123 --parent 2   (revert changes relative to 'feature/oauth')
```

#### Conflict Handling

If the target snapshot's changes have been modified since (i.e., a later snapshot touched the same lines), the inverse diff will conflict. In this case:

1. The bgprocess marks the conflicted files (same conflict system as §3.14)
2. The developer resolves conflicts manually or with `wt merge resolve`
3. After resolution, `wt revert --continue` creates the revert snapshot

#### Revert Metadata

The revert snapshot records its nature in its metadata. This is stored in the snapshot's metadata envelope alongside the normal snapshot fields:

```toml
[snapshot]
id = "snap_a1b2c3d4e5f6"
tree = "services/auth-service"
branch = "main"
author = "alice@company.com"
timestamp = "2025-01-15T14:32:00Z"
message = "Revert: undo broken OAuth token rotation"

[snapshot.revert]
reverted_snapshot = "snap_9f8e7d6c5b4a"          # The snapshot being reverted
reverted_message = "Add OAuth token rotation"      # Original snapshot's message (for context)
parent_index = 0                                   # Which parent was used (0 for non-merge reverts)
```

#### Revert Chains

Multiple snapshots can be reverted in sequence:

```
wt revert snap_aaa snap_bbb snap_ccc
```

This applies the inverse of `snap_aaa`, then `snap_bbb`, then `snap_ccc` (in that order) and creates a single combined revert snapshot. The metadata records all reverted snapshot IDs:

```toml
[snapshot.revert]
reverted_snapshots = ["snap_aaa", "snap_bbb", "snap_ccc"]
```

If any individual revert in the chain conflicts, the entire chain pauses for conflict resolution. The developer resolves and then runs `wt revert --continue` to proceed with the remaining reverts.

### 3.10 Reflog (Operation History & Safety Net)

The reflog is a **chronological log of every operation that changed branch tips or the current working state**. It is the safety net that makes append-only history truly non-destructive — even if a branch is deleted, the reflog remembers where it pointed. In Git, the reflog exists but is local-only, poorly documented, and most developers don't know about it until they desperately need it. In W0rkTree, the reflog is a first-class feature that is synced to the server and surfaced in the CLI.

#### What Gets Logged

Every operation that changes a branch tip or the working state is recorded:

| Operation | Logged As |
|---|---|
| `wt push` | `push` — branch tip moves forward |
| `wt merge` | `merge` — branch tip moves to merge snapshot |
| `wt revert` | `revert` — branch tip moves to revert snapshot |
| `wt snapshot restore` | `restore` — working tree moves to a historical snapshot |
| `wt branch switch` | `switch` — current branch changes |
| `wt branch create` | `create` — new branch tip created |
| `wt branch delete` | `delete` — branch tip removed (soft delete) |
| `wt merge request merge` | `mr_merge` — merge request executed |
| bgprocess auto-merge | `auto_merge` — bgprocess merged remote changes |
| bgprocess auto-snapshot | `auto_snapshot` — bgprocess created an auto-snapshot |

#### Storage

The reflog is stored locally by the bgprocess in `.wt/reflog/`:

```
.wt/reflog/
├── main.log                    # Reflog for the 'main' branch
├── feature/oauth.log           # Reflog for 'feature/oauth' branch
├── feature/billing.log         # Reflog for 'feature/billing' branch
└── _global.log                 # Global reflog (all branches, all operations)
```

Each branch file contains chronological entries. The global log aggregates all entries across all branches.

The reflog is also **synced to the server** for recovery across machines. If a developer loses their local machine, they can recover their reflog from the server.

#### Entry Format

Each reflog entry contains:

| Field | Description |
|---|---|
| `timestamp` | When the operation occurred (UTC) |
| `operation` | Operation type (push, merge, revert, restore, switch, create, delete, auto_merge, auto_snapshot, mr_merge) |
| `before_ref` | The branch tip / working state before the operation (snapshot ID, or `null` for create) |
| `after_ref` | The branch tip / working state after the operation (snapshot ID, or `null` for delete) |
| `user` | Who performed the operation (email) |
| `message` | Human-readable description |

#### CLI Commands

| Command | What it does |
|---|---|
| `wt reflog` | Show the reflog for the current branch (most recent first). |
| `wt reflog --branch <name>` | Show the reflog for a specific branch. |
| `wt reflog --all` | Show the global reflog across all branches. |
| `wt reflog --limit <N>` | Show only the last N entries. |
| `wt reflog --since <date>` | Show entries since a specific date. |
| `wt snapshot restore reflog@{N}` | Restore the working tree to the state at reflog entry N (0 = most recent). |

**Example `wt reflog` output:**

```
Branch: main

  #   Timestamp                 Op           Before         After          User                Message
  ─── ───────────────────────── ──────────── ────────────── ────────────── ─────────────────── ──────────────────────────────
  0   2025-01-15 14:32:00 UTC   revert       snap_f1e2d3    snap_a1b2c3    alice@company.com   Revert: undo broken OAuth rotation
  1   2025-01-15 13:10:00 UTC   push         snap_c4d5e6    snap_f1e2d3    bob@company.com     Push: 3 snapshots to main
  2   2025-01-15 11:45:00 UTC   merge        snap_7a8b9c    snap_c4d5e6    alice@company.com   Merge: feature/oauth into main
  3   2025-01-14 17:00:00 UTC   auto_merge   snap_1d2e3f    snap_7a8b9c    bgprocess           Auto-merge: remote changes (no conflicts)
  4   2025-01-14 16:30:00 UTC   push         snap_4g5h6i    snap_1d2e3f    alice@company.com   Push: update auth middleware
  ...

  Showing 5 of 127 entries. Use --limit or --since to filter.
```

#### Recovery via Reflog

The reflog enables recovery from almost any situation:

- **Recover a deleted branch**: `wt branch create <name> --from reflog@{N}` where N is the reflog entry before the delete
- **Undo a bad merge**: `wt snapshot restore reflog@{N}` where N is the entry before the merge
- **Recover from a bad revert**: `wt snapshot restore reflog@{N}` where N is the entry before the revert
- **See what changed while you were away**: `wt reflog --since "2 days ago"`

Soft-deleted branches are fully recoverable via the reflog. When a branch is deleted, the reflog records the final branch tip. Recovering the branch is as simple as creating a new branch pointing to that snapshot.

#### Retention Policy

Reflog retention is configurable in `.wt/config.toml`:

```toml
# .wt/config.toml

[reflog]
retention_days = 90                     # Keep reflog entries for 90 days (default)
max_entries_per_branch = 10000          # Maximum entries per branch log (oldest pruned first)
sync_to_server = true                   # Sync reflog to server for cross-machine recovery (default: true)
```

Tree-level override in `.wt-tree/config.toml`:

```toml
# services/auth-service/.wt-tree/config.toml

[reflog]
retention_days = 180                    # Keep auth-service reflog longer (security audit trail)
```

The bgprocess prunes expired reflog entries during its periodic maintenance cycle. Pruned entries are removed from the local `.wt/reflog/` files and from the server.

### 3.11 Shallow History & Partial Sync

W0rkTree supports **large histories and partial data** as first-class concepts. Not every developer needs every snapshot from the beginning of time, and not every developer needs every tree's content. Shallow history and partial sync are client-side optimizations — the server always has complete history.

#### Shallow Initialization

```
wt init --from <url> --depth <N>
```

This initializes a worktree with only the last N snapshots per branch. The bgprocess stores a marker indicating the history is shallow and what depth was requested.

#### Lazy History Loading

When a user requests older history than what's available locally, the bgprocess **transparently fetches it from the server**:

- `wt log` scrolling back past the shallow boundary → bgprocess fetches older snapshots on-demand
- `wt diff <old-snapshot>..<new-snapshot>` where the old snapshot isn't local → bgprocess fetches it
- `wt blame <file>` where the history for that file extends past the shallow boundary → bgprocess fetches what's needed

The user never sees a "history not available" error — the bgprocess handles it transparently with a brief loading indicator.

#### Partial Tree Sync

```
wt init --from <url> --trees <tree1,tree2>
```

This syncs only the specified child trees. All other trees are represented as **stubs** — they have metadata (tree name, latest snapshot ID, file list) but no file content. This is useful for large monorepos where a developer only works on a few services.

**Stub trees:**
- Appear in `wt status` as `[stub]` with their metadata
- Cannot be edited or snapshotted until materialized
- Can be materialized on-demand: `wt sync --tree <name>` downloads the full content
- Their metadata is always current — the bgprocess syncs stub metadata even for non-materialized trees

#### Lazy Blob Loading

Large files are not downloaded until accessed. The bgprocess stores a **stub/pointer** locally:

1. On sync, the bgprocess downloads file metadata (hash, size, MIME type) but not the content for large files
2. When a tool opens the file, the bgprocess **transparently serves the real content** by fetching it from the server on-demand
3. This uses platform-specific mechanisms: FUSE on Linux, ProjFS on Windows, FUSE-T on macOS (same mechanism as §3.15 Large File Handling)
4. Once fetched, the blob is cached locally for future access

#### Interaction with Auto-Snapshots

The bgprocess only auto-snapshots files that are **fully materialized locally**, not stubs. If a developer hasn't materialized a tree, changes to that tree on the server are tracked by the server but don't trigger local auto-snapshots.

#### Depth Expansion and Full Materialization

| Command | What it does |
|---|---|
| `wt sync --deepen <N>` | Fetch N more snapshots of history beyond the current shallow boundary. |
| `wt sync --full` | Download everything — convert from shallow to full. This fetches all history and all tree content. |
| `wt sync --tree <name>` | Materialize a specific stub tree (download its full content). |
| `wt sync --tree <name> --depth <N>` | Materialize a tree but only fetch the last N snapshots of its history. |

#### Interaction with `wt blame`

If the history for a file isn't fully local, the bgprocess transparently fetches what's needed from the server. The `blame` command works identically regardless of whether the history is shallow or full — the user never needs to know.

#### Configuration

```toml
# .wt/config.toml

[shallow]
enabled = true                          # Whether this worktree uses shallow mode
default_depth = 50                      # Default number of snapshots per branch
auto_deepen = true                      # Automatically fetch more history when needed (default: true)
auto_deepen_batch = 100                 # How many snapshots to fetch when auto-deepening
lazy_blobs = true                       # Enable lazy blob loading for large files
materialized_trees = [                  # Which trees are fully materialized
    "services/auth-service",
    "libs/shared-models",
]
stub_trees = [                          # Which trees are stubs (metadata only)
    "services/billing-engine",
    "services/api-gateway",
    "frontend/web-app",
]
```

#### Server-Side

The server always has **complete history**. Shallow is a client-side optimization only. The server:
- Responds to depth-limited sync requests by sending only the requested number of snapshots
- Serves on-demand history requests when the bgprocess auto-deepens
- Tracks which clients have shallow vs. full history (for efficient delta sync)
- Never discards history based on client shallow settings

### 3.12 Tags & Releases

Tags and releases are W0rkTree's answer to `git tag` — but with releases as a **first-class concept** rather than an afterthought bolted on by hosting platforms (like GitHub Releases).

#### Tags

**Tags** are named, immutable references to a specific snapshot. They mark significant points in history (releases, milestones, deployments).

**Two types:**

| Type | Description | Created with |
|---|---|---|
| **Lightweight tag** | Just a name pointing to a snapshot ID. No metadata beyond the name. | `wt tag create <name>` |
| **Annotated tag** | Includes a message, author, timestamp, and optional signature. The full version for releases and milestones. | `wt tag create <name> --message <msg>` |

**Key properties:**
- Tags are **global to the worktree** (not per-tree). A tag points to a snapshot, which belongs to a specific tree+branch.
- Tags are **immutable** — once created, a tag cannot be moved to point to a different snapshot. You must delete and recreate.
- Tag naming supports hierarchical names with `/` separators (e.g., `v1.0`, `release/2024.1`, `auth-service/v2.3`)
- Tag deletion is a **soft delete** — the tag is marked as deleted but recoverable via reflog (see §3.10)

**Tag metadata (annotated tag):**

```toml
[tag]
name = "v1.0.0"
snapshot = "snap_a1b2c3d4e5f6"
tree = "services/auth-service"
branch = "main"
type = "annotated"

[tag.annotation]
message = "Release v1.0.0 — stable OAuth implementation"
author = "alice@company.com"
timestamp = "2025-01-15T14:32:00Z"
signature = "ed25519:base64encodedSignature..."     # Optional
```

**Tag metadata (lightweight tag):**

```toml
[tag]
name = "deploy/staging-2025-01-15"
snapshot = "snap_a1b2c3d4e5f6"
tree = "services/auth-service"
branch = "main"
type = "lightweight"
```

#### Releases

**Releases** are a first-class concept built on top of tags. A release extends a tag with release notes, attached artifacts (binaries, archives), and a status.

**Release metadata:**

```toml
[release]
tag = "v1.0.0"
status = "stable"                                # "draft", "pre-release", or "stable"
created_by = "alice@company.com"
created_at = "2025-01-15T15:00:00Z"

[release.notes]
format = "markdown"
content = """
## What's New in v1.0.0

- Stable OAuth 2.0 implementation
- Token rotation with configurable intervals
- Full PKCE support

### Breaking Changes

- Removed deprecated `/auth/v0/` endpoints
"""

[[release.artifact]]
name = "auth-service-linux-amd64"
path = "artifacts/auth-service-linux-amd64.tar.gz"
hash = "sha256:a1b2c3d4..."
size_bytes = 15728640
mime_type = "application/gzip"

[[release.artifact]]
name = "auth-service-windows-amd64"
path = "artifacts/auth-service-windows-amd64.zip"
hash = "sha256:e5f6a7b8..."
size_bytes = 16252928
mime_type = "application/zip"
```

**Release properties:**
- A release is always tied to a tag (which is tied to a snapshot)
- Releases can be marked as `draft` (not visible to non-owners), `pre-release` (visible but flagged), or `stable`
- Release artifacts are stored in the server's object store, content-addressed like everything else
- Releases are stored on the server and visible via the admin panel and API

#### Access Control

| Permission | Who needs it | What it allows |
|---|---|---|
| `tag:create` | Developers, Maintainers | Create new tags |
| `tag:delete` | Maintainers, Admins | Delete tags (soft delete) |
| `release:create` | Maintainers, Admins | Create releases from tags |
| `release:delete` | Admins, Owners | Delete releases |

These permissions are added to the existing permission set defined in the access system (see §3.5).

#### Tag Sync

Tags sync between the bgprocess and server like branches:
- When a tag is created locally, the bgprocess syncs it to the server
- When a tag is created on the server (by another developer), the bgprocess pulls it down
- Tag deletions are synced as soft deletes

#### Git Compatibility

| Operation | Behavior |
|---|---|
| `wt git export` | Converts W0rkTree tags to Git tags. Annotated tags preserve message/author/timestamp. Lightweight tags become lightweight Git tags. |
| `wt git import` | Converts Git tags to W0rkTree tags. Annotated Git tags preserve their message and author. Lightweight Git tags become lightweight W0rkTree tags. |

#### CLI Commands

| Command | What it does |
|---|---|
| `wt tag list` | List all tags. Shows name, type, snapshot, and date. |
| `wt tag create <name>` | Create a lightweight tag at the current snapshot. |
| `wt tag create <name> --message <msg>` | Create an annotated tag with a message. |
| `wt tag create <name> --snapshot <id>` | Tag a specific snapshot (not the current one). |
| `wt tag show <name>` | Show tag details (snapshot, tree, branch, annotation if annotated). |
| `wt tag delete <name>` | Delete a tag (soft delete, recoverable via reflog). |
| `wt release list` | List all releases. Shows tag, status, date, and artifact count. |
| `wt release create <tag> --notes <file-or-string>` | Create a release from an existing tag. Notes can be a file path or inline string. |
| `wt release create <tag> --notes <notes> --artifact <path>` | Create a release with attached artifacts. Repeat `--artifact` for multiple files. |
| `wt release show <tag>` | Show release details (notes, artifacts, status). |
| `wt release delete <tag>` | Delete a release. |
| `wt release download <tag> --output <path>` | Download release artifacts to a local directory. |

### 3.13 Archiving & Export

W0rkTree provides a built-in archive system for producing **distributable archives from tree content** — without any VCS metadata. This is the equivalent of `git archive`, but integrated with W0rkTree's tree model, ignore system, and license compliance.

#### How It Works

The archive operation runs through the bgprocess — it reads from the local object store, so no server round-trip is needed if the content is local. For shallow histories, the bgprocess fetches the needed snapshot from the server before archiving.

The archive output contains **only file content** — no `.wt/`, `.wt-tree/`, or any other VCS metadata. The result is a clean, distributable archive.

#### CLI Commands

| Command | What it does |
|---|---|
| `wt archive <format>` | Export the current tree state as an archive. Supported formats: `tar.gz`, `tar.bz2`, `tar.xz`, `zip`. |
| `wt archive <format> --snapshot <id>` | Archive a specific snapshot's content (not the current working tree). |
| `wt archive <format> --branch <name>` | Archive the tip of a specific branch. |
| `wt archive <format> --tree <path>` | Archive a specific child tree only (not the entire worktree). |
| `wt archive <format> --prefix <dir>` | Prepend a directory prefix to all paths in the archive (e.g., `--prefix my-project-v1.0/`). |
| `wt archive <format> --output <path>` | Write the archive to a specific file path (default: stdout or auto-named in current directory). |

**Examples:**

```
# Archive current tree as tar.gz
wt archive tar.gz --output ./release.tar.gz

# Archive a specific snapshot as zip with a prefix
wt archive zip --snapshot snap_a1b2c3 --prefix my-project-v1.0/ --output my-project-v1.0.zip

# Archive only the auth-service tree
wt archive tar.gz --tree services/auth-service --output auth-service.tar.gz

# Archive a specific branch tip
wt archive tar.xz --branch release/1.0 --output release-1.0.tar.xz
```

#### Ignore Pattern Interaction

Archived content respects `.wt/ignore` and `.wt-tree/ignore` — ignored files are not included in the archive. Since ignored files are never captured in snapshots, they are naturally excluded from any archive produced from a snapshot. For archives of the current working tree, the ignore patterns are applied as a filter.

#### License Compliance Integration

When exporting a **public worktree**, license compliance is enforced:

- Paths with `license = "proprietary"` are **EXCLUDED** from the archive by default
- Paths with copyleft licenses (e.g., AGPL, GPL) include their license file in the archive
- Paths with `attribution_required = true` include NOTICE files
- The `--include-all` flag overrides license exclusions, but requires the user to have `release:create` permission and appropriate license grants for all included paths
- A `LICENSE` file is auto-generated at the archive root summarizing the licenses of all included content

#### Release Integration

Archives can be auto-generated as release artifacts:

```
wt release create v1.0.0 --notes "Release notes" --auto-archive tar.gz,zip
```

This creates the release AND generates `tar.gz` and `zip` archives as attached artifacts.

### 3.14 Merge Strategy & Conflict Resolution

This is one of W0rkTree's **most critical differentiators**. Git's merge/conflict UX is terrible — confusing conflict markers, a rebase vs. merge philosophical war, and no automatic conflict handling. W0rkTree must be fundamentally better.

**Key principle: No rebase. Ever.** History is append-only. Divergence is handled by merging, not by rewriting. This eliminates an entire class of Git disasters (force-push overwrites, lost commits during interactive rebase, diverged histories after rebase).

#### BGProcess Automatic Merge

The bgprocess continuously syncs remote branch updates. When it detects divergence (local changes and remote changes on the same branch), it attempts **automatic merge**:

| Scenario | BGProcess Behavior |
|---|---|
| Changes on **different files** | **Auto-merged silently.** The bgprocess creates a merge snapshot and logs it. No user intervention needed. |
| Changes on the **same file, different regions** (non-overlapping hunks) | **Auto-merged silently** with a notification: "bgprocess auto-merged remote changes into your working tree." |
| Changes **conflict** (same lines modified) | The bgprocess **pauses auto-sync for that branch**, marks files as conflicted, and notifies the user. |

The auto-merge behavior ensures that the common case (changes don't conflict) is completely invisible to the developer. They just keep working, and the bgprocess keeps their branch up to date.

#### Conflict Markers (Improved)

When conflicts DO happen, W0rkTree uses **improved conflict markers** that are far clearer than Git's confusing `<<<<<<<` / `=======` / `>>>>>>>` format:

```
<<<< YOUR CHANGES (branch: feature/auth, snapshot: snap_f1e2d3) >>>>
    let token = generate_token(user, Duration::hours(24));
    validate_token_signature(&token)?;
==== COMMON ANCESTOR (snapshot: snap_7a8b9c) ====
    let token = generate_token(user);
==== REMOTE CHANGES (branch: main, by: bob@company.com, snapshot: snap_c4d5e6) >>>>
    let token = generate_token(user, Duration::hours(12));
    log_token_generation(&token);
<<<< END CONFLICT >>>>
```

**Key improvements over Git:**
- Clear labels with branch name, author, and snapshot ID
- **Three-way display** — shows the common ancestor version alongside both sides, so the developer can see what each side actually changed
- Labeled section boundaries that are easy to parse visually

#### Machine-Readable Conflict Metadata

In addition to inline markers, W0rkTree stores machine-readable conflict metadata in `.wt/conflicts/` as JSON. This allows tools (editors, IDEs, CI) to parse conflicts without regex:

```
.wt/conflicts/
├── services/auth-service/src/oauth.rs.conflict.json
└── libs/shared-models/src/user.rs.conflict.json
```

Each conflict file contains structured data:

```json
{
  "file": "services/auth-service/src/oauth.rs",
  "conflicts": [
    {
      "line_start": 42,
      "line_end": 48,
      "ours": {
        "branch": "feature/auth",
        "snapshot": "snap_f1e2d3",
        "author": "alice@company.com",
        "content": "    let token = generate_token(user, Duration::hours(24));\n    validate_token_signature(&token)?;\n"
      },
      "ancestor": {
        "snapshot": "snap_7a8b9c",
        "content": "    let token = generate_token(user);\n"
      },
      "theirs": {
        "branch": "main",
        "snapshot": "snap_c4d5e6",
        "author": "bob@company.com",
        "content": "    let token = generate_token(user, Duration::hours(12));\n    log_token_generation(&token);\n"
      }
    }
  ]
}
```

#### Conflict Resolution CLI

| Command | What it does |
|---|---|
| `wt merge status` | Show all current conflicts — files, line ranges, and which branches are involved. |
| `wt merge resolve <file> --ours` | Take our version for the entire file. |
| `wt merge resolve <file> --theirs` | Take their version for the entire file. |
| `wt merge resolve <file>` | Mark a file as manually resolved (after the user edits the file to fix conflicts). |
| `wt merge abort` | Abort the in-progress merge. Restores the working tree to its pre-merge state. |
| `wt merge finish` | Complete the merge after all conflicts are resolved. Creates the merge snapshot. |

After resolution, the user can also just run `wt snapshot` — the bgprocess recognizes that all conflicts are resolved and creates the merge snapshot automatically.

#### Merge Strategies

| Strategy | CLI Flag | Behavior |
|---|---|---|
| **Auto** (default) | *(no flag)* | BGProcess attempts automatic merge. Falls back to manual for conflicts. |
| **Manual** | `--strategy manual` | Always stop and let the user review, even if auto-merge is possible. Useful for careful, audited merges. |
| **Ours** | `--strategy ours` | Always take our version on conflict. Non-conflicting changes from the source are still merged. |
| **Theirs** | `--strategy theirs` | Always take their version on conflict. Non-conflicting changes from ours are still merged. |

#### Binary File Conflicts

Binary files cannot be diffed line-by-line. When both sides modify a binary file:

1. The bgprocess **always** flags it as a conflict (binary files are never auto-merged)
2. The developer must choose: `wt merge resolve <file> --ours`, `wt merge resolve <file> --theirs`, or provide a manually merged version by replacing the file and running `wt merge resolve <file>`
3. The conflict metadata JSON includes `"type": "binary"` with size and hash info for both sides

#### Explicit Merge Commands

| Command | What it does |
|---|---|
| `wt merge <source>` | Merge a source branch into the current branch. This is an explicit action distinct from auto-sync. |
| `wt merge <source> --into <target>` | Merge into a specific branch (not the current one). |
| `wt merge <source> --no-ff` | Always create a merge snapshot even if fast-forward is possible (preserves branch topology in history). |
| `wt merge <source> --ff-only` | Only merge if fast-forward is possible. Fail with an error otherwise. |
| `wt merge <source> --strategy <name>` | Use a specific merge strategy (see table above). |

### 3.15 Large File & Binary Handling

W0rkTree handles large files **natively** — no separate LFS system, no `.gitattributes` configuration, no special commands. Large files just work. The bgprocess automatically detects, chunks, deduplicates, and lazily loads large files based on configurable thresholds.

#### Automatic Detection

The bgprocess detects large files based on a configurable size threshold. Files above the threshold are automatically treated as "large" and handled with chunked storage and lazy loading. No developer action required.

#### Chunked Storage

Large files are split into **content-defined chunks** using a rolling hash algorithm (FastCDC) for deduplication and efficient transfer:

- **Content-defined chunking** means chunk boundaries are determined by the file's content, not fixed offsets. This means that if you insert data at the beginning of a file, most chunks are unchanged — only the chunks around the edit point change.
- **Deduplication** is automatic — if two large files (or two versions of the same file) share chunks, the chunks are stored once.
- Chunk size is configurable (default: 4MB average), with minimum and maximum bounds to prevent degenerate cases.

#### Lazy Loading

When the bgprocess syncs from the server, large files are represented as **stubs** locally:

1. The stub file contains the original file's hash, size, chunk count, and MIME type
2. The bgprocess **transparently serves the real content** when the file is accessed, using platform-specific mechanisms:
   - **Linux**: FUSE filesystem
   - **Windows**: ProjFS (Projected File System)
   - **macOS**: FUSE-T
3. When a tool opens the stub file, the bgprocess intercepts the read, fetches the chunks from the local cache or server, assembles the file, and serves it to the tool
4. Once fetched, chunks are cached locally for future access
5. The user never sees the stub — to them, the file appears normal (correct size, correct permissions, correct content)

#### Upload Flow

1. BGProcess detects a new or modified large file (above threshold)
2. BGProcess chunks the file using FastCDC
3. Each chunk is content-addressed (SHA-256 hash)
4. BGProcess checks which chunks already exist on the server (dedup check)
5. Only new chunks are uploaded
6. BGProcess stores a local manifest linking the file to its chunks
7. The snapshot records the file's manifest hash, not the raw content

#### Diff Behavior

| File Type | Diff Behavior |
|---|---|
| **Binary file** (detected by MIME type or content analysis) | Size-change diff only: "Binary file changed (old: 1.2MB → new: 1.3MB, hash: abc → def)" |
| **Large text file** (above threshold but text content) | Full text diff (line-level). Stored chunked, but diffed normally. |
| **Binary with `--binary-as-text`** | Attempt text diff on binary files (useful for text-based binary formats like SVG, CSV, JSON) |

#### Configuration

Root `.wt/config.toml`:

```toml
[large_files]
threshold_bytes = 10485760          # 10MB — files larger than this are chunked
chunk_size_bytes = 4194304          # 4MB average chunk size
lazy_loading = true                 # Fetch large files on demand, not on sync
preload_patterns = ["*.rs", "*.ts"] # Always fully download these even if large
```

Tree-level override in `.wt-tree/config.toml`:

```toml
[large_files]
threshold_bytes = 5242880           # 5MB threshold for this tree (assets-heavy)
```

The root `.wt/config.toml` settings are authoritative. Tree-level `.wt-tree/config.toml` can adjust thresholds for their tree but cannot disable large file handling entirely.

#### Git LFS Interop

| Operation | Behavior |
|---|---|
| `wt git import` | Detects Git LFS pointers (files containing `version https://git-lfs.github.com/spec/v1`) and converts them to W0rkTree's native large file format. The actual LFS objects are fetched and imported. |
| `wt git export` | Converts W0rkTree large files back to Git LFS pointers if the target Git remote uses LFS. If not, large files are included directly in the Git repo. |

#### Quota Enforcement

The server enforces storage quotas per tenant. Large file chunk storage counts toward the tenant's total storage quota. The server rejects uploads that would exceed the quota, and the bgprocess reports the error to the user:

```
Error: Upload rejected — tenant storage quota exceeded (used: 4.8GB / limit: 5.0GB).
       This file requires 312MB of new storage. Contact your admin to increase the quota.
```

### 3.16 Diff Configuration & Capabilities

W0rkTree's diff system goes beyond the basic `wt diff` — it provides a full-featured, configurable diff engine that supports multiple targets, modes, filters, and output formats. All diff computation is **local** — no server round-trip required (for shallow histories, the bgprocess lazily fetches needed snapshots before diffing).

#### Diff Targets

| Command | What it compares |
|---|---|
| `wt diff` | Working tree vs. latest snapshot (changes since last snapshot) |
| `wt diff <snapshot-id>` | Working tree vs. a specific snapshot |
| `wt diff <id1>..<id2>` | Two specific snapshots against each other |
| `wt diff --branch <name>` | Current branch tip vs. another branch tip |
| `wt diff --branch <a>..<b>` | Two branch tips against each other |
| `wt diff --staged` | Staged snapshots that haven't been pushed (local vs. server state) |

#### Diff Modes

| Flag | What it does |
|---|---|
| `--stat` | Summary only — files changed, insertions, deletions (like `git diff --stat`) |
| `--name-only` | List changed file paths only, no diff content |
| `--word` | Word-level diff instead of line-level (useful for prose, documentation) |
| `--color` / `--no-color` | Force color output on or off (default: auto-detect terminal) |
| `--unified <N>` | Number of context lines around changes (default: 3) |

#### Rename & Copy Detection

The diff engine (in the protocol crate's `feature::diff::compute` module) detects file renames and copies:

| Flag | What it does |
|---|---|
| `--rename-threshold <0-100>` | Percentage similarity to consider a rename (default: 50%). A file deleted at path A and a similar file created at path B is detected as a rename if similarity ≥ threshold. |
| `--find-copies` | Also detect file copies (a new file that's similar to an existing file). More expensive than rename detection. |

In diff output, renames and copies are shown clearly:

```
Renamed: services/auth-service/src/auth.rs → services/auth-service/src/oauth.rs (92% similar)
Copied:  libs/shared-models/src/user.rs → services/billing-engine/src/user.rs (100% similar)
```

#### Filtering

| Flag | What it does |
|---|---|
| `-- <path>` | Diff only specific files or directories. |
| `--filter=M` | Only show modified files. |
| `--filter=A` | Only show added files. |
| `--filter=D` | Only show deleted files. |
| `--filter=R` | Only show renamed files. |
| `--filter=C` | Only show copied files (requires `--find-copies`). |
| `--filter=MAD` | Combine filters (modified, added, and deleted). |

#### Output Formats

| Flag | Format |
|---|---|
| *(default)* | Colored unified diff in the terminal. |
| `--format=patch` | Standard unified diff patch format, compatible with the `patch` command. Suitable for sharing diffs as files. |
| `--format=json` | Machine-readable JSON diff output for tooling and CI integration. |

**JSON output example:**

```json
{
  "files": [
    {
      "path": "services/auth-service/src/oauth.rs",
      "status": "modified",
      "hunks": [
        {
          "old_start": 42,
          "old_lines": 3,
          "new_start": 42,
          "new_lines": 5,
          "changes": [
            { "type": "context", "line": 42, "content": "fn generate_token(user: &User) -> Token {" },
            { "type": "delete", "line": 43, "content": "    Token::new(user.id)" },
            { "type": "add", "line": 43, "content": "    let expiry = Duration::hours(24);" },
            { "type": "add", "line": 44, "content": "    Token::new(user.id, expiry)" },
            { "type": "context", "line": 44, "content": "}" }
          ]
        }
      ],
      "stats": { "insertions": 2, "deletions": 1 }
    }
  ],
  "summary": { "files_changed": 1, "insertions": 2, "deletions": 1 }
}
```

#### Binary File Diff

| Scenario | Output |
|---|---|
| Default | `Binary file changed: services/assets/logo.png (old: 1.2MB, sha256:abc1... → new: 1.3MB, sha256:def2...)` |
| `--binary-as-text` | Attempt text diff on binary files. Useful for formats like SVG, CSV, large JSON. Falls back to binary summary if the content isn't valid text. |

#### BGProcess Integration

The bgprocess uses the protocol crate's `feature::diff::compute` module for all diff computation. All computation is local — the bgprocess reads from the local object store. For shallow histories where the needed snapshots aren't local, the bgprocess transparently fetches them from the server before computing the diff.

### 3.17 Configuration Hierarchy

W0rkTree uses a **layered configuration system** where more specific settings override broader ones. The `.wt/` root is **authoritative** — it sets the rules that all trees must follow. Trees can customize within the bounds the root allows.

#### Configuration Levels (Highest Priority First)

```
1. Environment variables          ← Overrides everything (for CI/CD, scripting)
   WT_SERVER=https://...
   WT_SYNC_AUTO=false
   WT_TENANT=acme-corp

2. .wt-tree/config.toml           ← Tree-specific settings (this tree only)
   Can customize: auto-snapshot rules, branch strategy, large file thresholds,
   tree-specific ignore patterns, tree-specific registered paths
   CANNOT override: server connection, tenant identity, root-level access,
   root-level ignore patterns, root-level license defaults

3. .wt/config.toml                ← Root worktree settings (project-wide)
   Authoritative for: server connection, sync settings, tenant identity,
   root-level ignore patterns, root-level registered paths, root-level
   license config, worktree visibility, large file defaults

4. ~/.config/w0rktree/config.toml ← User-level global settings
   User identity (name, email), default server, default sync preferences,
   editor/diff tool preferences, global ignore patterns

5. /etc/w0rktree/config.toml      ← System-level settings (admin-managed)
   (Windows: %PROGRAMDATA%\W0rkTree\config.toml)
   Server allowlist, security policies, proxy settings
```

#### Access & Permission Hierarchy

The hierarchy for access/permissions is **STRICT** — it follows a ceiling model where each level can restrict but never expand:

- Root `.wt/access/` defines the **ceiling** — it sets the maximum permissions any tree can grant
- Tree `.wt-tree/access/` can **RESTRICT** access further but CANNOT grant more than the root allows
- Subtree (`.wt-tree/` nested inside another `.wt-tree/`) follows the same rule — it can restrict but not expand what its parent tree allows

**Example:** If root says "frontend-team gets read-only", a `.wt-tree/access/` CANNOT upgrade them to write. But if root says "everyone gets write", a `.wt-tree/` CAN restrict its own tree to read-only for certain roles.

```
Root .wt/ policies (ceiling)
  └── Tree .wt-tree/ policies (can restrict, not expand)
       └── Subtree .wt-tree/ policies (can restrict, not expand parent tree)
            └── Registered path policies (most specific)
```

#### Subtree Nesting Layout

```
my-project/                      ← Root Worktree (.wt/)
├── .wt/
│   ├── config.toml              # Root config — authoritative
│   ├── ignore                   # Root ignore — cannot be overridden
│   └── access/
│       ├── roles.toml           # Root roles — ceiling for all trees
│       └── policies.toml        # Root policies — cascade to all trees
│
├── services/
│   ├── auth-service/            ← Tree (.wt-tree/)
│   │   ├── .wt-tree/
│   │   │   ├── config.toml     # Can customize within root bounds
│   │   │   ├── ignore          # Adds patterns, cannot negate root
│   │   │   └── access/
│   │   │       └── policies.toml  # Can restrict, not expand root
│   │   │
│   │   ├── plugins/
│   │   │   └── oauth-plugin/   ← Subtree (.wt-tree/ inside .wt-tree/)
│   │   │       ├── .wt-tree/
│   │   │       │   ├── config.toml  # Can customize within auth-service bounds
│   │   │       │   └── access/
│   │   │       │       └── policies.toml  # Can restrict, not expand auth-service
│   │   │       └── src/
│   │   └── src/
│   │
│   └── api-gateway/             ← Tree (.wt-tree/)
│       ├── .wt-tree/
│       │   └── config.toml
│       └── src/
│
└── libs/
    └── shared-models/           ← Tree (.wt-tree/)
        ├── .wt-tree/
        │   └── config.toml
        └── src/
```

#### What Each Level Can Configure

| Setting | System | User Global | Root `.wt/` | Tree `.wt-tree/` | Subtree `.wt-tree/` |
|---|---|---|---|---|---|
| Server connection | ✓ (allowlist) | ✓ (default) | ✓ (authoritative) | ✗ | ✗ |
| Tenant identity | ✗ | ✓ (default) | ✓ (authoritative) | ✗ | ✗ |
| User name/email | ✗ | ✓ (authoritative) | ✗ | ✗ | ✗ |
| Sync auto/interval | ✗ | ✓ (default) | ✓ (authoritative) | ✓ (tree override) | ✓ (subtree override) |
| Auto-snapshot rules | ✗ | ✓ (default) | ✓ (root default) | ✓ (tree override) | ✓ (subtree override) |
| Ignore patterns | ✗ | ✓ (global ignores) | ✓ (root, ADDITIVE) | ✓ (adds, can't negate root) | ✓ (adds, can't negate parent) |
| Access roles | ✗ | ✗ | ✓ (ceiling) | ✓ (restrict only) | ✓ (restrict only) |
| Access policies | ✗ | ✗ | ✓ (ceiling) | ✓ (restrict only) | ✓ (restrict only) |
| Registered paths | ✗ | ✗ | ✓ (root paths) | ✓ (tree paths) | ✓ (subtree paths) |
| License defaults | ✗ | ✗ | ✓ (root default) | ✓ (tree override) | ✓ (subtree override) |
| Large file threshold | ✗ | ✓ (default) | ✓ (root default) | ✓ (tree override) | ✓ (subtree override) |
| Branch protection | ✗ | ✗ | ✓ (root branches) | ✓ (tree branches) | ✓ (subtree branches) |
| Editor / diff tool | ✗ | ✓ (authoritative) | ✗ | ✗ | ✗ |
| Reflog retention | ✗ | ✗ | ✓ (root default) | ✓ (tree override) | ✓ (subtree override) |

#### User Global Config Example

`~/.config/w0rktree/config.toml`:

```toml
[user]
name = "Alice Developer"
email = "alice@company.com"

[defaults]
server = "https://wt.company.com"
sync_auto = true
sync_interval_secs = 30

[editor]
diff_tool = "delta"
merge_tool = "meld"

[ignore]
# Global ignore patterns (added to every worktree)
patterns = [
    ".DS_Store",
    "Thumbs.db",
    "*.swp",
    "*.swo",
    "*~",
]
```

#### Environment Variable Reference

All environment variables use the `WT_` prefix:

| Variable | Overrides | Example |
|---|---|---|
| `WT_SERVER` | Server URL | `WT_SERVER=https://wt.company.com` |
| `WT_TENANT` | Tenant identity | `WT_TENANT=acme-corp` |
| `WT_SYNC_AUTO` | Auto-sync toggle | `WT_SYNC_AUTO=false` |
| `WT_SYNC_INTERVAL` | Sync interval (seconds) | `WT_SYNC_INTERVAL=60` |
| `WT_SNAPSHOT_AUTO` | Auto-snapshot toggle | `WT_SNAPSHOT_AUTO=false` |
| `WT_LOG_LEVEL` | BGProcess log level | `WT_LOG_LEVEL=debug` |
| `WT_CONFIG_DIR` | User config directory | `WT_CONFIG_DIR=/custom/path` |

Environment variables are most useful for CI/CD pipelines and scripting, where you don't want to modify config files.

### 3.18 Branch Protection Rules

Branch protection rules are **server-enforced rules** that prevent certain operations on protected branches. In Git, branch protection is a hosting platform feature (GitHub, GitLab) — not part of Git itself. In W0rkTree, branch protection is a **first-class, in-repo concept** defined in configuration files and enforced by the server.

#### Protection Configuration

Protection rules are defined in `.wt/config.toml` (root level) and `.wt-tree/config.toml` (tree level):

**Root `.wt/config.toml`:**

```toml
[[branch_protection]]
branch = "main"
rules = [
    "no_direct_push",        # Must merge from another branch, no direct push
    "require_merge_review",  # At least 1 approval before merge (see review settings)
    "no_delete",             # Cannot delete this branch
    "no_force",              # Cannot force-overwrite (already default in W0rkTree, but explicit)
    "require_ci_pass",       # Merge blocked until CI reports success
    "require_snapshot_signature", # Snapshots must be signed
]

[branch_protection.review]
min_approvals = 1
dismiss_stale_reviews = true            # Re-request review if source branch changes after approval
require_from_roles = ["Maintainer", "Admin", "Owner"]  # Only these roles' approvals count

[[branch_protection]]
branch = "release/*"                    # Prefix match for release branches
rules = [
    "no_direct_push",
    "require_merge_review",
    "no_delete",
]

[branch_protection.review]
min_approvals = 2                       # Stricter for release branches
```

**Tree-level `.wt-tree/config.toml`:**

```toml
# services/auth-service/.wt-tree/config.toml

[[branch_protection]]
branch = "main"
rules = [
    "no_direct_push",
    "require_merge_review",
    "require_security_review",          # Tree-specific: security team must approve
]

[branch_protection.review]
min_approvals = 2
require_from_roles = ["security-reviewer", "Admin"]
```

#### Available Protection Rules

| Rule | What it enforces |
|---|---|
| `no_direct_push` | Must merge from another branch. Direct `wt push` to this branch is rejected. |
| `require_merge_review` | At least N approvals on a merge request before the merge can proceed. |
| `no_delete` | Branch cannot be deleted (even soft-deleted). |
| `no_force` | Cannot force-overwrite the branch tip. (This is already the W0rkTree default — history is append-only — but this makes it explicit and prevents any future override mechanism.) |
| `require_ci_pass` | Merge is blocked until CI reports success via webhook integration. |
| `require_snapshot_signature` | All snapshots pushed to this branch must be cryptographically signed. |
| `require_security_review` | At least one reviewer must hold a security-related role. |

#### Server Enforcement

Branch protection rules are enforced by the **SERVER**, not the bgprocess. The bgprocess syncs the configuration, but the server is the authority. This ensures protection cannot be bypassed by a modified client.

When a user tries to `wt push` to a protected branch, the server evaluates:

1. **Direct push check**: Is this a direct push or a merge? If `no_direct_push` is set, direct pushes are rejected with: `"Error: Branch 'main' requires merge via merge request. Direct push is not allowed."`
2. **Review check**: Is there an active merge request with enough approvals from qualified roles? If `require_merge_review` is set, the server checks approval count and reviewer roles.
3. **CI check**: Has CI passed? If `require_ci_pass` is set, the server checks CI status via webhook integration. If CI hasn't reported yet: `"Error: Merge blocked — waiting for CI status on merge request #42."`
4. **Signature check**: Are all snapshots signed? If `require_snapshot_signature` is set, the server verifies cryptographic signatures on every snapshot in the push.

#### Merge Request System

W0rkTree has a **built-in merge request system** — no external platform (GitHub PRs, GitLab MRs) needed:

| Command | What it does |
|---|---|
| `wt merge request <source> --into <target>` | Create a merge request to merge `<source>` branch into `<target>`. |
| `wt merge request list` | List open merge requests. |
| `wt merge request show <id>` | Show merge request details (source, target, diff summary, approvals, CI status). |
| `wt merge request approve <id>` | Approve a merge request. Requires appropriate role per the branch's protection config. |
| `wt merge request reject <id> --reason <msg>` | Reject a merge request with feedback. |
| `wt merge request merge <id>` | Execute the merge (if all protection rules pass). |
| `wt merge request close <id>` | Close a merge request without merging. |
| `wt merge request update <id>` | Update a merge request (re-sync source branch changes). |

**Merge request metadata:**

```toml
[merge_request]
id = "mr_42"
source_branch = "feature/oauth"
target_branch = "main"
author = "alice@company.com"
created_at = "2025-01-15T10:00:00Z"
status = "open"                         # "open", "approved", "merged", "rejected", "closed"

[merge_request.diff_summary]
files_changed = 12
insertions = 342
deletions = 89

[[merge_request.approval]]
reviewer = "bob@company.com"
role = "Maintainer"
approved_at = "2025-01-15T11:30:00Z"
snapshot_at_approval = "snap_f1e2d3"    # Tracks which version was approved

[[merge_request.approval]]
reviewer = "carol@company.com"
role = "security-reviewer"
approved_at = "2025-01-15T12:00:00Z"
snapshot_at_approval = "snap_f1e2d3"

[merge_request.ci]
status = "passed"                       # "pending", "running", "passed", "failed"
url = "https://ci.company.com/run/12345"
updated_at = "2025-01-15T11:45:00Z"
```

#### Stale Review Handling

If `dismiss_stale_reviews = true` is set and the source branch receives new snapshots after approval, all existing approvals are dismissed:

1. The server detects that the source branch tip has changed since the approval
2. All approvals are marked as `stale` (not deleted — they're visible in the merge request history)
3. The merge request status reverts to `open` (from `approved`)
4. Reviewers are notified that re-review is needed
5. This prevents the "approve and then sneak in changes" attack

#### Protection Inheritance

Tree-level protections **ADD** to root-level protections. A tree cannot remove a root-level protection, only add stricter rules:

- If root `.wt/config.toml` requires 1 approval on `main`, a tree `.wt-tree/config.toml` can require 2 but cannot require 0
- If root doesn't protect a branch, a tree can add protection for that branch within its tree
- The server evaluates both root and tree protections and enforces the **strictest combination**

#### New Permissions

These permissions are added to the access system (see §3.5):

| Permission | Who needs it | What it allows |
|---|---|---|
| `branch:protect` | Admins, Owners | Configure branch protection rules |
| `merge_request:create` | Developers, Maintainers | Create merge requests |
| `merge_request:approve` | Maintainers, Admins (configurable per branch) | Approve merge requests |
| `merge_request:merge` | Maintainers, Admins | Execute merges on merge requests |

#### Visibility

`wt branch show <name>` displays active protection rules for the branch:

```
Branch: main

  Protection rules:
    ✓ no_direct_push          — Must merge via merge request
    ✓ require_merge_review    — Minimum 1 approval from: Maintainer, Admin, Owner
    ✓ no_delete               — Cannot be deleted
    ✓ no_force                — Cannot force-overwrite
    ✓ require_ci_pass         — CI must pass before merge

  Open merge requests targeting this branch: 2
    #42  feature/oauth   (alice@company.com)  — 1/1 approvals, CI: passed
    #45  fix/token-expiry (bob@company.com)   — 0/1 approvals, CI: running
```

---

## 4. Complete CLI Command Reference

Every command below goes through the W0rkTree protocol. The CLI (`wt`) talks to the local bgprocess, which talks to the server. Simple language, one job per command, no overloaded commands.

### Getting Started

| Command | What it does |
|---|---|
| `wt init [path]` | Create a new worktree. Sets up `.wt/` folder and connects to the server. If you give it a path, it creates the worktree there instead of the current folder. |
| `wt init --from <url>` | Create a new worktree from an existing one on the server. This is how you get a copy of someone else's worktree — there's no "clone" command. |

### Seeing What's Going On

| Command | What it does |
|---|---|
| `wt status` | Show what files changed, what branch you're on, and whether there are staged snapshots waiting to be pushed. |
| `wt status --team` | Same as `wt status` but also shows what your teammates are working on — their staged snapshots across the worktree. |
| `wt staged` | List all your staged snapshots that haven't been pushed to the branch yet. Shows per-user and per-branch. |
| `wt log` | Show the snapshot history for the current branch. |
| `wt log --limit <n>` | Show only the last N snapshots. |
| `wt log --branch <name>` | Show snapshot history for a specific branch. |
| `wt diff` | Show what changed since the last snapshot. |

### Snapshots (Saving Your Work)

The bgprocess creates snapshots automatically in the background. These commands let you do it manually or inspect them.

| Command | What it does |
|---|---|
| `wt snapshot` | Create a snapshot of the current state right now. The bgprocess does this automatically, but you can force one whenever you want. |
| `wt snapshot --message <msg>` | Create a snapshot with a description. |
| `wt snapshot restore <id>` | Go back to a previous snapshot. Your current work is saved as a snapshot first so nothing is lost. |

### Pushing (Making Your Work Official)

Staged snapshots are visible to the team but not part of the branch yet. Pushing makes them part of the branch history.

| Command | What it does |
|---|---|
| `wt push` | Push your staged snapshots to the current branch. This is when your work becomes part of the branch's official history. |
| `wt push --branch <name>` | Push to a specific branch instead of the current one. |

### Branches

| Command | What it does |
|---|---|
| `wt branch list` | List all branches in the current tree. |
| `wt branch create <name>` | Create a new branch. |
| `wt branch switch <name>` | Switch to a different branch. |
| `wt branch delete <name>` | Delete a branch. This is a soft delete — the snapshots still exist and can be recovered. |

### Merging

| Command | What it does |
|---|---|
| `wt merge <source>` | Merge the source branch into your current branch. |
| `wt merge <source> --into <target>` | Merge the source branch into a specific target branch. |

### Trees (Projects Inside Projects)

Each tree is an independent unit of code with its own history, branches, and access rules. Trees can live inside other trees.

| Command | What it does |
|---|---|
| `wt tree add <path>` | Create a new child tree at the given path. Sets up `.wt-tree/` inside it. |
| `wt tree add --from <url> <path>` | Add a child tree by pulling it from the server. |
| `wt tree list` | List all child trees in the current worktree. |
| `wt tree remove <path>` | Remove a child tree. The tree's history is preserved on the server. |
| `wt tree status` | Show the status of all child trees at once. |

### Access Control (Who Can Do What)

Access rules live in `.wt/access/` (root level) and `.wt-tree/access/` (per tree). These commands help you inspect and test them.

| Command | What it does |
|---|---|
| `wt access show` | Show what permissions you have on the current tree. |
| `wt access show --user <email>` | Show what permissions a specific user has. |
| `wt access show --path <path>` | Show who has access to a specific registered path. |
| `wt access roles` | List all roles — built-in ones (Owner, Admin, Developer, etc.) plus any custom roles from `roles.toml`. |
| `wt access policies` | List all active access policies, merged from `.wt/` and `.wt-tree/`. |
| `wt access paths` | List all registered paths from `config.toml` that have path-level access rules. |
| `wt access validate` | Check your `.wt/access/` and `.wt-tree/access/` files for errors — bad syntax, missing registered paths, unknown roles. |
| `wt access test <user> <action> <path>` | Test whether a specific user can do a specific action on a specific path. Returns allow or deny with the reason. |
| `wt access init` | Create default `.wt/access/` files (or `.wt-tree/access/` if you're inside a child tree) with starter roles and policies. |

### Tenants (Users and Orgs on the Server)

A tenant is a user or organization registered on the W0rkTree server. You grant other tenants access to your worktree by their username or email.

| Command | What it does |
|---|---|
| `wt tenant list` | List all tenants that have access to this worktree. |
| `wt tenant create <name>` | Create a new tenant on the server (admin only). |
| `wt tenant switch <name>` | Switch which tenant you're acting as (if you belong to multiple). |
| `wt tenant grant <tenant> <permissions>` | Give a tenant access to this worktree. Use their username or email. Example: `wt tenant grant alice-dev tree:read,tree:write` |
| `wt tenant revoke <tenant>` | Remove a tenant's access to this worktree. |

### License Compliance

Every path in a worktree can have a license. The server enforces these licenses — if someone tries to export or copy code they're not licensed for, the system blocks it.

| Command | What it does |
|---|---|
| `wt license show` | Show license assignments for the current tree. |
| `wt license show --path <path>` | Show the license for a specific registered path. |
| `wt license set <path> <spdx-id>` | Assign a license to a registered path. Uses SPDX identifiers (e.g. `MIT`, `Apache-2.0`, `AGPL-3.0-only`, `proprietary`). |
| `wt license grant <path> <tenant> <level>` | Give a specific tenant permission to use code at a path. Levels: `read-only`, `modify`, `redistribute`. |
| `wt license revoke <path> <tenant>` | Take back a license grant from a tenant. |
| `wt license audit` | Show a full report of all license assignments and grants across the worktree. |
| `wt license validate` | Check all license config for errors — bad SPDX identifiers, inconsistent grants, missing paths. |
| `wt license check <path>` | Show what you (the current user) are allowed to do with a specific path — combines IAM permissions and license restrictions. |

### Background Process (The Local Worker)

The bgprocess runs on your machine. It watches your files, creates snapshots automatically, and syncs staged snapshots to the server. You usually don't need to touch it — it just works.

| Command | What it does |
|---|---|
| `wt worker start` | Start the background process. This usually happens automatically when you `wt init`. |
| `wt worker stop` | Stop the background process. File watching and auto-sync pause until you start it again. |
| `wt worker status` | Show whether the bgprocess is running, its sync state, and connection to the server. |
| `wt worker logs` | View the bgprocess logs. Useful for debugging sync or connection issues. |

### Server Management

These commands manage your connection to the remote W0rkTree server — not the local bgprocess.

| Command | What it does |
|---|---|
| `wt server status` | Show the remote server's status — is it reachable, what tenant you're connected as, how many worktrees. |
| `wt server connect <url>` | Connect this worktree to a W0rkTree server. |
| `wt server disconnect` | Disconnect from the server. The bgprocess keeps working locally but stops syncing. |

### Git Compatibility

These commands let you work with existing Git repos and push/pull to Git remotes like GitHub. This is a migration bridge — W0rkTree is the replacement, but it speaks Git when it needs to.

| Command | What it does |
|---|---|
| `wt git import <source>` | Import a Git repository into W0rkTree. Converts all commits, branches, and tags. Works with local paths or URLs. |
| `wt git export <tree> --output <path>` | Export a W0rkTree tree as a standard Git repository. Proprietary-licensed paths are blocked from export. |
| `wt git export <tree> --output <path> --mode squashed` | Export with auto-snapshots collapsed into logical commits. Cleaner history for Git users. |
| `wt git remote add <name> <url>` | Register a Git remote (GitHub, GitLab, etc.) on a tree. |
| `wt git remote list` | List all registered Git remotes. |
| `wt git remote remove <name>` | Remove a Git remote. |
| `wt git push <remote> <branch>` | Convert snapshots to Git commits and push to a Git remote. |
| `wt git pull <remote> <branch>` | Fetch Git commits from a remote, convert to snapshots, and merge. |
| `wt git mirror <tree> --remote <name> --branch <branch>` | Keep a tree in continuous two-way sync with a Git remote. Teammates using Git and teammates using W0rkTree push to the same repo. |

### Sync (Manual Override)

The bgprocess handles sync automatically. These commands are for when you need to force it or check on it.

| Command | What it does |
|---|---|
| `wt sync status` | Show the current sync state — what's staged, what's pending, any sync errors. |
| `wt sync now` | Force an immediate sync of staged snapshots to the server. |
| `wt sync pause` | Temporarily pause auto-sync. Snapshots still happen locally, they just don't go to the server. |
| `wt sync resume` | Resume auto-sync after a pause. |

### How Commands Flow Through the System

Every command follows this path:

```
You type a command
       │
       ▼
   wt (CLI)          ← Parses your command, validates input
       │
       ▼
  bgprocess          ← The local worker. Does the actual work:
       │                file watching, snapshots, local storage
       │
       ▼
  W0rkTree Server    ← The remote server. Stores canonical history,
                        enforces access + license, aggregates
                        staged snapshots for team visibility
```

- **Local-only commands** (`wt status`, `wt diff`, `wt snapshot`) talk to the bgprocess and return immediately.
- **Server commands** (`wt push`, `wt tenant grant`, `wt license set`) go through the bgprocess to the server.
- **Sync commands** (`wt sync now`, `wt staged`) check the bgprocess's sync state with the server.
- **Access/license commands** (`wt access validate`, `wt license check`) evaluate rules locally but the server is the final authority.

---

## 5. Spec Files To Update

| File | Current State | Required Changes |
|---|---|---|
| `WORKTREE_PLAN.md` | Comprehensive but outdated terminology & architecture | Major update — see §6.1 |
| `crates/worktree-protocol/specs/README.md` | Good overview but missing bgprocess/server split | Update architecture section — see §6.2 |
| `crates/worktree-protocol/specs/WorkTree.md` | Detailed dependency system spec | Update terminology, add IAM per-tenant details — see §6.3 |
| `crates/worktree-protocol/specs/tree/Tree.md` | Detailed tree/branch/commit spec | Update `.worktree` → `.wt`/`.wt-tree`, add staged snapshot visibility — see §6.4 |
| `docs/protocol-spec.md` | All TODO stubs | Write from scratch — see §6.5 |
| `docs/server-architecture.md` | All TODO stubs | Write from scratch — see §6.6 |
| `docs/git-compatibility.md` | All TODO stubs | Write from scratch — see §6.7 |
| `docs/cli-reference.md` | Complete but needs updates | Add bgprocess commands, staged visibility commands — see §6.8 |
| `docs/sdk-guide.md` | All TODO stubs | Write from scratch — see §6.9 |
| `docs/admin-panel.md` | Comprehensive API docs | Add tenant/IAM endpoints, staged snapshot visibility endpoints — see §6.10 |
| `docs/README.md` | Index page, generic content | Update to reflect corrected architecture — see §6.11 |

---

## 6. New Specs To Create

| New File | Purpose |
|---|---|
| `crates/worktree-protocol/specs/bgprocess/BgProcess.md` | Full specification for the local background process |
| `crates/worktree-protocol/specs/server/Server.md` | Full specification for the remote multi-tenant server |
| `crates/worktree-protocol/specs/iam/IAM.md` | Full IAM specification: tenant model (user/org with username + email), cross-tenant access grants, worktree visibility modes, teams, roles, policies, scopes. Must cover the registered-path scope, declarative `.wt/access/` and `.wt-tree/access/` file model, scope resolution order, and inheritance between root and tree configs. |
| `crates/worktree-protocol/specs/iam/DeclarativeAccess.md` | Spec for the Terraform-like declarative access model: how `config.toml` registers paths and simple tenant grants, how `roles.toml` and `policies.toml` reference them (including cross-tenant `{ tenant = "..." }` subjects), how `.wt/` and `.wt-tree/` interact, validation rules, sync behavior, and server enforcement. No globs — explicit registration only. |
| `crates/worktree-protocol/specs/iam/TenantModel.md` | Tenant identity model: username, email, org vs. personal, cross-tenant access resolution, how the server maps usernames/emails to TenantIds, worktree visibility modes (Private/Shared/Public). |
| `crates/worktree-protocol/specs/licensing/LicenseCompliance.md` | License compliance spec: per-path license assignment, SPDX identifiers, license grant model (read-only/modify/redistribute), enforcement on export/fork/sync/copy operations, interaction with IAM, proprietary code protection, public worktree license rules. |
| `crates/worktree-protocol/specs/sync/Sync.md` | Sync protocol between bgprocess and server |
| `crates/worktree-protocol/specs/visibility/StagedVisibility.md` | Staged snapshot visibility spec (staged ≠ pushed model) |
| `crates/worktree-protocol/specs/storage/Storage.md` | Storage architecture: local (bgprocess-managed) vs remote (server) |
| `crates/worktree-protocol/specs/security/Security.md` | Security model: encryption, auth, secret scanning, audit logging |
| `crates/worktree-protocol/specs/dot-wt/DotWt.md` | `.wt/` folder structure spec — root worktree configuration, access, tenant grants, and license config |
| `crates/worktree-protocol/specs/dot-wt-tree/DotWtTree.md` | `.wt-tree/` folder structure spec — individual tree configuration, access overrides, and tree-level license config |
| `docs/bgprocess.md` | User-facing docs for the background process |
| `docs/multi-tenancy.md` | User-facing docs for multi-tenant server setup, cross-tenant access, worktree visibility modes |
| `docs/staged-visibility.md` | User-facing docs for staged snapshot visibility |
| `docs/access-management.md` | User-facing guide for the declarative access model — how to register paths in `config.toml`, define roles and policies, use `.wt/` vs `.wt-tree/`, grant cross-tenant access by username/email, and test access |
| `docs/license-compliance.md` | User-facing guide for license compliance — how to assign licenses to paths, configure license grants, handle proprietary vs open-source code, and understand what the server enforces on export/fork/sync |

---

## 7. Detailed Change Plan Per File

### 7.1 `WORKTREE_PLAN.md`

**Priority: HIGH — This is the master plan document.**

Changes required:

1. **§1 Vision**: Replace "A Server — a persistent background process that watches your filesystem" with the two-runtime model: bgprocess (local) + server (remote)
2. **§2 Why Not Git?**: Expand table with the full Git problem catalog from §2 of this plan. Add protocol/security problems. Add UX/jargon problems.
3. **§3 Core Concepts**:
   - §3.1 Trees: Change `.worktree/` → `.wt/` (root) and `.wt-tree/` (individual trees). Clarify that these folders are for config/access only, not history storage.
   - §3.4 Tenants & Users: Major expansion. Define tenant as a user or org on the worktree-server with username + email. Add cross-tenant access model (simple grants in `config.toml` by username/email, full IAM in `policies.toml`). Add worktree visibility modes (Private/Shared/Public). Add IAM detail: access management at root worktree (`.wt/`), individual tree (`.wt-tree/`), individual registered path, and branch level.
   - Add new §3.X: **License Compliance** — per-path license assignment, license grants for cross-tenant code sharing, server-side enforcement on export/fork/sync, proprietary code protection. License sits above IAM in the enforcement stack.
   - Add new §3.6: **Staged Snapshot Visibility** — describe how staged snapshots sync to the root W0rkTree for team visibility (staged ≠ pushed, visible ≠ merged)
   - Add new §3.7: **The Background Process** — describe the bgprocess role as a full local VCS that syncs staged snapshots to the server
4. **§4 Architecture Overview**: Replace the ASCII diagram with the corrected two-runtime diagram from §3.1 of this plan. The current diagram shows one monolithic "SERVER" — split into bgprocess + server.
5. **§5 The Protocol**: Add sync protocol responsibilities. Add staged snapshot visibility wire format (staged sync messages distinct from branch push messages). Add IAM-aware message framing.
6. **§6 The Server**: Rename to "The Server (Remote)" and add the `worktree-bgprocess` as a separate section. Update the crate layout to reflect the split.
7. **§7 The Client & SDK**: Update CLI reference to include bgprocess management commands (`wt worker start/stop/status`). Add `wt status --team` for staged snapshot visibility. Add `wt staged` to view unpushed staged snapshots. Add `wt push` as the explicit action to move staged work into branch history.
8. **§8 Nested Tree Model**: Change all `.worktree/` references to `.wt/` (root) and `.wt-tree/` (child trees). Clarify that history is NOT stored in these folders. Update the ASCII diagram to show `.wt/` at root and `.wt-tree/` in each child tree.
9. **§9 Automatic Control Architecture**: Attribute auto-snapshot and auto-branch to the bgprocess, not "the server". The server receives staged snapshots for visibility; branch history only changes on explicit push. Clarify the two-step flow: bgprocess auto-snapshots → staged sync to server (visible, not merged) → explicit `wt push` (merged into branch).
10. **§10 Access & Control Architecture** — major rewrite:
     - Expand to describe per-tenant IAM with full RBAC + ABAC model
     - Define tenant as user/org with username + email. Add cross-tenant access: simple grants in `config.toml`, full IAM in `policies.toml` with `{ tenant = "..." }` subject type
     - Add worktree visibility modes: Private (default), Shared (explicit grants), Public (all tenants read, license governs copying)
     - Add `RegisteredPath` scope level to the permission hierarchy (currently stops at Branch — must go Global → Tenant → Tree → Branch → RegisteredPath)
     - Add file-level and path-level permissions using **explicit registration** in `config.toml` — no globs, no patterns
     - Add examples of path-scoped policies referencing registered paths from `config.toml`, including cross-tenant policies
     - Add the two-folder access model: `.wt/access/` (root) and `.wt-tree/access/` (per-tree)
     - Document `config.toml` path registration as the prerequisite for path-scoped policies
     - Document how access files are version-controlled, synced by bgprocess, and enforced by server
     - Document scope resolution order: most specific wins, deny beats allow at same level, `.wt-tree/` overrides `.wt/`
     - Document inheritance: root `.wt/` cascades to all trees, `.wt-tree/` overrides for its own tree
     - Add examples showing the same permission at different scopes
     - Reference the implemented code: `iam::engine`, `iam::policy`, `iam::scope`, `iam::role`, `access::resource`
     - Note gap: `Scope` enum needs `RegisteredPath` variant with exact-match semantics (not prefix or glob), `Resource::Subtree` needs proper scope support, `PolicySubject` needs `Tenant` variant
11. **Add new §11: License Compliance Architecture**:
     - Per-path license assignment in `config.toml` using SPDX identifiers
     - License grant model for cross-tenant proprietary code sharing (read-only / modify / redistribute)
     - Server-side enforcement on export, fork, sync, and copy operations
     - License sits above IAM: IAM YES + License YES = ALLOWED
     - Public worktree license rules: all tenants can read, license governs what they can take
     - Integration with Git export: license headers, LICENSE file auto-generation, proprietary path blocking
11. **§12 Platform Support**: Update to describe bgprocess installation (the local daemon), distinct from server installation.
12. **§13 Project Structure**: Add `worktree-bgprocess` (or `worktree-worker`) crate to the workspace layout. Update dependency graph.
13. **§14 Milestones & Roadmap**: Insert new phases for bgprocess development, staged snapshot visibility, multi-tenancy server features, and the staged ↔ pushed distinction in the sync protocol.
14. **§16 Open Questions**: Add questions about bgprocess ↔ server protocol versioning, staged snapshot privacy/opt-out controls, staged snapshot retention policy, and tenant isolation guarantees.
15. **Global**: Replace all instances of `.worktree/` with `.wt/`. Standardize naming to "W0rkTree".

### 7.2 `crates/worktree-protocol/specs/README.md`

Changes required:

1. Update "What is a WorkTree?" to include the two-runtime model
2. Add the bgprocess concept to Key Innovations section
3. Add Staged Snapshot Visibility to Key Innovations
4. Update the architecture example to show bgprocess + server
5. Add links to new spec files (BgProcess.md, Server.md, IAM.md, etc.)
6. Update "Why WorkTree Over Git?" table with the full Git problem catalog
7. Standardize naming to "W0rkTree"

### 7.3 `crates/worktree-protocol/specs/WorkTree.md`

Changes required:

1. §Core Concepts: Add multi-tenancy details — a tenant is a user or org on the server with username + email. Tenants own worktrees and can grant cross-tenant access.
2. §Architecture: Update WorkTree Structure to show `.wt/` at root and `.wt-tree/` in each child tree (replacing implied `.worktree/`)
3. §Dependency System: Add a note that dependency resolution is handled by the server, while the bgprocess surfaces dependency status locally
4. §Linked Branches: Clarify that linked branch synchronization is enforced by the server, not locally
5. §Automatic TODO Generation: Clarify that TODOs are stored on the server and synced to local via bgprocess
6. Add new section: **Tenant Model** — define tenant (user/org), cross-tenant access grants (by username/email or full IAM), worktree visibility modes (Private/Shared/Public)
7. Add new section: **Server-Side Enforcement** — all access control and license compliance enforced on the server; bgprocess does client-side fast-fail only
8. Add new section: **Declarative Access Model** — describe how `.wt/access/` and `.wt-tree/access/` work together, explicit path registration, scope hierarchy
9. Add new section: **License Compliance** — per-path license assignment, license grants, server enforcement on export/fork/sync
10. Standardize naming

### 7.4 `crates/worktree-protocol/specs/tree/Tree.md`

Changes required:

1. §Tree Structure → Directory Layout: Replace `.worktree/` with `.wt-tree/` for individual trees. Document that `.wt-tree/` is for tree-specific config and access, distinct from the root `.wt/`. Remove any implication that history is stored in `.wt-tree/`.
2. §Branches: Add note that branch state is synced between bgprocess and server
3. §Commits: Clarify that snapshots are created locally by bgprocess and synced to server. The server is the source of truth.
4. §Dependencies: Add path-level and file-level dependency examples
5. §Cross-Tree Relationships: Add staged snapshot visibility — when a user has staged snapshots touching a tree, other users see what's in flight
6. Add new section: **Staged Snapshot Visibility** — per-tree visibility of staged (unpushed) work, who has staged what and on which branch
7. Add new section: **Tree Access Control** — how `.wt-tree/access/policies.toml` works, how registered paths in `.wt-tree/config.toml` enable path-scoped policies, how tree-level access overrides root-level access, how cross-tenant policies interact with tree-level overrides
8. Add new section: **Tree License Configuration** — how `.wt-tree/config.toml` can set tree-level licenses, license grants at the tree level, how tree licenses override or inherit from root `.wt/config.toml`
8. Standardize naming

### 7.5 `docs/protocol-spec.md` — WRITE FROM SCRATCH

This is currently all TODOs. Write the full protocol specification:

1. **Object Model**: Define Blob, Tree, Snapshot, Branch, Manifest, Delta, Tenant, Account, Team, Role, Policy. Reference the implemented types in `worktree-protocol` crate.
2. **Tree Structure**: Define nested tree model, `.wt/` folder, tree mounting, path resolution.
3. **Wire Format**: Document the implemented wire format from `feature::wire::format` (magic bytes, versioning, flags, header/payload structure).
4. **Sync Protocol**: Define bgprocess ↔ server sync messages. Staged snapshot upload, branch push, branch pull, IAM token exchange. Clearly distinguish staged snapshot sync (automatic, for visibility) from branch push (explicit, merges into history). Include access config sync (`.wt/access/` and `.wt-tree/access/` file changes).
5. **Diff Semantics**: Document the implemented diff from `feature::diff::compute` (rename/copy detection, DiffOptions).
6. **Merge Semantics**: Define three-way merge, conflict model, resolution strategies. No rebase — append-only history.
7. **Snapshot Format**: Define the snapshot DAG structure, content-addressable Merkle tree with BLAKE3.
8. **Staged Snapshot Visibility Protocol**: Define messages for syncing staged snapshots to the server root W0rkTree. Define how the server stores/indexes staged snapshots per user/branch. Define how clients query staged snapshot state. Clearly specify: staged ≠ pushed — staged snapshots are visible but not part of branch history until explicitly pushed.
9. **Access Control Protocol**: Define how `.wt/access/` and `.wt-tree/access/` files are validated by the bgprocess, synced to the server, and enforced. Define path registration in `config.toml` as prerequisite for path-scoped policies. Define cross-tenant policy subjects (`{ tenant = "username" }` and `{ tenant = "email" }`). Define the scope resolution algorithm (registered path > branch > tree > tenant > global, `.wt-tree/` overrides `.wt/`). Define how the server rejects access config changes from users without `PolicyManage`/`TreeAdmin`. Define the `RegisteredPath` scope variant with exact-match semantics (no globs).
10. **License Compliance Protocol**: Define how per-path license assignments in `config.toml` are synced and enforced. Define license grant messages (tenant requests access, owner grants read-only/modify/redistribute). Define enforcement on export, fork, sync, and copy operations. Define how the license layer interacts with IAM (both must pass). Define SPDX identifier validation. Define how public worktrees serve content with license metadata.

### 7.6 `docs/server-architecture.md` — WRITE FROM SCRATCH

This is currently all TODOs. Write the full server architecture:

1. **Multi-Tenancy**: How tenants are isolated. Each tenant has its own worktrees, IAM, and storage namespace.
2. **IAM Engine**: Document the implemented `iam::engine` (AccessEngine, RBAC + ABAC, AccessDecision, AccessRequest). Reference the 871-line implementation. Document the scope resolution algorithm including the new `RegisteredPath` scope level. Document cross-tenant access evaluation: how the server resolves tenant usernames/emails to TenantIds, how `{ tenant = "..." }` subjects are matched, how simple `tenant_access` grants in `config.toml` are resolved into IAM policies.
3. **Declarative Access Enforcement**: How the server receives `.wt/access/` and `.wt-tree/access/` config files from bgprocess sync, validates them (including verifying that path-scoped policies reference paths registered in the corresponding `config.toml`, and that tenant subjects resolve to real tenants on the server), stores them, and applies them as live policies. How access config changes are themselves access-controlled (only users with `PolicyManage`/`TreeAdmin` can modify them). How `.wt-tree/` overrides `.wt/` for its own tree.
4. **License Compliance Engine**: How the server stores per-path license assignments, evaluates license grants on cross-tenant operations, blocks unauthorized export/fork/sync of proprietary code, and integrates license checks with IAM (both must pass). How license config in `.wt/config.toml` and `.wt-tree/config.toml` is synced and enforced.
4. **Daemon Lifecycle**: Server start/stop, health checks, graceful shutdown.
5. **Worktree Visibility & Cross-Tenant Access**: How worktree visibility modes (Private/Shared/Public) are configured and enforced. How the server resolves cross-tenant access from simple `tenant_access` grants and full IAM policies. How public worktrees serve content to all tenants while respecting license restrictions.
6. **Storage Backend**: Content-addressable store, object deduplication, per-tenant storage isolation.
7. **Sync Engine**: How the server handles incoming staged snapshots from bgprocess clients vs. explicit branch pushes. Staged snapshots are stored and indexed for visibility but do NOT modify branch history. Branch pushes go through conflict detection and merge into the branch's snapshot DAG.
8. **Staged Snapshot Aggregation**: How the server stores staged snapshots from all connected bgprocess clients, indexes them by user/branch/tree, and serves them via API so the team can see what's in flight. This is the "root W0rkTree view" — a live picture of all staged work across the organization.
9. **API Surface**: gRPC API for bgprocess sync, REST API for admin panel, SDK API.
10. **Access Control Enforcement**: Server-side permission checks at tree, branch, and registered-path level. How registered paths from `config.toml` are looked up (exact match, not pattern matching). How deny-overrides-allow resolution works. How `.wt-tree/` policies override `.wt/` policies. How the server fast-rejects operations before touching storage.
11. **License Compliance Enforcement**: Server-side license checks on export, fork, sync, and copy operations. How per-path licenses and license grants are evaluated. How the license layer interacts with IAM (both must pass). How proprietary code is blocked from unauthorized tenant access. How public worktree license metadata is served.

### 7.7 `docs/git-compatibility.md` — WRITE FROM SCRATCH

This is currently all TODOs. Write based on the comprehensive Git compatibility section already in `WORKTREE_PLAN.md` §11, but updated:

1. **Importing from Git**: Document the conversion process (from `WORKTREE_PLAN.md` §11.2) — this is a migration bridge, not a core feature.
2. **Exporting to Git**: Document export modes (full, squashed, shallow, single-tree).
3. **Git Remote Bridge**: Push/pull to GitHub/GitLab/Bitbucket.
4. **Live Mirror Mode**: Bidirectional sync for gradual team adoption.
5. **Object Mapping**: SHA-1 ↔ BLAKE3 hash index.
6. **Round-Trip Guarantee**: Import → export produces semantically identical repo.
7. **Framing**: Git compatibility exists to enable incremental adoption. W0rkTree is the replacement, not an extension.

### 7.8 `docs/cli-reference.md`

The existing CLI reference must be rewritten to match the full command list in §4 of this plan. All commands from §4 are the source of truth.

Additional changes:

Changes required:

1. Add `wt worker` command group:
   ```
   wt worker start        Start the background process
   wt worker stop         Stop the background process
   wt worker status       Show bgprocess status and sync state
   wt worker logs         View bgprocess logs
   ```
2. Update `wt server` to clarify this manages the REMOTE server, not the local daemon
3. Add `wt status --team` flag to show staged snapshots from all team members on the current tree
4. Add `wt staged` command — show all staged (unpushed) snapshots on the current tree, per user and branch
5. Add `wt push` command — explicitly push staged snapshots to the branch (this is when staged work becomes branch history)
6. Add `wt tenant` command group for multi-tenant management:
   ```
   wt tenant list             List tenants (admin only)
   wt tenant create <name>    Create a new tenant
   wt tenant switch <name>    Switch active tenant context
   wt tenant grant <tenant> <permissions>   Quick-grant access to a tenant by username/email
   wt tenant revoke <tenant>  Revoke a tenant's access
   ```
7. Add `wt license` command group for license compliance:
   ```
   wt license show                     Show license assignments for current tree
   wt license show --path <path>       Show license for a specific registered path
   wt license set <path> <spdx-id>     Assign a license to a registered path
   wt license grant <path> <tenant> <level>  Grant a license to a tenant (read-only/modify/redistribute)
   wt license revoke <path> <tenant>   Revoke a license grant
   wt license audit                    Audit all license assignments and grants
   wt license validate                 Validate all license config (SPDX identifiers, grant consistency)
   wt license check <path>             Check what the current user can do with a path (IAM + license)
   ```
8. Expand `wt access` as the primary access management command group (replaces `wt permission`):
   ```
   wt access show                  Show effective permissions for current user on current tree
   wt access show --user <email>   Show effective permissions for a specific user
   wt access show --path <path>    Show who has access to a registered path
   wt access roles                 List all roles (built-in + custom from .wt/access/roles.toml)
   wt access policies              List all active policies (merged from .wt/ and .wt-tree/)
   wt access paths                 List all registered paths from config.toml
   wt access validate              Validate .wt/access/ and .wt-tree/access/ files (syntax + registered path verification)
   wt access test <user> <action> <path>   Test whether a user can perform an action on a path
   ```
9. Add `wt access init` command — scaffold `.wt/access/` (or `.wt-tree/access/` if run inside a child tree) with default role and policy files
10. Update all references from `.worktree/` to `.wt/` (root) and `.wt-tree/` (trees)

### 7.9 `docs/sdk-guide.md` — WRITE FROM SCRATCH

This is currently all TODOs. Write based on the SDK crate structure:

1. **Installation**: Add as Cargo dependency, feature flags.
2. **Connecting**: `Client::connect_local()` connects to the local bgprocess. `Client::connect("server-url")` connects to a remote server.
3. **Tree Operations**: CRUD on trees, nested tree management.
4. **Snapshot Operations**: Create, list, inspect, compare, restore.
5. **Branch Operations**: Create, list, switch, delete, merge.
6. **Permission Operations**: Set/get/list permissions via IAM. Grant/revoke cross-tenant access by username/email.
7. **Staged Snapshot Visibility**: Query staged snapshots per tree/branch/user. Subscribe to staged snapshot events (new staged work arriving on the server).
8. **Sync Operations**: Manual sync trigger, sync status.
9. **License Operations**: Query license for a path, check effective permissions (IAM + license), list license grants, request license grants from worktree owner.

### 7.10 `docs/admin-panel.md`

Changes required:

1. Add tenant management API endpoints:
   ```
   GET    /api/tenants              List all tenants
   POST   /api/tenants              Create a tenant
   GET    /api/tenants/:id          Get tenant details
   PUT    /api/tenants/:id          Update tenant
   DELETE /api/tenants/:id          Delete tenant
   ```
2. Add IAM management API endpoints:
   ```
   GET    /api/tenants/:id/users        List users in tenant
   POST   /api/tenants/:id/users        Add user to tenant
   GET    /api/tenants/:id/teams        List teams in tenant
   POST   /api/tenants/:id/teams        Create team
   GET    /api/tenants/:id/roles        List roles (built-in + custom from .wt/access/ and .wt-tree/access/)
   POST   /api/tenants/:id/policies     Create access policy
   ```
3. Add staged snapshot visibility API endpoints:
   ```
   GET    /api/tenants/:id/staged            All staged snapshots across tenant
   GET    /api/repositories/:id/staged       Staged snapshots in a worktree (by user, by branch)
   GET    /api/repositories/:id/staged/:uid  Staged snapshots by a specific user
   WS     /api/repositories/:id/staged/live  WebSocket stream of staged snapshot updates
   ```
4. Add declarative access control API endpoints:
   ```
   GET    /api/repositories/:id/access                 List all active policies (merged from .wt/ and .wt-tree/ files)
   GET    /api/repositories/:id/access/roles            List roles (built-in + custom)
   GET    /api/repositories/:id/access/policies          List policies from root .wt/access/ and all .wt-tree/access/
   GET    /api/repositories/:id/access/registered-paths  List all registered paths from config.toml files
   GET    /api/repositories/:id/access/effective          Effective permissions matrix (who can do what where)
   GET    /api/repositories/:id/access/test               Test access: given user + action + path, returns allow/deny (IAM + license combined)
   POST   /api/repositories/:id/access/validate           Validate a proposed access config change before sync
   GET    /api/repositories/:id/trees/:tid/access         List policies for a specific tree (from its .wt-tree/access/)
   GET    /api/repositories/:id/access/tenants            List all tenants with access to this worktree (from config.toml grants + IAM policies)
   ```
5. Add license compliance API endpoints:
   ```
   GET    /api/repositories/:id/licenses                  List all license assignments (per-path)
   GET    /api/repositories/:id/licenses/:path            Get license for a specific path
   GET    /api/repositories/:id/licenses/grants           List all license grants (per-tenant per-path)
   POST   /api/repositories/:id/licenses/grants           Create a license grant
   DELETE /api/repositories/:id/licenses/grants/:gid      Revoke a license grant
   GET    /api/repositories/:id/licenses/check             Check effective access for a tenant on a path (IAM + license)
   GET    /api/repositories/:id/licenses/audit             License compliance audit log
   POST   /api/repositories/:id/licenses/validate          Validate license config (SPDX identifiers, grant consistency)
   ```
6. Update configuration docs to include multi-tenancy config options and worktree visibility modes
7. Add WebSocket endpoint documentation for real-time staged snapshot streaming
8. Add access audit log endpoint:
   ```
   GET    /api/repositories/:id/access/audit          Access decision audit log (who was allowed/denied what, when)
   ```

### 7.11 `docs/README.md`

Changes required:

1. Update FAQ: "What is Worktree?" → Describe as a Git replacement, not a Git companion
2. Add bgprocess documentation link
3. Add multi-tenancy documentation link
4. Add staged snapshot visibility documentation link
5. Update architecture description to reflect two-runtime model
6. Standardize naming

---

## 8. Naming & Terminology Standardization

Apply these changes **globally** across all files:

| Current (Inconsistent) | Correct | Notes |
|---|---|---|
| Worktree, WorkTree, worktree (product name) | **W0rkTree** | Product name uses zero. Crate/binary names stay lowercase. |
| `.worktree/` (in root) | **`.wt/`** | Root worktree config folder. Shorter, cleaner, doesn't conflict with `git worktree`. |
| `.worktree/` (in child trees) | **`.wt-tree/`** | Individual tree config folder. Each child tree gets its own `.wt-tree/` for tree-specific config and access. |
| worktree-server (as local daemon) | **worktree-bgprocess** or **worktree-worker** | The local background process |
| worktree-server (as remote) | **worktree-server** | The remote multi-tenant server |
| "server" (ambiguous) | **bgprocess** (local) or **server** (remote) | Always qualify which one |
| commit | **snapshot** | W0rkTree uses snapshots, not commits. Reserve "commit" for Git compat docs only. |
| staging area / index | **(removed)** | W0rkTree has no staging area. Never reference it except when explaining Git problems. |
| push / pull (Git sense) | **staged sync** (automatic) + **push** (explicit to branch) | bgprocess syncs staged snapshots automatically. `wt push` explicitly pushes staged work to the branch. No `git pull` equivalent — remote branch updates sync automatically. |
| `.worktreeignore` | **`.wt/ignore`** | Consistent with `.wt/` folder convention |
| repository / repo | **worktree** or **tree** | W0rkTree doesn't have "repositories" — it has trees |
| clone | **`wt init --from`** or **`wt tree add --from`** | No "clone" — you initialize a tree from a source |
| `wt permission` | **`wt access`** | Aligns with `.wt/access/` folder. "Access" is the user-facing term; "permission" is the internal atomic unit. |
| permission config via API only | **`.wt/access/*.toml`** + **`.wt-tree/access/*.toml`** (declarative, Terraform-style) | Access is defined as files in the worktree, version-controlled and synced. API is for querying/testing, not primary config. |
| scope stops at Branch | **scope goes to RegisteredPath** | `Global → Tenant → Tree → Branch → RegisteredPath`. Explicitly registered paths are the finest granularity. No globs. |
| glob/pattern-based path matching | **explicit path registration in `config.toml`** | No `**`, `*`, or `?`. You register the exact paths you want to control in `config.toml`, then reference them in `policies.toml`. |
| no license enforcement | **file-level license compliance** | Every path can have a license (SPDX). The server enforces license restrictions on export/fork/sync/copy. License grants control what cross-tenant operations are permitted. |
| tenant = abstract concept | **tenant = user or org with username + email** | Tenants are the identity unit on the server. You grant cross-tenant access by referencing tenant username or email in config. |

---

## 9. Implementation Alignment

The spec updates must align with what's actually implemented in the crates. Current implementation state:

| Crate | State | Spec Alignment Needed |
|---|---|---|
| `worktree-protocol` | **80-90%** — Core types, IAM engine, wire format, diff all implemented | Specs should document what's implemented, not aspirational features. Mark unimplemented sections as "PLANNED". |
| `worktree-server` | **40-50%** — Watcher + debouncer work. Handlers/daemon are `todo!()` | Current server code is actually the bgprocess. Specs must clarify this and plan the actual remote server. |
| `worktree-git` | **25-35%** — Hash index + transport types work. Converters are `todo!()` | Spec should mark Git compat as Phase 2. Don't block core specs on Git compat. |
| `worktree-cli` | **30-35%** — Full CLI grammar. All commands are stubs. | CLI reference should mark implemented vs. planned commands. |
| `worktree-sdk` | **20-25%** — API surface designed. No real implementation. | SDK guide should be aspirational but clearly marked as pre-release. |
| `worktree-admin` | **40-50%** — Types, utils, components. No pages. | Admin panel docs should mark available vs. planned endpoints. |

### New Crate Needed: `worktree-bgprocess`

The current `worktree-server` crate contains code that is actually the bgprocess (filesystem watcher, debouncer, auto-commit engine). One of two approaches:

**Option A — Rename**: Rename `worktree-server` to `worktree-bgprocess` and create a new `worktree-server` for the remote server.

**Option B — Split**: Keep `worktree-server` as the remote server and extract bgprocess code into a new `worktree-bgprocess` crate.

**Recommendation**: Option B. The existing `worktree-server` crate has the right module structure for a local daemon. Create a new `worktree-remote` or keep `worktree-server` as the remote and rename current to `worktree-worker`.

This decision must be finalized before spec updates proceed.

### Protocol Crate Updates Needed for Permissions

The `worktree-protocol` crate needs these implementation changes to support the spec:

1. **Add `Scope::RegisteredPath` variant**: `Scope::RegisteredPath(TenantId, TreeId, String)` — where the `String` is the exact registered path from `config.toml`. The `covers()` semantics are exact-match (a registered path scope covers only operations on that exact path or children under it), NOT glob/pattern matching.
2. **Add `RegisteredPath` type**: A validated path string that must exist in the corresponding `config.toml`. No wildcards, no globs — just a path like `"src/crypto"` or `"config/production.toml"`.
3. **Update `Resource::Subtree::to_scope()`**: Return `Scope::RegisteredPath` instead of falling back to `Scope::Tree`. The `path_prefix` field becomes a `RegisteredPath`.
4. **Enrich `Tenant` struct**: Add `email: String` field. Add `PolicySubject::Tenant(TenantId)` variant for cross-tenant policies. Add `WorktreeVisibility` enum (Private/Shared/Public) for worktree-level config.
5. **Add declarative config parsing**: New module to parse `.wt/config.toml` (registered paths, tenant grants, license config, visibility mode), `.wt/access/roles.toml`, `.wt/access/policies.toml`, and the equivalent `.wt-tree/` files into existing `Role`, `Policy`, and `TreeAccessRule` structs. Must validate that path-scoped policies reference paths that are actually registered. Must resolve `{ tenant = "..." }` subjects to `TenantId`s.
6. **Add scope resolution priority**: Formalize the evaluation order in `AccessEngine` — registered path > branch > tree > tenant > global, deny-wins-at-same-level, `.wt-tree/` overrides `.wt/` for that tree.
7. **Add `RegisteredPathPolicy`**: New struct tying a registered path to subjects and permissions, parsed from `policies.toml` when `scope = { path = "..." }` is used.
8. **Add license compliance module**: New `licensing/` module in the protocol crate. `License` struct (SPDX identifier, per-path assignment). `LicenseGrant` struct (tenant, path, grant level: ReadOnly/Modify/Redistribute). `LicenseCheck` function that evaluates whether a tenant can perform an operation on a path given the path's license and any grants. Integration point with `AccessEngine` — the engine must call both IAM check and license check, both must pass.

---

## 10. Execution Order & Dependencies

### Phase 1 — Foundation (Do First)

1. **Finalize naming decision**: bgprocess crate name, `.wt/` folder name, product name style
2. **Update `WORKTREE_PLAN.md`** — this is the master document everything else references
3. **Create `crates/worktree-protocol/specs/bgprocess/BgProcess.md`** — defines the local daemon
4. **Create `crates/worktree-protocol/specs/server/Server.md`** — defines the remote server
5. **Create `crates/worktree-protocol/specs/dot-wt/DotWt.md`** — defines `.wt/` folder (root worktree config, access, identity)
6. **Create `crates/worktree-protocol/specs/dot-wt-tree/DotWtTree.md`** — defines `.wt-tree/` folder (individual tree config, access overrides)

### Phase 2 — Core Specs (Depends on Phase 1)

7. **Write `docs/protocol-spec.md`** from scratch — references protocol crate implementation, includes access control protocol
8. **Write `docs/server-architecture.md`** from scratch — covers both bgprocess and server, includes declarative access enforcement
9. **Create `crates/worktree-protocol/specs/iam/IAM.md`** — full IAM spec with tenant model (user/org, username + email), cross-tenant access, worktree visibility modes, registered-path scopes, scope resolution order, `.wt/` vs `.wt-tree/` inheritance model
10. **Create `crates/worktree-protocol/specs/iam/DeclarativeAccess.md`** — the Terraform-like declarative access model: explicit path registration in `config.toml`, simple tenant grants in `config.toml`, `roles.toml` and `policies.toml` formats (including `{ tenant = "..." }` subjects), `.wt/` vs `.wt-tree/` interaction, validation, sync, enforcement. No globs.
11. **Create `crates/worktree-protocol/specs/iam/TenantModel.md`** — tenant identity (user/org, username, email), cross-tenant access resolution, worktree visibility modes (Private/Shared/Public), simple `tenant_access` grants vs full IAM policies
12. **Create `crates/worktree-protocol/specs/licensing/LicenseCompliance.md`** — per-path license assignment (SPDX), license grant model (read-only/modify/redistribute), enforcement on export/fork/sync/copy, interaction with IAM (both must pass), proprietary code protection, public worktree license rules, Git export license handling
13. **Create `crates/worktree-protocol/specs/sync/Sync.md`** — sync protocol, including access config sync and license config sync
14. **Create `crates/worktree-protocol/specs/visibility/StagedVisibility.md`** — staged snapshot visibility (staged ≠ pushed model)

### Phase 3 — User-Facing Docs (Depends on Phase 2)

15. **Update `docs/cli-reference.md`** — add bgprocess commands, staged snapshot visibility, `wt access` command group, `wt tenant` commands (including grant/revoke by username/email), `wt license` command group
16. **Write `docs/sdk-guide.md`** from scratch — include cross-tenant access and license operations
17. **Write `docs/git-compatibility.md`** from scratch — include license handling on Git export (proprietary path blocking, LICENSE file generation, license headers)
18. **Update `docs/admin-panel.md`** — add tenant/IAM/visibility/access/license endpoints
19. **Create `docs/bgprocess.md`** — user guide for the background process
20. **Create `docs/multi-tenancy.md`** — user guide for multi-tenant server setup, cross-tenant access by username/email, worktree visibility modes (Private/Shared/Public)
21. **Create `docs/staged-visibility.md`** — user guide for staged snapshot visibility
22. **Create `docs/access-management.md`** — user guide for declarative access: how to register paths in `config.toml`, define roles and policies, grant cross-tenant access by username/email, use `.wt/` vs `.wt-tree/`, test access, validate config
23. **Create `docs/license-compliance.md`** — user guide for license compliance: how to assign licenses to paths (SPDX), configure license grants for cross-tenant sharing, handle proprietary vs open-source code, use `wt license` commands, understand what the server enforces on export/fork/sync

### Phase 4 — Protocol Specs Deep Dive (Depends on Phase 2)

24. **Update `crates/worktree-protocol/specs/WorkTree.md`** — align with new architecture, add tenant model, cross-tenant access, declarative access model with `.wt/` and `.wt-tree/`, license compliance
25. **Update `crates/worktree-protocol/specs/tree/Tree.md`** — align with new architecture, add `.wt-tree/` as tree config folder, registered-path permissions, tree-level license config
26. **Update `crates/worktree-protocol/specs/README.md`** — update index
27. **Create `crates/worktree-protocol/specs/security/Security.md`** — security model, includes access audit logging, license compliance as a security feature (code theft prevention)
28. **Create `crates/worktree-protocol/specs/storage/Storage.md`** — storage architecture

### Phase 5 — Cleanup (Depends on All Above)

29. **Global find-and-replace**: `.worktree/` → `.wt/` (root) and `.wt-tree/` (trees), naming standardization, `wt permission` → `wt access`
30. **Update `docs/README.md`** — final index update with access management and license compliance docs
31. **Cross-reference audit**: ensure all docs link correctly, no broken references
32. **Mark implementation status**: tag each spec section as IMPLEMENTED / IN PROGRESS / PLANNED. Specifically mark: `Scope::RegisteredPath` as PLANNED, `.wt/` and `.wt-tree/` config parsing as PLANNED, explicit path registration as PLANNED, registered-path enforcement as PLANNED, cross-tenant access as PLANNED, tenant email field as PLANNED, `PolicySubject::Tenant` as PLANNED, license compliance module as PLANNED.

---

## 11. Open Items & Decisions Needed

### Must Decide Before Starting

| # | Decision | Options | Recommendation |
|---|---|---|---|
| 1 | bgprocess crate name | `worktree-bgprocess`, `worktree-worker`, `worktree-daemon` | `worktree-worker` (short, clear) |
| 2 | bgprocess binary name | `wt-worker`, `wt-bg`, `wtd` | `wt-worker` (consistent with crate) |
| 3 | Current `worktree-server` crate | Rename to `worktree-worker` and create new `worktree-server`, OR split | Split — create new `worktree-worker` crate, gradually move local daemon code there |
| 4 | Root config folder name | `.wt`, `.w0rktree`, `.worktree` | `.wt` (short, clean, no conflict with git worktree) |
| 4b | Tree config folder name | `.wt-tree`, `.wt-local`, `.tree` | `.wt-tree` (clearly distinct from root `.wt/`, identifies it as tree-specific) |
| 5 | Product name in docs | W0rkTree, Worktree, WorkTree | W0rkTree in marketing/headers, `worktree` in code/crate names |
| 6 | Auto-sync default | On by default, off by default | On by default with easy disable in `.wt/config.toml` |
| 7 | Staged snapshot visibility default | Always on, opt-out per tree, fully off | Always on — staged snapshots sync to server and are visible. Users can opt out of auto-staging per tree in `.wt/config.toml`. |
| 8 | Secret scanning default | On with default patterns, off | On with default patterns (env files, key patterns) |
| 9 | Server protocol | gRPC only, REST + gRPC, QUIC custom | gRPC over QUIC (with HTTP/2 fallback) for sync; REST for admin panel |
| 10 | Tenant isolation | Logical (shared DB/storage), Physical (separate storage) | Logical with namespace isolation; physical as enterprise tier option |
| 11 | Default worktree visibility | Private, Shared, Public | Private by default — owner must explicitly grant access or change visibility |
| 12 | Default license for new worktrees | None, MIT, Apache-2.0, configurable per server | None — owner must explicitly set a license. Server can set a default for the tenant via tenant settings. |
| 13 | License enforcement strictness | Soft (warn only), Hard (block), Configurable per tenant | Hard by default — server blocks unauthorized export/fork/sync. Tenants can configure per-worktree. |
| 14 | SPDX-only license identifiers | SPDX-only, custom strings allowed, both | SPDX-only with `spdx_strict = true` default. Custom strings allowed when `spdx_strict = false` for proprietary/internal licenses. |

### Open Technical Questions

1. **bgprocess ↔ server auth**: How does the bgprocess authenticate to the server on first connect? OAuth2 device flow? API key? CLI-initiated token exchange?
2. **Offline behavior**: When the server is unreachable, the bgprocess continues taking local snapshots. How is the re-sync handled when connectivity returns? Full sync? Delta sync?
3. **Staged visibility privacy**: Can a user prevent staged snapshots from syncing for certain trees or branches? Can a user work in "private mode" where snapshots are local-only until they explicitly push? What are the opt-out controls?
4. **Multi-server**: Can a bgprocess connect to multiple servers (e.g., work + personal)? Or is it one server per local worktree?
5. **Tenant data portability**: Can a tenant export all their data from one server and import to another?
6. **bgprocess resource limits**: How much CPU/memory/disk should the bgprocess be allowed to consume? Configurable limits?
7. **Snapshot deduplication**: If two bgprocess clients create identical snapshots (same content hash), does the server deduplicate?
8. **Conflict-free merge window**: When two users push staged snapshots to the same branch, if they edited different files, should the server auto-merge? What about same file, different lines? Note: this only applies at push time, not at staged sync time — staging is per-user and never conflicts.
9. **Staged snapshot retention**: How long does the server keep staged snapshots that were never pushed to a branch? Auto-expire after N days? Keep forever? Garbage collect when the branch is pushed?
10. **Access config conflict resolution**: If two users with `PolicyManage` permission edit `.wt/access/policies.toml` simultaneously, how are conflicts resolved? Last-write-wins? Merge? Require manual resolution? Should access config changes require a review/approval workflow?
11. **Registered path limits**: Should there be a maximum number of registered paths per `config.toml`? Per tree? Per worktree? What happens when a registered path is removed from `config.toml` but policies still reference it — error on sync? Auto-remove orphaned policies?
12. **Access config validation depth**: How strict is the bgprocess validation before syncing access config changes? Syntax only? Or also check that referenced role names exist, that permission strings are valid, that subjects resolve to real accounts/teams, and that all path-scoped policies reference registered paths?
13. **`.wt-tree/` creation flow**: When a new child tree is created (`wt tree add`), should a `.wt-tree/` folder be scaffolded automatically? With what defaults? Should it inherit the root `.wt/access/` policies explicitly or implicitly?
14. **Access audit retention**: How long should the server keep access decision audit logs? Per-tenant configurable? Required for compliance?
15. **Cross-tenant subject resolution**: When a policy references `{ tenant = "alice-dev" }` or `{ tenant = "bob@company.com" }`, how does the server resolve this to a `TenantId`? What happens if the tenant doesn't exist on the server? Error on config sync? Warn and skip? Lazy resolution on first access?
16. **License inheritance for nested trees**: If the root `.wt/config.toml` sets `license.default = "MIT"` but a child tree's `.wt-tree/config.toml` sets `license = "AGPL-3.0-only"`, does the tree license fully override or merge? What about individual path licenses — do they inherit from tree or from root?
17. **License grant revocation**: When a license grant is revoked, what happens to code the tenant already synced under that grant? Is it retroactive (server requests deletion)? Or only forward-looking (blocks future sync)?
18. **Public worktree staged visibility**: In a public worktree, should staged snapshots from all tenants be visible to all other tenants? Or only to tenants with explicit grants? What about file contents in staged snapshots for proprietary paths?
19. **License compliance and Git export**: When exporting to Git via `wt git export`, how are license restrictions handled? Block proprietary paths from the export entirely? Include a generated LICENSE file? Inject SPDX headers into source files? What about `wt git mirror` — does the license layer block proprietary files from being mirrored to a public Git remote?

---

## Summary

This plan covers **11 files to update**, **18 new files to create**, a **global terminology standardization**, **14 key decisions**, and **19 open technical questions** that must be resolved.

The four most important corrections are:

1. **The bgprocess vs. server split.** Every existing spec treats "the server" as a local daemon. In W0rkTree's actual architecture:
   - **`worktree-worker` (bgprocess)** = runs locally, full local VCS, watches files, auto-snapshots, syncs staged snapshots to server for team visibility (staged ≠ pushed)
   - **`worktree-server`** = runs remotely, multi-tenant, IAM, canonical branch history, stores staged snapshots for visibility, handles explicit branch pushes

2. **Tenant model and cross-tenant access.** A tenant is a user or organization on the worktree-server with a username and email. Tenants own worktrees and can grant other tenants access — either through simple username/email grants in `config.toml` or through full IAM policies in `policies.toml`. Worktrees have visibility modes: Private (owner-only), Shared (explicit grants), and Public (all tenants read, license governs copying).

3. **`.wt/` (root) vs. `.wt-tree/` (individual trees).** The root worktree gets `.wt/` for project-wide config and access. Each child tree gets its own `.wt-tree/` for tree-specific config and access overrides. Tree-level policies override root-level policies for that tree. This replaces the single `.worktree/` folder from current specs.

4. **Declarative access management with explicit path registration AND file-level license compliance.** Permissions are defined as **version-controlled TOML files** in `.wt/access/` and `.wt-tree/access/`, Terraform-style. Path-scoped access requires **explicit registration** in `config.toml` — no glob patterns, no wildcards. On top of IAM, **license compliance is enforced at the file level**: every path can have a license (SPDX), and the server blocks unauthorized export/fork/sync of proprietary code. License grants allow selective cross-tenant sharing of proprietary files. IAM controls what you can do within the worktree; licensing controls what you can take out of it. Both must pass for an operation to succeed.

Once these four distinctions are clear in all specs, everything else follows naturally.

**W0rkTree is not a Git wrapper. It is not a Git extension. It is the replacement.**