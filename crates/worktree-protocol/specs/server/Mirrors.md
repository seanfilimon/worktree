# W0rkTree Git Mirrors Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Data model](#2-data-model)
- [3. When a mirror syncs](#3-when-a-mirror-syncs)
- [4. Export: snapshots to Git commits](#4-export-snapshots-to-git-commits)
- [5. License-aware content](#5-license-aware-content)
- [6. Push](#6-push)
- [7. Credentials](#7-credentials)
- [8. Remote URLs and the SSRF guard](#8-remote-urls-and-the-ssrf-guard)
- [9. The git binary](#9-the-git-binary)
- [10. Status, events and audit](#10-status-events-and-audit)
- [11. REST API](#11-rest-api)
- [12. CLI](#12-cli)
- [13. Admin panel](#13-admin-panel)
- [14. Implementation status](#14-implementation-status)
- [15. Roadmap](#15-roadmap)
- [16. References](#16-references)

---

## 1. Overview

A **mirror** keeps a copy of a server tree in a Git repository elsewhere
(GitHub, GitLab, a company Git server). It is a *push* mirror: the W0rkTree
server is the source of truth, exports the tree's snapshots as Git commits
and pushes the mirrored branches and tags to the remote after every push and
on demand. Nothing is ever fetched back.

Mirrors belong to one tree and are managed by its administrators
(`tree:admin`). The implementation is the `internal/mirror` package of the
Go server (`mirror.Service`), started with the server; REST routes live in
`internal/httpapi/mirrors.go`, the CLI in `wt mirror` and the admin UI in
the tree's **Mirrors** tab.

Unlike `wt git push` (§13.8, client side), mirrors need no working copy:
the server builds the commits from its own metadata and object store.

## 2. Data model

`domain.Mirror`:

| Field | Meaning |
|---|---|
| `id`, `tenant_id`, `tree_id` | identity and owner |
| `url` | the Git remote (see §8) |
| `has_credential` | a credential is stored (the credential itself is never returned) |
| `branches` | branch glob patterns; empty = every branch |
| `tags` | also mirror the tree's tags (default true) |
| `active` | paused mirrors are not synced |
| `created_by`, `created_at` | who added it |
| `last_sync_at`, `last_status`, `last_error` | outcome of the last sync: `ok`, `error`, `running` or `disabled` (no usable git binary, §9) |

Branch patterns are `/`-separated globs: `*`, `?` and `[...]` match within
one segment, a `**` segment matches any number of segments. `main` matches
only `main`; `release/*` matches `release/1.0` but not `release/1/2`;
`feature/**` matches `feature` and everything below it.

Storage (§15): collection `mirrors`, key `DocKey(treeID, id)`, value the
mirror plus its credential and the creator's account id; collection
`mirror_maps`, key `DocKey(mirrorID)`, value the export state — the
snapshot id → commit id map, the fingerprint of the license filter the
commits were built with, and the object id last pushed for every ref (the
lease expectation of §6). Each mirror also has a bare Git repository
`<data>/mirrors/<id>.git` (rebuilt from scratch if it is lost).

At most 10 mirrors per tree; one tree cannot mirror twice to the same URL.

## 3. When a mirror syncs

- **Automatically**, 5 s after `branch.pushed`, `branch.created`,
  `branch.deleted`, `tag.pushed`, `tag.deleted` or `mr.merged` events of the
  tree (debounced: a burst of pushes gives one sync), shortly after the
  mirror is created or updated, and at server start for mirrors whose last
  sync failed, did not finish or never ran. A failed automatic sync is retried after
  1 m, 5 m, 30 m and 2 h; the next event restarts the cycle.
- **On demand** with `POST {tree}/mirrors/{id}/sync` / `wt mirror sync`.

Syncs of one mirror never overlap (a request waits for the running one).
Deleting a mirror cancels its running sync. `tree.deleted` removes the
tree's mirrors with their state and local repositories; remotes are never
deleted.

## 4. Export: snapshots to Git commits

A sync lists the mirrored refs — `refs/heads/<branch>` for every non-empty
branch matching the patterns, `refs/tags/<tag>` for every tag when `tags`
is on — and walks the history behind their snapshots, stopping at snapshots
already in the map. The new snapshots are written, parents first, in one
`git fast-import` run into the mirror's bare repository; the map then
records their commit ids. Re-syncs are therefore incremental: unchanged
history is never rewritten or re-read.

Commits are deterministic and match `wt git export`
(`crates/worktree-git`), so the same history always has the same commit
ids, on every mirror and every server:

- **author and committer**: the snapshot author parsed like the Rust
  exporter (`"Name <email>"`, `"email"` → name = local part, `"name"`;
  angle brackets and control characters removed; missing parts fall back to
  `unknown` / `unknown@localhost`) at the snapshot's `created_at`, in UTC
  (`+0000`);
- **message**: the snapshot message without trailing whitespace, ending in
  one newline (empty stays empty);
- **tree**: the manifest's files — chunked files reassembled, mode
  `100755` for executables, `100644` otherwise — after the license filter
  of §5;
- **parents**: the snapshot's parents in order (merge snapshots become merge
  commits); parents the server does not have are dropped.

Tags without a message become lightweight tags; tags with a message become
annotated tags (tagger and date from the tag, same identity rules).

Names Git cannot represent are skipped and reported (`result.skipped`):
branch and tag names that fail `git check-ref-format`, and paths with an
empty, `.`, `..` or `.git` component.

## 5. License-aware content

Mirroring redistributes the tree outside the platform, so it applies the
archive rule of §13.8 for an outside reader: for every path the most
specific general (grantee-less, unexpired) license rule decides, and paths
whose rule does not permit **export** — a proprietary license without a
grant, `read-only` or `modify` grants — are left out of the commits
(`result.excluded`). A generated `LICENSES/NOTICE` (same layout as archives)
lists the SPDX id of every included path covered by a rule and how many
paths were excluded; a file of that name in the tree is replaced. Snapshots
with no covered and no excluded path get no notice.

The filter is fingerprinted. When the tree's rules change, the next sync
re-exports the whole history with the new filter — commit ids change and
the push is forced (still under the lease of §6), so a newly restricted
file disappears from the mirrored history.

## 6. Push

After the export the local repository holds exactly the mirrored refs, and
the server runs

```
git push --porcelain [--force-with-lease=<ref>:<last pushed id>]... -- <url> <ref>:<ref>...
```

- Every mirrored ref is pushed; a ref this mirror pushed before carries a
  lease on the id it pushed, so a rewrite (license change) is forced but a
  ref that someone else moved on the remote is **not** overwritten: the push
  is rejected (`stale info`), the sync fails and says so.
- A ref pushed for the first time is a normal push: it fails if the remote
  already has a different, unrelated history there.
- Branches and tags deleted in the tree are deleted on the remote when this
  mirror pushed them (with the lease). Refs that merely stop matching the
  patterns are left alone. Nothing else on the remote is touched.
- The outcome per ref (`created`, `updated`, `forced`, `deleted`,
  `up_to_date`, `rejected`) is returned in `result.refs`; refs that were
  updated are recorded as the new lease expectation.

`git push` is bounded by a 10-minute timeout.

## 7. Credentials

A mirror of an `https://` or `http://` remote may store a **credential**: a
token (sent as `x-access-token:<token>`, which GitHub, GitLab and Gitea
accept) or `user:password`. It never appears on a command line, in logs or
in API responses:

- git receives it as an `http.extraHeader` (`Authorization: Basic …`)
  through the `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_n` / `GIT_CONFIG_VALUE_n`
  environment of the push only;
- every error message is redacted (the credential, its password part and
  the header value become `[redacted]`).

`ssh://` and `user@host:path` remotes use the server's own SSH keys
(`GIT_SSH_COMMAND`, default `ssh -o BatchMode=yes`); they take no
credential. URLs with a password in them are refused. Credentials are
stored in the metadata store like webhook secrets.

Every git command runs with an isolated configuration
(`GIT_CONFIG_NOSYSTEM=1`, an empty `GIT_CONFIG_GLOBAL`), no prompts
(`GIT_TERMINAL_PROMPT=0`, no credential helper or askpass), no HTTP
redirects (`http.followRedirects=false`), `gc.auto=0` and the transports
restricted by `GIT_ALLOW_PROTOCOL` to `https:http:ssh` (plus `file` with
`--mirror-allow-local`).

## 8. Remote URLs and the SSRF guard

Accepted forms: `https://`, `http://`, `ssh://` and scp-like
`user@host:path`. URLs must not contain whitespace or control characters,
start with `-`, or have a user or host starting with `-`.

Mirror hosts pass the same guard as webhooks (Webhooks §7): a host that is,
or resolves to, a loopback, link-local, private or otherwise non-public
address is refused with `400 bad_request` when the mirror is created or
updated, and again before every sync (DNS may have changed). `file://` URLs
(local repositories; spaces allowed in the path) are refused as well.

`--mirror-allow-local` (env `WT_MIRROR_ALLOW_LOCAL=true`) allows both:
`file://` remotes and hosts on private networks — for a company Git server
on the LAN, or tests.

## 9. The git binary

Mirrors run the `git` executable: `--git-binary` (env `WT_GIT_BINARY`,
default `git` looked up on `PATH`), version 2.31 or newer. When it is
missing or too old the server logs a warning at start, mirrors stay
configurable but are **disabled**: `GET {tree}/mirrors` reports
`git.available: false` with the reason, syncs set `last_status: disabled`
and `POST …/sync` answers `409 conflict`. The server's Docker image
(§15.1) is meant to include git for this reason.

## 10. Status, events and audit

Before the work starts the mirror's `last_status` becomes `running`; at
the end `ok` or `error` (with `last_error`, at most 2 KiB) and
`last_sync_at`. The server publishes:

- `mirror.synced` — payload `{mirror_id, url, commits, refs, pushed}`;
- `mirror.failed` — payload `{mirror_id, url, status, error}`;

with the requesting user (manual syncs) or `system` as actor. Both are
regular bus events (SSE, webhooks).

Audit actions: `mirror.create`, `mirror.update` (details: `changed`
fields), `mirror.delete`, `mirror.sync` (manual syncs); details include
`tree`, `url` and `branches`, never the credential.

## 11. REST API

Under `/api/v1/trees/{tenant}/{tree}`, all `tree:admin`:

| Method and path | Body / query | Response |
|---|---|---|
| `GET …/mirrors` | | `{"mirrors":[Mirror], "git":{available, version?, error?}}` (oldest first) |
| `POST …/mirrors` | `{url, branches?, tags?, credential?, active?}` | `201` Mirror |
| `GET …/mirrors/{id}` | | Mirror |
| `PATCH …/mirrors/{id}` | `{url?, branches?, tags?, credential?, active?}` | Mirror |
| `DELETE …/mirrors/{id}` | | `204` |
| `POST …/mirrors/{id}/sync` | `?wait=false` | `200 {mirror, result}`; `202 {mirror}` with `wait=false` |

`PATCH` changes only the fields present: `credential: ""` removes the
credential, `branches: []` mirrors every branch, a new `url` forgets what
was pushed to the old remote. The sync `result` is
`{commits, refs:[{ref, new?, status, reason?}], pushed, excluded?, skipped?, error?, duration_ms}`;
a failed export or push is reported there and in the mirror's status, not
as an error status. Errors: `400 bad_request` (validation, SSRF guard),
`401`, `403`, `404`, `409 conflict` (duplicate URL, more than 10 mirrors,
syncing an inactive mirror, mirrors disabled), `501 not_implemented`
(server without the mirror component).

## 12. CLI

```
wt mirror list                       [--in T/TREE]
wt mirror add URL [-b GLOB]... [--no-tags] [--token-env VAR] [--inactive] [--in T/TREE]
wt mirror show ID                    [--in T/TREE]
wt mirror update ID [--url U] [-b GLOB...|--all-branches] [--tags|--no-tags]
                    [--token-env VAR|--clear-token] [--active|--inactive] [--in T/TREE]
wt mirror remove ID [-y]             [--in T/TREE]
wt mirror sync ID [--no-wait]        [--in T/TREE]
```

Without `--in` the commands address the server tree of the current worktree
(or of the local tree selected with the global `--tree NAME`). v0.8
(§16.11) renamed the argument from `--tree TENANT/TREE`, which shadowed the
global flag; a global `--tree` value of the form `TENANT/TREE` is still
accepted as the server tree with a deprecation warning.
`--token-env VAR` reads the credential from the environment variable `VAR`
so it stays out of shell history and process lists. `list` shows the git
status when mirrors are disabled and every mirror's last error. `sync`
prints the new commits and the per-ref outcome and exits 1 when the sync
failed. `--json` prints the server responses verbatim.

## 13. Admin panel

The tree's **Mirrors** tab lists the mirrors (remote, branches, tags,
credential marker, status badge, last sync, last error), warns when git is
missing on the server, and — for tree administrators — adds and edits
mirrors (URL, branch globs, credential, tags, active), pauses and resumes
them, syncs one now (toast with the outcome) and deletes them. The list
refreshes while a sync is running.

## 14. Implementation status

| Item | Status |
|---|---|
| Mirror CRUD (REST) with audit, limits and validation | Built |
| Deterministic `git fast-import` export (identity, message, executables, chunked files, merge parents, annotated tags) matching `wt git export` | Built |
| Incremental re-sync through the persisted snapshot → commit map; rebuild when the local repository is lost | Built |
| License filter (archive rule) with `LICENSES/NOTICE`; history re-export when the rules change | Built |
| `git push --force-with-lease` of mirrored refs; deletion of refs deleted in the tree | Built |
| Credentials via `http.extraHeader` in the environment only; redaction | Built |
| SSRF guard shared with webhooks; `file://` and private hosts behind `--mirror-allow-local` | Built |
| Debounced automatic syncs, retries with backoff, resume at start, cleanup on `tree.deleted` | Built |
| `--git-binary` detection with a disabled status | Built |
| `mirror.synced` / `mirror.failed` events | Built |
| `wt mirror` CLI, admin Mirrors tab, OpenAPI operations | Built |

## 15. Roadmap

- Pull mirrors (import from a Git remote on a schedule) and two-way sync.
- SSH deploy keys stored per mirror instead of the server's own keys.
- Credential encryption at rest (shared with webhook secrets).
- Signed commits and tags in the mirror from snapshot signatures.
- Per-mirror path filters beyond the license rules.
- `git gc` of the local repositories and size reporting.
- Push progress and per-sync history in the admin panel.

## 16. References

- `docs/ENGINEERING_PLAN.md` §15.6 — binding contract; §13.8 — exports and NOTICE
- `services/server-go/internal/mirror/` — service, exporter, push, scheduler
- `services/server-go/internal/httpapi/mirrors.go` — REST routes; `openapi.yaml` — operations
- `services/server-go/internal/license/notice.go` — shared NOTICE rendering
- `services/server-go/internal/domain/v07.go` — `Mirror`, `mirror.synced`, `mirror.failed`
- `services/server-go/internal/config/config.go` — `--git-binary`, `--mirror-allow-local`
- `crates/worktree-git/src/{export,ident}.rs` — the client-side exporter the commits match
- `crates/worktree-cli/src/commands/mirror.rs` — CLI
- `apps/admin/src/pages/tree/MirrorsTab.tsx` — admin UI
- [`Webhooks.md`](./Webhooks.md) — the SSRF guard; [`Server.md`](./Server.md) — events, audit, errors
