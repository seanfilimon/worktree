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

### Access Config Sync

1. User edits .wt/access/*.toml or .wt-tree/access/*.toml
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
- TODO: Sync protocol messages, delta sync, staged snapshot upload
- PLANNED: Full gRPC service definitions, QUIC transport, offline queue

## Related Specifications

- [Staged Visibility](../visibility/StagedVisibility.md) — How staged snapshots are surfaced to teams
- [License Compliance](../licensing/LicenseCompliance.md) — License checks during sync operations
- [BGProcess](../bgprocess/) — The local daemon that drives sync
- [Server](../server/) — The remote server that receives sync traffic