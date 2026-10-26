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
| `email` | string or absent | v0.8 contact address, see [Tenant Lifecycle §2](./TenantLifecycle.md#2-contact-email-and-verification) |
| `email_verified_at` | timestamp or absent | v0.8; set once the address is verified with a single-use code |
| `attributes` | map or absent | v0.8 ABAC attributes |

Slug rules:

- **Organization slugs**: 1–63 characters from `[a-z0-9-]`, not starting or
  ending with `-` (`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`).
- **Personal slugs** equal the username (§3) and cannot be changed.
- **Renames** (v0.8): owners change an organization's slug with
  `POST /tenants/{t}/rename` / `wt tenant rename T NEW_SLUG`. The old slug is
  reserved for 90 days and answers `tenant_moved` with the new slug; see
  [Tenant Lifecycle §5](./TenantLifecycle.md#5-slug-renames).
  `wt tenant set-name` changes the display name.
- **Reserved slugs**: slugs of renamed and purged tenants cannot be used by
  new tenants or registrations for 90 days.

A **verified** tenant email names the tenant in REST tenant references, tree
grants and license grantees (`ops@acme.io` for `acme`).

## 3. Accounts

`domain.Account` has the following fields:

| Field | Notes |
|---|---|
| `id` | Account id |
| `username` | `^[a-z0-9][a-z0-9-]{1,38}$`, entered lower-cased. Reserved: `admin`, `anonymous`, `api`, `auth`, `events`, `healthz`, `metrics`, `system`. `admin` can be claimed only by the configured server admin. |
| `email` | Syntax-checked; verified on request with a single-use code (`POST /me/email/resend`, `POST /me/email/verify`, v0.8) |
| `email_verified_at` | When the email was verified (v0.8) |
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
| Display name | `PATCH /tenants/{t} {name}` / `wt tenant set-name T NAME` (`tenant:admin`) |
| Contact email | `PATCH /tenants/{t} {email}` / `wt tenant email set\|verify\|resend\|clear` (`tenant:admin`; verification is public with the code, v0.8) |
| Change slug | `POST /tenants/{t}/rename {slug}` / `wt tenant rename T NEW_SLUG` (`tenant:admin` + owner, v0.8) |
| Change plan | `PATCH /tenants/{t} {plan}` / `wt tenant plan T free\|pro\|enterprise` (server admin) |
| Suspend / resume / deactivate | `PATCH /admin/tenants/{t} {status, status_reason}` / `wt tenant suspend\|resume\|deactivate T [--reason]` (server admin, v0.5) |
| Set limits | `PUT /admin/tenants/{t}/limits` / `wt admin limits T ...` (server admin, v0.5) |
| Delete | `DELETE /tenants/{t}` / `wt tenant delete T` (owner, v0.8): suspended at once, purged after a 30-day recovery window. Personal tenants cannot be deleted. See [Tenant Lifecycle §6](./TenantLifecycle.md#6-deletion-recovery-window-and-restore). |
| Restore | `POST /tenants/{t}/restore` / `wt tenant restore T` (owner, during the recovery window, v0.8) |

`tenant.create` and `tenant.update` (display name, plan, status and limits)
are audited, as are the v0.8 lifecycle actions (`tenant.email`,
`tenant.email_verified`, `tenant.rename`, `tenant.delete`, `tenant.restore`,
`tenant.purge`). Renames publish `tenant.renamed`, purges `tenant.deleted`.

## 7. Tenant status

| Status | Effect |
|---|---|
| `active` (or `""`) | Normal |
| `suspended` | Read-only. Authorization denies every action other than `tenant:read`, `tree:read` and `iam:read`, with reason `tenant_suspended`. |
| `deactivated` | Authorization denies every action on the tenant and its trees. |

Status changes follow these rules:

- **Server admins** are never affected.
- **Service-account keys are revoked** (v0.8) when an active tenant is
  suspended or deactivated, by a server admin or by a scheduled deletion.
  Account sessions and tokens are not touched, no data is deleted and no
  membership changes. `resume` restores access immediately, but revoked keys
  must be reissued.
- **Tenant grants are paused** (v0.8): tree grants with `subject_type:
  "tenant"` that name a suspended or deactivated tenant give nothing until it
  is resumed.
- **Scheduled deletion** (v0.8) suspends the tenant with the reason
  `scheduled for deletion on <date>`; restoring it brings back the previous
  status.
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
PATCH  /api/v1/tenants/{t} {name?,plan?,email?} (tenant:admin; plan = server admin)
GET    /api/v1/tenants/{t}/members               (tenant:read)
PUT    /api/v1/tenants/{t}/members/{username} {role} | DELETE   (tenant:admin)
GET    /api/v1/tenants/{t}/usage
GET|PUT /api/v1/admin/tenants/{t}/limits         (server admin)
PATCH  /api/v1/admin/tenants/{t} {status,status_reason}          (server admin)
# v0.8 lifecycle (TenantLifecycle.md §9)
POST   /api/v1/tenants/{t}/email/verify {token} | /email/resend
POST   /api/v1/me/email/verify {token} | /me/email/resend
POST   /api/v1/tenants/{t}/rename {slug}         (tenant:admin + owner)
DELETE /api/v1/tenants/{t}                       (owner) -> 202 TenantDeletion
GET    /api/v1/tenants/{t}/deletion | POST /api/v1/tenants/{t}/restore
```

Requests naming a renamed tenant's old slug answer `301 tenant_moved` with
`error.details.new_slug` (REST) or `NotFound` with a `TENANT_MOVED`
`ErrorInfo` (gRPC).

```
wt tenant list | create SLUG [--name N] | show T | set-name T NAME | plan T PLAN
wt tenant rename T NEW_SLUG | delete T [--yes] | restore T
wt tenant email set T EMAIL | verify T CODE | resend T | clear T
wt tenant switch SLUG [--remote R]       # sets remote.<R>.tenant of the current worktree
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
- **Verified emails only.** Only *verified* tenant emails resolve to a
  tenant (v0.8); a verified address belongs to one tenant at most. Account
  emails can be verified (v0.8) but are not used to resolve principals, and
  SSO links an identity to an existing account by email only when
  `link_by_email` is enabled. See [Security](../security/Security.md).
- **Reserved names.** Reserved usernames stop accounts from shadowing URL
  segments and system actors. Organization slugs share the slug namespace with
  personal tenants, so a username whose slug is already taken by an
  organization — or reserved by a renamed or purged one — cannot register.
- **Suspension revokes automation, not people.** Suspension stops writes and
  revokes service-account keys, but account sessions keep working (read
  only). To lock a person out, disable their account.

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
| Tenant email and verification, account email verification, references by verified email | Built (v0.8, [Tenant Lifecycle](./TenantLifecycle.md)) |
| Slug renames with 90-day reservations; reservation after purge; `tenant_moved` | Built (v0.8) |
| Deletion with a recovery window, restore, purge job | Built (v0.8) |
| Service-account key revocation and paused tenant grants on suspension | Built (v0.8) |
| `wt tenant switch` | Built (v0.8) |
| ABAC attributes on tenants, accounts and trees (`/attributes` endpoints, `wt attr`) | Built (v0.8), see [ABAC](./ABAC.md) |
| Rust `worktree_protocol::iam::tenant` types | Legacy model, not used by the server |

## 15. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **Verification before activation.** Tenants are usable at once; the
  contact email is optional and verified independently. Email references in
  policies and declarative access documents are listed in
  [Tenant Lifecycle §16](./TenantLifecycle.md#16-roadmap).
- **Plan-driven limits.** Fixed quotas per plan, `max_snapshot_size`, and a
  billing integration. Limits are server defaults plus per-tenant overrides.
- **`shared` visibility** with discovery lists.
- **`[[tenant_access]]` shorthand** in `.wt/config.toml`.
- **Revoking account sessions on suspension.** Only service-account keys are
  revoked; `tenant:<slug>` policy principals of a suspended tenant still
  match (only tenant *grants* are paused).
- **An org-only distinction for teams and accounts.** Personal tenants can
  have members and teams like organizations.
- **A global current tenant in the CLI.** `wt tenant switch` changes the
  current worktree's remote; commands outside a worktree take the tenant as an
  argument.

## 16. References

- `services/server-go/internal/domain/types.go` (`Tenant`, `Account`,
  `Membership`, `Tree.Visibility`), `domain/v05.go` (`TenantLimits`, tenant
  status)
- `services/server-go/internal/httpapi/{tenants.go,manage.go,admin.go,ops.go,iam_invitations.go}`
- `services/server-go/internal/iam/{service.go,validate.go,invitations.go,tenant_lifecycle*.go}`
- `services/server-go/internal/httpapi/tenant_lifecycle.go`
- `services/server-go/internal/limits/`, `internal/authz/`
- `crates/worktree-cli/src/commands/{tenant,admin,invite,trees,remote}.rs`
- `docs/ENGINEERING_PLAN.md` §7, §12, §13.3, §13.7, §16.4
- Related specs: [Tenant Lifecycle](./TenantLifecycle.md), [IAM](./IAM.md), [Declarative Access](./DeclarativeAccess.md),
  [License Compliance](../licensing/LicenseCompliance.md),
  [Server](../server/Server.md)
