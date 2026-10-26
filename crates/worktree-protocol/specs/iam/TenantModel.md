# Tenant Model Specification

## Table of Contents

1. [Overview](#1-overview)
2. [Tenants](#2-tenants)
3. [Accounts](#3-accounts)
4. [Memberships and roles](#4-memberships-and-roles)
5. [Teams and service accounts](#5-teams-and-service-accounts)
6. [Lifecycle](#6-lifecycle)
7. [Tenant status](#7-tenant-status)
8. [Limits, quotas and usage](#8-limits-quotas-and-usage)
9. [Cross-tenant access](#9-cross-tenant-access)
10. [Tree visibility](#10-tree-visibility)
11. [Invitations](#11-invitations)
12. [REST API and CLI](#12-rest-api-and-cli)
13. [Security considerations](#13-security-considerations)
14. [Implementation status](#14-implementation-status)
15. [Roadmap](#15-roadmap)
16. [References](#16-references)

---

## 1. Overview

A **tenant** is the namespace that owns trees on a W0rkTree server. A tree is
addressed as `tenant/tree`, for example `wt clone http://host:7420 acme/web`.

There are two kinds of tenant:

| Kind | `Tenant.Kind` | Created by | Typical use |
|---|---|---|---|
| Personal | `user` | Account registration (one per account, slug = username) | An individual's own trees |
| Organization | `org` | `POST /tenants` / `wt tenant create` by any signed-in account | Teams, companies, projects |

People act through **accounts**. An account joins tenants through
**memberships**, each of which carries a role. Authorization is defined in
[IAM](./IAM.md). This document covers the tenant, account and membership data
model and its lifecycle.

## 2. Tenants

`domain.Tenant` (JSON names as in `internal/domain/types.go`):

| Field | Type | Notes |
|---|---|---|
| `id` | string (UUID) | Internal id; not used in URLs |
| `slug` | string | Unique on the server. Used in URLs, CLI arguments, policy resources (`wt:tenant/<slug>`) and principals (`tenant:<slug>`). |
| `name` | string | Display name, at most 200 characters (defaults to the slug) |
| `kind` | `user` \| `org` | See §1 |
| `plan` | `free` \| `pro` \| `enterprise` | Label set by server admins. It does not change limits (§8). |
| `created_at` | timestamp | |
| `status` | `""` / `active` \| `suspended` \| `deactivated` | v0.5, see §7 |
| `status_reason` | string | v0.5 |
| `limits` | `TenantLimits` or absent | v0.5 per-tenant overrides, see §8 |

Slug rules:

- **Organization slugs**: 1–63 characters from `[a-z0-9-]`, not starting or
  ending with `-` (`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`).
- **Personal slugs** equal the username (§3).
- Slugs cannot be changed. `wt tenant rename` changes only the display name.

The server keeps no tenant email address. Email belongs to accounts.

## 3. Accounts

`domain.Account` has the following fields:

| Field | Notes |
|---|---|
| `id` | Account id |
| `username` | `^[a-z0-9][a-z0-9-]{1,38}$`, entered lower-cased. Reserved: `admin`, `anonymous`, `api`, `auth`, `events`, `healthz`, `metrics`, `system`. `admin` can be claimed only by the configured server admin. |
| `email` | Syntax-checked, **not verified** |
| `display_name` | Free text |
| `tenant_id` | The personal tenant |
| `is_admin` | Server admin |
| `disabled` | A disabled account is denied every action |
| `created_at` | Timestamp |

Accounts are created in three ways:

- **Self-registration**: `POST /auth/register`, `wt register`. This works
  while `--allow-registration` is on, which is the default.
- **By a server admin**: `POST /admin/accounts`, `wt admin account-create`.
- **On first SSO login**, when `auto_create` is on.

Registration creates, in one step:

- the account;
- its personal tenant (`kind: "user"`, `plan: "free"`, slug = username);
- an `owner` membership in that tenant.

Server admins can disable or enable an account and grant or revoke server
admin with `PATCH /admin/accounts/{username}` or `wt admin account-set`.

## 4. Memberships and roles

A `domain.Membership` is a triple `{tenant_id, account_id, role}`. The role is
a built-in role (`owner`, `admin`, `maintainer`, `developer`, `viewer`) or a
custom role of that tenant. See [IAM §4](./IAM.md#4-roles) for what each role
allows.

- `GET /tenants/{t}/members` needs `tenant:read`.
- `PUT /tenants/{t}/members/{username} {role}` and `DELETE` need
  `tenant:admin`.
- Only owners (and server admins) may grant or revoke `owner`.
- The last owner cannot be removed or demoted (`409 {"code":"last_owner"}`).
  An owner may leave only while another owner remains.
- Personal tenants are ordinary tenants. Their owner may add members, teams
  and grants exactly as in an organization.

## 5. Teams and service accounts

Both belong to exactly one tenant. See [IAM §5](./IAM.md#5-teams) and
[IAM §2](./IAM.md#2-identities-and-principals).

- **Teams** group the tenant's accounts. They receive access only through tree
  grants and `team:<slug>` policy principals.
- **Service accounts** (`sa:<tenant>/<name>`) act inside their tenant with
  their own role, which can be anything except `owner`. They authenticate with
  keys.

## 6. Lifecycle

| Step | How |
|---|---|
| Create organization | `POST /tenants {slug, name}` / `wt tenant create SLUG [--name N]`. The caller becomes `owner`. Kind `org`, plan `free`. |
| Rename | `PATCH /tenants/{t} {name}` / `wt tenant rename T NAME` (`tenant:admin`) |
| Change plan | `PATCH /tenants/{t} {plan}` / `wt tenant plan T free\|pro\|enterprise` (server admin) |
| Suspend / resume / deactivate | `PATCH /admin/tenants/{t} {status, status_reason}` / `wt tenant suspend\|resume\|deactivate T [--reason]` (server admin, v0.5) |
| Set limits | `PUT /admin/tenants/{t}/limits` / `wt admin limits T ...` (server admin, v0.5) |
| Delete | Not supported. Deactivate the tenant instead (§7). |

`tenant.create` and `tenant.update` (rename, plan, status and limits) are
audited.

## 7. Tenant status

| Status | Effect |
|---|---|
| `active` (or `""`) | Normal |
| `suspended` | Read-only. Authorization denies every action other than `tenant:read`, `tree:read` and `iam:read`, with reason `tenant_suspended`. |
| `deactivated` | Authorization denies every action on the tenant and its trees. |

Status changes follow these rules:

- **Server admins** are never affected.
- **No revocation.** Suspension and deactivation revoke no tokens, delete no
  data and change no memberships. `resume` restores full access immediately.
- **Scope.** Status applies to resources of the tenant. Members can still use
  other tenants they belong to.

## 8. Limits, quotas and usage

`domain.TenantLimits` has these fields. Zero means "use the server default".

| Field | Enforced by |
|---|---|
| `max_trees` | REST tree creation → `409 {"code":"quota_exceeded"}` |
| `max_storage_bytes` | `limits.Limiter` push check, measuring new objects of accepted pushes (doc `tenant_usage`) → `PushDenied{Kind:"quota_exceeded"}` |
| `max_branches_per_tree` | Push check for new branches, and REST branch creation |
| `max_members` | REST member add → `409 quota_exceeded` |
| `requests_per_minute` | Rate limiter override for the tenant's principals |

Server defaults come from `--default-max-trees` / `WT_DEFAULT_MAX_TREES` and
`--default-max-storage` / `WT_DEFAULT_MAX_STORAGE`. Both default to `0`, which
means unlimited. The global rate limit is `--rate-limit` (1200 requests per
minute per principal).

The **effective** limit is the tenant override if set, else the server
default. `GET /tenants/{t}/usage` returns `{trees, branches, members,
storage_bytes, limits, effective}`. `wt admin usage T` prints it.

## 9. Cross-tenant access

A tenant's trees are private to its members unless the owners open them up.
These mechanisms combine; the rules are in [IAM §9](./IAM.md#9-evaluation-algorithm).

| Mechanism | Gives |
|---|---|
| **User grant** on a tree (`subject_type: "user"`) | One account of any tenant gets a role on that tree. |
| **Tenant grant** on a tree (`subject_type: "tenant"`, v0.5) | Every member (and service account) of another tenant gets a role on that tree. |
| **Policy allow** with `tenant:<slug>`, `user:`, `sa:<tenant>/<name>` or `authenticated` principals | Specific actions on specific resources, optionally conditional. |
| **Policy deny** with the same principals | Removes access, including on public trees and per path. |
| **Visibility** `internal` / `public` | `tree:read` for every signed-in principal, or for everyone (§10). |
| **License rules** with `grantee` | Per-path content rights for non-member tenants. See [License Compliance](../licensing/LicenseCompliance.md). |

Principles:

- **Default deny.** Nothing outside the tenant can see a private tree without
  a grant, an allow policy or a visibility setting.
- **Owners stay in control.** The lockout guard keeps owners able to repair
  IAM ([IAM §10](./IAM.md#10-owner-protection-and-tenant-status)).
- **Server enforcement.** Clients never decide.
- **Dual enforcement.** IAM and license rules must both allow content access.

## 10. Tree visibility

`Tree.Visibility` is set at creation (`POST /tenants/{t}/trees {visibility}`,
`wt trees create --visibility`) and changed with `PATCH` or `wt trees update`
(`tree:admin`):

| Value | Read access beyond roles and grants |
|---|---|
| `private` (default) | none |
| `internal` | every signed-in account and service account |
| `public` | everyone, including anonymous clients |

Visibility never grants write access. See
[Staged Visibility](../visibility/StagedVisibility.md) for what readers of a
tree can see of in-progress work.

## 11. Invitations

Holders of `iam:admin` invite people with `POST /tenants/{t}/invitations
{email? | username?, role, team?}` or `wt invite create T (--email E|--user U)
[--role R] [--team SLUG]`:

- **The code.** The response carries a one-time code `wti_…`, valid for
  7 days and stored hashed. The admin panel turns it into the link
  `/admin/invite/<code>`.
- **Preview.** `GET /invitations/{code}` is public.
- **Accept.** `POST /invitations/{code}/accept` (signed in) adds the
  membership and, if requested, the team membership. When the invitation
  names an email or username, the accepting account must match it. An
  existing higher role is never downgraded.
- **Defaults and limits.** The default role is `developer`. Only owners can
  invite as `owner`.
- **Revoke** with `DELETE /tenants/{t}/invitations/{id}` or
  `wt invite revoke`.

There is no email delivery. The inviter passes the code on.

## 12. REST API and CLI

```
GET    /api/v1/tenants                           -> tenants visible to the caller (all for server admins)
POST   /api/v1/tenants {slug,name}               -> 201 tenant
GET    /api/v1/tenants/{t}                       (tenant:read)
PATCH  /api/v1/tenants/{t} {name?,plan?}         (tenant:admin; plan = server admin)
GET    /api/v1/tenants/{t}/members               (tenant:read)
PUT    /api/v1/tenants/{t}/members/{username} {role} | DELETE   (tenant:admin)
GET    /api/v1/tenants/{t}/usage
GET|PUT /api/v1/admin/tenants/{t}/limits         (server admin)
PATCH  /api/v1/admin/tenants/{t} {status,status_reason}          (server admin)
```

```
wt tenant list | create SLUG [--name N] | show T | rename T NAME | plan T PLAN
wt tenant members T | add-member T USER [--role R] | set-role T USER ROLE | remove-member T USER
wt tenant suspend|resume|deactivate T [--reason R]
wt admin limits T [--max-trees N] [--max-storage SIZE] [--max-branches N] [--max-members N] [--rate N] [--clear]
wt admin usage T
wt remote add NAME URL --tenant T        # binds a worktree remote to a tenant
```

## 13. Security considerations

- **Isolation by authorization.** Tenants share one process, one `meta.db` and
  one object store. Isolation is enforced by authorization on every request,
  and object downloads are scoped to objects reachable from a tree the caller
  can read.
- **Unverified emails.** Account emails are never verified. They are
  therefore not used to resolve tenants or principals, and SSO links an
  identity to an existing account by email only when `link_by_email` is
  enabled. See [Security](../security/Security.md).
- **Reserved names.** Reserved usernames stop accounts from shadowing URL
  segments and system actors. Organization slugs share the slug namespace with
  personal tenants, so a username whose slug is already taken by an
  organization cannot register.
- **Suspension is not revocation.** Suspension stops writes but not
  authentication. To lock a person out, disable their account.

## 14. Implementation status

| Area | Status |
|---|---|
| Personal and organization tenants; registration creating the personal tenant | Built |
| Memberships with built-in and custom roles; owner rules; last-owner guard | Built |
| Teams, service accounts, invitations | Built |
| Plans (label only) | Built |
| Tenant status (`suspended`, `deactivated`) | Built (v0.5) |
| Per-tenant limits, server defaults, usage endpoint, quota enforcement | Built (v0.5) |
| Tenant grants and `tenant:<slug>` principals | Built (v0.5) |
| Tree visibility `private` / `internal` / `public` | Built |
| ABAC attributes on tenants, accounts and trees (`/attributes` endpoints, `wt attr`) | Built (v0.8), see [ABAC](./ABAC.md) |
| Rust `worktree_protocol::iam::tenant` types | Legacy model, not used by the server |

## 15. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **Tenant email and email verification.** A tenant email, verification
  before activation, and resolving tenant references by email.
- **Slug changes** with reservation of old slugs, and reservation after
  deletion.
- **Tenant deletion** with a recovery window and data purge.
- **Plan-driven limits.** Fixed quotas per plan, `max_snapshot_size`, and a
  billing integration. Limits are server defaults plus per-tenant overrides.
- **`shared` visibility** with discovery lists.
- **`[[tenant_access]]` shorthand** in `.wt/config.toml`.
- **Revoking tokens on suspension**, and pausing cross-tenant grants that
  reference a suspended tenant. A suspended grantee tenant keeps the access
  it was granted on other tenants' trees.
- **An org-only distinction for teams and accounts.** Personal tenants can
  have members and teams like organizations.
- **Multi-tenant context switching in the CLI** (`wt tenant switch`). Each
  worktree remote names its tenant (`[remote.<name>] tenant`).

## 16. References

- `services/server-go/internal/domain/types.go` (`Tenant`, `Account`,
  `Membership`, `Tree.Visibility`), `domain/v05.go` (`TenantLimits`, tenant
  status)
- `services/server-go/internal/httpapi/{tenants.go,manage.go,admin.go,ops.go,iam_invitations.go}`
- `services/server-go/internal/iam/{service.go,validate.go,invitations.go}`
- `services/server-go/internal/limits/`, `internal/authz/`
- `crates/worktree-cli/src/commands/{tenant,admin,invite,trees,remote}.rs`
- `docs/ENGINEERING_PLAN.md` §7, §12, §13.3, §13.7
- Related specs: [IAM](./IAM.md), [Declarative Access](./DeclarativeAccess.md),
  [License Compliance](../licensing/LicenseCompliance.md),
  [Server](../server/Server.md)
