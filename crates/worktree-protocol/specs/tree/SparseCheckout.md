# Sparse Checkout and Partial Clones

> Status: built in v0.7 (`docs/ENGINEERING_PLAN.md` §15.4).

## Overview

A **sparse checkout** writes only part of a tree to the working directory:
the files matching the tree's sparse patterns. A **partial clone** goes one
step further and downloads only the content of those files; everything else
is downloaded on demand when an operation needs it (diff, merge, blame,
checkout after widening the set). Both are local features of the Rust SDK
and the CLI. The server and the wire protocol are unchanged: a partial
clone simply asks `DownloadObjects` for fewer objects.

Typical use:

```text
wt clone http://localhost:7420 acme/monorepo mono --sparse services/api docs
wt sparse add services/web        # check out (and download) one more area
wt sparse list
wt sparse disable                 # back to a full checkout
```

---

## 1. Patterns

Patterns live in `.wt/trees/<tree>/sparse`, one per line (blank lines and
`#` comments ignored). The file's existence turns sparse checkout on for
that tree; `wt sparse disable` deletes it. Nested trees have their own
sparse settings.

Patterns are relative to the tree root and normalized when stored:

| Input | Stored | Meaning |
|---|---|---|
| `src`, `./src`, `src\` | `/src` | Plain path (no `*`, `?`, `[`): that file or directory and everything below it, anchored at the tree root |
| `docs/guide/` | `/docs/guide/` | Directory only |
| `*.md` | `*.md` | gitignore syntax: any `.md` file at any depth |
| `/*` | `/*` | Every top-level entry (`.` is stored as `/*`) |
| `!docs/private` | `!/docs/private` | Excludes again |

`..` components, empty patterns and comments are rejected; duplicates are
dropped.

**Matching.** For a file path the matcher checks every leading directory
from the top, then the file itself, against the compiled gitignore rules
(last matching pattern wins at each level). The deepest level with a match
decides: a positive match includes, a negated match excludes, no match at
any level excludes. So `docs` + `!docs/private` + `docs/private/keep.md`
checks out `docs/`, not `docs/private/`, except `docs/private/keep.md`.

Paths of an **unresolved merge conflict** (`MERGE_STATE.json`) always
count as inside the set, so their conflict markers are written and visible
until the merge is completed or aborted.

---

## 2. Working-tree semantics

With sparse checkout on, for paths **outside** the set:

| Operation | Behaviour |
|---|---|
| Checkout, switch, pull, merge, revert, rewrite | Never written, never deleted from disk |
| `wt status`, `wt diff` (working tree) | Not scanned: neither modified nor deleted; untracked files there are invisible |
| `wt snapshot` (and stash, auto-snapshot, staging) | The head entry is kept unchanged — exactly like restricted (stubbed) paths, §13.2 |
| Three-way merge | Computed on manifests as usual; content is read (downloaded if needed) only for content merges; the result is recorded in the merge snapshot but not written |

Paths **inside** the set behave as without sparse checkout (deleting one is
a deletion).

Changing the patterns (`set`, `add`, `remove`, `disable`, `reapply`)
updates the working directory under the tree lock and refuses during a
merge (`MergeInProgress`):

- head files inside the set that are missing on disk are written
  (downloaded in one batch first, in a partial clone);
- head files outside the set that are on disk and **unmodified** (content
  hash equals the head entry) are removed;
- **modified** head files outside the set are kept and reported
  (`kept_modified`); once they leave the set their changes are invisible
  to status and snapshots, so snapshot or restore them first and run
  `wt sparse reapply`;
- untracked files are never touched.

Every command reports `{tree, patterns, enabled, written, removed,
kept_modified, fetched}`.

---

## 3. Partial clones

### 3.1 Fetching only sparse content

When a tree has sparse patterns, `fetch` (and therefore `pull` and
`clone --sparse`) changes its download plan (`remote/fetch.rs`):

1. The server's `PullResponse.objects_to_fetch` hint is ignored (it covers
   every path).
2. Every manifest of the new snapshots is downloaded, as before.
3. Blobs and chunk lists are requested only for manifest entries inside the
   sparse set (stubbed paths are skipped as before); chunks only for those
   chunk lists.
4. If any entry outside the set lacked local content, the tree is marked
   **partial**: `.wt/trees/<tree>/partial` holds the remote name.

`wt clone --sparse PATTERN...` validates the patterns before creating the
destination, writes them right after `init`, then fetches — so a sparse
clone downloads the manifests, the sparse paths' blobs/chunks of every
fetched snapshot, and nothing else — and checks out only the sparse paths.
It combines with `--depth`.

### 3.2 On-demand download (lazy fetcher)

`ObjectStore` has an optional `ObjectFetcher`
(`fn fetch(&self, store, hashes) -> Result<usize>`):

- `get` falls back to the fetcher for a missing object, then reads it
  locally; `read_entry` fetches a missing chunk list, then all of its
  missing chunks in one batch; `prefetch` / `prefetch_entries` download a
  batch up front (used when checking out many files).
- `get_local`, `read_chunk_list_local`, `has`, `has_entry` and
  `entry_objects` never fetch.
- Objects the remote does not return stay missing (`ObjectNotFound`).

`Repository::open` installs the remote fetcher when any tree has a
`partial` marker whose remote is configured. It targets
`(remote URL, server tree id)` of each partial tree and calls
`DownloadObjects` scoped with `x-wt-tree-id`, using the saved credential.
Requests run on one background thread with its own Tokio runtime and a
cached connection (reconnecting once on failure), so the synchronous engine
can use it from any context. The server applies its normal rules
(reachability from the tree, license read checks).

Because every read goes through the store, diff, merge, blame, `show`,
archive, checkout and `sparse add`/`disable` work transparently on content
that was never downloaded.

**Offline.** When the server cannot be reached, the operation fails with a
remote error naming the cause, e.g. *"N object(s) needed here are not
stored locally (this is a partial clone: content outside the sparse
checkout is downloaded on demand) and could not be downloaded: cannot
connect to …; check the connection to the server and retry"*. Everything
that only needs local content (status, snapshot, diff of sparse paths,
log) keeps working.

### 3.3 Push

Snapshots made in a sparse or partial clone keep the head entries of paths
outside the set, so their manifests reference content the server already
has. Push only uploads objects the server reports missing
(`MissingObjects`), which are always local, so pushing from a partial clone
never needs the missing content and every other path stays intact on the
server.

### 3.4 Storage maintenance

- `wt gc` reads manifests and chunk lists with the non-fetching readers: it
  never downloads anything, and absent content simply is not marked.
- `wt fsck` treats missing blobs, chunk lists and chunks of a partial
  tree's snapshots like restricted paths: counted in `restricted_missing`,
  not reported as errors. A sparse checkout that is not a partial clone is
  checked as usual.

---

## 4. Status

`wt status` (text) adds a line such as
`Sparse checkout (/src /docs): 12 of 40 files checked out; partial clone,
other content is downloaded from origin on demand`. JSON adds

```json
"sparse": {
  "patterns": ["/src", "/docs"],
  "files_total": 40,
  "files_checked_out": 12,
  "partial_remote": "origin"
}
```

(absent when sparse checkout is off). `wt clone --sparse` prints and
returns the same object; `wt sparse list` returns `{tree, enabled,
patterns, files_total, files_checked_out, partial_remote}`.

---

## 5. Commands

| Command | Effect |
|---|---|
| `wt sparse set PATTERN...` | Enable sparse checkout with exactly these patterns |
| `wt sparse add PATTERN...` | Add patterns (requires sparse checkout on) and write the files they bring in |
| `wt sparse remove PATTERN...` | Remove patterns (each must be set; at least one must remain) |
| `wt sparse list` | Patterns and how many head files are checked out |
| `wt sparse disable` | Turn sparse checkout off; write every head file |
| `wt sparse reapply` | Re-run the working-directory update (e.g. after resolving `kept_modified`) |
| `wt clone URL T/TREE [DIR] --sparse PATTERN...` | Partial clone |

`wt fetch` and `wt pull` need no flag: they follow the tree's patterns.

---

## Implementation status

| Item | Status |
|---|---|
| Patterns, matcher, `.wt/trees/<tree>/sparse` | Built (`repo/sparse.rs`) |
| Working-tree semantics (scan, checkout, snapshot) | Built (`repo/working.rs`) |
| Partial fetch / clone, `partial` marker | Built (`remote/fetch.rs`, `remote/clone.rs`) |
| Lazy fetcher hook and remote fetcher | Built (`store.rs`, `remote/fetch.rs`) |
| gc / fsck in partial clones | Built (`repo/storage.rs`) |
| CLI `wt sparse`, `wt clone --sparse`, status line | Built |
| Cone mode / per-directory fast pruning | Not planned (patterns are evaluated per file) |
| Server-side filtering (`PullRequest` path filter) | Not planned (the client requests exactly what it needs) |
