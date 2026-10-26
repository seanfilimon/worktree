# Review Workflow

The v0.7 review features of merge requests (`docs/ENGINEERING_PLAN.md`
§15.7): path owners, review requests, owner approval, draft merge requests,
suggested changes, and the tenant storage quota on snapshots the server
writes. Review decisions, comments, CI gates and linked groups are described
in [Server](./Server.md) §9; merge methods in [Merging](./Merging.md).

## 1. Path owners

### 1.1 Rules

An owner configuration is an ordered list of rules
`{"pattern", "owners"}`:

- `pattern` uses the license-rule glob syntax (anchored at the tree root,
  `/`-separated): `**` as a whole segment matches any depth, `*` and `?`
  stay within a segment, a pattern that matches a directory covers
  everything below it (`docs`, `docs/` and `docs/**` are equivalent). A
  pattern without a slash is **not** matched at every depth: `*.go` covers
  top-level files, `**/*.go` any depth.
- `owners` are `user:<username>` or `team:<slug>` (a team of the tree's
  tenant). Input also accepts `@name` / `name` (users) and `@tenant/slug` /
  `tenant/slug` (teams); the server stores and returns the canonical form,
  team slugs lower-cased, duplicates removed.
- **The last matching rule wins.** A rule with no owners un-owns the paths
  it matches (so a later `generated/**` rule can exempt generated files).
- Limits: 1000 rules, 100 owners per rule, patterns up to 512 bytes.

### 1.2 Sources

1. **Server configuration** (doc collection `path_owners`, key = tree id,
   value `domain.PathOwners {tree_id, rules, updated_by, updated_at}`),
   written with `PUT {tree}/owners` (`tree:admin`). Every user and team must
   exist (`400` otherwise). An empty list removes the configuration.
2. Otherwise the **`OWNERS` file** at the root of the tree's default branch
   (tip at evaluation time; at most 256 KiB). One rule per line:
   `pattern  owner owner ...`, whitespace separated; `#` starts a comment at
   the start of a line or of a word; blank lines are ignored. Lines that
   cannot be parsed (no owners, an invalid owner, a malformed pattern) are
   skipped and reported in `errors` as `OWNERS:<line>: <reason>`.
3. Otherwise there are no owners (`source: none`).

`GET {tree}/owners` (`tree:read`) returns
`{"tree_id","rules","updated_by","updated_at","source","errors"}` with
`source` = `config` | `file` | `none`. The configuration is deleted with the
tree.

### 1.3 Owners of a merge request

The paths a merge request changes are the paths whose entries (content
hash, executable bit or chunking) differ between the merge base of target
and source and the source tip (every path of the source when the histories
are unrelated or the target is empty; none when the target already contains
the source). `GET {tree}/merge-requests/{n}/owners` (`tree:read`) returns

```json
{"source":"file","required":true,"satisfied":false,
 "paths":[{"path":"docs/guide.md","owners":["team:docs"],"approved":false,"approved_by":[]}],
 "missing":["docs/guide.md"]}
```

- An owned path is **approved** when a non-author reviewer whose latest
  decision is an approval of the **current source tip** (approvals go stale
  when the source moves, see Server §9) is one of its owners: a `user:`
  owner by username, a `team:` owner by membership.
- Unowned paths have `owners: []`, `approved: false` and never appear in
  `missing`. `satisfied` = `missing` is empty; `required` reports the
  target's `require_owner_approval`.

### 1.4 Owner approval (`require_owner_approval`)

`BranchProtection.require_owner_approval` (ceiling model: set when any
matching rule sets it) makes every owned changed path need an owner
approval. Merge readiness (`GET .../checks`) then reports
`"owners":{"required":true,"missing":[...]}` and a blocker of kind `owners`
naming the paths and their owners (up to ten); `POST .../merge` and linked
group merges answer `409 merge_blocked`. The approval count
(`required_approvals`) is checked independently.

## 2. Review requests

`MergeRequest.reviewers` lists requested reviewers (canonical form, at most
100). Requests are added

- explicitly: `"reviewers"` on `POST {tree}/merge-requests` or
  `POST {tree}/merge-requests/{n}/reviewers {"reviewers":[...]}` (the
  author or `mr:approve`; users/teams must exist; the author cannot be
  requested; `400` otherwise);
- automatically: the owners of the changed paths (minus the author) when a
  non-draft merge request is opened, when a draft is marked ready, and when
  its source branch moves (`branch.pushed`, including suggestion
  snapshots). Reviewers already requested are not requested again.

`DELETE {tree}/merge-requests/{n}/reviewers?reviewer=...` (repeatable; or a
`{"reviewers"}` body) withdraws requests (author or `mr:approve`; unknown
entries are ignored). Requests do not imply approvals; reviewer state is
derived from `approvals`.

Every request that adds reviewers publishes **`mr.review_requested`** with
the payload

```json
{"number": 3, "reviewers": ["user:bob", "team:docs"], "requested_by": "alice"}
```

(only the newly added reviewers; `requested_by` is the caller, or the pusher
for push-triggered requests) and is audited as `mr.review_request`
(`mr.review_request_remove` for removals). The notifications service turns
the event into inbox items.

## 3. Draft merge requests

- `POST {tree}/merge-requests {"draft": true, ...}` opens a draft
  (`MergeRequest.draft`). Owners are not requested yet.
- A draft is never mergeable: readiness has a blocker of kind `draft`, and
  `POST .../merge` (and a linked group containing it) answers
  **`409 draft`**.
- `POST {tree}/merge-requests/{n}/ready` (the author or `mr:merge`) clears
  `draft`, requests the owners, publishes **`mr.ready`** (payload: the merge
  request) and is audited as `mr.ready`. A merge request that is not a
  draft is returned unchanged; one that is not open answers `409 conflict`.

## 4. Suggested changes

### 4.1 Suggestions

A comment may carry a suggestion (`POST .../comments` with
`"suggestion": {"path","start_line","end_line","replacement","snapshot_id"?}`):

- lines `start_line..end_line` (1-based, inclusive, at most 10000) of `path`
  as of `snapshot_id` (default: the comment's snapshot, i.e. the source tip)
  are to be replaced by `replacement` (at most 256 KiB);
- the file must exist in that snapshot of the tree, be a text file (not
  binary, not chunked, at most 8 MiB) and have at least `end_line` lines
  (`400` otherwise);
- the comment's `path`/`line`/`snapshot_id` become the suggestion's
  path/start line/snapshot; `body` may be empty.

Replacement semantics (`review.ApplyLines`): the replacement is text; a
missing final newline is added, except when it replaces the last line of a
file that has none; `\n` becomes `\r\n` when the replaced lines use CRLF;
an empty replacement deletes the lines.

### 4.2 Applying

`POST {tree}/merge-requests/{n}/comments/{id}/apply` (optional
`{"message"}`) on an open merge request:

1. requires `tree:write` and a direct push to the source branch allowed by
   its branch protection (`403 branch_protection` otherwise);
2. computes the new file: the suggestion applied to the file at
   `snapshot_id`; when the source tip has a different version of the file,
   that version and the suggested one are merged line-wise (diff3,
   `internal/merge3`) with the suggestion's snapshot as base. If the tip
   changed the suggested lines (or adjacent ones), or the file is gone, the
   answer is **`409 suggestion_outdated`**; if the tip already contains the
   change, `409 conflict`;
3. writes the blob and manifest and a snapshot on the source branch:
   parent = the source tip, `author` = the caller's username, `pushed_by` =
   the caller's account, message = `{"message"}` or
   `Apply suggestion to <path> (!N, comment #ID)`, no signature;
4. runs the push checks of a push to the source (path ACLs, tenant quotas,
   signature requirements, in server order) on the new snapshot: quota
   failures answer `409 quota_exceeded`, others `403` with their kind;
5. moves the source tip with compare-and-swap (`409 conflict` when it moved
   meanwhile), marks the suggestion applied (`applied_snapshot`,
   `applied_by`, `applied_at`) and the comment resolved;
6. publishes `branch.pushed` `{"branch","old_tip","new_tip","merge_request","suggestion"}`
   and **`mr.suggestion_applied`**
   `{"number","comment_id","path","snapshot_id","applied_by"}`; audited as
   `mr.suggestion_apply`.

The answer is `{"comment": ReviewComment, "snapshot": Snapshot}`. Applying
twice answers `409 conflict`; a comment without a suggestion `400`.

## 5. Storage quotas on server-written snapshots

Server-side `merge` and `squash` snapshots (also in linked groups) and
suggestion snapshots are checked against the tenant's `max_storage_bytes`
like pushes (`internal/limits`): the new objects (merged blobs, manifest)
not referenced by the target and source tips are sized and refused with
**`409 quota_exceeded`** when they would exceed the quota; nothing is moved.
Accepted deltas are recorded through the `branch.pushed` event. Fast-forward
merges add no objects and are not checked.

## 6. CLI

| Command | Effect |
|---|---|
| `wt mr create ... --draft [--reviewer WHO]...` | Open a draft / request reviewers |
| `wt mr ready N [TREE]` | Mark a draft ready |
| `wt mr reviewers N [TREE] [add\|remove WHO...]` | List, request or withdraw reviewers |
| `wt mr owners N [TREE]` | Owners and approval state per changed path |
| `wt mr suggest N [TREE] --path P --lines A[-B] (--file F\|-m TEXT) [-b BODY]` | Suggest a change |
| `wt mr apply-suggestion N COMMENT_ID [TREE] [-m MSG]` | Apply a suggestion |
| `wt owners show [TREE]` / `wt owners set FILE [TREE]` / `wt owners set --clear [TREE]` | Read / replace / remove the server owner configuration (FILE in the OWNERS format, `-` = stdin) |
| `wt protect set PATTERN --require-owner-approval` | Require owner approval |

`wt mr show/list/comments/checks` show drafts, reviewers, suggestions and
the owner blocker; merge refusals hint at `wt mr ready`, `wt mr owners` and
the storage quota.

## 7. Admin panel

Merge request detail: inline comment threads on diff lines (click a line
number in the Changes view), a "Suggest change" mode in the line composer,
suggestion blocks with "Apply suggestion", the draft badge and "Ready for
review", a reviewers panel (request/remove, per-reviewer state) and a code
owners panel. Tree "Owners" tab: the rules editor (server configuration),
the OWNERS file source and parse errors. Protections: "Require owner
approval". Linked groups: a merge-method picker.

## Implementation Status

| Item | Status |
|---|---|
| Server owner configuration, OWNERS file fallback, `GET\|PUT {tree}/owners` | Built |
| `GET .../merge-requests/{n}/owners` | Built |
| Automatic owner review requests (open, ready, source pushes) | Built |
| `require_owner_approval` in readiness and merges | Built |
| Review requests endpoints, `mr.review_requested` | Built |
| Draft merge requests, `409 draft`, `.../ready`, `mr.ready` | Built |
| Suggestions, apply with line-wise rebase, `409 suggestion_outdated`, `mr.suggestion_applied` | Built |
| Storage quota on merge/squash and suggestion snapshots | Built |
| CLI and admin panel | Built |

## Roadmap

- Converting an open merge request back to a draft.
- Team owners from other tenants and nested team membership.
- Multi-file and multi-hunk suggestions; batch-applying several suggestions
  as one snapshot.
- Owner rules per branch (today the default branch's OWNERS file applies to
  every target).
- Dismissing stale reviews and re-requesting a reviewer after changes.

## References

- `services/server-go/internal/review/owners.go` (rules, sources,
  ownership, `require_owner_approval`), `requests.go` (drafts, review
  requests, push-triggered requests), `suggest.go` (suggestions, quota
  checks), `checks.go` (blockers `draft`, `owners`)
- `services/server-go/internal/httpapi/owners.go`, `mergerequests.go`,
  `reviews.go`, `openapi.yaml`
- `services/server-go/internal/domain/types.go` (v0.7 review block),
  `v05.go` (`ReviewComment.Suggestion`), `v07.go` (`PathOwners`, events)
- `services/server-go/internal/limits/limits.go` (storage accounting)
- `crates/worktree-cli/src/commands/{mr,owners,protect}.rs`
- `apps/admin/src/pages/tree/{MergeRequestDetail,OwnersTab,ProtectionsTab}.tsx`,
  `pages/tree/v07/**`, `api/v07review.ts`
- `docs/ENGINEERING_PLAN.md` §13.4, §14.1, §15.7
- Related specs: [Server](./Server.md), [Merging](./Merging.md),
  [License compliance](../licensing/LicenseCompliance.md) (glob syntax)
