# ABAC Attributes Specification

## Table of Contents

1. [Overview](#1-overview)
2. [Attributes](#2-attributes)
3. [Attribute conditions](#3-attribute-conditions)
4. [Evaluation](#4-evaluation)
5. [Declarative access files (TOML)](#5-declarative-access-files-toml)
6. [Simulator explanation](#6-simulator-explanation)
7. [Caching and performance](#7-caching-and-performance)
8. [REST API](#8-rest-api)
9. [CLI](#9-cli)
10. [Admin panel](#10-admin-panel)
11. [Audit](#11-audit)
12. [Security considerations](#12-security-considerations)
13. [Implementation status](#13-implementation-status)
14. [Roadmap](#14-roadmap)
15. [References](#15-references)

## 1. Overview

Attribute-based access control (ABAC, v0.8) lets policy statements
([IAM §8](./IAM.md)) depend on facts about the caller, the tree and the
tenant instead of only on names and roles. Tenants, accounts and trees
carry free-form string **attributes** (`department = "eng"`,
`classification = "secret"`, `region = "eu-west"`); a statement's
`conditions.attributes` tests them with a small set of operators. Typical
uses:

- "Developers without clearance may not touch trees classified `secret`"
  (a deny keyed on `tree.classification` and `principal.clearance`).
- "Engineers may administer the trees of tenants in the EU" (an allow keyed
  on `principal.department` and `tenant.region`).

Attribute conditions are one more condition kind next to IP ranges, MFA,
validity dates and time windows: every condition of a statement must hold
for the statement to apply, and the deny-overrides evaluation order of
IAM §9 is unchanged.

## 2. Attributes

| Object | Field | Set by | Read by |
|---|---|---|---|
| Tenant | `Tenant.Attributes` | `tenant:admin` | `tenant:read` |
| Account | `Account.Attributes` | server admins only | server admins; the account itself (`GET /me/attributes`) |
| Tree | `Tree.Attributes` | `tree:admin` | `tree:read` (also part of the tree JSON) |

Rules (checked by the server, the CLI and the admin panel alike):

- **Keys** match `^[a-z][a-z0-9_.-]{0,63}$` (dots are allowed: `org.unit`).
- **Values** are 1–256 bytes of UTF-8 without control characters. Empty
  values are not allowed: remove the attribute instead.
- At most **64 attributes** per object.
- Values are compared exactly (case-sensitive, no trimming).

Accounts cannot change their own attributes: attributes are an input to
authorization, so only server administrators set them. Service accounts
and anonymous callers have no principal attributes.

Writes replace the whole set (`PUT` with `{"attributes": {...}}`;
`{"attributes": {}}` clears it). The CLI's `wt attr set` / `unset` read
the current set, change it and write it back.

## 3. Attribute conditions

```json
"conditions": {
  "attributes": [
    {"key": "tree.classification", "op": "equals", "values": ["secret"]},
    {"key": "principal.clearance", "op": "not_in", "values": ["secret", "top-secret"]}
  ]
}
```

`key` is `<scope>.<attribute key>`:

| Scope | Object |
|---|---|
| `principal` | the calling account (`Account.Attributes`); none for service accounts and anonymous callers |
| `tree` | the tree of the resource; none for tenant-level requests |
| `tenant` | the tenant of the resource, i.e. the policy's tenant |

Operators (`op`, case-insensitive):

| Operator | Values | Holds when |
|---|---|---|
| `equals` | exactly 1 | the attribute is set and equal |
| `not_equals` | exactly 1 | the attribute is unset or different |
| `in` | 1–64 | the attribute is set and equal to one of the values |
| `not_in` | 1–64 | the attribute is unset or equal to none |
| `starts_with` | 1–64 prefixes | the attribute is set and starts with one of them |
| `exists` | none | the attribute is set |
| `not_exists` | none | the attribute is unset |

An **unset** attribute fails `equals`, `in`, `starts_with` and `exists`
and satisfies `not_equals`, `not_in` and `not_exists`. Write deny
statements with the negative operators ("deny unless cleared":
`principal.clearance not_in [...]`) so that a missing attribute is denied.

At most 32 attribute conditions per statement; all of them must hold.
Values follow the attribute value rules of §2.

Validation happens on every policy write (`POST/PUT
/tenants/{t}/policies`, `POST .../policies/validate`, access documents,
simulator candidates) with field names such as
`conditions.attributes[1].values[0]`, and locally in the Rust
declarative validator (§5) and `wt policy create/update`.

## 4. Evaluation

Attribute conditions are evaluated in step 5 of the IAM algorithm (IAM
§9), after the statement's principal, action and resource matched and its
other conditions (MFA, validity, time window, IP) held. The attributes
come from the account and tenant the evaluation loads anyway and from the
tree of the request, so an attribute change is visible to the very next
request.

A statement whose attribute conditions cannot be compiled (only possible
with corrupt stored data, since writes are validated) is treated like any
other broken condition: an allow never applies, a deny always applies
(fail closed).

`EffectivePermissions` (`GET /iam/effective`) evaluates attribute
conditions too: they do not need a concrete request.

Server administrators bypass policies, and therefore attribute
conditions, as before; the owner lockout guard (IAM §10) still prevents
policies from denying tenant owners `iam:*` and `tenant:admin`.

## 5. Declarative access files (TOML)

`.wt/access/policies.toml` and `<tree>/.wt-tree/access/policies.toml`
([DeclarativeAccess](./DeclarativeAccess.md)) accept attribute conditions
as an array of tables:

```toml
[[policy]]
name = "clearance"

[[policy.statement]]
sid = "need-clearance"
effect = "deny"
principals = ["role:developer"]
actions = ["tree:*"]
resources = ["wt:tree/acme/*"]

[[policy.statement.conditions.attributes]]
key = "tree.classification"
op = "equals"
value = "secret"                 # `value` = one value

[[policy.statement.conditions.attributes]]
key = "principal.clearance"
op = "not_in"
values = ["secret", "top-secret"] # `values` = a list
```

The inline form is equivalent:
`conditions = { attributes = [{ key = "tenant.region", op = "starts_with", values = ["eu-"] }] }`.
Keys other than `key`, `op`, `value` and `values` are `E2002`
(structure); giving both `value` and `values` is `E2002`; invalid keys,
operators and value counts are `E2011` (bad condition) located at
`policy[i] (name).statement[j].conditions.attributes[k].<field>`; more
than 32 conditions is `E2015`. `wt access pull` writes the inline form.
Tree-scope documents may use attribute conditions in their deny
statements.

## 6. Simulator explanation

`POST /iam/simulate` (and `wt iam simulate`, `wt policy simulate`, the
admin Access simulator) explains attribute conditions:

- **Overrides.** `context.attributes` replaces stored values for this
  evaluation: `{"principal.clearance": "secret", "tree.classification":
  null}` (`null` = unset). Keys are condition keys; values follow §2.
- **Response.** The Decision gains `attributes` — the attributes the
  evaluation saw per scope (`{"principal": {...}, "tree": {...},
  "tenant": {...}}`, after overrides) — and `conditions`, one entry per
  attribute condition of every statement whose principal, action and
  resource matched and whose other conditions held:
  `{policy, policy_id, statement, effect, key, op, values, value, matched}`
  (`statement` is the sid or `#<n>`; `value` is `null` when unset).
- **Reasons.** Each such condition also adds a reason with source
  `attribute` (effect `info`), e.g.
  `policy "clearance" statement "need-clearance": principal.clearance is unset; principal.clearance not_in ["secret", "top-secret"]: met`.
  A statement that does not apply because of an attribute names the first
  failing condition in its `policy` reason
  (`... does not apply: attribute condition principal.department in ["eng"] is not met (principal.department is "sales")`).

Outside the simulator the per-condition reasons and checks are not
collected (see §7); the `policy` reason still names the failing
condition.

## 7. Caching and performance

- Attribute conditions are compiled with their statement and cached with
  the compiled policies (`authz/policycache.go`); the cache compares
  attribute conditions field by field, so a policy change is picked up on
  the next evaluation.
- The attribute source of an evaluation is built only when an enabled
  policy of the tenant uses attribute conditions. Tenants without them
  evaluate exactly as before.
- Attributes are never cached across requests. Within one request the
  authorization request cache memoizes accounts and tenants; the
  attribute endpoints drop those entries after a write
  (`authz.InvalidateAttributes`), so evaluations later in the same request
  see the change.
- Benchmarks (`go test ./internal/authz -run '^$' -bench . -benchmem`,
  tenant with 300 policies and 200 grants): `BenchmarkAuthorize`,
  `BenchmarkEvaluateBranchPush` and `BenchmarkEffectivePermissions`
  allocate exactly as before attribute support (199–201, 382 and 3462
  allocs/op) with run times within measurement noise.
  `BenchmarkAuthorizeAttributes` (the same tenant with attribute
  conditions on a tenth of the policies) costs about 150 extra allocations
  per evaluation of an action those statements match and none for others.

## 8. REST API

All under `/api/v1`; bodies `{"attributes": {"key": "value"}}`, responses
the same shape (`{}` when empty).

| Method and path | Permission | Notes |
|---|---|---|
| `GET /tenants/{t}/attributes` | `tenant:read` | |
| `PUT /tenants/{t}/attributes` | `tenant:admin` | full replace; audited `tenant.attributes.update` |
| `GET /admin/accounts/{username}/attributes` | server admin | |
| `PUT /admin/accounts/{username}/attributes` | server admin | audited `account.attributes.update` |
| `GET /me/attributes` | signed in | own account; `{}` for service accounts |
| `GET {tree}/attributes` | `tree:read` | `{tree}` = `/trees/{tenant}/{tree}` |
| `PUT {tree}/attributes` | `tree:admin` | audited `tree.attributes.update` |
| `POST /iam/simulate` | `iam:read` (or self) | `context.attributes` overrides; response adds `attributes` and `conditions` |

Invalid bodies (missing `attributes`, bad keys or values, more than 64
entries) are `400 bad_request`.

## 9. CLI

```
wt attr show tenant|account|tree TARGET            # account TARGET may be `me`
wt attr set tenant|account|tree TARGET KEY=VALUE...
wt attr unset tenant|account|tree TARGET KEY...
wt iam simulate ... [--attr principal.<k>=V | --attr tree.<k>]...   # bare KEY = unset
wt policy simulate T --file F ... [--attr KEY[=VALUE]]...
```

TARGET is a tenant slug, a username or `TENANT/TREE`. `set` keeps the
other attributes; `unset` reports keys that were not set. `--json` prints
the server JSON. `wt policy create/update` check attribute conditions
locally before sending; `wt policy show` and the simulators render them
as `principal.department in ["eng", "ops"]`, and the simulators add an
"Attribute conditions" table (condition, actual value or `(unset)`,
met / not met).

## 10. Admin panel

- **Attribute editors** (key/value rows with the §2 rules checked as you
  type, Save / Reset, read-only for viewers) on tenant settings, on the
  account sheet (server admins) and on tree settings (tree admins); a
  read-only "My attributes" card in the user settings.
- **Policy editor**: an "Attribute conditions" section in the statement
  builder (scope, key, operator, values; values hidden for `exists` /
  `not_exists`), round-tripping with the JSON mode; statements without
  attribute conditions serialize exactly as before.
- **Access simulator**: attribute override rows (with "unset"), and an
  explanation card listing the attributes seen per scope and every
  condition with its actual value and a Matched / Not met badge.

## 11. Audit

Attribute writes are audited only when something changed:
`tenant.attributes.update`, `account.attributes.update` and
`tree.attributes.update`, with details `set` (changed or added keys),
`removed` (removed keys), `count` (attributes after the change) and the
target (`tenant`, `username` or `tree`). Attribute **values are never
written to the audit log**, since they may be sensitive (clearances).

## 12. Security considerations

- **Who controls the input.** A policy keyed on `tree.*` trusts the tree's
  admins and one keyed on `tenant.*` the tenant's admins: they can change
  those attributes. Account attributes can only be changed by server
  admins. Prefer `principal.*` for denies that must hold against tree
  admins.
- **Fail closed.** Unset attributes satisfy only the negative operators,
  so deny-unless statements stay closed for callers without the
  attribute; broken stored conditions deny (§4).
- **Visibility.** Tree attributes are visible to tree readers and tenant
  attributes to tenant readers; account attributes only to server admins
  and the account itself (`GET /auth/me`, `/me/attributes`). Tenant and
  team member lists strip them from the embedded accounts.

## 13. Implementation status

| Area | Status |
|---|---|
| Attributes on tenants, accounts and trees; validation; REST GET/PUT; `GET /me/attributes` | Built (v0.8 §16.5) |
| Attribute conditions (`equals`, `not_equals`, `in`, `not_in`, `starts_with`, `exists`, `not_exists`) on `principal.*`, `tree.*`, `tenant.*` | Built (v0.8) |
| Validation on policy writes, access documents and simulator candidates | Built (v0.8) |
| Rust declarative TOML syntax and validation (`E2002`, `E2011`, `E2015`), decompilation | Built (v0.8) |
| Simulator overrides, per-condition explanation (`attributes`, `conditions`, `attribute` reasons) | Built (v0.8) |
| Compiled-policy cache support; request-cache invalidation; benchmarks | Built (v0.8) |
| `wt attr show/set/unset`, `wt iam simulate --attr`, `wt policy simulate --attr`, attribute rendering in `wt policy` | Built (v0.8) |
| Admin attribute editors, condition builder, simulator explanation | Built (v0.8) |
| Audit of attribute changes (keys only) | Built (v0.8) |

## 14. Roadmap

- **Service-account attributes.** Service accounts have no attributes;
  `principal.*` conditions see them as unset.
- **Team attributes** and attributes of branches or paths.
- **Comparisons between attributes** (`principal.department` equals
  `tree.owner_department`) and numeric / ordered operators
  (`greater_than`, clearance levels as ordered sets).
- **Attributes from the identity provider** (SSO claims mapped onto
  account attributes at login).
- **Partial updates** (`PATCH` of single keys) and optimistic concurrency
  for attribute writes; `PUT` replaces the whole set.
- **Attribute-aware token scopes** and attribute conditions on custom
  roles.

## 15. References

- `services/server-go/internal/domain/abac.go` (limits, key and value
  rules, `AttributeCondition`), `domain/iam.go`
  (`PolicyConditions.Attributes`), `domain/types.go` (`Tenant`, `Account`,
  `Tree` `Attributes`)
- `services/server-go/internal/authz/attributes.go` (compilation,
  evaluation, explanation, overrides, trace, invalidation), hooks in
  `authz/policy.go`, `authz/authorizer.go`, `authz/validate.go`,
  `authz/policycache.go`; tests `attributes_test.go`,
  `attributes_bench_test.go`
- `services/server-go/internal/httpapi/attributes.go`, `iam_simulate.go`,
  `openapi.yaml`
- `services/server-go/internal/accesscfg/accesscfg.go` (canonical form)
- `crates/worktree-protocol/src/access/attributes.rs`,
  `access/declarative.rs`
- `crates/worktree-cli/src/commands/{attr,iam,policy}.rs`
- `apps/admin/src/components/abac/`, `apps/admin/src/api/abac.ts`,
  `apps/admin/src/pages/iam/{AttributeConditionsSection,AttributeOverridesInput,AttributeExplanation}.tsx`,
  `apps/admin/src/pages/tenants/TenantAttributesCard.tsx`,
  `apps/admin/src/pages/tree/TreeAttributesCard.tsx`
- `crates/worktree-e2e/tests/e2e/abac.rs`
- `docs/ENGINEERING_PLAN.md` §16.5
- Related specs: [IAM](./IAM.md), [Tenant Model](./TenantModel.md),
  [Declarative Access](./DeclarativeAccess.md)
