# W0rkTree Server Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. What the Server Is NOT](#2-what-the-server-is-not)
- [3. Responsibilities Summary](#3-responsibilities-summary)
- [4. Tenant Model](#4-tenant-model)
- [5. Tenant Isolation](#5-tenant-isolation)
- [6. Worktree Visibility](#6-worktree-visibility)
- [7. Access Control & IAM](#7-access-control--iam)
- [8. Cross-Tenant Access Resolution](#8-cross-tenant-access-resolution)
- [9. License Compliance Engine](#9-license-compliance-engine)
- [10. Branch Protection Enforcement](#10-branch-protection-enforcement)
- [11. Merge Request Management](#11-merge-request-management)
- [12. Staged Snapshot Aggregation](#12-staged-snapshot-aggregation)
- [13. Canonical History & Branch Storage](#13-canonical-history--branch-storage)
- [14. Storage Backend](#14-storage-backend)
- [15. Sync Engine](#15-sync-engine)
- [16. Tag & Release Storage](#16-tag--release-storage)
- [17. API Surface](#17-api-surface)
- [18. Server Configuration](#18-server-configuration)
- [19. Server Lifecycle](#19-server-lifecycle)
- [20. Health, Metrics & Audit Logging](#20-health-metrics--audit-logging)
- [21. Storage Quotas & Rate Limiting](#21-storage-quotas--rate-limiting)
- [22. Error Model](#22-error-model)
- [23. Implementation Status](#23-implementation-status)
- [24. Design Constraints](#24-design-constraints)

---

## 1. Overview

The `worktree-server` is the remote multi-tenant server that hosts worktrees for teams and organizations. It is the **central authority** for canonical history, access control, license compliance, and team collaboration.

Every W0rkTree deployment has exactly two runtimes:

| Runtime | Location | Role |
|---|---|---|
| `worktree-bgprocess` (a.k.a. `worktree-worker`) | Developer's machine | Watches files, creates snapshots, syncs to server |
| `worktree-server` | Remote host | Source of truth for history, access, compliance |

The server is the **source of truth**. If a local history diverges from the server, the server wins. The bgprocess syncs with the server but cannot bypass its enforcement. Every operation that crosses a trust boundary — pushing, merging, granting access, exporting — flows through the server.

This document specifies the server's complete responsibilities, data model, enforcement rules, API surface, configuration, and lifecycle.

---

## 2. What the Server Is NOT

The server is **not** the local daemon. The bgprocess (`worktree-worker`) handles everything on the developer's machine. The following are explicit non-responsibilities of the server:

- **Never watches files** — the server has no access to the developer's filesystem.
- **Never creates auto-snapshots** — it only stores what the bgprocess sends.
- **Never touches the working directory** — no reads, no writes, no filesystem events.
- **Never runs on the developer's machine** — it is always a remote process.
- **Never performs auto-merge** — merge conflict detection is local. The server enforces merge *rules* (reviews, CI gates) but the bgprocess computes the merge itself.
- **Never stores `.wt/` or `.wt-tree/` folders as local state** — it stores the *contents* of access and config files as part of snapshot data, but does not maintain a working directory.

If you are looking for the local daemon spec, see the bgprocess specification.

---

## 3. Responsibilities Summary

| Responsibility | Description |
|---|---|
| **Multi-tenant hosting** | Logical isolation with namespace separation. Physical isolation as enterprise option. |
| **IAM enforcement** | Tenants, accounts, teams, roles, policies — evaluated on every operation. |
| **Cross-tenant access** | Enforces sharing rules between tenants with identity resolution and policy evaluation. |
| **Canonical history** | Stores the authoritative snapshot DAG for every branch. Server history is the truth. |
| **Staged snapshot storage** | Receives and stores staged snapshots from bgprocess clients for team visibility. |
| **License compliance** | Per-path SPDX license enforcement on sync, export, fork, copy, and archive. |
| **Branch protection** | Merge request reviews, required CI checks, signature requirements — all server-side. |
| **Merge request system** | Built-in create, review, approve, reject, merge workflow with stale review handling. |
| **Tag & release management** | Immutable tags and releases with artifact storage. Naming and uniqueness enforced. |
| **Sync engine** | Delta sync, shallow sync, partial tree sync — serves and receives objects efficiently. |
| **Storage backend** | Content-addressable object store with BLAKE3 hashing, deduplication, and per-tenant namespacing. |
| **API surface** | gRPC for bgprocess sync, REST for admin panel and SDK, WebSocket for real-time events. |
| **Quota enforcement** | Storage limits, worktree count limits, and rate limiting per tenant plan. |
| **Audit logging** | Every access decision logged with full context for compliance and debugging. |

---

## 4. Tenant Model

A **tenant** is the identity unit in W0rkTree. Every action — creating a snapshot, pushing to a branch, granting access — is performed by a tenant.

### Tenant Properties

| Property | Type | Description |
|---|---|---|
| `id` | UUID | Immutable, server-generated unique identifier. |
| `username` | String (slug) | Unique, URL-safe slug. Used in paths, access grants, and API routes. Example: `acme-corp`, `alice`. |
| `email` | String | Verified email address. Used for notifications, identity verification, and cross-tenant resolution. |
| `display_name` | String | Human-readable name. Not unique. |
| `type` | Enum | `personal` (individual developer) or `organization` (team/company with member accounts). |
| `status` | Enum | `Active`, `Suspended`, `Deactivated`. Suspended tenants cannot push or create but can read. |
| `plan` | Enum | `Free`, `Pro`, `Enterprise`, `Custom`. Determines resource limits. |
| `created_at` | Timestamp | When the tenant was registered. |
| `updated_at` | Timestamp | Last modification to tenant record. |

### Tenant Types

**Personal tenants** are individual developer accounts. They own worktrees directly and can be granted access to other tenants' worktrees.

**Organization tenants** contain member accounts and teams. An organization tenant has:

| Component | Description |
|---|---|
| **Accounts** | Individual user accounts within the organization. Each account has its own credentials and identity. |
| **Teams** | Named groups of accounts. Used as IAM principals (e.g., `team:frontend-devs`). |
| **Org roles** | Organization-wide roles assigned to accounts (e.g., Org Owner, Org Admin, Org Member). |

### Tenant Status Transitions

```
Created → Active → Suspended → Active       (reactivation)
Created → Active → Suspended → Deactivated  (permanent removal)
Created → Active → Deactivated               (immediate removal)
```

- `Active` — full access according to plan and policies.
- `Suspended` — read-only. Cannot push, create snapshots, create branches, or modify access. Existing data is preserved.
- `Deactivated` — no access. Data retained for a configurable grace period, then purged.

---

## 5. Tenant Isolation

### Namespace Isolation (Default)

Every tenant's data is namespaced by their slug:

```
/<tenant-slug>/<worktree-name>/
```

All storage paths, object references, and API routes are prefixed with the tenant slug. This provides logical isolation — tenants cannot accidentally (or intentionally) access each other's raw storage.

### Physical Isolation (Enterprise Option)

Enterprise tenants may opt into physical isolation:

| Isolation Level | Storage | Compute | Network |
|---|---|---|---|
| **Logical** (default) | Shared object store, namespace-separated | Shared server processes | Shared endpoints |
| **Physical** (enterprise) | Dedicated storage volume or bucket | Dedicated worker pool (optional) | Dedicated endpoint (optional) |

Physical isolation is configured at the server level by an operator, not by the tenant. It is transparent to the bgprocess — the sync protocol is identical regardless of isolation mode.

### Cross-Tenant Boundary

Any operation that crosses a tenant boundary goes through two checks:

1. **IAM check** — Does the requesting tenant have a policy granting this action on this resource?
2. **License check** — Does the license for the affected paths allow this action by this tenant?

Both must pass. Failure of either results in denial.

---

## 6. Worktree Visibility

Every worktree has a visibility mode set in `.wt/config.toml` under `[worktree] visibility`:

| Mode | Default Access | Description |
|---|---|---|
| **Private** (default) | Owner-only | Other tenants must be explicitly granted access via IAM policies or `tenant_access` entries. Nothing is visible to outsiders. |
| **Shared** | Named tenants | Specific tenants are granted access by username or email. The worktree does not appear in public listings. |
| **Public** | All tenants read | All authenticated tenants can read, clone, and sync. Write access (push, branch create, merge) still requires explicit grants. License compliance governs what can be copied or redistributed. |

### Visibility Enforcement

- **Listing**: `wt list --remote` only returns worktrees the requesting tenant can see.
- **Clone/sync**: The server checks visibility before serving any objects.
- **Search**: Public worktrees appear in server-wide search. Private and shared worktrees only appear for authorized tenants.
- **Forking**: Public worktrees can be forked, but license compliance governs which paths are included. Proprietary-licensed paths are excluded from forks unless explicitly granted.

---

## 7. Access Control & IAM

### IAM Components

| Component | Description |
|---|---|
| **Tenants** | Users or organizations. The top-level identity. |
| **Accounts** | Individual user accounts within an organization tenant. |
| **Teams** | Named groups of accounts within a tenant. Referenced as `team:<name>` in policies. |
| **Roles** | Named permission sets. Built-in and custom. |
| **Policies** | RBAC + ABAC rules binding subjects (principals) to permissions (actions) at specific scopes (resources). |
| **Scopes** | Hierarchy of resource granularity for policy evaluation. |

### Built-in Roles

| Role | Permissions | Description |
|---|---|---|
| **Owner** | `*` | Full control. Can delete the worktree, transfer ownership, manage billing. Only one per worktree. |
| **Admin** | `*` except ownership transfer | Full access including IAM management, branch protection config, and tenant access grants. |
| **Maintainer** | `snapshot:*`, `branch:*`, `tag:*`, `merge-request:merge`, `release:*` | Can merge to protected branches, manage tags and releases. Cannot modify access policies. |
| **Developer** | `snapshot:read`, `snapshot:create`, `branch:list`, `branch:create`, `branch:push` | Can create snapshots and push to non-protected branches. Cannot merge to protected branches. |
| **Viewer** | `snapshot:read`, `branch:list`, `tree:sync` | Read-only access. Can clone and sync but not modify anything. |

Custom roles are defined in `.wt/access/roles.toml` and `.wt-tree/access/roles.toml`.

### Policy Model

Policies are defined in `.wt/access/policies.toml` and `.wt-tree/access/policies.toml`:

```toml
[[policy]]
name = "policy-unique-name"
description = "Human-readable description"
effect = "allow"                    # "allow" or "deny"
principals = ["tenant:alice", "team:backend"]  # Who
actions = ["branch:push", "snapshot:create"]   # What
resources = ["tree:api-service/*"]             # Where
conditions = {}                                # Optional ABAC conditions
```

### Scope Hierarchy

Scopes form a hierarchy from broadest to most specific:

```
Global → Tenant → Tree → Branch → RegisteredPath
```

**Resolution rules:**

1. Deny always beats Allow at the same scope level.
2. More specific scope overrides broader scope.
3. If no policy matches, the default is Deny (closed by default).
4. Tree-level policies ADD to root-level policies (strictest combination wins — the ceiling model).

### Policy Storage and Sync

Access files (`.wt/access/roles.toml`, `.wt/access/policies.toml`, and their `.wt-tree/` counterparts) are version-controlled and synced like any other file. However:

- Changes to access files are themselves access-controlled. Modifying policies requires `policy:manage` permission or the `Admin`/`Owner` role.
- The server parses and validates policy files on every push. Invalid policies are rejected with a descriptive error.
- The server maintains a compiled policy cache for fast evaluation. The cache is invalidated when access files change.

### Permission Actions

| Action | Description |
|---|---|
| `snapshot:read` | Read snapshot content and metadata |
| `snapshot:create` | Create new snapshots |
| `branch:list` | List branches |
| `branch:create` | Create new branches |
| `branch:push` | Push to a branch (non-protected) |
| `branch:merge` | Merge into a branch (protected or not) |
| `branch:delete` | Delete a branch |
| `tag:create` | Create tags |
| `tag:delete` | Delete tags |
| `release:create` | Create releases |
| `release:delete` | Delete releases |
| `tree:sync` | Sync (clone/pull) tree content |
| `tree:create` | Create new trees |
| `tree:delete` | Delete trees |
| `merge-request:create` | Create merge requests |
| `merge-request:review` | Approve or request changes |
| `merge-request:merge` | Merge a merge request |
| `policy:manage` | Create, modify, or delete access policies |
| `policy:view` | Read access policies |
| `config:manage` | Modify worktree or tree configuration |
| `file:read` | Read file content (used in license grants) |
| `file:modify` | Modify file content (used in license grants) |
| `file:redistribute` | Include file in exports/forks (used in license grants) |
| `*` | Wildcard — all actions |

---

## 8. Cross-Tenant Access Resolution

When a policy references an external tenant, the server resolves the identity:

### Resolution Flow

1. Policy contains a principal like `{ tenant = "alice-dev" }` or `{ tenant = "bob@company.com" }`.
2. Server resolves the username or email to an internal `TenantId` (UUID).
3. If the tenant does not exist on this server, the policy is rejected on config sync with error `UnknownTenant`.
4. The resolved `TenantId` is stored in the compiled policy cache.
5. On access check, the server evaluates all policies where the subject matches the requesting tenant's `TenantId`.

### Cross-Tenant Operation Flow

```
bgprocess (tenant: bob) → server: "push to alice/my-project/main"
                           │
                           ├─ 1. Authenticate bob (TLS client cert / token)
                           ├─ 2. Resolve target: tenant=alice, worktree=my-project, branch=main
                           ├─ 3. IAM check: evaluate policies for bob on alice/my-project/main
                           │     └─ Find matching policies, check effect (allow/deny)
                           ├─ 4. License check: evaluate license grants for affected paths
                           │     └─ For each path in the push, check license allows bob's action
                           ├─ 5. Branch protection check: is main protected? Are rules satisfied?
                           │     └─ If no_direct_push: reject (must use merge request)
                           └─ 6. If all checks pass: accept push, update branch DAG
```

---

## 9. License Compliance Engine

The server enforces file-level license compliance on every operation that moves data across trust boundaries.

### License Storage

Licenses are assigned per-path in `.wt/config.toml` and `.wt-tree/config.toml` using SPDX identifiers:

```toml
[licenses]
default = "MIT"

[licenses.paths]
"src/core/**" = "MIT"
"src/enterprise/**" = "LicenseRef-Proprietary"
"vendor/openssl/**" = "Apache-2.0"
```

The server indexes license assignments per worktree and keeps them current as snapshots are pushed.

### License Grant Levels

| Grant | Description |
|---|---|
| **Read-only** | Tenant can view the file content but cannot modify or include it in exports. |
| **Modify** | Tenant can view and modify the file within the worktree. |
| **Redistribute** | Tenant can include the file in exports, forks, archives, and `wt git export`. |

### Enforcement Points

| Operation | Enforcement |
|---|---|
| **Sync / Clone** | License grants checked per-path. Paths without read grant are excluded (stub metadata only). |
| **Export (`wt git export`)** | Proprietary paths excluded unless tenant has redistribute grant. |
| **Fork** | Cross-tenant forks respect license boundaries. Proprietary paths stripped from fork unless granted. |
| **Archive (`wt archive`)** | Same as export — redistribute grant required for inclusion. |
| **Copy between trees** | License compatibility checked. Incompatible licenses block the copy. |

### Dual Enforcement Rule

Both IAM and License must pass for every operation:

```
IAM check: Does this tenant have permission?  ──┐
                                                 ├─ BOTH must pass → Operation allowed
License check: Does the license allow this?    ──┘
```

A tenant with `Admin` IAM permissions still cannot redistribute a file with a read-only license grant. A tenant with a redistribute license grant still cannot push if they lack `branch:push` IAM permission.

---

## 10. Branch Protection Enforcement

The server enforces branch protection rules defined in `.wt/config.toml` and `.wt-tree/config.toml`. The bgprocess **cannot bypass** these rules.

### Protection Rules

| Rule | Description |
|---|---|
| `no_direct_push` | Reject direct pushes. Changes must go through a merge request. |
| `require_merge_review` | At least one approval required before merge. |
| `required_reviewers` | Minimum number of approving reviewers. |
| `no_delete` | Branch cannot be deleted. |
| `no_force` | Force-push is forbidden. History cannot be rewritten. |
| `require_ci_pass` | All specified CI checks must pass before merge is allowed. |
| `required_ci_checks` | List of CI check names that must report success. |
| `require_snapshot_signature` | Snapshots must carry a valid cryptographic signature. |

### Protection Inheritance (Ceiling Model)

Tree-level protections ADD to root-level protections. They cannot relax them:

- Root requires `required_reviewers = 2`. Tree can require 3 but cannot require 1.
- Root requires `require_ci_pass = true`. Tree cannot set it to `false`.
- Root requires `no_direct_push = true`. Tree cannot set it to `false`.

The server computes the effective protection by taking the **strictest combination** of root and tree rules.

### Push Rejection Flow

```
bgprocess → server: "push snapshot X to branch main"
                     │
                     ├─ Is main protected with no_direct_push?
                     │   └─ YES → Reject: "branch main requires a merge request"
                     │
                     ├─ Is no_force set and is this a force-push?
                     │   └─ YES → Reject: "force-push forbidden on branch main"
                     │
                     └─ Is require_snapshot_signature set?
                         └─ YES → Verify signature on snapshot X
                             └─ INVALID → Reject: "snapshot signature verification failed"
```

---

## 11. Merge Request Management

The server provides a built-in merge request system. No external platform (GitHub, GitLab) is required.

### Merge Request Lifecycle

```
Created → Open → Approved → Merged
                    ↓
               Changes Requested → Open (updated) → Approved → Merged
                                                        ↓
                                                      Closed (abandoned)
```

### Merge Request Properties

| Property | Type | Description |
|---|---|---|
| `id` | Integer | Auto-incrementing, unique within the worktree. |
| `title` | String | Human-readable title. |
| `description` | String | Markdown description. |
| `source_branch` | String | Branch to merge from. |
| `target_branch` | String | Branch to merge into. |
| `author` | TenantId | Who created the merge request. |
| `status` | Enum | `Open`, `Approved`, `Merged`, `Closed`. |
| `created_at` | Timestamp | Creation time. |
| `updated_at` | Timestamp | Last modification time. |
| `reviews` | List | Review records with reviewer identity, decision, and timestamp. |
| `ci_status` | Map | CI check name → status (`pending`, `running`, `passed`, `failed`). |

### Review Tracking

Each review records:

| Field | Description |
|---|---|
| `reviewer` | TenantId of the reviewer. |
| `role` | Role of the reviewer at the time of review (for audit). |
| `decision` | `approved`, `changes_requested`, or `commented`. |
| `snapshot_id` | The snapshot hash at the time of the review. |
| `timestamp` | When the review was submitted. |

### Stale Review Handling

When the source branch is updated after a review:

1. The server compares the current source branch head against `snapshot_id` in each approval.
2. If the source branch has new snapshots since the approval, the approval is marked **stale**.
3. Stale approvals do not count toward `required_reviewers`.
4. The reviewer must re-approve against the new head.

This prevents merging code that was not actually reviewed.

### Merge Execution

When a merge request is merged:

1. Server verifies all branch protection rules are satisfied (reviews, CI, signatures).
2. Server verifies the source branch is up-to-date with the target (or performs a merge).
3. Server creates a merge snapshot on the target branch referencing both parent snapshots.
4. Merge request status is set to `Merged`.
5. Source branch is optionally deleted (configurable per merge request).

### CI Integration

The server exposes webhook endpoints for CI systems to report check status:

```
POST /api/v1/ci/status
{
  "worktree": "acme/my-project",
  "merge_request": 42,
  "check_name": "build",
  "status": "passed",
  "details_url": "https://ci.example.com/builds/1234"
}
```

The server stores CI status per merge request and evaluates `require_ci_pass` / `required_ci_checks` during merge.

---

## 12. Staged Snapshot Aggregation

The server stores staged snapshots from all connected bgprocess clients. Staged snapshots provide **team visibility** into active work before it becomes part of branch history.

### Staged vs. Pushed

| Aspect | Staged Snapshot | Pushed Snapshot |
|---|---|---|
| **Visibility** | Visible to team via API and dashboard | Part of branch history |
| **Persistence** | Temporary, configurable retention | Permanent (append-only DAG) |
| **Creation** | Auto-uploaded by bgprocess during sync | Explicit `wt push` by developer |
| **Branch DAG** | Not part of branch DAG | Linked into branch DAG |
| **Conflict** | Cannot conflict (not on a branch tip) | May conflict with branch tip |

### Staged Snapshot Indexing

Staged snapshots are indexed by:

- **Tenant** — which developer's bgprocess uploaded it
- **Branch** — which branch the developer is working on
- **Tree** — which tree the changes are in
- **Timestamp** — when the staged snapshot was received

### Real-Time Streaming

The server provides a WebSocket endpoint for real-time staged snapshot events:

```
WS /api/v1/ws/staged/<tenant-slug>/<worktree-name>
```

Events:

| Event | Description |
|---|---|
| `staged.created` | A new staged snapshot was received. Includes snapshot metadata and changed file list. |
| `staged.updated` | A staged snapshot was replaced by a newer one from the same user/branch. |
| `staged.promoted` | A staged snapshot was pushed and is now part of branch history. |
| `staged.expired` | A staged snapshot was removed due to retention policy. |

### Retention Policy

Staged snapshot retention is configurable per tenant plan:

| Plan | Default Retention | Max Retention |
|---|---|---|
| Free | 24 hours | 7 days |
| Pro | 7 days | 30 days |
| Enterprise | 30 days | Unlimited |

Staged snapshots are automatically purged after retention expires. A push promotes the staged snapshot to permanent history, removing it from the staged index.

---

## 13. Canonical History & Branch Storage

The server stores the canonical snapshot DAG — the authoritative history of every branch.

### Branch Data Model

| Property | Description |
|---|---|
| `name` | Branch name (e.g., `main`, `feature/auth`). |
| `head` | Snapshot hash of the current branch tip. |
| `parent_snapshots` | DAG links from each snapshot to its parent(s). |
| `created_at` | When the branch was created. |
| `created_by` | TenantId of the creator. |
| `protection` | Computed effective protection rules. |

### Append-Only History

Branch history is **append-only**. There is no `rebase`, no `force-push` (unless explicitly allowed by protection rules), and no history rewriting. Every snapshot that becomes part of branch history is permanent.

### Conflict Detection on Push

When a bgprocess pushes to a branch:

1. Server checks if the push is a fast-forward (the pushed snapshot's parent is the current branch tip).
2. If fast-forward: accept, update branch tip.
3. If not fast-forward: reject with `ConflictDetected`. The bgprocess must merge locally and retry.
4. If `no_force` is set: force-push is rejected regardless. If `no_force` is not set and the push is marked as force: accept (overwrites branch tip).

---

## 14. Storage Backend

The server uses a content-addressable object store with BLAKE3 hashing.

### Object Types

| Type | Description |
|---|---|
| **Blob** | Raw file content. Identified by BLAKE3 hash of content. |
| **Tree** | Directory listing — maps file names to blob or subtree hashes. |
| **Snapshot** | Point-in-time record. References a tree hash, parent snapshot(s), author, timestamp, message. |
| **Manifest** | Lists all objects required to reconstruct a snapshot. Used for sync optimization. |
| **Delta** | Binary diff between two blobs. Used for efficient transfer, not stored long-term. |
| **Chunk** | Large file chunk (from FastCDC splitting). Content-addressed, deduplicated. |

### Deduplication

Identical content produces identical BLAKE3 hashes. The server stores each unique object exactly once, regardless of how many tenants, worktrees, or branches reference it.

For large files, content-defined chunking (FastCDC) means that even partial changes to a large file only produce new chunks for the changed regions. Unchanged chunks are deduplicated automatically.

### Per-Tenant Namespacing

Although objects are deduplicated at the storage layer, the **reference graph** is namespaced per tenant. Tenant A cannot enumerate or traverse tenant B's object references, even if they happen to share deduplicated blob content.

### Storage Layout

```
<storage_root>/
├── objects/                    # Content-addressable objects (shared, deduplicated)
│   ├── ab/                     # First two hex chars of hash
│   │   ├── cd1234...          # Full hash as filename
│   │   └── ef5678...
│   └── ...
├── tenants/
│   ├── <tenant-slug>/
│   │   ├── worktrees/
│   │   │   ├── <worktree-name>/
│   │   │   │   ├── branches/   # Branch tip pointers
│   │   │   │   ├── tags/       # Tag references
│   │   │   │   ├── releases/   # Release metadata
│   │   │   │   ├── staged/     # Staged snapshot index
│   │   │   │   └── config/     # Cached parsed config
│   │   │   └── ...
│   │   └── account/            # Tenant metadata, plans, quotas
│   └── ...
└── server/                     # Server-level config, audit logs
```

---

## 15. Sync Engine

The sync engine handles bidirectional data transfer between bgprocess clients and the server.

### Inbound Operations (bgprocess → server)

| Operation | Description |
|---|---|
| **Stage** | Upload staged snapshot. Server indexes it for team visibility. |
| **Push** | Push snapshot(s) to a branch. Server validates, runs checks, updates branch DAG. |
| **Config sync** | Upload changes to `.wt/` and `.wt-tree/` config/access files. Server validates and compiles. |
| **Tag push** | Create or update a tag on the server. |

### Outbound Operations (server → bgprocess)

| Operation | Description |
|---|---|
| **Branch update** | Serve new snapshots on a branch since the client's last known head. |
| **Clone** | Serve all objects needed to reconstruct a worktree at a given snapshot. |
| **Shallow clone** | Serve objects for a limited depth of history. |
| **Partial tree sync** | Serve metadata-only stubs for trees the client doesn't need locally. |
| **Tag pull** | Serve tag references and their target snapshots. |

### Delta Sync Protocol

The sync engine minimizes data transfer:

1. Client sends its known object hashes (have-list).
2. Server computes the set of objects the client needs (want-list minus have-list).
3. Server packs the needed objects, optionally computing binary deltas against objects the client already has.
4. Client applies deltas to reconstruct full objects.

### Shallow Sync

Clients can request a limited depth of history:

```
SYNC request: branch=main, depth=10
```

The server serves only the last 10 snapshots on main and their referenced trees/blobs. The client can request deeper history later with incremental fetches.

### Partial Tree Sync

For worktrees with many trees, clients can sync a subset:

```
SYNC request: branch=main, trees=["frontend", "api-service"]
```

Trees not in the requested set are served as **stubs** — metadata only (name, latest snapshot hash, size) without file content. Stubs can be hydrated later on demand.

---

## 16. Tag & Release Storage

### Tags

Tags are immutable pointers to specific snapshots:

| Property | Description |
|---|---|
| `name` | Unique within the worktree. Follows naming rules (no spaces, URL-safe). |
| `target` | Snapshot hash the tag points to. |
| `author` | TenantId of the creator. |
| `message` | Optional annotation message. |
| `signature` | Optional cryptographic signature. |
| `created_at` | Creation timestamp. |

Tags are **immutable once created**. To "move" a tag, delete and recreate it (requires `tag:delete` permission). The deletion is logged in the audit trail.

### Releases

Releases are named, publishable milestones associated with a tag:

| Property | Description |
|---|---|
| `tag` | The tag this release is associated with. |
| `name` | Human-readable release name (e.g., "v2.1.0 — Performance Release"). |
| `notes` | Markdown release notes. |
| `artifacts` | List of attached files (binaries, archives, documentation). |
| `created_at` | Creation timestamp. |
| `created_by` | TenantId of the creator. |

Release artifacts are stored in the content-addressable store and referenced by hash. They are subject to the same deduplication and quota rules as other objects.

---

## 17. API Surface

The server exposes three API transports:

### gRPC (Primary Sync Protocol)

- **Transport**: QUIC with HTTP/2 fallback.
- **Purpose**: All bgprocess ↔ server sync operations.
- **Authentication**: Mutual TLS (client certificates) or bearer tokens.
- **Services**:
  - `SyncService` — push, pull, stage, clone, shallow sync, partial sync.
  - `BranchService` — create, delete, list, get branch status.
  - `TagService` — create, delete, list tags.
  - `MergeRequestService` — create, list, review, merge, close.
  - `AuthService` — authenticate, refresh tokens, verify identity.

### REST (Admin & Integration)

- **Transport**: HTTPS (TLS 1.3).
- **Purpose**: Admin panel, web UI, SDK, third-party integrations, CI webhooks.
- **Authentication**: Bearer tokens (JWT), API keys.
- **Base path**: `/api/v1/`
- **Key endpoints**:

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Health check |
| `GET` | `/metrics` | Prometheus metrics |
| `GET` | `/tenants/:slug` | Get tenant info |
| `GET` | `/tenants/:slug/worktrees` | List worktrees |
| `GET` | `/tenants/:slug/worktrees/:name/branches` | List branches |
| `GET` | `/tenants/:slug/worktrees/:name/merge-requests` | List merge requests |
| `POST` | `/tenants/:slug/worktrees/:name/merge-requests` | Create merge request |
| `POST` | `/tenants/:slug/worktrees/:name/merge-requests/:id/review` | Submit review |
| `POST` | `/tenants/:slug/worktrees/:name/merge-requests/:id/merge` | Merge |
| `GET` | `/tenants/:slug/worktrees/:name/tags` | List tags |
| `GET` | `/tenants/:slug/worktrees/:name/releases` | List releases |
| `POST` | `/ci/status` | CI status webhook |
| `GET` | `/tenants/:slug/worktrees/:name/staged` | List staged snapshots |

### WebSocket (Real-Time Events)

- **Transport**: WSS (WebSocket over TLS).
- **Purpose**: Real-time event streaming for staged snapshots, branch updates, merge request activity.
- **Authentication**: Bearer token in connection handshake.
- **Endpoints**:
  - `/api/v1/ws/staged/:slug/:worktree` — staged snapshot events.
  - `/api/v1/ws/branches/:slug/:worktree` — branch update events.
  - `/api/v1/ws/merge-requests/:slug/:worktree` — merge request activity events.

---

## 18. Server Configuration

Server configuration is in a server-level config file (NOT `.wt/config.toml` — that's worktree config).

```toml
[server]
listen_addr = "0.0.0.0:443"       # REST + WebSocket
grpc_addr = "0.0.0.0:8443"        # gRPC sync protocol
storage_path = "/var/lib/w0rktree" # Root storage directory
log_level = "info"                 # trace, debug, info, warn, error
worker_threads = 0                 # 0 = auto-detect from CPU count

[tls]
cert_path = "/etc/w0rktree/cert.pem"
key_path = "/etc/w0rktree/key.pem"
client_ca_path = "/etc/w0rktree/ca.pem"  # For mTLS client auth

[tenant.defaults]
default_plan = "free"
max_worktrees_free = 5
max_worktrees_pro = 50
max_worktrees_enterprise = 0      # 0 = unlimited
max_storage_free_gb = 1
max_storage_pro_gb = 50
max_storage_enterprise_gb = 0     # 0 = unlimited

[tenant.defaults.rate_limits]
requests_per_minute_free = 60
requests_per_minute_pro = 600
requests_per_minute_enterprise = 0  # 0 = unlimited

[staged]
default_retention_hours = 168      # 7 days
max_retention_hours = 720          # 30 days

[sync]
max_push_size_mb = 100             # Max single push payload
max_clone_depth = 0                # 0 = unlimited
delta_compression = true

[audit]
enabled = true
log_path = "/var/log/w0rktree/audit.log"
retention_days = 365
```

---

## 19. Server Lifecycle

### Startup Sequence

1. **Load configuration** — Read and validate server config file.
2. **Initialize storage** — Connect to storage backend, verify integrity, run migrations if needed.
3. **Compile policy cache** — Load and compile all tenant access policies for fast evaluation.
4. **Start gRPC server** — Bind to `grpc_addr`, begin accepting bgprocess connections.
5. **Start REST server** — Bind to `listen_addr`, begin accepting HTTP/WebSocket connections.
6. **Start background tasks** — Staged snapshot expiry, quota recalculation, audit log rotation.
7. **Report ready** — Log startup complete, expose health check as healthy.

### Runtime

- Handle sync requests on gRPC (push, pull, stage, clone).
- Handle API requests on REST (admin, merge requests, CI webhooks).
- Handle WebSocket connections (real-time event streaming).
- Evaluate access policies on every request.
- Enforce license compliance on every data-crossing operation.
- Run periodic background tasks (expiry, quota, audit rotation).

### Graceful Shutdown

1. **Stop accepting new connections** — Close listening sockets.
2. **Drain active connections** — Wait for in-flight requests to complete (configurable timeout, default 30s).
3. **Flush pending writes** — Ensure all accepted pushes are persisted to storage.
4. **Close storage backend** — Flush caches, close file handles.
5. **Write final audit log entry** — Record shutdown event.
6. **Exit** — Process terminates with exit code 0.

If the drain timeout expires, forcefully terminate remaining connections and log a warning.

---

## 20. Health, Metrics & Audit Logging

### Health Check

```
GET /health
```

Response:

```json
{
  "status": "healthy",
  "storage": "ok",
  "grpc": "listening",
  "uptime_seconds": 86400
}
```

Returns HTTP 200 when healthy, HTTP 503 when degraded. Used by load balancers and orchestrators.

### Metrics

```
GET /metrics
```

Prometheus-format metrics:

| Metric | Type | Description |
|---|---|---|
| `wt_server_requests_total` | Counter | Total API requests by method, path, status. |
| `wt_server_request_duration_seconds` | Histogram | Request latency distribution. |
| `wt_sync_pushes_total` | Counter | Total pushes by tenant, worktree, result. |
| `wt_sync_pulls_total` | Counter | Total pulls by tenant, worktree. |
| `wt_sync_bytes_transferred` | Counter | Total bytes transferred (in/out). |
| `wt_staged_snapshots_active` | Gauge | Currently stored staged snapshots. |
| `wt_tenants_active` | Gauge | Number of active tenants. |
| `wt_storage_used_bytes` | Gauge | Storage used per tenant. |
| `wt_iam_decisions_total` | Counter | Access decisions by result (allow/deny). |
| `wt_merge_requests_total` | Counter | Merge requests by status. |
| `wt_websocket_connections` | Gauge | Active WebSocket connections. |

### Audit Logging

Every access decision is logged with full context:

```json
{
  "timestamp": "2025-07-14T10:30:00Z",
  "event": "access_decision",
  "tenant": "bob",
  "action": "branch:push",
  "resource": "alice/my-project/main",
  "decision": "deny",
  "reason": "branch_protection: no_direct_push is enabled",
  "source_ip": "198.51.100.42",
  "request_id": "req-abc123"
}
```

Audit logs are:
- Written to the configured `audit.log_path`.
- Rotated daily with configurable retention.
- Include every IAM decision, license check, branch protection evaluation, and administrative action.
- Immutable once written (append-only file, no modification).

---

## 21. Storage Quotas & Rate Limiting

### Storage Quotas

Each tenant has storage limits based on their plan:

| Plan | Worktree Limit | Storage Limit |
|---|---|---|
| Free | 5 | 1 GB |
| Pro | 50 | 50 GB |
| Enterprise | Unlimited | Unlimited (or custom) |
| Custom | Configurable | Configurable |

When a tenant exceeds their storage quota:
1. New pushes are rejected with `QuotaExceeded` error.
2. Staged snapshot uploads are rejected.
3. Existing data is preserved — no automatic deletion.
4. The tenant can delete data or upgrade their plan to restore push capability.

### Rate Limiting

Rate limits are enforced per tenant per time window:

| Plan | Requests/minute | Concurrent sync connections |
|---|---|---|
| Free | 60 | 2 |
| Pro | 600 | 10 |
| Enterprise | Unlimited | Unlimited |

Rate limit responses include standard headers:

```
HTTP 429 Too Many Requests
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1720958460
```

---

## 22. Error Model

The server uses structured errors with machine-readable codes:

| Code | HTTP Status | Description |
|---|---|---|
| `AuthenticationRequired` | 401 | No valid credentials provided. |
| `AccessDenied` | 403 | IAM policy denies the requested action. |
| `LicenseDenied` | 403 | License compliance denies the requested action. |
| `BranchProtectionViolation` | 403 | Branch protection rule prevents the operation. |
| `TenantNotFound` | 404 | Referenced tenant does not exist. |
| `WorktreeNotFound` | 404 | Referenced worktree does not exist. |
| `BranchNotFound` | 404 | Referenced branch does not exist. |
| `ConflictDetected` | 409 | Push is not a fast-forward. Client must merge and retry. |
| `QuotaExceeded` | 413 | Tenant has exceeded their storage or worktree quota. |
| `RateLimitExceeded` | 429 | Too many requests. |
| `InvalidPolicy` | 422 | Pushed access policy file has syntax or semantic errors. |
| `InvalidConfig` | 422 | Pushed config file has syntax or semantic errors. |
| `StaleReview` | 422 | Merge request has stale approvals due to source branch changes. |
| `CIChecksPending` | 422 | Required CI checks have not all passed. |
| `UnknownTenant` | 422 | Policy references a tenant that doesn't exist on this server. |
| `InternalError` | 500 | Unexpected server error. Logged for investigation. |

Error responses include the code, a human-readable message, and an optional `details` field:

```json
{
  "error": {
    "code": "BranchProtectionViolation",
    "message": "Direct push to 'main' is forbidden. Use a merge request.",
    "details": {
      "branch": "main",
      "rule": "no_direct_push",
      "worktree": "acme/my-project"
    }
  }
}
```

---

## 23. Implementation Status

| Area | Status | Notes |
|---|---|---|
| Routing & basic handlers | **Implemented** | In `worktree-server` crate, but mixed with bgprocess code. |
| gRPC sync protocol | **Partial** | Basic sync works. Delta sync and shallow sync are TODO. |
| REST API | **Partial** | Some endpoints exist. Full admin API is TODO. |
| Tenant management | **TODO** | Tenant CRUD, plan management, org structure. |
| IAM enforcement | **TODO** | Policy parsing exists but server-side enforcement is not wired. |
| License compliance | **TODO** | Data model exists in protocol crate but enforcement engine is not implemented. |
| Merge request system | **TODO** | Data model planned, no implementation. |
| Staged snapshot storage | **TODO** | bgprocess sends staged snapshots but server doesn't aggregate or index them. |
| Branch protection | **TODO** | Rules are parsed from config but not enforced on push. |
| WebSocket streaming | **TODO** | No real-time event system yet. |
| Storage quotas | **TODO** | No quota tracking or enforcement. |
| Audit logging | **TODO** | No structured audit logging. |
| CI integration | **TODO** | No webhook endpoints for CI status. |

### Migration Plan

1. **Extract pure server code** from the mixed bgprocess/server crate into a standalone `worktree-server` binary.
2. **Implement tenant management** — CRUD, plans, org/team structure.
3. **Wire IAM enforcement** — policy evaluation on every request.
4. **Implement staged snapshot aggregation** — storage, indexing, WebSocket streaming.
5. **Add branch protection enforcement** on push and merge.
6. **Build merge request system** — full lifecycle with review tracking.
7. **Implement license compliance engine** — per-path enforcement on all data-crossing operations.
8. **Add storage quotas and rate limiting**.
9. **Add audit logging** to all decision points.
10. **Build admin REST API** for web panel and SDK.

---

## 24. Design Constraints

These constraints are **non-negotiable architectural decisions**:

1. **The server is the source of truth.** If local history diverges, the server wins.
2. **The server never touches the working directory.** It has no filesystem access on the developer's machine.
3. **The bgprocess never enforces access control.** It reads access config for display only. The server enforces.
4. **Deny by default.** If no policy explicitly allows an action, it is denied.
5. **Dual enforcement.** Both IAM and license checks must pass. Neither can be bypassed.
6. **Append-only history.** Pushed snapshots are permanent. No rebase, no history rewriting.
7. **Ceiling model.** Tree-level config can only be stricter than root-level, never more permissive.
8. **Tenant isolation.** One tenant's data is never accessible to another without explicit policy grants.
9. **All enforcement is server-side.** The bgprocess is untrusted. Every claim is verified.
10. **Structured errors.** Every rejection includes a machine-readable code and human-readable explanation.