# License Compliance Specification

## Overview

W0rkTree enforces license compliance at the file level as a first-class concept. Git has no
equivalent — license compliance in Git is a legal/honor-system concern. In W0rkTree, the server
prevents unauthorized export, copy, fork, or sync of licensed code.

License compliance sits ABOVE IAM in the enforcement stack: both IAM check AND license check
must pass for an operation to succeed.

---

## Why This Exists

- Open-source projects with proprietary modules
- Multi-tenant collaboration where different paths have different licenses
- Code theft prevention for contractors/partners
- Compliance automation (no manual license audits)

In traditional Git workflows, license compliance is entirely the responsibility of the humans
involved. There is no mechanism in Git itself to prevent someone from copying proprietary code
into a public repository, stripping license headers, or violating copyleft requirements. The
only enforcement is legal action after the fact.

W0rkTree changes this by making the server aware of license metadata and enforcing it at the
protocol level. The server will reject operations that violate license constraints before they
happen.

---

## How Licenses Are Assigned

### Root-Level Configuration

In `.wt/config.toml` (root level):

```toml
[license]
default = "MIT"
spdx_strict = true

[[license.path]]
path = "services/billing-engine"
license = "proprietary"

[[license.path]]
path = "vendor/third-party-sdk"
license = "Apache-2.0"
attribution_required = true

[[license.path]]
path = "shared/utils"
license = "MIT"
```

### Tree-Level Configuration

In `.wt-tree/config.toml` (tree level):

```toml
[license]
license = "AGPL-3.0-only"
```

### Assignment Rules

- SPDX identifiers are the standard (MIT, Apache-2.0, AGPL-3.0-only, GPL-3.0-or-later, etc.)
- `spdx_strict = true` (default): only valid SPDX identifiers allowed
- `spdx_strict = false`: allows custom strings for proprietary/internal licenses
- `"proprietary"` is a special keyword meaning no external access without explicit grant
- SPDX expressions are supported: `"MIT OR Apache-2.0"`, `"GPL-2.0-only WITH Classpath-exception-2.0"`

### Validation

When `spdx_strict = true`, the server and bgprocess both validate license identifiers against
the SPDX license list (version 3.21+). Invalid identifiers are rejected at config parse time.

When `spdx_strict = false`, any string is accepted. This is intended for internal/proprietary
licenses that don't have SPDX identifiers (e.g., `"AcmeCorp-Internal-v2"`).

---

## License Grant Model

For proprietary code that a tenant wants to selectively share:

```toml
[[license.grant]]
path = "services/billing-engine"
tenant = "partner-corp"
grant = "read-only"        # Can read within worktree, cannot copy/export

[[license.grant]]
path = "services/billing-engine"
tenant = "contractor@dev.io"
grant = "modify"           # Can read and modify within worktree, cannot export

[[license.grant]]
path = "services/billing-engine/src/api.rs"
tenant = "partner-corp"
grant = "redistribute"     # Full permission to use, modify, copy, export
```

### Grant Levels

| Level          | Read | Modify | Export/Copy/Fork |
|----------------|------|--------|------------------|
| read-only      | ✓    | ✗      | ✗                |
| modify         | ✓    | ✓      | ✗                |
| redistribute   | ✓    | ✓      | ✓                |

### Grant Semantics

- **read-only**: The grantee can view file contents within the W0rkTree platform. They cannot
  modify the file, and they cannot export, copy, fork, or sync the file to any external
  system. This is for review/audit purposes.

- **modify**: The grantee can read and modify the file within W0rkTree. Modifications are
  tracked and attributed. However, the grantee cannot export the file outside the platform.
  This is for contractors who need to work on code but shouldn't be able to take it with them.

- **redistribute**: Full permission. The grantee can read, modify, export, copy, fork, and
  redistribute the file. This is equivalent to having no license restriction for that grantee.

### Grant Resolution

- Grants are path-specific. A grant on `services/billing-engine` applies to all files under
  that path.
- More specific grants override less specific grants for the same tenant.
- Grants are additive: if a tenant has `read-only` on `services/` and `modify` on
  `services/billing-engine`, the `modify` grant applies to `services/billing-engine`.
- Grants do NOT cascade upward: a grant on a child path does not grant access to the parent.

### Grant Expiration (Planned)

```toml
[[license.grant]]
path = "services/billing-engine"
tenant = "contractor@dev.io"
grant = "modify"
expires = "2025-06-30T23:59:59Z"    # Grant expires automatically
```

---

## What the Server Enforces

| Operation                          | License Check                                                                                      |
|------------------------------------|----------------------------------------------------------------------------------------------------|
| Tenant reads file                  | Allowed if IAM permits AND license allows read for that tenant                                     |
| Tenant forks/copies worktree       | Proprietary files excluded. Copyleft carries license. Attribution files include NOTICE.             |
| `wt git export`                    | License headers injected. Proprietary paths blocked. LICENSE file auto-generated.                   |
| Tenant syncs tree to own worktree  | Blocked for proprietary unless explicit grant exists                                               |
| Public worktree browsing           | All can read, but license governs copy/modify/redistribute                                         |
| `wt archive`                       | Proprietary paths excluded by default                                                              |
| Cross-tenant staged visibility     | Can see "Alice working on billing-engine/src/pricing.rs" but cannot read file contents if proprietary |
| `wt git mirror`                    | Proprietary files blocked from mirroring to public Git remotes                                     |
| Branch merge across tenants        | License checks applied to each file in the merge set                                               |

### Enforcement Details

**Fork/Copy**: When a worktree is forked or copied, the server walks the entire tree and
evaluates each path against the license configuration. Proprietary paths without a
`redistribute` grant for the target tenant are excluded from the fork. Copyleft-licensed paths
are included but the license metadata travels with them. Attribution-required paths trigger
automatic NOTICE file generation.

**Git Export**: `wt git export` generates a Git repository from the worktree. During export:
1. Proprietary paths are completely excluded (not even empty files)
2. Copyleft paths include license headers (injected if not already present)
3. Attribution-required paths generate entries in a root-level NOTICE file
4. A LICENSE file is auto-generated at the repo root based on the licenses present
5. If multiple licenses exist, a composite LICENSE file is generated with sections

**Archive**: `wt archive` creates a downloadable archive. By default, proprietary paths are
excluded. The `--include-all` flag requires the requesting user to have `redistribute` grants
for all proprietary paths, otherwise the command fails with a detailed error listing which
paths blocked it.

---

## License Compliance in the Access Stack

```
1. IAM check:     Does this tenant have permission for this action?     → YES/NO
2. License check: Does this file's license permit this operation?       → YES/NO
3. Final:         BOTH must pass. IAM YES + License YES = ALLOWED.
```

A tenant can have full `tree:read` + `tree:write` IAM permissions but still be blocked from
exporting proprietary code without a license grant. This is intentional — IAM governs what
you're allowed to do in the platform, license compliance governs what you're allowed to do
with the code.

### Example Scenarios

**Scenario 1: Contractor with full IAM but no license grant**
- Alice (contractor) has `tree:read`, `tree:write`, `sync:push`, `sync:pull` on the worktree
- `services/billing-engine` is licensed as `proprietary`
- Alice has no `[[license.grant]]` for that path
- Result: Alice can read and modify files in other paths, but CANNOT read, modify, export,
  or interact with `services/billing-engine` in any way

**Scenario 2: Partner with read-only license grant**
- Bob (partner) has `tree:read` IAM permission
- `services/billing-engine` is licensed as `proprietary`
- Bob has a `read-only` grant for that path
- Result: Bob can read the files but CANNOT modify, export, fork, or copy them

**Scenario 3: Public worktree with mixed licenses**
- Worktree is public (anyone can browse)
- `src/` is MIT licensed
- `vendor/sdk/` is Apache-2.0 with `attribution_required = true`
- `services/billing-engine` is proprietary
- Result: Anyone can read `src/` and `vendor/sdk/`. Anyone can fork/export `src/` freely.
  Forks include NOTICE for `vendor/sdk/`. `services/billing-engine` is completely invisible
  to external users.

---

## License Inheritance

- Root `.wt/config.toml` `license.default` applies to all paths without explicit license
- Tree `.wt-tree/config.toml` `license.license` overrides root default for that tree
- `[[license.path]]` overrides both for specific paths
- Most specific wins: path > tree > root default

### Inheritance Diagram

```
Root default: MIT
├── Tree A (no override): MIT
│   ├── services/billing-engine: proprietary (path override)
│   ├── vendor/sdk: Apache-2.0 (path override)
│   └── src/: MIT (inherited from root)
├── Tree B (override: AGPL-3.0-only): AGPL-3.0-only
│   ├── services/billing-engine: proprietary (path override, same as Tree A)
│   └── src/: AGPL-3.0-only (inherited from tree)
└── Tree C (no override): MIT
    └── everything: MIT (inherited from root)
```

### Path Matching

- Paths in `[[license.path]]` are matched as prefixes
- `path = "services/billing-engine"` matches `services/billing-engine/src/main.rs`
- More specific paths take priority over less specific ones
- Exact file paths take priority over directory paths

---

## Git Export License Handling

### `wt git export`

When exporting a worktree to a Git repository:

1. **Proprietary paths**: Blocked entirely. Not included in the export. If the entire worktree
   is proprietary, the export fails with an error.
2. **Copyleft paths** (GPL, AGPL, LGPL, etc.): Included with their license. License headers
   are injected into source files if not already present. The specific header format depends
   on the license.
3. **Attribution-required paths** (Apache-2.0, etc.): Included with a NOTICE file entry.
4. **Permissive paths** (MIT, BSD, ISC, etc.): Included freely.
5. **LICENSE file**: Auto-generated at the repo root. If multiple licenses exist, sections
   are created for each license with the list of paths they apply to.

### `wt git mirror`

- Proprietary files blocked from mirroring to public Git remotes
- Copyleft and permissive files mirrored normally
- Mirror config can specify which remotes are "public" vs "private"
- Private remotes can receive proprietary files (with appropriate grants)

### `wt archive`

- Proprietary paths excluded by default
- `--include-all` flag requires `redistribute` grants for all proprietary paths
- `--license-report` flag generates a license report alongside the archive
- Archive includes a generated LICENSE file and NOTICE file

---

## License Types and Categories

### Categories

| Category     | Examples                              | Default Behavior                    |
|--------------|---------------------------------------|-------------------------------------|
| Permissive   | MIT, BSD-2-Clause, ISC, Unlicense     | Free to read, modify, redistribute  |
| Copyleft     | GPL-3.0-only, AGPL-3.0-only, MPL-2.0 | Carries license to derivatives      |
| Attribution  | Apache-2.0                            | Requires NOTICE file                |
| Proprietary  | `proprietary`, custom strings         | Blocked unless explicit grant       |
| Public Domain| CC0-1.0, Unlicense                    | No restrictions                     |

### Copyleft Enforcement

W0rkTree understands copyleft semantics:
- Files derived from copyleft-licensed code inherit the copyleft license
- The server tracks derivation through snapshot history
- `wt git export` ensures copyleft requirements are met in the exported repo
- Violations are flagged as warnings (not hard blocks, since copyleft interpretation varies)

---

## Audit Trail

All license-related operations are logged:
- License config changes (who changed what, when)
- Grant additions/removals
- License check results (pass/fail, which check, which user)
- Export/archive operations and which paths were included/excluded

The audit trail is accessible via:
- `wt license audit` CLI command
- Admin panel license compliance dashboard
- REST API `/api/repositories/:id/license/audit`

---

## Error Messages

License violations produce clear, actionable error messages:

```
ERROR: License check failed for operation 'export'
  Path: services/billing-engine/src/pricing.rs
  License: proprietary
  Required grant: redistribute
  Your grants: read-only
  
  To export this file, you need a 'redistribute' grant.
  Contact the worktree owner to request access.
```

```
ERROR: License check failed for operation 'read'
  Path: services/billing-engine/src/pricing.rs
  License: proprietary
  Required grant: read-only (minimum)
  Your grants: (none)
  
  You do not have any license grant for this path.
  Contact the worktree owner to request access.
```

---

## Relationship to Other Specs

- **IAM**: License compliance sits above IAM. Both must pass. See `specs/iam/`.
- **Sync**: License checks are evaluated during sync operations. See `specs/sync/Sync.md`.
- **Staged Visibility**: Staged snapshot metadata is visible but file contents respect license
  restrictions. See `specs/visibility/StagedVisibility.md`.
- **Storage**: License metadata is stored alongside tree metadata. See `specs/storage/`.
- **Git Export**: License handling during export is detailed above and in `specs/server/`.

---

## Implementation Status

- **IMPLEMENTED**: None (new concept)
- **TODO**: License type definition, LicenseGrant type, LicenseCheck function, license
  config parsing, SPDX validation
- **PLANNED**: Full licensing module in protocol crate, server enforcement middleware,
  audit trail, admin panel dashboard, `wt license` CLI commands

### Implementation Order

1. Define `License`, `LicenseGrant`, `LicenseCheck` types in protocol crate
2. Implement SPDX identifier validation
3. Implement license config parsing from `.wt/config.toml` and `.wt-tree/config.toml`
4. Implement license inheritance resolution
5. Implement `LicenseCheck` function (path + operation + user → allow/deny)
6. Integrate license check into server access middleware (after IAM check)
7. Implement Git export license handling
8. Implement audit trail
9. Implement CLI commands (`wt license show`, `wt license audit`, etc.)
10. Implement admin panel dashboard