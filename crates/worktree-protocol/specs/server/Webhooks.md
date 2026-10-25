# W0rkTree Webhooks Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Data model](#2-data-model)
- [3. Event selection](#3-event-selection)
- [4. Delivery](#4-delivery)
- [5. Retries, failures and deactivation](#5-retries-failures-and-deactivation)
- [6. Delivery log](#6-delivery-log)
- [7. SSRF guard](#7-ssrf-guard)
- [8. REST API](#8-rest-api)
- [9. CLI](#9-cli)
- [10. Verifying deliveries](#10-verifying-deliveries)
- [11. Implementation status](#11-implementation-status)
- [12. Roadmap](#12-roadmap)
- [13. References](#13-references)

---

## 1. Overview

A webhook makes the server POST a signed JSON document to an HTTP endpoint
whenever an event of interest happens: a push, a merge request change, a
release, a TODO update and so on. Every event the server publishes on its
in-process event bus (the same events streamed by `GET /api/v1/events`)
can be delivered.

A webhook belongs either to a **tenant** (it sees events of every tree of
the tenant and the tenant's own events) or to one **tree** (it sees only
that tree's events). Tenant webhooks are managed by tenant administrators
(`tenant:admin`), tree webhooks by tree administrators (`tree:admin`).

The implementation is the `internal/webhooks` package of the Go server
(`webhooks.Service`), started with the server; REST routes live in
`internal/httpapi/webhooks.go` and the CLI in `wt webhook`.

## 2. Data model

`domain.Webhook`:

| Field | Meaning |
|---|---|
| `id` | UUID |
| `tenant_id`, `tree_id` | Owner; `tree_id` empty = tenant webhook |
| `url` | Target (`http://` or `https://`) |
| `description` | Free text (at most 1000 bytes) |
| `events` | Event-type patterns (§3), at most 50 |
| `active` | Inactive hooks receive nothing (except manual tests/redeliveries) |
| `has_secret` | Always true; the secret itself is never returned after creation |
| `created_by`, `created_at`, `updated_at` | Creator username (or `sa:<tenant>/<name>`) and times |
| `last_delivery_at`, `last_status` | Last attempt; status 0 = network error |
| `failure_count` | Consecutive failed automatic attempts |

`domain.WebhookDelivery` is one delivery of one event (with its retries):
`id`, `webhook_id`, `event`, `payload` (the exact request body sent),
`attempts`, `status_code`, `response` (first 2 KiB of the response body),
`error`, `duration_ms`, `succeeded`, `created_at`, `delivered_at` (last
attempt), `next_retry_at` (set while a retry is scheduled), `redelivery`.

Storage (document collections, `docs/ENGINEERING_PLAN.md` §14):

| Collection | Key | Value |
|---|---|---|
| `webhooks` | `DocKey(tenantID, id)` | the webhook plus its secret and the creator's account id |
| `webhook_deliveries` | `DocKey(webhookID, created_at, id)` | `WebhookDelivery` |

`created_at` in delivery keys is RFC 3339 in UTC with fixed-width
nanoseconds (`2006-01-02T15:04:05.000000000Z`) so that keys sort by time.
Deleting a webhook deletes its delivery log; deleting a tree
(`tree.deleted`) deletes the tree's webhooks and their logs.

Limits: 50 webhooks per tenant scope and per tree; URL at most 2048 bytes;
secret 1-256 bytes.

## 3. Event selection

A hook receives an event when all of these hold:

1. the hook is active;
2. the event belongs to the hook's tenant and — for tree hooks — to the
   hook's tree (tenant-level events such as `access.applied` reach only
   tenant hooks);
3. the event type matches one of the hook's patterns: an exact type
   (`branch.pushed`), a prefix ending in `.*` (`mr.*` matches `mr.opened`,
   `mr.merged`, ...) or `*` (everything). Patterns are lowercase dotted
   words (`[a-z0-9_-]`);
4. the hook's creator could still read the event: `tree:read` on the tree
   for tree events (checked on the tenant when the tree no longer exists,
   e.g. `tree.deleted`), `tenant:read` for tenant events. Creators whose
   account was deleted or disabled read nothing. This is re-checked for
   every event, so revoking the creator's access silences the hook.

Event types in use: `branch.pushed`, `branch.created`, `branch.deleted`,
`tag.pushed`, `tag.deleted`, `tree.created`, `tree.deleted`,
`snapshot.staged`, `staged.cleared`, `mr.opened`, `mr.approved`,
`mr.reviewed`, `mr.commented`, `mr.merged`, `mr.closed`, `group.merged`,
`ci.status`, `todo.created`, `todo.updated`, `release.created`,
`release.deleted`, `access.applied`, `policy.changed`, `webhook.failing`;
`ping` is sent only by the test endpoint.

## 4. Delivery

Each delivery is a `POST` of

```json
{
  "id": "6f1c…",               // delivery id
  "event": "branch.pushed",
  "tenant": "acme",            // tenant slug
  "tree": "web",               // tree name (omitted for tenant events)
  "actor": "ada",
  "at": "2026-05-06T07:08:09Z",
  "payload": { "branch": "main", "old_tip": "…", "new_tip": "…" }
}
```

with the headers

| Header | Value |
|---|---|
| `Content-Type` | `application/json` |
| `User-Agent` | `W0rkTree-Hookshot/<server version>` |
| `X-WT-Event` | the event type |
| `X-WT-Delivery` | the delivery id (stable across retries of one delivery) |
| `X-WT-Hook-ID` | the webhook id |
| `X-WT-Signature` | `sha256=<hex HMAC-SHA256(secret, body)>` |

The `payload` is the event's payload as published on the bus (the same
shape `GET /api/v1/events` streams).

Each attempt has a 10 s timeout. A `2xx` response is a success; any other
status, a timeout or a network error is a failure. Redirects are not
followed (a `3xx` is a failure). Proxy environment variables are ignored.
Deliveries run on a small worker pool so that a slow receiver does not hold
up event processing; deliveries stored but not yet attempted when the
server stopped are attempted after the next start.

## 5. Retries, failures and deactivation

A failed automatic delivery is retried after **1 minute, 5 minutes,
30 minutes and 2 hours** (five attempts in total); the schedule is stored
in the delivery (`next_retry_at`) and survives restarts. Every failed
automatic attempt increments the hook's `failure_count`; any successful
attempt resets it to 0. When `failure_count` reaches **20** the hook is
deactivated (`active: false`), its scheduled retries are dropped and the
server publishes

```json
{"type": "webhook.failing", "tenant_id": "…", "tree_id": "…", "actor": "system",
 "payload": {"webhook_id": "…", "url": "…", "failure_count": 20, "last_status": 500, "last_error": "…"}}
```

Reactivating the hook (`PATCH {"active": true}`) resets the failure count.

Manual deliveries — test pings and redeliveries — are attempted once,
synchronously, also on inactive hooks; they update `last_status` and a
success resets the failure count, but a failure neither counts towards
deactivation nor schedules retries.

## 6. Delivery log

Every delivery is recorded. The log keeps at most the **200** newest
deliveries per hook and none older than **30 days** (pruned when a new
delivery is recorded and hourly). Listing returns the newest first.

## 7. SSRF guard

By default the server refuses webhook targets that are, or resolve to,
loopback (`127.0.0.0/8`, `::1`), link-local (`169.254.0.0/16`,
`fe80::/10`, including cloud metadata addresses), private (`10/8`,
`172.16/12`, `192.168/16`, `fc00::/7`), unspecified, carrier-grade NAT
(`100.64/10`) and a few other non-public ranges (IPv4-mapped IPv6 forms
included):

- at create and update time the host is resolved and the request is
  refused with `400 bad_request` when any address is non-public or the
  host does not resolve;
- at delivery time the dialer re-checks the address it actually connects
  to, so DNS changes (rebinding) cannot bypass the check; such attempts
  fail with "refusing to connect to non-public address".

`--webhook-allow-private` (env `WT_WEBHOOK_ALLOW_PRIVATE=true`) disables
the guard, e.g. for a server whose CI runs on the same private network.
Plain `http://` targets are allowed.

## 8. REST API

Tenant scope (`tenant:admin`) under `/api/v1/tenants/{tenant}`; tree scope
(`tree:admin`) under `/api/v1/trees/{tenant}/{tree}`. Both offer:

| Method and path | Body / query | Response |
|---|---|---|
| `GET …/webhooks` | | `{"webhooks":[Webhook]}` (oldest first) |
| `POST …/webhooks` | `{url, events[], secret?, description?, active?}` | `201` Webhook (+ `secret` once when generated) |
| `GET …/webhooks/{id}` | | Webhook |
| `PATCH …/webhooks/{id}` | `{url?, events?, secret?, description?, active?}` | Webhook |
| `DELETE …/webhooks/{id}` | | `204` |
| `GET …/webhooks/{id}/deliveries` | `?limit=` (default 50, max 200) | `{"deliveries":[WebhookDelivery]}` newest first |
| `GET …/webhooks/{id}/deliveries/{did}` | | WebhookDelivery |
| `POST …/webhooks/{id}/deliveries/{did}/redeliver` | | `201` the new WebhookDelivery (`redelivery: true`) |
| `POST …/webhooks/{id}/test` | | `201` the `ping` WebhookDelivery |

When `secret` is omitted on create the server generates one
(`whsec_` + 64 hex digits) and returns it in the create response only.
A hook of one scope is not visible through another (`404`). Writes need an
authenticated caller (`401`). Validation errors are `400 bad_request`;
more than 50 hooks per scope is `409 conflict`; a server without the
webhooks component answers `501 not_implemented`.

Audit actions: `webhook.create`, `webhook.update` (details: `changed`
fields), `webhook.delete` (details include `url`, `events`, `tree`).
Secrets never appear in audit details.

## 9. CLI

```
wt webhook list                     [--tenant T | --tree T/TREE]
wt webhook create URL -e PATTERN... [--secret S] [--description D] [--inactive] [scope]
wt webhook show ID                  [scope]
wt webhook update ID [--url U] [-e PATTERN...] [--secret S] [--description D] [--active|--inactive] [scope]
wt webhook delete ID [-y]           [scope]
wt webhook deliveries ID [DELIVERY] [-n N] [scope]
wt webhook redeliver ID DELIVERY    [scope]
wt webhook test ID                  [scope]
```

Without `--tenant` or `--tree` the commands address the tree of the
current worktree (its `origin` tenant and remote tree name). `-e` accepts
repeated flags or a comma-separated list. `create` prints the generated
secret once. `test` and `redeliver` print the outcome and exit 1 when the
delivery failed. `--json` prints the server responses verbatim.

## 10. Verifying deliveries

Receivers should compute `HMAC-SHA256(secret, raw request body)`, hex
encode it, compare it with the `X-WT-Signature` value after `sha256=` in
constant time, and reject mismatches. Use `X-WT-Delivery` to deduplicate:
retries of one delivery carry the same id, a redelivery has a new one.
Example (Go): `hmac.Equal([]byte(sig), []byte("sha256="+hex.EncodeToString(mac.Sum(nil))))`;
the server's own helper is `webhooks.Verify(secret, body, signature)`.

## 11. Implementation status

| Item | Status |
|---|---|
| Tenant and tree webhook CRUD (REST) with audit | Built |
| Event-pattern and scope filtering, per-event creator permission re-check | Built |
| Signed JSON deliveries, 10 s timeout, worker pool | Built |
| Retries 1 m / 5 m / 30 m / 2 h, persisted across restarts | Built |
| Deactivation after 20 consecutive failures, `webhook.failing` event | Built |
| Delivery log (200 / 30 days), redeliver, test ping | Built |
| SSRF guard (create-time resolution + dial-time check), `--webhook-allow-private` | Built |
| Cleanup on `tree.deleted` | Built |
| `wt webhook` CLI | Built |
| Admin panel UI | Planned (workstream `admin-v06`) |

## 12. Roadmap

- Per-hook custom headers and content types (form encoding).
- Secret rotation with a grace period (two valid signatures).
- Signature timestamps (`t=…,v1=…`) against replay.
- Payload filters beyond event type (branch globs, paths).
- Delivery metrics on `/metrics` (attempts, failures, latency).

## 13. References

- `docs/ENGINEERING_PLAN.md` §14.2 — binding contract
- `services/server-go/internal/webhooks/` — service, delivery engine, SSRF guard
- `services/server-go/internal/httpapi/webhooks.go` — REST routes
- `services/server-go/internal/domain/v06.go` — `Webhook`, `WebhookDelivery`, `webhook.failing`
- `services/server-go/internal/config/config.go` — `--webhook-allow-private`
- `crates/worktree-cli/src/commands/webhook.rs` — CLI
- [`Server.md`](./Server.md) — events and SSE, audit log, error model
