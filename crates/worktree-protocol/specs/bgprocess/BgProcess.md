# BGProcess Specification

> **Document**: `specs/bgprocess/BgProcess.md`
> **Status**: Draft
> **Audience**: W0rkTree contributors and implementors

---

## Table of Contents

- [1. Overview](#1-overview)
- [2. What the BGProcess Is (and Is Not)](#2-what-the-bgprocess-is-and-is-not)
- [3. Architecture](#3-architecture)
- [4. Responsibilities](#4-responsibilities)
- [5. Lifecycle](#5-lifecycle)
- [6. Auto-Snapshot Engine](#6-auto-snapshot-engine)
- [7. Staged Snapshot Sync](#7-staged-snapshot-sync)
- [8. Remote Sync Protocol](#8-remote-sync-protocol)
- [9. Ignore Pattern Processing](#9-ignore-pattern-processing)
- [10. Large File Handling](#10-large-file-handling)
- [11. Automatic Merge](#11-automatic-merge)
- [12. Conflict Detection and Resolution](#12-conflict-detection-and-resolution)
- [13. Reflog Management](#13-reflog-management)
- [14. CLI Communication (IPC)](#14-cli-communication-ipc)
- [15. Configuration](#15-configuration)
- [16. Platform Support](#16-platform-support)
- [17. Security](#17-security)
- [18. CLI Commands](#18-cli-commands)
- [19. Error Handling and Recovery](#19-error-handling-and-recovery)
- [20. Observability](#20-observability)
- [21. Implementation Status](#21-implementation-status)
- [22. Migration Plan](#22-migration-plan)

---

## 1. Overview

The **bgprocess** (`worktree-worker`) is a persistent local daemon that runs on the
developer's machine. It is the core VCS engine — it manages all local versioning
operations and communicates with the remote W0rkTree server for sync, visibility,
and collaboration.

The bgprocess is one half of the W0rkTree two-runtime architecture. It owns
everything local: filesystem watching, auto-snapshots, branch management, local
history, diff computation, merge operations, large file handling, and sync
orchestration. The remote server owns everything shared: canonical history, access
control enforcement, staged snapshot aggregation, license compliance, and branch
protection.

Neither runtime duplicates the other's work. The bgprocess never enforces access
control. The server never watches files. This separation is a **core architectural
constraint**, not an implementation detail.

---

## 2. What the BGProcess Is (and Is Not)

### It IS

- A **local daemon** running on the developer's machine.
- The **only process** that touches the working directory.
- The **sync client** that uploads staged snapshots and downloads remote changes.
- The **single coordination point** for all local VCS operations.
- A **platform-native service** managed by the OS service manager.

### It is NOT

- **Not the server.** Current code in `worktree-server` conflates local bgprocess
  logic (watcher, debouncer, semantic event classifier, auto-snapshot engine) with
  server logic. This must be separated. The bgprocess will live in a dedicated
  `worktree-worker` crate.
- **Not a build tool.** It does not compile, test, or deploy code.
- **Not a web server.** It does not serve HTTP to external clients. It only listens
  on local IPC for CLI commands.
- **Not optional.** The bgprocess is required for all local W0rkTree operations.
  Without it, the `wt` CLI cannot function.

### Terminology Clarification

Throughout W0rkTree documentation and code:

| Term | Refers to |
|---|---|
| `bgprocess` | The local daemon specified in this document |
| `worktree-worker` | The Rust crate that implements the bgprocess |
| `worktree-server` | The **remote** server — canonical history, IAM, compliance |
| `daemon` | The long-running process entry point within `worktree-worker` |

> **Important:** Any existing code or documentation that refers to the bgprocess
> as "the server" is incorrect and should be updated.

---

## 3. Architecture

### Process Model

```
┌────────────────────────────────────────────────────────────────┐
│                      Developer Machine                          │
│                                                                 │
│   ┌───────────┐        IPC         ┌────────────────────────┐  │
│   │  wt CLI   │◄──────────────────►│   worktree-worker      │  │
│   │           │  (socket / pipe)   │   (bgprocess daemon)   │  │
│   └───────────┘                    │                        │  │
│                                    │  ┌──────────────────┐  │  │
│                                    │  │ Filesystem       │  │  │
│   ┌───────────┐                    │  │ Watcher          │  │  │
│   │  Working  │◄── fs events ─────►│  └──────────────────┘  │  │
│   │ Directory │                    │  ┌──────────────────┐  │  │
│   │           │                    │  │ Auto-Snapshot    │  │  │
│   │  .wt/     │                    │  │ Engine           │  │  │
│   │  .wt-tree/│                    │  └──────────────────┘  │  │
│   └───────────┘                    │  ┌──────────────────┐  │  │
│                                    │  │ Sync Engine      │  │  │
│                                    │  └────────┬─────────┘  │  │
│                                    │  ┌──────────────────┐  │  │
│                                    │  │ Large File VFS   │  │  │
│                                    │  └──────────────────┘  │  │
│                                    └────────┬───────────────┘  │
│                                             │                  │
└─────────────────────────────────────────────┼──────────────────┘
                                              │ W0rkTree Sync
                                              │ Protocol (TLS/QUIC)
┌─────────────────────────────────────────────┼──────────────────┐
│                                             │                  │
│                    ┌────────────────────────┴───────────────┐  │
│                    │          worktree-server               │  │
│                    │  (canonical history, IAM, compliance)  │  │
│                    └───────────────────────────────────────┘  │
│                           Remote Server                       │
└───────────────────────────────────────────────────────────────┘
```

### Internal Subsystems

The bgprocess is composed of the following cooperating subsystems, each running
as an async task within a single Tokio runtime:

| Subsystem | Responsibility |
|---|---|
| **Filesystem Watcher** | Monitors working directory via OS-native APIs; feeds raw events into the debouncer. |
| **Debouncer** | Collapses rapid successive changes to the same path within a configurable window. |
| **Semantic Classifier** | Classifies debounced events into semantic categories (code change, config change, dependency change, cross-tree change). |
| **Ignore Matcher** | Compiled ignore pattern matcher; filters events before they reach the classifier. |
| **Auto-Snapshot Engine** | Evaluates snapshot rules against pending changesets; creates snapshots when conditions are met. |
| **Snapshot Store** | Manages local snapshot objects, DAG relationships, and branch pointers. |
| **Diff Engine** | Computes diffs between manifests using `worktree_protocol::feature::diff::compute`. |
| **Merge Engine** | Handles local auto-merge for non-conflicting changes; detects and reports conflicts. |
| **Sync Engine** | Uploads staged snapshots to server; downloads remote branch updates; handles delta sync. |
| **Large File Manager** | Detects, chunks, deduplicates, and serves large files via virtual filesystem. |
| **Reflog Writer** | Logs all branch-tip-changing operations for recovery. |
| **IPC Server** | Listens for CLI commands on local socket/pipe; dispatches to appropriate subsystem. |
| **Config Manager** | Reads and watches `.wt/config.toml` and `.wt-tree/config.toml`; hot-reloads on change. |

### Data Flow

```
  filesystem events
        │
        ▼
  ┌─────────────┐     ┌───────────────┐
  │  OS Watcher  │────►│   Debouncer   │
  └─────────────┘     └───────┬───────┘
                              │
                              ▼
                     ┌────────────────┐
                     │ Ignore Matcher │──── (ignored) ──► /dev/null
                     └───────┬────────┘
                             │ (not ignored)
                             ▼
                    ┌─────────────────┐
                    │ Semantic        │
                    │ Classifier      │
                    └───────┬─────────┘
                            │
                            ▼
                   ┌─────────────────┐
                   │ Pending         │
                   │ Changeset       │
                   └───────┬─────────┘
                           │
                           ▼
                  ┌──────────────────┐
                  │ Auto-Snapshot    │──── (rules met) ──► Create Snapshot
                  │ Engine           │                           │
                  └──────────────────┘                          ▼
                                                        ┌──────────────┐
                                                        │ Sync Engine  │
                                                        │ (staged      │
                                                        │  upload)     │
                                                        └──────────────┘
```

---

## 4. Responsibilities

### 4.1 Core VCS (Local)

The bgprocess owns all local versioning:

- **Filesystem watching** — Monitors the working directory for create, modify,
  delete, and rename events using the `notify` crate with OS-native backends.
- **Auto-snapshots** — Creates snapshots automatically based on configurable rules
  (inactivity timeout, max changed files, on tree switch).
- **Manual snapshots** — Creates snapshots on explicit `wt snapshot` command.
- **Local history** — Maintains the full local snapshot DAG with branch pointers,
  parent relationships, and merge records.
- **Branch management** — Creates, switches, lists, deletes, and merges branches.
  All branch operations happen locally first, then sync to server.
- **Diff computation** — Computes diffs between snapshots using the protocol
  crate's `feature::diff::compute` module (`compute_diff()` and
  `compute_diff_default()`).
- **Local merge** — Auto-merges non-conflicting changes silently; surfaces
  conflicts with machine-readable metadata.
- **Ignore patterns** — Compiles `.wt/ignore` and `.wt-tree/ignore` into an
  optimized matcher applied to every filesystem event.
- **Reflog** — Logs all branch-tip-changing operations to `.wt/reflog/`.

### 4.2 Staged Snapshot Sync (The Key Innovation)

When the bgprocess captures a snapshot (auto or manual), it syncs to the server
as a **staged snapshot**:

- **Staged ≠ Pushed.** A staged snapshot is visible to the team but is **not** part
  of branch history. It lives in a separate visibility layer.
- **Team visibility.** Teammates can see that you are working, what files you have
  changed, and how far along you are — without requiring standups or status updates.
- **No branch pollution.** Staged snapshots do not appear in `wt log`. They are a
  work-in-progress layer that exists alongside the branch.
- **Explicit push.** The developer must run `wt push` to finalize staged work into
  branch history. This is the moment it becomes permanent.
- **Discard option.** Staged snapshots can be discarded without affecting history.
- **Offline accumulation.** When auto-sync is disabled, staged snapshots accumulate
  locally and sync when re-enabled or when the developer explicitly syncs.

### 4.3 Remote Sync

- **Upload staged snapshots** to the W0rkTree server.
- **Download remote branch updates** so local state stays current.
- **Upload file blobs** (and large file chunks) referenced by staged snapshots.
- **Delta sync** — Only uploads changed objects, not full snapshots.
- **Configurable interval** — Auto-sync runs on a configurable timer (default: 30s).
- **Manual sync** — `wt sync` triggers an immediate sync cycle.

### 4.4 Large File Handling

- **Automatic detection** — Files exceeding a configurable threshold (default 10 MB)
  are transparently handled as large files.
- **Content-defined chunking** — Uses FastCDC rolling hash algorithm for
  content-defined chunk boundaries (default target: 1 MB chunks).
- **Deduplication** — Identical chunks across files and snapshots are stored once.
- **Virtual filesystem** — Serves large file content transparently via platform
  VFS (FUSE on Linux, FUSE-T on macOS, ProjFS on Windows).
- **Lazy loading** — Chunks are fetched on demand when a file is read.
- **Local cache** — Fetched chunks are cached in platform-native storage.

### 4.5 `.wt/` and `.wt-tree/` Management

The bgprocess owns the root `.wt/` directory and all `.wt-tree/` directories:

- **Reads configuration** from `.wt/config.toml` and `.wt-tree/config.toml`.
- **Manages identity** in `.wt/identity/` (auth tokens, tenant info).
- **Manages reflog** in `.wt/reflog/`.
- **Manages conflict metadata** in `.wt/conflicts/`.
- **Manages hooks** in `.wt/hooks/`.
- **Manages local state** (current branch, pending changes, sync cursor).

### 4.6 Platform-Native Storage

All internal data (object store, history cache, chunk cache) is stored in the
platform-appropriate location. **Nothing** is stored inside the working directory
except `.wt/` and `.wt-tree/` configuration:

| Platform | Storage Path |
|---|---|
| Windows | `%APPDATA%\W0rkTree\` |
| Linux | `~/.local/share/w0rktree/` |
| macOS | `~/Library/Application Support/W0rkTree/` |

Within the storage directory:

```
<platform_storage>/
├── objects/                  ← Content-addressable blob storage
│   ├── <hash[0:2]>/
│   │   └── <hash[2:]>       ← Object files
│   └── ...
├── chunks/                   ← Large file chunk cache
│   ├── <chunk-hash[0:2]>/
│   │   └── <chunk-hash[2:]>
│   └── ...
├── snapshots/                ← Snapshot metadata cache
├── worktrees/                ← Per-worktree state
│   └── <worktree-hash>/
│       ├── branches.json     ← Local branch pointers
│       ├── sync_cursor       ← Last sync position
│       └── pending/          ← Pending staged snapshots
└── logs/                     ← Daemon log files
```

---

## 5. Lifecycle

### 5.1 Startup

The bgprocess starts in one of three ways:

1. **Automatic on `wt init`** — When a worktree is initialized, the bgprocess
   starts automatically.
2. **Automatic on `wt init --from`** — When cloning from a remote, the bgprocess
   starts after the initial sync completes.
3. **Manual** — `wt worker start` starts the bgprocess explicitly.

Startup sequence:

```
1. Read .wt/config.toml
   └── Parse worktree identity, sync settings, snapshot rules
2. Read .wt-tree/config.toml for each registered tree
   └── Parse per-tree overrides
3. Compile ignore patterns
   └── Hard ignores + soft defaults + .wt/ignore + .wt-tree/ignore
4. Initialize platform-native storage
   └── Create directories if needed, verify integrity
5. Start filesystem watcher
   └── Register working directory with OS-native watcher
6. Start IPC server
   └── Create Unix socket or named pipe
7. Authenticate to server (if configured)
   └── Read token from .wt/identity/token, verify with server
8. Start auto-sync loop
   └── Begin periodic sync at configured interval
9. Write PID file
   └── .wt/worker.pid — used for health checks and stale detection
10. Log startup complete
```

### 5.2 Runtime

During normal operation, the bgprocess runs the following concurrent loops:

| Loop | Interval | Purpose |
|---|---|---|
| **Watcher loop** | Continuous (event-driven) | Receives and debounces filesystem events |
| **Snapshot evaluation** | On each debounced batch | Checks auto-snapshot rules against pending changeset |
| **Sync loop** | Configurable (default 30s) | Uploads staged snapshots, downloads remote updates |
| **IPC listener** | Continuous (event-driven) | Accepts CLI commands via socket/pipe |
| **Config watcher** | On filesystem event | Hot-reloads config when `.wt/config.toml` or `.wt-tree/config.toml` changes |
| **Health check** | Every 60s | Verifies watcher is alive, IPC socket is bound, storage is accessible |
| **Maintenance** | Every 6h | Prunes expired reflog entries, cleans stale chunks, compacts object store |

### 5.3 Shutdown

Shutdown can be triggered by:

- `wt worker stop` — Graceful shutdown via IPC.
- OS signal (SIGTERM on Unix, service stop on Windows).
- Automatic on system shutdown (via service manager integration).

Shutdown sequence:

```
1. Stop accepting new IPC connections
2. Drain pending IPC requests (max 5 seconds)
3. Stop filesystem watcher
4. Create final snapshot if pending changes exist (configurable)
5. Complete any in-progress sync (max 30 seconds, then abort)
6. Flush pending reflog entries
7. Release filesystem locks
8. Remove PID file
9. Close IPC socket/pipe
10. Exit cleanly
```

### 5.4 Crash Recovery

If the bgprocess crashes or is killed:

1. Next `wt` CLI invocation detects stale PID file.
2. CLI offers to restart the bgprocess automatically.
3. On restart, the bgprocess:
   - Scans working directory for changes since last known state.
   - Replays any pending operations from the journal.
   - Resumes sync from the last cursor position.
   - Resumes normal operation.

---

## 6. Auto-Snapshot Engine

### 6.1 Configuration

Auto-snapshot rules are configured in `.wt/config.toml` (worktree-wide defaults)
and `.wt-tree/config.toml` (per-tree overrides):

```toml
# .wt/config.toml — worktree-wide defaults

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 300    # Snapshot after 5 min of no changes
max_changed_files = 50           # Snapshot when 50+ files changed
max_changed_bytes = "50MB"       # Snapshot when total change size exceeds 50MB
on_tree_switch = true            # Snapshot before switching trees/branches
on_branch_switch = true          # Snapshot before switching branches
message_format = "auto: {file_count} files changed in {tree}"
```

```toml
# .wt-tree/config.toml — per-tree override (additive, cannot weaken)

[auto_snapshot]
inactivity_timeout_secs = 120    # This tree snapshots more frequently
max_changed_files = 20           # Lower threshold for this tree
```

### 6.2 Snapshot Flow

```
1. File change detected by OS watcher
        │
        ▼
2. Event debounced (collapse rapid changes, default 200ms window)
        │
        ▼
3. Event checked against ignore matcher
        │
        ├── IGNORED → drop event, no further processing
        │
        └── NOT IGNORED ▼
                         │
4. Event classified (code, config, dependency, cross-tree)
        │
        ▼
5. Change added to pending changeset for the owning tree
        │
        ▼
6. Auto-snapshot rules evaluated:
   ┌────────────────────────────────────────────────────┐
   │ a. Inactivity timer                                │
   │    - Reset on each new change                      │
   │    - When timer expires → trigger snapshot          │
   │                                                    │
   │ b. Max changed files                               │
   │    - Count files in pending changeset               │
   │    - When count >= threshold → trigger snapshot      │
   │                                                    │
   │ c. Max changed bytes                               │
   │    - Sum byte deltas in pending changeset           │
   │    - When total >= threshold → trigger snapshot      │
   │                                                    │
   │ d. Tree/branch switch (pre-switch hook)            │
   │    - If pending changes exist → trigger snapshot    │
   │    - Then proceed with the switch                  │
   └────────────────────────────────────────────────────┘
        │
        ▼
7. Snapshot created:
   a. Compute manifest of current working directory state
   b. Compute diff against previous snapshot
   c. Hash and store new/modified blobs
   d. Create snapshot object (parent, manifest hash, message, timestamp)
   e. Update branch pointer
   f. Write reflog entry
        │
        ▼
8. Staged sync (if auto-sync enabled):
   a. Mark snapshot as "staged"
   b. Queue for upload to server
   c. Sync engine picks it up on next cycle
```

### 6.3 Snapshot Object Structure

Each snapshot contains:

| Field | Type | Description |
|---|---|---|
| `id` | `SnapshotId` (hash) | Content-addressable hash of the snapshot |
| `parents` | `Vec<SnapshotId>` | Parent snapshot(s) — one for normal, two for merge |
| `tree_id` | `TreeId` | Which tree this snapshot belongs to |
| `branch` | `String` | Branch name at time of creation |
| `manifest` | `ManifestHash` | Hash of the file manifest |
| `message` | `String` | Human-readable message (auto-generated or manual) |
| `author` | `TenantId` | Who created this snapshot |
| `timestamp` | `DateTime<Utc>` | When the snapshot was created |
| `metadata` | `SnapshotMetadata` | Additional metadata (auto vs manual, trigger reason) |

### 6.4 Pending Changeset

The pending changeset is an in-memory data structure tracking all changes since
the last snapshot:

```rust
struct PendingChangeset {
    /// The tree this changeset belongs to.
    tree_id: TreeId,
    /// Changed file paths with their event kinds.
    changes: HashMap<PathBuf, ChangeEntry>,
    /// Timestamp of the first change in this set.
    first_change: DateTime<Utc>,
    /// Timestamp of the most recent change.
    last_change: DateTime<Utc>,
    /// Total bytes changed (estimated from file sizes).
    total_bytes_changed: u64,
}

struct ChangeEntry {
    kind: EventKind,          // Created, Modified, Deleted, Renamed
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    size_delta: i64,          // Positive = growth, negative = shrink
}
```

---

## 7. Staged Snapshot Sync

### 7.1 Staged vs Pushed

This is the key innovation in W0rkTree's collaboration model:

| Aspect | Staged Snapshot | Pushed Snapshot |
|---|---|---|
| **Created by** | Auto-snapshot or manual `wt snapshot` | `wt push` |
| **Visible to team** | ✅ Yes (via server) | ✅ Yes |
| **Part of branch history** | ❌ No | ✅ Yes |
| **Appears in `wt log`** | ❌ No | ✅ Yes |
| **Can be discarded** | ✅ Yes, freely | ❌ No (append-only) |
| **Server stores** | ✅ In staging area | ✅ In canonical history |
| **Purpose** | Visibility, backup, WIP sharing | Permanent record |

### 7.2 Staging Flow

```
Developer works
      │
      ▼
Auto-snapshot created locally
      │
      ▼
Sync engine uploads to server → Server stores as staged snapshot
      │                                    │
      │                                    ▼
      │                         Team sees: "Alice has 3 staged
      │                          snapshots on feature-auth with
      │                          12 files changed"
      │
      ▼
Developer runs `wt push`
      │
      ▼
Staged snapshots merged into single pushed snapshot
      │
      ▼
Server adds to canonical branch history
      │
      ▼
Team sees push in `wt log`
```

### 7.3 Staged Snapshot Lifecycle

1. **Created** — Auto-snapshot engine or manual `wt snapshot` creates it.
2. **Queued** — Added to the sync queue.
3. **Uploading** — Blobs and metadata being uploaded to server.
4. **Staged** — Server has received it; visible to team.
5. **Pushed** — Developer runs `wt push`; server moves it to branch history.
   OR **Discarded** — Developer runs `wt snapshot discard`; server removes it.

---

## 8. Remote Sync Protocol

### 8.1 Sync Loop

The sync engine runs on a configurable interval (default 30 seconds):

```
┌──────────────────────────────────────────────────────┐
│                    Sync Cycle                         │
│                                                      │
│  1. Upload Phase (local → server)                    │
│     a. Check for pending staged snapshots             │
│     b. Upload new blobs (delta: only changed objects) │
│     c. Upload snapshot metadata                       │
│     d. Upload reflog entries                          │
│     e. Confirm staging with server                    │
│                                                      │
│  2. Download Phase (server → local)                  │
│     a. Fetch remote branch pointer updates            │
│     b. Download new snapshot metadata                 │
│     c. Download new blobs (lazy: only on demand)      │
│     d. Download staged snapshot notifications         │
│     e. Update local branch pointers                  │
│     f. Trigger auto-merge if needed                  │
│                                                      │
│  3. Reconciliation                                   │
│     a. Update sync cursor                            │
│     b. Log sync result                               │
│     c. Notify IPC clients of changes                 │
└──────────────────────────────────────────────────────┘
```

### 8.2 Delta Sync

The bgprocess never uploads the entire object store. It uses delta sync:

- **Object deduplication** — Objects are content-addressable. If the server already
  has an object (by hash), it is not re-uploaded.
- **Manifest diffing** — The sync engine computes the diff between the last synced
  manifest and the current manifest, uploading only changed blobs.
- **Chunk-level dedup** — For large files, individual chunks are deduplicated.
- **Compression** — All data in transit is compressed (zstd).

### 8.3 Offline Mode

When the server is unreachable:

1. Staged snapshots accumulate locally in the pending queue.
2. Auto-sync continues to attempt connection at the configured interval.
3. Local operations (snapshot, branch, merge) continue normally.
4. When connectivity is restored, all pending staged snapshots are uploaded.
5. Remote changes are downloaded and auto-merged as needed.

---

## 9. Ignore Pattern Processing

### 9.1 Compilation

On startup (and when ignore files change), the bgprocess compiles all ignore
patterns into a single optimized matcher:

```
Priority (highest to lowest):
─────────────────────────────
1. Hard ignores (always, cannot override):
   .wt/
   .wt-tree/
   .git/

2. Registered paths in config.toml:
   If a path is explicitly registered AND would be ignored,
   the registration wins — the path IS tracked.

3. Root .wt/ignore:
   Authoritative — patterns here cannot be negated by any tree.

4. Tree .wt-tree/ignore:
   Additive — adds patterns for this tree.
   Cannot negate root patterns.

5. Subtree .wt-tree/ignore:
   Additive — adds patterns for the subtree.
   Cannot negate parent tree patterns.

6. Soft defaults (ignored by default, can be overridden):
   node_modules/
   target/
   __pycache__/
   *.pyc
   .DS_Store
   Thumbs.db
   *.swp
   *.swo
   *~
   .env
   *.log
```

### 9.2 Runtime Filtering

The compiled matcher is applied at the **earliest possible point** in the event
pipeline — immediately after debouncing, before semantic classification:

```rust
// Pseudocode — actual implementation will use a compiled glob set
fn should_ignore(path: &Path, matcher: &IgnoreMatcher) -> bool {
    // Hard ignores — always
    if path.starts_with(".wt") || path.starts_with(".wt-tree") || path.starts_with(".git") {
        return true;
    }
    // Registered paths — never ignored
    if matcher.is_registered(path) {
        return false;
    }
    // Check compiled pattern set
    matcher.is_match(path)
}
```

Events that match are dropped immediately with **zero further processing cost**.
No diff, no hash, no snapshot evaluation.

### 9.3 Hot Reload

When `.wt/ignore` or `.wt-tree/ignore` changes:

1. The config watcher detects the change.
2. The ignore matcher is recompiled.
3. The new matcher replaces the old one atomically.
4. In-flight events are not retroactively filtered (they proceed with the
   matcher that was active when they entered the pipeline).

---

## 10. Large File Handling

### 10.1 Detection

Files exceeding the configured threshold are automatically treated as large files:

```toml
# .wt/config.toml
[storage]
large_file_threshold = "10MB"    # Default: 10 MB
chunk_target_size = "1MB"        # Default: 1 MB target chunk size
```

There is no `.gitattributes`-style tracking configuration. There is no separate
LFS extension. The same storage pipeline handles all files.

### 10.2 Chunking

Large files are split using the **FastCDC** (Fast Content-Defined Chunking) algorithm:

1. **Content-defined boundaries** — Chunk boundaries are determined by file content
   using a rolling hash, not fixed offsets. Small edits to a large file only produce
   new chunks for the changed regions.
2. **Target chunk size** — Configurable (default 1 MB). FastCDC produces chunks
   within a range around the target (typically 0.5x to 2x target).
3. **Content-addressable** — Each chunk is identified by its hash. Identical chunks
   across files, snapshots, or even worktrees are stored once.

### 10.3 Chunk Manifest

Each large file is represented by a chunk manifest:

```
ChunkManifest {
    file_hash: Hash,               // Hash of the complete file
    file_size: u64,                // Total file size in bytes
    chunks: Vec<ChunkRef>,         // Ordered list of chunks
}

ChunkRef {
    hash: Hash,                    // Content-addressable chunk hash
    offset: u64,                   // Byte offset in the original file
    size: u32,                     // Chunk size in bytes
}
```

### 10.4 Virtual Filesystem

The bgprocess serves large file content transparently via a platform-native
virtual filesystem:

| Platform | Technology | Details |
|---|---|---|
| Linux | FUSE | User-space filesystem mounted at worktree root |
| macOS | FUSE-T | macOS-compatible FUSE implementation |
| Windows | ProjFS | Windows Projected File System provider |

Applications see regular files. The lazy loading is completely transparent to
editors, build tools, and other programs.

### 10.5 Chunk Cache

Fetched chunks are cached locally in platform-native storage:

```
<platform_storage>/chunks/<hash[0:2]>/<hash[2:]>
```

Cache eviction policy:

- **LRU** — Least recently used chunks are evicted first.
- **Configurable max size** — Default 10 GB, configurable in `.wt/config.toml`.
- **Pin support** — Frequently accessed files can be pinned to prevent eviction.

---

## 11. Automatic Merge

### 11.1 When Auto-Merge Triggers

When the sync engine downloads remote branch updates and local changes exist on
the same branch, the merge engine evaluates whether auto-merge is possible:

```
Remote update arrives for branch "feature-auth"
      │
      ▼
Local has uncommitted/staged changes on "feature-auth"?
      │
      ├── NO → Fast-forward local branch pointer. Done.
      │
      └── YES ▼
              │
  Compute three-way diff: (common ancestor, local, remote)
              │
              ▼
  Classify changes by file:
      │
      ├── Different files changed → Auto-merge silently ✅
      │
      ├── Same file, different regions → Auto-merge with notification ✅
      │
      └── Same file, overlapping regions → CONFLICT ❌
```

### 11.2 Auto-Merge Behavior

| Scenario | Action | User Notification |
|---|---|---|
| Different files | Merge silently | None (visible in `wt status`) |
| Same file, non-overlapping hunks | Merge and notify | `wt status` shows "auto-merged: file.rs" |
| Same file, conflicting hunks | Pause sync, mark conflicts | `wt status` shows conflicts, blocks push |

### 11.3 Merge Snapshot

When auto-merge succeeds, the bgprocess creates a merge snapshot with two parents:

- Parent 1: The local branch tip.
- Parent 2: The remote branch tip.

The merge snapshot is automatically staged for sync.

---

## 12. Conflict Detection and Resolution

### 12.1 Conflict Detection

When the merge engine detects overlapping changes in the same file:

1. **Pause auto-sync** for the affected branch.
2. **Write conflict metadata** to `.wt/conflicts/<conflict-id>.json`.
3. **Write conflict markers** in the affected file.
4. **Update branch state** to "conflicted".

### 12.2 Conflict Metadata

```json
{
  "id": "conflict-abc123",
  "file": "src/auth/handler.rs",
  "branch": "feature-auth",
  "current_snapshot": "def456...",
  "incoming_snapshot": "789abc...",
  "ancestor_snapshot": "012def...",
  "hunks": [
    {
      "start_line": 42,
      "end_line": 58,
      "type": "content",
      "current_content": "...",
      "incoming_content": "...",
      "ancestor_content": "..."
    }
  ],
  "detected_at": "2025-01-15T10:30:00Z"
}
```

### 12.3 Conflict Markers

W0rkTree uses improved conflict markers that include snapshot hashes and
branch context:

```
<<<<<<< current (feature-auth @ abc123)
    let token = validate_token(&request)?;
    let user = lookup_user(token.subject())?;
=======
    let token = verify_jwt(&request.headers)?;
    let user = find_user_by_token(&token)?;
>>>>>>> incoming (feature-auth @ def456, from: main)
```

### 12.4 Resolution Commands

| Command | Action |
|---|---|
| `wt merge resolve <file>` | Mark a file as resolved (conflicts manually fixed) |
| `wt merge resolve --all` | Mark all files as resolved |
| `wt merge abort` | Abort the merge, revert to pre-merge state |
| `wt merge finish` | Complete the merge (all conflicts must be resolved) |

After resolution:

1. Conflict metadata files are removed.
2. Branch state returns to "clean".
3. Auto-sync resumes for the affected branch.
4. A merge snapshot is created with the resolved content.

---

## 13. Reflog Management

### 13.1 What Gets Logged

Every operation that changes a branch tip or modifies tree state:

| Operation | Logged |
|---|---|
| Snapshot creation (auto) | ✅ |
| Snapshot creation (manual) | ✅ |
| Branch create | ✅ |
| Branch switch | ✅ |
| Branch delete | ✅ |
| Merge (auto and manual) | ✅ |
| Push | ✅ |
| Sync (remote update applied) | ✅ |
| Tag creation/deletion | ✅ |
| Revert | ✅ |
| Conflict resolution | ✅ |

### 13.2 Storage

```
.wt/reflog/
├── HEAD                        ← Current branch reflog
├── _global.log                 ← All operations across all branches
└── refs/
    ├── main.log                ← Reflog for main branch
    ├── feature-auth.log        ← Reflog for feature-auth branch
    └── ...
```

Each entry is a single line:

```
<new-hash> <old-hash> <operation> <timestamp> <tenant> <message>
```

Example:

```
abc123 def456 snapshot 2025-01-15T10:30:00Z alice auto: 3 files changed in backend
def456 789abc merge    2025-01-15T10:25:00Z alice merge main into feature-auth
789abc 012def push     2025-01-15T10:20:00Z alice push feature-auth (5 snapshots)
```

### 13.3 Sync to Server

Reflog entries are synced to the server as part of the normal sync cycle. The
server maintains a **complete reflog** across all tenants and machines. This enables:

- **Cross-machine recovery** — If a developer loses their machine, the reflog can
  be recovered from the server.
- **Audit trail** — Administrators can audit the full history of operations.
- **Undo across machines** — A developer can undo operations performed on a
  different machine.

### 13.4 Retention

```toml
# .wt/config.toml
[reflog]
retention = "90d"              # Keep entries for 90 days
max_entries = 10000            # Maximum entries per branch
sync_to_server = true          # Sync reflog to server (default: true)
```

Expired entries are pruned during periodic maintenance (every 6 hours by default).

---

## 14. CLI Communication (IPC)

### 14.1 Transport

The bgprocess communicates with the `wt` CLI via local IPC:

| Platform | Transport | Address |
|---|---|---|
| Linux | Unix domain socket | `/tmp/wt-worker-<worktree-hash>.sock` |
| macOS | Unix domain socket | `/tmp/wt-worker-<worktree-hash>.sock` |
| Windows | Named pipe | `\\.\pipe\wt-worker-<worktree-hash>` |

The `<worktree-hash>` is a stable hash of the worktree root path, ensuring
each worktree has its own IPC channel.

### 14.2 Protocol

IPC messages use a simple length-prefixed JSON protocol:

```
[4 bytes: message length (u32 big-endian)] [JSON payload]
```

Request:

```json
{
  "id": "req-001",
  "command": "status",
  "args": {
    "tree_id": "backend"
  }
}
```

Response:

```json
{
  "id": "req-001",
  "status": "ok",
  "data": {
    "tree_id": "backend",
    "branch": "feature-auth",
    "changed_files": 3,
    "watcher_active": true,
    "staged_snapshots": 2,
    "sync_status": "connected",
    "last_sync": "2025-01-15T10:30:00Z"
  }
}
```

### 14.3 Command Dispatch

All `wt` CLI commands go through the bgprocess. The CLI itself is a thin client
that sends IPC requests and formats responses for the terminal:

| CLI Command | IPC Command | Handler |
|---|---|---|
| `wt status` | `status` | Query watcher + engine state |
| `wt snapshot` | `snapshot.create` | Trigger manual snapshot |
| `wt snapshot list` | `snapshot.list` | Query snapshot store |
| `wt branch create` | `branch.create` | Create branch in snapshot store |
| `wt branch switch` | `branch.switch` | Switch branch (snapshot first if dirty) |
| `wt push` | `sync.push` | Finalize staged snapshots into history |
| `wt sync` | `sync.trigger` | Trigger immediate sync cycle |
| `wt diff` | `diff.compute` | Compute diff via protocol crate |
| `wt log` | `log.query` | Query snapshot DAG |
| `wt merge` | `merge.start` | Initiate merge operation |
| `wt reflog` | `reflog.query` | Query reflog entries |

### 14.4 Concurrency

The IPC server handles multiple concurrent CLI connections. Operations that
modify state (snapshot, branch switch, merge) are serialized through a
single-writer lock. Read-only operations (status, log, diff) can run concurrently.

---

## 15. Configuration

### 15.1 Configuration Sources

| Source | Scope | Precedence |
|---|---|---|
| Built-in defaults | Global | Lowest |
| Environment variables | Process | |
| `.wt/config.toml` | Worktree | |
| `.wt-tree/config.toml` | Tree | |
| CLI flags | Command | Highest |

### 15.2 Environment Variables

| Variable | Effect | Default |
|---|---|---|
| `WT_SYNC_AUTO` | Enable/disable auto-sync | `true` |
| `WT_SNAPSHOT_AUTO` | Enable/disable auto-snapshots | `true` |
| `WT_LOG_LEVEL` | Daemon log level (trace/debug/info/warn/error) | `info` |
| `WT_SYNC_INTERVAL` | Sync interval in seconds | `30` |
| `WT_WORKER_SOCKET` | Override IPC socket path | Platform default |
| `WT_STORAGE_DIR` | Override platform storage directory | Platform default |

### 15.3 Full Configuration Reference

```toml
# .wt/config.toml — complete bgprocess configuration

[worktree]
name = "my-project"
tenant = "alice"
visibility = "private"         # private | shared | public

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 300
max_changed_files = 50
max_changed_bytes = "50MB"
on_tree_switch = true
on_branch_switch = true
message_format = "auto: {file_count} files changed in {tree}"

[sync]
auto = true
server = "https://wt.acme.dev"
interval_secs = 30
timeout_secs = 60
retry_count = 3
retry_delay_secs = 5

[storage]
large_file_threshold = "10MB"
chunk_target_size = "1MB"
chunk_cache_max_size = "10GB"

[watcher]
debounce_ms = 200
ignore_patterns = []            # Additional patterns (additive to .wt/ignore)

[reflog]
retention = "90d"
max_entries = 10000
sync_to_server = true

[worker]
log_level = "info"
log_file = ""                   # Empty = platform default location
health_check_interval_secs = 60
maintenance_interval_secs = 21600   # 6 hours
snapshot_on_shutdown = true

[diff]
rename_threshold = 50           # Similarity % for rename detection
copy_detection = false
context_lines = 3

[security]
secrets_scanning = true
secrets_patterns = []           # Additional patterns to scan for
```

---

## 16. Platform Support

### 16.1 Platform Matrix

| Capability | Linux | macOS | Windows |
|---|---|---|---|
| **File watcher** | `notify` (inotify) | `notify` (FSEvents) | `notify` (ReadDirectoryChangesW) |
| **Large file VFS** | FUSE | FUSE-T | ProjFS |
| **IPC** | Unix domain socket | Unix domain socket | Named pipe |
| **Service manager** | systemd user service | launchd agent | Windows Service |
| **Storage path** | `~/.local/share/w0rktree/` | `~/Library/Application Support/W0rkTree/` | `%APPDATA%\W0rkTree\` |
| **PID file** | `.wt/worker.pid` | `.wt/worker.pid` | `.wt/worker.pid` |

### 16.2 Service Management

The bgprocess integrates with the platform service manager for automatic start
on login and restart on crash:

**Linux (systemd):**
```ini
[Unit]
Description=W0rkTree Worker for %i
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/wt worker start --worktree %i
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

**macOS (launchd):**
```xml
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.w0rktree.worker</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/wt</string>
        <string>worker</string>
        <string>start</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
```

**Windows (Service):**
The bgprocess registers as a Windows Service via the `windows-service` crate,
supporting start/stop/restart through the Services management console and
`sc.exe` command-line tool.

---

## 17. Security

### 17.1 Authentication

- **Auth tokens** stored in `.wt/identity/token`.
- **Token format**: JWT with expiration, signed by the server.
- **Token refresh**: The bgprocess automatically refreshes tokens before expiry.
- **No plaintext passwords**: Passwords are never stored locally. Only tokens.

### 17.2 Transport Security

- **All server communication encrypted**: TLS 1.3 for HTTP/2, QUIC for
  high-performance sync.
- **Certificate pinning**: Optional, configurable in `.wt/config.toml`.
- **No fallback to plaintext**: The bgprocess refuses to sync over unencrypted
  connections.

### 17.3 Local Security

- **IPC socket permissions**: Unix socket created with `0600` permissions (owner
  only). Named pipe created with owner-only ACL.
- **PID file**: Used for stale process detection, not for security.
- **Storage directory permissions**: Created with `0700` (owner only).
- **No world-readable state**: All local state files are owner-readable only.

### 17.4 Secrets Scanning

On snapshot creation, the bgprocess optionally scans changed files for secrets:

- **Default patterns**: AWS keys, private keys, API tokens, passwords in config.
- **Configurable patterns**: Additional regex patterns in `.wt/config.toml`.
- **Behavior on detection**: Warning in CLI output; optionally block snapshot.
- **Not a replacement for dedicated tools**: This is a lightweight safety net,
  not a comprehensive secrets scanner.

---

## 18. CLI Commands

### 18.1 Worker Management

| Command | Description |
|---|---|
| `wt worker start` | Start the bgprocess for the current worktree |
| `wt worker stop` | Gracefully stop the bgprocess |
| `wt worker restart` | Stop and restart the bgprocess |
| `wt worker status` | Show bgprocess status (uptime, sync state, watcher health) |
| `wt worker logs` | View bgprocess logs (tail mode by default) |
| `wt worker logs --follow` | Stream bgprocess logs in real-time |

### 18.2 Status Output

```
$ wt worker status

W0rkTree Worker Status
══════════════════════
  PID:              12345
  Uptime:           2h 15m
  Worktree:         /home/alice/projects/my-project
  Config:           .wt/config.toml (loaded)

Filesystem Watcher
  Status:           active
  Watched paths:    1,247
  Pending changes:  3 files

Auto-Snapshot
  Enabled:          yes
  Last snapshot:    2 minutes ago (auto: 3 files changed in backend)
  Next trigger:     inactivity timer (2m 58s remaining)

Sync Engine
  Server:           https://wt.acme.dev
  Status:           connected
  Last sync:        30 seconds ago
  Staged snapshots: 2 (pending upload: 0)
  Queue:            empty

Large Files
  VFS:              active (FUSE)
  Cached chunks:    1.2 GB / 10 GB
  Pending fetches:  0
```

---

## 19. Error Handling and Recovery

### 19.1 Error Categories

| Category | Examples | Recovery |
|---|---|---|
| **Transient** | Network timeout, server 503 | Automatic retry with backoff |
| **Permanent** | Invalid auth token, worktree deleted | Log error, notify user, pause affected subsystem |
| **Filesystem** | Permission denied, disk full | Log error, notify user, continue other operations |
| **Corruption** | Damaged object store, invalid snapshot | Log error, attempt repair, fall back to server |

### 19.2 Retry Policy

For transient errors (network, server overload):

```
Attempt 1: immediate
Attempt 2: 5 seconds
Attempt 3: 15 seconds
Attempt 4: 60 seconds
Then: back off to sync interval, continue retrying
```

### 19.3 Journal

Critical operations (snapshot creation, branch pointer updates) are journaled
before execution. If the bgprocess crashes mid-operation, the journal is replayed
on restart to ensure consistency.

---

## 20. Observability

### 20.1 Logging

- **Framework**: `tracing` crate with structured logging.
- **Log levels**: `trace`, `debug`, `info`, `warn`, `error`.
- **Output**: File (platform log directory) and optional stdout.
- **Rotation**: Log files are rotated daily, retained for 7 days by default.

### 20.2 Metrics

The bgprocess tracks internal metrics (exposed via `wt worker status` and
optionally via Prometheus endpoint):

| Metric | Type | Description |
|---|---|---|
| `wt_watcher_events_total` | Counter | Total filesystem events received |
| `wt_watcher_events_ignored` | Counter | Events dropped by ignore matcher |
| `wt_snapshots_created_total` | Counter | Snapshots created (auto + manual) |
| `wt_sync_cycles_total` | Counter | Sync cycles completed |
| `wt_sync_duration_seconds` | Histogram | Sync cycle duration |
| `wt_sync_bytes_uploaded` | Counter | Total bytes uploaded |
| `wt_sync_bytes_downloaded` | Counter | Total bytes downloaded |
| `wt_pending_changes` | Gauge | Files in pending changeset |
| `wt_staged_snapshots` | Gauge | Staged snapshots awaiting push |
| `wt_chunk_cache_bytes` | Gauge | Large file chunk cache size |

---

## 21. Implementation Status

### 21.1 What Exists Today

The following code exists in `worktree-server` but **belongs in the bgprocess**
(`worktree-worker`):

| Module | Location | Status |
|---|---|---|
| `Debouncer` | `worktree-server/src/watcher/debounce.rs` | ✅ Implemented with tests |
| `EventKind` | `worktree-server/src/watcher/debounce.rs` | ✅ Implemented |
| `DebouncedEvent` | `worktree-server/src/watcher/debounce.rs` | ✅ Implemented |
| `WatcherConfig` | `worktree-server/src/config/settings.rs` | ✅ Implemented |
| `AutoSnapshotConfig` | `worktree-server/src/config/settings.rs` | ✅ Implemented |
| `SemanticEvent` | `worktree-server/src/engine/event.rs` | ⚠️ Types defined, `classify_event` is `todo!()` |
| `Daemon` | `worktree-server/src/service/daemon.rs` | ⚠️ Skeleton, `start()` is `todo!()` |
| `handle_init` | `worktree-server/src/api/handlers.rs` | ⚠️ Skeleton, all handlers are `todo!()` |

### 21.2 What Exists in worktree-protocol

The protocol crate provides shared types and algorithms used by the bgprocess:

| Module | Purpose | Status |
|---|---|---|
| `feature::diff::compute` | Diff computation (`compute_diff`, `compute_diff_default`) | ✅ Fully implemented with tests |
| `feature::diff::delta` | Delta types (`Delta`, `DeltaKind`) | ✅ Implemented |
| `feature::diff::manifest` | Manifest types for diffing | ✅ Implemented |
| `core::id` | ID types (`TreeId`, `SnapshotId`, `BranchId`) | ✅ Implemented |
| `core::hash` | Content-addressable hashing | ✅ Implemented |

### 21.3 What Needs to Be Built

| Component | Priority | Complexity |
|---|---|---|
| **`worktree-worker` crate** | 🔴 Critical | High — new crate, extract from server |
| **IPC server** | 🔴 Critical | Medium — socket/pipe listener, JSON protocol |
| **Auto-snapshot engine** | 🔴 Critical | Medium — rule evaluation, changeset tracking |
| **Sync engine** | 🔴 Critical | High — delta sync, conflict detection |
| **Ignore pattern compiler** | 🟡 High | Low — glob set compilation |
| **Reflog writer** | 🟡 High | Low — append-only log files |
| **Large file manager** | 🟡 High | High — FastCDC, VFS integration |
| **Config hot-reload** | 🟢 Medium | Low — watch config files, recompile |
| **Service manager integration** | 🟢 Medium | Medium — systemd/launchd/Windows Service |
| **Secrets scanning** | 🔵 Low | Low — regex scanning on snapshot |
| **Metrics/Prometheus** | 🔵 Low | Low — counter/gauge exports |

---

## 22. Migration Plan

### Phase 1: Create `worktree-worker` Crate

1. Create `crates/worktree-worker/` with standard Cargo layout.
2. Add dependency on `worktree-protocol` (for shared types, diff, hash).
3. Move the following from `worktree-server`:
   - `watcher/debounce.rs` → `worktree-worker/src/watcher/debounce.rs`
   - `watcher/fs.rs` → `worktree-worker/src/watcher/fs.rs`
   - `engine/event.rs` → `worktree-worker/src/engine/event.rs`
   - `config/settings.rs` (watcher + auto-snapshot portions) → `worktree-worker/src/config/`
   - `service/daemon.rs` → `worktree-worker/src/daemon.rs`
4. Update `worktree-server` to remove extracted code and depend on `worktree-worker`
   only if needed for shared types (prefer protocol crate for shared types).

### Phase 2: Implement Core Subsystems

1. IPC server (Unix socket + named pipe).
2. Ignore pattern compiler.
3. Auto-snapshot engine (rule evaluation, pending changeset).
4. Reflog writer.

### Phase 3: Implement Sync

1. Staged snapshot upload.
2. Remote branch download.
3. Delta sync optimization.
4. Offline queue.

### Phase 4: Implement Advanced Features

1. Large file detection and chunking.
2. Virtual filesystem (FUSE/ProjFS).
3. Auto-merge engine.
4. Conflict detection and resolution.

### Phase 5: Production Readiness

1. Service manager integration.
2. Crash recovery and journaling.
3. Secrets scanning.
4. Metrics and observability.
5. Comprehensive test suite.

---

## Appendix A: Glossary

| Term | Definition |
|---|---|
| **bgprocess** | The local W0rkTree daemon running on the developer's machine |
| **staged snapshot** | A snapshot synced to the server but not yet part of branch history |
| **pushed snapshot** | A snapshot that is part of canonical branch history |
| **pending changeset** | In-memory set of tracked changes since the last snapshot |
| **sync cycle** | One complete upload + download + reconciliation pass |
| **sync cursor** | Position marker tracking what has been synced |
| **chunk** | A content-defined segment of a large file |
| **chunk manifest** | Ordered list of chunk references composing a large file |
| **debounce** | Collapsing rapid successive events to the same path into one event |
| **semantic event** | A classified filesystem event (code change, config change, etc.) |
| **IPC** | Inter-process communication between the CLI and bgprocess |
| **reflog** | Chronological log of all branch-tip-changing operations |

## Appendix B: Related Specifications

| Specification | Relevance |
|---|---|
| `specs/WorkTree.md` | Master specification; sections 3, 6, 9, 11, 12, 15 are directly relevant |
| `specs/dot-wt/` | `.wt/` directory structure and configuration |
| `specs/dot-wt-tree/` | `.wt-tree/` directory structure and per-tree configuration |
| `specs/sync/` | Sync protocol details (planned) |
| `specs/server/` | Remote server specification (planned) |
| `specs/storage/` | Object storage specification (planned) |
| `specs/security/` | Security model specification (planned) |