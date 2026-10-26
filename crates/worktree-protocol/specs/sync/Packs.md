# Pack and Delta Transfer Specification

> **Version:** 0.8 — describes the pack format with binary deltas and the
> `UploadPack` / `QueryUpload` / `DownloadPack` RPCs as built
> (`crates/worktree-protocol/src/pack/`,
> `services/server-go/internal/syncsvc/pack_*.go`,
> `crates/worktree-sdk/src/remote/pack.rs`, `docs/ENGINEERING_PLAN.md`
> §16.1). Features described but not built are listed under
> [Roadmap](#roadmap).

## Overview

Before v0.8 every object travelled whole: a one-line edit to a 100 KB file
re-sent 100 KB (gzip-compressed by gRPC). v0.8 adds **packs**: one byte
stream carrying many objects, each either whole or as a **copy/insert
delta** against an object the receiver already has, each payload
optionally zstd-compressed, with a BLAKE3 trailer over the whole stream.

- **Push** uploads the missing objects as one pack (`UploadPack`), with
  deltas against the server-known parent snapshot. Uploads are
  **resumable**: an interrupted upload continues from the bytes the server
  kept (`QueryUpload`).
- **Fetch, pull and clone** download the hinted objects as one pack
  (`DownloadPack`), with deltas against the snapshot the client already
  has, and between versions inside the pack (a clone's history).
- Servers store objects **whole** (WTZ1, Compression.md) after
  reconstructing and verifying them; deltas exist only on the wire.
- Clients fall back to `UploadObjects` / `DownloadObjects` when a server
  answers `Unimplemented` (pre-v0.8 servers, `--disable-packs`).

The object model (Storage.md) is unchanged: hashes are BLAKE3 of the
uncompressed content, so a pack never changes an object's id.

---

## 1. Pack format

```text
pack    = header entry* end trailer
header  = "WTPK" version:u8 (=1) reserved:3×u8 (=0)
entry   = type:u8 kind:u8 codec:u8 hash:32 [base:32 if type = DELTA]
          size:uvarint raw_len:uvarint pay_len:uvarint payload:pay_len
end     = 0x00
trailer = BLAKE3(header .. end):32
```

| Field | Meaning |
|---|---|
| `type` | `1` WHOLE (the decoded payload is the object), `2` DELTA (the decoded payload is a delta against `base`), `0` end of entries |
| `kind` | The `ObjectKind` wire value: `0` unspecified, `1` blob, `2` manifest, `3` chunk list |
| `codec` | `0` raw, `1` one zstd frame that decodes to exactly `raw_len` bytes |
| `hash` | BLAKE3 of the object content |
| `base` | Hash of the delta base (DELTA only) |
| `size` | Object size; at most 64 MiB |
| `raw_len` | Decoded payload length (WHOLE: equal to `size`) |
| `pay_len` | Payload bytes that follow (raw codec: equal to `raw_len`) |

Integers are unsigned LEB128 varints (protobuf encoding, at most 10
bytes). Readers reject: a bad magic or version, non-zero reserved bytes,
unknown type/kind/codec, objects over 64 MiB, payloads over 65 MiB,
inconsistent lengths, zstd frames that do not decode to `raw_len` bytes,
delta chains deeper than 8, a trailer mismatch (`checksum`), and any byte
after the trailer. A stream that ends before the trailer is **truncated**
(an interrupted upload), which is distinct from corrupt.

**Bases** are objects the receiver already has or earlier entries of the
same pack. **Delta depth**: an object the receiver has is depth 0, a WHOLE
entry is depth 0, a DELTA entry is its base's depth plus one, at most
**8**. Per-entry problems — a base the receiver does not have (or may not
use), a delta that does not apply, content that does not match `hash` —
make only that entry unusable; the rest of the pack is still processed.

**Writers** (both implementations):

- compress a payload with zstd (level 3) when it is at least 64 bytes and
  the frame is smaller; otherwise store it raw;
- try a delta only when a base is offered, the object is at least 16 bytes,
  the base is at most 16 MiB (bounded window) and the chain stays within
  depth 8; keep the delta only when it saves **at least 25 %**
  (`4 × delta_len ≤ 3 × object_len`, comparing raw lengths so the choice
  does not depend on the zstd implementation);
- skip objects already written to the same pack.

### Deltas

```text
delta  = base_len:uvarint target_len:uvarint op*
op     = 0x00 len:uvarint bytes:len            INSERT (len ≥ 1)
       | 0x01 offset:uvarint len:uvarint        COPY base[offset .. offset+len] (len ≥ 1)
```

Applying checks `base_len` against the base, every copy range, every
insert length, and that the output is exactly `target_len` bytes.

The encoder is deterministic and specified exactly, so the Rust and Go
writers produce identical bytes:

1. If the base or the target is shorter than 16 bytes, or the base is
   longer than 16 MiB, the delta is a single INSERT of the target (no op
   for an empty target).
2. **Index**: for every block-aligned base offset `i = 0, 16, 32, …` with
   `i + 16 ≤ base_len`, map `fingerprint(base[i .. i+16])` to `i`; the
   first offset wins.
3. **Scan** the target with a rolling fingerprint from `p = 0` (pending
   insert start `s = 0`) while `p + 16 ≤ target_len`: when the window's
   fingerprint is indexed at `o` and `base[o .. o+16]` equals the window,
   extend the match backwards (not before `s`, not before base offset 0)
   and forwards (while both sides agree), emit the pending INSERT
   `target[s .. start]` if non-empty and a COPY of the match, and continue
   after the match with a fresh fingerprint; otherwise roll the window by
   one byte.
4. Emit the remaining `target[s ..]` as an INSERT.

`fingerprint(w) = Σ w[j] · P^(15−j) mod 2³²` with `P = 16777619`; rolling
removes `w[0] · P^15`, multiplies by `P` and adds the next byte (wrapping
32-bit arithmetic).

### Compatibility vectors

`crates/worktree-protocol/testdata/pack/vectors.json` holds nine delta
vectors (empty target, short inputs, identical, edited, prepend/append,
edited text, repetitive, unrelated, shuffled blocks) and three pack vectors
(empty, mixed kinds with in-pack and external bases and a duplicate, a
12-step chain that hits the depth limit). Both test suites
(`src/pack/tests.rs`, `syncsvc/pack_format_test.go`) check that their
encoder reproduces every delta and every raw (uncompressed) pack byte for
byte, and decode the raw pack, the Rust-written zstd pack and the
Go-written zstd pack of each vector (zstd output differs between
implementations, so compressed packs are compared by content).
Regenerate with `WT_UPDATE_GOLDEN=1 cargo test -p worktree-protocol
pack::tests::golden` and then `WT_UPDATE_GOLDEN=1 go test
./internal/syncsvc -run PackGolden`.

## 2. RPCs

```protobuf
rpc UploadPack(stream PackChunk) returns (UploadPackResponse);
rpc QueryUpload(QueryUploadRequest) returns (QueryUploadResponse);
rpc DownloadPack(DownloadPackRequest) returns (stream PackChunk);

message PackChunk { bytes data = 1; string upload_id = 2; uint64 offset = 3; TreeId tree_id = 4; }
```

`PackChunk.data` is at most **4 MiB**; `offset` is the byte offset of
`data` within the pack, and offsets are contiguous within a stream. Packs
are already compressed, so pack RPCs do not use gRPC gzip.

### `UploadPack`

- **Authorization**: the caller must be authenticated (like
  `UploadObjects`). When the first chunk names `tree_id`, the caller needs
  `tree:write` on that tree; delta bases outside the pack must then be
  objects reachable from the tree's branch tips, tags or staged snapshots
  that the caller may read (the `DownloadObjects` rules). Without
  `tree_id` only bases inside the pack are accepted. Objects merely stored
  on the server (for example by a push that was later rejected) are not
  valid bases. These rules keep a delta from probing the content of
  objects the caller cannot read.
- **Upload id**: chosen by the client, 1–64 characters of `A–Z a–z 0–9 -
  _`; one stream per id at a time (`Aborted` otherwise).
- **Spooling and resume**: received bytes are appended to
  `<data>/uploads/<upload_id>.pack`, and a `pack_uploads` document (key:
  upload id; `{id, account_id, tree_id, created_at, updated_at}`) records
  the upload. A new upload must start at offset 0. A stream for an
  existing upload (same account; `PermissionDenied` otherwise) may start
  at any offset up to the bytes kept — the spool is truncated to it — but
  not beyond (`FailedPrecondition`). When a stream breaks off, the prefix
  stays for a resume.
- **Completion**: at a clean end of stream the pack is checked in one
  pass (framing, depth, trailer, and the external bases collected and
  authorized as above) and then unpacked: every entry is reconstructed,
  verified exactly like `UploadObjects` (BLAKE3, manifest and chunk-list
  validation) and stored whole, by eight workers in parallel. The response
  counts `objects_stored` (new), `objects_already_present`, lists
  `rejected` entries (unusable entries and failed verifications) and
  reports `bytes_received`. Spool file and document are removed.
- **Incomplete or corrupt**: a pack that ends before its trailer keeps the
  spool and fails with `FailedPrecondition` ("pack upload incomplete … resume
  at that offset"); a corrupt pack (framing or checksum) is discarded with
  `InvalidArgument`.
- **Limits**: chunks over 4 MiB are `InvalidArgument`; a pack larger than
  `--max-pack-size` (default 4 GiB) or than the remaining storage quota of
  the tree's tenant (`max_storage_bytes`, see Storage.md) is
  `ResourceExhausted` and discarded. The push itself still runs the exact
  quota check of Storage.md.
- **Cleanup**: uploads not touched for **24 hours** are removed (spool and
  document), as are spool files without a document; the server checks
  hourly. An account keeps at most **8** interrupted uploads: starting a
  ninth drops its least recently touched one, which bounds the spool space
  an account can use.

### `QueryUpload`

Returns `{found, offset}`: the bytes kept for an upload of the caller.
Uploads of other accounts and unknown ids are `found = false`.

### `DownloadPack`

```protobuf
message DownloadPackRequest { TreeId tree_id = 1; repeated ContentHash want = 2;
                              repeated ContentHash have = 3; bool allow_deltas = 4; }
```

- The caller needs read access to the tree (anonymous callers can read
  public trees). Served are the objects of `want` that are reachable from
  the tree and readable by the caller — the `DownloadObjects` rules,
  including license-restricted paths — plus the chunks of every wanted
  chunk list, minus the objects in `have`. Other hashes are skipped.
- With `allow_deltas`, bases are chosen per object: a blob or chunk list
  against the object at the same path in the previous manifest (the base
  manifests are the first 16 `have` hashes that are manifests, then each
  wanted manifest in request order), a chunk against the chunk at the same
  index of that path's previous chunk list, a manifest against the previous
  manifest. A base from `have` must itself be reachable and readable;
  otherwise the object is sent whole. Entries are ordered so that in-pack
  bases precede their deltas.
- The pack streams as `PackChunk` messages of at most 4 MiB with
  increasing offsets.

### Disabling

`worktree-server --disable-packs` (env `WT_DISABLE_PACKS`) answers all
three RPCs with `Unimplemented`; `--max-pack-size SIZE` (env
`WT_MAX_PACK_SIZE`, e.g. `4GiB`) bounds uploads.

## 3. Client behaviour

**Push** (`remote/push.rs`, `remote/pack.rs`): after `Negotiate`, the
missing objects are ordered snapshot by snapshot (oldest first: chunks,
chunk lists and blobs of each manifest, then the manifest) and each gets a
base candidate — the object at the same path in the snapshot's first
parent (for chunks: the same index of that path's chunk list), the parent
manifest for a manifest. A candidate is used only when it is an object of a
server-known snapshot (one reachable from a server branch tip) or an
earlier entry, and stored locally. The pack is written to a temporary file
(so it can be resent from any offset) and uploaded with a fresh upload id
and the tree id. Then:

- `Unimplemented`: the client remembers that the server has no packs for
  the rest of the session and uploads whole objects.
- A failure that may be transient (`Unavailable`, `Aborted`, `Cancelled`,
  `DeadlineExceeded`, `Internal`, `Unknown`, `FailedPrecondition`): the
  client resumes **once** — `QueryUpload`, then a new stream from the
  offset kept (from 0 when nothing was kept). If the server still holds
  the interrupted stream (`Aborted`), the resume waits briefly and retries.
- Other errors (`PermissionDenied`, `ResourceExhausted`,
  `InvalidArgument`, …) fail the push.
- Entries the server rejected are uploaded again whole with
  `UploadObjects`, which reports real corruption.

**Fetch, pull, clone** (`remote/fetch.rs`): for each branch, the objects
of the `Pull` hint list that are not stored locally are requested with one
`DownloadPack`; `have` lists the manifest of the snapshot the client
already has for that branch, then its entries and chunks (at most 100,000
hashes). Objects are verified and stored as they arrive; unusable entries
are skipped. The existing completeness checks then download anything
still missing with `DownloadObjects` (all of it on servers without packs).
Sparse (partial) clones keep using `DownloadObjects`.

**Switches**: `WT_NO_PACK=1` (also `true`/`yes`) disables packs for push,
fetch, pull and clone; `wt push --no-pack` sets it for one push.
Progress output is unchanged; `remote::push_with_stats` reports how a push
travelled (`TransferStats`: pack or not, objects, deltas, object bytes,
pack bytes, resumed).

**Testing hook**: `WT_PACK_TEST_INTERRUPT_AFTER=N` makes the first upload
stream of a push stop after `N` bytes (in 64 KiB chunks), wait 300 ms and
reset the stream, so the resume runs against a real server (used by the
e2e scenario).

## 4. Metrics

`GET /metrics` (Server.md) renders, per direction (`upload`, `download`):

| Metric | Meaning |
|---|---|
| `worktree_pack_bytes_total{direction}` | Pack bytes transferred |
| `worktree_pack_object_bytes_total{direction}` | Size of the objects the packs carried (what whole-object transfer sends before gRPC gzip) |
| `worktree_pack_bytes_saved_total{direction}` | Object bytes minus pack bytes (deltas plus compression) |
| `worktree_pack_deltas_total{direction}` | Objects transferred as deltas |
| `worktree_pack_upload_resumes_total` | Uploads continued at a non-zero offset |

Measurements: `docs/performance.md` §7 (`worktree-bench --workload
edits`).

## Implementation Status

| Item | Status |
|---|---|
| Pack format, copy/insert deltas, depth limit, trailer (Rust and Go) | Built (v0.8 §16.1) |
| Cross-language golden vectors (byte-for-byte deltas and raw packs; zstd packs decoded both ways) | Built (v0.8 §16.1) |
| `UploadPack` with spool, `pack_uploads`, resume, size and quota limits, base authorization | Built (v0.8 §16.1) |
| `QueryUpload` | Built (v0.8 §16.1) |
| `DownloadPack` with path-based base selection and chunk-list expansion | Built (v0.8 §16.1) |
| 24 h cleanup of interrupted uploads | Built (v0.8 §16.1) |
| SDK push / fetch / pull / clone over packs, `Unimplemented` fallback, one resume, `WT_NO_PACK`, `wt push --no-pack` | Built (v0.8 §16.1) |
| Pack metrics (`worktree_pack_*`) | Built (v0.8 §16.1) |
| `--disable-packs`, `--max-pack-size` | Built (v0.8 §16.1) |
| Packs for sparse (partial) clones and lazy fetches | Roadmap |

## Roadmap

- **Thin packs for sparse clones and on-demand fetches**: partial clones
  and the lazy fetcher still download whole objects.
- **Resume across processes**: the upload id lives only for one `wt push`;
  a push restarted after a crash starts a new upload (the server cleans the
  old one after 24 h).
- **Resumable downloads**: an interrupted `DownloadPack` is restarted by
  the next fetch (objects already stored are not requested again).
- **Better base selection**: renames and copies (content similarity), more
  than the first parent, and bases for files whose path is new.
- **Streaming pack construction**: the client writes the whole pack before
  uploading, and the server checks a pack in a full pass before unpacking.
- **Server-side packed storage** (objects stay whole on disk; see
  Storage.md).

## References

- `crates/worktree-protocol/src/pack/` — `mod.rs` (format constants,
  varints), `delta.rs` (encoder, `apply`), `writer.rs` (`PackWriter`),
  `reader.rs` (`PackReader`), `tests.rs` (unit tests, golden vectors)
- `crates/worktree-protocol/testdata/pack/vectors.json`
- `services/server-go/internal/syncsvc/pack_format.go` (Go format),
  `pack_upload.go` (`UploadPack`, `QueryUpload`, spool, janitor),
  `pack_download.go` (`DownloadPack`), `pack_format_test.go`,
  `pack_test.go`
- `services/server-go/internal/opsmetrics/pack.go`,
  `services/server-go/internal/config/packs.go`
- `crates/worktree-sdk/src/remote/pack.rs` (`upload`, `download`, base
  selection, `TransferStats`), `remote/push.rs`, `remote/fetch.rs`
- `crates/worktree-cli/src/commands/sync.rs` — `wt push --no-pack`
- `crates/worktree-bench/src/edits.rs` — the `edits` workload
- Tests: `crates/worktree-sdk/tests/remote_packs.rs`,
  `crates/worktree-e2e/tests/e2e/packs.rs`
- Related: Sync.md (object transfer), Storage.md (object model, quotas),
  Compression.md (WTZ1, gRPC gzip)
