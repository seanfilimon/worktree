# Storage Architecture Specification

## Overview
W0rkTree uses a content-addressable object store inspired by Git's model but with significant improvements: BLAKE3 hashing (faster than SHA-256, tree-hashing mode), native large file chunking (no separate LFS), per-tenant namespace isolation, and a split storage model between the local bgprocess and the remote server.

## Storage Model: Local vs Remote

### Local Storage (BGProcess)
The bgprocess stores objects locally in a platform-appropriate location (NOT in the working directory):
- Windows: `%APPDATA%\W0rkTree\stores\<worktree-hash>\`
- Linux: `~/.local/share/w0rktree/stores/<worktree-hash>/`
- macOS: `~/Library/Application Support/W0rkTree/stores/<worktree-hash>/`

Local storage contains:
- Full objects for materialized trees (blobs, trees, snapshots, manifests)
- Stub metadata for non-materialized trees (shallow/partial sync)
- Large file chunk cache (LRU eviction when cache full)
- Reflog data (synced to server for recovery)

### Remote Storage (Server)
The server stores ALL objects for ALL tenants. It is the canonical source of truth.
- Content-addressable object store
- Per-tenant namespace isolation: `/<tenant-slug>/<worktree-name>/objects/`
- Never discards history
- Storage quotas per tenant/plan

## Object Types

### Blob
Raw file content, content-addressed by BLAKE3 hash.
- Small files: stored as single blob
- Large files (above threshold): stored as chunked manifest (see Large File section)
```
blob/<hash> → raw bytes
```

### Tree Object
Represents a directory listing — maps names to blob hashes or child tree hashes.
```
tree/<hash> → [(name, type, hash), ...]
```
Types: blob, tree, symlink

### Snapshot
A snapshot captures the complete state of a W0rkTree tree at a point in time. Contains:
- ID (BLAKE3 hash of content)
- Parent snapshot ID(s) (1 for normal, 2+ for merge)
- Tree object hash (root of the content tree)
- Author (email)
- Timestamp (UTC)
- Message
- Metadata (revert info, auto-snapshot flag, etc.)

### Manifest (Large File)
For files above the large file threshold, instead of a single blob, a manifest maps the file to its chunks:
```
manifest/<hash> → {
    original_hash: BLAKE3,
    size: u64,
    chunk_count: u32,
    mime_type: String,
    chunks: [(offset, size, hash), ...]
}
```

### Delta
Compressed diff between two object versions. Used for efficient sync.

### Tag
Named reference to a snapshot. Lightweight (name→snapshot) or annotated (name→snapshot + message + author + timestamp + signature).

### Branch
Named pointer to the tip snapshot of a branch. Mutable (moves forward on push).

## Content Addressing
All objects are addressed by their BLAKE3 hash:
- BLAKE3 is faster than SHA-256 and SHA-1
- Tree-hashing mode for large inputs
- Content-addressable: identical content → same hash → automatic deduplication
- Hash used for integrity verification (corrupt objects detected immediately)

## Large File Chunking
Files above the configurable threshold (default 10MB) are stored as chunks:

### FastCDC Algorithm
- Content-defined chunking: chunk boundaries determined by rolling hash of file content
- Average chunk size: configurable (default 4MB)
- Min chunk size: 1MB, max chunk size: 16MB
- Content-defined means inserting data at the beginning only affects nearby chunks, not all chunks

### Deduplication
- Chunks are content-addressed independently
- If two large files share content, shared chunks stored once
- If a large file is edited, only changed chunks re-uploaded
- Cross-file and cross-version deduplication is automatic

### Lazy Loading
- BGProcess stores stubs locally for large files not yet fetched
- Stub contains: hash, size, chunk count, MIME type
- Real content served on demand via FUSE/ProjFS/FUSE-T
- Chunks cached in local LRU cache after first access

## Storage Layout

### Local (BGProcess)
```
~/.local/share/w0rktree/stores/<worktree-hash>/
├── objects/
│   ├── blobs/          # Content blobs
│   ├── trees/          # Tree objects
│   ├── snapshots/      # Snapshot objects
│   ├── manifests/      # Large file manifests
│   └── chunks/         # Large file chunks (LRU cache)
├── refs/
│   ├── branches/       # Branch tip references
│   └── tags/           # Tag references
├── stubs/              # Stub metadata for non-materialized content
└── state/
    ├── head            # Current branch reference
    └── sync_state      # Last sync position per branch
```

### Remote (Server)
```
/var/lib/w0rktree/
├── tenants/
│   ├── acme-corp/
│   │   ├── my-project/
│   │   │   ├── objects/    # All objects for this worktree
│   │   │   ├── refs/       # Branch and tag references
│   │   │   ├── staged/     # Staged snapshots per user
│   │   │   └── releases/   # Release artifacts
│   │   └── another-project/
│   │       └── ...
│   └── alice-dev/
│       └── ...
└── shared/
    └── chunks/             # Deduplicated chunks (cross-tenant for public worktrees)
```

## Garbage Collection
- Auto-snapshots generate many small objects. Periodic GC needed.
- Server GC: unreachable objects (not referenced by any branch, tag, or staged snapshot) marked for deletion
- Grace period: 30 days (configurable) before actual deletion
- Staged snapshot GC: staged snapshots not pushed within retention window are cleaned up
- Reflog GC: entries beyond retention period pruned
- Large file chunk GC: chunks not referenced by any manifest are removed

## Object Deduplication
- Content-addressed: identical objects stored exactly once
- Works across branches, trees, tenants (for public worktrees)
- Large file chunks deduplicated independently

## Integrity Verification
- Every object verified by BLAKE3 hash on read
- Corrupt objects detected immediately
- BGProcess re-fetches corrupt objects from server
- Server verifies all uploaded objects

## Storage Quotas (Server)
| Plan | Max Storage | Max Worktrees | Max Trees/Worktree |
|---|---|---|---|
| Free | 1 GB | 5 | 10 |
| Pro | 50 GB | Unlimited | Unlimited |
| Enterprise | Custom | Custom | Custom |

Quota enforcement: server rejects uploads that would exceed quota.

## Chunk Cache Management

### LRU Cache Policy
The local chunk cache uses a Least Recently Used eviction policy:
- Default cache size: 2 GB (configurable in `.wt/config.toml`)
- When cache is full, least recently accessed chunks are evicted first
- Pinned chunks (actively open files) are never evicted
- Cache size can be configured per-worktree or globally

### Cache Configuration
```toml
[storage.cache]
max_size = "2GB"
eviction_policy = "lru"
pin_open_files = true
prefetch_adjacent_chunks = true
```

### Prefetching
- When a chunk is accessed, adjacent chunks in the same manifest can be prefetched
- Prefetch strategy is configurable: none, adjacent, full
- Prefetched chunks enter the LRU cache at lower priority

## Object Serialization

### Wire Format
Objects are serialized for storage and transport using a compact binary format:
- Header: object type (1 byte) + uncompressed size (varint)
- Body: zstd-compressed content
- Trailer: BLAKE3 hash (32 bytes) for integrity

### Compression
- Default compression: zstd level 3 (good balance of speed and ratio)
- Configurable compression level per object type
- Blobs: zstd (general) or type-specific (e.g., skip compression for already-compressed formats like PNG, ZIP)
- Trees/Snapshots: always zstd (high ratio due to repetitive structure)
- Chunks: zstd with dictionary training on similar content types

## Pack Files

### Purpose
For efficient sync, objects can be grouped into pack files:
- Reduces network round-trips (one transfer instead of many)
- Enables delta compression between related objects
- Used during push/pull operations

### Pack File Format
```
pack/<hash> → {
    version: u8,
    object_count: u32,
    objects: [
        { type, size, data | delta_base + delta },
        ...
    ],
    index: [(hash, offset, size), ...],
    checksum: BLAKE3
}
```

### Delta Compression in Packs
- Objects can be stored as deltas against a base object
- Base selection heuristic: prefer objects of similar type and size
- Delta chain depth limit: 50 (configurable)
- Reduces transfer size significantly for incremental syncs

## Shallow and Partial Sync

### Shallow Sync
- Only fetch snapshots up to a certain depth (e.g., last 100 snapshots)
- Older snapshots represented as stubs with metadata only
- Full history fetchable on demand

### Partial Sync (Sparse)
- Only materialize certain paths locally
- Configured via `.wt/config.toml` sparse patterns:
```toml
[sync.sparse]
include = ["src/", "docs/README.md"]
exclude = ["assets/videos/"]
```
- Non-materialized paths have stubs with metadata (hash, size, type)
- Accessing a non-materialized file triggers on-demand fetch via virtual filesystem

### Stub Format
```
stub/<hash> → {
    object_type: blob | tree | manifest,
    hash: BLAKE3,
    size: u64,
    mime_type: Option<String>,
    is_large_file: bool,
    chunk_count: Option<u32>
}
```

## Reflog

### Purpose
The reflog records all reference changes (branch tips, HEAD moves):
- Every branch tip update logged with before/after hashes
- Every HEAD change logged
- Enables recovery from accidental operations

### Reflog Entry Format
```
{
    ref_name: String,
    old_hash: BLAKE3,
    new_hash: BLAKE3,
    user: String,
    timestamp: UTC,
    operation: String,   // "snapshot", "push", "pull", "merge", "revert", etc.
    message: String
}
```

### Reflog Sync
- Reflog entries synced to server for disaster recovery
- Server retains reflog entries per tenant retention policy
- Local reflog pruned per local retention setting (default: 90 days)

## Concurrent Access

### Local Locking
- BGProcess is the single writer to local storage
- CLI communicates via IPC — no direct storage access
- File-level locks for critical operations (branch tip updates)

### Server-Side Coordination
- Optimistic concurrency for push operations
- Compare-and-swap for branch tip updates (prevents lost updates)
- Server rejects push if branch tip has moved since last pull
- Conflict resolution handled at the protocol layer (see SyncProtocol.md)

## Backup and Recovery

### Local Recovery
- If local storage is corrupted, BGProcess detects via BLAKE3 verification
- Corrupt objects automatically re-fetched from server
- Full local store can be rebuilt from server (re-clone)

### Server Recovery
- Server storage is the source of truth
- Regular backups of object store and metadata
- Point-in-time recovery supported (enterprise feature)
- Cross-region replication for high availability (enterprise feature)

## Performance Considerations

### Read Path
1. CLI requests file content via IPC
2. BGProcess checks local object store
3. If present: decompress and serve
4. If stub: fetch from server, cache locally, then serve
5. Virtual filesystem layer handles transparent lazy loading

### Write Path
1. CLI sends file changes via IPC
2. BGProcess computes BLAKE3 hash
3. If hash already exists in store: skip write (dedup)
4. If new: compress and write to local store
5. Large files: chunk via FastCDC, store manifest + chunks
6. Create/update snapshot object referencing new tree

### Sync Path
1. Compute delta between local and remote branch tips
2. Pack missing objects into pack file
3. Transfer pack via QUIC/gRPC
4. Server unpacks and stores objects
5. Update remote branch tip (compare-and-swap)

## Implementation Status
- IMPLEMENTED: BLAKE3 hashing (core::hash), blob/tree/snapshot/manifest/delta types (object module)
- TODO: Content-addressable store, local storage engine, GC, pack files, chunk cache
- PLANNED: Server storage backend, tenant namespace isolation, quota enforcement, shallow/partial sync