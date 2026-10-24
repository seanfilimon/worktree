# Tenant Model Specification

> **Status**: Draft v1.0
> **Last Updated**: 2025-01-15
> **Scope**: W0rkTree Protocol — Identity & Access Management

---

## Table of Contents

1. [Overview](#overview)
2. [What is a Tenant?](#what-is-a-tenant)
3. [Tenant Properties](#tenant-properties)
4. [Tenant Types](#tenant-types)
5. [Tenant Lifecycle](#tenant-lifecycle)
6. [Cross-Tenant Access](#cross-tenant-access)
7. [Tenant Resolution](#tenant-resolution)
8. [Worktree Visibility Modes](#worktree-visibility-modes)
9. [Organization Tenants](#organization-tenants)
10. [Tenant Data Model](#tenant-data-model)
11. [Tenant Limits and Quotas](#tenant-limits-and-quotas)
12. [Tenant Attributes for ABAC](#tenant-attributes-for-abac)
13. [Security Considerations](#security-considerations)
14. [Implementation Status](#implementation-status)

---

## Overview

A **tenant** is the identity unit on the W0rkTree server. Every person or organization
that uses W0rkTree is a tenant. Tenants own worktrees and can grant other tenants access.

Unlike Git hosting platforms where identity is an afterthought layered on top by each
provider, W0rkTree treats tenant identity as a **first-class protocol concept**. The
tenant model is the foundation upon which the entire IAM system is built.

Key principles:

- **Every actor is a tenant** — whether individual or organization
- **Tenants own worktrees** — ownership is the root of all access
- **Tenants grant access** — cross-tenant collaboration is built into the protocol
- **Tenants are server-resolved** — the server is the source of truth for identity

---

## What is a Tenant?

A tenant represents a single identity on a W0rkTree server. It is defined by:

- A **username** (slug): unique identifier, URL-safe (e.g., `"alice-dev"`, `"acme-corp"`)
- An **email**: used for identification and grant resolution (e.g., `"alice@company.com"`)
- Can be **personal** (individual developer) or **organization** (team/company)
- Owns worktrees with full control
- Can be granted access to other tenants' worktrees

### Username Rules

Usernames (slugs) must conform to the following rules:

- 1–64 characters in length
- Lowercase alphanumeric characters and hyphens only: `[a-z0-9-]`
- Must start and end with an alphanumeric character
- No consecutive hyphens (`--`)
- Must be unique across the entire server
- Cannot be a reserved name (e.g., `admin`, `system`, `root`, `api`, `www`)

### Email Rules

- Must be a valid email address (RFC 5322)
- Must be verified before the tenant is activated
- Must be unique across the entire server
- Used as an alternate lookup key for cross-tenant grants
- Used for notifications and account recovery

---

## Tenant Properties

| Property           | Type                        | Required | Description                             |
|--------------------|-----------------------------|----------|-----------------------------------------|
| `id`               | `TenantId` (UUID v7)       | Yes      | Internal unique identifier              |
| `name`             | `String`                    | Yes      | Display name (human-readable)           |
| `slug`             | `String`                    | Yes      | URL-safe username (unique)              |
| `email`            | `String`                    | Yes      | Primary email address (unique)          |
| `type`             | `personal` / `organization` | Yes      | Account type                            |
| `status`           | `Active` / `Suspended`      | Yes      | Account status                          |
| `plan`             | `Free` / `Pro` / `Enterprise` / `Custom` | Yes | Subscription plan              |
| `max_accounts`     | `u32`                       | No       | Max accounts (org tenants only)         |
| `max_trees`        | `u32`                       | Yes      | Maximum number of worktrees             |
| `max_storage_bytes`| `u64`                       | Yes      | Total storage quota in bytes            |
| `created_at`       | `DateTime<Utc>`             | Yes      | Account creation timestamp              |
| `updated_at`       | `DateTime<Utc>`             | Yes      | Last modification timestamp             |
| `attributes`       | `HashMap<String, String>`   | No       | Custom attributes for ABAC conditions   |

### Property Details

**`id` — TenantId**

A UUID v7 (time-ordered) that serves as the internal primary key. This ID is:
- Never exposed in URLs or user-facing interfaces
- Used internally for policy resolution and storage
- Immutable once assigned
- Time-ordered for efficient indexing

**`slug` — Username**

The tenant's public identifier. Used in:
- Worktree URLs: `wt://server/alice-dev/my-project`
- Cross-tenant grants: `{ tenant = "alice-dev" }`
- CLI commands: `wt clone alice-dev/my-project`

**`email` — Primary Email**

Used for:
- Account verification and recovery
- Cross-tenant grants by email: `{ tenant = "alice@company.com" }`
- Notifications (snapshot activity, access changes, etc.)
- Tenant resolution (fallback after slug lookup)

---

## Tenant Types

### Personal Tenant

A personal tenant represents an individual developer.

- One account (the owner) — no sub-accounts
- Cannot have teams (teams are org-only)
- Simplified management — fewer IAM concepts to worry about
- Ideal for solo developers and open-source contributors

### Organization Tenant

An organization tenant represents a team, company, or group.

- Can have multiple **accounts** (members)
- Supports **teams** for group-based access control
- Has an **owner account** with full control
- Has **admin accounts** that can manage the org
- Members inherit org-level access to org-owned worktrees
- Subject to `max_accounts` limit based on plan

---

## Tenant Lifecycle

### Creation

1. User signs up with username, email, display name
2. Server validates uniqueness of slug and email
3. Server assigns a TenantId (UUID v7)
4. Email verification is initiated
5. Tenant status is set to `Active` upon email verification
6. Default plan and limits are applied

### Modification

- Display name, email can be updated (email requires re-verification)
- Slug changes are allowed but rate-limited (once per 30 days)
- Old slugs are reserved for 90 days to prevent hijacking
- Plan and limits are updated via billing system

### Suspension

- Tenant status set to `Suspended`
- All access tokens are revoked
- Worktrees remain intact but become read-only
- Cross-tenant grants referencing this tenant are paused
- Can be reactivated by admin or billing resolution

### Deletion

- Soft-delete with 30-day recovery window
- All worktrees are marked for deletion
- All cross-tenant grants are revoked
- Slug is reserved for 1 year after deletion
- After recovery window: permanent deletion of all data

---

## Cross-Tenant Access

Cross-tenant access is the mechanism by which one tenant grants another tenant
access to their worktrees. This is a core feature of W0rkTree's collaborative model.

### Two Methods of Granting Access

#### Method 1: Simple Grants in `.wt/config.toml`

Simple grants are a shorthand for common access patterns. They are defined in the
worktree's root configuration file:

```toml
# .wt/config.toml

# Grant by username (slug)
[[tenant_access]]
tenant = "partner-corp"
permissions = ["tree:read"]

# Grant by email
[[tenant_access]]
tenant = "bob@company.com"
permissions = ["tree:read", "tree:write"]

# Grant multiple permissions to a collaborator
[[tenant_access]]
tenant = "alice-dev"
permissions = ["tree:read", "tree:write", "branch:create", "snapshot:create"]

# Read-only access for an external reviewer
[[tenant_access]]
tenant = "external-reviewer"
permissions = ["tree:read", "snapshot:read"]
```

Simple grants are **syntactic sugar** — the server resolves them into full IAM policies
internally. They are equivalent to:

```toml
# What the server generates internally from [[tenant_access]]
[[policy]]
name = "tenant-grant-partner-corp"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]
scope = "worktree"
permissions = ["tree:read"]
```

#### Method 2: Full IAM Policies in `.wt/access/policies.toml`

For more complex access patterns, use full IAM policies:

```toml
# .wt/access/policies.toml

# Basic cross-tenant read access
[[policy]]
name = "partner-readonly"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]
scope = "worktree"
permissions = ["tree:read"]

# Cross-tenant access with branch restriction
[[policy]]
name = "contractor-staging-only"
effect = "allow"
subjects = [{ tenant = "contractor-dev" }]
scope = { branch = "staging" }
permissions = ["tree:read", "tree:write", "branch:read", "snapshot:create"]

# Cross-tenant access with path restriction (path must be registered)
[[policy]]
name = "partner-docs-only"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]
scope = { path = "docs/" }
permissions = ["tree:read", "tree:write"]

# Deny a specific tenant from a sensitive path
[[policy]]
name = "deny-contractor-secrets"
effect = "deny"
subjects = [{ tenant = "contractor-dev" }]
scope = { path = "config/secrets.toml" }
permissions = ["tree:read", "tree:write"]

# Grant by email for tenants you know by email
[[policy]]
name = "email-based-grant"
effect = "allow"
subjects = [{ tenant = "carol@partner.com" }]
scope = "worktree"
permissions = ["tree:read", "branch:read"]
```

### When to Use Which Method

| Scenario                          | Recommended Method      |
|-----------------------------------|-------------------------|
| Simple read/write access          | `[[tenant_access]]`     |
| Branch-scoped access              | Full IAM policy         |
| Path-scoped access                | Full IAM policy         |
| Deny rules                        | Full IAM policy         |
| Conditional access (ABAC)         | Full IAM policy         |
| Quick collaboration setup         | `[[tenant_access]]`     |

### Cross-Tenant Access Principles

1. **Owner always has full access** — cannot be restricted by any policy
2. **Default is deny** — no access unless explicitly granted
3. **Grants are worktree-scoped** — granting access to one worktree doesn't affect others
4. **Grants are version-controlled** — stored in `.wt/` files, tracked in snapshots
5. **Server enforces** — even if a client has the data locally, the server validates on sync

---

## Tenant Resolution

When the server encounters a tenant reference (in a `[[tenant_access]]` block or a
`{ tenant = "..." }` policy subject), it must resolve the reference to a concrete
TenantId.

### Resolution Algorithm

```
resolve_tenant(reference: String) -> Result<TenantId, Error>:
    1. Try to look up `reference` as a username (slug)
       - If found → return the tenant's TenantId
    2. If not found, try to look up `reference` as an email
       - If found → return the tenant's TenantId
    3. If not found → return Error("tenant '{reference}' not found on server")
```

### Resolution Details

- **Step 1: Slug lookup** — Exact match against the `slug` field. Case-insensitive
  (slugs are always stored lowercase).

- **Step 2: Email lookup** — Exact match against the `email` field. Case-insensitive
  for the domain part, case-sensitive for the local part (per RFC 5321, though in
  practice most providers treat it as case-insensitive).

- **Step 3: Error** — If neither lookup succeeds, the server returns an error during
  config sync. The worktree owner must fix the reference before the policy takes effect.

### Resolution Timing

- **On config sync**: When `.wt/config.toml` or `.wt/access/policies.toml` is synced
  to the server, tenant references are resolved and validated.
- **Resolved TenantId is stored**: The server stores the resolved TenantId alongside
  the policy for efficient access checks.
- **Re-resolution on tenant changes**: If a tenant changes their slug, the server
  re-resolves all policies referencing the old slug (using the stored TenantId as
  the source of truth).

### Error Handling

| Scenario                          | Behavior                                      |
|-----------------------------------|-----------------------------------------------|
| Tenant not found                  | Error on config sync; policy not applied       |
| Tenant suspended                  | Policy stored but not active; access denied    |
| Tenant deleted                    | Policy marked invalid; access denied           |
| Ambiguous reference               | Not possible — slug and email are both unique  |

---

## Worktree Visibility Modes

Every worktree has a visibility mode that determines default access for tenants
that are NOT explicitly granted access.

| Mode      | Default Access | Who Can Read                | Who Can Write                        |
|-----------|----------------|-----------------------------|--------------------------------------|
| `private` | None           | Owner + explicit grants     | Owner + explicit grants              |
| `shared`  | None           | Owner + listed tenants      | Owner + listed tenants with write    |
| `public`  | Read-all       | All authenticated tenants   | Owner + explicit grants              |

### Private Mode (Default)

- No access for anyone except the owner
- All access must be explicitly granted via `[[tenant_access]]` or IAM policies
- Most secure — suitable for proprietary code, internal projects
- The default visibility for all new worktrees

```toml
# .wt/config.toml
[worktree]
visibility = "private"
```

### Shared Mode

- No default access, but the worktree is "discoverable" by listed tenants
- Listed tenants can see the worktree in search results and their dashboard
- Access still requires explicit grants, but discovery is enabled
- Suitable for collaborative projects within a known group

```toml
# .wt/config.toml
[worktree]
visibility = "shared"

[[tenant_access]]
tenant = "team-lead"
permissions = ["tree:read", "tree:write", "branch:create"]

[[tenant_access]]
tenant = "qa-engineer"
permissions = ["tree:read", "branch:read"]
```

### Public Mode

- All authenticated tenants on the server can read the worktree
- Write access still requires explicit grants
- Suitable for open-source projects, public documentation, shared libraries
- Server may enforce rate limits on public reads

```toml
# .wt/config.toml
[worktree]
visibility = "public"

# Only explicit grants get write access
[[tenant_access]]
tenant = "trusted-contributor"
permissions = ["tree:write", "branch:create", "snapshot:create"]
```

### Visibility and IAM Interaction

- Visibility mode sets the **baseline** access level
- IAM policies can **add** permissions on top of the baseline
- IAM **deny** policies can **restrict** below the baseline (e.g., deny a specific
  tenant from reading a public worktree)
- The owner's access is **never** affected by visibility mode

---

## Organization Tenants

Organization tenants extend the basic tenant model with multi-user capabilities.

### Organization Structure

```
Organization Tenant (e.g., "acme-corp")
├── Owner Account
├── Admin Accounts
├── Member Accounts
└── Teams
    ├── Team "backend" → [alice, bob]
    ├── Team "frontend" → [carol, dave]
    └── Team "devops" → [eve]
```

### Accounts

An **account** is an individual user within an organization tenant.

| Property   | Type               | Description                          |
|------------|--------------------|--------------------------------------|
| `id`       | `AccountId` (UUID) | Unique account identifier            |
| `username` | `String`           | Account username within the org      |
| `email`    | `String`           | Account email                        |
| `role`     | `Role`             | Account's role within the org        |
| `status`   | `Active`/`Suspended`| Account status                      |

### Account Roles within an Organization

| Role        | Capabilities                                              |
|-------------|-----------------------------------------------------------|
| `owner`     | Full control — transfer ownership, delete org, manage all |
| `admin`     | Manage accounts, teams, worktrees, policies               |
| `member`    | Access worktrees based on team membership and policies     |

### Teams

Teams are groups of accounts used for group-based access control.

```toml
# .wt/access/policies.toml — Using teams in policies
[[policy]]
name = "backend-team-access"
effect = "allow"
subjects = [{ team = "backend" }]
scope = "worktree"
permissions = ["tree:read", "tree:write", "branch:create", "snapshot:create"]
```

### Inheritance

- Organization members **inherit** the org's cross-tenant grants
- If "acme-corp" is granted `tree:read` on a worktree, all members of "acme-corp"
  can read that worktree
- Team-based policies within the org provide finer-grained control
- Individual account policies override team policies (most specific wins)

### Organization Limits

| Plan         | Max Accounts | Max Trees | Max Storage |
|--------------|-------------|-----------|-------------|
| Free         | 5           | 10        | 1 GB        |
| Pro          | 50          | 100       | 50 GB       |
| Enterprise   | Unlimited   | Unlimited | Custom      |
| Custom       | Custom      | Custom    | Custom      |

---

## Tenant Data Model

### Rust Pseudocode

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Unique identifier for a tenant
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId(Uuid);

/// The type of tenant account
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantType {
    /// Individual developer account
    Personal,
    /// Team or company account with sub-accounts
    Organization,
}

/// Current status of the tenant
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantStatus {
    /// Active and fully operational
    Active,
    /// Suspended — read-only, no new operations
    Suspended,
}

/// Subscription plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Free,
    Pro,
    Enterprise,
    Custom(String),
}

/// Resource limits for the tenant
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_accounts: Option<u32>,   // None for personal tenants
    pub max_trees: u32,
    pub max_storage_bytes: u64,
    pub max_snapshot_size_bytes: u64,
    pub max_branches_per_tree: u32,
}

/// The core tenant struct
#[derive(Debug, Clone)]
pub struct Tenant {
    /// Internal unique identifier (UUID v7)
    pub id: TenantId,
    /// Human-readable display name
    pub name: String,
    /// URL-safe username (unique, lowercase)
    pub slug: String,
    /// Primary email address (unique)
    pub email: String,
    /// Personal or organization account
    pub tenant_type: TenantType,
    /// Current account status
    pub status: TenantStatus,
    /// Subscription plan
    pub plan: Plan,
    /// Resource limits
    pub limits: ResourceLimits,
    /// Custom attributes for ABAC conditions
    pub attributes: HashMap<String, String>,
    /// Account creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp
    pub updated_at: DateTime<Utc>,
}
```

### Serialization Format

Tenants are stored server-side. The wire format (for API responses) uses JSON:

```json
{
  "id": "01912345-6789-7abc-def0-123456789abc",
  "name": "Alice Developer",
  "slug": "alice-dev",
  "email": "alice@company.com",
  "type": "personal",
  "status": "active",
  "plan": "pro",
  "limits": {
    "max_trees": 100,
    "max_storage_bytes": 53687091200
  },
  "attributes": {
    "department": "engineering",
    "location": "us-west"
  },
  "created_at": "2025-01-15T10:30:00Z",
  "updated_at": "2025-01-15T10:30:00Z"
}
```

---

## Tenant Limits and Quotas

### Enforcement Points

Limits are enforced at several points in the protocol:

| Limit                | Enforcement Point                    | Behavior on Exceed                  |
|----------------------|--------------------------------------|-------------------------------------|
| `max_trees`          | Tree creation                        | Error: "tree limit reached"         |
| `max_storage_bytes`  | Snapshot sync (push)                 | Error: "storage quota exceeded"     |
| `max_accounts`       | Account creation (org only)          | Error: "account limit reached"      |
| `max_snapshot_size`  | Snapshot creation                    | Error: "snapshot too large"         |
| `max_branches`       | Branch creation                      | Error: "branch limit reached"       |

### Quota Tracking

- Storage usage is tracked per-tenant across all worktrees
- Snapshot deduplication is accounted for (shared chunks count once)
- Deleted worktrees free up quota after garbage collection
- Quota information is available via the server API

---

## Tenant Attributes for ABAC

Tenants can have arbitrary key-value attributes used in ABAC (Attribute-Based
Access Control) policy conditions.

### Built-in Attributes

| Attribute      | Description                    | Example Values          |
|----------------|--------------------------------|-------------------------|
| `department`   | Organizational department      | `"engineering"`, `"qa"` |
| `location`     | Geographic location            | `"us-west"`, `"eu"`     |
| `clearance`    | Security clearance level       | `"public"`, `"secret"`  |
| `team_size`    | Number of team members         | `"5"`, `"50"`           |

### Using Attributes in Policies

```toml
# .wt/access/policies.toml

# Only allow tenants in the EU to access GDPR-related data
[[policy]]
name = "gdpr-data-eu-only"
effect = "allow"
subjects = [{ all_authenticated = true }]
scope = { path = "data/gdpr/" }
permissions = ["tree:read"]

[[policy.conditions]]
attribute = "location"
operator = "StartsWith"
value = "eu"

# Deny access to secret data for tenants without clearance
[[policy]]
name = "deny-secret-without-clearance"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "config/secrets/" }
permissions = ["tree:read", "tree:write"]

[[policy.conditions]]
attribute = "clearance"
operator = "NotEquals"
value = "secret"
```

---

## Security Considerations

### Tenant Isolation

- Tenants are fully isolated by default (private visibility)
- Cross-tenant access requires explicit opt-in by the worktree owner
- Server enforces tenant boundaries on all operations
- No implicit access through tenant relationships (except org membership)

### Slug Squatting Prevention

- Reserved slugs list prevents impersonation of system accounts
- Slug changes are rate-limited and old slugs are reserved temporarily
- Deleted tenant slugs are reserved for 1 year

### Email Verification

- Tenants must verify their email before activation
- Email changes require re-verification
- Unverified tenants cannot be used as policy subjects

### Suspended Tenant Behavior

- Suspended tenants cannot perform any write operations
- Suspended tenants cannot authenticate (tokens are revoked)
- Policies referencing suspended tenants are paused (treated as if tenant doesn't exist)
- Worktrees owned by suspended tenants become read-only for all grantees

---

## Implementation Status

### Implemented

- `Tenant` struct with `id`, `name`, `slug`, `status`, `plan`, `limits`, `attributes`
- `TenantId` type (UUID-based)
- `TenantStatus` enum (`Active`, `Suspended`)
- `Plan` enum (`Free`, `Pro`, `Enterprise`, `Custom`)
- `ResourceLimits` struct with storage and tree limits
- ABAC attribute storage on tenant

### TODO

- [ ] `email` field on `Tenant` struct
- [ ] `TenantType` enum (`Personal`, `Organization`)
- [ ] `PolicySubject::Tenant` variant for cross-tenant policies
- [ ] Tenant resolution algorithm (slug → email → error)
- [ ] Cross-tenant access resolution in the access engine
- [ ] `[[tenant_access]]` config parsing and server-side expansion
- [ ] Worktree visibility modes (`private`, `shared`, `public`)
- [ ] Slug reservation on change/deletion

### Planned

- [ ] Organization tenant support (accounts, teams, inheritance)
- [ ] Team membership management
- [ ] Tenant billing and plan enforcement
- [ ] Tenant API (CRUD operations, quota queries)
- [ ] Email verification flow
- [ ] Tenant suspension and deletion workflows
- [ ] Audit logging for tenant operations

---

## References

- [IAM Specification](./IAM.md) — Full IAM system overview
- [Declarative Access](./DeclarativeAccess.md) — Declarative config files for access control
- [W0rkTree Protocol](../protocol/) — Core protocol specification