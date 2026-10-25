# W0rkTree Code Search Specification

Server-side search of file contents across trees (`docs/ENGINEERING_PLAN.md`
§14.3): a trigram index over the default branch of every tree, literal and
regular-expression queries, permission-aware results, the REST endpoint and
`wt search code`.

## Table of Contents

1. [Overview](#1-overview)
2. [What is indexed](#2-what-is-indexed)
3. [Index lifecycle](#3-index-lifecycle)
4. [Queries](#4-queries)
5. [Trigram prefilter](#5-trigram-prefilter)
6. [Access control](#6-access-control)
7. [REST API](#7-rest-api)
8. [CLI](#8-cli)
9. [Limits](#9-limits)
10. [Implementation status](#10-implementation-status)
11. [Roadmap](#11-roadmap)
12. [References](#12-references)

## 1. Overview

Code search answers "which lines of which files contain X" over the trees a
caller may read. It searches the **tip of each tree's default branch** only
(not other branches, staged snapshots or history). Matching is line based,
like `grep`: every hit is one line, with optional context lines.

The index is an in-memory component of the server (`codesearch.Index`). It
stores no file content of its own beyond what it keeps in memory; a small
metadata document per indexed tree is kept in the `code_index_meta` document
collection (key = tree id).

## 2. What is indexed

For the default-branch tip of a tree, every manifest entry is indexed except:

| Skipped | Rule |
|---|---|
| Chunked files | `ManifestEntry.chunked = true` (large files stored as chunk lists) |
| Large files | manifest size, or stored blob size, over **1 MiB** (`MaxFileSize`) |
| Binary files | a NUL byte in the first 8000 bytes |

Large files are recognised from the manifest and never read. Binary files
are read once; their hash is remembered so that incremental rebuilds do not
read them again.

Each indexed file keeps its content and the set of its byte trigrams (three
consecutive bytes, with ASCII letters folded to lower case); the tree index
holds one posting list (sorted file ids) per trigram.

## 3. Index lifecycle

- **Lazy build.** A tree is indexed by the first search whose scope includes
  it. Trees nobody searches are never indexed.
- **Freshness on every query.** Each search reads the default branch's
  current tip; when it differs from the indexed snapshot (or the tree's
  default branch was changed) the index is rebuilt before the tree is
  searched.
- **Push events.** `branch.pushed` for the default branch of an already
  indexed tree rebuilds its index right away (pushes to other branches and
  to never-searched trees are ignored).
- **Incremental rebuilds.** A rebuild reuses every file whose path and
  content hash are unchanged (content and trigram set), so only new or
  changed blobs are read from the object store. Posting lists are rebuilt.
- **Deletion.** `tree.deleted` drops the tree's index and its
  `code_index_meta` document.
- **Detached builds.** A build is not cancelled by the search that started
  it. When the search's time budget runs out first, the search returns
  (truncated) and the build completes for later searches. Concurrent
  searches of a tree share one build. A build is bounded by 2 minutes.
- A tree whose index cannot be built (for example a missing manifest object)
  is skipped by the search and logged; other trees are still searched.

The metadata document (`codesearch.Meta`):

```json
{"tree_id":"...","branch":"main","snapshot_id":"...","files":120,"bytes":481233,
 "trigrams":20311,"skipped_binary":3,"skipped_large":1,"skipped_chunked":2,
 "reused":118,"built_at":"2026-10-25T10:00:00Z","build_ms":14}
```

## 4. Queries

| Field | Meaning |
|---|---|
| `q` | Literal text (at least **3 characters**, no line breaks) or, with `regex`, an RE2 expression (non-empty). At most 1000 bytes. |
| `regex` | Treat `q` as a regular expression (Go `regexp`, RE2 syntax). |
| `case` | `true` = case-sensitive. Default `false`: Unicode simple case folding (as RE2 `(?i)`). |
| `path` | Path glob limiting the files searched (below). |
| `context` | Lines of context before and after each hit, 0-10 (default 2). |
| `limit` | Maximum hits, 1-1000 (default 50; larger values are clamped). |

Matching is per line: content is split at `\n`, a trailing `\r` is removed
from each line, and the expression is matched against each line on its own
(so `^` and `$` anchor at line boundaries and no match spans lines). Literal
queries match as substrings.

**Path globs** use the license-rule glob syntax (`**` any depth, `*` and `?`
within a segment, `[...]` classes; anchored at the tree root; a pattern that
names a directory covers everything beneath it), with one addition: a
pattern without `/` matches the file name at any depth (`*.go` ≡ `**/*.go`).
Malformed patterns are rejected with `400`.

**Ordering.** Hits are ordered by tenant slug, tree name, path and line.

**Truncation.** Searching stops once `limit` hits are collected and another
hit exists, or when the 5-second time budget is exhausted; `truncated` is
then `true` and the hits found so far are returned.

## 5. Trigram prefilter

A query is compiled into a boolean formula over trigrams that every matching
file must satisfy (a necessary condition). Only files satisfying it are
scanned line by line.

- The formula is derived from the parsed (and simplified) RE2 syntax tree;
  literal searches are first turned into the equivalent quoted expression.
- Sub-expressions carry, where small enough (at most 64 strings), the exact
  set of strings they can match; concatenation takes cross products and
  alternation unions, so `foo(bar|baz)` requires the trigrams of `foobar` or
  of `foobaz`. Larger sets fall back to the AND of the parts' conditions.
- Character classes of up to 16 runes expand to exact strings; `.`, larger
  classes, `*`, `?` and `{0,n}` impose no condition; `x+` and `x{n,}` require
  `x`'s condition.
- Case-folded literals expand each rune to its full simple-fold orbit
  (`k` → `k`, `K`, U+212A KELVIN SIGN), so case-insensitive searches never
  miss files because of non-ASCII folds.
- Strings shorter than three bytes impose no condition; an expression with no
  usable trigrams scans every (path-matching) file.

## 6. Access control

The REST layer selects the trees to search and filters per path; the index
itself does not authorize.

- **Trees.** Only trees on which the caller holds `tree:read` are searched.
  Anonymous callers search **public** trees only.
- **Paths.** For the files of a tree that produced hits, the same read
  restrictions as the REST content endpoints (§13.1) apply: paths denied by a
  `tree:read` path-deny policy (`AccessEvaluator.Evaluate` with
  `Resource.Path`) and paths the tree's license rules keep from the caller
  (`LicenseChecker` read checks) are dropped silently. Restricted hits do not
  count towards `limit`. The check runs in batches of 64 files.
- An explicit `tree` the caller may not read answers `401`/`403` like the
  other tree endpoints; trees outside the caller's access in a tenant- or
  server-wide search are simply skipped.

## 7. REST API

```
GET /api/v1/search/code?q=&tenant=&tree=&path=&regex=false&case=false&context=2&limit=50
```

Scope: `tree=TENANT/TREE` (or `tenant=T&tree=TREE`) searches one tree;
`tenant=T` the trees of one tenant; neither searches every readable tree on
the server.

Response `200`:

```json
{
  "hits": [
    {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "…",
     "path": "src/main.rs", "line": 12, "text": "fn main() {",
     "before": ["", "// entry point"], "after": ["    run();"]}
  ],
  "truncated": false,
  "searched_trees": 3
}
```

`before`/`after` are omitted when empty. `searched_trees` counts the trees
actually searched (readable and indexable).

Errors: `400 bad_request` (missing or short `q`, invalid regex, bad `regex`
/ `case` / `context` / `limit` / `path`, `tree` without tenant, `tree` and
`tenant` disagreeing), `404 not_found` (unknown tenant or tree), `401`/`403`
(explicit tree not readable), `501 not_implemented` (code search disabled).

## 8. CLI

```
wt search code QUERY [--tree TENANT/TREE] [--tenant T] [--path GLOB]
                     [-E|--regex] [-i|--ignore-case] [--context N] [-n|--limit N]
```

- `--tree` is the global `--tree` flag; for this command it takes
  `TENANT/TREE`.
- grep semantics: case-sensitive unless `-i`; no context unless `--context`
  (sent as `case=true|false` and `context=N`).
- Output: `TENANT/TREE:PATH:LINE:TEXT` per hit; context lines are
  `TENANT/TREE:PATH-LINE-TEXT`; non-adjacent groups are separated by `--`;
  overlapping context is printed once and match lines are never printed as
  context. Literal matches are highlighted when colors are on. A truncated
  result ends with a dimmed note. `--json` prints the REST response.
- Context uses `--context N` rather than `-C N`, because `-C PATH` is the
  global "run in directory" flag.
- `wt search QUERY [-n N]` (names of tenants, trees, branches and accounts)
  is unchanged; the `code` subcommand cannot be combined with its arguments.

## 9. Limits

| Limit | Value |
|---|---|
| Indexed file size | ≤ 1 MiB |
| Literal query | ≥ 3 characters |
| Query length | ≤ 1000 bytes |
| Hits per query | ≤ 1000 (default 50) |
| Context lines | 0-10 |
| Time budget per query | 5 s |
| Line text in a hit | ≤ 1000 bytes (cut at a character boundary) |
| Index build | ≤ 2 min |

## 10. Implementation status

| Item | Status |
|---|---|
| Trigram index of default-branch tips (text ≤ 1 MiB; binary and chunked skipped) | Built (v0.6 §14.3) |
| Lazy build, stale-tip rebuild, incremental rebuild on `branch.pushed`, drop on `tree.deleted` | Built (v0.6 §14.3) |
| `code_index_meta` metadata documents | Built (v0.6 §14.3) |
| Literal and RE2 search, Unicode case folding, path globs, context, limit, 5 s budget | Built (v0.6 §14.3) |
| Regex trigram prefilter | Built (v0.6 §14.3) |
| `tree:read`, path-deny and license filtering; anonymous public-only search | Built (v0.6 §14.3) |
| `GET /api/v1/search/code` | Built (v0.6 §14.3) |
| `wt search code` | Built (v0.6 §14.3) |

## 11. Roadmap

- Persisting the index across restarts (it is rebuilt lazily after a
  restart; only the metadata survives).
- A memory budget with eviction of rarely searched trees.
- Searching branches other than the default branch, tags or history.
- Match offsets in hits (and server-side highlighting); the CLI highlights
  literal matches itself.
- Ranking (by file importance or match quality) instead of path order.
- Searching chunked and larger files.
- Admin visibility of index status (the metadata is stored but not exposed
  over REST).

## 12. References

- `services/server-go/internal/codesearch/` — `codesearch.go` (index
  lifecycle, events), `index.go` (building, posting lists), `trigram.go`
  (query analysis), `search.go` (queries, matching, limits)
- `services/server-go/internal/httpapi/codesearch.go` — REST endpoint, scope
  and read filtering
- `services/server-go/internal/httpapi/snapshots.go` — `readRestrictions`
  (shared §13.1 path checks)
- `services/server-go/internal/domain/v06.go` — `CodeSearchHit`
- `crates/worktree-cli/src/commands/search.rs` — `wt search` and
  `wt search code`
- `docs/ENGINEERING_PLAN.md` §13.1, §14.3
