# W0rkTree IAM Specification

## Table of Contents

1. [Overview](#1-overview)
2. [Identities and principals](#2-identities-and-principals)
3. [Action catalog](#3-action-catalog)
4. [Roles](#4-roles)
5. [Teams](#5-teams)
6. [Tree grants](#6-tree-grants)
7. [Tree visibility](#7-tree-visibility)
8. [Policies](#8-policies)
9. [Evaluation algorithm](#9-evaluation-algorithm)
10. [Owner protection and tenant status](#10-owner-protection-and-tenant-status)
11. [Tokens, scopes and authentication](#11-tokens-scopes-and-authentication)
12. [Inspecting access: simulator and effective permissions](#12-inspecting-access-simulator-and-effective-permissions)
13. [Where decisions are enforced](#13-where-decisions-are-enforced)
14. [REST API](#14-rest-api)
15. [CLI](#15-cli)
16. [Audit](#16-audit)
17. [Design rationale](#17-design-rationale)
18. [Implementation status](#18-implementation-status)
19. [Roadmap](#19-roadmap)
20. [References](#20-references)

---

## 1. Overview

W0rkTree has identity and access management built into the server. You do not
add it through a hosting platform. Every request made over REST or gRPC is
authorized by one engine, `internal/authz` in the Go server
(`services/server-go`). The engine evaluates a triple
*(principal, action, resource)* and returns a `Decision` that explains its
reasoning.

The building blocks are:

| Concept | What it is | Stored as |
|---|---|---|
| **Account** | A human user with a password, optional TOTP and optional SSO links. | `domain.Account` |
| **Tenant** | A personal (`user`) or organization (`org`) namespace that owns trees. See [Tenant Model](./TenantModel.md). | `domain.Tenant` |
| **Membership** | An account's role in a tenant. | `domain.Membership` |
| **Team** | A named group of accounts inside one tenant. | `domain.Team`, `domain.TeamMember` |
| **Role** | A built-in role (`owner`, `admin`, `maintainer`, `developer`, `viewer`) or a tenant-defined custom role. | `domain.CustomRole` (custom only) |
| **Grant** | A role on one tree, given to a user, team, service account or tenant. | `domain.Grant` |
| **Service account** | A non-human identity owned by a tenant. It authenticates with keys. | `domain.ServiceAccount` |
| **Policy** | A named, versioned list of allow and deny statements with conditions, scoped to one tenant. | `domain.Policy` |
| **Token** | A bearer credential: a sign-in session, a personal API token or a service-account key, optionally limited by scopes. | `domain.APIToken` |

You can manage all of these from the admin panel (`/admin/`), the REST API
(`/api/v1`) and the `wt` CLI. The user-facing guide is `docs/iam.md`.

Policies, roles and grants are stored on the server, in `meta.db`. They can be
edited through the API. They can also be written as TOML files and applied with
`wt access apply`; see [Declarative Access](./DeclarativeAccess.md).

## 2. Identities and principals

Each authenticated request carries a `domain.Principal`, filled in by
`iam.Service.Authenticate`:

| Field | Users | Service accounts |
|---|---|---|
| `AccountID` | account id | `""` |
| `Username` | username | `sa:<tenant>/<name>` |
| `TenantID` | the personal tenant | the owning tenant |
| `Kind` | `user` (or `""`) | `service_account` |
| `ServiceAccountID` | `""` | service-account id |
| `IsAdmin` | `Account.IsAdmin` (server admin) | `false` |
| `Scopes` | token scopes (empty means unrestricted) | key scopes |
| `MFA` | whether the session was created after a second factor | `false` |
| `TokenID` | id of the presented token | id of the key |

A request with no credential is **anonymous**: its principal is `nil`.
Anonymous callers may call `Ping` and read public trees, plus anything a
policy explicitly allows to `anonymous`.

The engine does not trust the principal it receives. It re-reads the account
or service account from the store on every evaluation, so disabling an
identity or changing admin status takes effect on the next request. An unknown
account or service account counts as unauthenticated.

**Server admins** (`Account.IsAdmin`) are created with `--admin-user` /
`--admin-password` (`iam.EnsureAdmin`) or promoted with
`PATCH /admin/accounts/{username}`. They are allowed every action, and roles
and policies are not consulted for them. Token scopes still restrict them.

**Service accounts** belong to one tenant and act there with their own `Role`.
That role can be any role except `owner`. A service account also holds any
tree grants made to it. Outside its tenant, a service account has only what
grants, visibility and policies give it.

## 3. Action catalog

Every authorization check names one **action** from a fixed catalog
(`authz.Actions()`, served by `GET /api/v1/iam/actions` and `wt iam actions`).
An action has the form `<area>:<verb>`. Policies, custom roles and token
scopes can also use the patterns `<area>:*` and `*`.

| Action | Area | Meaning | Lowest built-in role |
|---|---|---|---|
| `tenant:read` | tenant | View the tenant, its members and its trees | viewer |
| `tenant:admin` | tenant | Manage tenant members and settings | admin |
| `tree:read` | tree | Read a tree: browse, clone, pull, download | viewer |
| `tree:write` | tree | Write to a tree: push, stage, tag | developer |
| `tree:create` | tree | Create trees in the tenant | maintainer |
| `tree:admin` | tree | Administer a tree: settings, protections, licenses, deletion | admin |
| `branch:create` | branch | Create branches | developer |
| `branch:delete` | branch | Delete branches | maintainer |
| `branch:push` | branch | Push to a specific branch (needs `tree:write`; policies may deny it per branch) | developer |
| `mr:create` | mr | Open merge requests | developer |
| `mr:approve` | mr | Approve merge requests | maintainer |
| `mr:merge` | mr | Merge and close merge requests | maintainer |
| `release:create` | release | Create and edit releases, upload release assets (v0.5) | developer |
| `release:delete` | release | Delete releases and release assets (v0.5) | maintainer |
| `ci:report` | ci | Report CI check results for snapshots (v0.5) | developer |
| `iam:read` | iam | View teams, roles, policies, grants, service accounts | maintainer |
| `iam:admin` | iam | Manage teams, roles, policies, grants, service accounts, invitations | admin |
| `server:admin` | server | Administer the server | server admins only |

Several operations have no action of their own and reuse the catalog:

- Tags are created with `tree:write` and deleted with `tree:admin`.
- Branch protection, tag protection and license rules are managed with
  `tree:admin`.
- `mr:merge` also covers closing merge requests.
- Snapshots are created locally. On the server, a snapshot exists only once it
  has been pushed (`tree:write` and `branch:push`) or staged (`tree:write`).

**Tree-scoped actions** are the `tree:read`, `tree:write`, `tree:admin`,
`branch:*`, `mr:*`, `release:*` and `ci:report` actions. Only these can be
granted by tree grants (§6).

## 4. Roles

### 4.1 Built-in roles

The five built-in roles form a strict superset hierarchy:
`owner ⊃ admin ⊃ maintainer ⊃ developer ⊃ viewer`. A built-in role grants an
action when it ranks at or above the action's lowest role, shown in the table
in §3 (`authz.RoleAllows`):

| Action | viewer | developer | maintainer | admin | owner |
|---|:-:|:-:|:-:|:-:|:-:|
| `tenant:read`, `tree:read` | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tree:write`, `branch:create`, `branch:push`, `mr:create`, `release:create`, `ci:report` | | ✓ | ✓ | ✓ | ✓ |
| `branch:delete`, `mr:approve`, `mr:merge`, `tree:create`, `iam:read`, `release:delete` | | | ✓ | ✓ | ✓ |
| `tree:admin`, `tenant:admin`, `iam:admin` | | | | ✓ | ✓ |
| `server:admin` | | | | | |

`owner` grants the same actions as `admin`. What sets owners apart is
enforced by the handlers and by the lockout guard (§10):

- Only owners and server admins can assign or revoke the `owner` role,
  whether through memberships, grants or invitations.
- A tenant must always keep at least one owner.
- Owners are exempt from the declarative-access escalation check and may use
  `?force=true` on resources managed by config (see
  [Declarative Access](./DeclarativeAccess.md)).
- Policy denies never take `iam:*` or `tenant:admin` away from an owner.

### 4.2 Custom roles

A tenant may define custom roles (`domain.CustomRole`):

- **Name**: 2–40 characters from `[a-z0-9-]`, starting with a letter or digit,
  unique in the tenant, and not the name of a built-in role.
- **Permissions**: catalog actions or `<area>:*`. A role may not contain `*`
  and may not grant `server:*` (`authz.ValidatePermissions`).
- **Assignment**: a custom role is assigned exactly like a built-in one. It
  can be a membership role, a grant role, a service-account role or an
  invitation role.
- **Deletion**: `DELETE /tenants/{t}/roles/{name}` answers `409` while the
  role is still assigned.
- **Undefined roles**: a role name that is neither built-in nor defined in
  the tenant grants nothing, and the decision says so.
- **Config-managed roles**: roles created by `wt access apply` carry
  `managed_by: "config"` and are read-only in the REST API (§14).

`GET /tenants/{t}/roles` lists the built-in roles first (read-only, with
their expanded permissions), followed by the custom roles.

## 5. Teams

A team (`domain.Team`) groups accounts inside one tenant:

- **Slug**: 2–40 characters from `[a-z0-9-]`.
- **Name and description**: free text.
- **Member roles**: `maintainer` or `member`. A team maintainer may manage the
  team's membership without holding `iam:admin`.

A team has **no tenant-wide role**. Its access comes from:

- **Tree grants** made to the team (`subject_type: "team"`, §6).
- **Policy statements** naming `team:<slug>` (§8).

Deleting a team removes its memberships and grants.

## 6. Tree grants

A grant (`domain.Grant`) gives a subject a role on **one tree**. Each subject
has at most one grant per tree, and `PUT` replaces it.

| `subject_type` | `subject` | Matches |
|---|---|---|
| `user` | username (any account on the server) | that account |
| `team` | team slug in the tree's tenant | every member of the team |
| `service_account` (alias `sa`) | service-account name in the tree's tenant | that service account |
| `tenant` (v0.5) | tenant slug | every member of that tenant, and its service accounts |

The role can be built-in or a custom role of the tree's tenant. Only owners
can grant `owner`.

Grants have three properties:

- **Tree-scoped actions only.** A grant covers only the actions listed at the
  end of §3, so it never gives `tenant:*` or `iam:*` access.
- **They can raise access.** A grant can lift a subject above its tenant role
  on one tree. It can also give access to accounts outside the tenant, which
  is how cross-tenant collaboration works.
- **They never lower access.** To restrict access, use a policy deny (§8).

## 7. Tree visibility

`Tree.Visibility` is one of three values:

| Visibility | Anonymous | Any signed-in principal | Tenant members |
|---|---|---|---|
| `private` (default) | — | — | per role |
| `internal` | — | `tree:read` | per role |
| `public` | `tree:read` | `tree:read` | per role |

An unknown or empty visibility is treated as `private`.

Visibility widens `tree:read` and nothing else. It does not grant
`tenant:read`. A policy deny still overrides it: for example, a policy can
deny `tree:read` on a public tree to one team, or on one path to everyone.

## 8. Policies

### 8.1 Structure

A policy (`domain.Policy`) belongs to one tenant. It has these fields:

- `name` and `description`;
- `enabled`: only enabled policies are evaluated;
- `version`: incremented on every update;
- `created_by` and timestamps;
- `managed_by`: `"config"` when the policy came from `wt access apply`;
- `statements`: the list described below.

Each statement (`domain.PolicyStatement`) has these fields:

```json
{
  "sid": "no-contractor-pushes-to-main",
  "effect": "deny",
  "actions": ["branch:push"],
  "resources": ["wt:branch/acme/*/main"],
  "principals": ["team:contractors", "tenant:partner"],
  "conditions": { "require_mfa": true, "ip_ranges": ["10.0.0.0/8"] }
}
```

| Field | Values |
|---|---|
| `sid` | Optional statement id, at most 128 characters. Used in decision reasons. |
| `effect` | `allow` or `deny` |
| `actions` | Non-empty list of catalog actions, `<area>:*` or `*` |
| `resources` | Non-empty list of resource patterns (§8.2) |
| `principals` | Non-empty list of principal patterns (§8.3) |
| `conditions` | Optional; every condition that is set must hold (§8.4) |

`authz.ValidateStatements` checks every statement before it is stored. It
reports each problem with its statement index and field, for example
`actions[1]` or `conditions.time_window.timezone`.
`POST /tenants/{t}/policies/validate` and `wt policy validate` expose the same
check.

### 8.2 Resources

| Pattern | Covers |
|---|---|
| `*` | everything in the tenant |
| `wt:tenant/<tenant>` | the tenant and every tree in it |
| `wt:tree/<tenant>/<tree>` | the tree, its branches and its paths |
| `wt:branch/<tenant>/<tree>/<branch>` | one branch (branch names may contain `/`) |
| `wt:path/<tenant>/<tree>/<path>` | one tree-relative file path |

Patterns follow these matching rules:

- **Wildcards.** `*` inside a segment matches any run of characters within
  that segment. A whole `**` segment matches zero or more segments.
- **Ancestors.** A statement matches a request when one of its resources
  covers the request's own resource **or one of its ancestors**. A statement
  on `wt:tenant/acme` therefore applies to all of acme's trees, and one on
  `wt:tree/acme/web` applies to every path in `web`.
- **Path patterns are exact.** A path pattern matches a file path segment by
  segment. To cover a directory, end the pattern with `/**`:
  `wt:path/acme/web/secrets/**`.
- **When paths are evaluated.** Path resources are evaluated only when the
  caller supplies a path: per-file read checks on pull and REST reads, and
  per-file write checks on push (§13).
- **The policy's tenant.** A policy is evaluated only for resources of its own
  tenant. Its patterns must still spell out the tenant slug.

### 8.3 Principals

| Pattern | Matches |
|---|---|
| `*` | everyone, including anonymous callers |
| `authenticated` | every signed-in principal (users and service accounts) |
| `anonymous` | callers without a credential |
| `user:<username>` | that account |
| `team:<slug>` | members of that team in the policy's tenant |
| `role:<role>` | any principal holding that role for this request (tenant role, team or tree grant role, service-account role) |
| `sa:<name>` | that service account of the policy's tenant |
| `sa:<tenant>/<name>` | a service account of any tenant |
| `tenant:<slug>` (v0.5) | every member of that tenant, and its service accounts |

Names may contain `*`, for example `user:ext-*`.

### 8.4 Conditions

| Key | Holds when |
|---|---|
| `ip_ranges` | the client IP is inside one of the CIDRs (a bare address means a single host) |
| `require_mfa` | the principal's session was created after a second factor (`Principal.MFA`) |
| `not_before` / `not_after` | the request time is inside the RFC 3339 bounds |
| `time_window` | `{days: ["mon".."sun"], start: "HH:MM", end: "HH:MM", timezone: "<IANA>"}` matches. `end` is exclusive. A window may wrap midnight, and then belongs to the day it starts on. The default timezone is `UTC`, and tzdata is embedded so this works on Windows. |

Some conditions cannot be decided. The client IP may be unknown, or a stored
condition may be corrupt. In those cases an **allow** statement does not apply
and a **deny** statement does: evaluation fails closed.

The client IP is the socket peer address. `X-Forwarded-For` is not
consulted.

SSO sessions are issued with `MFA = false`. A `require_mfa` statement
therefore treats them like password sessions without a second factor.

## 9. Evaluation algorithm

`authz.Authorizer.Evaluate(principal, action, resource, env)` runs these steps.
`Authorize` is the yes/no form that services call: it returns an error wrapping
`ErrUnauthenticated` for anonymous callers and `ErrForbidden` for everyone
else.

0. **Normalize.** A tree that does not belong to the requested tenant is
   denied. A branch or path resource without a tree is invalid. An unknown
   action is an implicit deny. `branch:push` is evaluated as `tree:write`
   (the *target* action), followed by a branch deny pass (step 6).
1. **Identity.** The principal is re-read from the store. A disabled account
   or service account is denied (source `disabled`).
2. **Token scopes.** When `Principal.Scopes` is non-empty, one scope pattern
   must cover the target action. Otherwise the request is denied (source
   `token_scope`). This step runs before the admin bypass, so scopes bind
   server admins too.
3. **Server admin.** A server admin is allowed (source `server_admin`). No
   later step runs. `server:admin` is refused for everyone else.
4. **Tenant status (v0.5).** A `deactivated` tenant denies every action. A
   `suspended` tenant denies every action other than the read actions
   (`tenant:read`, `tree:read`, `iam:read`), with reason `tenant_suspended`.
   Suspension revokes no credentials and changes no stored data.
5. **Roles.** The engine collects every role the principal holds for the
   resource:
   - the tenant membership role (users);
   - the service account's own role (only in its tenant);
   - direct tree grants (user or service account);
   - team grants on the tree, for every team the account is in;
   - tenant grants on the tree, when the principal belongs to the granted
     tenant.

   Grant roles apply only to tree-scoped actions. A built-in role allows the
   action per §4.1. A custom role allows it when one of its permissions covers
   it. **Visibility** then adds an allow for `tree:read` (§7).
6. **Policies.** The engine evaluates every enabled policy of the resource's
   tenant. A statement matches when all of the following hold:
   - one of its actions covers the target action;
   - one of its resources covers the request's resource or an ancestor;
   - one of its principals matches;
   - its conditions hold.

   A matching **deny** is an explicit deny. It overrides every allow from
   roles, grants, visibility and policy allows. A matching **allow** grants
   the action even without a role.

   For `branch:push`, a deny-only pass also runs over the branch. The push is
   refused by any deny statement that matches either:
   - `branch:push` on the branch, its tree or its tenant; or
   - `tree:write` on the branch resource itself.

   A policy allow cannot grant `branch:push` without `tree:write`.

   **Lockout guard (v0.5):** a deny statement does not apply to a tenant
   owner for `iam:*` and `tenant:admin`. The reason is recorded as
   `owner_protected`.
7. **Default.** The request is allowed when some role, visibility rule or
   policy allowed it. Otherwise the result is `implicit_deny`, with a message
   such as "not a member of the tenant" or "role viewer does not permit it".

A `Decision` has three fields:

- `allowed`;
- `effect`: `allow`, `deny` (explicit) or `implicit_deny`;
- `reasons`: an ordered list of `{source, detail, effect}`.

Reason sources are `server_admin`, `disabled`, `tenant_role`, `team_role`,
`tree_grant`, `custom_role`, `service_account`, `visibility`, `token_scope`,
`policy`, `default` and, in v0.5, `tenant_suspended` and `owner_protected`.
A reason's effect is `allow`, `deny` or `info`.

Store lookups are memoized **per request only** (`authz.WithRequestCache`), so
IAM changes take effect on the next request. `syncsvc.Push` shares one cache
across its `tree:write`, `branch:push` and protection checks.

**Example.** Policy `protect-main` holds the deny statement shown in §8.1, and
bob is a developer in team `contractors`. `wt iam simulate --principal user:bob
--action branch:push --tenant acme --tree acme/web --branch main` reports:

- `tenant_role` allow: developer grants `tree:write`;
- `policy` deny: `policy "protect-main" statement "no-contractor-pushes-to-main" denies branch:push`;
- effect `deny`.

## 10. Owner protection and tenant status

- **Last owner.** Removing or demoting the last owner of a tenant fails with
  `409 {"code":"last_owner"}`. An owner may leave the tenant only while another
  owner remains.
- **Owner protection.** Policy denies are ignored for owners on `iam:*` and
  `tenant:admin`. A badly written policy therefore cannot lock the tenant out
  of repairing its own IAM.
- **Tenant status.** Server admins set it with `PATCH /admin/tenants/{t}`
  `{"status","status_reason"}` or with `wt tenant suspend|resume|deactivate
  <tenant> [--reason]`. `active` (or `""`) is normal. For `suspended` and
  `deactivated`, see step 4 of §9. Server admins are never affected.

## 11. Tokens, scopes and authentication

This section summarizes authentication. Hashing, token format, lockouts, TLS
and SSO internals are in [Security](../security/Security.md).

| Token kind | Issued by | Notes |
|---|---|---|
| `session` | Password login (`POST /auth/login`), MFA login (`/auth/login/mfa`), SSO callback | Valid for 30 days by default. Records `last_ip`, `user_agent`, `mfa`. Listed by `GET /auth/sessions` and `wt session list`. |
| `api` | `POST /auth/tokens {name, ttl_hours, scopes}`, `wt token create` | Personal token, optionally scoped and expiring |
| `service` | `POST /tenants/{t}/service-accounts/{name}/keys`, `wt sa key-create` | Service-account key. `AccountID` is empty. Optionally scoped. |

All tokens are `wt_…` bearer strings, shown once and stored as SHA-256 hashes.

**Scopes** are action patterns such as `tree:read`, `tree:*` or `*`. A scoped
token can do at most what its scopes cover, and only when the identity is
allowed the action in the first place. Note that `branch:push` is checked as
`tree:write` at the scope step, so a push token needs a scope that covers
`tree:write` (for example `tree:write` or `tree:*`).

**Two-factor authentication** (TOTP, RFC 6238) works as follows:

- When TOTP is enabled, `POST /auth/login` returns `{mfa_required: true,
  mfa_token}`. The `wtm_…` token is valid for 5 minutes and can be used once.
- `POST /auth/login/mfa {mfa_token, code}` exchanges it, plus a TOTP code or a
  recovery code, for a session with `mfa = true`.
- `wt login` prompts for the code, or reads `--mfa-code` / `WT_MFA_CODE`.

**SSO** uses OpenID Connect with PKCE (S256), state and nonce, and accepts
RS256 and ES256 ID tokens. Accounts can be auto-created on first login.
Linking an identity to an existing account by email requires the opt-in
`link_by_email`. See [Security](../security/Security.md#6-single-sign-on-openid-connect).

**Invitations** (`wti_…`, 7 days by default) are created by `iam:admin`
holders:

- An invitation adds a membership with the invited role (default `developer`)
  and, optionally, a team membership.
- When the invitation names an email or username, the accepting account must
  match it.
- Accepting never downgrades an existing role.

**Password resets** (`wtr_…`, 1 hour by default) are issued by server admins.
Redeeming one revokes every token of the account.

## 12. Inspecting access: simulator and effective permissions

- **Simulator.** `POST /iam/simulate` takes
  `{principal:{type:"user"|"service_account", name, tenant?}, action, tenant,
  tree?, branch?, path?, context:{ip?, mfa?}}` and returns the full
  `Decision`. In v0.5 it also accepts `"candidate_policies":[Policy]`, which
  replace the tenant's policies for this one evaluation, so you can dry-run a
  policy change.
  - CLI: `wt iam simulate --principal user:U|sa:T/N --action A --tenant T
    [--tree TREE] [--branch B] [--path P] [--ip IP] [--mfa]`.
  - For a file: `wt policy simulate --file policy.json …`.
  - Admin panel: the **Access simulator** page (`/iam/simulator`).
- **Effective permissions.** `GET /iam/effective?tenant=&tree=&principal=`
  returns `{role, team_roles:[{team, role}], grants:[grant],
  permissions:[action]}`, and `wt iam effective` prints it. The default
  principal is the caller. Time and MFA conditions are evaluated for "now",
  and statements with IP conditions are skipped because they need a concrete
  request.

## 13. Where decisions are enforced

All enforcement happens on the server. Clients display policies but never
decide.

| Operation | Checks |
|---|---|
| gRPC `Push` | `tree:write` → `branch:push` on the branch → branch protection → license `CheckPush` → PushChecks: `pathacl` (`tree:write` per changed path, any explicit deny rejects with `access_denied`), quotas, signatures |
| gRPC `Pull` / `DownloadObjects` | `tree:read`. Paths that a `tree:read` policy denies or a license withholds are returned in `stubbed_paths`, and their blobs are refused. |
| REST content (`blob`, `files`, `compare`, file history, snapshot file lists, archive) | `tree:read`, then per path: policy deny → `403 path_denied`, license → `451 license_restricted`. Listings omit restricted paths and add `restricted_count`. |
| `Stage`, tags | `tree:write`. Tag deletion needs `tree:admin`. |
| REST management | The action named in each route (`branch:create`, `mr:*`, `release:*`, `ci:report`, `iam:read`, `iam:admin`, `tenant:admin`, `tree:admin`, `server:admin`) |
| `SyncAccessConfig` / access-config REST | `iam:admin` plus the escalation check ([Declarative Access](./DeclarativeAccess.md)) |

Every REST `403` and every gRPC `PermissionDenied` is audited as
`access.denied`. The metric `wt_authz_decisions_total{decision}` counts
allows and denies.

## 14. REST API

All routes are under `/api/v1` and use the error envelope
`{"error":{"code","message"}}`. Reads need `iam:read` and changes need
`iam:admin`, unless stated otherwise.

```
GET    /iam/actions                                         (public)
GET    /tenants/{t}/teams | POST {slug,name,description}
GET    /tenants/{t}/teams/{team}                            -> {team, members, grants}
PATCH  /tenants/{t}/teams/{team} {name?,description?} | DELETE
PUT    /tenants/{t}/teams/{team}/members/{username} {role}  (team maintainers too) | DELETE
GET    /tenants/{t}/roles | POST {name,description,permissions}
GET|PATCH|DELETE /tenants/{t}/roles/{name}                  (DELETE 409 while assigned)
GET    /tenants/{t}/policies | POST {name,description,enabled,statements}
POST   /tenants/{t}/policies/validate {statements}          -> {valid, errors:[{statement,field,message}]}
GET|PUT|DELETE /tenants/{t}/policies/{id}                   (PUT = full replace, version++)
GET    /trees/{t}/{tree}/grants | PUT {subject_type,subject,role} | DELETE ?subject_type=&subject=
GET    /tenants/{t}/service-accounts | POST {name,description,role}
GET|PATCH|DELETE /tenants/{t}/service-accounts/{name}       (PATCH {description?,role?,disabled?})
POST   /tenants/{t}/service-accounts/{name}/keys {name,ttl_hours,scopes} -> {token, api_token}
DELETE /tenants/{t}/service-accounts/{name}/keys/{id}
GET    /tenants/{t}/invitations | POST {email?|username?, role, team?} -> {invitation, code}
DELETE /tenants/{t}/invitations/{id}
GET    /invitations/{code} (public preview) | POST /invitations/{code}/accept
POST   /iam/simulate                                        (tenant iam:read)
GET    /iam/effective?tenant=&tree=&principal=
GET    /tenants/{t}/access-config | POST .../access-config/plan | .../apply   (v0.5)
PATCH  /admin/tenants/{t} {status,status_reason}            (server admin, v0.5)
```

Self-service authentication routes are listed in
[Security](../security/Security.md).

Every mutating IAM call is audited. A policy or role with
`managed_by: "config"` answers `409 {"code":"managed_by_config"}` to `PUT`,
`PATCH` and `DELETE`, unless an owner passes `?force=true`.

## 15. CLI

All IAM commands accept `--server` and `--json`. `TREE` arguments are
`tenant/tree`.

```
wt team list T | create T SLUG [--name] [--description] | show T SLUG | update T SLUG | delete T SLUG
wt team add T SLUG USER [--maintainer] | remove T SLUG USER
wt role list T | create T NAME --permission ACTION... [--description] | show | update | delete
wt policy list T | show T ID | create T --file F|- | update T ID --file F | delete T ID
wt policy validate T --file F | enable T ID | disable T ID | simulate --file F ...
wt grant list TREE | set TREE (--user U|--team SLUG|--sa NAME) --role R | remove TREE (--user U|--team SLUG|--sa NAME)
                                          (v0.5 adds a tenant subject, see `wt grant set --help`)
wt sa list T | create T NAME --role R [--description] | show | update [--role] [--disable|--enable] | delete
wt sa key-create T NAME [--name K] [--ttl-hours N] [--scope S]... | key-delete T NAME KEY_ID
wt token list | create NAME [--ttl-hours N] [--scope S]... | revoke ID
wt session list | revoke ID | revoke-others
wt mfa status | enable | disable [--password-stdin] | recovery-codes
wt invite list T | create T (--email E|--user U) [--role R] [--team SLUG] | revoke T ID | show CODE | accept CODE
wt iam actions | effective [--tenant T] [--tree TREE] [--principal ...] | simulate ...
wt tenant members T | add-member T USER [--role R] | set-role T USER ROLE | remove-member T USER
wt tenant suspend|resume|deactivate T [--reason]
wt access validate | plan | apply | pull                    (see Declarative Access)
```

Run `wt <command> --help` for the exact flags.

## 16. Audit

IAM changes and sign-ins are written to the audit log
(`GET /audit`, `wt audit`). The actions include:

- teams: `team.create`, `team.update`, `team.delete`, `team.member.set`,
  `team.member.remove`;
- roles: `role.create`, `role.update`, `role.delete`;
- policies: `policy.create`, `policy.update`, `policy.delete`;
- grants: `grant.set`, `grant.remove`;
- service accounts: `sa.create`, `sa.update`, `sa.delete`, `sa.key.create`,
  `sa.key.delete`;
- memberships: `member.update`, `member.remove`;
- invitations: `invitation.create`, `invitation.revoke`, `invitation.accept`;
- MFA: `mfa.enroll`, `mfa.enable`, `mfa.disable`, `mfa.recovery_codes`;
- sessions: `session.revoke`, `session.revoke_others`;
- password resets: `password.reset_code`, `password.reset`;
- SSO: `sso.login`, `sso.login_failed`, `sso.config.update`;
- denials: `access.denied`;
- declarative access: `access.applied`.

Action filters also accept a `<prefix>.*` form, for example
`wt audit --action 'policy.*'`. The full list is in
[Security](../security/Security.md#11-audit-log).

## 17. Design rationale

- **One engine.** REST, gRPC, the simulator and effective permissions all go
  through `authz.Evaluate`, so what the simulator shows is what is enforced.
- **Deny overrides, with no scope precedence.** An explicit deny anywhere wins
  over every allow, and statements on a tenant or tree cover everything
  beneath them. This order is easier to reason about than "most specific scope
  wins", and it makes the tree-level declarative documents restrict-only.
- **Least surprise for grants.** Grants only add access, and only on one tree.
  Restrictions are always expressed as denies.
- **Fail closed.** Undecidable conditions keep denies in force and allows
  inactive. Corrupt statements never match.
- **No caching across requests.** Revocations, role changes and disabling take
  effect immediately.

## 18. Implementation status

| Area | Status |
|---|---|
| Action catalog with v0.5 actions; built-in role matrix | Built (`internal/authz/actions.go`) |
| Custom roles, teams, tree grants (user, team, service account) | Built |
| Tenant grants (`subject_type: "tenant"`) and `tenant:<slug>` principals | Built (v0.5, `access` workstream) |
| Policies: deny-overrides evaluation, wildcards, CIDR / MFA / time conditions, validation | Built |
| `branch:push` per-branch denies; `wt:path` denies on read (stubbed paths, 403) and write (`pathacl`) | Built |
| Lockout guard (`owner_protected`, `last_owner`); tenant status | Built (v0.5) |
| Service accounts and scoped keys; scoped personal tokens; sessions; TOTP; invitations; password resets; OIDC SSO | Built |
| Simulator (including `candidate_policies`) and effective permissions | Built |
| Declarative access files (`wt access`) | Built (v0.5), see [Declarative Access](./DeclarativeAccess.md) |
| Token scopes for the v0.5 actions | Partial: `iam.ValidateScopes` still lists only the v0.4 actions, so `release:*` and `ci:report` scopes are rejected when a token or key is created. A `*` scope works. |
| Rust `worktree_protocol::iam` (`engine`, `role`, `permission`, `policy`, `scope`) and `access::{tree_access, branch_access}` | Legacy protocol model from v1. The server does not consult it. Its names (`sync:push`, `snapshot:create`, …) survive only as declarative-TOML aliases. |

## 19. Roadmap

The following were described in earlier versions of this spec but are not
built, and are not part of the v0.5 contract:

- **Registered paths.** Explicit `[[registered_path]]` entries in
  `config.toml` that would make a path eligible for access control. Today,
  path control uses `wt:path/...` glob resources directly. The Rust config
  types (`config::RegisteredPath`) exist but nothing reads them.
- **Scope-precedence resolution.** "Most specific scope wins" across Global →
  Tenant → Tree → Branch → RegisteredPath, and server-wide (global) policies.
  Policies are tenant-scoped, and the order is deny-overrides.
- **Branch-scoped and path-scoped role assignment.** For example, "Maintainer
  on `main` only". Roles are assigned per tenant (membership) or per tree
  (grant). Per-branch restrictions are expressed as denies.
- **Custom ABAC attributes** on tenants or accounts (`department`,
  `clearance_level`, …) and generic condition operators (`Equals`, `In`,
  `StartsWith`, `GreaterThan`, …). Only the fixed conditions of §8.4 exist.
- **Additional fine-grained actions**: `branch:read`, `branch:protect`,
  `snapshot:create`, `snapshot:read`, `snapshot:restore`, `sync:pull`,
  `tag:create`, `tag:delete`, `merge_request:*` and the `*Manage` permissions.
  These are covered by the catalog actions in §3.
- **Ownership transfer** as a distinct operation and action.
- **Tenant-wide team roles.** Teams receive access only through tree grants
  and policies.
- **Policy history.** Earlier policy versions, diffs and rollback. Only the
  `version` counter is kept. `wt access plan` shows pending changes.
- **Trusted proxy headers** (`X-Forwarded-For`) for IP conditions and rate
  limits.
- **MFA-asserted SSO sessions.** Reading `amr` / `acr` claims so that IdP MFA
  satisfies `require_mfa`.
- **Access files versioned with snapshots**, protected on push, and
  re-applied automatically by the background process. See
  [Declarative Access](./DeclarativeAccess.md#roadmap).

## 20. References

- `services/server-go/internal/authz/` (`doc.go`, `actions.go`,
  `authorizer.go`, `policy.go`, `resource.go`, `validate.go`, `cache.go`)
- `services/server-go/internal/iam/` (`service.go`, `token.go`, `scopes.go`,
  `serviceaccount.go`, `sessions.go`, `mfa.go`, `totp.go`, `invitations.go`,
  `reset.go`, `middleware.go`, `validate.go`)
- `services/server-go/internal/sso/`, `internal/accesscfg/`,
  `internal/pathacl/`
- `services/server-go/internal/domain/{iam,types,v05}.go`
- `services/server-go/internal/httpapi/{iam*.go,tenants.go,sso.go,access_routes.go}`
- `services/server-go/internal/syncsvc/{push.go,pull.go,accesscfg.go}`
- `crates/worktree-cli/src/commands/{team,role,policy,grant,sa,token,session,mfa,invite,iam,sso,tenant,access}.rs`
- `crates/worktree-protocol/src/iam/` (legacy model)
- `docs/iam.md`, `docs/ENGINEERING_PLAN.md` §12, §13.1, §13.7
- Related specs: [Tenant Model](./TenantModel.md),
  [Declarative Access](./DeclarativeAccess.md),
  [Staged Visibility](../visibility/StagedVisibility.md),
  [License Compliance](../licensing/LicenseCompliance.md),
  [Security](../security/Security.md), [Server](../server/Server.md)
