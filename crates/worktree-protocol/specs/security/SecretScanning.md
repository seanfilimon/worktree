# W0rkTree Secret Scanning and License Audit Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. The shared rule set](#2-the-shared-rule-set)
- [3. Detection algorithm](#3-detection-algorithm)
- [4. Fingerprints](#4-fingerprints)
- [5. Client-side scanning](#5-client-side-scanning)
- [6. Server-side scanning of pushes](#6-server-side-scanning-of-pushes)
- [7. Modes, bypass and the allowlist](#7-modes-bypass-and-the-allowlist)
- [8. Findings](#8-findings)
- [9. On-demand scans](#9-on-demand-scans)
- [10. License audit](#10-license-audit)
- [11. Events, audit and documents](#11-events-audit-and-documents)
- [12. REST API](#12-rest-api)
- [13. CLI](#13-cli)
- [14. Admin panel](#14-admin-panel)
- [15. Performance](#15-performance)
- [16. Implementation status](#16-implementation-status)
- [17. Roadmap](#17-roadmap)
- [18. References](#18-references)

---

## 1. Overview

Secrets (API keys, tokens, private keys, passwords) are caught twice:

1. **On the client, before every snapshot** (`worktree_sdk::scan`), which
   refuses the snapshot by default. `wt snapshot --no-scan` and
   `[secret_scanning] block_on_match = false` let content through locally.
2. **On the server, on every push** (`internal/scan`, a push checker), which
   records what it finds and — in a tree's `block` mode — rejects the push.
   The client's `--no-scan` has no effect here.

Both sides run the **same rule set** from one JSON file, and produce the
same findings, excerpts and fingerprints for the same bytes. Tree
administrators manage the server side per tree: the mode (`off`, `warn` —
the default — or `block`), exclusions, an allowlist of known false
positives, and the findings themselves.

The same workstream adds the **license audit** of a snapshot: files per
license, unlicensed files, proprietary files reachable by other tenants
through grants, and a strict check of every rule's SPDX expression against
the SPDX License List.

## 2. The shared rule set

`crates/worktree-protocol/src/scan_rules.json` is the single source of
truth. The Rust SDK embeds it through `worktree_protocol::scan_rules`
(`include_str!`); the Go server embeds a byte-identical copy,
`services/server-go/internal/scan/rules.json` (Go cannot embed files outside
its module), and `TestRulesFileMatchesProtocol` fails when the two differ.

```json
{
  "version": 1,
  "allow_marker": "wt:allow-secret",
  "max_scan_line_len": 8192,
  "placeholder_markers": ["example", "xxx", "changeme", "your", "${", "{{", "****", "..."],
  "rules": [
    {
      "id": "github-token",
      "description": "GitHub token (ghp_, gho_, ghu_, ghs_, ghr_)",
      "pattern": "\\bgh[pousr]_[A-Za-z0-9]{36,255}\\b",
      "keywords": ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
      "min_entropy": 2.5,
      "skip_placeholders": true
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Stable rule id reported with findings; several rules may share one (`generic-secret`) |
| `pattern` | Regex in the common subset of Rust `regex` and Go RE2, matched against bytes in multi-line mode; never crosses a line break |
| `keywords` | The rule only runs on input (Go: lines) containing one of them, ASCII case-insensitive; every match contains one |
| `min_run` | Optimization hint: a matching line holds a run of at least this many bytes other than space, `\t`, `\r`, `\n`, `\f` |
| `min_entropy` | Minimum Shannon entropy (bits per byte) of the value |
| `needs_letter_and_digit` | The value must contain an ASCII letter and an ASCII digit |
| `skip_placeholders` | Values containing a placeholder marker are not findings |
| `mask_rest_of_line` | The value is a structural marker (a PEM header): it is shown and the rest of the line masked |
| `validate_tail` | `key_body`: a base64 key body must follow (next lines, or after an escaped `\n`) |

Built-in rules, in priority order (earlier rules win overlapping matches):
`private-key`, `aws-access-key`, `aws-secret-key`,
`github-fine-grained-token`, `github-token`, `gitlab-token`,
`slack-webhook`, `slack-token`, `stripe-secret-key`, `google-api-key`,
`npm-token`, `anthropic-api-key`, `openai-api-key`, `jwt` and two
`generic-secret` rules (quoted and `.env`/YAML-style assignments to
password / secret / token / api_key / credential variables).

`crates/worktree-protocol/src/scan_rule_cases.json` holds the **parity
cases**: inputs (assembled from string, `repeat` and `hex` parts, so the file
contains no literal secret) and the exact findings expected — rule, line,
redacted excerpt and, for some, the fingerprint. The Rust test
`scan::secrets::tests::shared_parity_cases` and the Go test
`TestParityCases` both check them. Both scanners also verify that the rule
file and the case file themselves trip no rule.

## 3. Detection algorithm

For one file's bytes (both implementations):

1. Empty input, or a NUL byte in the first 8000 bytes (binary), yields
   nothing.
2. Each rule whose keyword prefilter passes is matched against the input
   (leftmost-first, non-overlapping). The reported value is the first
   participating capture group, else the whole match. The value must pass
   the post-filters (placeholders, letter and digit, entropy); `key_body`
   rules additionally require a key body after the match.
3. Candidates are sorted by position and grouped by line. Lines containing
   the allow marker `wt:allow-secret` are dropped; so are candidates ending
   beyond the first 8192 bytes of their line.
4. Per line, candidates are deduplicated: the earliest rule wins, anything
   overlapping an accepted span is dropped.
5. The excerpt is the line (without a trailing `\r`) with every accepted
   value redacted: values of 12 or more characters keep their first and last
   four (`AKIA****Y3U6`), shorter ones become `****`; a `mask_rest_of_line`
   value is kept and the rest of the line becomes `****`. Excerpts longer
   than 200 bytes are windowed (60 bytes before the secret, 120 after, `…`
   marking cuts). Excerpts never contain the secret.

Known divergences, outside realistic secrets: Go matches negated classes and
counted repetitions per UTF-8 rune where Rust (Unicode off) counts bytes, Go
`\s` excludes `\v`, and a truncated multi-byte sequence becomes one U+FFFD
per byte in Go excerpts.

## 4. Fingerprints

A finding's **fingerprint** identifies the secret wherever it appears:

```
fingerprint = hex(BLAKE3("wt-secret-v1" NUL rule-id NUL value))[0..24]
```

`value` is the reported secret bytes; for `key_body` rules it is the header,
a newline and the first key body line, so that different private keys have
different fingerprints. The fingerprint is a truncated hash (96 bits) of the
secret: it does not reveal high-entropy secrets, but a weak custom-pattern
match could be guessed from it, which is why only tree members see it.
`worktree_protocol::scan_rules::fingerprint` and Go `scan.Fingerprint`
implement it; the parity cases pin known values.

## 5. Client-side scanning

`worktree_sdk::scan::SecretScanner` runs before every snapshot (including
the daemon's auto-snapshots) on added and modified files, configured by
`[secret_scanning]` (`enabled`, `block_on_match`, `exclude` globs, extra
`patterns` — custom rules have no post-filters and report their `name` as
rule id). Findings (`SecretFinding { rule, path, line, excerpt,
fingerprint }`) refuse the snapshot unless `block_on_match = false` or
`wt snapshot --no-scan`. See [`Security.md`](./Security.md) §13.

`wt compliance scan [PATH]` scans the working files of the current tree on
demand (ignored paths, nested trees and `.wt/` skipped; files over 8 MiB
skipped), even when automatic scanning is disabled, and prints every
finding with its fingerprint; it exits 1 when something is found.

## 6. Server-side scanning of pushes

`scan.Service` is a `domain.PushChecker`, run by `syncsvc.Push` after the
license check and before anything is stored (`PushChecks`: path ACLs,
review, limits, tree config, **scan**, signing). For a push it collects:

- for each new snapshot (newest first), the manifest entries that differ
  from its first parent (added or changed blob), deduplicated by
  (path, blob) — so a secret added and removed again within one push is
  still found, attributed to the snapshot that introduced it;
- with more than 256 new snapshots (imports), only what the new tip adds
  relative to the old tip;
- nothing for a branch created at an already stored snapshot.

Skipped: chunked files, files larger than the tree's `max_file_bytes`
(default 1 MiB, at most 16 MiB), paths matching an `exclude` glob (license
rule glob syntax), and — once 64 MiB have been collected — the rest of the
push (logged; the event carries `truncated`). Blobs are read from the object
store (the push pipeline verified their presence) and scanned by up to eight
workers. Allowlisted fingerprints are dropped; the rest is deduplicated by
(fingerprint, path).

Failures: in `warn` mode a scanner failure (an unreadable manifest) is
logged and the push proceeds; in `block` mode it fails the push (fail
closed).

## 7. Modes, bypass and the allowlist

| Mode | Push with findings |
|---|---|
| `off` | Not scanned |
| `warn` (default) | Accepted; findings stored (`action: warned`), event and audit |
| `block` | Rejected (`action: blocked`) unless the pusher holds `branch:bypass_protection` on the tree (owners), in which case it is accepted (`action: bypassed`) |

A blocked push is answered with `PushRejection.access_denied` whose reason
lists up to ten findings and the way out (the wire format has no dedicated
variant; `PushDenied.Kind` is `secret_detected` and that is what the audit
`branch.push_rejected` records):

```
access denied: secret scanning blocked the push: 1 possible secret(s)
  src/app.rs:2 [github-token] const T: &str = "ghp_****2Zx9"; (fingerprint fd07e02d5902c12c017b886a)
remove the secret from every new snapshot and push again; a false positive can be allowed by a tree admin with `wt compliance allow FINGERPRINT --reason ...`
```

The **allowlist** (`scan_allow`) names fingerprints of known false
positives with a required reason (at most 500 bytes; at most 1000 entries per
tree). Allowlisted secrets produce no findings and never block, in any mode
and for anyone. Adding an entry resolves the open findings with that
fingerprint as `false_positive`. The inline `wt:allow-secret` marker works
on the server too (it is part of the rule set).

## 8. Findings

One finding per (fingerprint, path) and tree; its id is
`hex(BLAKE3(fingerprint NUL path))[0..16]`. A later push containing the same
secret at the same path updates it: `count`, `last_seen_at`, `line`,
`excerpt`, `branch`, `snapshot_id`, `action`, `pushed_by`. A resolved
finding is reopened when the secret lands again (a warned or bypassed push;
a blocked push leaves it resolved). At most 100 findings are stored per
push.

```json
{
  "id": "9f2c51e07d3a4b18", "tree_id": "…", "path": "config/aws.env", "line": 2,
  "rule": "aws-access-key", "excerpt": "AWS_ACCESS_KEY_ID=AKIA****Y3U6",
  "fingerprint": "683e63ab39e025dc3e6ea642", "branch": "main", "snapshot_id": "…",
  "status": "open", "action": "warned", "count": 1, "pushed_by": "bob",
  "first_seen_at": "…", "last_seen_at": "…"
}
```

Resolutions: `revoked`, `false_positive`, `test_data`, `wont_fix`, with an
optional note (at most 1000 bytes); `reopen` sets a finding back to open.
Resolving does not let a secret through a blocked push — the allowlist does.

## 9. On-demand scans

`POST {tree}/compliance/scan {ref?}` scans every file of a snapshot (a
branch, tag or snapshot id; default the default branch) with the tree's
exclusions and size cap and returns the matches, each flagged `allowed` when
allowlisted. Nothing is stored. `wt compliance scan --server [--ref REF]`
calls it and exits 1 when a match is not allowlisted.

## 10. License audit

`GET {tree}/compliance/licenses?ref=` audits a snapshot (default: the
default branch tip) against the tree's license rules
([`LicenseCompliance.md`](../licensing/LicenseCompliance.md)):

- **Files per license** — every file resolved against the live general rules
  (no grantee, not expired; most specific glob wins), counted per SPDX
  expression with the globs that assign it and whether it is proprietary.
- **Unlicensed paths** — files no general rule matches.
- **Exposed proprietary paths** — files whose governing license is
  proprietary but which a non-member can read: the general rule itself has a
  grant (`grantee: ""`, every other tenant), or a live grantee rule matching
  the path permits reading (`grantee: "<slug>"`).
- **Rule checks** — every rule (expired ones included and flagged) with the
  number of files it governs and a **strict SPDX check**: the expression
  must parse (`license.ValidateSPDX`), every license identifier must be on
  the SPDX License List in its canonical case and not deprecated
  (`GPL-2.0`, `GPL-2.0+`, `LGPL-2.1`… are deprecated: use the `-only` /
  `-or-later` forms), exceptions after `WITH` must be listed exceptions;
  `LicenseRef-…`, `DocumentRef-…:LicenseRef-…` and W0rkTree's `Proprietary`
  keyword are accepted.

The SPDX License List (version 3.25: licenses, deprecated identifiers and
exceptions) is embedded in the server (`internal/scan/spdx.txt`). Path lists
are capped at 1000 entries (`truncated`); counts are exact.

`GET /api/v1/admin/compliance` (server administrators) summarizes every
tree: mode, open / resolved / blocked findings, allowlist size, license rules
and rules failing the strict check, totals, the 20 most recent open findings,
the rule set version and the SPDX list version.

## 11. Events, audit and documents

- Event **`secret.detected`** for every push with findings (any mode):
  `{tree, branch, new_tip, mode, action, count, allowed, truncated,
  findings: [{id, path, line, rule, excerpt, fingerprint}] (≤ 20)}`.
  Webhooks and notifications can subscribe to it.
- Audit **`secret.detected`** (resource `branch`, details `tree`, `branch`,
  `new_tip`, `mode`, `action`, `count`, `rules`, `paths`); a blocked push is
  also audited as `branch.push_rejected` with reason `secret_detected`.
  REST changes are audited as `compliance.settings`,
  `compliance.finding.resolve`, `compliance.finding.reopen`,
  `compliance.allow.add`, `compliance.allow.remove`.

| Collection | Key | Value |
|---|---|---|
| `scan_settings` | tree id | `Settings {tree_id, mode, exclude, max_file_bytes, updated_by, updated_at}` |
| `scan_findings` | `DocKey(treeID, findingID)` | `Finding` |
| `scan_allow` | tree id | `AllowList {tree_id, entries: [{fingerprint, reason, created_by, created_at}]}` |

`tree.deleted` removes the three documents of the tree.

## 12. REST API

| Method and path | Action | Notes |
|---|---|---|
| `GET {tree}/compliance/settings` | `tree:read` | Defaults when never set |
| `PUT {tree}/compliance/settings` | `tree:admin` | `{mode?, exclude?, max_file_bytes?}` |
| `GET {tree}/compliance/findings` | `tree:write` | `?status=open` (default) `\|resolved\|all`, `limit` ≤ 1000 |
| `POST {tree}/compliance/findings/{id}/resolve` | `tree:admin` | `{resolution, note?}` or `{reopen: true}` |
| `GET {tree}/compliance/allowlist` | `tree:write` | |
| `POST {tree}/compliance/allowlist` | `tree:admin` | `{fingerprint, reason}` → 201 |
| `DELETE {tree}/compliance/allowlist?fingerprint=` | `tree:admin` | 204 |
| `POST {tree}/compliance/scan` | `tree:admin` | `{ref?}` |
| `GET {tree}/compliance/licenses` | `tree:admin` | `?ref=` |
| `GET /api/v1/admin/compliance` | `server:admin` | |

`{tree}` is `/api/v1/trees/{tenant}/{tree}`. Without the scan component the
routes answer 501. Operations are in `openapi.yaml` (tag `Compliance`).

## 13. CLI

```bash
wt compliance scan [PATH]                     # local working files; exit 1 on findings
wt compliance scan --server [URL] [--ref REF] # a snapshot on the server
wt compliance findings [--status open|resolved|all] [--limit N] [--tree T/N]
wt compliance resolve ID --as revoked|false-positive|test-data|wont-fix [--note TEXT]
wt compliance resolve ID --reopen
wt compliance allow FINGERPRINT --reason TEXT
wt compliance allow --list
wt compliance disallow FINGERPRINT [--yes]
wt compliance settings [--mode off|warn|block] [--exclude GLOB]... [--clear-exclude] [--max-file-bytes N]
wt license audit [TENANT/TREE] [--ref REF] [--strict]  # --strict: exit 1 on invalid rules or exposure
```

`--server` on `wt compliance scan` shares the global `--server URL` flag:
bare, it scans on the default server; with a URL it also selects that
server. Every command takes `--json`.

## 14. Admin panel

- Tree tab **Compliance** (`pages/tree/ComplianceTab.tsx`): scanning mode
  (radio), excluded paths and size cap (tree admins edit, others read);
  findings filtered open / resolved / all with location, redacted excerpt,
  last push action, fingerprint and when; resolve (dialog with resolution
  and note), allowlist (dialog with the fingerprint prefilled and a required
  reason), reopen; the allowlist with removal; the license audit of the
  default branch (summary counts, files per license, exposed paths,
  unlicensed paths, rule checks).
- Page **Compliance** (`pages/CompliancePage.tsx`, server admins): totals,
  modes, the per-tree table linking to each tree's tab, and the latest open
  findings.

## 15. Performance

Go benchmarks (`go test -bench . ./internal/scan`, AMD Ryzen Threadripper
PRO 9995WX, one core per file):

| Benchmark | Throughput | Cost per MiB |
|---|---|---|
| `BenchmarkScanProse` (no keywords) | ~175 MB/s | ~6 ms |
| `BenchmarkScanSource` (source code, keywords on every line, no long values) | ~135 MB/s | ~8 ms |
| `BenchmarkScanDense` (worst case: keyword and long value on every line) | ~4 MB/s | ~270 ms |
| `BenchmarkCheckPushPlan` (64 files / 1 MiB, one secret: manifests, blob reads, scan, store) | ~190 MB/s | ~5.5 ms |

The keyword prefilter skips rules without a keyword in the file; rules with
keywords run line by line on matching lines only (Go's regexp is much faster
on short inputs) and `min_run` skips lines without a long enough token. The
worst case is bounded by the 1 MiB default file cap, the 64 MiB push budget
and parallel workers. `TestScanThroughputFloor` fails if 1 MiB of source
takes more than 5 s. The Rust scanner (regex crate with literal
prefilters) scans 10 MB of source in well under a second in release builds
(`perf_scan_10mb_under_one_second`, ignored by default).

## 16. Implementation status

| Item | Status |
|---|---|
| Shared JSON rule set embedded by the SDK and the server; byte-identity and parity tests | Built |
| Fingerprints (client and server) | Built |
| Push checker: per-snapshot diff, caps, exclusions, parallel scan, allowlist | Built |
| Modes `off` / `warn` (default) / `block`, `branch:bypass_protection` bypass | Built |
| Findings with dedupe, resolve / reopen, `secret.detected` event, audit | Built |
| On-demand server scan | Built |
| License audit with the embedded SPDX License List; server-wide summary | Built |
| REST, OpenAPI, `wt compliance`, `wt license audit`, admin tab and page | Built |

## 17. Roadmap

- A dedicated `SecretDetected` push rejection variant on the wire (today an
  `AccessDenied` with a structured reason).
- Server-side custom patterns per tree (the client's `[secret_scanning]
  patterns` are local only) and tenant-wide defaults.
- Scanning chunked files and archives; scanning staged snapshots.
- Verifying found credentials against their issuer (validity checks) and
  automatic revocation hooks.
- Push warnings in the client output for `warn` mode (the protocol has no
  warning channel; findings are visible through `wt compliance findings`,
  events and notifications).
- Automatic refresh of the embedded SPDX License List from the SPDX
  project's data files.
- Copyleft and license-compatibility analysis in the audit.

## 18. References

- `docs/ENGINEERING_PLAN.md` §16.7 — binding contract
- `crates/worktree-protocol/src/scan_rules.{json,rs}`, `scan_rule_cases.json` — shared rules and parity cases
- `crates/worktree-sdk/src/scan/{mod,secrets}.rs` — client scanner
- `services/server-go/internal/scan/` — `rules.go` (engine), `push.go` (push checker), `scan.go` (settings, findings, allowlist), `report.go` (on-demand scan, summary), `licaudit.go` + `spdx.txt` (license audit), benchmarks
- `services/server-go/internal/httpapi/compliance.go` — REST routes; `openapi.yaml` — operations
- `crates/worktree-cli/src/commands/compliance.rs`, `commands/license/audit.rs` — CLI
- `apps/admin/src/pages/tree/ComplianceTab.tsx`, `pages/CompliancePage.tsx`, `pages/tree/complianceApi.ts` — admin UI
- `crates/worktree-e2e/tests/e2e/server_secret_scan.rs`, `secret_scan.rs` — end-to-end scenarios
- [`Security.md`](./Security.md), [`LicenseCompliance.md`](../licensing/LicenseCompliance.md)
