# W0rkTree Specification

> **Version:** 0.5 (describes the system as built after the v0.5 workstreams, `docs/ENGINEERING_PLAN.md` §13)
> **Status:** Authoritative for behaviour; features that are described but not built are listed under [Roadmap](#roadmap)
> **Terminology:** "W0rkTree" (product name, with a zero), `worktree` (code, all lowercase), `wt` (the CLI)

---

## Table of Contents

1. [Overview](#1-overview)
2. [Core Concepts](#2-core-concepts)
3. [Architecture](#3-architecture)
4. [Worktree Structure](#4-worktree-structure)
5. [Configuration Hierarchy](#5-configuration-hierarchy)
6. [Tenants, Visibility and Access](#6-tenants-visibility-and-access)
7. [Dependencies and TODOs](#7-dependencies-and-todos)
8. [Tags and Releases](#8-tags-and-releases)
9. [Merge and Conflict Resolution](#9-merge-and-conflict-resolution)
10. [License Compliance](#10-license-compliance)
11. [Ignore Patterns](#11-ignore-patterns)
12. [Large Files](#12-large-files)
13. [Shallow History and Restricted Paths](#13-shallow-history-and-restricted-paths)
14. [Branch Protection and Merge Requests](#14-branch-protection-and-merge-requests)
15. [Reflog](#15-reflog)
16. [Revert](#16-revert)
17. [Archives](#17-archives)
18. [Diff](#18-diff)
19. [Snapshot Signing](#19-snapshot-signing)
20. [Hooks and Secret Scanning](#20-hooks-and-secret-scanning)
21. [Git Compatibility](#21-git-compatibility)
22. [Comparison with Git](#22-comparison-with-git)
23. [Design Principles](#23-design-principles)
24. [Command Reference Summary](#24-command-reference-summary)
25. [Implementation Status](#implementation-status)
26. [Roadmap](#roadmap)
27. [References](#references)

---

## 1. Overview

W0rkTree is a distributed version control system with its own object model, wire
protocol, server and identity system. It is not a Git wrapper; Git is supported
only through an import/export bridge (`wt git ...`).

What W0rkTree provides beyond Git:

- **Multi-tenant server.** Accounts, tenants (user or organization), teams,
  roles, policies, service accounts and per-tree grants are server concepts,
  enforced on every gRPC and REST call.
- **Nested trees.** One worktree can contain several independently versioned
  trees, each with its own branches, tags, reflog and server-side tree.
- **Automatic snapshots.** An optional background daemon (`worktree-bg`)
  snapshots watched worktrees after a quiet period.
- **Staged snapshots.** Work in progress can be uploaded as a *staged*
  snapshot that teammates see (`wt status --team`, admin panel) without it
  becoming branch history.
- **Append-only shared history.** There is no rebase, reset or force push;
  the server accepts a push only if it descends from the current branch tip.
- **Server-enforced rules.** Branch protection, merge requests with reviews
  and CI gates, path-level license rules, path-level policies, quotas.
- **Large files without LFS.** Files over 8 MiB are stored as content-defined
  chunks automatically.
- **Cross-tree coordination.** Snapshots can declare dependencies on other
  trees; pushing them creates TODOs in the target tree; merge requests in
  different trees can be merged together as a linked group.

---

## 2. Core Concepts

### Worktree

A **worktree** is a directory on disk containing a `.wt/` metadata directory
at its root (see [DotWt.md](./dot-wt/DotWt.md)). `wt init` creates one;
`wt clone` creates one from a server tree. A worktree holds one or more trees.
Every command run inside the directory (or with `-C <path>`) finds the
worktree by walking up to the nearest `.wt/`.

### Tree

A **tree** is the unit of versioning: it has its own branches, `HEAD`, tags,
reflog, stat-cache index and lock. Every worktree has a **root tree** (path
`""`, named after the directory unless `--name` is given). Further trees are
registered with `wt tree add NAME PATH`, which writes a `.wt-tree/` marker
directory into `PATH`. A tree's snapshots exclude the paths of trees nested
inside it, so a file belongs to exactly one tree: the deepest registered tree
whose directory contains it. See [Tree.md](./tree/Tree.md).

Each tree maps to one server tree (`<tenant>/<remote_name>`; `remote_name`
defaults to the tree name).

### Snapshot

A **snapshot** is an immutable record of a tree's complete file state:

| Field | Meaning |
|---|---|
| `id` | UUID v4 string (not content-derived; identical content gets a new id) |
| `tree_id` | Tree id (UUID; adopts the server's tree id on first contact) |
| `branch` | Branch the snapshot was created on (informational) |
| `parents` | Zero (first snapshot), one, or two (merge) parent ids |
| `manifest` | BLAKE3 hash of the protobuf `ManifestRecord` listing every file |
| `author`, `message`, `created_at`, `auto_generated` | Metadata |
| `dependencies` | Declared dependencies on other trees (become TODOs on push) |
| `reverts` | Set on revert snapshots |
| `signature` | Optional Ed25519 signature (`signature`, `signing_key_id` on the wire) |

Snapshot records are stored locally as `.wt/snapshots/<id>.json` and travel on
the wire as `SnapshotRecord` messages. They are not content-addressed objects.

### Manifest and objects

A **manifest** is a sorted, duplicate-free list of `ManifestEntry {path,
blob_hash, size, executable, chunked}` with `/`-separated paths relative to the
tree root. Manifests are flat (no directory objects). Manifest paths are
validated: no empty, `.` or `..` components, no backslash or NUL, never inside
`.wt/` or `.wt-tree/`.

**Objects** are content-addressed by BLAKE3-256 of their exact bytes:

| Kind (`ObjectKind`) | Content |
|---|---|
| `OBJECT_KIND_BLOB` | Raw file bytes, or one chunk of a large file |
| `OBJECT_KIND_MANIFEST` | Protobuf-encoded `ManifestRecord` |
| `OBJECT_KIND_CHUNK_LIST` | Protobuf-encoded `ChunkList` (large files, §12) |

Locally they live uncompressed at `.wt/objects/<hh>/<62 hex>`; the server
stores the same bytes under the same hashes, so transfer is a byte copy.

### Branch

A **branch** is a named, mutable pointer to a snapshot (or empty, before the
first snapshot) inside one tree. Branches of different trees are unrelated
even when they share a name. Branches are created with `wt branch create`
(or `wt switch -c`), switched with `wt switch`, and soft-deleted with
`wt branch archive` (restorable with `wt branch restore`). Remote-tracking
branches (`refs/remotes/<remote>/<branch>`) record what the server had at the
last fetch.

### Tag and release

A **tag** names a snapshot within a tree (`wt tag create NAME [REV] [-m MSG]`;
with a message it is annotated). A **release** is a server object attached to
a tag of a server tree, with notes, draft/prerelease flags and uploaded assets.

### Staged snapshot

A **staged snapshot** is work in progress uploaded with `wt stage` (or by the
daemon after an auto-snapshot when `sync.auto_stage = true`). The server keeps
at most one staged entry per (tree, account, branch); a newer stage replaces
it. Staged entries carry the changed-file summary against the shared branch
head and are visible to everyone who can read the tree. They never become
branch history: `wt push` publishes snapshots, and a successful push clears
the pusher's staged entry for that branch. `wt stage --discard` / `wt unstage`
remove it explicitly.

### Tenant and account

An **account** is a user identity on a server (username, email, password,
optional TOTP MFA). A **tenant** is an owner namespace: every account has a
personal tenant (kind `user`, slug = username) and can create organization
tenants (kind `org`). Trees belong to tenants. Membership roles are `owner`,
`admin`, `maintainer`, `developer`, `viewer`, plus custom roles.

### Remote

A **remote** is a named server entry in `.wt/config.toml`:
`[remote.<name>] url = "http://localhost:7420", tenant = "acme"`. One port
serves gRPC (`worktree.sync.v1.SyncService`, HTTP/2 cleartext or TLS) and the
REST API.

### Terminology

| W0rkTree | Closest Git term | Note |
|---|---|---|
| snapshot | commit | UUID id, flat manifest |
| tree | repository | several per worktree |
| worktree | working copy + `.git` | one `.wt/` per worktree |
| push / fetch / pull | push / fetch / pull | same directions; push must fast-forward |
| stage | (none) | uploads WIP for visibility, not an index |
| switch | `checkout <branch>` / `switch` | |
| restore | `checkout -- <path>` / `restore` | |
| branch archive | branch delete | soft delete, restorable |

There is no staging area and no `add` command: `wt snapshot` records the whole
working state of the tree (minus ignored paths). The per-tree `index.json` is
only a stat cache that avoids re-hashing unchanged files.

---

## 3. Architecture

```
 wt (CLI, Rust) ──► worktree-sdk (Rust) ──► .wt/ on disk
      │                  │  remote: gRPC (h2c or TLS) + REST, same port
      │ IPC: NDJSON/TCP  ▼
 worktree-bg (Rust) ──► worktree-server (Go, services/server-go) ──► bbolt meta.db + objects/
   (optional daemon)          ▲ REST + SSE
                         apps/admin (React/Vite admin panel, served at /admin/)
 worktree-git (Rust) — Git import/export over worktree-sdk
```

| Component | Crate / path | Role |
|---|---|---|
| CLI | `crates/worktree-cli` (binary `wt`) | Every user command; thin layer over the SDK |
| SDK | `crates/worktree-sdk` | `Repository` (local engine), object store, config, merge/diff, remote client, IPC types |
| Daemon | `crates/worktree-bg` (binary `worktree-bg`) | Watches worktrees, auto-snapshots, auto-stages, background fetch |
| Protocol | `crates/worktree-protocol` | `sync.proto`, hashing, ignore matcher, declarative-access compiler |
| Server | `services/server-go` | Tenants, IAM, canonical branch history, objects, reviews, CI, TODOs, releases, audit, GC |
| Admin panel | `apps/admin` | Web UI over the REST API |
| Git bridge | `crates/worktree-git` | `wt git import/export/push/remote` |

### Separation of concerns (as built)

- **All local version control runs in the CLI process via the SDK.** Snapshots,
  branches, merges, diffs, reverts and pushes do not need the daemon; `wt`
  works with the daemon stopped. The daemon only adds automation (watching,
  auto-snapshots, auto-staging, background fetching). See
  [BgProcess.md](./bgprocess/BgProcess.md).
- **CLI and daemon never interleave writes to a tree:** every mutating
  operation takes the per-tree OS file lock `.wt/trees/<tree>/LOCK`.
- **The server never touches a working directory and never creates
  snapshots.** Merging a merge request (`POST .../merge-requests/{n}/merge`)
  fast-forwards the target branch to the source tip with a compare-and-swap;
  if the branches have diverged it answers `409` and the merge must be done
  locally (`wt merge`) and pushed.
- **Local history is complete for the branches you have**; the server is the
  authority for shared branch tips. A push whose `new_tip` does not descend
  from the server tip is rejected with `ConflictDetected{server_tip}`; the
  client must pull (merge) first.
- **Enforcement is server-side.** The client reads no access rules; access,
  protection, license and quota checks happen on the server for every push,
  pull, download and REST call. Local-only operations are not restricted.

---

## 4. Worktree Structure

```
my-project/                          ← worktree root (root tree "my-project")
├── .wt/                             ← metadata (never snapshotted), see DotWt.md
│   ├── config.toml                  ← WorktreeConfig
│   ├── ignore                       ← worktree-wide ignore patterns (local only)
│   ├── objects/<hh>/<62 hex>        ← blobs, manifests, chunk lists
│   ├── snapshots/<uuid>.json        ← snapshot records
│   ├── trees/<tree>/                ← per-tree state: tree.toml, HEAD, refs/, reflog.jsonl, ...
│   ├── conflicts/<tree>.json        ← conflict list during a merge
│   ├── hooks/                       ← worktree-wide snapshot hooks
│   └── access/{roles,policies}.toml ← declarative access (tenant scope)
├── .wtignore                        ← ignore patterns, tracked like any file
├── services/
│   └── auth/                        ← nested tree "auth"
│       ├── .wt-tree/                ← marker + tree-level config (never snapshotted)
│       │   ├── config.toml          ← name, id, [auto_snapshot], [secret_scanning], [hooks]
│       │   ├── ignore
│       │   ├── hooks/
│       │   └── access/policies.toml ← tree-scope deny statements
│       └── src/...
└── README.md                        ← belongs to the root tree
```

Rules:

- One `.wt/` per worktree, at the root. A nested `.wt/` is invalid (`wt tree
  validate` reports it).
- The root tree has no `.wt-tree/`; its state is under `.wt/trees/<root>/`.
- Every nested tree has a `.wt-tree/` marker; its registration lives in
  `.wt/trees/<name>/tree.toml`. Nesting is recursive.
- `.wt/` and `.wt-tree/` contents are never part of a snapshot, so they are
  not shared by push/clone. Use tracked `.wtignore` files for ignore rules
  that the team should share.

Per-user state lives outside the worktree in `$WT_HOME` (default `~/.wt`):
`credentials.toml`, `config.toml` (user defaults), `keys/` (signing keys),
`bg.json`, `bg.toml`, `bg.log`.

---

## 5. Configuration Hierarchy

Configuration is TOML at three levels; a more specific level overrides a less
specific one key by key:

```
1. <WT_HOME>/config.toml          user defaults: [user], [signing], [secret_scanning]
2. .wt/config.toml                worktree: [worktree] [user] [auto_snapshot] [sync]
                                  [remote.<name>] [secret_scanning] [hooks] [signing] [git_remote.<name>]
3. <tree>/.wt-tree/config.toml    tree: [auto_snapshot] [secret_scanning] [hooks]
```

`wt config get KEY --show-origin` prints the value and the file it came from;
`wt config set KEY VALUE` writes `.wt/config.toml`, `--tree NAME` writes the
tree file, `--global` writes the user file. Unknown keys are preserved when a
file is rewritten.

Environment variables used by the tools:

| Variable | Effect |
|---|---|
| `WT_HOME` | Per-user state directory (default `~/.wt`) |
| `WT_SERVER` | Default server for server-side commands (same as `--server`) |
| `WT_PASSWORD` | Password for `wt login` / `wt register` / password prompts |
| `WT_MFA_CODE` | TOTP or recovery code for `wt login` |
| `WT_LOG` | Daemon log filter (default `info`) |

Example `.wt/config.toml` (as written by `wt init` plus a remote):

```toml
[worktree]
name = "my-project"
id = "3f6c1d2a-8f0e-4c55-9d7b-2a1e5b9c0d44"
created_at = "2026-10-25T09:00:00+00:00"
default_branch = "main"

[user]
name = "Ada Lovelace"
email = "ada@example.com"

[auto_snapshot]
enabled = true
inactivity_secs = 30
max_changed_files = 50

[sync]
auto_stage = true
remote = "origin"
auto_fetch = true
interval_secs = 300

[remote.origin]
url = "http://localhost:7420"
tenant = "acme"
```

Access permissions are **not** part of this hierarchy: they are server state
(roles, grants, policies), optionally managed declaratively from
`.wt/access/` files (§6). There is no "permission ceiling" computed from
config files; the server's evaluation order is described in
[IAM.md](./iam/IAM.md).

The full key reference is in [DotWt.md](./dot-wt/DotWt.md) and
[DotWtTree.md](./dot-wt-tree/DotWtTree.md).

---

## 6. Tenants, Visibility and Access

### Tenants

| Property | Values |
|---|---|
| `slug` | lowercase `[a-z0-9-]`, unique; personal tenant slug = username |
| `kind` | `user` or `org` |
| `plan` | `free`, `pro`, `enterprise` |
| `status` | `active` (default), `suspended` (writes denied), `deactivated` (everything denied) |
| `limits` | optional quotas: max trees, storage bytes, branches per tree, members, requests/minute |

CLI: `wt tenant list | create SLUG | show T | members T | add-member T USER
--role R | suspend|resume|deactivate T [--reason]`.

### Tree visibility

Set per server tree (`wt trees create acme/web --visibility internal`):

| Visibility | Who can read |
|---|---|
| `private` (default) | Tenant members and explicit grantees |
| `internal` | Any authenticated account |
| `public` | Everyone, including anonymous reads |

Write access always requires a role, grant or allow policy.

### Access control

Authorization (server package `internal/authz`) combines: membership roles
(built-in or custom), team grants and direct tree grants (users, teams,
service accounts, and `tenant:<slug>` for cross-tenant access), token scopes,
and tenant policies with allow/deny statements whose resources and
conditions (CIDR, MFA, time windows) can target trees, branches and paths.
An explicit deny wins; tenant owners are protected from locking themselves out
of `iam:*` and `tenant:admin`.

Policies and custom roles can be managed declaratively from TOML files and
applied Terraform-style (`wt access validate | plan | apply | pull`):

```toml
# .wt/access/roles.toml
[[role]]
name = "release-manager"
description = "Cut releases"
permissions = ["tree:read", "release:*", "mr:merge"]
```

```toml
# .wt/access/policies.toml
[[policy]]
name = "protect-main"
description = "Only maintainers push to main"
enabled = true
[[policy.statement]]
sid = "no-direct-main"
effect = "deny"
principals = ["role:developer", "team:contractors"]
actions = ["branch:push"]
resources = ["wt:branch/acme/web/main"]
conditions = { mfa = true }
```

`<tree>/.wt-tree/access/policies.toml` holds tree-scoped policies that may
contain **deny statements only**. Applied roles and policies are marked
`managed_by = "config"` and become read-only in the REST API. Details:
[DeclarativeAccess.md](./iam/DeclarativeAccess.md), [IAM.md](./iam/IAM.md),
[TenantModel.md](./iam/TenantModel.md).

---

## 7. Dependencies and TODOs

A snapshot can declare that another tree must do work:

```bash
wt snapshot -m "Dashboard UI" \
  --depend backend@main --todo "GET /api/dashboard endpoint" \
  --details "paginated, last 30 days" --priority high --blocking --linked
```

- `--depend TREE[@BRANCH]` names the target tree (same tenant, or
  `--depend-tenant T`); the group of flags is repeatable. Without `--todo` the
  title is the first line of the snapshot message.
- The dependency is stored on the snapshot (`Snapshot.dependencies`,
  `SnapshotRecord.dependencies`, fields `tenant_slug, tree, branch, title,
  details, priority, blocking, linked`).
- When the snapshot is **pushed**, the server's TODO service creates a TODO in
  the target tree (status `open`, per-tree number), provided the pusher can
  read the target tree (otherwise it audits `todo.skipped`). Duplicates (same
  source snapshot, target tree and title) are not recreated. Event
  `todo.created`.
- A pushed snapshot whose message contains a line `Completes: #N` (or
  `Completes: tenant/tree#N`) marks that TODO `done` and records the snapshot
  as `resolved_by`.
- Statuses: `open`, `in_progress`, `done`, `cancelled`. Priorities: `low`,
  `medium` (default), `high`, `critical`.

```bash
wt todo list [--status open] [--all-trees]
wt todo show 3
wt todo claim 3                    # assign to me, status in_progress
wt todo complete 3 [--snapshot ID]
wt todo cancel 3
wt todo create "Rotate keys" ...   # manual TODO
wt deps status [--branch B]        # outgoing dependencies and their TODO status, incoming TODOs
wt deps graph [--format dot]
```

Branch protection can block merging while open blocking TODOs target the
source branch (`wt protect set ... --block-on-todos`, §14). Merge requests in
different trees can be bound into a **linked group** that merges all-or-nothing
(`wt mr link NAME acme/web#4 acme/api#9`, `wt mr merge-group ID`). The
`linked` flag on a dependency is recorded on the TODO; linking the merge
requests is an explicit step.

---

## 8. Tags and Releases

### Tags

Tags are per tree, both locally (`.wt/trees/<tree>/refs/tags/<name>.json`) and
on the server. A tag records `name, target, message, tagger, created_at`.

```bash
wt tag                                 # list
wt tag create v1.0.0                   # lightweight, at HEAD
wt tag create v1.0.0 main~2 -m "First stable release"   # annotated
wt tag delete v1.0.0
```

`wt push` also pushes local tags the server does not have (`PushTag`); `wt
fetch` downloads server tags (`ListTags`). Tag names follow branch-name rules.

Tags are not immutable by default: they can be deleted locally, and on the
server by `tree:admin`. **Tag protections** make them so:

```bash
wt protect tag set 'v*' --no-delete --no-move [--tagger alice]
```

With `no_move` an existing tag cannot be replaced; with `no_delete` it cannot
be deleted; `allowed_taggers` restricts who may create matching tags.

### Releases

A release belongs to a server tree and a tag (unique per tree):

| Field | Meaning |
|---|---|
| `tag`, `snapshot_id` | Tag and the snapshot it points to (the tag is created at `--snapshot` if missing) |
| `name`, `notes` | Title and Markdown notes |
| `draft`, `prerelease` | Flags |
| `assets` | Uploaded files (≤ 64 MiB each), stored in the object store with name, size, hash, content type |

```bash
wt release create v1.0.0 --name "1.0" --notes-file CHANGELOG.md [--draft] [--prerelease] [--snapshot ID]
wt release upload v1.0.0 dist/app.tar.gz dist/app.zip
wt release list | show v1.0.0 | edit v1.0.0 ... | delete v1.0.0
wt release download v1.0.0 [ASSET] [-o DIR]
```

Actions `release:create` / `release:delete`; events `release.created` /
`release.deleted`. Release snapshots and assets are GC roots.

---

## 9. Merge and Conflict Resolution

There is one way to combine histories: merge. No rebase, squash or
cherry-pick.

```bash
wt merge feature                     # fast-forward when possible, else three-way merge
wt merge feature --no-ff -m "Merge feature"
wt merge feature --strategy ours     # conflicting hunks resolved to the current branch
wt merge feature --strategy theirs   # conflicting hunks resolved to the source
wt merge status                      # merge in progress, conflicts left
wt merge resolve src/a.rs --ours|--theirs|--file merged.rs
wt merge resolve --all --theirs
wt merge --continue [-m MSG]         # or: wt snapshot -m MSG once markers are gone
wt merge abort                       # also: wt merge --abort
```

- The merge base is the nearest common ancestor; the algorithm is a line-based
  three-way merge (`crates/worktree-sdk/src/merge`). Non-overlapping changes,
  in different files or different regions of one file, merge cleanly and
  produce a merge snapshot with two parents (ours first).
- Conflict kinds: `content` (overlapping lines; markers written),
  `modify_delete`, `add_add`, `binary` (ours kept in the working tree).
- Markers always include the base section (diff3 style). Labels name the
  side, the branch and the short snapshot id:

```
<<<<<<< ours (main 1a2b3c4d)
    return validateToken(user.token);
||||||| base
    return checkAuth(user);
=======
    return verifyCredentials(user.email, user.password);
>>>>>>> theirs (feature 5e6f7a8b)
```

- Machine-readable state: `.wt/trees/<tree>/MERGE_STATE.json` (source branch,
  source/target tips, base, message) and `.wt/conflicts/<tree>.json` (one
  `ConflictInfo {path, kind, hunks:[{ours_start, ours_len, theirs_start,
  theirs_len}]}` per conflict).
- While a merge is in progress: switching branches, reverting and
  auto-snapshots are refused; `wt snapshot` completes the merge only when no
  conflict markers remain. Exit code `3` signals conflicts.
- `wt pull` = fetch, then fast-forward (with checkout) or merge of
  `<remote>/<branch>` into the local branch.

The server never merges content: merge requests are fast-forward only (§14).

---

## 10. License Compliance

Licenses are tracked per **path glob** as server rules on each tree
(`LicenseRule {path_glob, spdx, grant, grantee, expires_at}`), not in local
config files:

```bash
wt license list
wt license set 'vendor/openssl/**' Apache-2.0 --grant redistribute
wt license set 'src/enterprise/**' LicenseRef-Proprietary --grant read-only \
  --grantee partner-corp --expires 2027-01-01T00:00:00Z
wt license remove 'vendor/openssl/**'
wt license detect [PATH] [--apply]     # SPDX guesses from file contents
```

Rules never restrict the owning tenant's members or server admins. For
everyone else the most specific matching rule decides:

| Grant | Read | Push changes | Export |
|---|---|---|---|
| `read-only` | yes | no | no |
| `modify` | yes | yes | no |
| `redistribute` | yes | yes | yes |
| none | proprietary SPDX ids (`LicenseRef-Proprietary`, ...): no access; other licenses: full | | |

`grantee` restricts a rule to one tenant (`""` = every non-owning tenant);
`expires_at` ends it. Paths no rule matches are unrestricted. Enforcement:

- **Push:** `CheckPush` on every changed path (after authorization and review
  checks); violations are `PushDenied{kind:"license_violation"}`.
- **Pull/clone/fetch:** paths the caller may not read are returned in
  `PullResponse.stubbed_paths`, their blobs are withheld, and
  `DownloadObjects` refuses them (§13).
- **REST reads:** `451 {"code":"license_restricted"}` for content; omitted from
  listings (`restricted_count`).
- **Archives/exports:** `wt archive`, `wt git export` and the server archive
  endpoint skip paths whose rule forbids redistribution and add a generated
  `LICENSES/NOTICE` (§17).

License checks and IAM checks are independent; both must pass. Details:
[LicenseCompliance.md](./licensing/LicenseCompliance.md).

---

## 11. Ignore Patterns

Gitignore syntax everywhere (matcher: `ignore` crate).

| Source | Scope | Shared with the team |
|---|---|---|
| Hard-coded: `.wt/`, `.wt-tree/`, `.git/` | Always ignored, cannot be re-included | n/a |
| Soft defaults: `__pycache__/`, `*.pyc`, `.env`, `*.log`, `node_modules/`, `.venv/`, `dist/`, `build/`, `target/` | Ignored unless re-included with a `!pattern` | n/a |
| `.wt/ignore` | Whole worktree, root-relative | No (inside `.wt/`) |
| `<dir>/.wtignore` | That directory and below | Yes (tracked file) |
| `<tree>/.wt-tree/ignore` | That tree | No (inside `.wt-tree/`) |

Rules:

- Patterns at the root level (`.wt/ignore`, the root `.wtignore`) cannot be
  re-included by `!pattern`s in deeper `.wtignore` or `.wt-tree/ignore`
  files. Within one file the last matching pattern wins, so `!` works for that
  file's own patterns.
- Ignored and hard-coded paths are never hashed or snapshotted; the daemon
  drops their file events before debouncing.
- Compiled matchers are cached per tree.

```bash
wt ignore list                 # effective patterns
wt ignore add '*.tmp'          # appends to .wt/ignore
wt ignore check build/out.o    # prints whether a path is ignored
```

---

## 12. Large Files

There is no LFS and no tracking configuration:

- Files larger than **8 MiB** are split with FastCDC
  (`scan::chunk_boundaries`; min 256 KiB, average 1 MiB, max 4 MiB). Each
  chunk is a blob; the manifest entry's hash names a `ChunkList {size,
  chunks:[{hash, size}]}` object and `chunked = true`.
- Unchanged chunks are shared between versions and files, so editing part of a
  large file stores and transfers only the changed chunks.
- Every reader (checkout, restore, diff, merge, archive, Git export, secret
  scanning, license detection, server content endpoints) goes through
  `ObjectStore::read_entry` / `internal/content`, which reassembles chunks.
- Push uploads chunk lists and chunks; the server verifies that every chunk is
  stored before accepting a push. The maximum single object is 64 MiB.

Files are always fully materialized on checkout; see [Roadmap](#roadmap) for
lazy loading.

---

## 13. Shallow History and Restricted Paths

### Shallow sync

```bash
wt clone http://localhost:7420 acme/web --depth 20
wt fetch --depth 50
wt fetch --deepen 100
wt pull --depth 10
```

`--depth` sets `PullRequest.depth`. The boundary snapshot ids are written to
`.wt/trees/<tree>/shallow`. `wt log` stops there and marks "(shallow
boundary)"; a merge-base search that reaches the boundary fails with an error
suggesting `wt fetch --deepen N`. The server always keeps full history.

### Restricted (stubbed) paths

When the server withholds paths the caller may not read (license rules or
`tree:read` path-deny policies), they are listed in
`.wt/trees/<tree>/stubbed`, not written on checkout, and reported by `wt
status` as "restricted". Snapshots built from such a working tree keep the
withheld entries unchanged.

There is no partial *tree* sync: `wt clone` fetches exactly one server tree
into a new worktree; other trees are cloned separately.

---

## 14. Branch Protection and Merge Requests

Protections are server rules per tree, matched by branch-name glob
(`path.Match`, e.g. `main`, `release/*`), managed with `wt protect` or the
REST API (`/protections`), not with config files:

```bash
wt protect set main --no-direct-push --approvals 2 --no-delete \
  --require-ci --check build --check test --require-signed --require-linear --block-on-todos
wt protect set 'release/*' --no-direct-push --approvals 1 --pusher release-bot
wt protect list
wt protect remove 'release/*'
```

| Field | Effect |
|---|---|
| `no_direct_push` | Pushes to the branch are rejected (changes land via merge requests); `allowed_pushers` are exempt |
| `required_approvals` | Approvals of the current source tip needed to merge an MR |
| `no_delete` | Branch cannot be deleted |
| `require_ci_pass`, `required_checks` | Every listed CI context (or every reported one if the list is empty) must be `success` on the source tip |
| `require_signed` | Every new snapshot must carry a verified signature (§19) |
| `require_linear` | MR must fast-forward; pushes containing merge snapshots are rejected |
| `block_on_todos` | Open blocking TODOs targeting the source branch block the merge |

Merge requests are server objects (per-tree number):

```bash
wt mr create --source feature --target main --title "OAuth2" [--description D]
wt mr list [--state open|merged|closed|all]
wt mr show 4 [--diff]
wt mr review 4 --approve|--request-changes|--comment -m "..."
wt mr comment 4 -m "nit" --path src/a.rs --line 12
wt mr checks 4                 # mergeable?, blockers, approvals (required/current/stale), CI, TODOs
wt mr merge 4 | close 4
wt ci report --snapshot ID --context ci/build --state success [--url U]
wt ci status [--branch main | --snapshot ID]
```

Approvals record the source tip they approved; a new push to the source makes
them stale. A `changes_requested` review blocks until that reviewer approves
the current tip. Authors cannot approve their own MR. Blocked merges answer
`409 {"code":"merge_blocked"}`. Merging fast-forwards the target to the
source tip; a diverged target answers `409` (merge locally, push, retry).

When several rules match a branch they are combined into their most
restrictive form. Protection is never bypassed, not even by server admins.
Protection is not inherited between trees: each server tree has its own rules.

---

## 15. Reflog

Each tree has an append-only reflog, `.wt/trees/<tree>/reflog.jsonl`, one JSON
`ReflogEntry` per line:

```json
{"at":"2026-10-25T10:30:00Z","tree":"web","branch":"main","action":"snapshot","old":"5e6f...","new":"9a0b...","message":"Add rate limiting"}
```

Actions: `init`, `snapshot`, `auto-snapshot`, `switch`, `branch.create`,
`branch.archive`, `branch.restore`, `merge`, `fast-forward`, `revert`,
`restore`, `pull`, `push`, `tag.create`, `tag.delete`.

```bash
wt reflog [--branch main] [-n 20]
wt show main@{2}          # revisions @{N} and BRANCH@{N} resolve through the reflog
wt reflog expire [--older-than 90d] [--keep 1000]
```

Default retention is 90 days / 1000 entries per tree, applied by the daemon's
maintenance task (every 6 h) and by `wt reflog expire`. Malformed lines (torn
writes) are skipped when reading. The reflog is local; the server keeps an
audit log of server-side actions instead (`wt audit`).

---

## 16. Revert

```bash
wt revert 5e6f7a8b                       # undo one snapshot as a new snapshot
wt revert 5e6f7a8b --message "Revert auth changes"
wt revert 7c8d9e0f --parent 1            # merge snapshots need --parent N
```

The revert snapshot records `reverts = <id>`; its default message is
`Revert "<first line>"` followed by `This reverts snapshot <id>.`. If later
snapshots touched the same files, the inverse change is applied with a
three-way merge; conflicts abort the revert without changing anything. The
working tree must be clean. `wt show` prints `Reverts: <id>`.

---

## 17. Archives

```bash
wt archive -o dist/web.tar.gz                 # HEAD of the current tree
wt archive v1.0.0 -o web-1.0.0.zip            # format from the extension
wt archive main~3 -o out.bin --format tar.gz
wt archive v1.0.0 -o x.zip --no-license       # skip license processing
wt archive v1.0.0 -o x.zip --include-restricted
```

Archives contain the files of one snapshot of one tree, with no VCS metadata.
When the tree has a remote, its license rules (cached locally from the server)
are applied: a generated `LICENSES/NOTICE` lists SPDX ids per path, and paths
whose rule forbids redistribution are skipped with a warning unless
`--include-restricted`. The server offers the same as
`GET /api/v1/trees/{tenant}/{tree}/archive/{snapshot}?format=tar.gz|zip`
(omits paths the caller may not redistribute — license `CheckExport` — or
read, and is audited as `tree.export`).

---

## 18. Diff

```bash
wt diff                         # working tree vs HEAD
wt diff main                    # working tree vs a revision
wt diff main feature            # two revisions
wt diff HEAD~2 HEAD -- src/     # restricted to paths
wt diff --stat | --name-only | --name-status
wt diff --find-renames[=PCT]    # "renamed: a -> b" (exact and similarity renames)
wt diff --word-diff
wt --json diff                  # FileDiff objects
```

Output is unified diff (`--- a/P`, `+++ b/P`, 3 lines of context). Binary
files (NUL in the first 8 KiB or invalid UTF-8) are reported, not diffed.
Revisions everywhere accept `HEAD`, `@`, `HEAD~N`, `^N` (N-th parent, chainable
as in `main~2^2`), branch names, `origin/<branch>`, tag names, id prefixes of
at least 4 characters, and `@{N}` / `BRANCH@{N}`.

---

## 19. Snapshot Signing

- Keys are Ed25519, stored at `$WT_HOME/keys/<name>.key` (hex seed, mode 0600)
  and `<name>.pub`.
- `[signing] key = "<name>"`, `auto = true` signs every locally created
  snapshot; `wt snapshot --sign` signs one.
- The signature covers `signing::signed_payload`: the protobuf
  `SnapshotRecord` with `signature`, `signing_key_id`, `tree_id` and
  `branch_name` cleared.
- `wt identity generate [NAME] | list | show [NAME] | register [NAME] |
  revoke ID | verify [SNAPSHOT]`; `register` uploads the public key
  (`POST /api/v1/auth/keys`) and records the server key id.
- On push the server verifies every signed snapshot against the pusher's
  unrevoked keys; invalid signatures are rejected (`signature_required`),
  valid ones are stored with `verified = true`. Branches with
  `require_signed` reject unsigned snapshots.
- `wt log --show-signature` shows verification status.

See [Security.md](./security/Security.md).

---

## 20. Hooks and Secret Scanning

**Hooks.** Executables `.wt/hooks/<phase>` (all trees) and
`<tree>/.wt-tree/hooks/<phase>` run for `pre-snapshot` and `post-snapshot`
(on Windows also `<phase>.cmd`, `.bat`, `.ps1`). Environment: `WT_HOOK`,
`WT_TREE`, `WT_TREE_PATH`, `WT_BRANCH`, `WT_ROOT`, and `WT_SNAPSHOT` for post
hooks; the working directory is the tree root; `[hooks] timeout_secs`
(default 60). A failing pre hook aborts the snapshot; post hook failures are
printed to stderr. `wt snapshot --no-verify` skips hooks.

**Secret scanning.** Before a snapshot, added and modified files are scanned
with built-in rules (private keys, AWS keys, GitHub, GitLab, Slack, Stripe,
Google, npm, OpenAI and Anthropic tokens, JWTs, and high-entropy values
assigned to password/secret/token variables). A finding aborts the snapshot with a
redacted report unless `[secret_scanning] block_on_match = false` (warn only)
or `wt snapshot --no-scan`. Lines containing `wt:allow-secret` are exempt.
`patterns = [{name, regex}]` adds rules; `exclude` globs skip paths.

---

## 21. Git Compatibility

```bash
wt git import ../repo [DEST]                       # new worktree from a Git repo or URL
wt git import https://example.com/r.git --branches main,dev --tree web
wt git import ../repo --all-branches
wt git export ../out.git [--rev main]              # write history into a Git repo (bare if *.git)
wt git push https://example.com/r.git --branch main [--force]
wt git remote add github https://example.com/r.git # stored as [git_remote.github] url
wt git remote list | remove github
```

| Git | W0rkTree |
|---|---|
| Commit | Snapshot (deterministic id derived from the commit id) |
| Branch / tag | Branch / tag |
| Author and date | Preserved |
| Merge parents | Preserved |
| Root `.gitignore` | Patterns copied into `.wt/ignore` |

The snapshot-id ↔ commit-sha mapping is kept in `.wt/git-map.json`, so repeated
exports are incremental. Content round-trips byte for byte; ids differ
(SHA-1 vs UUID). Git export applies license processing like `wt archive`.

---

## 22. Comparison with Git

| Aspect | Git | W0rkTree |
|---|---|---|
| Units | One repository per `.git` | Worktree with one or more independently versioned trees |
| Snapshot id | SHA-1 of commit content | UUID v4; content addressing is for files and manifests (BLAKE3) |
| Staging | Index + `git add` | None; `wt snapshot` records the working state |
| Branch delete | Hard delete | `wt branch archive` (soft, restorable) |
| History rewriting | rebase, reset, force push | None; server requires fast-forward pushes |
| Server | Hosting is separate | Built-in multi-tenant server with IAM, MRs, CI gates, TODOs, releases |
| WIP visibility | None until push | Staged snapshots |
| Large files | Git LFS | Automatic FastCDC chunking above 8 MiB |
| Access control | Hosting platform | Server-enforced roles, grants, policies (path-level), declarative TOML |
| Licensing | None | Per-path SPDX rules enforced on push, pull and export |
| Automation | Manual | Optional daemon: auto-snapshot, auto-stage, background fetch |
| Recovery | reflog (local) | Per-tree reflog (local), branch archive, server audit log, GC grace period |
| Interop | n/a | Git import/export/push bridge |

---

## 23. Design Principles

1. **One job per command.** `wt switch` switches branches, `wt restore`
   restores files, `wt branch create` creates branches. `wt switch -c` is the
   one convenience combination.
2. **Plain terminology.** Snapshot, tree, stage, switch, restore.
3. **Automatic when wanted.** The daemon snapshots and stages watched
   worktrees; without it everything is explicit and still works.
4. **Append-only shared history.** Pushes must descend from the server tip;
   mistakes are fixed by new snapshots (`wt revert`).
5. **Soft deletes where it matters.** Local branches are archived, not
   deleted. Objects that become unreachable on the server are deleted by GC
   only after a grace period (default 720 h). Note that `wt restore` and
   `wt switch --force` discard uncommitted changes, and tags and server
   branches can be deleted unless protected.
6. **Server-side enforcement.** The client is never trusted for access,
   protection, license or quota decisions.
7. **Git as a bridge, not a dependency.**

---

## 24. Command Reference Summary

Global flags: `--json`, `-C <path>`, `--tree <name>`, `--no-color`,
`--server <url>` (env `WT_SERVER`). Exit codes: `0` ok, `1` error, `2` usage,
`3` merge conflicts. Full reference: `docs/cli-reference.md`.

| Area | Commands |
|---|---|
| Local | `init [PATH] [--name N] [--branch B] [--watch]`, `status [--team]`, `snapshot -m MSG [--allow-empty] [--no-scan] [--no-verify] [--sign] [--depend ...]`, `log [-n N] [REV] [--oneline] [--show-signature]`, `show REV [--stat]`, `diff`, `branch list/create/archive/restore`, `switch NAME [-c] [--force]`, `merge`, `revert`, `restore [PATH...] [--from REV]`, `tag`, `tree list/add/remove/validate/init-config`, `reflog`, `ignore list/add/check`, `config list/get/set`, `archive` |
| Sync | `remote list/add/remove`, `login/logout/whoami/register/password-reset`, `push [--remote R] [--branch B]`, `fetch [--depth N] [--deepen N]`, `pull [--depth N]`, `clone URL TENANT/TREE [DEST] [--depth N] [--watch]`, `stage [--discard]`, `unstage` |
| Daemon | `daemon start [--foreground] / stop / restart / status / watch / unwatch / pause / resume / snapshot / logs [-n N] [--follow]` |
| Server | `tenant`, `trees`, `mr`, `protect`, `license`, `audit`, `admin`, `search`, `events`, `team`, `role`, `policy`, `grant`, `sa`, `token`, `session`, `mfa`, `invite`, `iam`, `sso` |
| Coordination | `todo`, `deps`, `release`, `ci` |
| Access & identity | `access validate/plan/apply/pull`, `identity generate/list/show/register/revoke/verify` |
| Git | `git import / export / push / remote` |

---

## Implementation Status

| Feature | Status | Notes |
|---|---|---|
| Local engine: init, trees, snapshots, branches, tags, merge, revert, restore, diff, reflog, archive | Built | `crates/worktree-sdk/src/repo` |
| Nested trees with `.wt-tree/` markers | Built | Registration is local (`.wt/trees/`), not shared by push/clone |
| Object store, BLAKE3, flat protobuf manifests | Built | |
| gRPC sync (push/fetch/pull/clone/stage), REST API, admin panel | Built | |
| IAM: tenants, teams, roles, policies, grants, service accounts, MFA, SSO | Built | v0.4 |
| Background daemon: watch, auto-snapshot, auto-stage | Built | |
| Daemon background fetch (`WatchBranches`), IPC token, maintenance, shutdown snapshot | Built | v0.5 §13.11 |
| Chunked large files, shallow sync, stubbed paths | Built | v0.5 §13.2 |
| Path-level read/write enforcement, license grantee/expiry | Built | v0.5 §13.1 |
| Reviews, CI statuses, merge readiness, linked groups, tag protections | Built | v0.5 §13.4 |
| Snapshot dependencies, TODOs, releases | Built | v0.5 §13.5 |
| Snapshot signing | Built | v0.5 §13.6 |
| Declarative access files, tenant status, lockout guard | Built | v0.5 §13.7 |
| Git import from URLs, Git push/remotes, license-aware archives, `wt license detect` | Built | v0.5 §13.8 |
| Config hierarchy, hooks, secret-scanning config, tree validation | Built | v0.5 §13.9 |
| Merge strategies/resolve, revert `--parent`, rename/word diff, reflog revisions and expiry | Built | v0.5 §13.10 |
| GC, quotas, rate limiting, TLS, health checks, metrics | Built | v0.5 §13.3 |
| Lazy loading, partial worktree sync, automatic remote merges | Roadmap | see below |

## Roadmap

Described in earlier versions of this specification but neither built nor part
of v0.5:

- **Virtual filesystem / lazy loading** (FUSE, FUSE-T, ProjFS) and on-demand
  history fetching past a shallow boundary; configurable large-file threshold
  and chunk size (`[large_files]`, `[storage]`).
- **Multi-tree clone / partial tree sync** (`wt init --from URL --trees a,b`,
  stub trees, `wt tree sync`), and sharing a worktree's tree layout through
  the server.
- **Automatic merging by the daemon** of non-conflicting remote changes.
- **A single `wt sync` command** combining stage, push and fetch.
- **Branch strategies** (`branch_strategy = feature-branch | trunk-based |
  release-branch`) and server enforcement of them.
- **Linked branches declared in config** and automatic linked groups created
  from `--linked` dependencies; `wt branch merge-linked`.
- **Static tree- and branch-level dependencies** (`[[dependency]]` in
  `.wt-tree/config.toml`, version constraints, `.wt-tree/branches/*.toml`),
  `wt depend add`, `wt branch status`, TODO-branch auto-creation on claim,
  `wt todo link`, e-mail notifications.
- **Registered paths** (`[[registered_path]]`) and sensitivity levels;
  path-level rules today use glob resources in policies and license rules.
- **Config-file-defined protections, licenses and tenant access**
  (`[branch_protection]`, `[licenses]`, `[tenant_access]` in config.toml) and
  a permission/threshold "ceiling" model between config levels.
- **Server-synced reflog** and cross-machine reflog recovery; `[reflog]`
  config section.
- **Signed tags**, copy detection in diff, `[diff]` config section, `wt diff
  --patch` as a separate mode.
- **Live bidirectional Git mirroring** and serving trees over the Git
  protocol to Git clients.
- **Verified e-mail addresses** for accounts.
- **Environment-variable overrides for arbitrary config keys** (`WT_*` per
  key) and a system-wide config level.
- **Platform-native data directories** (`%APPDATA%`, `~/.local/share`): all
  repository data lives in the worktree's `.wt/`.

## References

- `docs/ENGINEERING_PLAN.md` — §1-§13 contracts
- `crates/worktree-sdk/src/repo/mod.rs` — authoritative `.wt/` layout
- `crates/worktree-sdk/src/model.rs` — Snapshot, Manifest, Dependency, ReflogEntry, merge types
- `crates/worktree-sdk/src/config.rs` — `WorktreeConfig`, `wt_home()`
- `crates/worktree-sdk/src/store.rs`, `src/scan/cdc.rs` — object store, chunking
- `crates/worktree-sdk/src/repo/{trees,refs,history,snapshotting,merging,diffing,reflog,ignore_rules,working}.rs`
- `crates/worktree-sdk/src/{hooks,signing}.rs`, `src/scan/secrets.rs`
- `crates/worktree-sdk/src/remote/` — push, fetch, clone, stage, credentials
- `crates/worktree-cli/src/cli.rs`, `src/commands/` — CLI surface
- `crates/worktree-bg/` — daemon
- `crates/worktree-git/` — Git bridge
- `crates/worktree-protocol/proto/sync.proto` — wire contract
- `services/server-go/internal/domain/{types,iam,v05}.go` — server types
- `services/server-go/internal/{syncsvc,authz,review,todo,release,signing,license,pathacl,gc,limits}/`
