# W0rkTree IAM Specification

## Overview

W0rkTree has a native, built-in IAM (Identity and Access Management) system. Unlike Git, where access control is bolted on by hosting platforms (GitHub, GitLab, Bitbucket), W0rkTree's IAM is a **first-class protocol concept** enforced at the server level.

This means:
- Access control is part of the protocol, not an afterthought
- Policies are version-controlled alongside your code
- Permissions are enforced on every sync operation by the server
- Cross-tenant access is a native concept, not a platform-specific feature
- The same IAM model works regardless of which W0rkTree server you connect to

## Components

The IAM system is composed of six core components:

1. **Tenants** — Users or organizations with a username + email. The identity unit on the server. (See [TenantModel.md](./TenantModel.md))
2. **Accounts** — Individual user accounts within an organization tenant
3. **Teams** — Named groups of accounts for collective policy assignment
4. **Roles** — Named permission sets (built-in or custom-defined)
5. **Policies** — RBAC + ABAC rules binding subjects → permissions at scopes
6. **Scopes** — The hierarchy of resources that permissions apply to: Global → Tenant → Tree → Branch → RegisteredPath

These components work together to provide fine-grained, auditable access control over every operation in W0rkTree.

---

## Built-in Roles

W0rkTree ships with five built-in roles that form a strict superset hierarchy:

| Role       | Permissions                                      | Description                                  |
|------------|--------------------------------------------------|----------------------------------------------|
| Owner      | All permissions                                  | Full control. Cannot be restricted.          |
| Admin      | All except owner-transfer                        | Manages tenants, access, config              |
| Maintainer | Branch merge, tree admin, policy management      | Manages branches and deployments             |
| Developer  | Read, write, snapshot, branch create             | Standard development work                    |
| Viewer     | Read-only across all scopes                      | Can see but not modify                       |

### Superset Hierarchy

```
Owner ⊃ Admin ⊃ Maintainer ⊃ Developer ⊃ Viewer
```

This means:
- Every permission a Viewer has, a Developer also has
- Every permission a Developer has, a Maintainer also has
- Every permission a Maintainer has, an Admin also has
- Every permission an Admin has, an Owner also has (plus owner-transfer)

### Role Assignment

Roles can be assigned at any scope level:
- A user can be a **Developer** at the Tenant scope (all trees)
- But a **Maintainer** for a specific tree
- And **Viewer** only on a specific branch of another tree

The most specific role assignment wins when evaluating permissions.

---

## Permission Set

W0rkTree defines a comprehensive set of atomic permissions organized by category. Each permission represents a single, indivisible operation that can be allowed or denied.

### Tree Permissions

| Permission    | Description                                           |
|---------------|-------------------------------------------------------|
| `tree:read`   | Read tree contents (files, directories, metadata)     |
| `tree:write`  | Write to tree contents (create, modify, delete files) |
| `tree:delete` | Delete an entire tree                                 |
| `tree:admin`  | Manage tree settings, configuration, and metadata     |

### Branch Permissions

| Permission       | Description                                        |
|------------------|----------------------------------------------------|
| `branch:read`    | Read branch contents and metadata                  |
| `branch:create`  | Create new branches                                |
| `branch:delete`  | Delete branches                                    |
| `branch:merge`   | Merge branches together                            |
| `branch:protect` | Modify branch protection rules                     |

### Snapshot Permissions

| Permission         | Description                                      |
|--------------------|--------------------------------------------------|
| `snapshot:create`  | Create new snapshots (commits)                   |
| `snapshot:read`    | Read snapshot history and contents               |
| `snapshot:restore` | Restore a previous snapshot                      |

### Sync Permissions

| Permission   | Description                                           |
|--------------|-------------------------------------------------------|
| `sync:push`  | Push local changes to the server                      |
| `sync:pull`  | Pull remote changes from the server                   |

### Management Permissions

| Permission      | Description                                        |
|-----------------|----------------------------------------------------|
| `PolicyManage`  | Create, modify, delete access policies             |
| `RoleManage`    | Create, modify, delete custom roles                |
| `TeamManage`    | Create, modify, delete teams and team membership   |
| `AccountManage` | Create, modify, delete accounts within a tenant    |

### Admin Permissions

| Permission    | Description                                          |
|---------------|------------------------------------------------------|
| `TreeAdmin`   | Full administrative control over a tree              |
| `TenantAdmin` | Full administrative control over a tenant            |

### Tag & Release Permissions

| Permission       | Description                                       |
|------------------|---------------------------------------------------|
| `tag:create`     | Create tags on snapshots                          |
| `tag:delete`     | Delete existing tags                              |
| `release:create` | Create releases from tags or snapshots            |
| `release:delete` | Delete existing releases                          |

### Merge Request Permissions

| Permission              | Description                                |
|-------------------------|--------------------------------------------|
| `merge_request:create`  | Open a new merge request                   |
| `merge_request:approve` | Approve a merge request                    |
| `merge_request:merge`   | Execute the merge of an approved request   |

### Role → Permission Mapping

| Permission                | Owner | Admin | Maintainer | Developer | Viewer |
|---------------------------|:-----:|:-----:|:----------:|:---------:|:------:|
| `tree:read`               |   ✓   |   ✓   |     ✓      |     ✓     |   ✓    |
| `tree:write`              |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `tree:delete`             |   ✓   |   ✓   |            |           |        |
| `tree:admin`              |   ✓   |   ✓   |     ✓      |           |        |
| `branch:read`             |   ✓   |   ✓   |     ✓      |     ✓     |   ✓    |
| `branch:create`           |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `branch:delete`           |   ✓   |   ✓   |     ✓      |           |        |
| `branch:merge`            |   ✓   |   ✓   |     ✓      |           |        |
| `branch:protect`          |   ✓   |   ✓   |     ✓      |           |        |
| `snapshot:create`         |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `snapshot:read`           |   ✓   |   ✓   |     ✓      |     ✓     |   ✓    |
| `snapshot:restore`        |   ✓   |   ✓   |     ✓      |           |        |
| `sync:push`               |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `sync:pull`               |   ✓   |   ✓   |     ✓      |     ✓     |   ✓    |
| `PolicyManage`            |   ✓   |   ✓   |     ✓      |           |        |
| `RoleManage`              |   ✓   |   ✓   |            |           |        |
| `TeamManage`              |   ✓   |   ✓   |            |           |        |
| `AccountManage`           |   ✓   |   ✓   |            |           |        |
| `TreeAdmin`               |   ✓   |   ✓   |     ✓      |           |        |
| `TenantAdmin`             |   ✓   |   ✓   |            |           |        |
| `tag:create`              |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `tag:delete`              |   ✓   |   ✓   |     ✓      |           |        |
| `release:create`          |   ✓   |   ✓   |     ✓      |           |        |
| `release:delete`          |   ✓   |   ✓   |     ✓      |           |        |
| `merge_request:create`    |   ✓   |   ✓   |     ✓      |     ✓     |        |
| `merge_request:approve`   |   ✓   |   ✓   |     ✓      |           |        |
| `merge_request:merge`     |   ✓   |   ✓   |     ✓      |           |        |

---

## Scope Hierarchy

Scopes define **where** a permission applies. They form a strict hierarchy from broadest to most specific:

```
Global                           ← Server-wide (superadmin only)
  └── Tenant                     ← All trees owned by a tenant
       └── Tree                  ← An entire worktree
            └── Branch           ← A specific branch within a tree
                 └── RegisteredPath  ← A specific file/directory (explicit registration in config.toml)
```

### Scope Properties

| Scope          | Identifier             | Who Can Set             | Typical Use Case                     |
|----------------|------------------------|-------------------------|--------------------------------------|
| Global         | (implicit)             | Server superadmin       | Platform-wide policies               |
| Tenant         | Tenant slug or email   | Tenant owner/admin      | Org-wide defaults                    |
| Tree           | Tree name/ID           | Tree owner/admin        | Per-project access                   |
| Branch         | Branch name            | Tree maintainer+        | Protected branches                   |
| RegisteredPath | File/directory path    | Tree maintainer+        | Sensitive file protection            |

### Scope Resolution Rules

1. **Most specific wins**: A policy at the RegisteredPath scope overrides a policy at the Branch scope
2. **Deny beats Allow**: At the same scope level, Deny always wins over Allow
3. **Inheritance**: If no policy exists at a specific scope, the parent scope's policy applies
4. **Default Deny**: If no policy matches at any scope level, the action is denied

---

## Policy Model

Policies are the core mechanism for binding subjects to permissions at scopes. They are defined as TOML files stored in version-controlled directories.

### Policy Storage Locations

| Location                          | Scope              | Description                          |
|-----------------------------------|--------------------|-----------------------------------------|
| `.wt/access/policies.toml`       | Worktree-wide      | Root-level policies for the worktree    |
| `.wt-tree/access/policies.toml`  | Tree-specific      | Policies scoped to a specific tree      |

### Policy Structure

```toml
[[policy]]
name = "policy-name"
effect = "allow"          # "allow" or "deny"
subjects = [...]          # Who this policy applies to
scope = "..."             # Where this policy applies
permissions = [...]       # What actions are allowed/denied
conditions = [...]        # Optional: ABAC conditions
```

### Subject Types

Subjects define **who** a policy applies to:

```toml
# A specific account
subjects = [{ account = "alice" }]

# A team
subjects = [{ team = "backend-team" }]

# A built-in or custom role
subjects = [{ role = "Developer" }]

# A tenant (by username)
subjects = [{ tenant = "partner-corp" }]

# A tenant (by email)
subjects = [{ tenant = "alice@company.com" }]

# All authenticated users
subjects = [{ all_authenticated = true }]

# Multiple subjects (OR — any match triggers)
subjects = [
    { team = "backend-team" },
    { account = "bob" },
    { tenant = "partner-corp" }
]
```

### Scope Types

Scopes define **where** a policy applies:

```toml
# Entire worktree
scope = "worktree"

# Entire tree
scope = "tree"

# Specific branch
scope = { branch = "main" }

# Specific registered path
scope = { path = "config/production.toml" }
```

### Comprehensive Policy Examples

#### Example 1: Team-wide Development Access

```toml
[[policy]]
name = "backend-team-dev-access"
effect = "allow"
subjects = [{ team = "backend-team" }]
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
    "merge_request:create"
]
```

#### Example 2: Cross-Tenant Read Access (by username)

```toml
[[policy]]
name = "partner-readonly"
effect = "allow"
subjects = [{ tenant = "partner-corp" }]
scope = "worktree"
permissions = ["tree:read", "branch:read", "snapshot:read", "sync:pull"]
```

#### Example 3: Cross-Tenant Write Access (by email)

```toml
[[policy]]
name = "contractor-write-access"
effect = "allow"
subjects = [{ tenant = "bob@contractor.io" }]
scope = "worktree"
permissions = [
    "tree:read",
    "tree:write",
    "branch:read",
    "branch:create",
    "snapshot:create",
    "snapshot:read",
    "sync:push",
    "sync:pull"
]
```

#### Example 4: Path-Level Restriction (Lock Production Config)

First, register the path in `.wt/config.toml`:
```toml
[[registered_path]]
path = "config/production.toml"
description = "Production configuration — restricted access"
```

Then deny writes in `.wt/access/policies.toml`:
```toml
[[policy]]
name = "lock-production-config"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { path = "config/production.toml" }
permissions = ["tree:write"]
```

And allow only specific maintainers:
```toml
[[policy]]
name = "allow-infra-team-production"
effect = "allow"
subjects = [{ team = "infra-team" }]
scope = { path = "config/production.toml" }
permissions = ["tree:read", "tree:write"]
```

Since Deny and Allow are at the same scope level here, the Deny would normally win. However, the infra-team members are matched by both policies. The resolution is: the **more specific subject** (team) is evaluated alongside the **broader subject** (all_authenticated). Deny on `all_authenticated` applies to everyone, but the explicit Allow for `infra-team` at the same scope level means the engine must resolve the conflict — and in this case, **Deny wins**. To make this work correctly, the deny should target a narrower subject:

```toml
[[policy]]
name = "lock-production-config"
effect = "deny"
subjects = [{ role = "Developer" }, { role = "Viewer" }]
scope = { path = "config/production.toml" }
permissions = ["tree:write"]

[[policy]]
name = "allow-infra-team-production"
effect = "allow"
subjects = [{ team = "infra-team" }]
scope = { path = "config/production.toml" }
permissions = ["tree:read", "tree:write"]
```

#### Example 5: Branch-Specific Protection

```toml
[[policy]]
name = "protect-main-branch"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { branch = "main" }
permissions = ["sync:push", "branch:delete"]

[[policy]]
name = "allow-maintainers-main"
effect = "allow"
subjects = [{ role = "Maintainer" }]
scope = { branch = "main" }
permissions = ["sync:push", "branch:merge"]
```

#### Example 6: Deny Override for Sensitive Operations

```toml
# Allow devs to do everything on feature branches
[[policy]]
name = "dev-feature-branches"
effect = "allow"
subjects = [{ role = "Developer" }]
scope = "tree"
permissions = [
    "tree:read", "tree:write",
    "branch:read", "branch:create", "branch:delete",
    "snapshot:create", "snapshot:read",
    "sync:push", "sync:pull"
]

# But deny deletion of release branches
[[policy]]
name = "deny-release-branch-delete"
effect = "deny"
subjects = [{ all_authenticated = true }]
scope = { branch = "release/*" }
permissions = ["branch:delete"]
```

> **Note**: Branch name patterns like `release/*` are evaluated as prefix matches, not globs. This is different from RegisteredPath which requires exact match via explicit registration.

#### Example 7: Combined Team + Tenant + Role Policy

```toml
[[policy]]
name = "comprehensive-access"
effect = "allow"
subjects = [
    { team = "core-team" },
    { tenant = "trusted-partner" },
    { account = "special-contractor" }
]
scope = "worktree"
permissions = [
    "tree:read", "tree:write",
    "branch:read", "branch:create",
    "snapshot:create", "snapshot:read",
    "sync:push", "sync:pull",
    "merge_request:create"
]
```

---

## ABAC (Attribute-Based Access Control)

In addition to role-based policies, W0rkTree supports attribute-based conditions on policies. This allows fine-grained access decisions based on runtime context.

### Condition Structure

```toml
[[policy]]
name = "business-hours-only"
effect = "allow"
subjects = [{ team = "contractors" }]
scope = "worktree"
permissions = ["tree:read", "tree:write", "sync:push", "sync:pull"]
conditions = [
    { attribute = "time.hour", operator = "GreaterThan", value = "8" },
    { attribute = "time.hour", operator = "LessThan", value = "18" }
]
```

### Supported Operators

| Operator     | Description                      | Example                                           |
|--------------|----------------------------------|---------------------------------------------------|
| `Equals`     | Exact match                      | `{ attribute = "env", operator = "Equals", value = "production" }` |
| `NotEquals`  | Not equal                        | `{ attribute = "env", operator = "NotEquals", value = "test" }` |
| `Contains`   | String contains                  | `{ attribute = "email", operator = "Contains", value = "@company.com" }` |
| `StartsWith` | String prefix match              | `{ attribute = "ip", operator = "StartsWith", value = "10.0." }` |
| `GreaterThan`| Numeric greater than             | `{ attribute = "time.hour", operator = "GreaterThan", value = "8" }` |
| `LessThan`   | Numeric less than                | `{ attribute = "time.hour", operator = "LessThan", value = "18" }` |
| `In`         | Value in list                    | `{ attribute = "department", operator = "In", value = "engineering,security" }` |
| `NotIn`      | Value not in list                | `{ attribute = "region", operator = "NotIn", value = "restricted-zone" }` |

### Built-in Attributes

| Attribute        | Type    | Description                                  |
|------------------|---------|----------------------------------------------|
| `time.hour`      | Integer | Current hour (0-23) in UTC                   |
| `time.day`       | String  | Day of week (Monday, Tuesday, ...)           |
| `time.date`      | String  | ISO 8601 date                                |
| `ip.address`     | String  | Client IP address                            |
| `ip.range`       | String  | Client IP CIDR range                         |
| `tenant.plan`    | String  | Requesting tenant's subscription plan        |
| `tenant.type`    | String  | personal or organization                     |
| `tenant.status`  | String  | Active or Suspended                          |

### Custom Attributes

Tenants can define custom attributes on their accounts for use in ABAC conditions:

```toml
# On the tenant record
[attributes]
department = "engineering"
clearance_level = "3"
office_location = "building-a"
```

Then reference in policies:

```toml
[[policy]]
name = "engineering-only-access"
effect = "allow"
subjects = [{ all_authenticated = true }]
scope = { path = "src/internal/" }
permissions = ["tree:read"]
conditions = [
    { attribute = "department", operator = "Equals", value = "engineering" }
]
```

### ABAC Examples

#### Time-Based Access

```toml
[[policy]]
name = "weekend-readonly"
effect = "deny"
subjects = [{ role = "Developer" }]
scope = "worktree"
permissions = ["tree:write", "sync:push"]
conditions = [
    { attribute = "time.day", operator = "In", value = "Saturday,Sunday" }
]
```

#### IP-Based Access

```toml
[[policy]]
name = "office-only-admin"
effect = "allow"
subjects = [{ role = "Admin" }]
scope = "worktree"
permissions = ["TenantAdmin", "PolicyManage", "RoleManage"]
conditions = [
    { attribute = "ip.address", operator = "StartsWith", value = "10.0.1." }
]
```

#### Combined RBAC + ABAC

```toml
[[policy]]
name = "senior-eng-production-access"
effect = "allow"
subjects = [{ team = "senior-engineers" }]
scope = { path = "config/production.toml" }
permissions = ["tree:read", "tree:write"]
conditions = [
    { attribute = "clearance_level", operator = "GreaterThan", value = "2" },
    { attribute = "ip.address", operator = "StartsWith", value = "10.0." }
]
```

All conditions are AND-evaluated: every condition must be true for the policy to apply.

---

## Scope Resolution Algorithm

When the server evaluates an access request, it follows this deterministic algorithm:

### Input
- **Subject**: The requesting user (account, tenant, teams, roles)
- **Permission**: The atomic permission being requested (e.g., `tree:write`)
- **Resource**: The target resource with its scope (e.g., branch "main", path "config/prod.toml")

### Algorithm

```
function evaluate(subject, permission, resource) -> Allow | Deny:
    1. Collect ALL policies where:
       - subject matches any entry in policy.subjects
       - permission is in policy.permissions
       - All ABAC conditions (if any) evaluate to true

    2. Filter to policies whose scope COVERS the requested resource:
       - Global covers everything
       - Tenant covers all trees in the tenant
       - Tree covers all branches and paths in the tree
       - Branch covers the specific branch and paths within it
       - RegisteredPath covers only that exact path

    3. Group matching policies by scope level (most specific first):
       Level 5: RegisteredPath
       Level 4: Branch
       Level 3: Tree
       Level 2: Tenant
       Level 1: Global

    4. Starting from the most specific level that has matching policies:
       a. If ANY policy at this level has effect = "deny" → return DENY
       b. If ANY policy at this level has effect = "allow" → return ALLOW
       c. (This level has no matching policies — continue to next)

    5. Move to the next broader scope level and repeat step 4

    6. If no matching policy found at any level → return DENY (default deny)
```

### Resolution Examples

**Example A**: Developer pushes to `main` branch

```
Policies:
  P1: Allow Developer tree:write at Tree scope
  P2: Deny all_authenticated sync:push at Branch("main") scope

Evaluation for sync:push on Branch("main"):
  Level 4 (Branch): P2 matches → DENY
  (P1 at Level 3 is never evaluated)
  Result: DENY
```

**Example B**: Maintainer pushes to `main` branch

```
Policies:
  P1: Allow Developer tree:write at Tree scope
  P2: Deny all_authenticated sync:push at Branch("main") scope
  P3: Allow Maintainer sync:push at Branch("main") scope

Evaluation for sync:push on Branch("main"):
  Level 4 (Branch): P2 (deny) and P3 (allow) both match
  → Deny beats Allow at same level → DENY
  Result: DENY
```

> This is by design: to allow Maintainer push on main, you must narrow the deny subject instead of broadening the allow.

**Example C**: Path-level override

```
Policies:
  P1: Allow backend-team tree:write at Tree scope
  P2: Deny all_authenticated tree:write at RegisteredPath("config/production.toml")
  P3: Allow infra-team tree:write at RegisteredPath("config/production.toml")

Evaluation for tree:write on "config/production.toml" by backend-team member NOT in infra-team:
  Level 5 (RegisteredPath): P2 matches (deny) → DENY
  Result: DENY

Evaluation for tree:write on "config/production.toml" by infra-team member:
  Level 5 (RegisteredPath): P2 (deny) and P3 (allow) both match
  → Deny beats Allow at same level → DENY
  Result: DENY (must restructure policies to avoid this conflict)
```

---

## .wt-tree/ Override Rules

W0rkTree supports a two-level policy hierarchy: root-level policies in `.wt/access/` and tree-level policies in `.wt-tree/access/`. Tree-level policies have strict rules about what they can do relative to root policies.

### Core Principle: Restrict Only, Never Expand

Tree policies can **restrict** access granted by root policies but can **never expand** access beyond what root policies allow.

### Rules

1. **If root allows `tree:read` + `tree:write` for team-A**:
   - Tree CAN restrict to `tree:read` only (removing write)
   - Tree CANNOT add `tree:delete` (not granted by root)

2. **If root denies `tree:write` for team-B**:
   - Tree CANNOT allow `tree:write` for team-B (root deny is absolute)
   - Tree CAN add additional denies for team-B

3. **If root has no policy for user-C**:
   - Tree CANNOT grant access to user-C (no root-level allow to restrict)
   - Tree CAN explicitly deny user-C (though default deny already applies)

### Evaluation Order

```
function evaluate_with_tree_override(subject, permission, resource):
    root_result = evaluate(subject, permission, resource, root_policies)
    tree_result = evaluate(subject, permission, resource, tree_policies)

    if root_result == DENY:
        return DENY                    # Root deny is absolute
    if tree_result == DENY:
        return DENY                    # Tree can restrict
    if root_result == ALLOW and tree_result == ALLOW:
        return ALLOW                   # Both agree
    if root_result == ALLOW and tree_result == NO_MATCH:
        return ALLOW                   # Root allows, tree has no opinion
    if root_result == NO_MATCH:
        return DENY                    # No root allow = no access
    return DENY                        # Default
```

### Example

Root policies (`.wt/access/policies.toml`):
```toml
[[policy]]
name = "team-access"
effect = "allow"
subjects = [{ team = "dev-team" }]
scope = "worktree"
permissions = ["tree:read", "tree:write", "branch:create", "sync:push", "sync:pull"]
```

Tree policies (`.wt-tree/access/policies.toml`):
```toml
# Restrict dev-team to read-only in this tree
[[policy]]
name = "restrict-dev-team"
effect = "deny"
subjects = [{ team = "dev-team" }]
scope = "tree"
permissions = ["tree:write", "sync:push"]
```

Result: dev-team has `tree:read`, `branch:create`, `sync:pull` in this tree but NOT `tree:write` or `sync:push`.

---

## Access Config as Version-Controlled Files

A key design principle of W0rkTree IAM is that access configuration is treated as code.

### Storage

| File                              | Purpose                            |
|-----------------------------------|------------------------------------|
| `.wt/config.toml`                 | Worktree config, path registration |
| `.wt/access/roles.toml`          | Custom role definitions            |
| `.wt/access/policies.toml`       | Root-level access policies         |
| `.wt-tree/config.toml`           | Tree-level config, path registration|
| `.wt-tree/access/policies.toml`  | Tree-level access policies         |

### Properties

1. **Version-Controlled**: All access files are part of the worktree and are versioned through snapshots
2. **Synced**: Access files are synced to/from the server like any other file
3. **Auditable**: Full history of who changed what access when (via snapshot history)
4. **Protected**: Only users with `PolicyManage` or `TreeAdmin` permissions can modify access files
5. **Server-Enforced**: The server validates access files on every sync and rejects invalid configurations

### Modification Flow

```
1. User with PolicyManage edits .wt/access/policies.toml
2. BGProcess detects the file change
3. BGProcess validates locally:
   - TOML syntax check
   - Permission name validation
   - Role reference validation
   - Registered path verification (for path-scoped policies)
4. BGProcess syncs to server
5. Server validates:
   - Tenant resolution (username/email lookup)
   - Path registration verification
   - Policy consistency check
   - Modifier authorization (does user have PolicyManage?)
6. Server applies policies immediately
7. Other BGProcess clients sync updated access config
8. Policies take effect across all connected clients
```

### Protection of Access Files

Even though access files are "just files," they have special protections:

- Only users with `PolicyManage` or `TreeAdmin` can modify files in `.wt/access/`
- Only users with `PolicyManage` or `TreeAdmin` can modify files in `.wt-tree/access/`
- The server rejects sync operations that modify access files from unauthorized users
- Modifications to access files are logged with additional audit metadata

---

## Custom Roles

In addition to the five built-in roles, tenants can define custom roles in `.wt/access/roles.toml`:

```toml
[[role]]
name = "code-reviewer"
description = "Can read code and approve merge requests, but not write directly"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:read",
    "sync:pull",
    "merge_request:approve"
]

[[role]]
name = "release-manager"
description = "Can manage releases and protected branches"
permissions = [
    "tree:read",
    "branch:read",
    "branch:merge",
    "branch:protect",
    "snapshot:read",
    "sync:pull",
    "tag:create",
    "tag:delete",
    "release:create",
    "release:delete"
]

[[role]]
name = "ci-bot"
description = "Automated CI/CD service account"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:read",
    "sync:pull",
    "sync:push",
    "tag:create",
    "release:create"
]
```

Custom roles can then be referenced in policies:

```toml
[[policy]]
name = "reviewer-access"
effect = "allow"
subjects = [{ role = "code-reviewer" }]
scope = "worktree"
permissions = [
    "tree:read",
    "branch:read",
    "snapshot:read",
    "sync:pull",
    "merge_request:approve"
]
```

---

## Implementation Status

### Implemented

- **AccessEngine** — Core evaluation engine (871 lines of Rust)
- **RBAC** — Full role-based access control with 5 built-in roles
- **ABAC** — Attribute-based conditions with 8 operators
- **20+ Permissions** — Full set of atomic permissions across all categories
- **Policy Evaluation** — Scope-based resolution with deny-overrides
- **Conditions** — Time-based, IP-based, and custom attribute conditions

### TODO

- `Scope::RegisteredPath` variant — Implement path-level scope matching
- `PolicySubject::Tenant` variant — Implement cross-tenant subject matching
- Tenant email field — Add email to Tenant struct for email-based resolution
- Declarative config parsing — Parse `.wt/access/*.toml` and `.wt-tree/access/*.toml`
- Config validation — Local and server-side validation of policy files
- Path registration — Parse `[[registered_path]]` from `config.toml`

### Planned

- Cross-tenant access resolution — Full tenant lookup and resolution pipeline
- License compliance integration — Tie access control to license plan limits
- Organization accounts — Multi-account tenants with team management
- Audit logging — Detailed logs of all access decisions and policy changes
- Policy simulation — Dry-run mode to test policy changes before applying

---

## Security Considerations

1. **Default Deny**: Any operation without an explicit Allow policy is denied
2. **Deny Precedence**: At the same scope level, Deny always beats Allow
3. **Root Policy Supremacy**: Tree-level policies cannot override root-level denies
4. **Server Enforcement**: All access checks happen on the server, never trust the client
5. **Access File Protection**: Only authorized users can modify access configuration
6. **Audit Trail**: All access decisions are loggable for compliance
7. **No Implicit Trust**: Cross-tenant access requires explicit grants

---

## Related Specifications

- [Declarative Access Control](./DeclarativeAccess.md) — Detailed specification of the declarative config format
- [Tenant Model](./TenantModel.md) — Tenant identity, types, and cross-tenant access