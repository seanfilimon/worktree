# Performance

How to load-test and profile a W0rkTree server, what was measured for
v0.7 (ENGINEERING_PLAN §15.10, workstream `hardening`), and which
hotspots were fixed. Limits and tuning flags are specified in
[`specs/server/Performance.md`](../crates/worktree-protocol/specs/server/Performance.md).

- [1. The load generator: `worktree-bench`](#1-the-load-generator-worktree-bench)
- [2. Go benchmarks](#2-go-benchmarks)
- [3. Profiling a running server (`--pprof`)](#3-profiling-a-running-server---pprof)
- [4. Measurements](#4-measurements)
- [5. Hotspots fixed](#5-hotspots-fixed)
- [6. Known hotspots not fixed yet](#6-known-hotspots-not-fixed-yet)
- [7. v0.8: pack and delta transfer](#7-v08-pack-and-delta-transfer)

---

## 1. The load generator: `worktree-bench`

`crates/worktree-bench` drives concurrent workloads through the
worktree-sdk async API in-process (no `wt` processes), so the numbers are
those of a real client: snapshotting, negotiation, object upload and
download, push and REST calls.

```sh
cargo build -p worktree-bench --release

# Start a throw-away server (WT_SERVER_BIN, target/{debug,release}/worktree-server,
# or `go build`), run every workload, stop the server.
target/release/worktree-bench --concurrency 8 --iterations 5 --files 200 --file-size 4k

# Against an existing server, with an existing account.
target/release/worktree-bench --server https://wt.example.com --user alice --password ... \
    --workload push,search --json
```

Without `--server` the bench starts the Go server on a free localhost port
with a temporary data directory, the flags the e2e harness uses and
`--rate-limit 0` (all requests come from one user), and kills that child
process on exit. Without `--user` it registers a fresh user (the server
must allow registration). `$WT_HOME` points at a temporary directory, so
the developer's own credentials are never read or written.

| Workload | What each worker does | Reported ops |
|---|---|---|
| `push` | Its own tree with `--files` files of `--file-size` bytes. Round 0 writes every file; later rounds rewrite `--changed` files (0 = all). Each round: local snapshot, then push. | `push` (MB/s = file bytes written that round), `snapshot (local)` |
| `clone` | Clones a seeded tree (`--files` × `--file-size`) `--iterations` times into fresh directories. | `clone` (MB/s = tree bytes) |
| `search` | One cold search first (builds the code index), then `--iterations` code searches per worker over the bench tenant's trees (`GET /api/v1/search/code?q=…&tenant=…`). Zero hits count as an error. | `search (cold)`, `search` |
| `edits` (v0.8, §7; not part of `all`) | Its own tree of `--files` source-like text files of `--file-size` bytes, pushed once; every later round inserts one line into `--changed` files (default 10) and pushes. Prints what went over the wire (pack bytes and deltas, or whole objects with `WT_NO_PACK=1`). | `edits: initial push`, `edits: push` (MB/s = bytes sent), `edits: snapshot (local)` |

Other flags: `--workload push,clone,search` (any subset, in order),
`--concurrency/-c`, `--iterations/-n`, `--policies N` (creates N
non-matching access policies in the bench tenant first, so every
authorization evaluates them), `--query`, `--server-bin`,
`--server-arg FLAG` (repeatable, passed to the started server), `--json`,
`--keep` (keep the temporary directory with worktrees, server data and
logs), and `--cpu-profile FILE [--profile-workload W] [--profile-seconds N]`
(see §3).

Each row reports count, errors, p50/p95/p99/max/mean latency (nearest-rank
percentiles of successful operations), throughput (successful operations
per second of the phase's wall time) and MB/s where meaningful. The
process exits non-zero when any operation failed and then prints the tail
of the started server's log. `--json` prints the configuration and the
rows as one JSON document instead of the table.

## 2. Go benchmarks

```sh
cd services/server-go
go test ./internal/syncsvc    -run '^$' -bench . -benchmem   # push validation, diffing, negotiate
go test ./internal/authz      -run '^$' -bench . -benchmem   # authorization evaluation
go test ./internal/codesearch -run '^$' -bench . -benchmem   # code search queries, index build

# CPU and allocation profiles of one benchmark
go test ./internal/authz -run '^$' -bench 'Authorize/tree:read' -benchmem \
    -cpuprofile cpu.out -memprofile mem.out -o authz.test
go tool pprof -top -cum authz.test cpu.out
go tool pprof -top -sample_index=alloc_space authz.test mem.out
```

| File | Benchmarks | Fixture |
|---|---|---|
| `syncsvc/push_bench_test.go` | `DecodeManifest`, `DiffManifests`, `Push`, `CheckObjects`, `Negotiate` | 10k-entry manifests (10 entries changed per push), the package fakes plus a map-backed or on-disk object store |
| `authz/authorizer_bench_test.go` | `Authorize` (tree:read, tree:write, mr:approve), `EvaluateBranchPush`, `EffectivePermissions`, `CompilePolicies` | 300 policies × 2 statements (user, team and tenant principals; some IP and time-window conditions), 200 tree grants, 50 teams |
| `codesearch/search_bench_test.go` | `Search` (rare/common literal, case folding, regex, path filter, no match), `IndexBuild` | 2,000 files, ~6 MB of Go-like source |

The `store=disk` sub-benchmarks write 20k objects per fixture and take a
few minutes on slow file systems; use `-bench 'memory'` for a quick run.

## 3. Profiling a running server (`--pprof`)

`worktree-server --pprof` (env `WT_PPROF=true`, default off) serves the Go
runtime profiles to **server admins** (others get 401/403; with the flag
off both routes answer 404 `not_found`, like unknown paths):

| Route | Content |
|---|---|
| `GET /debug/pprof` | HTML index with absolute links and sample counts |
| `GET /debug/pprof/{profile}` | `heap`, `allocs`, `goroutine`, `block`, `mutex`, `threadcreate` (gzipped protobuf, `?debug=1` text), `profile` (CPU, `?seconds=N`, default 30), `trace` (`?seconds=N`), `cmdline`, `symbol` |

```sh
TOKEN=$(curl -s -X POST http://localhost:7420/api/v1/auth/login \
    -H 'Content-Type: application/json' -d '{"username":"admin","password":"…"}' | jq -r .token)
curl -H "Authorization: Bearer $TOKEN" -o cpu.pprof "http://localhost:7420/debug/pprof/profile?seconds=20"
go tool pprof -top -cum worktree-server cpu.pprof
```

`block` and `mutex` stay empty unless the respective profile rates are set
in the process. The routes are rate-limited like the rest of the API.

The bench can capture a server CPU profile while it runs:
`--cpu-profile cpu.pprof --profile-workload search --profile-seconds 10`
(a started server gets `--pprof` and its admin is used; with `--server`
the bench user must be a server admin and the server must run with
`--pprof`). The capture starts with the named workload (default: the
first) and must fit in the 60 s REST timeout (`--profile-seconds` ≤ 50).

## 4. Measurements

Machine: AMD Ryzen Threadripper PRO 9995WX (96 cores, `GOMAXPROCS` 64),
Windows 11 Pro 10.0.26200, NTFS, Go 1.27.1, Rust 1.99.1 (bench built with
`--release`), server and bench on the same host over loopback. File system
calls are slow on this host (an `os.Stat` of an object takes 50–250 µs,
opening a file ~1 ms; the CPU profiles are ~90 % `runtime.cgocall` in file
system syscalls), and other builds were running at the same time, so treat
the numbers as relative: before/after pairs were run back to back with the
same flags. On Linux with a warm page cache the disk-bound numbers are an
order of magnitude lower.

### 4.1 End-to-end (`worktree-bench`, started server)

"Before" is the v0.7 foundation (`main` at the start of §15.10, client and
server); "after" includes the fixes of §5.

**S1** — `-c 8 -n 5 --files 200 --file-size 4k` (all workloads):

| op | p50 before | p50 after | p95 before | p95 after | ops/s before | ops/s after |
|---|---:|---:|---:|---:|---:|---:|
| push | 2658 ms | 1409 ms | 2754 ms | 1512 ms | 1.71 | 2.42 |
| clone | 1629 ms | 1580 ms | 1995 ms | 1638 ms | 3.85 | 3.98 |
| snapshot (local) | 1771 ms | 1628 ms | 1870 ms | 1872 ms | — | — |
| search (warm, 9 trees) | 3.9 ms | 3.9 ms | 4.9 ms | 4.7 ms | 1949 | 1921 |
| search (cold) | 2145 ms | 2128 ms | | | | |

Single worker (`-c 1 -n 5 --files 200 --file-size 4k --workload push,clone`):
push p50 906 → 486 ms, clone p50 1059 → 796 ms. With eight concurrent
clones the client side (writing 200 files per checkout) dominates, so the
parallel downloads barely show in S1.

**S2** — large tree, incremental pushes: `-c 4 -n 6 --files 5000
--file-size 512 --changed 10 --workload push` (round 0 pushes 5,000 files,
rounds 1–5 change 10 files each; p50 is an incremental push, max the
initial one):

| op | before | after |
|---|---:|---:|
| push p50 (incremental, 10 of 5,000 files) | 828 ms | 146 ms |
| push max (initial, 5,000 files) | 40.3 s | 26.9 s |
| ops/s | 0.29 | 0.36 |

**S3** — authorization-heavy search: `-c 8 -n 20 --files 20 --file-size 1k
--workload push,search --policies 300` (search over 9 trees of a tenant with
300 policies):

| op | before | policy cache only | after |
|---|---:|---:|---:|
| search p50 | 46.4 ms | 40.8 ms | 23.8 ms |
| search p95 | 54.3 ms | 52.1 ms | 33.1 ms |
| search ops/s | 169 | 189 | 317 |
| push p50 | 260 ms | 232 ms | 229 ms |

### 4.2 Go benchmarks

`ns/op` (lower is better); allocations per op in parentheses.

Push validation (`internal/syncsvc`, 10k-entry manifests, 10 changed):

| Benchmark | before | after |
|---|---:|---:|
| `Push/store=memory` | 7.40 ms (80k allocs) | 7.35 ms (80k allocs) |
| `Push/store=disk` | 1,152 ms (140k allocs) | 8.1 ms (80k allocs) |
| `CheckObjects/store=disk/parent=tip` | 459 ms | 5.6 ms |
| `CheckObjects/store=memory/parent=tip` | 2.5 ms | 5.4 ms ¹ |
| `CheckObjects/store=disk/parent=none` | 618 ms | 943 ms ² |
| `Negotiate/store=disk` (10k hashes) | 487 ms | 508 ms ³ |
| `DecodeManifest/entries=10000` | 3.4–3.8 ms, 2.4 MB, 40k allocs | unchanged |
| `DiffManifests/entries=10000/changed=10` | 0.11–0.21 ms, 5 allocs | unchanged |
| `DiffManifests/entries=10000/changed=10000` | 0.43–0.73 ms | unchanged |

¹ The step now decodes the parent's manifest, which the changed-path diff
of the same push needs anyway (it is cached in the push plan), so the whole
`Push` is unchanged in memory. ² Same code path (no trusted parent); the
difference is file-system noise. ³ Server code unchanged; the client now
offers only the changed objects (10 instead of 10,000 hashes).

Authorization (`internal/authz`, 300 policies, median of 3 runs):

| Benchmark | before | after |
|---|---:|---:|
| `Authorize/tree:read` | 492 µs (1,913 allocs) | 119 µs (201 allocs) |
| `Authorize/tree:write` | 419 µs | 117 µs |
| `Authorize/mr:approve` | 466 µs | 120 µs |
| `EvaluateBranchPush` (shared request cache) | 470 µs (2,094 allocs) | 135 µs (382 allocs) |
| `EffectivePermissions` | 704 µs (4,652 allocs) | 403 µs (2,940 allocs) |
| `CompilePolicies` (what a cache hit saves) | 254 µs (1,712 allocs) | unchanged |

Code search (`internal/codesearch`, warm index, 2,000 files; not changed):

| Benchmark | ns/op |
|---|---:|
| `Search/literal-rare` | 46 µs |
| `Search/literal-rare-fold` | 581 µs |
| `Search/literal-common` (50 hits) | 69 µs |
| `Search/literal-common-limit1000` | 583 µs |
| `Search/regex` | 495 µs |
| `Search/regex-fold-path` | 314 µs |
| `Search/no-match` | 1.2 µs |
| `IndexBuild` (cold, disk store) | 932 ms |

## 5. Hotspots fixed

### 5.1 Object transfer ran on one stream (client)

Server profiles of S1 showed 90 % of push time in `DiskStore.Put`
(`FlushFileBuffers` 33 %, `MoveFileEx` 25 %, `CreateFile` 12 %, `Stat`
11 %) and 84 % of clone time in `DiskStore.Get` (`CreateFile`), all inside
`UploadObjects` / `DownloadObjects`. The server handles one stream
sequentially and the client sent every batch (up to 1,024 objects) on a
single stream, so a push waited for each object's file operations one
after the other.

Fix (`crates/worktree-sdk/src/remote/client.rs`): uploads and downloads
split into up to `TRANSFER_STREAMS` = 4 concurrent streams (batches are
halved while each part keeps at least 32 objects; batch size limits still
hold), each on a clone of the client over the same HTTP/2 connection; the
gzip fallback still applies per stream. Result: S1 push p50 −47 %
(2658 → 1409 ms), single-worker push −46 %, single-worker clone −25 %.

### 5.2 Incremental pushes re-checked every file of the tree (server + client)

For a push of one snapshot on top of the branch tip, the server stat'ed
every blob of the new manifest (`checkObjects`) and the client offered
every blob to `Negotiate` (another stat each): 2 × 10,000 stats for a
10-file change to a 10k-file tree — over a second on this host
(`Push/store=disk` 1,152 ms).

Fixes:

- Server (`syncsvc/push.go`, `trustedBlobs`): blobs of the manifests of
  frontier snapshots that are the current tip of a branch of the tree are
  not stat'ed again — those snapshots were accepted with all objects
  present and objects reachable from a branch tip are never collected.
  Every other referenced object (new blobs, chunked entries, parents that
  are not a tip) is still checked; tests in `push_trusted_test.go`.
- Client (`remote/push.rs`, `objects_for`): objects referenced by the
  manifest of a server-known parent are neither negotiated nor uploaded.

Result: `Push/store=disk` 1,152 → 8.1 ms; S2 incremental push p50
828 → 146 ms (−82 %).

### 5.3 Policies were recompiled and re-read for every check (server)

`Authorize` with 300 policies spent ~80 % in `compilePolicies` (resource
parsing, `time.LoadLocation`, allocations) on every call made without a
request cache — e.g. once per tree in a code search. After caching, the
search profile showed `meta.(*Bolt).ListPolicies` (JSON-decoding all 300
policies) at 55 %, called twice per tree (tree authorization and the
path-deny filter).

Fixes:

- `authz/policycache.go`: the Authorizer keeps the last compilation per
  tenant with a deep copy of the source policies and reuses it only while
  the store returns identical policies (field-by-field comparison), so
  updates apply on the next evaluation. A test fails when a policy type
  gains a field the comparison does not cover.
- `httpapi/codesearch.go` (one line, via `httpapi/hardening.go`): the
  read-only code search handler runs with an authorization request cache,
  so memberships, grants and policy lists are read once per search.

Result: `Authorize` 4× faster (≈470 → ≈120 µs, 10× fewer allocations); S3
search p50 46.4 → 23.8 ms, throughput 169 → 317 searches/s.

## 6. Known hotspots not fixed yet

- **Per-object fsync and rename** in `objstore.DiskStore.Put` remain the
  largest server cost of uploads (42 % `FlushFileBuffers` in the S3 push
  profile). Batching durability (write all objects of an upload, then sync
  once before acknowledging) needs an object-store API change.
- **Redundant stat on upload**: `syncsvc.storeObject` calls `Has` and
  then `DiskStore.Put` calls `Has` again for every new object (two stats
  per object). Dropping the first check (Put is idempotent) saves ~10 % of
  upload time. Not changed here because `syncsvc/objects.go` belonged to
  another workstream during §15.10.
- **Manifest decoding** (`decodeManifest`: 3.4 ms and 40k allocations for
  10k entries) runs several times per push for the same immutable manifest
  (object checks, changed paths, quota delta in `limits.pushDelta`,
  download reachability). A small LRU of decoded manifests keyed by hash in
  `loadManifest` (`syncsvc/objects.go`) would remove most of it.
- **`Negotiate` and `DownloadObjects` stat/read sequentially** per request;
  bounded parallelism inside the handlers would help first pushes and
  clones of large trees beyond the client's four streams.
- **`ListPolicies` decoding** still happens once per request (and per
  Authorize outside request-cached handlers); caching decoded policies in
  the metadata store would remove it.
- **Local snapshotting** (`snapshot (local)` ≈ 1.6 s for 200 files under
  load on this host) is client file-system bound and was not profiled.

## 7. v0.8: pack and delta transfer

ENGINEERING_PLAN §16.1 (workstream `pack-delta`): push, fetch, pull and
clone move objects as one pack with copy/insert deltas and zstd payloads
(format and RPCs in
[`specs/sync/Packs.md`](../crates/worktree-protocol/specs/sync/Packs.md)).
"Before" is the whole-object path (`UploadObjects`, gzip-compressed gRPC
messages, four parallel streams), selected with `WT_NO_PACK=1` against the
same build; "after" is the default.

```sh
cargo build -p worktree-bench --release
# after (packs)
target/release/worktree-bench -c 1 -n 6 --files 500 --file-size 100k --changed 10 --workload edits
# before (whole objects)
WT_NO_PACK=1 target/release/worktree-bench -c 1 -n 6 --files 500 --file-size 100k --changed 10 --workload edits
```

The tree is 500 files × 100 KiB = 51.2 MB of source-like text (random
words; zstd compresses it about 3×). Round 0 pushes it; rounds 1–5 each
insert one line into 10 files and push. Same machine and caveats as §4
(other builds were running; the pairs were run back to back).

**Bytes** (exact; the same objects are transferred either way):

| push | objects | object bytes (whole-object transfer, before gzip) | pack bytes (after) |
|---|---:|---:|---:|
| initial (500 files + manifest) | 501 | 51,233,000 | 17,213,841 (zstd, no deltas) |
| one small-edit push (10 files + manifest) | 11 | 1,057,180 | ~1,630 (11 deltas) |
| five small-edit pushes | 55 | 5,285,900 | 8,123–8,171 |

A small-edit push sends ~650× fewer bytes: the ten edited 100 KiB files
and the 33 KB manifest travel as eleven delta entries of ~150 bytes each,
most of which is the entry header (object and base hash).
The whole-object path sends the same 1.06 MB per push, reduced only by
gzip (about the same 3× as zstd for this content).

**Time** (`worktree-bench` latencies; two back-to-back pairs):

| op | before (pair 1 / 2) | after (pair 1 / 2) |
|---|---:|---:|
| initial push (51 MB) | 1756 / 2761 ms | 1852 / 3285 ms |
| small-edit push p50 | 187 / 202 ms | 88 / 149 ms |
| small-edit push p95 | 206 / 253 ms | 108 / 186 ms |

A third pair, run while other builds saturated the host, measured 3374 vs
16937 ms (initial) and 217 vs 578 ms (p50) and is not used.

Small-edit pushes are 25–50 % faster on loopback, where bandwidth is not
the limit; over a real network the ~650× byte reduction dominates. The
initial push is about as fast as before: an instrumented run spent 0.82 s
building the pack on the client (reading and recompressing every object on
one thread) and 1.08 s unpacking on the server (one verification pass plus
eight parallel store workers — before they were added, unpacking stored
objects one by one and the initial push took 6.9 s). Known follow-ups:
build the pack on several threads and stream it while it is written, and
skip the server's separate verification pass for packs without external
bases.

Server counters (`GET /metrics`, Packs.md §4) report the same numbers in
production: `worktree_pack_bytes_total`, `worktree_pack_object_bytes_total`,
`worktree_pack_bytes_saved_total` and `worktree_pack_deltas_total` per
direction, and `worktree_pack_upload_resumes_total`.
