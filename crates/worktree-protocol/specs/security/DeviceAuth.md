# Device Authorization and Refresh Tokens Specification

The OAuth 2.0 device authorization grant (RFC 8628) for `wt login --device`,
and short-lived access tokens renewed with single-use, rotating refresh
tokens (`docs/ENGINEERING_PLAN.md` §16.6, v0.8 workstream `device-auth`).

## Table of Contents

1. [Overview](#1-overview)
2. [Token pairs](#2-token-pairs)
3. [Refresh, rotation and reuse detection](#3-refresh-rotation-and-reuse-detection)
4. [Device authorization flow](#4-device-authorization-flow)
5. [Approving a device](#5-approving-a-device)
6. [Rate limits and expiry](#6-rate-limits-and-expiry)
7. [Storage](#7-storage)
8. [Clients: SDK, CLI and admin panel](#8-clients-sdk-cli-and-admin-panel)
9. [REST reference](#9-rest-reference)
10. [Threat model](#10-threat-model)
11. [Implementation status](#11-implementation-status)
12. [Roadmap](#12-roadmap)
13. [References](#13-references)

---

## 1. Overview

Before v0.8 every login produced one opaque session token valid for 30
days. Since v0.8 interactive logins produce a **token pair**:

- an **access token**: an ordinary `wt_` bearer token of kind `session`,
  valid **1 hour**, sent as `Authorization: Bearer` on REST and gRPC;
- a **refresh token** (`wtrt_<family>.<secret>`), valid **30 days** from its
  issue and **single use**, exchanged at `POST /api/v1/auth/refresh` for a
  new pair.

A machine without a browser (or where typing a password is undesirable)
signs in with the **device flow**: the CLI shows a short user code, the user
approves it in the admin panel where they are already signed in (with MFA if
enabled), and the CLI receives a token pair.

```
wt login --device                         admin panel (signed in)
  │ POST /auth/device ──► device_code, user_code BCDF-GHJK
  │ prints code + /admin/device?code=BCDF-GHJK ─────────► /device: review
  │ POST /auth/device/token (every 5 s)                       │ approve (+ TOTP)
  │   ◄── 400 authorization_pending                           ▼
  │   ◄── 200 {token, refresh_token}  ◄── approved ── POST /auth/device/{code}/approve
  ▼
credentials.toml: token + refresh_token + expires_at
```

## 2. Token pairs

Issued by `POST /auth/login` (accounts without MFA), `POST /auth/login/mfa`,
`POST /auth/register` and the device token endpoint. The login response
keeps its v0.4 shape and gains fields:

```json
{"token": "wt_...", "account": {...}, "token_type": "Bearer",
 "expires_in": 3600, "refresh_token": "wtrt_...", "refresh_expires_in": 2592000}
```

- The access token is a session (`kind` `session`) listed by
  `GET /auth/sessions` with its client address, user agent and MFA flag.
  Its scopes and MFA flag are those of the login; device sessions carry the
  scopes requested by the device and are named `device: <client name>`.
- Clients that ignore `refresh_token` keep working for an hour per login.
- SSO logins (`#sso_token=`) and personal API tokens are unchanged: no
  refresh token, their own lifetimes.
- Server options (`iam.Options`): `AccessTTL` (default 1 h), `RefreshTTL`
  (default 30 days), `RefreshReuseGrace` (default 10 s).

## 3. Refresh, rotation and reuse detection

`POST /api/v1/auth/refresh {"refresh_token": "wtrt_..."}` (no
`Authorization` header) returns a new pair. Every refresh token belongs to a
**family**, one per login:

1. The presented token must be the family's **current** token, unexpired,
   the family not revoked and the account enabled. The answer is a new
   access token (the previous access session is deleted, so a login is one
   row in the session list) and a new refresh token valid 30 days from now.
2. Presenting a token the family already **rotated** is reuse — a copy is in
   someone else's hands. The family is **revoked**: its current access
   session is deleted and every token of the family stops working; the
   answer is `401 invalid_grant` and the event is audited as
   `auth.refresh_reuse`.
3. Exception for client races: the **immediately previous** token presented
   within **10 seconds** of its rotation (two processes sharing a
   credential file refreshing at once) is rejected with `401 invalid_grant`
   without revoking anything; the client re-reads the rotated token.
4. Unknown, malformed, expired or revoked tokens answer `401 invalid_grant`.

Revocation also follows sessions: `POST /auth/logout`,
`DELETE /auth/sessions/{id}` and `wt session revoke` end the session's
family; `POST /auth/sessions/revoke-others` ends every other family of the
account; a password reset ends all of them.

## 4. Device authorization flow

1. **Start.** `POST /api/v1/auth/device` (public). Optional JSON body:
   `client_name` (shown to the approving user, default the User-Agent; the
   CLI sends `wt CLI on <hostname>`), `client_id` (`[A-Za-z0-9._-]`, ≤ 64;
   the CLI sends `wt-cli`), `scopes` (action patterns, like scoped API
   tokens; or the RFC 6749 space-separated `scope`). Answer:

   ```json
   {"device_code": "wtd_...", "user_code": "BCDF-GHJK",
    "verification_uri": "https://wt.example.com/admin/device",
    "verification_uri_complete": "https://wt.example.com/admin/device?code=BCDF-GHJK",
    "expires_in": 600, "interval": 5}
   ```

   The base URL is `--public-url` when configured, else the scheme and
   host of the request. User codes are 8 characters from the 20 consonants
   `BCDFGHJKLMNPQRSTVWXZ` (no vowels, so no words; ≈ 2.6·10¹⁰ codes),
   shown as `XXXX-XXXX` and accepted in any case with or without the hyphen.
   The device code is a 256-bit secret only the device knows.
2. **Poll.** `POST /api/v1/auth/device/token {"device_code", "grant_type"?}`
   (public; `grant_type`, when given, must be
   `urn:ietf:params:oauth:grant-type:device_code`). Errors are `400` with
   the OAuth code in the error envelope:

   | `error.code` | Meaning | Client action |
   |---|---|---|
   | `authorization_pending` | Not decided yet | Poll again after `interval` |
   | `slow_down` | Polled before the interval passed (1 s tolerance) | Add 5 s to the interval |
   | `access_denied` | The user denied | Stop |
   | `expired_token` | The codes expired (10 minutes) | Start again |
   | `invalid_grant` | Unknown or already redeemed device code | Stop |
   | `unsupported_grant_type` | Wrong `grant_type` | Fix the client |

   After approval the next poll returns a token pair (§2) plus the account
   and is audited as `auth.login` with `method=device`. The device code is
   then deleted: it is redeemed exactly once.

## 5. Approving a device

The admin page `/admin/device` (`verification_uri`; with `?code=` the code
is prefilled but still confirmed by the user) calls, as the signed-in user:

- `GET /api/v1/auth/device/{user_code}` → `{user_code, client_id,
  client_name, scopes, status, remote_ip, user_agent, created_at,
  expires_at, mfa_required}`. `remote_ip` and `user_agent` are the device's,
  so the user can recognise the request.
- `POST /api/v1/auth/device/{user_code}/approve {"code"?}` and
  `POST /api/v1/auth/device/{user_code}/deny`, audited as
  `auth.device_approve` / `auth.device_deny`. A request that is no longer
  pending answers `409`.

Rules:

- Only user accounts with an unscoped session or token may approve
  (scoped tokens and service accounts: `403`).
- **MFA is respected.** When the account has TOTP enabled and the caller's
  session was not created with a second factor, `mfa_required` is true and
  approving needs a current TOTP code or an unused recovery code in `code`
  (`403` otherwise; wrong codes count against the account's MFA rate limit,
  [Security §5](./Security.md#5-two-factor-authentication)). The device
  session's `mfa` flag is set when the account has MFA, so MFA-conditioned
  policies treat it like an MFA login.
- The device receives the approving account's identity with the scopes the
  device requested (empty = full access); the approver cannot widen them.

## 6. Rate limits and expiry

| Limit | Value | Answer |
|---|---|---|
| Device authorizations started per client address | 30 per 10 minutes | `429 rate_limited` |
| Pending device authorizations (server-wide) | 10 000 | `429 rate_limited` |
| Unknown / malformed user codes per account | 10 per 10 minutes | `429 rate_limited` |
| Polling interval | 5 s, +5 s per `slow_down` | `400 slow_down` |
| Device and user code lifetime | 10 minutes | `expired_token` / `404` |
| Second-factor failures | MFA limit (10 per 15 minutes) | `403` |

The general per-principal request limits ([Security §9](./Security.md#9-rate-limits-and-quotas))
apply on top. Expired device authorizations are swept when new ones start
(at most every 30 s); expired refresh families are pruned during
revocation scans.

## 7. Storage

DocStore collections (§13.14 conventions):

| Collection | Key | Value |
|---|---|---|
| `device_codes` | `DocKey("u", USER-CODE)` | the request: device-code hash, client, scopes, status, device address and user agent, interval, last poll, approver, MFA flag, timestamps |
| `device_codes` | `DocKey("d", sha256(device_code))` | `{user_code}` (index from the device code) |
| `refresh_tokens` | family id (UUID) | the family: account, current access session id, session name, scopes, MFA flag, sha256 of the current refresh token, issue / expiry, hashes of rotated tokens (last 1000) with rotation times, revocation time and reason |

Only SHA-256 hashes of device codes and refresh tokens are stored. The raw
refresh token embeds the family id so a lookup needs no index; comparisons
are constant time. Updates are atomic per document (`UpdateDoc`), so a
refresh token or device code cannot be redeemed twice concurrently.

## 8. Clients: SDK, CLI and admin panel

**SDK** (`worktree-sdk::remote`). `credentials.toml` stores `token`,
`refresh_token` and `expires_at` per server (file mode `0600` on Unix,
rewritten atomically). The SDK renews transparently:

- before connecting (gRPC sessions, clones, lazy fetches, `RestClient`
  requests) when the access token expires within two minutes;
- after `Unauthenticated` / `401`: the session opener and `wt whoami`
  reconnect once, `RestClient` retries the request once;
- when the saved token already differs from the rejected one (another
  process renewed it) that token is used without a request; when the
  server rejects a refresh token the file is re-read once after 300 ms to
  catch a concurrent rotation.

`WT_TOKEN` credentials and saved API tokens are never refreshed. APIs:
`login_device_start`, `login_device_wait`, `refresh_credential`,
`Credential::new`.

**CLI.** `wt login [SERVER] --device [--scope ACTION]... [--no-browser]`
prints the code and URL on stderr, opens the browser (`$BROWSER`, else the
platform opener) only when stderr is a terminal and `WT_NO_BROWSER` is
unset, waits, and saves the pair. `wt session list` has a `via` column
(`sign-in` or `device (<client>)`).

**Admin panel.** `/device` (inside the authenticated shell, so signing in
first returns to it): enter or confirm the code, review client, scopes,
device address and expiry, enter a second-factor code when required,
approve or deny. The API client stores the refresh token (`localStorage`
`wt.admin.refresh`) and renews once on a `401`, sharing one refresh between
concurrent requests and tabs.

## 9. REST reference

| Method and path | Auth | Answer |
|---|---|---|
| `POST /api/v1/auth/device` | public | device authorization (§4) |
| `POST /api/v1/auth/device/token` | public (device code) | token pair or OAuth error (§4) |
| `GET /api/v1/auth/device/{user_code}` | signed-in user | device request (§5) |
| `POST /api/v1/auth/device/{user_code}/approve` | signed-in user (+ MFA code) | device request |
| `POST /api/v1/auth/device/{user_code}/deny` | signed-in user | device request |
| `POST /api/v1/auth/refresh` | public (refresh token) | token pair or `401 invalid_grant` (§3) |

Token pair: `{token, token_type: "Bearer", expires_in, refresh_token,
refresh_expires_in, account}`. All answers carrying secrets send
`Cache-Control: no-store`. Every operation is in `openapi.yaml` (tag `Auth`).

## 10. Threat model

| Threat | Mitigation |
|---|---|
| Stolen access token | Valid at most 1 hour |
| Stolen refresh token | Single use; the next use by either party revokes the family (§3) |
| Guessing user codes | 2.6·10¹⁰ codes, 10-minute lifetime, 10 failures per account per 10 minutes |
| Phishing ("approve this code") | The page shows the device's client name, address, user agent and requested access and warns to approve only sign-ins the user started; codes expire quickly |
| Approval with a stolen password-only session | Accounts with MFA must present a second factor unless the session already verified one |
| Polling floods | `slow_down` interval growth plus per-address and server-wide limits |
| Replay of a redeemed device code | Deleted on redemption (`invalid_grant`) |
| Credential file disclosure | `0600` on Unix; tokens are hashed server-side |

## 11. Implementation status

| Item | Status |
|---|---|
| Token pairs from password, MFA, registration and device logins | Built (v0.8) |
| Refresh rotation, reuse detection with race grace, family revocation with sessions | Built (v0.8) |
| RFC 8628 device flow (start, poll errors, slow down, approve / deny with MFA) | Built (v0.8) |
| Rate limits, expiry and sweeping | Built (v0.8) |
| SDK transparent refresh, `credentials.toml` pairs | Built (v0.8) |
| `wt login --device`, `wt session list` device sessions | Built (v0.8) |
| Admin `/device` page, admin client refresh | Built (v0.8) |

## 12. Roadmap

- Refresh tokens for SSO logins (the redirect hands over only an access
  token, which keeps the 30-day session lifetime).
- JWT (self-contained) access tokens; tokens are opaque and looked up.
- Revoking the server-side login on `wt logout` (it forgets the local
  credential only).
- Device-flow `client_id` registration and per-client policies; any client
  may start a flow.
- Refreshing a gRPC stream in the middle of a long transfer (renewal happens
  when a connection is opened).

## 13. References

- `services/server-go/internal/iam/{device.go,refresh.go}` (+ tests)
- `services/server-go/internal/httpapi/device.go`, `auth.go`,
  `iam_security.go`, `openapi.yaml`
- `crates/worktree-sdk/src/remote/{auth.rs,device.rs,session.rs,credentials.rs,rest.rs}`
- `crates/worktree-cli/src/commands/{auth.rs,session.rs}`
- `apps/admin/src/{pages/DevicePage.tsx,api/device.ts,api/client.ts}`
- `crates/worktree-e2e/tests/e2e/device_auth.rs`
- RFC 8628 (device authorization grant), RFC 6749 §5.2 (token errors),
  OAuth 2.0 Security BCP (refresh token rotation)
- Related specs: [Security](./Security.md), [IAM](../iam/IAM.md)
