# W0rkTree Code Search Specification

Server-side search of file contents and symbol definitions across trees
(`docs/ENGINEERING_PLAN.md` §14.3, §15.9): a persistent trigram and symbol
index over the branch tips of every tree, literal, regular-expression and
symbol queries, permission-aware results, the REST endpoints, `wt search
code` and `wt admin code-index`.

## Table of Contents

1. [Overview](#1-overview)
2. [What is indexed](#2-what-is-indexed)
3. [Symbols](#3-symbols)
4. [Index storage](#4-index-storage)
5. [Index lifecycle](#5-index-lifecycle)
6. [Branches](#6-branches)
7. [Code queries](#7-code-queries)
8. [Symbol queries](#8-symbol-queries)
9. [Trigram prefilter](#9-trigram-prefilter)
10. [Access control](#10-access-control)
11. [REST API](#11-rest-api)
12. [CLI](#12-cli)
13. [Admin UI](#13-admin-ui)
14. [Configuration](#14-configuration)
15. [Limits](#15-limits)
16. [Performance](#16-performance)
17. [Implementation status](#17-implementation-status)
18. [Roadmap](#18-roadmap)
19. [References](#19-references)

## 1. Overview

Code search answers "which lines of which files contain X" and "where is
the function/type/constant X defined" over the trees a caller may read. By
default it searches the **tip of each tree's default branch**; a query may
name another branch or ask for all branch tips (§6). Staged snapshots and
history are not searched. Code matching is line based, like `grep`: every
hit is one line, with optional context lines.

The index (`codesearch.Index`) is a server component. Since v0.7 it is
**persistent**: the index of every searched tree lives on disk under
`<data>/codeindex/` and survives restarts; decoded indexes are kept in
memory by an LRU capped at `--code-index-memory` and loaded lazily. A small
metadata document per indexed tree (the default branch's `Meta`) is also
kept in the `code_index_meta` document collection (key = tree id). The
`code_index_v2` collection reserved in §15 is not used: the on-disk state
files are authoritative.

## 2. What is indexed

For an indexed branch tip, every manifest entry is indexed except:

| Skipped | Rule |
|---|---|
| Chunked files | `ManifestEntry.chunked = true` (large files stored as chunk lists) |
| Large files | manifest size, or stored blob size, over **1 MiB** (`MaxFileSize`) |
| Binary files | a NUL byte in the first 8000 bytes |

Large files are recognised from the manifest and never read. Binary files
are read once; their hash is remembered so that rebuilds do not read them
again.

For each indexed file the index keeps its content, one posting list (sorted
file ids) per byte trigram (three consecutive bytes, with ASCII letters
folded to lower case) and the symbols defined in it (§3).

## 3. Symbols

A line-based regular-expression extractor (`symbols.go`) finds definitions
by file extension:

| Language | Extensions | Kinds |
|---|---|---|
| Go | `.go` | `function`, `method` (container = receiver type), `struct`, `interface`, `type`, `const` (also in `type (…)` / `const (…)` groups) |
| Rust | `.rs` | `function`, `method` (in `impl`/`trait` blocks, container = type/trait), `struct` (and `union`), `enum`, `trait`, `type`, `const` (`const`/`static`) |
| TypeScript / JavaScript | `.ts .tsx .mts .cts .js .jsx .mjs .cjs` | `function` (declarations and `const f = (…) =>`), `class`, `method` (class bodies, incl. property arrow functions), `interface`, `type`, `enum`, `const` (top-level `const`) |
| Python | `.py .pyi` | `function`, `class`, `method` (container = class), `const` (top-level `UPPER_CASE = …`) |
| Java | `.java` | `class` (and `record`), `interface` (and `@interface`), `enum`, `method` (incl. constructors), `const` (`static final`) |
| C / C++ | `.c .h` / `.cc .cpp .cxx .c++ .hpp .hh .hxx .h++ .ipp` | `function` (definitions; function-like macros), `method` (`A::f` definitions and declarations in class bodies), `struct` (and `union`), `class`, `enum`, `type` (`typedef`, `using X =`, `} name;`), `const` (object-like `#define`, `const`/`constexpr`) |

Brace languages track the nesting depth (string literals, character
literals, `//` and `/* */` comments and JS template literals are skipped)
so that only declarations at the top level, directly inside a type body
(methods) or inside a namespace/module/`extern "C"` block are reported —
never local definitions inside function bodies. Python uses indentation and
skips triple-quoted strings. Prototypes outside type bodies (`int f(int);`)
and forward declarations (`struct x;`) are not definitions.

Each symbol has a name, kind, 1-based line, container (enclosing type of a
method, or of a nested type), the defining line as signature (trimmed,
trailing `{` removed, at most 200 bytes) and the language. Lines over 2000
bytes are not scanned and a file contributes at most 5000 symbols. The
extractor is heuristic: unusual formatting can hide or invent a symbol.

## 4. Index storage

```
<data>/codeindex/<tree id>/state.json        indexed branches of the tree
<data>/codeindex/<tree id>/<manifest>.seg    one segment per indexed manifest
```

(Tree ids with characters outside `[A-Za-z0-9._-]` are hex-encoded as
`x-<hex>`.)

**Segments.** The index of one manifest is an immutable *segment* named by
the manifest hash; branches whose tips have the same manifest share it.
The same encoding is used on disk and in memory (a loaded segment keeps the
buffer and references file contents and posting lists inside it without
copying). Little-endian; `uv` = unsigned LEB128 varint; `str` = `uv(len)` +
bytes:

```
magic "WTCS" | u32 format version (1)
uv(len) stats JSON {files, bytes, trigrams, symbols, skipped_*, reused, built_at, build_ms}
uv(nDocs)   { str path | [32]blob hash | uv(content length) }      (path order)
uv(nBinary) { str path | [32]blob hash }                           (skipped binary files)
uv(nSyms)   { str name | u8 kind | uv(doc) | uv(line) | str container | str signature }
uv(nTri)    { [3]trigram | uv(posting length) }                    (trigrams ascending)
postings    concatenated; each list = uv(first doc id) uv(delta)…
contents    concatenated in doc order
u32 CRC-32C (Castagnoli) of every preceding byte
```

Symbols are stored sorted by lower-cased name (then path and line), which
makes exact and prefix lookups a binary search. Kind codes are the index in
`function, method, class, struct, interface, trait, enum, type, const`.

**State file.** `state.json` = `{"format":1,"tree_id","default_branch",
"branches":[{"branch","snapshot_id","meta":Meta,"last_used"}]}`;
`meta.manifest` names the branch's segment (empty for an empty branch).

**Atomic swap.** A new segment is written to `<name>.tmp-<random>`,
fsynced and renamed into place; `state.json` is replaced the same way only
after the segments it names exist; segments no branch refers to (and stale
temporary files) are deleted afterwards. A crash at any point leaves a
consistent index — at worst an unreferenced segment that the next write of
that tree removes.

**Format mismatch and corruption.** A segment or state file with another
format version, a failing checksum, a structural error (bounds, unsorted
trigrams, posting ids out of range) or an unparsable state file is
discarded and the affected branch (or tree) is rebuilt from the object
store. A missing segment file is rebuilt the same way.

**Memory.** Decoded segments are cached in a global LRU capped at
`--code-index-memory` bytes (default 256 MiB; 0 = unlimited). The estimate
is the encoded size plus per-trigram, per-file and per-symbol overhead
(about 3.5× the indexed content for typical source code; see §16). Evicted segments stay on disk and
are loaded again when a search needs them (without a data directory they
are rebuilt). The most recently loaded segment is never evicted, so a
single segment larger than the cap still works.

## 5. Index lifecycle

- **Lazy build and load.** A branch is indexed by the first search that
  covers it. After a restart the state file of a tree is read on first use
  and its segments are loaded from disk on demand — nothing is rebuilt
  unless the branch tip changed.
- **Freshness on every query.** Each search reads the current tip of every
  branch it covers; when it differs from the indexed snapshot the branch is
  rebuilt before it is searched.
- **Push events.** `branch.pushed` for an indexed branch of a tree rebuilds
  it right away (pushes to other branches and to never-searched trees are
  ignored). `branch.deleted` drops the branch; `tree.deleted` drops the
  tree's index, its files and its `code_index_meta` document. Indexes of
  trees deleted while the server was down are dropped when the status
  endpoint finds them.
- **Deduplication.** A build first looks for an existing segment of the
  tip's manifest (another branch, or the disk) and reuses it.
- **Incremental rebuilds.** A new segment reuses the content of every blob
  whose hash appears in the branch's previous segment or the default
  branch's segment, so only new or changed blobs are read from the object
  store; trigrams and symbols are recomputed.
- **Detached builds.** A build is not cancelled by the search that started
  it. When the search's time budget runs out first, the search returns
  (truncated) and the build completes for later searches. Concurrent
  searches of a branch share one build. A build is bounded by 2 minutes.
- **Rebuild.** `POST /api/v1/admin/code-index/rebuild` discards a tree's
  index (memory and disk) and rebuilds the branches that were indexed (the
  default branch when none was) from the object store.
- A tree whose index cannot be built (for example a missing manifest
  object) is skipped by the search and logged; other trees are still
  searched.

The metadata document (`codesearch.Meta`, also the per-branch `meta` of the
state file):

```json
{"tree_id":"...","branch":"main","snapshot_id":"...","manifest":"<hex>","files":120,
 "bytes":481233,"trigrams":20311,"symbols":830,"skipped_binary":3,"skipped_large":1,
 "skipped_chunked":2,"reused":118,"built_at":"2026-10-25T10:00:00Z","build_ms":14}
```

## 6. Branches

| Query | Branch tips searched per tree |
|---|---|
| (default) or `branches=default` | the default branch |
| `branch=B` | branch `B`; trees without `B` contribute nothing (they still count in `searched_trees`) |
| `branches=all` | the default branch and the most recently updated other branches, up to `--code-index-branches` (default 10) per tree |

`branch` and `branches=all` are mutually exclusive (`400`). Hits are ordered
by tenant, tree, then branch (default branch first, then by name), path and
line. A file version (same path and blob hash) already searched on an
earlier branch is skipped, so a file unchanged on a feature branch is
reported once, on the default branch; branches with identical tips share
one segment. A tree keeps at most `--code-index-branches` indexed branches:
indexing one more drops the least recently searched non-default branch.

## 7. Code queries

| Field | Meaning |
|---|---|
| `q` | Literal text (at least **3 characters**, no line breaks) or, with `regex`, an RE2 expression (non-empty). At most 1000 bytes. |
| `regex` | Treat `q` as a regular expression (Go `regexp`, RE2 syntax). |
| `case` | `true` = case-sensitive. Default `false`: Unicode simple case folding (as RE2 `(?i)`). |
| `path` | Path glob limiting the files searched (below). |
| `context` | Lines of context before and after each hit, 0-10 (default 2). |
| `limit` | Maximum hits, 1-1000 (default 50; larger values are clamped). |
| `branch`, `branches` | Branch selection (§6). |

Matching is per line: content is split at `\n`, a trailing `\r` is removed
from each line, and the expression is matched against each line on its own
(so `^` and `$` anchor at line boundaries and no match spans lines). Literal
queries match as substrings.

**Path globs** use the license-rule glob syntax (`**` any depth, `*` and `?`
within a segment, `[...]` classes; anchored at the tree root; a pattern that
names a directory covers everything beneath it), with one addition: a
pattern without `/` matches the file name at any depth (`*.go` ≡ `**/*.go`).
Malformed patterns are rejected with `400`.

**Truncation.** Searching stops once `limit` hits are collected and another
hit exists, or when the 5-second time budget is exhausted; `truncated` is
then `true` and the hits found so far are returned.

## 8. Symbol queries

| Field | Meaning |
|---|---|
| `q` | Name or part of a name, 1-200 bytes; matched case-insensitively. |
| `kind` | Comma-separated kinds (`function, method, class, struct, interface, trait, enum, type, const`); default all. Unknown kinds are `400`. |
| `path`, `tenant`, `tree`, `limit`, `branch`, `branches` | As for code queries. |

**Ranking.** Exact name matches rank above prefix matches above substring
matches; among exact matches those in the query's case come first. Within a
rank shorter names come first, then results are ordered by name, tenant,
tree, branch (default first), path and line. `match` in each result names
the rank (`exact`, `prefix`, `substring`). Exact and prefix candidates are
found by binary search over the sorted names, substrings by a scan.
Truncation works as for code queries (`limit` reached, or the 5 s budget).

## 9. Trigram prefilter

A code query is compiled into a boolean formula over trigrams that every
matching file must satisfy (a necessary condition). Only files satisfying it
are scanned line by line.

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

## 10. Access control

The REST layer selects the trees to search and filters per path; the index
itself does not authorize.

- **Trees.** Only trees on which the caller holds `tree:read` are searched.
  Anonymous callers search **public** trees only.
- **Paths.** For the files of a tree that produced hits (or symbols), the
  same read restrictions as the REST content endpoints (§13.1) apply: paths
  denied by a `tree:read` path-deny policy (`AccessEvaluator.Evaluate` with
  `Resource.Path`) and paths the tree's license rules keep from the caller
  (`LicenseChecker` read checks) are dropped silently. Restricted results
  do not count towards `limit`. The check runs in batches of 64 files.
- An explicit `tree` the caller may not read answers `401`/`403` like the
  other tree endpoints; trees outside the caller's access in a tenant- or
  server-wide search are simply skipped.
- The index status and rebuild endpoints require `server:admin`.

## 11. REST API

```
GET  /api/v1/search/code?q=&tenant=&tree=&path=&regex=false&case=false&context=2&limit=50&branch=|branches=all
GET  /api/v1/search/symbols?q=&kind=&tenant=&tree=&path=&limit=50&branch=|branches=all
GET  /api/v1/admin/code-index
POST /api/v1/admin/code-index/rebuild?tree=TENANT/TREE
```

Scope: `tree=TENANT/TREE` (or `tenant=T&tree=TREE`) searches one tree;
`tenant=T` the trees of one tenant; neither searches every readable tree on
the server.

Code search response `200`:

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

Symbol search response `200` (`SymbolSearchResult`):

```json
{
  "symbols": [
    {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "…",
     "path": "src/config.rs", "line": 5, "name": "parse_config", "kind": "function",
     "signature": "pub fn parse_config(s: &str) -> Config", "language": "rust",
     "match": "exact"},
    {"tenant": "acme", "tree": "web", "branch": "main", "snapshot_id": "…",
     "path": "src/config.rs", "line": 10, "name": "parse_strict", "kind": "method",
     "container": "Config", "signature": "pub fn parse_strict(&self) -> bool",
     "language": "rust", "match": "prefix"}
  ],
  "truncated": false,
  "searched_trees": 1
}
```

Index status `200` (`CodeIndexStatus`):

```json
{"format_version": 1, "persistent": true, "memory_bytes": 16872345,
 "memory_limit": 268435456, "loaded_segments": 2, "evictions": 0,
 "disk_bytes": 9103412, "max_branches": 10, "builds": 0, "disk_loads": 2,
 "trees": [{"tree_id": "…", "tenant": "acme", "tree": "web", "default_branch": "main",
   "state": "ready", "files": 3000, "bytes": 4700000, "symbols": 39750, "trigrams": 61234,
   "built_at": "2026-10-25T10:00:00Z", "branch_count": 2, "segments": 1,
   "memory_bytes": 16872345, "disk_bytes": 9103412,
   "branches": [{"branch": "main", "snapshot_id": "…", "manifest": "<hex>", "files": 3000,
     "bytes": 4700000, "symbols": 39750, "reused": 0, "built_at": "…", "build_ms": 1650,
     "loaded": true, "building": false}]}]}
```

`builds` and `disk_loads` count segments built and loaded from disk since
the server started — after a restart `builds` stays 0 until a branch tip
changes or a rebuild is requested. `state` is `ready`, `building` or
`error` (with `error`). The rebuild answers `{"rebuilt": n, "trees":
[CodeIndexTree]}` after the rebuild finished (without `tree`: every indexed
tree) and is audited as `code_index.rebuild`.

Errors: `400 bad_request` (missing or short `q`, invalid regex, bad `regex`
/ `case` / `context` / `limit` / `path` / `kind` / `branches`, `branch`
with `branches=all`, `tree` without tenant, `tree` and `tenant` disagreeing,
rebuild `tree` not `TENANT/TREE`), `404 not_found` (unknown tenant or
tree), `401`/`403` (explicit tree not readable; admin endpoints without
`server:admin`), `501 not_implemented` (code search disabled).

## 12. CLI

```
wt search code QUERY [--tree TENANT/TREE] [--tenant T] [--path GLOB]
                     [-E|--regex] [-i|--ignore-case] [--context N] [-n|--limit N]
                     [--symbol [--kind KIND[,KIND…]]...]
                     [--branch B | --all-branches]
wt admin code-index [rebuild [TENANT/TREE]]
```

- `--tree` is the global `--tree` flag; for this command it takes
  `TENANT/TREE`.
- grep semantics: case-sensitive unless `-i`; no context unless `--context`
  (sent as `case=true|false` and `context=N`).
- Output: `TENANT/TREE:PATH:LINE:TEXT` per hit; context lines are
  `TENANT/TREE:PATH-LINE-TEXT`; non-adjacent groups are separated by `--`;
  overlapping context is printed once and match lines are never printed as
  context. With `--branch`/`--all-branches` the prefix is
  `TENANT/TREE@BRANCH`. Literal matches are highlighted when colors are on.
  A truncated result ends with a dimmed note. `--json` prints the REST
  response.
- `--symbol` searches definitions (`GET /api/v1/search/symbols`; one
  character is enough): `TENANT/TREE:PATH:LINE: KIND [CONTAINER.]NAME
  SIGNATURE` per symbol, best matches first. `--kind` (repeatable or
  comma-separated) requires `--symbol`; `--symbol` conflicts with `--regex`
  and `--context`.
- Context uses `--context N` rather than `-C N`, because `-C PATH` is the
  global "run in directory" flag.
- `wt admin code-index` prints the index status (format, memory against the
  cap, disk use, builds and disk loads since start, per tree: state,
  branches/segments, files, symbols, memory, disk, built); `rebuild
  TENANT/TREE` (or no tree: every indexed tree) rebuilds and prints the
  rebuilt trees. Server admins only.
- `wt search QUERY [-n N]` (names of tenants, trees, branches and accounts)
  is unchanged; the `code` subcommand cannot be combined with its arguments.

## 13. Admin UI

`/search/code` (`apps/admin/src/pages/search/`):

- A **Code / Symbols** toggle (`kind=symbol` in the page URL). Symbol mode
  hides the regex and case toggles, shows a **Kind** filter
  (`symbol_kind=` in the URL) and lists definitions grouped by tree: kind
  badge, `Container.name` with the match highlighted, signature, branch
  (with a branch scope) and a `path:line` link to the file view.
- A **Branch** selector: *Default branch*, *All branches*
  (`branches=all`) or, when the Tree field names a tree, one of its
  branches (`branch=`). Code results then show the branch per file.
- For server admins, a collapsed **Index status** panel (loaded when
  opened): memory and disk use, builds and disk loads since start, the
  indexed trees with state, branches, files, symbols, memory, disk and
  build age, a per-tree **Rebuild** button and **Rebuild all**.

## 14. Configuration

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--code-index-memory SIZE` | `WT_CODE_INDEX_MEMORY` | `256MiB` | Memory cap of loaded segments (`256MiB`, `1G`, `512MB`, bytes; `0` = unlimited) |
| `--code-index-branches N` | `WT_CODE_INDEX_BRANCHES` | `10` | Most branches indexed per tree (1-1000) |

The index directory is always `<data>/codeindex`. When it cannot be created
the index runs in memory only (logged).

## 15. Limits

| Limit | Value |
|---|---|
| Indexed file size | ≤ 1 MiB |
| Literal query | ≥ 3 characters |
| Code query length | ≤ 1000 bytes |
| Symbol query length | 1-200 bytes |
| Hits / symbols per query | ≤ 1000 (default 50) |
| Context lines | 0-10 |
| Time budget per query | 5 s |
| Line text in a hit | ≤ 1000 bytes (cut at a character boundary) |
| Symbol signature | ≤ 200 bytes; lines over 2000 bytes are not scanned; ≤ 5000 symbols per file |
| Branches per tree | `--code-index-branches` (default 10) |
| Index build | ≤ 2 min |

## 16. Performance

Benchmarks in `services/server-go/internal/codesearch/v2bench_test.go`
(`go test -run '^$' -bench V2 -benchtime 10x ./internal/codesearch`) over a
synthetic tree of **3000 source files** (Go, Rust, TypeScript, Python;
4.7 MB, 39,750 symbols). Segment: 8.7 MiB on disk, ~16.1 MiB in memory.
Measured on an AMD Ryzen Threadripper PRO 9995WX, Windows 11, Go 1.27,
on-disk object store:

| Operation | Time |
|---|---|
| Cold build (3000 blob reads from the object store, trigrams, symbols, encode, write) | 1.66 s (2.8 MB/s; dominated by per-file object reads on Windows) |
| Rebuild with every blob reused (CPU: trigrams, symbols, encoding) | 153 ms (30.8 MB/s) |
| Load the tree's index from disk after a restart (read + verify + decode) | 10.5 ms |
| Literal search, rare string (1 hit) | 20 µs |
| Literal search, common word (`handler`, limit 50) | 0.36 ms |
| Regex `func (Parse\|Render)\w+\(` (874 hits) | 4.4 ms |
| Symbol search `parse` (ranked, limit 50, over 39,750 symbols) | 1.9 ms |

## 17. Implementation status

| Item | Status |
|---|---|
| Trigram index of branch tips (text ≤ 1 MiB; binary and chunked skipped) | Built (v0.6 §14.3) |
| Lazy build, stale-tip rebuild, incremental rebuild on `branch.pushed`, drop on `tree.deleted` | Built (v0.6 §14.3) |
| `code_index_meta` metadata documents | Built (v0.6 §14.3) |
| Literal and RE2 search, Unicode case folding, path globs, context, limit, 5 s budget | Built (v0.6 §14.3) |
| Regex trigram prefilter | Built (v0.6 §14.3) |
| `tree:read`, path-deny and license filtering; anonymous public-only search | Built (v0.6 §14.3) |
| `GET /api/v1/search/code`, `wt search code` | Built (v0.6 §14.3) |
| Persistent, versioned on-disk segments with CRC-32C, atomic swap, corruption/format-mismatch rebuild | Built (v0.7 §15.9) |
| Lazy loading with a memory-capped LRU (`--code-index-memory`) | Built (v0.7 §15.9) |
| Symbol extraction (Go, Rust, TS/JS, Python, Java, C/C++), `GET /api/v1/search/symbols`, exact > prefix > substring ranking | Built (v0.7 §15.9) |
| `branch=` / `branches=all` with manifest and file-version dedup, `--code-index-branches`, `branch.deleted` | Built (v0.7 §15.9) |
| `GET /api/v1/admin/code-index`, `POST /api/v1/admin/code-index/rebuild` | Built (v0.7 §15.9) |
| `wt search code --symbol/--kind/--branch/--all-branches`, `wt admin code-index [rebuild]` | Built (v0.7 §15.9) |
| Admin UI: symbol mode, kind filter, branch selector, index status panel | Built (v0.7 §15.9) |

## 18. Roadmap

- Parser-based symbol extraction (tree-sitter) for precision, references
  ("find usages") and more languages.
- Memory-mapped segments instead of reading them into memory.
- Searching tags, staged snapshots or history.
- Match offsets in hits (and server-side highlighting); the CLI and UI
  highlight literal matches themselves.
- Ranking code hits (by file importance or match quality) instead of path
  order.
- Searching chunked and larger files.
- Sharing segments of identical manifests across trees (forks).

## 19. References

- `services/server-go/internal/codesearch/` — `codesearch.go` (index
  lifecycle, events, branch states, builds), `segment.go` (binary format),
  `store.go` (on-disk layout, atomic writes, GC), `cache.go` (LRU),
  `index.go` (building, posting lists), `symbols.go` (extractor),
  `symsearch.go` (symbol queries), `status.go` (status, rebuild),
  `trigram.go` (query analysis), `search.go` (code queries, branches,
  matching, limits), `v2bench_test.go` (benchmarks)
- `services/server-go/internal/httpapi/codesearch.go` — REST endpoints,
  scope and read filtering
- `services/server-go/internal/httpapi/snapshots.go` — `readRestrictions`
  (shared §13.1 path checks)
- `services/server-go/internal/config/codeindex.go` — flags
- `services/server-go/internal/domain/v06.go` — `CodeSearchHit`
- `crates/worktree-cli/src/commands/search.rs` — `wt search`,
  `wt search code` and `wt admin code-index`
- `apps/admin/src/pages/search/` — code search page
- `crates/worktree-e2e/tests/e2e/code_search.rs`, `code_search_v2.rs`
- `docs/ENGINEERING_PLAN.md` §13.1, §14.3, §15.9
