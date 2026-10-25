# Tree Specification

> **Version:** 0.5 — describes trees, branches and snapshots as built (`crates/worktree-sdk/src/repo`, `docs/ENGINEERING_PLAN.md` §3, §13).
> Features described but not built are listed under [Roadmap](#roadmap).

## Overview

A **tree** is the unit of versioning in W0rkTree. Each tree has its own
branches, `HEAD`, tags, reflog, stat-cache index, lock and server-side tree.
One worktree (one `.wt/` directory) holds a **root tree** and any number of
**nested trees**. Creating a branch, snapshotting or merging in one tree
never touches another tree.

The point of trees is to replace a flat monorepo history with several focused
histories: frontend snapshots contain only frontend files, a backend branch
does not exist in the frontend tree, and access, protection and license rules
are configured per server tree.

```
my-platform/                 root tree "my-platform" (state in .wt/trees/my-platform/)
├── .wt/
├── frontend/                nested tree "frontend"  (marker frontend/.wt-tree/)
│   └── .wt-tree/
├── backend/                 nested tree "backend"
│   └── .wt-tree/
└── docs/                    plain directory → part of the root tree
```

---

## 1. Tree Model

### 1.1 Identity

Each tree is registered in `.wt/trees/<name>/tree.toml`:

```toml
name = "frontend"
id = "0b7f3c9e-5d2a-4e61-8f0c-6a1d2e3b4c5d"   # UUID; replaced by the server tree id on first push/fetch/clone
path = "frontend"                            # '/'-separated, relative to the worktree root; "" = root tree
default_branch = "main"
remote_name = "frontend"                     # server tree name (default: the tree name)
```

| Field | Rules |
|---|---|
| `name` | One filesystem-safe path component: 1-100 chars, no `/ \ : * ? " < > |`, no control characters, must not start with `.` or end with `.` or space |
| `id` | Stable id carried by every snapshot (`tree_id`) |
| `path` | Relative, inside the worktree, not `.wt`/`.wt-tree`, unique among trees |
| `default_branch` | Copied from the worktree's `worktree.default_branch` when the tree is added |
| `remote_name` | Name of the server tree under the remote's tenant |

The current branch is stored separately in `.wt/trees/<name>/HEAD`.

### 1.2 Root tree and nested trees

- `wt init [PATH] [--name N] [--branch B]` creates the root tree, named after
  the directory (sanitized; `root` as fallback) unless `--name` is given,
  with an empty default branch (`main` unless `--branch`).
- `wt tree add NAME PATH` registers `PATH` (created if missing) as a nested
  tree and writes `PATH/.wt-tree/config.toml` containing `name` and `id`.
  Nesting may be arbitrarily deep; a tree inside a nested tree is fine.
- `wt tree list` lists every tree (root first, then by path) with path and
  current branch.
- `wt tree remove NAME` unregisters a nested tree: the `.wt-tree/` marker,
  `.wt/trees/NAME/` (refs, reflog, index) and its conflict file are deleted;
  working files are kept and become part of the enclosing tree. Snapshot
  records stay in `.wt/snapshots/`; pushed history stays on the server. The
  root tree cannot be removed.
- `wt tree validate [NAME]` checks a tree's layout (`.wt-tree/` marker,
  config and ignore files parse, no nested `.wt/`). `wt tree init-config`
  scaffolds `.wt-tree/{config.toml, ignore, hooks/, access/}`.

### 1.3 Which tree a command acts on

Commands use `--tree NAME` if given; otherwise the deepest registered tree
whose directory contains the current directory (or `-C` path); otherwise the
root tree.

### 1.4 File ownership between trees

A file belongs to exactly one tree: the deepest tree whose directory contains
it. A tree's working scan and snapshots skip the directories of trees nested
inside it, so the root tree's manifest never lists `frontend/...` files once
`frontend` is a tree. Moving a file across a tree boundary is a delete in one
tree and an add in the other.

### 1.5 Mapping to the server

A tree is pushed to `<remote tenant>/<remote_name>` on the remote named by
`--remote` (default `sync.remote`, usually `origin`). `ResolveTree
{tenant_slug, tree_name, create_if_missing}` creates the server tree on the
first push; the local tree then adopts the server's tree id
(`Repository::set_tree_id`, also rewriting the `.wt-tree/` marker). Nested
trees are independent server trees: each is pushed, fetched, staged and
cloned on its own. `wt clone URL TENANT/TREE [DEST]` creates a new worktree
whose root tree is that server tree.

The tree registry (`.wt/trees/`) and markers (`.wt-tree/`) are not part of any
snapshot, so the layout of nested trees is local to each worktree.

---

## 2. Branches

### 2.1 Storage

```
.wt/trees/<tree>/HEAD                          current branch name
.wt/trees/<tree>/refs/branches/<branch>        tip snapshot id (empty file = no snapshots yet)
.wt/trees/<tree>/refs/branches.meta.json       {"<branch>": {"created_at": "...", "archived": false}}
.wt/trees/<tree>/refs/remotes/<remote>/<branch> last fetched server tip
```

`BranchInfo` (SDK, `--json`): `name, tip, is_current, created_at, archived,
upstream_tip` (tip of `refs/remotes/origin/<name>`).

Branch and tag names may contain `/` (`feature/login`). Every component must be
non-empty, must not start with `.` or `-`, must not end with `.lock` or `.`; the
name must not contain whitespace, control characters, `~ ^ : ? * [ \ " < > |`,
`..` or `@{`, must be at most 200 characters, and `HEAD` is reserved.

### 2.2 Operations

```bash
wt branch                          # list (same as: wt branch list); -a/--all includes archived
wt branch create feature-auth      # at HEAD; does not switch
wt branch create hotfix --from v1.2.0
wt switch feature-auth             # rewrite the working tree to the branch tip
wt switch -c spike                 # create at HEAD and switch
wt switch main --force             # discard uncommitted changes
wt branch archive feature-auth     # soft delete
wt branch restore feature-auth
```

- `wt switch` refuses when the working tree has uncommitted changes
  (`DirtyWorkingTree`) unless `--force`, or unless
  `auto_snapshot.on_branch_switch` says otherwise (`snapshot`: take an
  auto-snapshot first; `stash`: keep the changes without a snapshot;
  `refuse`: the default refusal). If both branches point at the same
  snapshot, uncommitted changes are carried over untouched. Switching is
  refused while a merge is in progress.
- Switching rewrites tracked files only: untracked and ignored files and
  nested trees' directories are left alone.
- `wt branch archive` cannot archive the current or the default branch. An
  archived branch keeps its tip and history, is hidden from listings, and its
  name cannot be reused until `wt branch restore`.
- Creating, switching, archiving and restoring are recorded in the reflog
  (`branch.create`, `switch`, `branch.archive`, `branch.restore`).

### 2.3 Remote branches

`wt fetch` updates `refs/remotes/<remote>/*` for every server branch and never
moves local branches. `wt pull` fetches, then fast-forwards (with checkout) or
merges `<remote>/<branch>` into the local branch. `wt clone` creates a local
branch for every remote branch and checks out the default branch. Revisions
can name `origin/<branch>`.

On the server, branches can also be created and deleted with `wt trees
branch-create TREE NAME [--from REV]` / `branch-delete` (subject to
`no_delete` protection). A server branch deletion is a hard delete; its
objects become unreachable and are removed by GC after the grace period
(default 720 h).

### 2.4 Branch protection

Protection is a server rule on the tree, matched by branch-name glob, set with
`wt protect set [TREE] PATTERN ...` (fields: `no_direct_push`,
`required_approvals`, `allowed_pushers`, `no_delete`, `require_ci_pass`,
`required_checks`, `require_signed`, `require_linear`, `block_on_todos`).
All matching rules are combined into their most restrictive form and apply to
everyone, server admins included. The client does not evaluate protections;
the server rejects violating pushes and merges. See
[WorkTree.md §14](../WorkTree.md#14-branch-protection-and-merge-requests).

---

## 3. Snapshots

### 3.1 Structure

```rust
pub struct Snapshot {
    pub id: SnapshotId,              // UUID v4 string
    pub tree_id: String,
    pub branch: String,
    pub parents: Vec<SnapshotId>,    // [] first snapshot, [p] normal, [ours, theirs] merge
    pub manifest: ObjectHash,        // BLAKE3 of the protobuf ManifestRecord
    pub author: String,              // "Name <email>" from [user], else OS user, else "unknown"
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub auto_generated: bool,
    pub dependencies: Vec<Dependency>,
    pub reverts: Option<SnapshotId>,
    pub signature: Option<SnapshotSignature>,  // { key_id, signature: 64 bytes }
}
```

Stored as `.wt/snapshots/<id>.json`; on the wire as `SnapshotRecord` (fields
`id, tree_id, branch_name, parents, manifest_hash, author, message,
created_at, auto_generated, dependencies, reverts, signature,
signing_key_id`).

A snapshot does not store a change list or diffs. Changes are computed by
comparing manifests (`Added`, `Modified` — content or executable bit —,
`Deleted`; renames are detected at diff time with `--find-renames`). The
manifest is the complete file list:

```
ManifestEntry { path: "src/auth/oauth.rs", hash: <blake3>, size: 4211, executable: false, chunked: false }
```

Snapshot ids are random, so two snapshots of identical content have different
ids but share the same manifest and blob objects. Tags pointing at a snapshot
are found through the tag refs, not stored on the snapshot.

### 3.2 Creating a snapshot

```bash
wt snapshot -m "Add OAuth2 provider support"
wt snapshot -m "WIP" --allow-empty
wt snapshot -m "Fixture with fake key" --no-scan     # skip secret scanning
wt snapshot -m "Signed" --sign
wt snapshot -m "Dashboard UI" --depend backend --todo "GET /api/dashboard" --priority high --blocking
```

`Repository::snapshot(tree, SnapshotOptions)` does, under the tree lock:

1. Run `pre-snapshot` hooks (`.wt/hooks/`, then `<tree>/.wt-tree/hooks/`);
   a failure aborts. `--no-verify` skips hooks.
2. Scan the working directory (ignore rules applied, nested trees skipped),
   hashing only files whose size/mtime differ from the stat cache
   (`index.json`), storing new blobs (chunk lists for files over 8 MiB).
3. Secret-scan added/modified files; findings abort (`SecretsDetected`)
   unless `[secret_scanning] block_on_match = false` or `--no-scan`.
4. If the manifest equals the head's and `--allow-empty` is not set, fail
   with `NoChanges`.
5. Write the manifest, build the `Snapshot` (parents = current tip), sign it
   when `[signing]` is configured or `--sign` is given, write the record,
   move the branch tip, save the index, append a reflog entry (`snapshot` or
   `auto-snapshot`).
6. Run `post-snapshot` hooks (failures reported, not fatal).

During a conflicted merge, `wt snapshot` succeeds only once no conflict
markers remain, and then records the merge snapshot (two parents).

Auto-snapshots are the same call with `auto_generated = true` and the message
from `auto_snapshot.message_format` (default `auto: {count} files changed`),
made by the daemon for every tree of a watched worktree that has changes; see
[BgProcess.md](../bgprocess/BgProcess.md). They land on the current branch
and are never pushed automatically.

### 3.3 History graph

Snapshots form a DAG per tree through `parents`. `wt log [-n N] [REV]
[--oneline] [--show-signature]` walks first parents from `REV` (default
`HEAD`) and stops at a shallow boundary. `wt show REV [--stat]` prints the
metadata (including dependencies and `Reverts:`) and the patch against the
first parent.

```
main:     a3f8 ← b7d1 ← c4a6 ← d9e2 ←───────── j7k8 (merge: parents d9e2, f8c2)
                          └─ feature: e5f3 ← f8c2 ┘
```

Revision syntax: `HEAD` / `@`, `HEAD~N`, `REV^N`, chains like `main~2^2`,
branch, `origin/<branch>`, tag, id prefix (≥ 4 chars), `@{N}`, `BRANCH@{N}`.

### 3.4 Append-only guarantees

- There is no amend, rebase, reset or force push. A local branch tip moves
  only by snapshot, merge (fast-forward or merge snapshot), revert, pull or
  clone/fetch plumbing.
- The server accepts `Push{old_tip, new_tip}` only if `new_tip` descends from
  the current server tip and every referenced snapshot, manifest, blob, chunk
  list and chunk is present; otherwise `ConflictDetected{server_tip}` or a
  rejection.
- `wt revert` undoes a snapshot by adding a new one.
- `wt restore [PATH...] [--from REV]` and `wt switch --force` overwrite
  uncommitted working changes; these are not recoverable.

---

## 4. Staged Snapshots (per tree)

`wt stage [--remote R]` uploads the tree's working state as a staged snapshot
on its server tree:

1. Build a manifest from the working directory and upload the objects the
   server lacks.
2. Compare it with the shared branch head (the server tip of the current
   branch, else the last fetched remote-tracking tip) to produce
   `files_changed`, `files_added`, `files_modified`, `files_deleted`.
3. Send `StageUploadRequest`. When the working tree equals the local branch
   tip, that snapshot's id is staged; otherwise a fresh id names the WIP.

| | Staged | Pushed |
|---|---|---|
| Created by | `wt stage`, daemon auto-stage | `wt push` |
| Part of branch history | No | Yes |
| Visible to | Everyone who can read the tree (`wt status --team`, `wt trees staged TREE`, admin panel, `GET .../staged`) | Same, as history |
| Multiplicity | One per (tree, account, branch); a newer stage replaces it | Every snapshot |
| Removed by | Successful push of that branch by the same account, `wt stage --discard` / `wt unstage` (`Unstage` RPC), server retention (default 168 h) | Never (objects GC'd only if unreachable) |
| Protection checks | None | Yes |

```
$ wt status --team
...
Staged work on origin:
  alice  feature-auth  3 files    2 minutes ago     Add OAuth2 flow
      + 1  ~ 2  - 0
  bob    main          2 files    15 minutes ago    auto: 2 files changed
      + 0  ~ 2  - 0
```

Events `snapshot.staged` / `staged.cleared` reach the admin panel and `wt
events`. `GET .../staged/summary` aggregates activity per branch.

---

## 5. Access Control (per tree)

Access is decided by the server for each server tree:

- **Roles:** tenant membership role (`owner`, `admin`, `maintainer`,
  `developer`, `viewer`, or a custom role).
- **Grants:** `wt grant set acme/web --user bob|--team qa|--sa ci --role R`;
  `subject_type: "tenant"` grants cross-tenant access.
- **Visibility:** `private`, `internal`, `public` (`wt trees update TREE
  --visibility V`).
- **Policies:** tenant policies with allow/deny statements; resources can name
  a tree (`wt:tree/acme/web`), a branch (`wt:branch/acme/web/main`) or paths
  inside a tree (`wt:path/acme/web/src/crypto/**`), with `*`/`**` wildcards. Path-level denies are enforced on
  push (`tree:write` per changed path → `PushDenied{kind:"access_denied"}`),
  on pull (`stubbed_paths`) and on REST reads (`403 {"code":"path_denied"}`).
- **Declarative tree policies:** `<tree>/.wt-tree/access/policies.toml`
  contains deny statements only and is applied with `wt access apply` to the
  tree's server tree.

```toml
# frontend/.wt-tree/access/policies.toml
[[policy]]
name = "lock-crypto"
enabled = true
[[policy.statement]]
sid = "only-security-writes-crypto"
effect = "deny"
principals = ["team:contractors"]
actions = ["tree:write"]
resources = ["wt:path/acme/frontend/src/crypto/**"]
```

Resource syntax and evaluation order: [IAM.md](../iam/IAM.md),
[DeclarativeAccess.md](../iam/DeclarativeAccess.md).

---

## 6. Licenses (per tree)

License rules are server rules of the tree: `wt license set [TREE] GLOB SPDX
[--grant read-only|modify|redistribute] [--grantee TENANT] [--expires T]`.
The most specific glob wins; owning-tenant members are never restricted.
`wt license detect [PATH]` suggests SPDX ids from file contents (`--apply`
proposes rules). See [LicenseCompliance.md](../licensing/LicenseCompliance.md).

---

## 7. Ignore Patterns (per tree)

A tree's scan applies, in order of increasing precedence:

1. hard-coded `.wt/`, `.wt-tree/`, `.git/` (cannot be re-included);
2. soft defaults `__pycache__/ *.pyc .env *.log node_modules/ .venv/ dist/
   build/ target/` (re-includable with `!pattern`);
3. `.wt/ignore` and the root `.wtignore` (root level; cannot be re-included
   by deeper files);
4. `<tree>/.wt-tree/ignore` for that tree;
5. `.wtignore` files in subdirectories (deeper wins; last match wins within a
   file).

Only `.wtignore` files are tracked and therefore shared. Ignored paths never
trigger daemon snapshots. Compiled matchers are cached per tree.

```
# frontend/.wtignore
dist/
*.local.env
!dist/keep.txt
```

---

## 8. Dependencies, TODOs and Linked Merges

### 8.1 Snapshot dependencies

A snapshot can carry dependencies on other trees (`Dependency {tenant, tree,
branch, title, details, priority, blocking, linked}`), declared with
`wt snapshot --depend TREE[@BRANCH] [--depend-tenant T] [--todo TITLE]
[--details TEXT] [--priority low|medium|high|critical] [--blocking]
[--linked]`. The flag group is repeatable.

### 8.2 TODOs

When such a snapshot is pushed, the server creates a TODO in the target tree
(if the pusher can read it). TODO fields: `number` (per target tree),
`target_branch`, `title`, `details`, `priority`, `blocking`, `linked`,
`status` (`open`, `in_progress`, `done`, `cancelled`), source tenant / tree /
branch / snapshot, `author`, `assignee`, `resolved_by`.

```bash
# in the backend tree
wt todo list                            # e.g. #3 "GET /api/dashboard", high, blocking, from acme/frontend
wt todo claim 3                         # assigns you, status in_progress
wt snapshot -m "Add dashboard endpoint

Completes: #3"
wt push                                 # the server marks TODO #3 done (resolved_by = that snapshot)
wt todo complete 3 --snapshot 5e6f7a8b  # or complete explicitly
```

`wt deps status [--branch B]` shows, for the current tree, each outgoing
dependency with the state of its TODO and the incoming TODOs; `wt deps graph
[--format dot]` prints the graph. REST: `GET {tree}/todos`,
`GET {tree}/dependencies`.

### 8.3 Linked merges

Merge requests in different trees can be grouped so they merge together:

```bash
wt mr link oauth acme/frontend#4 acme/backend#9
wt mr groups
wt mr merge-group <id>     # all members must be mergeable; tips move in order, rolled back on failure
```

A member of a linked group cannot be merged alone (`409
{"code":"linked_group"}`). With `block_on_todos` protection, open blocking
TODOs whose target branch is the MR's source branch block the merge.

---

## 9. Tags and Releases (per tree)

Tags live in `.wt/trees/<tree>/refs/tags/<name>.json` (`TagInfo {name, target,
message, tagger, created_at}`); the same name can exist in different trees.
`wt tag create NAME [REV] [-m MSG]`, `wt tag delete NAME`. `wt push` pushes
missing tags; `wt fetch` downloads server tags. Tag protections (`wt protect
tag set PATTERN --no-delete --no-move --tagger U`) make server tags immutable.

Releases belong to a server tree and one of its tags: `wt release create TAG
[--name N] [--notes T | --notes-file F] [--draft] [--prerelease]
[--snapshot ID]`, `wt release upload TAG FILE...`, `wt release download TAG
[ASSET] [-o DIR]`.

---

## 10. Merge and Conflicts (per tree)

`wt merge SOURCE [-m MSG] [--no-ff] [--strategy ours|theirs|manual]` merges a
branch or revision of the same tree into the current branch:

- up to date → nothing; fast-forward possible and no `--no-ff` → move the tip
  and check out (reflog `fast-forward`);
- otherwise a three-way merge against the merge base. Clean results become a
  merge snapshot `[ours, theirs]`; conflicts leave the merged working tree
  with markers, `.wt/trees/<tree>/MERGE_STATE.json` and
  `.wt/conflicts/<tree>.json`, and exit code `3`.

```json
[{"path":"src/auth/login.rs","kind":"content",
  "hunks":[{"ours_start":45,"ours_len":2,"theirs_start":45,"theirs_len":3}]},
 {"path":"assets/logo.png","kind":"binary","hunks":[]}]
```

Resolution: edit files, or `wt merge resolve PATH --ours|--theirs|--file F`,
`wt merge resolve --all --ours|--theirs`; check with `wt merge status`;
finish with `wt merge --continue [-m MSG]` (or `wt snapshot -m MSG`); give up
with `wt merge abort`. Binary conflicts keep "ours" in the working tree until
resolved.

Merges never cross trees, and the daemon never merges.

---

## 11. Large Files (per tree)

Files over 8 MiB are chunked automatically (FastCDC, 256 KiB / 1 MiB / 4 MiB);
the threshold is fixed and not configurable per tree. Only changed chunks are
stored and transferred. Checkout always writes complete files.

---

## 12. Reflog (per tree)

`.wt/trees/<tree>/reflog.jsonl`, one `ReflogEntry {at, tree, branch, action,
old, new, message}` per line, covering all branches of the tree:

```json
{"at":"2026-10-25T09:12:03Z","tree":"frontend","branch":"feature-auth","action":"switch","old":"9a0b...","new":"5e6f...","message":"moving from main to feature-auth"}
{"at":"2026-10-25T10:30:41Z","tree":"frontend","branch":"feature-auth","action":"snapshot","old":"5e6f...","new":"7c8d...","message":"Add OAuth2 provider support"}
```

`wt reflog` prints the entries newest first.

`wt reflog [--branch B] [-n N]`, `wt reflog expire [--older-than 90d]
[--keep N]` (default 90 days / 1000 entries per tree, also applied by the
daemon every 6 h). `@{N}` and `BRANCH@{N}` revisions read the reflog.

---

## 13. Shallow History and Restricted Paths (per tree)

`wt clone --depth N`, `wt fetch --depth N | --deepen N`, `wt pull --depth N`.
Boundary snapshot ids: `.wt/trees/<tree>/shallow`. Paths withheld by the
server for license or path-deny reasons: `.wt/trees/<tree>/stubbed` (not
written on checkout; `wt status` reports them as restricted).

---

## 14. Tree-Level Configuration

`<tree>/.wt-tree/config.toml` holds the marker identity plus overrides of the
worktree configuration for this tree:

```toml
name = "frontend"
id = "0b7f3c9e-5d2a-4e61-8f0c-6a1d2e3b4c5d"

[auto_snapshot]
max_changed_files = 20
message_format = "auto({branch}): {count} files changed"

[secret_scanning]
exclude = ["tests/fixtures/**"]

[hooks]
timeout_secs = 120
```

Resolution order: `$WT_HOME/config.toml` < `.wt/config.toml` <
`<tree>/.wt-tree/config.toml`. `wt config get KEY --tree frontend
--show-origin` shows the effective value and its file; `wt config set KEY
VALUE --tree frontend` writes the tree file. Full reference:
[DotWtTree.md](../dot-wt-tree/DotWtTree.md).

---

## 15. Operations Summary

```bash
wt init my-platform && cd my-platform
wt tree add frontend frontend
wt tree add backend backend
wt --tree frontend snapshot -m "Initial frontend"
cd backend && wt snapshot -m "Initial backend"
wt remote add origin http://localhost:7420 --tenant acme
wt login -u ada
wt push                          # pushes the backend tree (current directory) to acme/backend
wt --tree frontend push          # pushes acme/frontend
wt --tree frontend log --oneline
wt diff HEAD~1 HEAD --stat
wt revert 5e6f7a8b
```

---

## Implementation Status

| Feature | Status |
|---|---|
| Tree registry, root/nested trees, `.wt-tree/` markers, path ownership | Built |
| `wt tree list/add/remove`; `validate`, `init-config` | Built (the latter two in v0.5) |
| Per-tree branches, HEAD, tags, reflog, index, lock | Built |
| Branch archive/restore (soft delete) | Built |
| Snapshots, flat manifests, DAG, revision syntax | Built (`@{N}` in v0.5) |
| Staged snapshots, team status, unstage | Built (unstage in v0.5) |
| Server-side protections, grants, path policies, license rules per tree | Built |
| Snapshot dependencies, TODOs, linked merge-request groups | Built (v0.5) |
| Merge strategies, `merge resolve/status/abort`, conflict hunks | Built (v0.5) |
| Chunked large files, shallow sync, stubbed paths | Built (v0.5) |
| Tree-level config overrides, hooks, ignore file | Built (v0.5) |
| Branch strategies, stub trees, partial worktree sync | Roadmap |

## Roadmap

- **Branch strategies** (`branch_strategy = "feature-branch" | "trunk-based" |
  "release-branch" | "custom"`) and server enforcement.
- **Protection, license and dependency rules declared in
  `.wt-tree/config.toml`** (`[[branch_protection]]`, `[license]`,
  `[[license.path_override]]`, `[[license.grant]]`, `[[dependency]]` with
  version constraints, `[[linked_branches]]`, `[[registered_path]]`); today
  these are server rules managed by CLI/REST, and path rules use globs.
- **Branch metadata** `created_by`, `parent_branch`, `linked_group`, and tree
  metadata `parent`, `created_by`, `description` in the tree record.
- **Branch-level dependency files** (`.wt-tree/branches/<branch>.toml`),
  `wt branch status`, `wt deps check`, automatic TODO branches on `wt todo
  claim` (`--assign-to`), `wt todo link`, e-mail notifications, automatic
  linked groups from `--linked` dependencies, `wt branch merge-linked`.
- **Auto-snapshot before switching** as an always-on behaviour (today opt-in
  via `on_branch_switch = "snapshot"`); `min_interval_secs`.
- **Stub trees and partial worktree sync** (`wt init --from URL --trees a,b`,
  `wt sync --tree`, `[stub]` in `wt tree list`), sharing the nested-tree
  layout through the server, and multi-tree clone.
- **Lazy loading of large files** and a configurable `[large_files]`
  threshold with a root-level ceiling.
- **Tree-level reflog retention** (`[reflog] retention_days`) and server-side
  reflog.
- **Snapshot change lists with stored hunks**, `Renamed`/`Copied` operations
  recorded on the snapshot, content-derived snapshot ids.
- **Specificity-ordered scope hierarchy** (`RegisteredPath > Branch > Tree >
  Tenant > Global`) and subject syntax `{ tenant = "alice" }`; the built
  policy model is described in IAM.md.

## References

- `crates/worktree-sdk/src/repo/mod.rs` — layout doc comment
- `crates/worktree-sdk/src/repo/trees.rs` — tree registry, markers, `tree_for_path`
- `crates/worktree-sdk/src/repo/refs.rs` — branches, archive/restore, remote refs, tags
- `crates/worktree-sdk/src/repo/history.rs` — snapshot records, revisions, ancestry
- `crates/worktree-sdk/src/repo/snapshotting.rs` — status, snapshot creation, checkout
- `crates/worktree-sdk/src/repo/working.rs` — scanning, stat cache, materialization
- `crates/worktree-sdk/src/repo/merging.rs`, `src/merge/` — merge, revert, restore
- `crates/worktree-sdk/src/repo/{reflog,ignore_rules,names,lock}.rs`
- `crates/worktree-sdk/src/model.rs` — `Snapshot`, `Manifest`, `BranchInfo`, `TagInfo`, `ConflictInfo`
- `crates/worktree-sdk/src/remote/{session,push,fetch,clone,stage}.rs`
- `crates/worktree-cli/src/commands/{tree,branch,snapshot,history,merge,todo,deps,mr}.rs`
- `crates/worktree-protocol/proto/sync.proto` — `SnapshotRecord`, `ManifestRecord`, `StageUploadRequest`, `Dependency`
- `services/server-go/internal/{review,todo,staged,syncsvc}/`
- `services/server-go/internal/domain/{types,v05}.go`
