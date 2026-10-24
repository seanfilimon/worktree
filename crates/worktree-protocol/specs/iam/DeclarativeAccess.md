# Declarative Access Control Specification

> **Status**: Draft v0.1
> **Component**: worktree-protocol / IAM
> **Last Updated**: 2025-01

---

## Table of Contents

1. [Overview](#overview)
2. [Key Principle: Explicit Registration, No Globs](#key-principle-explicit-registration-no-globs)
3. [File Structure](#file-structure)
4. [Path Registration](#path-registration)
5. [Custom Roles](#custom-roles)
6. [Access Policies](#access-policies)
7. [Tree-Level Overrides](#tree-level-overrides)
8. [How It Works (Flow)](#how-it-works-flow)
9. [Validation Rules](#validation-rules)
10. [Scope Hierarchy](#scope-hierarchy)
11. [Simple Tenant Grants (Shorthand)](#simple-tenant-grants-shorthand)
12. [Examples](#examples)
13. [Error Handling](#error-handling)
14. [Implementation Status](#implementation-status)

---

## Overview

W0rkTree uses a **declarative, Terraform-style access control model**. Access rules are defined
as version-controlled TOML files stored in `.wt/access/` (root-level) and `.wt-tree/access/`
(tree-level). These files are synced, snapshotted, and versioned like any other worktree content,
but with special enforcement rules:

- Only users with `PolicyManage` or `TreeAdmin` permissions can modify access files.
- The server validates all access configuration on sync and rejects invalid policies.
- Policies take effect immediately upon successful sync.

Path-level access requires **explicit path registration** in `config.toml` — no glob patterns,
no wildcards, no regex. This is intentional and central to the design philosophy.

---

## Key Principle: Explicit Registration, No Globs

Every path you want to apply access control to **must be explicitly registered** in `config.toml`
before it can be referenced in a policy. This is the single most important design decision in
W0rkTree's declarative access model.

### How It Works

**Step 1**: Register the path in `config.toml`:

```toml
[[registered_path]]
path = "config/production.toml"
description = "Production configuration — restricted access"

[[registered_path]]
path = "secrets/api-keys.toml"
description = "API keys file — deny all except admins"

[[registered_path]]
path = "deploy/kubernetes/prod-manifest.yaml"
description = "Production Kubernetes manifest"
```

**Step 2**: Reference the registered path in `policies.toml`:

```toml
[[policy]]
name = "lock-production-config"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "config/production.toml" }
permissions = ["tree:write"]
```

If you reference a path in a policy that is **not** registered in `config.toml`, the validation
step will reject the policy with a clear error message.

### Why No Globs?

This is a deliberate design choice. Here is the rationale:

1. **Predictability** — You know exactly which paths are access-controlled. There is no
   ambiguity about whether a new file matches a pattern. You look at `config.toml` and see the
   complete list.

2. **Auditability** — Security auditors can read `config.toml` and `policies.toml` to get a
   complete picture. No hidden matches, no surprising expansions. The registered path list is
   the single source of truth.

3. **Performance** — Path matching is O(1) exact-match lookup against a hash set, not O(n)
   pattern matching against a list of globs. This matters when the access engine runs on every
   sync operation.

4. **Intentionality** — Like Terraform, you declare exactly what you manage. If you want a path
   controlled, you register it. This forces teams to think about what needs protection rather
   than writing overly broad patterns that catch unintended files.

5. **Simplicity** — No regex expertise required. No glob syntax to learn. No edge cases around
   `*` vs `**` vs `?`. The path is the path.

6. **Refactoring safety** — When files move, the registered path must be updated explicitly.
   This surfaces access control changes during code review rather than silently breaking or
   expanding access via glob match changes.

### What About Directories?

You can register a directory path to control access to all files within it:

```toml
[[registered_path]]
path = "config/production/"
description = "Entire production config directory"
```

A trailing `/` indicates a directory. All files within this directory (recursively) are covered
by policies that reference this path. This is the closest thing to a "wildcard" — but it is
still an explicit, registered path, not a pattern.

---

## File Structure

The declarative access configuration lives in two locations:

```
.wt/                                  ← Root-level (worktree-wide)
├── config.toml                       ← Path registration + tenant grants
└── access/
    ├── roles.toml                    ← Custom role definitions
    └── policies.toml                 ← Access policies

.wt-tree/                             ← Tree-level (per-tree overrides)
├── config.toml                       ← Tree-scoped path registration
└── access/
    └── policies.toml                 ← Tree-level policies (restrict only)
```

### File Responsibilities

| File | Purpose | Who Can Edit |
|---|---|---|
| `.wt/config.toml` | Register paths, tenant grants, worktree settings | Owner, Admin |
| `.wt/access/roles.toml` | Define custom roles beyond the 5 built-ins | Owner, Admin |
| `.wt/access/policies.toml` | Root-level access policies | PolicyManage, TreeAdmin |
| `.wt-tree/config.toml` | Tree-scoped path registration | PolicyManage, TreeAdmin |
| `.wt-tree/access/policies.toml` | Tree-level restriction policies | PolicyManage, TreeAdmin |

---

## Path Registration

### .wt/config.toml — Root-Level Path Registration

Paths registered at the root level apply across the entire worktree.

```toml
# ============================================================
# W0rkTree Configuration
# ============================================================

[worktree]
name = "my-project"
visibility = "private"   # private | shared | public

# ============================================================
# Registered Paths
# ============================================================
# Every path that needs access control must be listed here.
# No globs. No wildcards. Exact paths only.
# Trailing "/" denotes a directory (recursive).
# ============================================================

[[registered_path]]
path = "config/production.toml"
description = "Production environment configuration"

[[registered_path]]
path = "config/staging.toml"
description = "Staging environment configuration"

[[registered_path]]
path = "secrets/"
description = "All secret files — directory-level restriction"

[[registered_path]]
path = "deploy/kubernetes/prod-manifest.yaml"
description = "Production Kubernetes deployment manifest"

[[registered_path]]
path = "deploy/terraform/"
description = "All Terraform infrastructure files"

[[registered_path]]
path = "docs/internal/roadmap.md"
description = "Internal product roadmap — restricted visibility"

[[registered_path]]
path = ".wt/access/policies.toml"
description = "The access policies file itself — meta-protection"

[[registered_path]]
path = "src/auth/crypto.rs"
description = "Cryptographic implementation — security-sensitive"
```

### Path Registration Rules

1. Paths are relative to the worktree root.
2. Paths must not start with `/` or `./`.
3. Paths must not contain `..` (no parent traversal).
4. Trailing `/` indicates directory scope (all children recursively).
5. Paths are case-sensitive on case-sensitive filesystems.
6. A registered path does not need to exist yet — you can register paths for files that will
   be created in the future.
7. Duplicate path registrations are rejected during validation.

---

## Custom Roles

### .wt/access/roles.toml — Custom Role Definitions

W0rkTree ships with 5 built-in roles: Owner, Admin, Maintainer, Developer, Viewer.
You can define additional custom roles for finer-grained access control.

```toml
# ============================================================
# Custom Role Definitions
# ============================================================
# Custom roles extend the built-in set.
# They do NOT replace built-in roles.
# Permissions must be valid atomic permission names.
# ============================================================

[[role]]
name = "security-reviewer"
description = "Can read all code and approve security-sensitive changes"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:read",
    "merge_request:approve",
]

[[role]]
name = "release-manager"
description = "Can create releases and tags, merge release branches"
permissions = [
    "tree:read",
    "branch:read",
    "branch:merge",
    "snapshot:read",
    "snapshot:create",
    "tag:create",
    "tag:delete",
    "release:create",
    "release:delete",
    "sync:push",
    "sync:pull",
]

[[role]]
name = "ci-bot"
description = "Automated CI/CD bot — push to specific branches, create snapshots"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull",
]

[[role]]
name = "auditor"
description = "Read-only access to everything including policies"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:read",
    "sync:pull",
]

[[role]]
name = "intern"
description = "Limited write access — can push but not merge or create branches"
permissions = [
    "tree:read",
    "tree:write",
    "branch:read",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull",
]
```

### Custom Role Rules

1. Role names must be lowercase alphanumeric with hyphens (e.g., `security-reviewer`).
2. Role names must not conflict with built-in role names (case-insensitive).
3. Permissions listed must be valid atomic permission names.
4. Custom roles cannot grant permissions beyond what the policy assigner has.
5. Custom roles are referenced by name in policies, just like built-in roles.

---

## Access Policies

### .wt/access/policies.toml — Root-Level Policies

This is where the bulk of access control is defined. Policies bind subjects to permissions
at specific scopes.

```toml
# ============================================================
# Root-Level Access Policies
# ============================================================

# --------------------------------------------------
# 1. Worktree-wide team access
# --------------------------------------------------
[[policy]]
name = "dev-team-standard-access"
effect = "allow"
subjects = [{ team = "developers" }]
scope = "worktree"
permissions = [
    "tree:read",
    "tree:write",
    "branch:read",
    "branch:create",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull",
]

[[policy]]
name = "maintainers-full-branch"
effect = "allow"
subjects = [{ team = "maintainers" }]
scope = "worktree"
permissions = [
    "tree:read",
    "tree:write",
    "branch:read",
    "branch:create",
    "branch:delete",
    "branch:merge",
    "branch:protect",
    "snapshot:create",
    "snapshot:read",
    "snapshot:restore",
    "sync:push",
    "sync:pull",
    "tag:create",
    "release:create",
]

# --------------------------------------------------
# 2. Cross-tenant access (by username)
# --------------------------------------------------
[[policy]]
name = "partner-readonly"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]
scope = "worktree"
permissions = ["tree:read", "branch:read", "snapshot:read", "sync:pull"]

# --------------------------------------------------
# 3. Cross-tenant access (by email)
# --------------------------------------------------
[[policy]]
name = "contractor-alice-write"
effect = "allow"
subjects = [{ tenant = "alice@contractor.io" }]
scope = "worktree"
permissions = [
    "tree:read",
    "tree:write",
    "branch:read",
    "branch:create",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull",
]

# --------------------------------------------------
# 4. Branch-specific access
# --------------------------------------------------
[[policy]]
name = "protect-main-branch"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { branch = "main" }
permissions = ["sync:push", "branch:delete"]

[[policy]]
name = "allow-main-merge-maintainers"
effect = "allow"
subjects = [{ team = "maintainers" }, { role = "Admin" }]
scope = { branch = "main" }
permissions = ["branch:merge", "sync:push"]

[[policy]]
name = "release-branch-lock"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { branch = "release/*" }
permissions = ["sync:push"]

[[policy]]
name = "release-manager-push"
effect = "allow"
subjects = [{ role = "release-manager" }]
scope = { branch = "release/*" }
permissions = ["sync:push", "tag:create", "release:create"]

# --------------------------------------------------
# 5. Path-level access control
# --------------------------------------------------
[[policy]]
name = "deny-secrets-all"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "secrets/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "allow-secrets-admins"
effect = "allow"
subjects = [{ role = "Admin" }, { role = "Owner" }]
scope = { path = "secrets/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "lock-production-config"
effect = "deny"
subjects = [{ team = "developers" }, { role = "intern" }]
scope = { path = "config/production.toml" }
permissions = ["tree:write"]

[[policy]]
name = "crypto-review-required"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "src/auth/crypto.rs" }
permissions = ["tree:write"]

[[policy]]
name = "crypto-security-team"
effect = "allow"
subjects = [{ team = "security" }, { role = "security-reviewer" }]
scope = { path = "src/auth/crypto.rs" }
permissions = ["tree:read", "tree:write"]

# --------------------------------------------------
# 6. Deny overrides
# --------------------------------------------------
[[policy]]
name = "deny-intern-merge"
effect = "deny"
subjects = [{ role = "intern" }]
scope = "worktree"
permissions = ["branch:merge", "branch:delete", "branch:protect"]

[[policy]]
name = "deny-terraform-non-infra"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "deploy/terraform/" }
permissions = ["tree:write"]

[[policy]]
name = "allow-terraform-infra-team"
effect = "allow"
subjects = [{ team = "infrastructure" }]
scope = { path = "deploy/terraform/" }
permissions = ["tree:read", "tree:write"]

# --------------------------------------------------
# 7. Role-based combinations
# --------------------------------------------------
[[policy]]
name = "ci-bot-access"
effect = "allow"
subjects = [{ account = "ci-bot-01" }, { account = "ci-bot-02" }]
scope = "worktree"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull",
]

[[policy]]
name = "ci-bot-branch-restriction"
effect = "deny"
subjects = [{ account = "ci-bot-01" }, { account = "ci-bot-02" }]
scope = { branch = "main" }
permissions = ["sync:push"]

[[policy]]
name = "auditor-read-all"
effect = "allow"
subjects = [{ role = "auditor" }]
scope = "worktree"
permissions = ["tree:read", "branch:read", "snapshot:read", "sync:pull"]
```

---

## Tree-Level Overrides

### .wt-tree/config.toml — Tree-Scoped Path Registration

Trees can register additional paths for tree-level access control.

```toml
# ============================================================
# Tree-Level Configuration
# ============================================================

[tree]
name = "frontend"

# Tree-scoped path registrations
[[registered_path]]
path = "src/components/payment/"
description = "Payment UI components — PCI compliance restricted"

[[registered_path]]
path = "src/config/feature-flags.json"
description = "Feature flag configuration"

[[registered_path]]
path = "tests/e2e/payment/"
description = "Payment E2E tests — contain test credentials"
```

### .wt-tree/access/policies.toml — Tree-Level Policies

Tree-level policies can only **restrict** access granted by root-level policies. They
**cannot expand** access beyond what the root level allows.

```toml
# ============================================================
# Tree-Level Policies (Restriction Only)
# ============================================================

# Restrict payment components to the payments team
[[policy]]
name = "payment-components-lock"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "src/components/payment/" }
permissions = ["tree:write"]

[[policy]]
name = "payment-team-write"
effect = "allow"
subjects = [{ team = "payments" }]
scope = { path = "src/components/payment/" }
permissions = ["tree:read", "tree:write"]

# Lock feature flags to leads only
[[policy]]
name = "feature-flags-restrict"
effect = "deny"
subjects = [{ team = "developers" }]
scope = { path = "src/config/feature-flags.json" }
permissions = ["tree:write"]

[[policy]]
name = "feature-flags-leads"
effect = "allow"
subjects = [{ team = "tech-leads" }]
scope = { path = "src/config/feature-flags.json" }
permissions = ["tree:read", "tree:write"]
```

### Tree Override Rules

The server evaluates **both** root-level and tree-level policies and takes the most
restrictive combination:

1. If root-level **allows** `tree:read` + `tree:write` for `team-A`, and the tree restricts
   to `tree:read` only → result is `tree:read` only.

2. If root-level **denies** `tree:write` for `team-B`, and the tree tries to allow
   `tree:write` for `team-B` → result is still **deny**. The tree cannot override a root deny.

3. If root-level grants no permissions for a user, the tree-level cannot grant any either.
   Tree policies only operate within the permission space already granted at the root level.

4. Tree-level policies are evaluated after root-level. The final result is the **intersection**
   of what root allows and what tree allows, minus anything either level denies.

---

## How It Works (Flow)

The end-to-end flow for declarative access changes:

### Step 1: Edit Access Files

A user with `PolicyManage` or `TreeAdmin` permission edits the access configuration files
locally. This uses the same editing workflow as any other worktree file.

```
# Edit root policies
$EDITOR .wt/access/policies.toml

# Or edit tree-level policies
$EDITOR .wt-tree/access/policies.toml

# Or register new paths
$EDITOR .wt/config.toml
```

### Step 2: Path Registration Check

For any policy that uses a `{ path = "..." }` scope, the referenced path **must** be
registered in the appropriate `config.toml`:

- Root policies (`.wt/access/policies.toml`) → paths registered in `.wt/config.toml`
- Tree policies (`.wt-tree/access/policies.toml`) → paths registered in `.wt-tree/config.toml`
  **or** `.wt/config.toml` (tree inherits root registrations)

### Step 3: BGProcess Detects Change

The background process monitors `.wt/` and `.wt-tree/` directories for changes to access
configuration files. When a change is detected:

- The file modification is queued for validation.
- The BGProcess begins local pre-validation before any sync attempt.

### Step 4: Local Validation

BGProcess performs client-side validation:

1. **Syntax check** — Is the TOML valid?
2. **Schema check** — Do all required fields exist? Are types correct?
3. **Role reference check** — Do referenced roles exist (built-in or in `roles.toml`)?
4. **Permission name check** — Are all permission names valid atomic permissions?
5. **Registered path check** — Do all `{ path = "..." }` scopes reference registered paths?
6. **Duplicate check** — Are there duplicate policy names?

If validation fails, the BGProcess logs the error and notifies the user. The invalid
configuration is **not** synced.

### Step 5: Sync to Server

On successful local validation, BGProcess syncs the access files to the server as part of
the normal sync operation. Access file changes are included in the sync payload alongside
any other file changes.

### Step 6: Server Validation

The server performs additional validation that requires server-side context:

1. **Tenant resolution** — Do all `{ tenant = "..." }` subjects resolve to real tenants?
2. **Path registration consistency** — Cross-check registered paths across root and trees.
3. **Policy consistency** — Are there logical contradictions?
4. **Permission escalation check** — Is the editing user attempting to grant permissions
   they don't have?
5. **Tree override validation** — Do tree-level policies only restrict (not expand)?

If server validation fails, the sync is rejected with a descriptive error.

### Step 7: Policy Application

On successful server validation:

1. Server stores the updated policy configuration.
2. Server recomputes the effective access matrix.
3. New policies take effect **immediately** for all subsequent access checks.
4. Server logs the policy change for audit purposes.

### Step 8: Client Sync

Other BGProcess clients connected to the same worktree receive the updated access
configuration on their next sync cycle. They update their local copies of the access files.

---

## Validation Rules

### Syntax and Schema

| Rule | Error If Violated |
|---|---|
| TOML must parse without errors | `E1001: Invalid TOML syntax at line X` |
| Policy must have `name` field | `E1002: Policy missing required field 'name'` |
| Policy must have `effect` field | `E1003: Policy missing required field 'effect'` |
| Effect must be "allow" or "deny" | `E1004: Invalid effect 'X', must be 'allow' or 'deny'` |
| Policy must have `subjects` array | `E1005: Policy missing required field 'subjects'` |
| Subjects must have at least one entry | `E1006: Policy subjects cannot be empty` |
| Policy must have `scope` field | `E1007: Policy missing required field 'scope'` |
| Policy must have `permissions` array | `E1008: Policy missing required field 'permissions'` |
| Permissions must have at least one entry | `E1009: Policy permissions cannot be empty` |

### Referential Integrity

| Rule | Error If Violated |
|---|---|
| Role refs must exist (built-in or custom) | `E2001: Unknown role 'X' in policy 'Y'` |
| Permission names must be valid | `E2002: Unknown permission 'X' in policy 'Y'` |
| Path scopes must reference registered paths | `E2003: Path 'X' not registered in config.toml` |
| Tenant subjects must resolve (server-side) | `E2004: Tenant 'X' not found on server` |
| Account subjects must resolve (server-side) | `E2005: Account 'X' not found` |
| Team subjects must resolve (server-side) | `E2006: Team 'X' not found` |

### Logical Consistency

| Rule | Error If Violated |
|---|---|
| No duplicate policy names in same file | `E3001: Duplicate policy name 'X'` |
| No duplicate path registrations | `E3002: Duplicate registered path 'X'` |
| No duplicate custom role names | `E3003: Duplicate role name 'X'` |
| Custom role names must not shadow built-ins | `E3004: Role 'X' conflicts with built-in role` |
| No circular role dependencies | `E3005: Circular dependency detected in role 'X'` |

### Security Rules

| Rule | Error If Violated |
|---|---|
| Editor must have PolicyManage or TreeAdmin | `E4001: Insufficient permission to modify access config` |
| Cannot grant permissions you don't have | `E4002: Permission escalation — cannot grant 'X'` |
| Tree policies cannot expand root access | `E4003: Tree policy 'X' attempts to expand root access` |

---

## Scope Hierarchy

The scope hierarchy determines policy precedence from most specific to least specific:

```
RegisteredPath    ← Most specific (exact file or directory)
    ↑
  Branch          ← Specific branch
    ↑
   Tree           ← Entire tree
    ↑
  Tenant          ← All trees in a tenant
    ↑
  Global          ← Server-wide (superadmin only)
```

### Resolution Order

1. **Most specific scope wins.** A policy at `RegisteredPath` scope takes precedence over
   a policy at `Branch` scope for the same permission.

2. **Deny beats Allow at the same level.** If there is both an Allow and a Deny at the
   `RegisteredPath` level for the same permission, the result is **Deny**.

3. **Fall through to broader scope.** If no policy matches at the most specific level,
   the engine checks the next broader scope.

4. **Default is Deny.** If no policy matches at any scope level, access is denied.

### Interaction Between .wt/ and .wt-tree/

- `.wt/` policies define the **ceiling** — the maximum possible access.
- `.wt-tree/` policies define **restrictions within that ceiling**.
- The effective permission set is the intersection, minus any denies from either level.

---

## Simple Tenant Grants (Shorthand)

For common cross-tenant access scenarios, W0rkTree provides a shorthand syntax in
`.wt/config.toml` that avoids the need for full policy definitions:

```toml
# ============================================================
# Simple Tenant Grants
# ============================================================
# Shorthand for granting other tenants access to this worktree.
# These are syntactic sugar — the server resolves them into
# full IAM policies internally.
# ============================================================

# Grant by username
[[tenant_access]]
tenant = "alice-dev"
permissions = ["tree:read", "branch:read", "sync:pull"]

# Grant by email
[[tenant_access]]
tenant = "bob@company.com"
permissions = ["tree:read", "tree:write", "branch:read", "branch:create", "sync:push", "sync:pull"]

# Grant an entire org read access
[[tenant_access]]
tenant = "partner-corp"
permissions = ["tree:read", "branch:read", "snapshot:read", "sync:pull"]

# Multiple grants for the same tenant at different scopes are NOT supported
# in the shorthand — use full policies in .wt/access/policies.toml instead.
```

### Shorthand Expansion

The server internally expands each `[[tenant_access]]` entry into a full IAM policy:

```toml
# This shorthand:
[[tenant_access]]
tenant = "alice-dev"
permissions = ["tree:read", "branch:read", "sync:pull"]

# Becomes internally:
[[policy]]
name = "__auto_tenant_grant_alice-dev"
effect = "allow"
subjects = [{ tenant = "alice-dev" }]
scope = "worktree"
permissions = ["tree:read", "branch:read", "sync:pull"]
```

Auto-generated policies from shorthand:
- Have names prefixed with `__auto_tenant_grant_`.
- Are always `effect = "allow"`.
- Are always scoped to `"worktree"`.
- Cannot be overridden by tree-level policies (they are root-level).
- Can be overridden by explicit deny policies in `.wt/access/policies.toml`.

---

## Examples

### Example 1: Open Source Project with Protected Releases

```toml
# .wt/config.toml
[worktree]
name = "my-oss-project"
visibility = "public"

[[registered_path]]
path = "CHANGELOG.md"
description = "Changelog — maintainers only"

# .wt/access/policies.toml
[[policy]]
name = "public-read"
effect = "allow"
subjects = [{ all_authenticated = true }]
scope = "worktree"
permissions = ["tree:read", "branch:read", "snapshot:read", "sync:pull"]

[[policy]]
name = "protect-changelog"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "CHANGELOG.md" }
permissions = ["tree:write"]

[[policy]]
name = "maintainers-changelog"
effect = "allow"
subjects = [{ team = "maintainers" }]
scope = { path = "CHANGELOG.md" }
permissions = ["tree:read", "tree:write"]
```

### Example 2: Enterprise Monorepo with Team Boundaries

```toml
# .wt/config.toml
[worktree]
name = "acme-monorepo"
visibility = "private"

[[registered_path]]
path = "services/billing/"
description = "Billing service — PCI restricted"

[[registered_path]]
path = "services/auth/"
description = "Auth service — security team only"

[[registered_path]]
path = "infrastructure/"
description = "Infrastructure config — SRE only"

# .wt/access/policies.toml
[[policy]]
name = "all-devs-base"
effect = "allow"
subjects = [{ team = "engineering" }]
scope = "worktree"
permissions = ["tree:read", "tree:write", "branch:read", "branch:create", "snapshot:create", "snapshot:read", "sync:push", "sync:pull"]

[[policy]]
name = "billing-deny-all"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "services/billing/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "billing-team-access"
effect = "allow"
subjects = [{ team = "billing" }]
scope = { path = "services/billing/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "auth-deny-all"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "services/auth/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "auth-security-team"
effect = "allow"
subjects = [{ team = "security" }]
scope = { path = "services/auth/" }
permissions = ["tree:read", "tree:write"]

[[policy]]
name = "infra-deny-all"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "infrastructure/" }
permissions = ["tree:write"]

[[policy]]
name = "infra-sre-access"
effect = "allow"
subjects = [{ team = "sre" }]
scope = { path = "infrastructure/" }
permissions = ["tree:read", "tree:write"]
```

---

## Error Handling

### Client-Side Errors

When the BGProcess detects an invalid access configuration locally, it:

1. Logs the validation error(s) with specific error codes and messages.
2. Does **not** sync the invalid configuration.
3. Keeps the previous valid configuration in effect.
4. Notifies the user via the standard notification channel.

### Server-Side Errors

When the server rejects an access configuration on sync:

1. Returns a structured error response with error codes and messages.
2. The BGProcess reports the rejection to the user.
3. The previous valid configuration remains in effect on the server.
4. The user must fix the issue and re-sync.

### Recovery

If access configuration becomes corrupted or causes lockout:

1. **Owner always has access** — the Owner role cannot be restricted by policies.
2. **Server admin override** — server administrators can reset access configuration.
3. **Snapshot restore** — since access files are versioned, restore a previous snapshot.
4. **Manual file edit** — the Owner can edit `.wt/access/` files to fix the issue.

---

## Implementation Status

### Implemented

- **AccessEngine** — 871-line policy evaluation engine with RBAC + ABAC support
- **Scope matching** — Global, Tenant, Tree, Branch scope evaluation
- **Built-in roles** — Owner, Admin, Maintainer, Developer, Viewer
- **20+ atomic permissions** — Full permission set across all categories
- **Policy conditions** — ABAC operators (Equals, NotEquals, Contains, etc.)
- **Policy model** — Effect, subjects, scope, permissions structure

### TODO (Next Phase)

- **RegisteredPath scope** — `Scope::RegisteredPath` variant in the scope enum
- **Config parsing** — TOML deserialization for `config.toml`, `roles.toml`, `policies.toml`
- **Tenant resolution** — Server-side resolution of tenant usernames and emails
- **Local validation** — BGProcess pre-sync validation pipeline
- **Path registration enforcement** — Validate path scopes against registered paths

### Planned (Future)

- **Declarative config sync** — Full end-to-end flow from edit to enforcement
- **Server-side validation** — Comprehensive server validation including escalation checks
- **Audit logging** — Policy change tracking and access audit trail
- **Policy diff** — Show what changed between policy versions
- **Policy dry-run** — Test a policy change before applying it
- **License compliance integration** — Tie access policies to license requirements