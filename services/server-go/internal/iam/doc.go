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
// # Authorization
//
// [Service] implements [domain.Authorizer]. Server admins (Account.IsAdmin)
// may perform every action. Disabled accounts may perform none. Otherwise
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
