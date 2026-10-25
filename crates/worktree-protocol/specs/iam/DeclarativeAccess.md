# Declarative Access Specification

## Table of Contents

1. [Overview](#1-overview)
2. [Files](#2-files)
3. [TOML format](#3-toml-format)
4. [Compiler and canonical JSON](#4-compiler-and-canonical-json)
5. [Validation](#5-validation)
6. [Workflow: validate, plan, apply, pull](#6-workflow-validate-plan-apply-pull)
7. [Wire protocol](#7-wire-protocol)
8. [Server apply](#8-server-apply)
9. [REST API](#9-rest-api)
10. [Tree documents: restrict only](#10-tree-documents-restrict-only)
11. [Recovery and lockout](#11-recovery-and-lockout)
12. [Examples](#12-examples)
13. [Implementation status](#13-implementation-status)
14. [Roadmap](#14-roadmap)
15. [References](#15-references)

---

## 1. Overview

You can keep custom roles and policies as TOML files in the worktree, review
them like code, and apply them to a server tenant in the style of Terraform:

```
.wt/access/*.toml ──wt access validate──► compiler (worktree-protocol)
                  ──wt access plan──────► SyncAccessConfig{dry_run}  ──► changes
                  ──wt access apply─────► SyncAccessConfig           ──► roles + policies replaced
                  ◄─wt access pull──────── GetAccessConfig
```

The files describe the same objects as the IAM REST API: custom roles and
policies (see [IAM](./IAM.md)). Applying a document **replaces** the roles and
policies that earlier applies of the same document created. These objects are
marked `managed_by: "config"`. Roles and policies created through the REST API
or the admin panel are never touched by an apply.

The server is the source of truth. Editing a file has no effect until it is
applied, and every apply is validated, checked for escalation, audited and
published as an event.

## 2. Files

| File | Scope | May contain |
|---|---|---|
| `.wt/access/roles.toml` | tenant | `[[role]]` |
| `.wt/access/policies.toml` | tenant | `[[policy]]` (allow and deny statements) |
| `<tree>/.wt-tree/access/policies.toml` | one tree | `[[policy]]` with **deny statements only** |

The tenant is the worktree remote's tenant, or `--tenant`. A tree document
applies to the server tree the local tree is linked to. `wt tree init-config`
scaffolds `.wt-tree/access/`.

## 3. TOML format

```toml
# .wt/access/roles.toml
[[role]]
name = "release-manager"
description = "Cut releases"
permissions = ["tree:read", "release:*", "mr:merge"]
```

```toml
# .wt/access/policies.toml
[[policy]]
name = "protect-main"
description = "Only maintainers push to main"
enabled = true

[[policy.statement]]
sid = "no-direct-main"
effect = "deny"                 # allow | deny
principals = ["role:developer", "team:contractors", "user:bob", "tenant:partner"]
actions = ["branch:push"]
resources = ["wt:branch/acme/web/main"]
conditions = { mfa = true }
```

**Role fields**:

- `name`: 2–40 characters from `[a-z0-9-]`, and not a built-in role name;
- `description`: optional;
- `permissions`: catalog actions or `<area>:*`. `*` and `server:*` are not
  allowed.

**Policy fields**:

- `name`: required;
- `description`: optional;
- `enabled`: default `true`;
- one or more `[[policy.statement]]` tables.

**Statement fields** have the same meaning as the JSON statement fields in
[IAM §8](./IAM.md#8-policies):

| Field | Meaning |
|---|---|
| `sid` | Optional statement id |
| `effect` | `allow` or `deny` |
| `principals` | `user:`, `team:`, `role:`, `sa:`, `tenant:`, `authenticated`, `anonymous`, `*` |
| `actions` | Catalog actions, `<area>:*`, `*` |
| `resources` | `*` or `wt:tenant/…`, `wt:tree/…`, `wt:branch/…`, `wt:path/…` with `*` and `**` |
| `conditions` | Inline table (below) |

**Condition keys**:

| Key | Meaning |
|---|---|
| `ip_ranges` | List of CIDRs |
| `mfa` | Requires a session created after a second factor. Written `require_mfa` in JSON. |
| `not_before`, `not_after` | RFC 3339 timestamps |
| `time_window` | `{ days = ["mon", …], start = "09:00", end = "18:00", timezone = "Europe/Berlin" }` |

**Permission aliases.** Names from the v1 protocol model are accepted and
translated. `wt access plan` prints a warning for each one:

| Alias | Canonical action |
|---|---|
| `sync:push` | `branch:push` |
| `snapshot:create` | `tree:write` |
| `branch:merge` | `mr:merge` |
| `tree:delete` | `tree:admin` |

## 4. Compiler and canonical JSON

The compiler lives in the Rust module `worktree_protocol::access::declarative`:

- `parse_roles(&str)` and `parse_policies(&str)` read the TOML files.
- `compile(...)` produces the **canonical JSON document**:

  ```json
  {
    "roles":    [ { "name": "...", "description": "...", "permissions": ["..."] } ],
    "policies": [ { "name": "...", "description": "...", "enabled": true,
                    "statements": [ { "sid": "...", "effect": "deny", "actions": [...],
                                      "resources": [...], "principals": [...],
                                      "conditions": { "require_mfa": true } } ] } ]
  }
  ```

The field names are those of the Go `CustomRole`, `Policy` and
`PolicyStatement` JSON (`internal/domain/iam.go`). The document is the only
format the server accepts. TOML never reaches the server.

## 5. Validation

The client checks the document before sending it. The server checks it
again, because a client can be modified.

**Local checks** (`wt access validate`, also run by `plan` and `apply`):

- TOML syntax and schema: required fields and types.
- Action names, using the catalog plus aliases.
- Principal and resource syntax, using the same grammar as
  `authz.ValidateStatements`.
- Condition syntax: CIDRs, timestamps in order, `HH:MM` clock times, `mon` to
  `sun`, IANA timezones.
- No duplicate role or policy names.
- No built-in role names.
- No `allow` statement in a tree document.

Each error carries a stable code in the `E2xxx` range, the file and the
offending policy, statement or role. The exact codes are listed by the error
type of `worktree_protocol::access::declarative`.

**Server checks** (`accesscfg.Service.Apply`):

1. The document decodes as canonical JSON.
2. Statements pass `authz.ValidateStatements`, and roles pass
   `authz.ValidatePermissions`.
3. Every `team:` and `user:` reference resolves to a team of the tenant or an
   existing account. Every `role:` reference names a built-in role or a role
   in the document or the tenant. Every `tenant:` reference names an existing
   tenant.
4. Tree documents contain deny statements only.
5. **No escalation.** Every permission granted by a role or by an allow
   statement must be among the caller's own effective permissions. Tenant
   owners and server admins are exempt.

Problems come back as `accepted: false` with `validation_errors`. Nothing is
changed.

## 6. Workflow: validate, plan, apply, pull

```
wt access validate [--dir DIR]          # compile and check locally, no server
wt access plan     [--tenant T]         # SyncAccessConfig{dry_run: true}, prints changes
wt access apply    [--tenant T] [--yes] # prints the plan, asks, applies
wt access pull     [--tenant T]         # GetAccessConfig, writes .wt/access/*.toml
```

The plan lists one line per affected object, for example:

```
role release-manager: create
policy protect-main: update (2 statements)
policy legacy-readers: delete
```

`apply` asks for confirmation unless you pass `--yes`. When stdin is not a
terminal, `--yes` is required. `pull` writes the last applied document back
as TOML, so a tenant configured in the admin panel and then applied can be
brought under version control.

The SDK functions are `worktree_sdk::remote::access::{plan, apply, pull}`.

## 7. Wire protocol

Both RPCs are part of `worktree.sync.v1.SyncService`
(`crates/worktree-protocol/proto/sync.proto`).

`SyncAccessConfig(AccessConfigSyncRequest) → AccessConfigSyncResponse`:

| Field | Value |
|---|---|
| `tenant_id` | Tenant id or slug |
| `config_type` | `ACCESS_CONFIG_TYPE_POLICIES`. The content holds both roles and policies. |
| `content` | Canonical JSON (at most 1 MiB) |
| `config_hash` | BLAKE3 of `content` |
| `tree_id` | Set for tree documents |
| `dry_run` | `true` for `plan` |
| response `accepted`, `validation_errors`, `changes` | Result, errors and the human-readable plan |

`GetAccessConfig(GetAccessConfigRequest{tenant_id, tree_id?, config_type})` →
`{found, content, config_hash, updated_at, updated_by}`.

The other `AccessConfigType` values (`ROLES`, `TENANT_ACCESS`,
`BRANCH_PROTECTION`, `LICENSE`) are defined on the wire but are not used by
`wt access`.

## 8. Server apply

`accesscfg.Service.Apply(ctx, principal, tenant, tree, content, dryRun)` runs
these steps:

1. **Authorize.** The caller needs `iam:admin` on the tenant.
2. **Validate.** Run the checks in §5.
3. **Plan.** Compare the document with the roles and policies managed by
   config for this document. The document is identified by the tenant plus
   the tree (or none). Each object gets one of:
   - `create`: in the document, not on the server;
   - `update`: content differs;
   - `delete`: on the server as config-managed, gone from the document.
4. **Stop for a dry run.** A dry run returns the plan here.
5. **Replace atomically.** Write the changes, setting
   `CustomRole.ManagedBy` / `Policy.ManagedBy` to `"config"`. Updated
   policies get `version++`.
6. **Record.** Store an `AccessConfigDoc`:
   - collection `access_docs`, key `DocKey(tenantID, treeID)`;
   - fields `{tenant_id, tree_id, kind, content, hash, updated_by,
     updated_at}`.
7. **Audit and publish.** Audit `access.applied`, with the plan in the
   details, and publish event `access.applied`.

Objects managed by config are **read-only in the REST API**. `PUT`, `PATCH`
and `DELETE` on them answer `409 {"code":"managed_by_config"}`, unless a
tenant owner adds `?force=true`. A forced edit is overwritten by the next
apply.

## 9. REST API

```
GET  /api/v1/tenants/{t}/access-config                 -> AccessConfigDoc | 404
POST /api/v1/tenants/{t}/access-config/plan   {"content": <canonical JSON>} -> {accepted, errors, changes}
POST /api/v1/tenants/{t}/access-config/apply  {"content": <canonical JSON>} -> {accepted, errors, changes}
GET|POST ... /api/v1/trees/{t}/{tree}/access-config[/plan|/apply]           (tree documents)
POST /api/v1/iam/simulate {..., "candidate_policies": [Policy]}             (dry-run a policy change)
```

In the admin panel, the tenant IAM **Access config** tab shows the last
applied document and can run plan and apply on pasted JSON.

## 10. Tree documents: restrict only

`<tree>/.wt-tree/access/policies.toml` lets the people who own a tree tighten
access to it without touching tenant-wide files. Tree documents follow these
rules:

- **Deny statements only.** An allow statement is a validation error.
- **Separate documents.** Their policies are stored as config-managed policies
  of the tree's tenant, tracked per tree document. Applying one tree's
  document never changes another document's objects.
- **Same evaluation.** They are evaluated like every other policy. Under
  deny-overrides, a tree document can only remove access, which is the
  "trees restrict, never expand" rule.

## 11. Recovery and lockout

These mechanisms protect against a bad apply:

- **Lockout guard.** Deny statements never apply to tenant owners for
  `iam:*` and `tenant:admin` (reason `owner_protected`), so an owner can
  always re-apply or fix policies.
- **Last owner.** A tenant always keeps at least one owner
  (`409 {"code":"last_owner"}`).
- **Server admins** are not subject to policies.
- **Rollback.** Revert the TOML in the worktree (for example
  `wt restore .wt/access --from <rev>`) and run `wt access apply` again.
- **Dry run first.** `wt access plan` and `POST /iam/simulate` with
  `candidate_policies` show the effect of a change before it is applied.

## 12. Examples

**Open-source project with protected releases**

```toml
# .wt/access/policies.toml (tenant "oss")
[[policy]]
name = "release-branches"
[[policy.statement]]
sid = "developers-do-not-push-release"
effect = "deny"
principals = ["role:developer", "role:viewer"]
actions = ["branch:push", "branch:delete"]
resources = ["wt:branch/oss/*/release/**"]

[[policy]]
name = "release-managers"
[[policy.statement]]
effect = "allow"
principals = ["team:release-managers"]
actions = ["release:*", "mr:merge"]
resources = ["wt:tenant/oss"]
```

Deny overrides allow, so a deny aimed at `authenticated` would also block
release managers. To exempt a group from a deny, narrow the deny's principals,
as above, instead of adding an allow. A developer who belongs to
`release-managers` still matches `role:developer` and is still denied the
push. Give such people a maintainer role or a custom role instead.

**Restricting a sensitive directory in one tree**

```toml
# payments/.wt-tree/access/policies.toml
[[policy]]
name = "payments-secrets"
[[policy.statement]]
effect = "deny"
principals = ["tenant:contractor-co", "role:viewer"]
actions = ["tree:read", "tree:write"]
resources = ["wt:path/acme/payments/config/secrets/**"]
```

Contractors and viewers receive the rest of the tree on pull. The files under
`config/secrets/` are stubbed for them (`stubbed_paths`), and pushes touching
those paths are rejected.

## 13. Implementation status

| Area | Status |
|---|---|
| TOML format, aliases, compiler to canonical JSON, local validation | Built (v0.5, `worktree_protocol::access::declarative`) |
| `wt access validate / plan / apply / pull`, SDK `remote::access` | Built (v0.5) |
| `SyncAccessConfig` / `GetAccessConfig` backed by `accesscfg.Service` | Built (v0.5) |
| Escalation check, plan, atomic replace, `managed_by`, `AccessConfigDoc`, audit and event | Built (v0.5) |
| REST access-config routes; `candidate_policies` in the simulator | Built (v0.5) |
| Other `AccessConfigType` kinds (`TENANT_ACCESS`, `BRANCH_PROTECTION`, `LICENSE`) | Defined on the wire only |

## 14. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **Path registration.** `[[registered_path]]` in `.wt/config.toml` and
  `.wt-tree/config.toml`, the "no globs" rule and the `E2003` "path not
  registered" error. Path control uses `wt:path/...` resources with `*` and
  `**`.
- **The v1 policy schema**: `subjects = [{ team = … }]`, `scope = { branch =
  … }`, `permissions = [...]` and `conditions = [{ attribute, operator, value
  }]` with custom attributes. The statement schema in §3 replaces it.
- **Simple tenant grants**: the `[[tenant_access]]` shorthand in
  `.wt/config.toml`. Use tree grants with `subject_type: "tenant"` or
  `tenant:<slug>` principals.
- **Tenant references by email** (`{ tenant = "bob@contractor.io" }`).
  References are slugs.
- **Automatic sync by the background process.** The daemon would watch
  `.wt/access/` and apply changes on its own. Applying is always an explicit
  `wt access apply`.
- **Access files as protected tree content.** The server would reject pushes
  that modify access files unless the pusher has `iam:admin`, and other
  clients would receive server-side changes on sync. Access files are local
  configuration, and the server keeps only the applied document.
- **Policy history and diffs** between applied versions. Only the last
  `AccessConfigDoc` and each policy's `version` counter are kept.
- **The E1xxx (schema), E3xxx (logical) and E4xxx (security) error families**,
  including circular role dependencies. Roles do not inherit from each other,
  so cycles cannot occur.
- **Scope-precedence resolution** between root and tree files. Evaluation is
  deny-overrides ([IAM §9](./IAM.md#9-evaluation-algorithm)).

## 15. References

- `crates/worktree-protocol/src/access/declarative/` (parser, compiler,
  validation errors)
- `crates/worktree-sdk/src/remote/access.rs`
- `crates/worktree-cli/src/commands/access.rs`, `policy.rs`, `tree.rs`
  (`init-config`)
- `crates/worktree-protocol/proto/sync.proto` (`SyncAccessConfig`,
  `GetAccessConfig`, `AccessConfigType`)
- `services/server-go/internal/accesscfg/`, `internal/syncsvc/accesscfg.go`,
  `internal/httpapi/access_routes.go`
- `services/server-go/internal/authz/validate.go`, `actions.go`
- `services/server-go/internal/domain/{iam,v05}.go` (`AccessConfigDoc`,
  `ManagedBy`)
- `docs/ENGINEERING_PLAN.md` §13.7, §13.14
- Related specs: [IAM](./IAM.md), [Tenant Model](./TenantModel.md),
  [`.wt/`](../dot-wt/DotWt.md), [`.wt-tree/`](../dot-wt-tree/DotWtTree.md)
