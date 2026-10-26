# W0rkTree Server Performance Specification

## Table of Contents

- [1. Overview](#1-overview)
- [2. Measurement tooling](#2-measurement-tooling)
- [3. Measurements](#3-measurements)
- [4. Performance-relevant behaviour](#4-performance-relevant-behaviour)
- [5. Limits](#5-limits)
- [6. Tuning flags](#6-tuning-flags)
- [7. Runtime profiling](#7-runtime-profiling)
- [8. Implementation status](#8-implementation-status)
- [9. Roadmap](#9-roadmap)
- [10. References](#10-references)

---

## 1. Overview

This document fixes how W0rkTree server performance is measured, the
numbers measured for v0.7, the behaviour the sync and authorization paths
rely on to stay fast, the hard limits a deployment runs into, and the
flags that tune them. User-facing instructions and full before/after
tables are in `docs/performance.md`.

## 2. Measurement tooling

| Tool | Scope |
|---|---|
| `worktree-bench` (`crates/worktree-bench`) | End-to-end load: N concurrent workers pushing their own trees (`--files`, `--file-size`, `--changed`), cloning a seeded tree, and running code searches; through the worktree-sdk async API. Starts its own server (free port, temp data dir, `--rate-limit 0`) or targets `--server`. Reports count, errors, p50/p95/p99/max/mean latency (nearest rank), ops/s and MB/s, as a table or `--json`. |
| Go benchmarks (`*_bench_test.go`) | `syncsvc` (manifest decode, diff, object checks, Push, Negotiate; 10k-entry manifests), `authz` (Authorize, branch-push evaluation, effective permissions, policy compilation; 300 policies, 200 grants), `codesearch` (queries over a 2,000-file warm index, index build). |
| `--pprof` | Go runtime profiles of a running server for server admins (§7). |

Percentiles are computed over successful operations only; failures are
counted separately and make the bench exit non-zero. Throughput is
successful operations divided by the wall time of the workload phase.

## 3. Measurements

Reference host: AMD Threadripper PRO 9995WX, Windows 11 Pro, NTFS, Go
1.27.1, loopback; file system calls are slow on this host (stat 50–250 µs),
so absolute disk-bound numbers are pessimistic.

| Scenario | v0.7 foundation | v0.7 (§15.10) |
|---|---:|---:|
| Push p50, 8 workers × 200 files × 4 KiB | 2658 ms | 1409 ms |
| Push p50, 1 worker × 200 files × 4 KiB | 906 ms | 486 ms |
| Clone p50, 1 worker, 200 files × 4 KiB | 1059 ms | 796 ms |
| Incremental push p50 (10 of 5,000 files changed), 4 workers | 828 ms | 146 ms |
| Initial push, 5,000 files × 512 B, 4 workers | 40.3 s | 26.9 s |
| Code search p50, 8 workers, 9 trees, 300 policies | 46.4 ms | 23.8 ms |
| Code search p50, 8 workers, 9 trees, no policies | 3.9 ms | 3.9 ms |
| `Push` RPC, 10k-entry manifest, disk store (Go benchmark) | 1,152 ms | 8.1 ms |
| `Authorize`, 300 policies (Go benchmark) | ≈470 µs | ≈120 µs |
| Manifest decode, 10k entries | 3.4 ms | 3.4 ms |
| Manifest diff, 10k entries, 10 changed | 0.11 ms | 0.11 ms |

## 4. Performance-relevant behaviour

### 4.1 Object transfer

- A client splits object uploads and downloads into up to **4 concurrent
  streams** (`UploadObjects` / `DownloadObjects` calls) over one HTTP/2
  connection. Batches hold at most 1,024 objects / 128 MiB (upload) or
  2,048 hashes (download); a single batch is halved while each part keeps
  at least 32 objects. The server processes each stream sequentially, so
  concurrency comes from streams.
- `Negotiate` and object uploads skip the objects referenced by the
  manifest of a parent snapshot the server is known to have (reachable
  from a server branch tip): an incremental push offers and uploads only
  what changed.

### 4.2 Push validation

The server checks that every object referenced by a pushed snapshot is
stored, except the blobs (non-chunked entries) of the manifest of a
frontier snapshot that is currently the tip of a branch of the tree: such
a snapshot was accepted with all its objects present, and objects
reachable from a branch tip are never garbage-collected. New blobs,
chunked entries and parents that are not a tip are always checked. The
decoded parent manifest is reused for the changed-path computation
(license and path checks).

### 4.3 Authorization

- Compiled policies are cached per tenant in the authorizer together with
  a copy of their source; the cache is used only while the store returns
  identical policies, so policy changes apply to the next evaluation.
- A request cache (`authz.WithRequestCache`) memoizes the caller's
  account, memberships, teams, tree grants and tenant policy lists for one
  unit of work. It is attached by `Push` (all checks of one push), path
  ACL evaluation, and the read-only code search handler (one IAM read per
  search instead of two per tree).

## 5. Limits

| Limit | Value | Where |
|---|---|---|
| gRPC message size (both directions) | 64 MiB | `server.maxGRPCMessage`, SDK `MAX_MESSAGE_SIZE` |
| Largest object per `ObjectData` | 64 MiB (client: 64 MiB − 4 KiB) | `syncsvc.MaxObjectSize`, SDK `MAX_OBJECT_SIZE` |
| Files chunked above | 8 MiB (FastCDC) | SDK `CHUNK_THRESHOLD` |
| Largest file | 4 GiB | `content.MaxFileSize` |
| Snapshots per push | 100,000 | `syncsvc.maxPushSnapshots` |
| Hashes per `Negotiate`/`DownloadObjects` request | 1,000,000 (client sends ≤ 2,048) | `syncsvc.maxHashesPerRequest`, SDK `HASH_BATCH` |
| Objects / bytes per upload stream (client) | 1,024 / 128 MiB | SDK `UPLOAD_BATCH_OBJECTS`, `UPLOAD_BATCH_BYTES` |
| Concurrent transfer streams per operation (client) | 4 | SDK `TRANSFER_STREAMS` |
| HTTP/2 concurrent streams per connection | 250 | `server.newHTTPServer` |
| HTTP/2 receive buffer | 16 MiB per connection, 8 MiB per stream | `server.newHTTPServer` |
| HTTP header read timeout / idle timeout / max header bytes | 10 s / 2 min / 1 MiB | `server.newHTTPServer` (no read/write timeout: transfers and SSE are long-lived) |
| JSON request body | 1 MiB | `httpapi.maxJSONBody` |
| Rate limit | 1,200 requests/min per principal or anonymous IP (`/healthz`, `/metrics` exempt) | `--rate-limit` |
| Policy statements per policy | 100 | `httpapi.maxPolicyStatements` |
| Declarative access documents | 1 MiB, 200 roles, 200 policies, 100 statements | `accesscfg` |
| Code search | files ≤ 1 MiB indexed, pattern ≤ 1,000 bytes, ≥ 3 literal characters, limit ≤ 1,000 hits (default 50), context ≤ 10 lines, 5 s budget, lines clipped at 1,000 bytes | `codesearch` |
| Blame | files ≤ 32 MiB | `blame.MaxFileSize` |
| Diff / compare | 1 MiB per diffed file, 32 MiB per compared blob, 250 snapshots per compare | `httpapi/insights.go` |
| Admin stats cache | 10 s | `httpapi.statsCacheTTL` |
| Compiled-policy cache | 4,096 tenants (then restarted) | `authz.maxCachedTenants` |
| Event bus | 64 buffered events per subscriber, 200 in history | `events` |

## 6. Tuning flags

| Flag (env) | Default | Effect on performance |
|---|---|---|
| `--rate-limit` (`WT_RATE_LIMIT`) | 1200 | Requests per minute per principal; 0 disables. Load tests from one account need 0 or a higher value. |
| `--compress-objects` (`WT_COMPRESS_OBJECTS`) | true | zstd (WTZ1) storage: less disk, more CPU on write and read. |
| `--gc-interval` / `--gc-grace` (`WT_GC_INTERVAL` / `WT_GC_GRACE`) | 24h / 720h | Garbage collection walks every tree; schedule it off-peak. |
| `--staged-retention` (`WT_STAGED_RETENTION`) | 168h | Bounds staged-snapshot storage. |
| `--audit-retention` (`WT_AUDIT_RETENTION`) | 0 (keep) | Bounds the audit log size. |
| `--default-max-trees` / `--default-max-storage` | 0 (unlimited) | Quotas; storage quotas add a size computation per push. |
| `--pprof` (`WT_PPROF`) | false | Enables `/debug/pprof` for server admins (§7). No cost while profiles are not being taken. |
| `--log-level` (`WT_LOG_LEVEL`) | info | `debug` adds detail logs (e.g. every rejected object); keep `info` under load. |

Clients: gRPC gzip compression of object transfers is on by default and
falls back automatically when a server cannot decode it.

## 7. Runtime profiling

With `--pprof`, `GET /debug/pprof` (HTML index with absolute links) and
`GET /debug/pprof/{profile}` (`heap`, `allocs`, `goroutine`, `block`,
`mutex`, `threadcreate`, `profile`, `trace`, `cmdline`, `symbol`, served by
Go's `net/http/pprof`) are available to server admins (`server:admin`);
anonymous callers get 401 and other users 403. Without the flag both
routes answer 404 `not_found` and do not reveal that they exist. Both are
documented in `openapi.yaml` (tag Admin, `x-wt-action: server:admin`).
`worktree-bench --cpu-profile FILE --profile-workload W` captures a CPU
profile during a workload.

## 8. Implementation status

| Item | Status |
|---|---|
| `worktree-bench` load generator (push, clone, search; table and JSON reports; own or existing server; CPU profile capture; `--policies`) | Built |
| Go benchmarks for push validation, manifest diffing, code search, authorization | Built |
| `--pprof` admin-only `/debug/pprof` endpoints | Built |
| Parallel object transfer streams (client) | Built |
| Incremental object checks on push (server) and negotiation of changed objects only (client) | Built |
| Compiled-policy cache; request cache for code search | Built |
| Batched object durability (one sync per upload) | Planned |
| Decoded-manifest cache, single `Has` per uploaded object | Planned |
| Continuous benchmark tracking in CI | Planned |

## 9. Roadmap

- Batch fsync of uploaded objects (sync once per upload stream before
  acknowledging) behind an object-store API.
- LRU cache of decoded manifests keyed by hash for push validation,
  quota deltas and download reachability.
- Bounded parallelism inside `Negotiate` and `DownloadObjects` for first
  pushes and clones of large trees.
- Cache decoded policy lists in the metadata store (invalidated on write).
- Record `worktree-bench --json` results in CI on a fixed runner and flag
  regressions.

## 10. References

- `docs/ENGINEERING_PLAN.md` §15.10 — binding contract
- `docs/performance.md` — how to run, full before/after tables, profiles
- `crates/worktree-bench/` — load generator
- `services/server-go/internal/syncsvc/push_bench_test.go`, `internal/authz/authorizer_bench_test.go`, `internal/codesearch/search_bench_test.go` — benchmarks
- `services/server-go/internal/httpapi/pprof.go` — profiling endpoints
- `services/server-go/internal/syncsvc/push.go` — `trustedBlobs`
- `services/server-go/internal/authz/policycache.go` — compiled-policy cache
- `crates/worktree-sdk/src/remote/client.rs`, `remote/push.rs` — parallel transfer, negotiation of changed objects
- [`Server.md`](./Server.md) — operations (TLS, GC, quotas, rate limits, metrics)
- [`../sync/Sync.md`](../sync/Sync.md) — sync protocol
