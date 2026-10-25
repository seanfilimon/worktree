// Package authz is the W0rkTree v0.4 authorization engine
// (docs/ENGINEERING_PLAN.md §12.1).
//
// [New] returns an [Authorizer] implementing [domain.Authorizer] (used by
// every service for yes/no checks) and [domain.AccessEvaluator] (the full
// engine behind the IAM simulator and effective-permission endpoints).
//
// # Evaluation
//
// For (principal, action, resource) the steps are:
//
//  1. The principal is re-read from the store (accounts by id, service
//     accounts by id) so that disabling and admin changes apply
//     immediately; unknown identities are unauthenticated. A disabled
//     account or service account is denied (source "disabled").
//  2. Token scopes: when Principal.Scopes is non-empty the action must
//     match one scope pattern ("tree:read", "tree:*", "*"), else deny
//     (source "token_scope"). Scopes bind server admins too.
//  3. Server admins (Account.IsAdmin in the store) are allowed everything;
//     roles and policies are not consulted. server:admin is never granted
//     to anyone else, by roles or by policies.
//  4. Roles held for the request: the tenant membership role (users), the
//     service account's own role (service accounts, in their tenant only),
//     direct tree grants for the user or service account, and team grants
//     on the tree for every team the account belongs to. Tree grants only
//     cover tree-scoped actions (tree:read/write/admin, branch:*, mr:*).
//     Built-in roles follow the matrix below; other names are custom roles
//     of the resource tenant (CustomRole.Permissions, "area:*" supported);
//     undefined roles grant nothing. Visibility widens tree:read exactly as
//     in v0.3: public trees are readable by everyone, internal trees by
//     every authenticated principal (users and service accounts).
//  5. Enabled policies of the resource tenant. A matching deny statement is
//     an explicit deny that overrides every allow (roles, grants,
//     visibility and policy allows). A matching allow statement grants the
//     action even without a role.
//  6. Otherwise the request is allowed iff some role, visibility rule or
//     policy allowed it; else it is an implicit deny.
//
// branch:push is evaluated as tree:write on the tree, followed by a
// deny-only pass for the branch: a deny statement for branch:push on the
// branch (or its tree / tenant), or for tree:write on the branch resource
// itself, refuses the push. Policy allows cannot grant branch:push without
// tree:write. syncsvc.Push evaluates it after the tree:write check.
//
// Built-in role matrix (owner ⊃ admin ⊃ maintainer ⊃ developer ⊃ viewer):
//
//	| Action         | viewer | developer | maintainer | admin | owner |
//	|----------------|:------:|:---------:|:----------:|:-----:|:-----:|
//	| tenant:read    |   ✓    |     ✓     |     ✓      |   ✓   |   ✓   |
//	| tree:read      |   ✓    |     ✓     |     ✓      |   ✓   |   ✓   |
//	| tree:write     |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| branch:create  |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| branch:push    |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| mr:create      |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| branch:delete  |        |           |     ✓      |   ✓   |   ✓   |
//	| mr:approve     |        |           |     ✓      |   ✓   |   ✓   |
//	| mr:merge       |        |           |     ✓      |   ✓   |   ✓   |
//	| tree:create    |        |           |     ✓      |   ✓   |   ✓   |
//	| iam:read       |        |           |     ✓      |   ✓   |   ✓   |
//	| tree:admin     |        |           |            |   ✓   |   ✓   |
//	| tenant:admin   |        |           |            |   ✓   |   ✓   |
//	| iam:admin      |        |           |            |   ✓   |   ✓   |
//	| server:admin   |        |           |            |       |       |
//
// # Policy statements
//
// Resources are "*" or "wt:tenant/<slug>", "wt:tree/<tenant>/<tree>",
// "wt:branch/<tenant>/<tree>/<branch>" and "wt:path/<tenant>/<tree>/<path>";
// "*" inside a segment matches within that segment and a "**" segment
// matches any number of segments. A request matches a statement resource
// when the pattern covers the request's own resource or one of its
// ancestors (a statement on "wt:tenant/acme" applies to all of acme's
// trees). Principals are "user:<username>", "team:<slug>", "role:<role>"
// (any role held for the request), "sa:<name>" (a service account of the
// policy's tenant), "sa:<tenant>/<name>", "authenticated", "anonymous" and
// "*". Actions are catalog actions, "<area>:*" or "*".
//
// Conditions (all set ones must hold): ip_ranges (CIDRs, net/netip; the
// client IP comes from RequestEnv.RemoteIP), require_mfa (Principal.MFA),
// not_before / not_after, and a weekly time_window in an IANA time zone
// (tzdata is embedded, so this works on Windows). A condition that cannot
// be decided — an unknown client IP or a corrupt stored condition — makes
// an allow statement not apply and a deny statement apply (fail closed).
// [ValidateStatements] rejects malformed statements before they are stored.
//
// Decision.Reasons lists every contributing source in evaluation order,
// with effect "allow", "deny" or "info", so the simulator output explains
// itself.
//
// # v0.5 additions (§13.7)
//
//   - Tenant status: after the server-admin bypass, a suspended tenant
//     refuses every action except tenant:read, tree:read and iam:read
//     (source "tenant_suspended"); a deactivated tenant refuses everything
//     (source "tenant_deactivated"). Nothing is revoked, so resuming the
//     tenant restores all access.
//   - Tenant grants: Grant{SubjectType:"tenant"} gives its role on the tree
//     to every member and service account of that tenant (cross-tenant
//     tree access). Policy principals "tenant:<slug>" match the same set.
//   - Lockout guard: deny statements never apply to tenant owners (by
//     membership) for iam:* and tenant:admin; the skipped statement is
//     recorded with source "owner_protected".
//   - [WithCandidatePolicies] evaluates with a candidate set of policies in
//     place of the tenant's stored ones (the simulator's dry run).
//
// # Caching
//
// Store lookups are memoized per request, never across requests: each
// Evaluate / Authorize call uses the cache attached to its context by
// [WithRequestCache], or a private one. syncsvc.Push attaches one so its
// tree:write, branch:push and branch-protection checks share a single read
// of the account, membership, grants and policies. Because nothing is
// cached between requests, IAM changes take effect on the next request.
package authz
