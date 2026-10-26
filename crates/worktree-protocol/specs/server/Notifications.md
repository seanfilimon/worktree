# W0rkTree Notifications Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Data model](#2-data-model)
- [3. Who is notified](#3-who-is-notified)
- [4. Watch levels](#4-watch-levels)
- [5. Rules applied to every notification](#5-rules-applied-to-every-notification)
- [6. Live delivery (SSE)](#6-live-delivery-sse)
- [7. Email](#7-email)
- [8. REST API](#8-rest-api)
- [9. CLI](#9-cli)
- [10. Admin panel](#10-admin-panel)
- [11. Storage](#11-storage)
- [12. Implementation status](#12-implementation-status)
- [13. Roadmap](#13-roadmap)
- [14. References](#14-references)

---

## 1. Overview

Every user account has an **inbox** of notifications: short records of
something on the server that involves the user (a review request, an
`@mention`, a CI failure on their merge request, ...) or that happened in a
tree they watch. Notifications are created by the server from the events it
publishes on its in-process event bus (the same events streamed by
`GET /api/v1/events` and delivered to webhooks); nothing has to be
configured for the defaults to work.

Notifications are read and managed over REST (`/api/v1/notifications`),
streamed live to the owning account over SSE (`notification.created`),
optionally emailed through an SMTP server, listed by `wt inbox` and shown
by the admin panel's bell and Notifications page.

The implementation is the `internal/notify` package of the Go server
(`notify.Service`), started with the server; REST routes live in
`internal/httpapi/notifications.go`.

Service accounts have no inbox: the notification endpoints answer `403`
for them, and they are never recipients.

## 2. Data model

`domain.Notification`:

| Field | Meaning |
|---|---|
| `id` | UUID |
| `account_id` | Recipient |
| `reason` | Why the recipient got it (table below) |
| `event` | Bus event type that caused it (`mr.opened`, `ci.status`, ...) |
| `tenant_id`, `tree_id` | Where it happened |
| `title` | One line, e.g. `bob requested a review on !3 Add login` |
| `body` | Excerpt (comment text, CI description, release notes; at most 500 bytes) |
| `url` | Admin-panel path, e.g. `/trees/acme/web/merge-requests/3` |
| `actor` | Username of who caused it |
| `created_at`, `read_at` | `read_at` absent while unread |

Reasons, most specific first (a recipient with several reasons for one
event gets the first):

| Reason | Meaning |
|---|---|
| `review_requested` | You (or a team you are in) were asked to review |
| `mentioned` | `@you` (or `@your-team`) in a merge request description, comment or review |
| `ci_failed` | A CI check failed on the source tip of an open merge request you authored or pushed |
| `todo_assigned` | A TODO was assigned to you |
| `mr_merged` | Your merge request was merged |
| `mr_commented` | Your merge request was commented on, reviewed, approved or had a suggestion applied |
| `release` | A release was published in a tree you watch |
| `watching` | Any other merge request event or CI failure in a tree you watch |

`domain.WatchSetting` is an account's level for one tree, or for every tree
of a tenant (`tree_id` empty): `level` (`participating` | `watching` |
`ignoring`), `email` (also email watching notifications) and `updated_at`.

Notification settings per account: `email` (default `true`) — email
participating notifications; `email_configured` (read-only) — whether the
server can send email at all.

## 3. Who is notified

| Event | Participating recipients | Watchers |
|---|---|---|
| `mr.opened` | `@mentions` in title and description; reviewers named in the payload | yes |
| `mr.review_requested` `{number, reviewers: ["user:x","team:y"], requested_by}` | each reviewer; teams of the tree's tenant are expanded to their members; `requested_by` is never notified | yes |
| `mr.ready` | reviewers named in the merge request payload | yes |
| `mr.commented` | the merge request author (`mr_commented`); `@mentions` in the comment | yes |
| `mr.reviewed` | the author; `@mentions` in the review text | yes |
| `mr.approved` | the author; `@mentions` in the approval text | yes |
| `mr.merged` | the author (`mr_merged`) | yes |
| `mr.closed` | — | yes |
| `mr.suggestion_applied` | the author | yes |
| `ci.status` with `failure` / `error` | author of every open merge request whose source tip is the snapshot, plus the snapshot's pusher and author | yes |
| `todo.created`, `todo.updated` | the assignee, when the assignee changed (tracked per TODO) | — |
| `release.created`, `release.updated` | — | yes (`release`), once per release, never for drafts |

Mentions are `@name` tokens not preceded by a letter, digit, `_`, `.`,
`/`, `-` or `@` (so e-mail addresses do not match); `name` resolves to an
account by username, else to a team slug of the tree's tenant.

`tree.deleted` removes the watch settings and per-tree state of the tree;
notifications already created stay until they expire.

## 4. Watch levels

| Level | Effect |
|---|---|
| `participating` (default) | Only participating reasons |
| `watching` | Participating reasons plus every watched event (table above) |
| `ignoring` | Nothing from the tree, not even mentions or review requests |

The effective setting for a tree is the tree's own setting, else the
tenant's, else `participating`. Setting `ignoring` on a tenant silences
every tree of it that has no setting of its own.

## 5. Rules applied to every notification

- **Actor exclusion**: the account whose username is the event's actor is
  never notified (you are not told about your own comment, merge or
  assignment).
- **Read permission**: a recipient must hold `tree:read` on the tree when
  the notification is created; otherwise it is skipped. Disabled accounts
  are skipped.
- **Deduplication**: an identical notification (same account, reason,
  event, tree, target URL, actor, title and body) within one minute is
  created once.
- **Retention**: an account keeps at most 1000 notifications for at most 90
  days; older or excess ones are deleted on insert and hourly.

## 6. Live delivery (SSE)

Each created notification is published as a `notification.created` event
whose payload is the `Notification`. The event has an empty `tenant_id` and
`tree_id`, so tenant- or tree-scoped subscribers (scoped SSE streams,
webhooks) never receive it. `GET /api/v1/events` (and `/events/recent`)
deliver it only on streams without `tenant`/`tree` and only to the
notification's own account — not even server administrators' firehose sees
other accounts' notifications.

## 7. Email

Email is enabled with `--smtp-url` (env `WT_SMTP_URL`):

- `smtp://[user:pass@]host[:port]` — default port 587; STARTTLS is used
  when the server offers it; credentials are sent with AUTH PLAIN, which
  Go's SMTP client allows only over TLS or to localhost.
- `smtps://[user:pass@]host[:port]` — implicit TLS, default port 465.

`--smtp-from` (env `WT_SMTP_FROM`) sets the From address (default
`"W0rkTree" <noreply@HOST>` with the host of `--public-url`, else
`localhost`). Without `--smtp-url` no email is sent. Invalid values stop the
server at startup; the URL is never echoed (it may contain a password).

A notification is emailed when SMTP is configured, the account has an
email address and either the account's `email` setting is on (participating
reasons) or the effective watch setting has `email` (any reason). Messages
are `multipart/alternative` (plain text and HTML, quoted-printable UTF-8)
with the subject `[tenant/tree] <title>`, headers `Auto-Submitted:
auto-generated` and `X-WT-Notification: <id>`, and — when `--public-url`
is set — links to `<public-url>/admin<url>` and the Notifications page.
HTML is escaped by `html/template`.

Emails are queued (256 pending) and sent by a background sender; a failed
attempt is retried after 30 s, 2 m, 10 m and 30 m, then dropped with a log
line. The queue is in memory: emails pending at shutdown are lost.

## 8. REST API

All routes act on the caller's own account (`x-wt-action: self`) unless
noted; without the notify service they answer `501 not_implemented`.

| Method and path | Purpose |
|---|---|
| `GET /api/v1/notifications?unread=true&limit=N` | `{"notifications": [...newest first], "unread_count"}`; `limit` default 50, max 1000 |
| `POST /api/v1/notifications/{id}/read` | Mark one read (idempotent) → the notification |
| `POST /api/v1/notifications/read-all` | Mark every unread one read → `{"updated": n}` |
| `DELETE /api/v1/notifications/{id}` | Remove one ("done") → 204 |
| `GET /api/v1/notifications/settings` | `{"email", "email_configured", "updated_at"?}` |
| `PUT /api/v1/notifications/settings` | `{"email"?: bool}` |
| `GET /api/v1/notifications/watches` | `{"watches": [WatchSetting + "tenant" slug + "tree" name]}` |
| `GET\|PUT /api/v1/trees/{tenant}/{tree}/watch` | Tree setting (`tree:read`); PUT `{"level", "email"?}` (omitted `email` is unchanged) |
| `GET\|PUT /api/v1/tenants/{tenant}/watch` | Tenant setting (`tenant:read`) |

A GET of a watch scope without a stored setting returns the default
(`participating`, `email: false`, zero `updated_at`). Unknown levels are
`400`. Ids of other accounts' notifications are `404`.

## 9. CLI

| Command | Effect |
|---|---|
| `wt inbox [--all] [-n N] [--json]` | Unread notifications (`--all`: read ones too) with a short id, reason, tree, age and title |
| `wt inbox read ID... \| --all` | Mark read; ids may be unique prefixes |
| `wt inbox done ID...` | Remove notifications |
| `wt inbox watch [TREE] [--tenant T] [--level participating\|watching\|ignoring] [--email\|--no-email]` | Show or set a watch level (default tree: the current worktree's) |
| `wt inbox watches` | Stored watch settings |
| `wt inbox settings [--email on\|off]` | Show or change notification settings |

## 10. Admin panel

- A bell in the top bar shows the unread count and previews the newest
  unread notifications (open one to mark it read, "Mark all read", "View
  all"). It refreshes on `notification.created` from the shell's unscoped
  event stream and once a minute.
- `/notifications`: the inbox (unread / all, filter by reason, mark read,
  done, mark all read), the email setting and the list of watched trees and
  tenants with level and email controls.
- A "Watch" menu on every tree header: the three levels with descriptions
  and "Also email me watching notifications".

## 11. Storage

| Collection | Key | Value |
|---|---|---|
| `notifications` | `DocKey(accountID, created_at as fixed-width RFC 3339 nanoseconds, id)` | `Notification` |
| `watch_settings` | `DocKey(accountID, tenantID, treeID)` (`treeID` "" for a tenant) | `WatchSetting` |
| `notification_settings` | `accountID` | `{email, updated_at}` |
| `notify_state` | `DocKey("todo", treeID, %010d number)` / `DocKey("release", treeID, tag)` | last notified TODO assignee / announced release id |

Deduplication state is in memory.

## 12. Implementation status

| Item | Status |
|---|---|
| Bus-driven notifications for every reason in §2 | Built |
| Team expansion of reviewers and `@team` mentions | Built |
| Actor exclusion, `tree:read` check, one-minute dedup, 1000 / 90-day retention | Built |
| Tree and tenant watch levels with email flag | Built |
| REST endpoints and OpenAPI operations | Built |
| Per-account `notification.created` over SSE | Built |
| SMTP email (STARTTLS / implicit TLS, AUTH PLAIN, text + HTML, retries) | Built |
| `wt inbox` CLI | Built |
| Admin bell, Notifications page, tree Watch menu | Built |
| `mr.review_requested` producers (path owners, reviewer requests) | Workstream `review-v07` |

## 13. Roadmap

- Persistent email queue and per-account digest emails.
- Unsubscribe links (`List-Unsubscribe`) with signed tokens.
- Per-reason preferences (e.g. no emails for `mr_commented`).
- Notification threads (group by merge request) and "mark thread read".
- Mentions in snapshot messages and TODO details.

## 14. References

- `docs/ENGINEERING_PLAN.md` §15.3 — binding contract
- `services/server-go/internal/notify/` — rules, storage, email sender
- `services/server-go/internal/httpapi/notifications.go` — REST routes
- `services/server-go/internal/httpapi/events.go` — SSE filter for `notification.created`
- `services/server-go/internal/domain/v07.go` — `Notification`, `WatchSetting`, levels, events
- `services/server-go/internal/config/config.go` — `--smtp-url`, `--smtp-from`
- `crates/worktree-cli/src/commands/inbox.rs` — CLI
- `apps/admin/src/pages/notifications/`, `apps/admin/src/components/shell/NotificationBell.tsx` — admin UI
- [`Server.md`](./Server.md) — events and SSE; [`Webhooks.md`](./Webhooks.md) — the other bus consumer
