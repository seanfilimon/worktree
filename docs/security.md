# Security

This page describes the security properties of the v2 implementation: what is enforced, where, and
what is not (yet). The long-form threat model and future mechanisms are in the
[Security spec](../crates/worktree-protocol/specs/security/Security.md); anything only described
there is marked *Roadmap* below.

- [Secret scanning](#secret-scanning)
- [Accounts, passwords and tokens](#accounts-passwords-and-tokens)
- [Authorization](#authorization)
- [History guarantees](#history-guarantees)
- [Integrity](#integrity)
- [License enforcement](#license-enforcement)
- [Audit log](#audit-log)
- [Transport and deployment](#transport-and-deployment)
- [Local files](#local-files)
- [Roadmap](#roadmap)
- [Reporting vulnerabilities](#reporting-vulnerabilities)

---

## Secret scanning

Every `wt snapshot` (and every daemon auto-snapshot) scans the added and modified files for
credentials **before** anything is written. If a rule matches, the snapshot is refused with
`SecretsDetected` and the findings — rule, path, line and a redacted excerpt such as
`AWS_ACCESS_KEY_ID=AKIA****MPLE` — are printed. Nothing is stored.

The scanner (`worktree_sdk::scan::scan_secrets`) is a pure function: binary content is skipped,
matches never span lines, and each finding carries the rule id, path, 1-based line and a redacted
excerpt.

### Built-in rules

| Rule id | Detects |
|---|---|
| `private-key` | PEM / OpenSSH / PGP private key blocks (only when a key body follows the header) |
| `aws-access-key` | AWS access key ids (`AKIA`, `ASIA`, `ABIA`, `ACCA` + 16 characters) |
| `aws-secret-key` | 40-character AWS secret keys assigned to `aws_*secret*`-style variables |
| `github-fine-grained-token` | GitHub fine-grained PATs (`github_pat_…`) |
| `github-token` | GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`) |
| `gitlab-token` | GitLab personal access tokens (`glpat-…`) |
| `slack-webhook` | Slack incoming webhook URLs |
| `slack-token` | Slack API tokens (`xoxb-`, `xoxa-`, `xoxp-`, `xoxr-`, `xoxs-`) |
| `stripe-secret-key` | Stripe live secret / restricted keys (`sk_live_`, `rk_live_`) |
| `google-api-key` | Google API keys (`AIza…`) |
| `npm-token` | npm access tokens (`npm_…`) |
| `anthropic-api-key` | Anthropic API keys (`sk-ant-…`) |
| `openai-api-key` | OpenAI-style keys (`sk-`, `sk-proj-`, `sk-svcacct-`, `sk-admin-`) |
| `jwt` | Signed JSON Web Tokens |
| `generic-secret` | High-entropy values assigned to `password`, `secret`, `api_key`, `token`, `credential`, `access_key`, `private_key` variables (quoted, `.env` and YAML styles) |

Generic rules apply an entropy floor and ignore obvious placeholders (`change-me`, `xxxx`, …) to
keep false positives low.

### Handling false positives

| Option | Scope |
|---|---|
| Add `wt:allow-secret` in a comment on the same line | That line only |
| `wt snapshot --no-scan` | That one snapshot |
| `SnapshotOptions { skip_secret_scan: true, .. }` | SDK callers |

```python
TEST_KEY = "AKIAIOSFODNN7EXAMPLE"  # wt:allow-secret (documented AWS example key)
```

Prefer the inline marker: it is reviewable and survives in history. Scanning is client-side; a
malicious client can skip it, so it protects against accidents, not adversaries.

## Accounts, passwords and tokens

| Item | Implementation |
|---|---|
| Password storage | **argon2id** with a fresh random salt per hash, stored in PHC string format (`$argon2id$v=19$m=…,t=…,p=…$salt$key`). Hashes with outdated parameters are upgraded on the next successful login. |
| Password policy | 8 to 1024 bytes. |
| Verification | Constant-time comparison; unknown usernames and wrong passwords return the same error. |
| Login rate limiting | Failed logins are limited per (username, client address); by default 10 failures within 5 minutes lock that pair until the window passes. |
| Usernames | Lowercase `[a-z0-9-]`, 2–39 characters. Reserved names (`admin`, `anonymous`, `api`, `auth`, `events`, `healthz`, `metrics`, `system`) cannot be registered; `admin` is created only through `--admin-password`. |
| Bearer tokens | `wt_` + base64url of 32 random bytes. The server stores only the **hex SHA-256** of each token, so a database leak does not reveal usable tokens. |
| Session lifetime | Tokens from `POST /api/v1/auth/login` expire after 30 days by default. |
| API tokens | Named tokens with optional expiry (`ttl_hours`); listed and revoked individually. The raw value is shown once. |
| Revocation | `wt logout` and `POST /api/v1/auth/logout` revoke the token used; `DELETE /api/v1/auth/tokens/{id}` revokes any of your tokens. |
| Disabled accounts | Server admins can disable an account; every action is then refused. |

Tokens travel as `Authorization: Bearer <token>` (REST) or `authorization` gRPC metadata. GET
requests may pass `access_token=<token>` as a query parameter so browser `EventSource` clients can
subscribe to SSE; avoid this form elsewhere, since URLs end up in logs.

On the client, `wt` never prints tokens (also not with `--json`) and stores them in
`$WT_HOME/credentials.toml`, created with mode `0600` on Unix. On Windows the file inherits the
ACL of the user profile directory.

## Authorization

All access control is enforced by the server; clients only display it. Every gRPC and REST request
is authorized against the caller's role in the tree's tenant, tree visibility, and the server-admin
flag — see the [role matrix](./server-architecture.md#role-matrix) and
[visibility rules](./server-architecture.md#visibility). Defaults are closed: new trees are
`private`, unknown visibility values are treated as `private`, unknown roles grant nothing.

Branch protection is never bypassed — not even by server admins. Pushes are fast-forward only for
everyone.

## History guarantees

W0rkTree has no command or RPC that rewrites history.

| Guarantee | Mechanism |
|---|---|
| Snapshots are immutable | Records are written once; the server rejects a different record under an existing id. |
| No force-push | The server accepts a push only if `new_tip` descends from the current tip; branch tips move by compare-and-swap. |
| No hard delete of branches locally | `wt branch archive` is a soft delete; `wt branch restore` brings it back. |
| Undo is additive | `wt revert` creates a new snapshot. |
| Local operations are recorded | `.wt/trees/<tree>/reflog.jsonl` is append-only. |
| Server actions are recorded | The audit log is append-only and has no delete API. |
| Tags are immutable on the server | Re-pushing an existing tag name is refused. |

`wt restore` and `wt switch --force` discard *uncommitted* working-directory changes by design; they
never touch snapshots.

## Integrity

- Blobs and manifests are addressed by BLAKE3-256 of their exact bytes. The client verifies hashes
  when reading and downloading; the server recomputes the hash of every uploaded object and rejects
  mismatches.
- Manifests are canonical (sorted, unique, validated paths), so equal file sets always hash equally.
- Snapshot ids are random UUIDs and are not content hashes; integrity of a snapshot's *content* comes
  from its manifest hash. Signed snapshots are *Roadmap*.

## License enforcement

Trees can attach SPDX licenses with grants (`read-only`, `modify`, `redistribute`) to path globs.
The server enforces them on push for members of other tenants and for anonymous callers, and fails
closed on unknown grants. See [license enforcement](./server-architecture.md#license-enforcement).
License rules restrict what *other tenants* may do; they are not a confidentiality boundary within
the owning tenant.

## Audit log

The server records logins (including failures), token changes, membership and tree changes, pushes
(including rejections), tags, protection and license changes and merge-request transitions. Entries
include actor, action, resource, remote address and time; sensitive detail values are redacted.
See [audit log](./server-architecture.md#audit-log).

## Transport and deployment

- The server listens in **cleartext** (gRPC over h2c and plain HTTP on one port). Tokens and file
  contents are therefore readable on the network unless you terminate TLS in front of the server
  with a reverse proxy that supports HTTP/2 to the upstream.
- Bind to `127.0.0.1` (`--addr 127.0.0.1:7420`) when the proxy runs on the same host.
- Pass the admin password via `WT_ADMIN_PASSWORD` rather than `--admin-password`.
- Disable self-registration on private deployments: `--allow-registration=false`.
- CORS allows only `http://localhost:5173` plus explicitly configured origins.
- gRPC messages are capped at 64 MiB in each direction.

## Local files

- The daemon's IPC listens on `127.0.0.1` only, on a port recorded in `$WT_HOME/bg.json`. It is not
  authenticated: any local process can ask it to snapshot, watch or unwatch a directory, pause, or
  shut down. Avoid running the daemon on shared multi-user hosts.
- `.wt/` contains full history in plain files. Anyone who can read the worktree can read its history.

## Roadmap

Specified in [`specs/security/Security.md`](../crates/worktree-protocol/specs/security/Security.md)
but **not implemented** in v2:

- Native TLS 1.3 / QUIC transport and mTLS.
- OAuth2 device flow and JWT refresh tokens.
- Ed25519 snapshot signing and "require signed snapshots" protection.
- Declarative RBAC + ABAC policies and permission ceilings.
- Per-tenant encryption at rest with key rotation.
- Owner-only IPC sockets / named pipes for the daemon.
- Server-side secret scanning.

## Reporting vulnerabilities

Do not open a public issue. Report vulnerabilities privately to the maintainers as described in
[CONTRIBUTING.md](../CONTRIBUTING.md#security-vulnerabilities).
