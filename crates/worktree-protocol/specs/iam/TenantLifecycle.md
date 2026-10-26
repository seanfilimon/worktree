# Tenant Lifecycle Specification

## Table of Contents

1. [Overview](#1-overview)
2. [Contact email and verification](#2-contact-email-and-verification)
3. [Account email verification](#3-account-email-verification)
4. [References by verified email](#4-references-by-verified-email)
5. [Slug renames](#5-slug-renames)
6. [Deletion, recovery window and restore](#6-deletion-recovery-window-and-restore)
7. [The purge job](#7-the-purge-job)
8. [Suspension effects](#8-suspension-effects)
9. [REST API](#9-rest-api)
10. [gRPC](#10-grpc)
11. [CLI](#11-cli)
12. [Admin panel](#12-admin-panel)
13. [Documents, events and audit](#13-documents-events-and-audit)
14. [Security considerations](#14-security-considerations)
15. [Implementation status](#15-implementation-status)
16. [Roadmap](#16-roadmap)
17. [References](#17-references)

---

## 1. Overview

v0.8 (`docs/ENGINEERING_PLAN.md` §16.4) gives organization tenants a full
lifecycle on top of the model in [Tenant Model](./TenantModel.md):

| Stage | What happens |
|---|---|
| Contact email | A tenant admin sets `Tenant.email`; a single-use code proves the mailbox. A verified address names the tenant in grants and license rules. |
| Rename | An owner changes the slug. The old slug is reserved for 90 days and answers "tenant moved" with the new slug. |
| Delete | An owner schedules the deletion. The tenant is suspended (read-only) and its service-account keys are revoked for a recovery window of 30 days. |
| Restore | During the window an owner cancels the deletion. |
| Purge | After the window a background job deletes the tenant's data and the tenant record; the slug stays reserved for 90 days. |

Personal tenants (`kind: "user"`, slug = username) cannot be renamed or
deleted. The service is `iam.TenantLifecycle`; the HTTP layer authorizes,
the service validates, stores, publishes events and runs the purge job.

## 2. Contact email and verification

`Tenant.email` is the tenant's contact address and
`Tenant.email_verified_at` the time it was verified (absent = unverified).

- **Set.** `PATCH /tenants/{t} {email}` (`tenant:admin`). The address must be
  a bare `local@domain` address (`iam.ValidateEmail`). `""` clears it.
  Setting the current address again changes nothing. Any change clears
  `email_verified_at`, deletes outstanding codes of the tenant and issues a
  new code.
- **Code.** `wte_` followed by 32 random bytes (base64url). Only its sha256
  is stored (`email_tokens`). A code is valid for **24 hours** and **single
  use**; it is bound to the tenant and to the address it was sent to, so a
  code stops working when the address changes. Issuing a new code (resend)
  invalidates the earlier ones.
- **Delivery.** The code goes out through the notification mailer
  (`--smtp-url`, see [Notifications](../server/Notifications.md)) as a
  plain-text + HTML email with the CLI command and, when `--public-url` is
  set, the link `<public-url>/admin/tenants/<slug>?tab=settings&email_token=<code>`.
  Without SMTP the code is written to the server log at level INFO
  (`msg="email verification (SMTP is not configured; the code is logged
  instead)" subject=tenant name=<slug> email=<address> token=<code>`), and
  the API reports `delivery: "log"`.
- **Verify.** `POST /tenants/{t}/email/verify {token}` is public: the code
  proves control of the mailbox. Every unusable code (malformed, unknown,
  expired, used, for another tenant or another address) answers the same
  `400 invalid_email_token`.
- **Uniqueness.** A verified address belongs to at most one tenant: verifying
  an address another tenant already verified answers `409 email_taken`.
- **Resend.** `POST /tenants/{t}/email/resend` (`tenant:admin`) →
  `202 {email, delivery, expires_at}`; `409 no_email` without an address,
  `409 email_already_verified` when verified.

## 3. Account email verification

Accounts use the same mechanism (subject `account`):

- `POST /me/email/resend` (signed in) mails a code to the account's email
  (or logs it) → `202 {email, delivery, expires_at}`;
  `409 email_already_verified`.
- `POST /me/email/verify {token}` sets `Account.email_verified_at` and
  returns the account. Tenant codes do not verify accounts and vice versa.
- Service accounts have no email (`403`).

Account emails cannot be changed through the API, so a verification stays
valid for the account's lifetime.

## 4. References by verified email

A verified tenant email can stand for the tenant wherever a tenant is named
by reference (`iam.ResolveTenantRef`: slug, then id, then verified email):

| Where | Example |
|---|---|
| REST `{tenant}` path segments | `GET /tenants/ops@acme.io` |
| Tree grants with `subject_type: "tenant"` | `PUT {tree}/grants {"subject_type":"tenant","subject":"ops@acme.io","role":"viewer"}` stores the tenant's id and slug |
| License-rule grantees | `PUT {tree}/licenses {"grantee":"ops@acme.io",...}` stores the slug |

Unverified addresses never resolve. Stored references keep the tenant id (or
slug, rewritten on renames), so changing the email later does not change
existing grants.

## 5. Slug renames

`POST /tenants/{t}/rename {slug}` needs `tenant:admin` **and** the owner role
(server admins may always). Rules:

- The new slug follows the organization slug rules
  (`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`) and must differ from the current
  one.
- It must not be used by another tenant or an account's username
  (`409 slug_taken`), nor reserved for another tenant (`409 slug_reserved`).
  A tenant may rename back to one of its own reserved slugs, which frees that
  reservation.
- Personal tenants: `409 personal_tenant`. A tenant scheduled for deletion:
  `409 deletion_scheduled`.

Effects:

- **Reservation.** The old slug is stored in `tenant_slugs` for **90 days**
  (`{slug, tenant_id, renamed_to, reserved_at, expires_at}`). Until it
  expires no tenant can be created with it, no account can register it as a
  username, and other tenants cannot rename to it.
- **Moved answers.** REST requests naming the old slug answer
  `301 tenant_moved` (§9), gRPC `ResolveTree` answers `NotFound` with the new
  slug (§10). Reservations of chained renames (a → b → c) all point at the
  tenant, so every old slug reports the current one.
- **Rewritten references.** Everything that stores the slug by value is
  rewritten: resources `wt:tenant/<slug>`, `wt:tree/<slug>/…`,
  `wt:branch/<slug>/…` and `wt:path/<slug>/…` of the tenant's own policies,
  `tenant:<slug>` principals in every tenant's policies, license-rule
  grantees on every tree, and the display name of tenant grants. Tree ids,
  tenant ids and memberships do not change, so clones keep working once their
  remote is switched.
- `tenant.renamed` is published (payload `{old_slug, new_slug}`) and the
  rename is audited as `tenant.rename`.

## 6. Deletion, recovery window and restore

`DELETE /tenants/{t}` (owners and server admins) → `202 TenantDeletion`:

```json
{"tenant_id":"…","slug":"acme","requested_by":"olga","requested_at":"…",
 "purge_at":"…","previous_status":"","revoked_keys":2}
```

- The deletion is recorded in `tenant_deletions` (key tenant id) with
  `purge_at = now + --tenant-deletion-window` (default **30 days**).
- The tenant becomes `suspended` with the reason
  `scheduled for deletion on <date>`: every write (pushes, merges, settings,
  IAM changes) is refused, reads keep working ([Tenant Model §7](./TenantModel.md#7-tenant-status)).
- The tenant's service-account keys are revoked (`revoked_keys`); they are not
  restored.
- A second request answers `409 deletion_scheduled`; personal tenants
  `409 personal_tenant`.
- `GET /tenants/{t}/deletion` (`tenant:read`) returns the record (for
  countdowns) or `404 deletion_not_scheduled`.

`POST /tenants/{t}/restore` (owners and server admins) cancels the deletion
during the window: the tenant gets back the status and reason it had before
(normally active), the record is removed. Owner checks for delete and
restore do not depend on the tenant's status, so owners can restore a
suspended tenant; the caller's token scopes must still permit
`tenant:admin`.

## 7. The purge job

`TenantLifecycle.Start(ctx)` runs every `--tenant-purge-interval` (default 1
hour). Each run (`RunOnce`):

1. Purges every tenant whose `purge_at` has passed:
   - deletes its trees (branches, snapshots, tags, staged snapshots,
     protections, merge requests, license rules and grants go with each tree)
     and publishes `tree.deleted` for each, so owners of tree-scoped documents
     clean up and garbage collection reclaims the objects;
   - deletes its teams, service accounts (with their keys and grants),
     policies, invitations, memberships and custom roles;
   - deletes tenant grants naming it on other tenants' trees;
   - deletes its `access_docs`, `linked_groups`, `tenant_usage` and
     verification codes;
   - reserves its final slug for 90 days (`tenant_slugs`, no `renamed_to`);
   - deletes the tenant record (`DeleteTenant`, implemented by the meta
     stores; a store without it leaves a deactivated record);
   - removes the deletion record, publishes `tenant.deleted`
     (`{slug, trees}`) and audits `tenant.purge` (actor `system`).
2. Drops expired verification codes and expired slug reservations.

A failed purge leaves the deletion record in place and is retried on the next
run; every step is idempotent.

## 8. Suspension effects

Whenever a tenant becomes suspended or deactivated — by a deletion or by a
server admin (`PATCH /admin/tenants/{t} {status}`) — from the active state:

- **Service-account keys are revoked** (audited as
  `service_account.keys_revoked` for admin status changes). Resuming does not
  bring them back; new keys must be issued.
- **Cross-tenant grants naming it are paused.** A tree grant with
  `subject_type: "tenant"` whose tenant is suspended or deactivated gives
  nothing while that lasts; the decision explains it ("tenant acme grant on
  tree "web" is paused: tenant acme is suspended"). Resuming restores the
  grants, since nothing is deleted.

## 9. REST API

```
PATCH  /api/v1/tenants/{t} {email}            -> tenant              (tenant:admin; "" clears)
POST   /api/v1/tenants/{t}/email/verify {token} -> tenant            (public)
POST   /api/v1/tenants/{t}/email/resend       -> 202 VerificationSent (tenant:admin)
POST   /api/v1/me/email/resend                -> 202 VerificationSent (signed in)
POST   /api/v1/me/email/verify {token}        -> account             (signed in)
POST   /api/v1/tenants/{t}/rename {slug}      -> {tenant, reservation} (tenant:admin + owner)
DELETE /api/v1/tenants/{t}                    -> 202 TenantDeletion  (owner)
GET    /api/v1/tenants/{t}/deletion           -> TenantDeletion | 404 deletion_not_scheduled (tenant:read)
POST   /api/v1/tenants/{t}/restore            -> tenant              (owner)
```

Error codes: `invalid_email_token` (400), `no_email`,
`email_already_verified`, `email_taken`, `slug_taken`, `slug_reserved`,
`personal_tenant`, `deletion_scheduled` (409), `deletion_not_scheduled`
(404). Without the lifecycle service the routes answer `501
not_implemented`.

**Moved slugs.** Any route with a `{tenant}` path segment that names a
reserved old slug answers:

```
HTTP/1.1 301 Moved Permanently
Cache-Control: no-store
X-WT-Tenant-Moved: acme-inc

{"error":{"code":"tenant_moved","message":"tenant \"acme\" was renamed to \"acme-inc\"; …",
          "details":{"old_slug":"acme","new_slug":"acme-inc"}}}
```

No `Location` header is sent, so HTTP clients do not follow it (a followed
redirect would turn a POST into a GET). `error.details` is the general
machine-readable extension of the error envelope
([REST API](../server/RestApi.md)). The SDK maps it to
`SdkError::TenantMoved {old, new}`.

## 10. gRPC

`ResolveTree` (and access-config sync by tenant slug) for a reserved old slug
answers `NotFound` with:

- the message `tenant "acme" was renamed to "acme-inc"; update the remote (wt
  tenant switch acme-inc)`;
- a `google.rpc.ErrorInfo` detail: reason `TENANT_MOVED`, domain `worktree`,
  metadata `old_slug`, `new_slug`;
- the trailers `wt-tenant-moved-from` and `wt-tenant-moved-to`.

Calls that use tree ids keep working after a rename. The SDK reads the
trailers (falling back to the message) and returns
`SdkError::TenantMoved`, which the CLI prints with the hint to run
`wt tenant switch NEW`.

## 11. CLI

```
wt tenant rename T NEW_SLUG               # slug (owners); prints the reservation and the switch hint
wt tenant set-name T NAME                 # display name (was `wt tenant rename T NAME` before v0.8)
wt tenant delete T [--yes]                # schedules the deletion; prints purge_at
wt tenant restore T
wt tenant email set T EMAIL | verify T CODE | resend T | clear T
wt tenant switch SLUG [--remote R]        # sets remote.<R>.tenant in .wt/config.toml (default [sync].remote)
wt tenant show T                          # includes the email (verified/unverified) and a scheduled deletion
```

`wt tenant rename` with a value that is not a slug fails with a hint to use
`set-name`. `wt tenant switch` works offline; it changes only the worktree
configuration. `delete` asks for confirmation (`--yes` when stdin is not a
terminal).

## 12. Admin panel

Tenant → Settings (`apps/admin/src/pages/tenants/`):

- **Contact email** card: status (verified / unverified badge), set or remove
  the address, verify with the code (pre-filled from the `email_token` query
  parameter of the email link), resend.
- **Change slug** dialog from the General card (owners and server admins of
  active organization tenants), listing the consequences.
- **Danger zone**: delete with type-the-slug confirmation; while a deletion
  is scheduled, a live countdown to the purge, who requested it and a Restore
  button. Personal tenants show why they cannot be deleted.
- Opening an old slug shows "This tenant was renamed" with a link to the new
  slug.

## 13. Documents, events and audit

| Collection | Key | Value |
|---|---|---|
| `email_tokens` | hex sha256 of the code | `{subject, subject_id, email, created_by, created_at, expires_at, used_at, _key}` |
| `tenant_slugs` | the reserved slug | `SlugReservation` |
| `tenant_deletions` | tenant id | `TenantDeletion` |

Events: `tenant.renamed` (`{old_slug, new_slug}`), `tenant.deleted`
(`{slug, trees}`, at purge), plus `tree.deleted` for every purged tree.

Audit actions: `tenant.email`, `tenant.email_verified`,
`tenant.email_verification_sent`, `account.email_verified`,
`account.email_verification_sent`, `tenant.rename`, `tenant.delete`,
`tenant.restore`, `tenant.purge`, `service_account.keys_revoked`.

Server flags: `--tenant-deletion-window` (`WT_TENANT_DELETION_WINDOW`,
default `720h`), `--tenant-purge-interval` (`WT_TENANT_PURGE_INTERVAL`,
default `1h`).

## 14. Security considerations

- **Codes** are 256-bit, stored hashed, single use, short-lived and bound to
  the address. Redemption is atomic (`UpdateDoc`). Logging codes is the
  documented fallback without SMTP; operators who log to shared systems
  should configure SMTP.
- **Slug squatting.** Reservations stop others from taking over a renamed or
  purged tenant's slug (and with it clones, remotes and links that still use
  it) for 90 days.
- **Least surprise on renames.** Policy denies keep applying after a rename
  because their resources are rewritten in the same operation.
- **Deletion is reversible** for the window; the purge is not. Suspension
  revokes service-account keys at once because automation is the main risk
  of a tenant that is being shut down.
- **Owner checks** respect token scopes: a scoped token without
  `tenant:admin` cannot rename, delete or restore.

## 15. Implementation status

| Area | Status |
|---|---|
| Tenant email, verification codes (24 h, single use), SMTP or log delivery | Built (v0.8 §16.4) |
| Account email verification (`/me/email/*`) | Built (v0.8 §16.4) |
| Verified emails in REST tenant references, tenant grants and license grantees | Built (v0.8 §16.4) |
| Slug renames, 90-day reservations, `tenant_moved` (REST 301, gRPC ErrorInfo + trailers), reference rewriting | Built (v0.8 §16.4) |
| Deletion with recovery window, restore, purge job, slug reservation after purge | Built (v0.8 §16.4) |
| Key revocation and paused tenant grants on suspension | Built (v0.8 §16.4) |
| CLI (`rename`, `set-name`, `delete`, `restore`, `email`, `switch`) and admin settings | Built (v0.8 §16.4) |

## 16. Roadmap

- **Email references in policies and declarative access**
  (`tenant:ops@acme.io` principals, `{ tenant = "ops@acme.io" }` in
  `.wt/access/*.toml`). Policies use slugs.
- **Verified account emails for SSO linking and invitations.** SSO still
  links by email only with `link_by_email`, and email invitations do not
  require a verified address.
- **Changing an account's email** (with re-verification).
- **Automatic remote updates.** Clients print a hint; `wt tenant switch` is
  explicit. The background daemon does not rewrite remotes.
- **Personal tenant deletion** together with the account.
- **Configurable reservation period** and early release of a reservation by
  a server admin.

## 17. References

- `services/server-go/internal/iam/{tenant_lifecycle.go,tenant_lifecycle_rename.go,tenant_lifecycle_delete.go}`
- `services/server-go/internal/httpapi/tenant_lifecycle.go` (+ small hooks in
  `helpers.go` `tenantByRef`, `manage.go` PATCH email, `tenants.go`,
  `trees.go`, `respond.go` error details)
- `services/server-go/internal/syncsvc/tenant_moved.go`,
  `internal/authz/authorizer.go` (paused tenant grants),
  `internal/meta/tenant_delete.go`, `internal/notify/direct.go`,
  `internal/config/tenants.go`
- `crates/worktree-cli/src/commands/tenant.rs`,
  `crates/worktree-sdk/src/remote/{client.rs,auth.rs}` (`SdkError::TenantMoved`)
- `apps/admin/src/api/tenantLifecycle.ts`,
  `apps/admin/src/pages/tenants/{TenantEmailCard,TenantRenameDialog,TenantDangerZone,TenantMovedNotice}.tsx`
- `crates/worktree-e2e/tests/e2e/tenant_lifecycle.rs`
- `docs/ENGINEERING_PLAN.md` §16.4; related: [Tenant Model](./TenantModel.md),
  [IAM](./IAM.md), [Notifications](../server/Notifications.md)
