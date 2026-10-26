# W0rkTree Config-as-Code Specification

> **Version:** 0.8 — describes config-as-code as built (`docs/ENGINEERING_PLAN.md` §16.3).
> Features described but not built are listed under [Roadmap](#14-roadmap).

## Table of Contents

- [1. Overview](#1-overview)
- [2. Files](#2-files)
- [3. `branches.toml`](#3-branchestoml)
- [4. `protections.toml`](#4-protectionstoml)
- [5. `config.toml`](#5-configtoml)
- [6. `access/`](#6-access)
- [7. Diagnostics](#7-diagnostics)
- [8. Validation on push](#8-validation-on-push)
- [9. Application after the push](#9-application-after-the-push)
- [10. REST API](#10-rest-api)
- [11. CLI](#11-cli)
- [12. Admin panel](#12-admin-panel)
- [13. Implementation status](#13-implementation-status)
- [14. Roadmap](#14-roadmap)
- [15. References](#15-references)

---

## 1. Overview

A tree's shared `.wt-tree/` files ([DotWtTree.md](./DotWtTree.md) §10)
are versioned with its code. Config-as-code makes some of them
**authoritative server configuration**: when a push (or a server-side
merge) moves the tree's **default branch** and changes one of them, the
server

1. validates the files of the new tip — an invalid configuration rejects
   the push with located errors (`file:line:col CODE message`);
2. after the branch moved, applies them: branch descriptions and owners,
   branch protections, and the tree's deny policies;
3. records the applied state (and the outcome of the last attempt) in the
   `tree_config` document of the tree, publishes `config.applied` and
   audits the change.

Pushes to other branches are not checked: a configuration takes effect only
once it reaches the default branch (directly or through a merge request).

The Rust validator (`worktree_protocol::config::as_code`, used by `wt`) and
the Go server (`internal/treeconfig`) implement the same rules and report
the same codes at the same positions; both run the conformance cases in
`crates/worktree-protocol/testdata/config_as_code/`.

---

## 2. Files

Tree-relative paths (`.wt-tree/` is at the root of the tree; a nested
tree's own directory):

| File | Effect on the server | Applied when |
|---|---|---|
| `.wt-tree/branches.toml` | `Branch.description`, `Branch.owner` ([§3](#3-branchestoml)) | the file changed |
| `.wt-tree/protections.toml` | config-managed branch protections ([§4](#4-protectionstoml)) | the file changed |
| `.wt-tree/config.toml` | none (client settings); validated only ([§5](#5-configtoml)) | — |
| `.wt-tree/access/policies.toml` | the tree's access document via accesscfg ([§6](#6-access)) | the file (or `roles.toml`) changed; other files under `access/` are ignored |
| `.wt-tree/access/roles.toml` | not allowed (E2013) | — |

`branches.toml` and `protections.toml` are shared `.wt-tree/` files like
`config.toml`: the SDK snapshots them and the server accepts them in
manifests (`is_shared_tree_config_path`, `IsSharedTreeConfigPath`).
Removing a file is a change: removing `protections.toml` deletes the
config-managed protections, removing `access/policies.toml` deletes the
tree's config-managed policies, removing `branches.toml` clears the
metadata it had set.

Files larger than 256 KiB are refused (E3009). All files are TOML 1.0.

---

## 3. `branches.toml`

```toml
[[branch]]
name = "main"                         # required
description = "Production web app"    # optional, at most 1000 characters
owner = "team:platform"               # optional: user:NAME or team:SLUG
```

`branch = [{ name = "a" }, ...]` (an array of inline tables) is accepted
too. Rules:

- top-level keys: only `branch`; table keys: `name`, `description`,
  `owner` (others: E3002);
- `name` is required (E3003) and must be a valid branch name — no
  whitespace, control characters or `~^:?*[\`, no empty, `.` or `..`
  components, at most 255 bytes (E3004); each branch at most once (E3006);
- `owner` is `user:` + 1-39 of `[a-z0-9-]` or `team:` + 1-63 of
  `[a-z0-9-]`, starting with a letter or digit (E3008); the server also
  requires the account, or the team of the tree's tenant, to exist (E3008
  "does not exist in this tenant");
- at most 500 branches (E3009).

Strings are trimmed. A declared branch that does not exist yet is reported
as warning W3102; its metadata is applied as soon as it is created (push of
a new branch, `POST {tree}/branches`).

Declared branches are **config-managed**: `PATCH {tree}/branches/{name}`
answers 409 `config_managed`, and `wt branch describe|owner` explains that
the file must be changed instead.

## 4. `protections.toml`

```toml
[[protection]]
pattern = "release/*"            # required
no_direct_push = true
required_approvals = 1           # 0-100
allowed_pushers = ["alice"]      # usernames (a single string is accepted)
no_delete = true
require_ci_pass = true
required_checks = ["ci/build"]   # at most 50, 1-255 bytes each
require_signed = false
require_linear = false
block_on_todos = false
merge_methods = ["squash"]       # fast-forward | merge | squash | rebase
no_force_push = true
require_owner_approval = false
```

The fields are those of the REST API's `BranchProtection` (absent booleans
are false). Rules: unknown keys E3002; `pattern` required (E3003), a
`path.Match` glob with `**` segments, no empty segment, at most 512 bytes
(E3005), each pattern at most once (E3006); `required_approvals`,
`allowed_pushers`, `required_checks` and `merge_methods` out of range or
unknown: E3007; more than 100 protections, 100 pushers or 50 checks: E3009.
Duplicates in lists are dropped; merge methods are kept in preference
order.

**Config-managed protections.** The `tree_config` document remembers the
patterns the configuration created (`managed_patterns`). On apply:

- a declared pattern that is managed (or new) is created or updated;
- a declared pattern that already exists **and was set through the REST
  API** (`wt protect`) is left untouched — warning W3101; delete the API
  rule to let the file manage it;
- a managed pattern that is no longer declared is deleted.

Rules set through the API therefore never change because of the file. A
managed rule edited through the API is overwritten by the next apply.

## 5. `config.toml`

The tree-level client settings of [DotWtTree.md](./DotWtTree.md) §3 are
validated so a broken file cannot reach the default branch:

| Section | Keys (kind) |
|---|---|
| `[auto_snapshot]` | `enabled` (bool), `inactivity_secs` / `inactivity_timeout_secs` (integer ≥ 0), `max_changed_files` (integer ≥ 0), `max_changed_bytes` (integer ≥ 0 or a size such as `"50MB"`), `message_format` (string), `on_branch_switch` (`refuse` / `snapshot` / `stash`) |
| `[secret_scanning]` | `enabled`, `block_on_match` (bool), `exclude` (array of strings), `patterns` (array of `{ name, regex }` tables, both non-empty) |
| `[hooks]` | `enabled` (bool), `timeout_secs` (integer ≥ 0) |

Wrong types are E3002, out-of-range values E3007, a pattern without name or
regex E3003. Other sections (read only at the worktree level) are warning
W3103, unknown keys warning W3104; the legacy identity keys `name` / `id`
are ignored. Nothing in `config.toml` is applied on the server.

## 6. `access/`

`.wt-tree/access/policies.toml` is the tree's declarative access document
([DeclarativeAccess.md](../iam/DeclarativeAccess.md)): deny statements
only, every resource inside the tree. The server compiles it to the
canonical JSON document (aliases translated, warning W2101) and applies it
with `accesscfg` exactly like `wt access apply --tree-doc`; policies are stored
as `<tree>/<name>`, config-managed.

Changing the access document (adding, editing or removing `policies.toml`)
requires the pusher to hold **`iam:admin`** in the tree's tenant. Otherwise
the push is rejected with E3010 and a hint ("ask a tenant admin to push
this change"). The document is checked with a dry run before the push is
accepted: structural problems use the E20xx codes of the access compiler
(E2001 syntax, E2002 structure, E2011 date-times), refusals by accesscfg
(unknown principals, privilege escalation, invalid statements) are E3011,
positioned at the `[[policy]]` / `[[policy.statement]]` header they refer
to. Locally, `wt` additionally runs the full declarative validation (E20xx
codes, e.g. E2008 for an allow statement).

## 7. Diagnostics

```json
{"code": "E3008", "severity": "error", "file": ".wt-tree/branches.toml",
 "line": 3, "col": 1, "message": "owner user:ghost does not exist in this tenant"}
```

Lines and columns are 1-based; columns count characters. A diagnostic
points at the key whose value is wrong, at the array element, or at the
`[[table]]` header of an entry that lacks a field; `0:0` means "no
position" (E3010). Syntax errors are positioned by each parser in its own
way (the line is usually the same). Text form: `file:line:col: CODE
message`.

| Code | Severity | Meaning | Who |
|---|---|---|---|
| E3001 | error | not valid TOML | both |
| E3002 | error | unknown key (branches, protections) or wrong type | both |
| E3003 | error | required field missing or empty | both |
| E3004 | error | invalid branch name | both |
| E3005 | error | invalid protection pattern | both |
| E3006 | error | duplicate branch / pattern | both |
| E3007 | error | value out of range or not allowed | both |
| E3008 | error | owner syntax; (server) owner does not exist | both |
| E3009 | error | too many entries, text or file too long | both |
| E3010 | error | `access/` changed without iam:admin | server |
| E3011 | error | access document refused by accesscfg | server |
| W3101 | warning | protection pattern managed through the API; entry ignored | server |
| W3102 | warning | declared branch does not exist yet | server |
| W3103 | warning | `config.toml` section not read at tree level | both |
| W3104 | warning | unknown key in `config.toml` | both |
| E20xx / W21xx | — | access files ([DeclarativeAccess.md](../iam/DeclarativeAccess.md)) | both |

## 8. Validation on push

`treeconfig.Service` is a `PushChecker` (after quotas, before secret
scanning and signatures). For a push to the default branch whose changed
paths include a config-as-code file it reads the files of the new tip,
validates them, checks owners and — when `access/` changed — iam:admin and
the access dry run. Errors reject the push: the rejection is
`access_denied` with the reason `invalid .wt-tree configuration: <up to 10
diagnostics>`, the attempt is recorded in `tree_config.last_attempt`
(accepted false, errors) and audited as `config.rejected`.

Server-side merges and suggestion commits do not run push checkers; their
configuration is validated when it is applied ([§9](#9-application-after-the-push)):
an invalid merged configuration is recorded as a rejected attempt and not
applied (the merge itself stands).

`wt push` validates the files of the pushed tip locally first when pushing
the tree's default branch and refuses to contact the server on errors.

## 9. Application after the push

The service subscribes to `branch.pushed` (published by pushes and
merges). For the default branch it compares the config files of `old_tip`
and `new_tip`; when one changed it validates again and applies, in this
order: `branches.toml`, `protections.toml`, `access/`. The principal is the
pusher seen by the push check, else the account that merged (the event's
actor). Applies are serialized per server.

The `tree_config` document (DocStore collection `tree_config`, key: tree
id):

```json
{
  "tree_id": "…", "default_branch": "main",
  "applied": {
    "snapshot_id": "…", "branch": "main", "applied_by": "alice", "applied_at": "…",
    "files": [{"path": ".wt-tree/branches.toml", "hash": "<blake3>", "size": 120}],
    "branches": [{"name": "main", "description": "…", "owner": "team:platform"}],
    "protections": [BranchProtection], "managed_patterns": ["release/*"],
    "access_hash": "<blake3 of the applied access document>",
    "changes": ["protection release/*: create", "branch main: description set, owner team:platform"],
    "warnings": [Diagnostic]
  },
  "last_attempt": {"snapshot_id": "…", "branch": "main", "by": "bob", "at": "…",
                   "accepted": false, "errors": [Diagnostic], "warnings": []}
}
```

Event `config.applied` (payload `snapshot_id`, `branch`, `files`,
`changes`, `warnings` count) and audit `config.apply` follow every apply;
`branch.update` audits REST metadata changes. The document is deleted when
the tree is deleted (`tree.deleted`). Branch metadata is written without
touching the branch tip (`meta.BranchMetaUpdater`).

## 10. REST API

| Method and path | Action | Description |
|---|---|---|
| `GET {tree}/config` | tree:read | the `tree_config` state; `applied` and `last_attempt` are null without config-as-code |
| `POST {tree}/config/validate` | tree:write | `{"files": {path: content}}` → `{valid, diagnostics, branches, protections, access_changes}`; validates as if pushed, changes nothing (access files: E3010 without iam:admin, dry-run plan in `access_changes`); non-config paths are 400 |
| `PATCH {tree}/branches/{name}` | tree:write (owner: tree:admin) | `{"description"?, "owner"?}` — absent unchanged, `""` clears; owner must exist; 409 `config_managed` for declared branches |

`GET {tree}/branches` returns `description` and `owner` (omitted when
empty). Without the service the routes answer 501.

## 11. CLI

| Command | Description |
|---|---|
| `wt trees config show [TENANT/TREE]` | applied configuration, declared branches and protections (API-managed ones marked), access hash, changes, warnings, and the errors of a rejected last attempt |
| `wt trees config validate [PATH] [--online] [--remote-tree T/NAME]` | validate the files of a tree directory (default: the current tree) locally; `--online` also asks the server (owners, iam:admin, access dry run); exit 1 on errors |
| `wt branch describe NAME TEXT` | set a branch's description on the origin tree (`""` clears) |
| `wt branch owner NAME user:X\|team:Y` / `--clear` | set or remove a branch's owner (tree:admin) |
| `wt status` | lists problems of the working copy's config files (`config_problems` in `--json`) |
| `wt push` | refuses an invalid configuration on the default branch before contacting the server |

`wt trees branches` shows the owner and description columns.

## 12. Admin panel

The tree's **Config** tab (`pages/tree/TreeConfigTab.tsx`) shows the
rejected last attempt with its located errors, the applied configuration
(source snapshot link, files, declared branches, protections with
"config" / "API (ignored)", access document, changes, warnings) and a
form that validates pasted files with `POST {tree}/config/validate`. The
**Branches** tab shows owners and descriptions and edits them (owner only
for tree admins; config-managed branches explain that the file must
change).

## 13. Implementation status

| Feature | Status |
|---|---|
| Server-side validation of pushed `config.toml`, `branches.toml`, `protections.toml`, `access/**` with located errors | Built (v0.8 §16.3) |
| Shared codes and positions (Rust and Go conformance cases) | Built |
| Branch description / owner from `branches.toml`, applied to branches created later | Built |
| Config-managed protections that never clobber API rules | Built |
| Tree access documents applied on push (iam:admin required) | Built |
| Validation and application of configurations merged through merge requests | Built (applied after the merge; invalid ones recorded, not applied) |
| `tree_config` state, `config.applied`, audit | Built |
| `GET {tree}/config`, `POST {tree}/config/validate`, `PATCH {tree}/branches/{name}` | Built |
| `wt trees config show/validate`, `wt branch describe/owner`, checks in `wt status` / `wt push` | Built |
| Admin Config tab, branch metadata in the Branches tab | Built |

## 14. Roadmap

- Rejecting a merge request whose source branch carries an invalid
  configuration before it is merged (today the merge succeeds and the
  configuration is recorded as rejected).
- Tag protections, licenses, webhooks and path owners from `.wt-tree/`
  files.
- Tenant-level config-as-code (a tenant configuration tree).
- Locating access refusals at the exact statement field (they point at the
  policy or statement header).

## 15. References

- `services/server-go/internal/treeconfig/` — `toml.go` (positioned TOML parser), `validate.go`, `diag.go`, `treeconfig.go` (push check, apply, state)
- `services/server-go/internal/httpapi/treeconfig.go` — REST routes; `openapi.yaml`
- `services/server-go/internal/meta/branchmeta.go` — `UpdateBranchMeta`
- `services/server-go/internal/syncsvc/convert.go` — shared `.wt-tree/` paths
- `crates/worktree-protocol/src/config/{as_code,branches,protections}.rs` — Rust validator and parsers
- `crates/worktree-protocol/testdata/config_as_code/` — conformance cases
- `crates/worktree-sdk/src/model.rs` — `is_shared_tree_config_path`
- `crates/worktree-cli/src/commands/config_as_code.rs` — CLI
- `apps/admin/src/pages/tree/{TreeConfigTab,BranchesTab}.tsx`, `treeConfigApi.ts`
- `crates/worktree-e2e/tests/e2e/config_as_code.rs` — end-to-end scenario
