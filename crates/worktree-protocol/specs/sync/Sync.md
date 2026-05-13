# Sync Protocol Specification

## Overview

The sync protocol governs communication between the bgprocess (local) and the W0rkTree server (remote). It handles staged snapshot uploads, branch pushes, branch pulls, access config sync, tag sync, and large file chunk transfers. Transport: gRPC over QUIC (with HTTP/2 fallback).

## Key Concepts

- **Staged sync** (automatic): bgprocess syncs local snapshots to server as "staged" for team visibility. Automatic, runs on configured interval.
- **Branch push** (explicit): developer runs `wt push` to move staged snapshots into branch history. Explicit, goes through conflict detection.
- **Branch pull** (automatic): bgprocess receives remote branch updates from server. Automatic.
- These three operations are DISTINCT. Staged sync ≠ branch push.

## Sync Flow

### Staged Snapshot Upload

1. BGProcess creates snapshot (auto or manual)
2. BGProcess checks if auto-sync is enabled
3. If enabled, BGProcess uploads snapshot objects (blobs, tree, manifest) to server
4. Server stores as "staged snapshot" — visible to team but NOT part of any branch
5. Server indexes staged snapshot by user, branch, tree
6. Other users see staged activity via `wt status --team`, `wt staged`, admin panel

#### Sequence

```
Developer          BGProcess              Server
   |                   |                     |
   | (edits files)     |                     |
   |------------------>|                     |
   |                   | snapshot created    |
   |                   |-------------------->|
   |                   | upload objects      |
   |                   |-------------------->|
   |                   |       ACK (staged)  |
   |                   |<--------------------|
   |                   |                     |
   |                   |   (team can now see |
   |                   |    staged activity) |
```

#### Upload Protocol

- BGProcess computes object set: blobs (file content), tree manifest, snapshot metadata
- BGProcess sends object IDs to server first (have/want negotiation)
- Server responds with which objects it needs (dedup — many objects already exist)
- BGProcess uploads only new objects
- Server writes objects to content-addressable store
- Server creates staged snapshot record pointing to the objects
- Server returns ACK with staged snapshot ID

#### Prototype HTTP Contract

The Rust prototype and Go server currently expose a REST compatibility endpoint for staged upload:

```
POST /staged
```

The request uploads one SDK snapshot plus the added/modified file objects needed by that snapshot.
The server validates each uploaded file by:

- requiring a relative path
- decoding base64 content
- checking byte size
- recomputing BLAKE3 and comparing it to the declared hash
- writing content-addressed objects to server storage
- recording `StagedSnapshot` metadata in a server-side staged index

This prototype endpoint is deliberately not authoritative IAM design. It exists to stabilize the
client/bgprocess contract while the production Go server is planned. The production service should
keep the same semantic boundary: bgprocess uploads staged snapshot objects; the server verifies,
authorizes, stores, indexes, and ACKs only after durable persistence.

The current Rust request shape is compatible with Go:

- `snapshot_id`
- `tenant`
- `worktree`
- `tree_id`
- `branch`
- `objects[]`

Each object contains `path`, `hash`, `size`, and `content`. JSON `content` is base64 text decoded
by Go into bytes. The previous `content_base64` field is obsolete.

The Go server also exposes this flow through protobuf:

```proto
service SyncService {
  rpc StageSnapshot(StageSnapshotRequest) returns (StageSnapshotResponse);
  rpc ListStagedSnapshots(ListStagedSnapshotsRequest) returns (ListStagedSnapshotsResponse);
}
```

The gRPC service shares the same object store, staged store, audit recorder, and IAM authorizer as
REST.

#### Auto-Sync Behavior

- BGProcess runs on a configurable interval (default: 30 seconds)
- Each cycle: check for new local snapshots since last sync
- If new snapshots exist: upload them
- If no new snapshots: no-op (no traffic)
- BGProcess tracks "last synced snapshot" per branch per tree

### Explicit Branch Push

1. Developer runs `wt push`
2. BGProcess sends push request to server with snapshot IDs to push
3. Server evaluates:
   a. Branch protection rules (no_direct_push? merge request required?)
   b. IAM check (does user have sync:push permission?)
   c. Conflict detection (has the branch tip moved since the user's last pull?)
4. If conflict: reject push, user must merge first
5. If OK: server advances branch tip to include the pushed snapshots
6. Server notifies other bgprocess clients of branch update

#### Push Request

```
PushRequest {
    branch: BranchName,
    tree: TreePath,
    snapshot_ids: Vec<SnapshotId>,      // Ordered list of snapshots to push
    expected_tip: Option<SnapshotId>,   // The branch tip the user expects (for conflict detection)
    force: bool,                        // Force push (requires elevated permission)
}
```

#### Push Response

```
PushResponse {
    status: PushStatus,                 // Accepted, Rejected, ConflictDetected
    new_tip: Option<SnapshotId>,        // New branch tip after push
    conflict: Option<ConflictInfo>,     // Details if conflict detected
    message: String,                    // Human-readable status message
}
```

#### Conflict Detection

- Server compares `expected_tip` with actual branch tip
- If they match: no conflict, push proceeds
- If they differ: conflict detected, push rejected
- User must pull, merge, then retry push
- `force: true` bypasses conflict detection (requires `sync:force_push` permission)

### Branch Pull (Automatic)

1. Server notifies bgprocess of branch update (or bgprocess polls on interval)
2. BGProcess requests delta: "I have up to snapshot X, give me what's new"
3. Server sends new snapshot objects
4. BGProcess applies to local history
5. If local has uncommitted changes on the same branch: BGProcess attempts auto-merge (see merge spec)

#### Pull Request

```
PullRequest {
    branch: BranchName,
    tree: TreePath,
    last_known_tip: SnapshotId,         // Last snapshot the client knows about
}
```

#### Pull Response

```
PullResponse {
    snapshots: Vec<Snapshot>,           // New snapshots since last_known_tip
    objects: Vec<Object>,               // New objects needed by those snapshots
    new_tip: SnapshotId,                // Current branch tip
    has_more: bool,                     // If true, client should request more (pagination)
}
```

#### Server-Initiated Notifications

- Server pushes branch update notifications over persistent connection
- BGProcess subscribes to branches it cares about
- Notification contains: branch name, new tip, who pushed
- BGProcess then issues PullRequest to fetch actual data

## Canonical Push/Pull (v1 REST Wire Contract)

The v1 canonical sync surface uses REST over HTTP/2 with bearer-token auth (same `authMiddleware` as `/staged`). gRPC parity is deferred. All endpoints live under `/api/` to separate them from the prototype `/staged` flow.

### Endpoints

```
POST   /api/push
POST   /api/pull
POST   /api/objects/check
GET    /api/objects/{hash}
GET    /api/refs?tenant=&worktree=&tree_id=
```

All require `Authorization: Bearer <token>`. All emit audit events via the shared `Recorder`.

### POST /api/push

Promotes a snapshot chain from local (or staged) into the canonical branch tip with compare-and-swap conflict detection.

Request body (JSON):

```json
{
  "tenant": "acme",
  "worktree": "main",
  "tree_id": "uuid",
  "branch": "main",
  "expected_tip": "snapshot-id-or-null",
  "new_tip": "snapshot-id",
  "snapshot_chain": [
    {
      /* full Snapshot record */
    }
  ],
  "objects": [{ "hash": "blake3hex", "size": 1234, "path": "src/foo.rs" }]
}
```

`objects[]` lists hashes referenced by the chain — **metadata only**, no bytes. Client must first call `POST /api/objects/check` and upload missing blobs via `PUT /api/objects/{hash}` (see below) before invoking `/api/push`. This keeps push idempotent and lightweight.

Server actions in a single Postgres txn:

1. IAM check `branch:push` on `tenant:${tenant}/${worktree}/${tree_id}/branches/${branch}`.
2. Verify every hash in `objects[]` exists in `ObjectStore` (reject 412 Precondition Failed otherwise).
3. CAS: `UPDATE canonical_branches SET tip_snapshot_id = $new WHERE ... AND tip_snapshot_id IS NOT DISTINCT FROM $expected`. Zero rows → 409 Conflict.
4. Insert snapshot rows into `canonical_snapshots` (idempotent on PK).
5. Insert `canonical_snapshot_objects` rows linking each snapshot to its object hashes.
6. Audit emit `canonical_push` with decision/reason.

Response (200):

```json
{ "status": "accepted", "new_tip": "snapshot-id", "snapshots_committed": 3 }
```

Conflict (409):

```json
{
  "status": "conflict",
  "actual_tip": "snapshot-id",
  "message": "branch advanced by another client"
}
```

Missing-objects precondition (412):

```json
{ "status": "missing_objects", "missing": ["hash1", "hash2"] }
```

### POST /api/pull

Returns the snapshot chain and object set the client needs to advance to the current canonical tip.

Request:

```json
{
  "tenant": "acme",
  "worktree": "main",
  "tree_id": "uuid",
  "branch": "main",
  "last_known_tip": "snapshot-id-or-null"
}
```

Server actions:

1. IAM check `branch:pull`.
2. Read current `tip_snapshot_id` from `canonical_branches`.
3. Walk parent chain from current tip back to `last_known_tip` (or genesis), collecting snapshots and their object hashes.
4. Emit `canonical_pull` audit event.

Response:

```json
{
  "new_tip": "snapshot-id",
  "snapshots": [
    /* in oldest-to-newest order */
  ],
  "objects_needed": ["hash1", "hash2"],
  "up_to_date": false
}
```

If `last_known_tip == current tip`: `up_to_date: true`, empty arrays. Client downloads each object via `GET /api/objects/{hash}` and verifies BLAKE3 locally before applying snapshots to `.wt/state.json`.

### POST /api/objects/check

Simple negotiation: client asks which hashes the server is missing before uploading.

Request: `{ "hashes": [ "h1", "h2", "h3" ] }`
Response: `{ "missing": [ "h2" ] }`

IAM check `object:check`. This is the v1 substitute for full Have/Want negotiation — adequate because objects are immutable and content-addressed.

### PUT /api/objects/{hash}

Upload a single object blob. Body is raw bytes (or base64 JSON; see `Content-Type`). Server BLAKE3-verifies before persisting to `ObjectStore.Put`. Idempotent (no-op if hash already present).

IAM check `object:write`. Returns 201 on store, 200 if already present, 400 on hash mismatch.

### GET /api/objects/{hash}

Stream blob bytes. 404 if absent. Client MUST verify BLAKE3 of received bytes equals requested hash; mismatch is a protocol violation, reject.

IAM check `object:read`. `Content-Type: application/octet-stream`.

### GET /api/refs

List branch tips for a tree.

Response:

```json
{ "branches": [{ "name": "main", "tip": "snapshot-id", "updated_at": "..." }] }
```

IAM check `ref:list`.

### Push/Pull Sequence (Happy Path)

```
Client                                    Server
  |  POST /api/objects/check { hashes }     |
  |---------------------------------------->|
  |             { missing: [...] }          |
  |<----------------------------------------|
  |  PUT /api/objects/{h} (loop, missing)   |
  |---------------------------------------->|
  |  POST /api/push { chain, expected_tip } |
  |---------------------------------------->|
  |     { status: accepted, new_tip }       |
  |<----------------------------------------|
```

```
Client                                    Server
  |  POST /api/pull { last_known_tip }      |
  |---------------------------------------->|
  |  { snapshots, objects_needed, new_tip } |
  |<----------------------------------------|
  |  GET /api/objects/{h} (loop)            |
  |---------------------------------------->|
  |  apply snapshots to state.json          |
```

### Horizontal Scale Notes

- `canonical_branches` CAS via Postgres `UPDATE ... WHERE tip = $expected` is naturally multi-pod safe.
- `canonical_snapshots` and `canonical_snapshot_objects` inserts are idempotent on PK.
- `ObjectStore` must be horizontally consistent (shared volume or S3-compatible). v1 ships with per-pod `LocalObjectStore` and requires either single-replica or tenant-sticky routing. v2 will add an S3 adapter (clean interface today).
- Push notification fan-out (WebSocket subscribers learn of `canonical.branch.advanced`) is deferred to a Kafka-backed Phase 2; v1 clients poll via `/api/pull` or `/api/refs`.

### Access Config Sync

1. User edits .wt/access/_.toml or .wt-tree/access/_.toml
2. BGProcess detects change, validates locally
3. BGProcess syncs to server
4. Server validates (tenant resolution, path registration, policy consistency)
5. Server applies policies immediately
6. Server pushes updated config to other bgprocess clients

#### Validation Rules

- BGProcess performs local validation first (TOML syntax, known fields, SPDX identifiers)
- Server performs authoritative validation (tenant existence, path registration, policy conflicts)
- If server rejects: BGProcess reverts local change and notifies user
- Access config changes take effect immediately on the server — no "push" needed

### Tag Sync

1. Tag created locally → bgprocess syncs to server
2. Tag created by another user on server → bgprocess pulls it down
3. Tag deletions synced as soft deletes

#### Tag Wire Format

```
TagSyncMessage {
    action: TagAction,                  // Create, Delete, Update
    tag: Tag {
        name: String,
        target: SnapshotId,
        tagger: AccountEmail,
        message: Option<String>,
        timestamp: DateTime<Utc>,
        signature: Option<Signature>,
    },
}
```

### Large File Chunk Transfer

1. BGProcess chunks large file using FastCDC
2. BGProcess sends chunk manifest to server
3. Server responds with which chunks it already has (dedup check)
4. BGProcess uploads only new chunks
5. Server stores chunks in content-addressable store

#### Chunking Strategy

- Algorithm: FastCDC (Fast Content-Defined Chunking)
- Minimum chunk size: 256 KB
- Average chunk size: 1 MB
- Maximum chunk size: 4 MB
- Content-defined boundaries mean similar files share chunks even if offsets differ

#### Chunk Upload Protocol

```
ChunkManifest {
    file_path: FilePath,
    total_size: u64,
    chunks: Vec<ChunkRef {
        id: ChunkId,                    // BLAKE3 hash of chunk content
        offset: u64,
        size: u32,
    }>,
}
```

1. BGProcess sends ChunkManifest
2. Server responds with `needed: Vec<ChunkId>` (chunks it doesn't already have)
3. BGProcess uploads needed chunks as binary streams
4. Server verifies each chunk's BLAKE3 hash matches its ID
5. Server stores chunks and links them to the file via the manifest

## Delta Sync

- BGProcess tracks "last synced state" per branch
- Only new objects transferred
- Object-level deduplication across branches and trees
- Content-addressable hashing (BLAKE3) enables efficient comparison

### Have/Want Negotiation

```
HaveWantRequest {
    have: Vec<ObjectId>,                // Objects the client already has
    want: Vec<ObjectId>,                // Objects the client needs
}

HaveWantResponse {
    send: Vec<ObjectId>,                // Objects the server will send
    already_have: Vec<ObjectId>,        // Objects the server confirms client has
}
```

- This is similar to Git's pack negotiation but operates on W0rkTree objects
- Much simpler because W0rkTree objects are content-addressed and immutable
- No "thin pack" complexity — just send the objects that are missing

### Object Transfer Ordering

- Objects sent in dependency order: blobs first, then trees, then snapshots
- This allows the receiver to verify each object as it arrives
- Streaming: objects sent as a stream, not as a single batch
- Backpressure: receiver can slow down the stream if it's overwhelmed

## Offline Mode

- When server unreachable: bgprocess continues all local operations
- Staged snapshots accumulate locally
- On reconnect: delta sync catches up (only transfer what's new)
- No full re-sync needed

### Offline Detection

- BGProcess maintains persistent connection to server
- Connection loss detected via heartbeat timeout (default: 10 seconds)
- BGProcess transitions to offline mode automatically
- All local operations continue: snapshots, branches, merges, tags
- Staged snapshots marked as "pending sync"

### Reconnection Strategy

- Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s (capped)
- On reconnect: BGProcess sends all pending staged snapshots
- Server reconciles: merges pending snapshots into staged state
- If branch tip has moved: BGProcess detects on next pull and handles normally

### Conflict-Free Offline Operations

- Local snapshots always succeed (no server dependency)
- Local branch operations always succeed
- Push requires server — queued until reconnection
- Access config changes queued until server validation

## Transport

- Primary: gRPC over QUIC (encrypted, multiplexed, handles NAT)
- Fallback: gRPC over HTTP/2 (for networks that block UDP)
- All traffic encrypted (TLS 1.3)
- Authentication: JWT token in metadata headers

### QUIC Benefits

- Connection migration: laptop moves from WiFi to cellular, connection survives
- Multiplexed streams: multiple sync operations in parallel without head-of-line blocking
- 0-RTT reconnection: fast reconnect after brief disconnections
- Built-in encryption: TLS 1.3 integrated into the handshake

### Fallback Detection

- BGProcess attempts QUIC first
- If QUIC fails (UDP blocked, corporate firewall): falls back to HTTP/2
- Fallback is automatic and transparent to the user
- BGProcess logs which transport is in use
- User can force transport: `WT_TRANSPORT=quic` or `WT_TRANSPORT=http2`

## Wire Format

- Object serialization: Bincode (compact, fast) for sync protocol
- JSON for admin/REST API
- Protocol crate's `feature::wire::format` module: magic bytes, version, flags, header/payload structure

### Sync Message Envelope

```
SyncMessageEnvelope {
    magic: [u8; 4],                     // b"WT01"
    version: u8,                        // Protocol version (currently 1)
    flags: u8,                          // Compression, encryption flags
    message_type: u16,                  // Type of sync message
    payload_length: u32,                // Length of payload in bytes
    payload: Vec<u8>,                   // Bincode-serialized payload
}
```

### Compression

- Payloads over 1 KB are compressed with zstd (level 3)
- Compression flag set in envelope flags
- Receiver checks flag before deserializing
- Binary blobs (file content) may use different compression based on content type

## Sync Configuration

```toml
[sync]
auto = true
interval_secs = 30
```

Override per tree in .wt-tree/config.toml.

Disable: `wt sync pause`, `WT_SYNC_AUTO=false`

### Full Configuration Options

```toml
[sync]
auto = true                             # Enable auto-sync (default: true)
interval_secs = 30                      # Auto-sync interval (default: 30)
max_batch_size_mb = 100                 # Max batch size for uploads (default: 100)
chunk_size_mb = 1                       # Average chunk size for large files (default: 1)
transport = "auto"                      # "auto", "quic", "http2" (default: "auto")
offline_queue_max = 1000                # Max queued operations in offline mode (default: 1000)
compression = "zstd"                    # "zstd", "none" (default: "zstd")
compression_level = 3                   # zstd compression level (default: 3)
```

## Error Handling

### Retryable Errors

- Network timeout → retry with backoff
- Server temporarily unavailable (503) → retry with backoff
- Rate limited (429) → retry after Retry-After header
- Partial upload failure → resume from last acknowledged chunk

### Non-Retryable Errors

- Authentication failure (401) → prompt user to re-authenticate
- Authorization failure (403) → inform user of missing permission
- Conflict detected (409) → user must resolve manually
- Invalid request (400) → bug in client, log and report

### Idempotency

- All sync operations are idempotent
- Retrying an upload of the same object is safe (content-addressed)
- Retrying a push with the same expected_tip either succeeds again or fails with conflict
- Server assigns operation IDs for deduplication of side effects

## Security Considerations

- All traffic encrypted (TLS 1.3)
- JWT tokens rotated on each session
- Object integrity verified via BLAKE3 hashes
- Server validates all objects on receipt (no trust-the-client)
- Rate limiting per tenant to prevent abuse
- Large upload quotas configurable per tenant

## Implementation Status

- IMPLEMENTED: Wire format module in protocol crate
- IMPLEMENTED: Rust prototype `POST /staged` flow for single-snapshot staged upload with BLAKE3 verification and server-side staged index persistence
- IMPLEMENTED: Rust staged request compatibility with the Go field shape and `WT_SERVER_URL` / `WT_TENANT`
- IMPLEMENTED: Go REST `POST /staged` and `GET /staged`
- IMPLEMENTED: Go gRPC `SyncService.StageSnapshot` and `SyncService.ListStagedSnapshots`
- IMPLEMENTED: Go Postgres/file staged metadata stores with canonical staged identity idempotency and conflict detection
- IMPLEMENTED: Shared Go bearer-auth principal path for REST and gRPC staged APIs
- IMPLEMENTED: Shared Go default-deny `Authorizer` seam for staged create/list and audit decisions
- IMPLEMENTED: JSON demo credentials and policy files for local staged auth/IAM demos
- IMPLEMENTED: Canonical push/pull v1 REST contract (`POST /api/push`, `POST /api/pull`, `POST /api/objects/check`, `PUT/GET /api/objects/{hash}`, `GET /api/refs`) with Postgres-backed `canonical_branches` CAS, `canonical_snapshots`, `canonical_snapshot_objects`, and BLAKE3-verified `ObjectStore.Put`/`Get`
- IMPLEMENTED: Client canonical push/pull logic in `crates/worktree-server/src/sync/{push,pull}.rs`; SDK orchestrators in `worktree-sdk/src/engine/sync.rs` delegate to these
- IMPLEMENTED: `remote_tip` per-branch field in `WorktreeState` for CAS expected-tip tracking
- TODO: Simple object-check is the v1 negotiation. Full Have/Want envelope, batched object transfer, streaming pack files
- PLANNED: gRPC parity for canonical endpoints, QUIC transport, offline queue, S3-compatible `ObjectStore` adapter for horizontal scale, Kafka-backed push notifications

## Related Specifications

- [Staged Visibility](../visibility/StagedVisibility.md) — How staged snapshots are surfaced to teams
- [License Compliance](../licensing/LicenseCompliance.md) — License checks during sync operations
- [BGProcess](../bgprocess/) — The local daemon that drives sync
- [Server](../server/) — The remote server that receives sync traffic
