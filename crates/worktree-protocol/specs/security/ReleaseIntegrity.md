# Release Integrity Specification

Signed tags, verified releases, asset checksums and software bills of
materials (`docs/ENGINEERING_PLAN.md` §15.8, v0.7 workstream `trust`).

## Table of Contents

1. [Overview](#1-overview)
2. [Signed tags](#2-signed-tags)
3. [Tag verification on the server](#3-tag-verification-on-the-server)
4. [Signed-tag protections](#4-signed-tag-protections)
5. [Release assets and checksums](#5-release-assets-and-checksums)
6. [Verified releases](#6-verified-releases)
7. [SBOM (SPDX 2.3)](#7-sbom-spdx-23)
8. [Client verification](#8-client-verification)
9. [REST and wire reference](#9-rest-and-wire-reference)
10. [Admin panel](#10-admin-panel)
11. [Threat model](#11-threat-model)
12. [Implementation status](#12-implementation-status)
13. [Roadmap](#13-roadmap)
14. [References](#14-references)

---

## 1. Overview

A release is trustworthy when a consumer can check, without trusting the
transport or the server's storage, that

- the release's tag was created by a known person (an Ed25519 signature by a
  key registered to their account, the same keys as snapshot signing,
  [Security §12](./Security.md#12-snapshot-signing)), and
- every downloaded asset is byte-identical to what was uploaded (SHA-256
  checksums published as a `SHA256SUMS` file).

```
wt tag v1.0 --sign ──► TagRecord + Ed25519 signature ──► PushTag / POST {tree}/tags
                                                              │ verify against signing_keys
                                                              ▼
                                   Tag{verified, signer} ── Release{verified}
                                                              │
   wt release download ──► files ── wt release verify ◄── GET .../checksums + tag signature
```

## 2. Signed tags

A tag (`TagInfo` locally, `TagRecord` on the wire, `domain.Tag` on the
server) is signed over its **canonical payload**: the deterministic protobuf
encoding of

```
TagRecord {
  name        = 1   // tag name
  target      = 2   // SnapshotId of the tagged snapshot
  message     = 3   // optional; an empty message is encoded as ABSENT
  tagger      = 4   // the tagger value chosen by the signer
  created_at  = 5   // google.protobuf.Timestamp (seconds + nanos)
  signature   = 6   // CLEARED
  signing_key_id = 7 // CLEARED
}
```

- Algorithm: Ed25519 (`ed25519-dalek` / `crypto/ed25519`), 64-byte signature.
- `signing_key_id` is the server key id after `wt identity register`, else
  the key fingerprint (`SHA256:` + unpadded base64 of SHA-256 of the public
  key); the server resolves either.
- Rust: `worktree_sdk::signing::tag_signed_payload`; Go:
  `signing.TagPayload` / `signing.TagPayloadFromRecord`. Both pin the same
  cross-language test vector (seed `0x00..0x1f`, tag `v1.0.0` at
  `0f8fad5b-…`, message `"Release 1.0.0\n\nSigned."`, tagger
  `Ada Lovelace <ada@example.com>`, created `2026-01-02T03:04:05.123456789Z`).

Creating signed tags:

| Command | Effect |
|---|---|
| `wt tag NAME [REV] [-m MSG] --sign` (= `wt tag create … --sign`) | Signs with `[signing] key`, else the `default` key |
| `[signing] key = "K"`, `auto = true` | Every tag created with `wt tag` is signed (like snapshots) |
| `wt tag push NAME [--remote R]` | Pushes one tag and fails with the server's reason when refused |
| `wt push` | Also pushes local tags the server lacks (refusals are logged, not fatal) |

The signature is stored in the local tag file
(`.wt/trees/<tree>/refs/tags/<name>.json`, field `signature
{key_id, signature}`, omitted when unsigned) and travels with the tag on
push, fetch and clone. The key is loaded before the tag is written, so a
missing key leaves no unsigned tag behind.

## 3. Tag verification on the server

Every tag about to be stored — `PushTag`, `POST {tree}/tags`, and the tag a
`POST {tree}/releases` creates — passes `signing.Service.CheckTagSignature`:

1. A tag carrying a signature or key id must verify: the key must be
   registered and unrevoked, the signature 64 bytes and valid over the
   canonical payload. Otherwise the tag is refused with
   `PushDenied{Kind:"signature_required", Reason:"tag 'X' has an invalid
   signature (…)"}` (REST `403 signature_required`; `PushTag` answers
   `accepted=false` with the reason) and audit `tag.signature_invalid` is
   recorded.
2. A verified tag is stored with `Verified=true`, `Signer` = username of the
   key's account, `Signature`, `SigningKeyID` and `SignedTagger` (the signed
   tagger value). `Tagger` stays the authenticated creator (clients never
   choose it).
3. Clients cannot set `Verified` or `Signer`; a server without the signing
   service drops signatures instead of storing them unchecked.

Records sent back (`ListTags`, `GET {tree}/tags`) carry the signature, key
id and — for signed tags — the signed tagger as the record's `tagger`, so
any client can re-verify them.

Signed REST creates must name the exact record: `target` must be the
snapshot id (not a branch), and `created_at` (RFC 3339 with nanoseconds) and
`tagger` must be the signed values; `signature` is base64.

## 4. Signed-tag protections

`TagProtection.require_signed` (`wt protect tag set PATTERN
--require-signed`, `PUT {tree}/tag-protections`): tags matching `PATTERN`
must carry a verified signature. Unsigned matching tags are refused on push,
REST create and release creation with `signature_required` ("tag 'v2' is
protected by 'v*' and must be signed (wt tag v2 --sign)"), audited as
`tag.signature_invalid` with reason `unsigned`. Other tag protection flags
(`no_move`, `no_delete`, `allowed_taggers`) apply as before.

## 5. Release assets and checksums

- `PUT {tree}/releases/{tag}/assets/{name}` computes the hex SHA-256 of the
  bytes and stores it on the asset (`ReleaseAsset.sha256`), next to the
  BLAKE3 content address (`hash`).
- `GET {tree}/releases/{tag}/checksums` (tree:read; drafts need
  release:create) returns `text/plain` in `sha256sum` format, one line per
  asset sorted by name: `<64 hex>␠␠<name>\n`. Assets uploaded before v0.7
  (no stored `sha256`) are hashed from their stored bytes.
- `wt release checksums TAG [TREE]` prints it.

## 6. Verified releases

`Release.verified` (with `signing_key_id`, `signer`) is computed on every
read and never stored: a release is verified when its tag has
`verified=true` **and** still points at the release's snapshot (a tag
force-recreated elsewhere no longer vouches for it). `wt release show/list`
display it.

## 7. SBOM (SPDX 2.3)

`POST {tree}/releases/{tag}/sbom` (release:create; `wt release create
--sbom`, `wt release sbom TAG`) generates an SPDX 2.3 JSON document of the
release's snapshot and attaches it as asset `sbom.spdx.json`
(`application/spdx+json`, replacing an earlier one) → `201 ReleaseAsset`.

| Field | Value |
|---|---|
| `spdxVersion`, `dataLicense`, `SPDXID` | `SPDX-2.3`, `CC0-1.0`, `SPDXRef-DOCUMENT` |
| `name` | `<tenant>/<tree>@<tag>` |
| `documentNamespace` | `<scheme>://<host>/spdx/<tenant>/<tree>/<tag>/<uuid>` (unique per generation) |
| `creationInfo` | `created` (UTC, seconds), creators `Tool: W0rkTree-<version>`, `Person: <requester>` |
| `packages[0]` | `SPDXRef-Package`: tree name, `versionInfo` = tag, `filesAnalyzed: true`, `packageVerificationCode` (SHA-1 of the sorted file SHA-1s), `licenseInfoFromFiles` (rule SPDX ids or `NOASSERTION`), comment with snapshot id and omitted-path count |
| `files[]` | `./<path>` sorted, `SPDXRef-File-<n>`, checksums `SHA1`, `SHA256` and `BLAKE3` (non-chunked files: the manifest hash), `licenseConcluded` = SPDX expression of the most specific general, unexpired license rule covering the path (else `NOASSERTION`) |
| `relationships` | `DOCUMENT DESCRIBES Package`, `Package CONTAINS File-n` |
| `hasExtractedLicensingInfos` | one entry per `LicenseRef-*` used |

Files the requester may not read (license read rules, tree:read path
policies) are omitted and counted in the package comment; an SBOM lists
names and hashes only, so the redistribute grant of archive exports is not
required. At most 200 000 files.

## 8. Client verification

`wt tag verify NAME [--account USER] [--remote [--from R]]` verifies the
local (or the server's) copy of a tag against the local keys plus the
registered keys of `USER`; exit 1 unless the signature is good.

`wt release verify TAG [-d DIR] [--in TENANT/TREE] [--account USER]
[--allow-unsigned] [--ignore-missing]`:

1. fetches the release, its `SHA256SUMS` and the server's tag record;
2. rebuilds the canonical tag payload from the record (signed tagger,
   base64 signature) and verifies it against the local keys, the signer's
   registered keys (`GET /accounts/{signer}/keys`) and `--account`'s;
3. checks that the tag points at the release's snapshot;
4. hashes `DIR/<asset>` for every checksum line (`ok`, `mismatch`,
   `missing`) and flags assets without a checksum line (`unlisted`).

It fails (exit 1) on an unsigned tag (unless `--allow-unsigned`), an
unknown or invalid signature, a moved tag, a mismatch, an unlisted asset or
a missing file (unless `--ignore-missing`). `--json` prints
`{tag, snapshot_id, tag_target, signer, server_verified, signature:
{status, …}, assets: [{name, expected, actual, status}]}`. The verdict does
not rely on the server's `verified` flag.

## 9. REST and wire reference

| Operation | Notes |
|---|---|
| `PushTag(TagRecord{…, signature, signing_key_id})` | §3; refusals in `PushTagResponse.error` |
| `ListTags` | records include signatures (§3) |
| `POST {tree}/tags` `{name, target, message, force?, signature?, signing_key_id?, tagger?, created_at?}` | §3 |
| `GET {tree}/tags` | `Tag` with `signature`, `signing_key_id`, `verified`, `signer`, `signed_tagger` |
| `PUT {tree}/tag-protections` | `TagProtection.require_signed` |
| `GET {tree}/releases[/{tag}]` | `Release.verified`, `signing_key_id`, `signer`; `ReleaseAsset.sha256` |
| `GET {tree}/releases/{tag}/checksums` | §5 |
| `POST {tree}/releases/{tag}/sbom` | §7 |

Audit actions: `tag.signature_invalid`, plus `tag.create` / `tag.push`
(details `verified`, `signing_key_id`) and `release.asset.upload` for SBOMs.
Event `tag.pushed` payloads carry `verified`.

## 10. Admin panel

Tags tab: a "Verified" badge (signer and key in the tooltip) and the signed
tagger under the tagger. Releases tab: "Verified" badge on verified
releases, a SHA-256 column on assets (shield mark when the release is
verified), a "Checksums" dialog with the `SHA256SUMS` text (copy and
download), "Download SBOM" and "Generate / Regenerate SBOM" (release
writers).

## 11. Threat model

| Threat | Mitigation |
|---|---|
| Tampered tag record in transit or storage | Signature over the canonical record; clients re-verify (`wt tag verify --remote`, `wt release verify`) |
| Forged signer identity | Keys are registered per account; `Signer` is the key owner, not a client field; revoked keys stop verifying |
| Unsigned release tags on protected names | `require_signed` tag protections, also for release-created tags |
| Swapped or corrupted asset | SHA-256 per asset, `SHA256SUMS`, `wt release verify`; BLAKE3 addressing in the store |
| Tag moved after release | A release is only verified while its tag points at its snapshot |
| Malicious server lying about `verified` | `wt release verify` checks the signature itself against the signer's published keys |

Not covered: a compromised server that also replaces the signer's
published public keys (prefer local keys or keys obtained out of band), and
a compromised server that swaps an asset together with its checksum — the
tag signature covers the snapshot, not the assets, and `SHA256SUMS` is not
signed yet (see Roadmap).

## 12. Implementation status

| Feature | Status |
|---|---|
| Canonical tag payload, Rust↔Go test vector | Built (v0.7 §15.8) |
| `wt tag --sign`, `[signing] auto` for tags, `wt tag verify`, `wt tag push` | Built (v0.7 §15.8) |
| Verification on `PushTag`, REST tag create and release-created tags | Built (v0.7 §15.8) |
| `TagProtection.require_signed`, `wt protect tag set --require-signed` | Built (v0.7 §15.8) |
| Asset SHA-256, `GET …/checksums`, `wt release checksums` | Built (v0.7 §15.8) |
| Verified releases (computed) | Built (v0.7 §15.8) |
| SPDX 2.3 SBOM generation and attachment | Built (v0.7 §15.8) |
| `wt release verify` | Built (v0.7 §15.8) |
| Admin badges, checksums dialog, SBOM download/generation | Built (v0.7 §15.8) |
| Signed `SHA256SUMS` / per-asset signatures | Roadmap |

## 13. Roadmap

- A detached signature over `SHA256SUMS` (signed by the release author) so
  asset integrity no longer depends on the server.
- SBOM package metadata from manifests (Cargo.toml, package.json, go.mod)
  and dependency relationships.
- Verifying tags signed by keys of other servers (key transparency).

## 14. References

- `crates/worktree-sdk/src/signing.rs` — tag payload, signing, verification, checksum helpers
- `crates/worktree-sdk/src/remote/{convert.rs,tags.rs}` — tag records on the wire, `push_tag`
- `crates/worktree-cli/src/commands/{tag.rs,release.rs,identity.rs,protect.rs}`
- `services/server-go/internal/signing/tags.go` — `TagPayload`, `VerifyTag`, `CheckTagSignature`
- `services/server-go/internal/syncsvc/trees.go` — `PushTag` verification block
- `services/server-go/internal/release/integrity.go` — checksums, verified releases, SBOM
- `services/server-go/internal/httpapi/{manage.go,releases.go}`, `openapi.yaml`
- `apps/admin/src/pages/tree/{TagsTab.tsx,ReleasesTab.tsx,TrustBadges.tsx,trust.ts}`
- `crates/worktree-e2e/tests/e2e/release_integrity.rs`
