# Security Specification

## Table of Contents

1. [Overview and trust boundaries](#1-overview-and-trust-boundaries)
2. [Transport](#2-transport)
3. [Passwords and accounts](#3-passwords-and-accounts)
4. [Tokens and sessions](#4-tokens-and-sessions)
5. [Two-factor authentication](#5-two-factor-authentication)
6. [Single sign-on (OpenID Connect)](#6-single-sign-on-openid-connect)
7. [One-time codes: invitations and password resets](#7-one-time-codes-invitations-and-password-resets)
8. [Authorization, path ACLs and licenses](#8-authorization-path-acls-and-licenses)
9. [Rate limits and quotas](#9-rate-limits-and-quotas)
10. [API hardening](#10-api-hardening)
11. [Audit log](#11-audit-log)
12. [Snapshot signing](#12-snapshot-signing)
13. [Secret scanning](#13-secret-scanning)
14. [Integrity and branch protection](#14-integrity-and-branch-protection)
15. [Client-side secrets and the local daemon](#15-client-side-secrets-and-the-local-daemon)
16. [Threat model](#16-threat-model)
17. [Implementation status](#17-implementation-status)
18. [Roadmap](#18-roadmap)
19. [References](#19-references)

---

## 1. Overview and trust boundaries

Git's native protocol has no authentication, no authorization and no audit.
W0rkTree builds them into the server and the client.

```
wt CLI ──IPC (127.0.0.1 + token)──► worktree-bg
   │                                   │
   └──────── gRPC + REST (Bearer token, optional TLS / mTLS) ───► worktree-server ──► meta.db + objects/
                                                                     ▲
                                              browser (admin SPA, same origin; CORS allow-list)
```

| Boundary | Trust |
|---|---|
| CLI ↔ daemon | Same user on the same machine. Loopback TCP plus a per-daemon random token. |
| Client ↔ server | Authenticated per request with a bearer token. TLS when configured. The server never trusts client-side decisions. |
| Server ↔ storage | Local disk owned by the server process. Integrity comes from content addressing (BLAKE3). Data is not encrypted at rest. |
| Server ↔ IdP | OIDC over HTTPS: discovery, JWKS and token endpoint. ID tokens are verified. |

## 2. Transport

The server serves gRPC and REST on **one port** (default `:7420`, `--addr`).

- **Cleartext by default.** Without TLS flags the server speaks HTTP/1.1 and
  h2c. It logs a warning when it listens in cleartext on a non-loopback
  address.
- **TLS.** `--tls-cert` / `--tls-key` (`WT_TLS_CERT`, `WT_TLS_KEY`) serve
  HTTPS and gRPC over TLS on the same port, with ALPN `h2`. Clients use
  `https://` remote URLs.
- **mTLS.** `--tls-client-ca` (`WT_TLS_CLIENT_CA`) requires and verifies
  client certificates against that CA. It is optional.
- **Message limits.** gRPC messages are at most 64 MiB in both directions.
  REST JSON bodies are at most 1 MiB, and release assets at most 64 MiB.
- **CORS.** `http://localhost:5173` (the Vite dev server) plus
  `--cors-origins` / `WT_CORS_ORIGINS` are allowed. `*` allows any origin.
  The admin SPA is served from `/admin/` on the same origin.

## 3. Passwords and accounts

- **Hashing.** Passwords are hashed with **argon2id** in PHC format
  (`$argon2id$v=19$m=…,t=…,p=…$salt$key`). The default parameters follow
  RFC 9106 §4: 64 MiB memory, 3 passes, 4 lanes, a 16-byte random salt and a
  32-byte key. Verification:
  - re-derives the key with the stored parameters;
  - compares in constant time;
  - bounds the parameters it accepts from a stored hash;
  - upgrades hashes with outdated parameters on the next successful login.
- **Lengths.** Passwords are 8–1024 bytes, with the minimum counted in
  characters.
- **Usernames.** `^[a-z0-9][a-z0-9-]{1,38}$`. Reserved: `admin`, `anonymous`,
  `api`, `auth`, `events`, `healthz`, `metrics`, `system`.
- **Registration.** Self-registration is controlled by
  `--allow-registration`, which defaults to on. Server admins create accounts
  with `POST /admin/accounts`, which bypasses that setting.
- **Emails are not verified.** Nothing trusts them for identity unless an
  administrator opts in (`link_by_email`, §6).
- **Password changes.** `POST /auth/password` revokes the caller's other
  sessions. An admin password set (`POST /admin/accounts/{u}/password`)
  revokes all of that account's tokens.
- **Disabling** an account takes effect on the next request, because the
  authorizer re-reads the account every time.

## 4. Tokens and sessions

- **Format.** `wt_` followed by the unpadded base64url encoding of 32 random
  bytes from `crypto/rand`. Only the hex SHA-256 is stored, and malformed
  tokens are rejected without a store lookup.
- **Kinds.** `session` (password, MFA or SSO login), `api` (personal) and
  `service` (service-account keys). Sessions default to **30 days**. API
  tokens and keys can be given a TTL (`ttl_hours`) or never expire. All can be
  **scoped** to action patterns ([IAM §11](../iam/IAM.md#11-tokens-scopes-and-authentication)).
- **Presentation.** REST and gRPC use `Authorization: Bearer <token>`. GET
  requests may also send `?access_token=`, for `EventSource` clients that
  cannot set headers.
- **Failures.** A request **with an invalid credential** is rejected:
  HTTP `401` with `WWW-Authenticate: Bearer realm="worktree"`, or gRPC
  `Unauthenticated`. It is never downgraded to anonymous.
- **Use tracking.** Token use is recorded at most once per minute per token
  (`last_used_at`). Sessions also record `last_ip`, `user_agent` and `mfa`.
- **Session management.** `GET /auth/sessions` (the current session is
  marked), `DELETE /auth/sessions/{id}`, `POST /auth/sessions/revoke-others`,
  `POST /auth/logout`. CLI: `wt session list | revoke | revoke-others`,
  `wt logout`. API tokens: `wt token list | create | revoke`.

## 5. Two-factor authentication

TOTP follows RFC 6238: HMAC-SHA-1, 6 digits, a 30-second step and ±1 step of
skew.

- **Enrollment.** `POST /auth/mfa/totp/enroll` returns a 160-bit secret
  (base32) and `otpauth://totp/W0rkTree:<user>?secret=…&issuer=W0rkTree`. The
  admin panel renders the QR code client-side. The secret stays pending until
  `POST /auth/mfa/totp/confirm {code}`.
- **Recovery codes.** Confirmation returns **10 recovery codes** `xxxx-xxxx`
  in Crockford base32. They are stored as SHA-256 hashes and each can be used
  once.
- **Replay protection.** Each accepted step is recorded, so a code cannot be
  replayed.
- **Changes.** Disabling (`DELETE /auth/mfa/totp {password}`) requires the
  password. Regenerating recovery codes requires a current code.
- **Login.** With TOTP enabled, `POST /auth/login` returns `{mfa_required:
  true, mfa_token}`. The `wtm_…` token:
  - is hashed and kept in process memory;
  - is valid for 5 minutes;
  - can be used once;
  - allows 5 wrong codes.

  `POST /auth/login/mfa {mfa_token, code}` accepts a TOTP or a recovery code
  and returns a session with `mfa = true`.
- **Failure limit.** Wrong second factors are limited per account: 10 within
  15 minutes, counted across login, confirmation, regeneration and disable.
- **Policies.** A policy can require MFA with
  `conditions.require_mfa` ([IAM §8.4](../iam/IAM.md#84-conditions)).

## 6. Single sign-on (OpenID Connect)

`internal/sso` is a self-contained OIDC relying party for one provider. It is
configured by server admins with `PUT /admin/sso` or `wt sso set`.

- **Configuration fields:**
  - `enabled`, `name` (the button label);
  - `issuer`, `client_id`, `client_secret` (write-only; reads show
    `has_client_secret`);
  - `redirect_url` (defaults to `--public-url` + `/api/v1/auth/sso/callback`);
  - `scopes` (default `openid email profile`);
  - `allowed_domains`, `auto_create`;
  - `link_by_email` (default **off**);
  - `username_claim` (default `preferred_username`).
- **Flow:**
  1. `GET /auth/sso/start?redirect=/admin/` redirects to the IdP with
     **PKCE (S256)**, a random **state** and a random **nonce**. Both are
     single use and expire after 10 minutes.
  2. The provider configuration comes from
     `/.well-known/openid-configuration` and the JWKS. Both are cached, and
     the JWKS is refreshed when an unknown key id appears.
  3. The code exchange uses `client_secret_basic`, `client_secret_post` or a
     public client.
- **ID-token verification.** RS256 or ES256 (P-256) only, and the algorithm
  must be advertised by the provider. The server checks `iss`, `aud`, `azp`,
  `exp`, `iat`, `nbf` and `nonce`, with 2 minutes of clock skew. A userinfo
  fallback supplies the email.
- **Account mapping**, in order:
  1. An existing `(issuer, subject)` link wins.
  2. When `link_by_email` is on, an existing account with the same
     **verified** email is linked. An email collision without the opt-in is a
     `409` for an administrator to resolve.
  3. When `auto_create` is on, a new account is created, with a sanitized,
     de-duplicated username and an unusable random password.

  When `allowed_domains` is set, a verified email in one of those domains is
  required. A disabled account cannot sign in.
- **Result.** The callback issues a session (MFA when the ID token's `amr`
  names a second factor) and redirects to
  `<redirect>#sso_token=<token>`. Errors redirect to
  `/admin/login#sso_error=<message>`. The redirect target must be a
  same-origin path. Callback responses send `Cache-Control: no-store` and
  `Referrer-Policy: no-referrer`.
- **Audit.** `sso.login`, `sso.login_failed`, `sso.config.update`.

## 7. One-time codes: invitations and password resets

Both codes use the token format (`prefix` + 32 random bytes) and are stored
as SHA-256 hashes.

| Code | Prefix | Default lifetime | Notes |
|---|---|---|---|
| Invitation | `wti_` | 7 days | Single use. Bound to the named email or username when one is given. Revocable. |
| Password reset | `wtr_` | 1 hour | Issued by a server admin (`POST /admin/accounts/{u}/reset-code`, `wt admin reset-code`). Redeemed with `POST /auth/password-reset` or `wt password-reset`. Revokes **every token** of the account. There is no email delivery. |

## 8. Authorization, path ACLs and licenses

Every request is authorized by `internal/authz`. The evaluation order is
covered in [IAM §9](../iam/IAM.md#9-evaluation-algorithm):

- disabled identities are denied;
- token scopes are checked (they bind admins too);
- server admins are allowed;
- tenant status is checked;
- roles, grants and visibility are collected;
- tenant policies apply, with deny overriding allow, and the lockout guard
  protects owners;
- otherwise the request is an implicit deny.

On top of IAM:

- **Path ACLs (v0.5).** `tree:read` path denies stub files on pull and answer
  `403 path_denied` on REST reads. `tree:write` path denies reject pushes
  (`pathacl`).
- **License rules.** Per-path rights (read, write, export) for non-member
  tenants. See [License Compliance](../licensing/LicenseCompliance.md).
- **Object scoping.** `DownloadObjects` requires `x-wt-tree-id` for
  non-admins and serves only objects reachable from that tree. A known hash is
  not enough to fetch an object.

## 9. Rate limits and quotas

| Limit | Default | Response |
|---|---|---|
| Failed logins per (username, client IP) | 10 within 5 minutes, then locked until the window passes. Unknown users and wrong passwords look identical and take equal time. | Login error |
| Wrong second factors per account | 10 within 15 minutes | MFA error |
| Requests per principal (account or service-account id) or, for anonymous callers, per remote IP (v0.5) | `--rate-limit` / `WT_RATE_LIMIT` = 1200 per minute, token bucket. `0` disables it. A tenant's `requests_per_minute` overrides it. | HTTP `429 {"code":"rate_limited"}` with `Retry-After`, `X-RateLimit-Limit`, `X-RateLimit-Remaining`; gRPC `ResourceExhausted`. Counted in `worktree_rate_limited_total`. |
| Tenant quotas: trees, storage, branches per tree, members (v0.5) | Server defaults (unlimited) plus per-tenant overrides | `409 quota_exceeded` / `PushDenied{Kind:"quota_exceeded"}` |

`/healthz` and `/metrics` are exempt from rate limiting. The client IP is the
socket peer address, or — when the peer is a trusted proxy
(`--trusted-proxies`) — the rightmost untrusted `X-Forwarded-For` entry.

## 10. API hardening

- **Strict JSON.** Bodies are limited to 1 MiB, unknown fields are rejected,
  exactly one JSON value is accepted, and the content type is checked.
- **Error envelope.** Errors use one envelope, `{"error":{"code","message"}}`.
  Internal errors are logged with a request id and returned without details.
  Every response carries `X-Request-Id`.
- **Response headers.** Responses carry `X-Content-Type-Options: nosniff`.
  Responses containing secrets (tokens, codes, TOTP secrets) send
  `Cache-Control: no-store`.
- **Blobs.** Blob downloads send `nosniff`, and content-addressed blobs are
  cached as immutable.
- **Status codes.** `501 not_implemented` for disabled features. `403` and
  `401` are distinct, and anonymous callers get `401`.

## 11. Audit log

The audit log is written to `meta.db` through `internal/audit`.

- **Record shape.** Each `domain.AuditEvent` has:
  - `id`: UUIDv7, time-sortable;
  - `tenant_id`: the tenant owning the resource;
  - `actor_id`, `actor` (a username, `anonymous` or `system`);
  - `action`, `resource_type`, `resource_id`;
  - `details`: values of sensitive-looking keys (password, token, secret,
    authorization, api key, …) are replaced with `[REDACTED]`;
  - `remote_addr`, `at`.
- **Append-only.** No API edits or deletes events. Retention is
  `--audit-retention` (default `0`, meaning keep forever). The GC prunes older
  events.
- **Access.** `GET /api/v1/audit?tenant=&action=&resource_type=&resource_id=&since=&until=&limit=&format=ndjson`
  and `wt audit [--tenant] [--action A|prefix.*] [--resource-type]
  [--resource-id] [--since] [--until] [-n N] [--format ndjson]`.
  - Tenant admins see their tenants. Server admins see everything.
  - NDJSON streams every match with no limit, for export to a SIEM.
  - The default page is 100 events, and at most 1000.

Recorded actions include:

| Area | Actions |
|---|---|
| Authentication | `auth.register`, `auth.login`, `auth.login_failed`, `auth.mfa_challenge`, `auth.logout`, `auth.password_change`, `auth.password_change_failed`, `sso.login`, `sso.login_failed` |
| Credentials | `token.create`, `token.revoke`, `session.revoke`, `session.revoke_others`, `mfa.enroll`, `mfa.enable`, `mfa.disable`, `mfa.recovery_codes`, `password.reset_code`, `password.reset`, `account.password_reset` |
| Accounts and tenants | `account.create`, `account.update`, `tenant.create`, `tenant.update`, `member.update`, `member.remove`, `invitation.create`, `invitation.revoke`, `invitation.accept` |
| IAM | `team.*`, `role.*`, `policy.*`, `grant.set`, `grant.remove`, `sa.*`, `sso.config.update`, `access.applied`, `access_config.sync` |
| Trees | `tree.create`, `tree.update`, `tree.delete`, `branch.create`, `branch.delete`, `branch.push`, `branch.push_rejected`, `tag.create`, `tag.delete`, `tag.push`, `protection.update`, `protection.delete`, `license.update`, `license.delete`, `mr.open`, `mr.approve`, `mr.merge`, `mr.close`, `tree.export` |
| Denials and signals | `access.denied` (every REST 403 and gRPC `PermissionDenied`), `license.denied`, `snapshot.signature_invalid`, `todo.skipped` |
| Server | `server.start`, `server.stop` |

## 12. Snapshot signing

Snapshot signing is part of v0.5 (§13.6 of the engineering plan).

- **Keys.** Ed25519 keys are created with `wt identity generate [NAME]
  [--force]`.
  - The private key is a hex seed in `<wt home>/keys/<name>.key`, mode 0600.
    The public key is in `<name>.pub`.
  - Configuration: `[signing] key = "<name>"` and `auto = true|false`.
    `wt snapshot --sign` signs a single snapshot.
  - The private key never leaves the machine.
- **Signature.** The signature is Ed25519 over `signing::signed_payload`: the
  `SnapshotRecord` protobuf with `signature`, `signing_key_id`, `tree_id` and
  `branch_name` cleared. It is stored in `SnapshotRecord.signature` and
  `signing_key_id`, using the server key id after `wt identity register`, or
  else the key fingerprint `SHA256:<base64>`.
- **Registration.** `wt identity register` uploads the public key
  (`POST /auth/keys`). `wt identity revoke ID` revokes it
  (`DELETE /auth/keys/{id}`). `GET /accounts/{username}/keys` lists an
  account's public keys for any signed-in caller.
- **Verification on push.** `signing.Service` is a push checker. Every new
  snapshot that carries a signature must verify against an **unrevoked**
  registered key — of any account, since pushers may push snapshots others
  signed; the key's owner is recorded with the key id. A valid signature marks the snapshot
  `Verified = true`. An invalid one rejects the push with
  `PushDenied{Kind:"signature_required"}` and is audited as
  `snapshot.signature_invalid`. On branches whose protection sets
  `require_signed`, every new snapshot must be verified.
- **Local checks.** `wt identity verify [SNAPSHOT]` and `wt log
  --show-signature` verify locally. The admin panel shows a "verified" badge.

## 13. Secret scanning

Secret scanning runs **on the client, before every snapshot**, in
`worktree_sdk::scan`, including auto-snapshots by the daemon.

- **Built-in rules.** Private-key blocks (checked to contain a key body), AWS
  access and secret keys, GitHub (classic and fine-grained), GitLab, Slack
  tokens and webhooks, Stripe secret keys, Google API keys, npm tokens,
  Anthropic and OpenAI API keys, JWTs, and generic `secret = "…"`
  assignments.
- **Filters.** Matches pass through placeholder suppression, a
  Shannon-entropy floor and a letter-and-digit requirement. Only the first
  8 KiB of each line is scanned, binary files are skipped, and excerpts are
  redacted.
- **Suppression.** A line containing `wt:allow-secret` is ignored.
- **Configuration.** The `[secret_scanning]` section:

  ```toml
  [secret_scanning]
  enabled = true            # default
  block_on_match = true     # default; false = warn only
  exclude = ["fixtures/**"] # globs never scanned
  patterns = [{ name = "internal-token", regex = "itk_[A-Za-z0-9]{32}" }]
  ```

  It can be set in the user config, `.wt/config.toml` or
  `<tree>/.wt-tree/config.toml`.
- **Bypass.** `wt snapshot --no-scan` skips the scan.
- **Result.** A blocking finding aborts the snapshot with the file, line, rule
  and a redacted excerpt.

## 14. Integrity and branch protection

- **Content addressing.** Every object is addressed by BLAKE3-256 of its
  bytes. The server stores only objects whose hash matches and refuses pushes
  that reference missing objects.
- **No history rewriting.** Pushes must fast-forward the server tip, and the
  tip moves by compare-and-swap. There is no force push.
- **Branch protection** is enforced on the server **for everyone, server
  admins included**. There is no bypass permission. The rules are:
  - `no_direct_push`, `allowed_pushers`, `required_approvals`, `no_delete`;
  - v0.5: `require_ci_pass` with `required_checks`, `require_signed`,
    `require_linear`, `block_on_todos`.

  Tag protections (`no_move`, `no_delete`, `allowed_taggers`) apply to tag
  pushes and REST tag changes.

## 15. Client-side secrets and the local daemon

- **Saved logins** are kept in `$WT_HOME/credentials.toml` (default `~/.wt`).
  The file is rewritten atomically and created with mode `0600` on Unix. On
  Windows it inherits the user profile's ACLs. Passwords can be read from
  stdin (`--password-stdin`) or `WT_PASSWORD`, and MFA codes from
  `--mfa-code` / `WT_MFA_CODE`.
- **The daemon** (`worktree-bg`) listens on **127.0.0.1 on a random port**,
  recorded in `$WT_HOME/bg.json`. In v0.5 `bg.json` also holds a random
  `token`. Every IPC request must carry it, and the daemon rejects any request
  that does not. The daemon uses the saved credentials for staging and
  fetching. IPC requests never carry server tokens.

## 16. Threat model

| Threat | Mitigation |
|---|---|
| Eavesdropping or MITM on sync | TLS (`--tls-cert/--tls-key`), optional mTLS. A warning is logged for cleartext on non-loopback addresses. |
| Credential stuffing, brute force | argon2id, per-(user, IP) login lockout, MFA, per-account second-factor limit |
| Stolen token | Hashed at rest. Expiry, scopes, session listing and revocation. Password reset revokes everything. |
| Account takeover through SSO email collision | `link_by_email` off by default, verified email required for linking, `(issuer, subject)` links |
| Privilege escalation | Deny-overrides, grants limited to tree actions, no `server:*` in roles, owner-only owner assignment, declarative escalation check |
| Self-lockout by policy | Lockout guard (`owner_protected`), last-owner guard |
| Code theft or exfiltration | License rules (read, write, export), stubbed paths, scoped object downloads, license-aware exports |
| Secret leakage into history | Pre-snapshot secret scanning |
| History tampering | BLAKE3 addressing, fast-forward-only pushes, CAS, server-verified signatures |
| Supply-chain impersonation | Ed25519 snapshot signatures, `require_signed` |
| Abuse, denial of service | Rate limiting, quotas, message and body limits |
| Insider misuse | Audit log with `access.denied`, NDJSON export |
| Local process abuse of the daemon | Loopback only plus the per-daemon IPC token |

## 17. Implementation status

| Area | Status |
|---|---|
| argon2id passwords, `wt_` tokens with kinds and scopes, sessions, login lockout | Built |
| TOTP MFA with recovery codes and MFA-token login | Built |
| OIDC SSO (PKCE, state, nonce, RS256/ES256, `link_by_email`) | Built |
| Invitations and admin-issued password resets | Built |
| Audit log with redaction; filters; retention and NDJSON (v0.5); `access.denied`, `server.start` / `server.stop` (v0.5) | Built |
| TLS, optional mTLS, cleartext warning | Built (v0.5) |
| Per-principal rate limiting, quotas | Built (v0.5) |
| Path ACLs, license read enforcement, scoped `DownloadObjects` | Built (v0.5) |
| Snapshot signing and verification | Built (v0.5) |
| Client-side secret scanning with config | Built (`[secret_scanning]` honoured in v0.5) |
| IPC token for the daemon | Built (v0.5) |
| Credential file protection on Windows | Partial (relies on profile ACLs) |

## 18. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **QUIC transport** and **certificate pinning** in the client.
- **Mandatory encryption** with "no anonymous mode". Cleartext and anonymous
  reads of public trees are supported by design.
- **Encryption at rest**: per-tenant data keys, envelope encryption, an
  HSM-backed master key and key rotation. The data directory is protected only
  by the operating system.
- **OAuth2 device flow, JWT access tokens and refresh tokens.** Tokens are
  opaque bearer strings. Sessions expire and are renewed with `wt login`.
- **Tokens in `.wt/identity/token`.** Credentials live in
  `$WT_HOME/credentials.toml`. The **OS keychain** is not used.
- **Server-side secret scanning** on push, the `secret_detected` audit event,
  and permission-gated scan overrides. `--no-scan` is local and
  unconditional.
- **Auditing every allow decision** in `access_decision` records that include
  the policies evaluated. Denials are audited, and the simulator explains any
  decision on demand.
- **A protection-bypass permission** (`admin:bypass_protection`).
- **Plan-based rate limits** (requests, sync operations, concurrent syncs per
  plan) and per-tenant aggregate limits. Limits are per principal, with tenant
  overrides.
- **Federation** (server ↔ server mTLS).
- **Incident tooling**: automated alerting, a tenant-wide lockdown switch
  beyond `suspended` / `deactivated`, and extended audit retention during
  incidents.
- **Email verification** and email-based account recovery.
- **`acr` assurance levels** from the IdP (`amr` is honoured).
- **Same-user verification of IPC peers** (named pipes or Unix sockets with
  ACLs). The daemon uses loopback TCP plus a token.

## 19. References

- `services/server-go/internal/iam/` (`password.go`, `token.go`,
  `ratelimit.go`, `sessions.go`, `mfa.go`, `totp.go`, `scopes.go`,
  `invitations.go`, `reset.go`, `middleware.go`, `validate.go`, `doc.go`)
- `services/server-go/internal/sso/` (`sso.go`, `flow.go`, `oidc.go`,
  `redirect.go`)
- `services/server-go/internal/authz/`, `internal/pathacl/`,
  `internal/license/`, `internal/signing/`, `internal/limits/`,
  `internal/audit/`
- `services/server-go/internal/server/server.go` (TLS, interceptors),
  `internal/config/config.go`
- `services/server-go/internal/httpapi/{respond.go,middleware.go,sso.go,iam_security.go,keys.go,ops.go}`
- `crates/worktree-sdk/src/{scan/secrets.rs,signing.rs,config.rs,ipc.rs,remote/credentials.rs}`
- `crates/worktree-bg/`, `crates/worktree-cli/src/commands/{auth,mfa,session,token,identity,audit,sso}.rs`
- `docs/ENGINEERING_PLAN.md` §7, §12.2, §13.3, §13.6, §13.9, §13.11
- Related specs: [IAM](../iam/IAM.md),
  [License Compliance](../licensing/LicenseCompliance.md),
  [Server](../server/Server.md), [Sync](../sync/Sync.md),
  [BgProcess](../bgprocess/BgProcess.md)
