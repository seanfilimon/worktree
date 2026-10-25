// Package iam implements authentication (accounts, passwords, API tokens)
// and authorization (role-based access to tenants and trees) for the
// W0rkTree server.
//
// # Authentication
//
// [Service] implements [domain.Authenticator]:
//
//   - Passwords are hashed with argon2id and stored in PHC string format
//     ("$argon2id$v=19$m=…,t=…,p=…$salt$key") with a fresh random salt per
//     hash. Verification re-derives the key with the parameters recorded in
//     the hash and compares in constant time, so parameters can be raised
//     later without invalidating existing hashes (hashes using outdated
//     parameters are transparently upgraded on the next successful login).
//   - Passwords must be 8..1024 bytes long (minimum counted in characters).
//   - Usernames match ^[a-z0-9][a-z0-9-]{1,38}$ (input is ASCII-lower-cased
//     first). The names admin, anonymous, api, auth, events, healthz, metrics
//     and system are reserved; "admin" may be claimed only through
//     [EnsureAdmin].
//   - Register creates the account, its personal tenant (slug = username,
//     kind "user", plan "free") and an owner membership.
//   - Tokens are "wt_" followed by the unpadded base64url encoding of 32
//     bytes from crypto/rand. Only the hex SHA-256 of a token is stored.
//     Login issues a token named "session" valid for Options.SessionTTL
//     (default 30 days); CreateToken issues named tokens with an optional
//     expiry. Token use is recorded (TouchToken) at most once per minute
//     per token.
//   - Failed logins are rate limited per (username, client IP): by default
//     10 failures within 5 minutes lock further attempts for that pair
//     until the window passes. Unknown usernames and wrong passwords produce
//     the same error and take the same time.
//
// # Account security (v0.4, docs/ENGINEERING_PLAN.md §12.2)
//
// [Service] also implements [domain.AccountSecurity]:
//
//   - Tokens have a Kind: "session" (password, MFA or SSO login; records
//     the client IP and user agent, and whether a second factor was used),
//     "api" (personal tokens, optionally restricted to Scopes) and
//     "service" (service-account keys: no AccountID). Pre-v0.4 tokens
//     without a Kind count as sessions when named "session" ([TokenKind]).
//     [Service.Authenticate] fills Principal.Kind/Scopes/MFA/TokenID;
//     service-account keys yield a principal with Username
//     "sa:<tenant>/<name>" and TenantID = the owning tenant. Stores that
//     implement [TokenMetaToucher] also get the latest client IP and user
//     agent on token use ([WithUserAgent] / [UserAgentFrom]).
//   - Scopes are concrete actions ("tree:read"), area wildcards
//     ("tree:*") or "*" ([ValidateScopes], [ScopeAllows]).
//   - TOTP (RFC 6238: HMAC-SHA-1, 6 digits, 30 s, ±1 step) with a 160-bit
//     secret. Enrollment stores a pending secret until confirmed with a
//     code; confirmation issues 10 single-use recovery codes "xxxx-xxxx"
//     (Crockford base32, stored as sha256). Each accepted TOTP step is
//     recorded so a code cannot be replayed. Disabling requires the
//     password, regenerating recovery codes a current TOTP code.
//   - With TOTP enabled, [Service.LoginWithMeta] returns an MFA token
//     ("wtm_…") instead of a session; [Service.CompleteMFALogin] exchanges
//     it plus a TOTP or recovery code for a session with MFA set. MFA
//     tokens live in process memory (hashed), expire after 5 minutes, are
//     single use and allow 5 wrong codes. Wrong second factors are also
//     limited per account (default 10 per 15 minutes, across login,
//     confirmation, regeneration and disable). The single-step
//     [Service.Login] refuses accounts with MFA ("mfa required").
//   - Invitations ("wti_…", 7 days by default) and admin-issued password
//     resets ("wtr_…", 1 hour by default) are one-time codes stored as
//     sha256. Accepting an invitation never downgrades an existing role;
//     redeeming a reset revokes every token of the account.
//
// # Authorization
//
// [Service] implements [domain.Authorizer] (the full policy engine lives
// in internal/authz). Token scopes are enforced first, for everyone.
// Service accounts act with their Role in their own tenant only. Server
// admins (Account.IsAdmin) may perform every action. Disabled accounts may
// perform none. Otherwise
// the caller's role in the tenant (its Membership) must grant the action.
// Built-in roles form a strict superset hierarchy
// (owner ⊃ admin ⊃ maintainer ⊃ developer ⊃ viewer), mirroring
// worktree-protocol's iam::role built-ins:
//
//	| Action         | viewer | developer | maintainer | admin | owner |
//	|----------------|:------:|:---------:|:----------:|:-----:|:-----:|
//	| tenant:read    |   ✓    |     ✓     |     ✓      |   ✓   |   ✓   |
//	| tree:read      |   ✓    |     ✓     |     ✓      |   ✓   |   ✓   |
//	| tree:write     |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| branch:create  |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| mr:create      |        |     ✓     |     ✓      |   ✓   |   ✓   |
//	| branch:delete  |        |           |     ✓      |   ✓   |   ✓   |
//	| mr:approve     |        |           |     ✓      |   ✓   |   ✓   |
//	| mr:merge       |        |           |     ✓      |   ✓   |   ✓   |
//	| tree:create    |        |           |     ✓      |   ✓   |   ✓   |
//	| tree:admin     |        |           |            |   ✓   |   ✓   |
//	| tenant:admin   |        |           |            |   ✓   |   ✓   |
//	| server:admin   |        |           |            |       |       |
//
// server:admin is granted only to server admins. Owner currently has the
// same action set as admin; owner-only operations (such as transferring
// ownership) are enforced by callers that compare roles directly.
// Membership roles not in the table (custom roles) grant nothing.
//
// Tree visibility widens tree:read only:
//
//	| Visibility | anonymous | any authenticated account | tenant member     |
//	|------------|-----------|---------------------------|-------------------|
//	| public     | read      | read                      | per role          |
//	| internal   | —         | read                      | per role          |
//	| private    | —         | —                         | per role          |
//
// Unknown or empty visibility is treated as private. An anonymous caller
// denied an action receives an error wrapping [domain.ErrUnauthenticated];
// an authenticated caller receives one wrapping [domain.ErrForbidden].
//
// Out of scope: the version-controlled access-config documents
// (MetaStore.PutAccessConfig — policies, custom roles, ABAC conditions and
// .wt-tree overrides described in specs/iam/IAM.md) are stored but not yet
// evaluated; only the built-in role matrix above is enforced.
//
// # Transport integration
//
// [UnaryInterceptor], [StreamInterceptor] and [Middleware] resolve an
// "authorization: Bearer <token>" credential (HTTP additionally accepts an
// access_token query parameter on GET requests, for EventSource clients) and
// attach the resulting principal with [WithPrincipal]. Requests without a
// credential continue anonymously; requests with an invalid credential are
// rejected (gRPC codes.Unauthenticated, HTTP 401 JSON error).
package iam
