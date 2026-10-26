# License Compliance Specification

## Table of Contents

1. [Overview](#1-overview)
2. [License rules](#2-license-rules)
3. [Path globs and specificity](#3-path-globs-and-specificity)
4. [Who is restricted](#4-who-is-restricted)
5. [Grants, grantees and expiry](#5-grants-grantees-and-expiry)
6. [SPDX expressions](#6-spdx-expressions)
7. [Enforcement points](#7-enforcement-points)
8. [Exports and the NOTICE file](#8-exports-and-the-notice-file)
9. [License detection](#9-license-detection)
10. [Errors](#10-errors)
11. [Audit](#11-audit)
12. [License compliance in the access stack](#12-license-compliance-in-the-access-stack)
13. [Implementation status](#13-implementation-status)
14. [Roadmap](#14-roadmap)
15. [References](#15-references)

---

## 1. Overview

A tree can assign an SPDX license to paths and state what **other tenants**
may do with those files. The available rights are read, write (push) and
export. The server enforces these rules on every read, push and export,
**in addition to IAM**: an operation must be allowed by IAM **and** by the
license rule.

The rules exist to cover cases like these:

- an open-source project with proprietary modules;
- a partner who may read but not take code;
- a contractor who may change code but not export it.

License metadata lives on the server, per tree, in `meta.db`. It is not
stored in worktree config files.

## 2. License rules

`domain.LicenseRule`:

| Field | Meaning |
|---|---|
| `tree_id` | Tree the rule belongs to |
| `path_glob` | Tree-relative glob (§3). There is one rule per glob per tree, and `PUT` replaces it. |
| `spdx` | SPDX license expression (§6) |
| `grant` | `read-only`, `modify`, `redistribute` or `""` (license default) |
| `grantee` | Tenant slug the rule applies to. `""` means every non-owning tenant. (v0.5) |
| `expires_at` | Optional RFC 3339 time; the rule stops applying afterwards (v0.5) |

You manage rules through REST or the CLI:

```
GET    /api/v1/trees/{t}/{tree}/licenses              (tree:read)
PUT    /api/v1/trees/{t}/{tree}/licenses  LicenseRule (tree:admin; validated)
DELETE /api/v1/trees/{t}/{tree}/licenses?path_glob=   (tree:admin)

wt license list [TREE]
wt license set [TREE] GLOB SPDX [--grant read-only|modify|redistribute|license-default] [--grantee TENANT] [--expires RFC3339]
wt license remove [TREE] GLOB
wt license detect [PATH] [--apply]
```

`wt license set` defaults to `--grant read-only`. `--grant license-default`
sends the `""` grant, which falls back to the license default (also available
through the REST API and the admin panel). `license.Validate` checks the glob, the grant value and the SPDX
syntax, and the v0.5 handlers also check `grantee` and `expires_at`. Errors
answer `400`.

A tree with no rules, and every path that no rule matches, is unrestricted.
IAM alone governs those paths.

## 3. Path globs and specificity

Globs are '/'-separated and anchored at the tree root. A `\` is accepted as
`/`, and a leading `/` or `./` is ignored.

| Syntax | Matches |
|---|---|
| `**` (whole segment) | zero or more segments: `**/*.rs` matches `a.rs` and `src/x/a.rs` |
| `*` | any run of characters within one segment |
| `?` | exactly one character within one segment |
| `[...]` | a character class, as in Go `path.Match` |
| anything else | itself (there are no escapes) |

Globs follow two further rules:

- **Directories cover their contents.** A pattern that matches a directory
  applies to everything beneath it, so `vendor`, `vendor/` and `vendor/**`
  are equivalent.
- **No implicit depth.** A pattern without a slash is not matched at every
  depth, unlike `.gitignore`: `*.rs` covers top-level files only.

When several rules match a path, **only the most specific one applies**. Rules
are compared in this order:

1. longest literal prefix (characters before the first wildcard; the `/`
   before a trailing `**` segment does not count);
2. fewest wildcards;
3. most literal characters;
4. lexically smallest normalized pattern.

So an exact file beats its directory, a deeper directory beats a shallower
one, and `src/*.rs` beats `src/**`. Rules do not add up: the winning rule's
grant is the whole answer for that path.

## 4. Who is restricted

Rules **never** restrict:

- server admins;
- principals whose own tenant owns the tree (`Principal.TenantID`, which
  includes the owning tenant's service accounts);
- members of the owning tenant, with any role.

Everyone else is a **non-member** and receives the rights of the rule that
governs the path. Non-members include accounts of other tenants, principals
reaching the tree through grants or `internal` / `public` visibility, and
anonymous callers.

## 5. Grants, grantees and expiry

| `grant` | read | write (push) | export |
|---|:-:|:-:|:-:|
| `read-only` | ✓ | | |
| `modify` | ✓ | ✓ | |
| `redistribute` | ✓ | ✓ | ✓ |
| `""` with a proprietary license | | | |
| `""` with any other license | ✓ | ✓ | ✓ |
| unknown value (corrupt data) | | | |

The rights mean the following:

- **Read** covers pull and clone content, `DownloadObjects`, REST blob and
  file views, diffs and history.
- **Write** covers pushing changes to those paths.
- **Export** covers archives, `wt git export` and other copies out of the
  platform.

A proprietary license is `Proprietary` (any case), `LicenseRef-Proprietary`
or any `LicenseRef-*proprietary*` (`license.IsProprietary`). Without a grant,
an open-source license restricts nothing: IAM still decides who may push.

**Grantee (v0.5).** A rule with `grantee = "partner"` applies only to
principals of tenant `partner`. When a non-member is checked, the server
considers only the rules whose grantee is `""` or the caller's tenant, and
then picks the most specific of those (§3). This lets you write a general
restriction and widen it for one tenant:

```
PUT {path_glob: "billing/**", spdx: "Proprietary", grant: ""}                      # nobody outside acme
PUT {path_glob: "billing", spdx: "Proprietary", grant: "modify", grantee: "partner",
     expires_at: "2026-12-31T23:59:59Z"}                                            # partner may work on it
```

Rules are stored keyed by `(tree, path_glob)`, so one glob can carry only one
rule. A grantee rule therefore needs its own glob, and that glob must be more
specific than the general rule's. In the example, `billing` (no wildcard)
beats `billing/**`, which makes `partner` get `modify`. Other tenants skip
the `partner` rule and fall back to `billing/**`.

**Expiry (v0.5).** Once `expires_at` has passed, the rule is ignored. The path
then falls back to the next most specific applicable rule, or to unrestricted
when none is left. To make access lapse into a restriction, always keep a
general (`grantee: ""`) rule beneath an expiring grantee rule.

## 6. SPDX expressions

`spdx` must be a syntactically valid SPDX 2.3 license expression:

```
expr     = and-expr *( "OR" and-expr )
and-expr = with-expr *( "AND" with-expr )
with     = primary [ "WITH" exception-id ]
primary  = "(" expr ")" | license-id [ "+" ] | [ "DocumentRef-" id ":" ] "LicenseRef-" id
```

Validation follows these rules:

- Operators must be upper case.
- `Proprietary` is accepted as a keyword.
- Identifiers are **not** checked against the SPDX license list.
- An expression is at most 1024 characters, nested at most 32 deep.

Examples: `MIT`, `MIT OR Apache-2.0`,
`GPL-2.0-only WITH Classpath-exception-2.0`, `LicenseRef-Acme-Internal`.

## 7. Enforcement points

| Operation | Check | Result for a non-member without the right |
|---|---|---|
| gRPC `Push` | `CheckPush` on every path that differs between the server tip and the new tip, after IAM and branch protection | Push rejected: `PushDenied{Kind:"license_violation", Path, License}`, sent to the client as `AccessDenied` with the reason |
| gRPC `Pull` (v0.5) | `CheckRead` per path of the new tip's manifest | The path is listed in `PullResponse.stubbed_paths` and its blob is left out of `objects_to_fetch`. Audited as `license.denied` once per pull, with details `count` and first path. |
| gRPC `DownloadObjects` (v0.5) | Blobs of paths the caller may not read are refused | No content leaves the server |
| SDK checkout (v0.5) | Stubbed paths are not written. They are listed in `.wt/trees/<tree>/stubbed` and shown as "restricted" by `wt status`. | The rest of the tree works normally |
| REST content: `blob`, `files`, `compare`, file history, snapshot file lists (v0.5) | `CheckRead` per path | Content requests answer `451 {"code":"license_restricted"}`. Listings omit the path and add `restricted_count`. |
| `GET {tree}/archive/{snapshot}` (v0.5) | Read rights per path | Unreadable paths are omitted and `LICENSES/NOTICE` is added. Audited as `tree.export`. |
| `wt archive`, `wt git export` (v0.5) | Local, using the tree's rules | See §8 |

`staged` metadata lists paths but never content. See
[Staged Visibility](../visibility/StagedVisibility.md#11-interaction-with-licenses-and-path-policies).

When the rules or the membership cannot be loaded, the checker returns an
error, and the caller treats that as a denial (fail closed).

## 8. Exports and the NOTICE file

`wt archive [REV] -o FILE [--format tar.gz|zip]` and `wt git export` consult
the tree's license rules. When the tree has a remote, the rules are cached
from the server. The export follows these rules:

- A generated **`LICENSES/NOTICE`** file lists the SPDX expression governing
  each exported path.
- Paths whose rule does not allow export (redistribution) are **skipped with
  a warning**. Pass `--include-restricted` to keep them.
- `--no-license` skips license processing entirely: no rules are read and no
  NOTICE is written.

The server archive endpoint (§7) applies the same NOTICE generation, and
filters by the caller's read rights.

## 9. License detection

`wt license detect [PATH]` runs `scan::detect_license` over the files and
prints an SPDX guess per file, based on license texts and SPDX headers.
`--apply` proposes the matching server rules (`PUT {tree}/licenses`) for
confirmation. Detection reads file content through `ObjectStore::read_entry`,
so chunked large files work.

## 10. Errors

A denial carries the operation, the path, the license, the rule, and the
required and granted grants, for example:

```
license check failed for operation "write": path "billing/src/pricing.rs" is licensed
"Proprietary" (rule "billing/**"); required grant: modify; granted: read-only
```

The required grant per operation is `read-only` for read, `modify` for write
and `redistribute` for export. A missing grant prints as `(none)`.

## 11. Audit

| Action | When |
|---|---|
| `license.update` | Rule created or replaced (details: glob, SPDX, grant) |
| `license.delete` | Rule removed |
| `license.denied` | Pull with stubbed paths (v0.5) |
| `tree.export` | Server archive (v0.5) |
| `branch.push_rejected` | Push refused, including license violations |
| `access.denied` | Every REST 403 and gRPC `PermissionDenied` |

Query them with `wt audit --action 'license.*'` or `GET /api/v1/audit`.

## 12. License compliance in the access stack

```
request ─► IAM (authz.Evaluate: roles, grants, visibility, policies) ─► license rule for each path ─► allowed
```

The two layers answer different questions:

- **IAM** decides what a principal may do in the platform.
- **The license layer** decides what a non-member may do with particular
  files.

A partner with `tree:write` through a grant still cannot push to a
`read-only` path. A reader of a public tree still receives proprietary paths
only as stubs.

## 13. Implementation status

| Area | Status |
|---|---|
| Per-tree rules, glob syntax and specificity, grant semantics, proprietary defaults | Built (`internal/license`) |
| SPDX expression validation | Built (syntactic) |
| REST and CLI management; push enforcement | Built |
| `grantee` and `expires_at` | Built (v0.5) |
| Read enforcement: stubbed paths, `DownloadObjects` refusal, REST 451 and `restricted_count` | Built (v0.5) |
| License-aware archives (server and local), `LICENSES/NOTICE`, `wt git export` | Built (v0.5) |
| `wt license detect [--apply]` | Built (v0.5) |
| `license.CheckExport` | Built, used by the export tooling |
| Redistribution rule on forks out of the tree's tenant (caller and destination tenant) | Built (v0.8, [Forks](../tree/Forks.md) §6) |

## 14. Roadmap

The following were described in earlier versions of this spec but are not
built:

- **License assignment in config files** (`[license]`, `[[license.path]]` and
  `[[license.grant]]` in `.wt/config.toml` / `.wt-tree/config.toml`), and
  root → tree → path inheritance of a default license. Rules are server-side
  per tree.
- **Strict SPDX-list validation** (`spdx_strict`, SPDX list 3.21+). Validation
  is syntactic only.
- **`attribution_required`**, a generated root `LICENSE` file with a section
  per license, and **license-header injection** into exported source files.
  Exports carry `LICENSES/NOTICE` only.
- **Copyleft tracking.** Derivation through history, copyleft carried to
  derivatives, and warnings on violations. License categories (permissive,
  copyleft, attribution, public domain) are not modelled: only "proprietary or
  not" matters.
- **License checks on cross-tenant merges** (merge requests from a fork in
  another tenant carry no license evaluation of the incoming paths) and
  copying license rules into forks. Forks across tenants are built (v0.8):
  forking out of the tree's tenant applies the export rule of §8 to the
  copied branch and tag tips for the caller and the destination tenant
  ([Forks](../tree/Forks.md) §6); push mirrors are built in v0.7
  ([Mirrors](../server/Mirrors.md)).
- **`wt license audit`**, `wt archive --include-all`, `--license-report`, and
  a license compliance dashboard. Use `wt audit` and the admin tree Licenses
  view.
- **Grantees by email** (`tenant = "bob@contractor.io"`). Grantees are tenant
  slugs.
- **Several grants on one glob.** Grants for several tenants on the same path
  set would need rules keyed by `(glob, grantee)`. Today a grantee rule needs
  its own, more specific glob (§5).
- **Export-right filtering on the server archive.** The archive endpoint
  filters by read rights. Local exports apply the redistribution rule (§8).

## 15. References

- `services/server-go/internal/license/{license.go,glob.go,spdx.go}`
- `services/server-go/internal/syncsvc/{push.go,pull.go,objects.go}`
- `services/server-go/internal/httpapi/{trees.go,snapshots.go,insights.go,exports.go}`
- `services/server-go/internal/domain/types.go` (`LicenseRule`),
  `domain/interfaces.go` (`LicenseChecker`)
- `crates/worktree-cli/src/commands/{license,archive,git}.rs`
- `crates/worktree-sdk/src/scan/license.rs`, `crates/worktree-git/`
- `docs/ENGINEERING_PLAN.md` §2, §13.1, §13.2, §13.8
- Related specs: [IAM](../iam/IAM.md), [Tenant Model](../iam/TenantModel.md),
  [Sync](../sync/Sync.md),
  [Staged Visibility](../visibility/StagedVisibility.md),
  [Security](../security/Security.md)
