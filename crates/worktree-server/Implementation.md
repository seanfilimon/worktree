# Implementation Details — `worktree-server`

The `worktree-server` crate is the **background daemon** for the Worktree version control system. It watches the filesystem for changes, automatically creates snapshots, manages synchronization with remote servers, enforces access control, and provides a gRPC API for programmatic control. It is designed to run as a persistent system service.

---

## Crate Metadata

- **Name:** `worktree-server`
- **Binary:** `worktree-server`
- **Edition:** 2021
- **Dependencies:** `worktree-protocol` (sibling), `worktree-git` (sibling), `tokio` 1 (full), `serde` 1 (derive), `toml` 0.8, `tracing` 0.1, `tracing-subscriber` 0.3, `thiserror` 1, `notify` 7, `uuid` 1 (v4), `chrono` 0.4 (serde)

---

## Architecture

```
src/
├── lib.rs              # 9 public modules + run() stub
├── main.rs             # Binary entry point (tokio::main)
├── error.rs            # ServerError (8 variants)
├── config/
│   ├── mod.rs
│   └── settings.rs     # ServerConfig (TOML-based)
├── api/
│   ├── mod.rs
│   ├── grpc.rs         # GrpcServer placeholder
│   └── handlers.rs     # 4 request/response pairs + handler stubs
├── auth/
│   ├── mod.rs
│   ├── session.rs      # Session (time-limited bearer tokens)
│   └── enforcer.rs     # PermissionEnforcer (in-memory RBAC)
├── engine/
│   ├── mod.rs
│   ├── event.rs        # SemanticEvent classification
│   ├── auto_commit.rs  # AutoCommitEngine (threshold-based)
│   ├── auto_branch.rs  # AutoBranchEngine (event-based)
│   └── rules.rs        # Declarative Rule/Condition/Action system
├── git/
│   ├── mod.rs
│   ├── import.rs       # GitImportService
│   ├── export.rs       # GitExportService + ExportMode
│   ├── remote.rs       # GitRemoteService
│   ├── mirror.rs       # GitMirrorService
│   └── submodule.rs    # SubmoduleService
├── service/
│   ├── mod.rs
│   ├── daemon.rs       # Daemon (lifecycle coordinator)
│   ├── health.rs       # HealthTracker + HealthStatus
│   └── install.rs      # Platform service installer
├── storage/
│   ├── mod.rs
│   ├── backend.rs      # StorageBackend trait
│   ├── disk.rs         # DiskStorage (Git-style fan-out)
│   └── index.rs        # ObjectIndex (in-memory hash→kind)
├── sync/
│   ├── mod.rs
│   ├── transport.rs    # Transport (QUIC/TCP)
│   ├── push.rs         # PushOperation
│   └── pull.rs         # PullOperation
└── watcher/
    ├── mod.rs
    ├── fs.rs           # FileSystemWatcher (notify crate)
    └── debounce.rs     # Debouncer (event deduplication)
```

---

## Data Flow Architecture

The server processes filesystem changes through a multi-stage pipeline:

```
FileSystemWatcher (notify)
    │ raw OS events
    ▼
Debouncer (collapse rapid events)
    │ DebouncedEvent batches
    ▼
classify_event() → SemanticEvent
    │ CodeChange / DependencyChange / ConfigChange / CrossTreeChange
    ▼
┌───────────────┬──────────────────┬─────────────┐
│ AutoCommit    │ AutoBranch       │ Rules       │
│ Engine        │ Engine           │ Engine      │
│ (thresholds)  │ (event count)    │ (declarative│
│               │                  │  conditions)│
└───────┬───────┴────────┬─────────┴──────┬──────┘
        │                │                │
        ▼                ▼                ▼
    API Handlers (snapshot/branch/init/status)
        │
    ┌───┼───────────┬──────────────┐
    ▼   ▼           ▼              ▼
Storage  Auth      Sync         Git Layer
(Disk)   (RBAC)   (QUIC/TCP)   (Import/Export)
```

---

## Error Handling

`ServerError` has 8 variants — one per subsystem:

| Variant | Source |
|---------|--------|
| `Config` | Configuration parsing |
| `Io` | `std::io::Error` (auto-conversion) |
| `Watcher` | FS watcher failures |
| `Storage` | Object store errors |
| `Engine` | Automation engine errors |
| `Auth` | Authentication/authorization |
| `Api` | API layer errors |
| `Git` | Git interoperability |

---

## `config::settings` — Server Configuration

`ServerConfig` is loaded from a TOML file via `ServerConfig::load(path)`:

| Section | Fields | Defaults |
|---------|--------|----------|
| Top-level | `data_dir`, `listen_addr` | — |
| `auto_snapshot` | `enabled`, `inactivity_timeout_secs`, `max_changed_files` | `true`, `30`, `50` |
| `watcher` | `debounce_ms`, `ignore_patterns` | `200`, `[.git/**, .worktree/**, target/**, node_modules/**]` |

---

## `watcher/fs` — File System Watcher

**Fully implemented.**

`FileSystemWatcher` wraps `notify::RecommendedWatcher` (platform-native FS events):
- `new()` — Creates watcher + `mpsc` channel for event delivery.
- `watch(path)` — Start recursive watching.
- `unwatch(path)` — Stop watching.
- `stop()` — Drop and recreate watcher to clear all watches.

Events arrive on `self.receiver` as `Result<notify::Event, notify::Error>`.

---

## `watcher/debounce` — Event Deduplication

**Fully implemented with tests.**

`Debouncer` collapses rapid filesystem events on the same path within a configurable time window:

- `push(event)` — If an event for the same path exists within `delay_ms`, replaces it (keeps latest timestamp and kind). Otherwise adds new event.
- `flush()` — Returns events whose window has elapsed (they've "settled"). Events are sorted chronologically.
- `flush_all()` — Drains everything (for shutdown).

`DebouncedEvent` carries `path: PathBuf`, `kind: EventKind` (Created/Modified/Deleted/Renamed), and `timestamp: DateTime<Utc>`.

Tests verify: same-path deduplication, different-path preservation, `flush_all` draining.

---

## `engine/event` — Semantic Event Classification

`SemanticEvent` enum classifies raw FS events into meaningful categories:

| Variant | When |
|---------|------|
| `CodeChange { tree_id, paths }` | Source code files changed |
| `DependencyChange { tree_id, path }` | `Cargo.toml`, `package.json`, etc. |
| `ConfigChange { tree_id, path }` | `.wt/config.toml`, `.wt-tree/config.toml` |
| `CrossTreeChange { tree_ids, paths }` | Changes spanning multiple trees |

`classify_event(raw)` inspects file extensions and names — currently a `todo!()` stub.

---

## `engine/auto_commit` — Automatic Snapshot Engine

`AutoCommitEngine` decides when to auto-snapshot:
- `min_event_threshold: 1` — Minimum events before considering.
- `max_event_threshold: 100` — Force snapshot above this count.
- `evaluate(events)` — Returns `Option<String>` commit message. Stub.

---

## `engine/auto_branch` — Automatic Branch Suggestion

`AutoBranchEngine` suggests new branch names when events exceed a threshold (default 5). For example, large refactors or new dependency additions might trigger automatic branch isolation.

---

## `engine/rules` — Declarative Automation

Serializable rule system:

**Conditions:** `FileCountExceeds(usize)`, `InactivityTimeout(u64)`, `PathPattern(String)`

**Actions:** `CreateSnapshot(message)`, `CreateBranch(name)`, `Notify(message)`

`Rule { name, condition, action }` — evaluated when events arrive. All types implement `Display` for human-readable output.

---

## `auth/session` — Authentication Sessions

**Fully implemented with tests.**

`Session` binds a `user_id: AccountId` to a `token: String` with `expires_at: DateTime<Utc>`:
- `is_expired()` — Past expiry time.
- `remaining()` — Time until expiry (`None` if expired).

Tests verify: future session not expired, past session is expired.

---

## `auth/enforcer` — Permission Enforcement

**Fully implemented with 6 tests.**

`PermissionEnforcer` stores `Vec<PermissionGrant>` where each grant is `(AccountId, Permission, Scope)`:
- `grant(user_id, permission, scope)` — Add a grant.
- `check(user, permission, scope) -> bool` — Uses `Scope::covers()` from `worktree-protocol` for hierarchical matching (Global covers Tree).
- `revoke_all(user) -> usize` — Remove all grants for a user.

Tests cover: no grants → deny, exact match → allow, Global covers Tree, different tree → deny, wrong permission → deny, revoke_all.

---

## `api/grpc` — gRPC Server

`GrpcServer` skeleton for future tonic integration:
- Tracks `addr: String` and `running: bool`.
- `start()` / `stop()` are `todo!()` stubs.

---

## `api/handlers` — API Request Handlers

4 request/response pairs, all stubs:

| Handler | Request | Response | Purpose |
|---------|---------|----------|---------|
| `handle_init` | `name`, `root_path` | `tree_id` | Create new tree |
| `handle_status` | `tree_id` | `branch`, `changed_files`, `watcher_active` | Query state |
| `handle_snapshot` | `tree_id`, `message` | `snapshot_id`, `manifest_hash` | Create snapshot |
| `handle_branch` | `tree_id`, `branch_name`, `create` | `branch_id`, `branch_name` | Branch management |

---

## `storage/backend` — Storage Abstraction

`StorageBackend` trait (`Send + Sync`):
- `store(hash, data)` — Write content-addressed blob.
- `retrieve(hash)` — Read blob by hash.
- `exists(hash)` — Check existence.
- `delete(hash)` — Default no-op.
- `object_count()` — Default `None`.

---

## `storage/disk` — Disk-Based Storage

**Partially implemented (path logic + exists).**

`DiskStorage` uses Git-style fan-out: `<root>/objects/<XX>/<YYYYYY...>` where `XX` = first 2 hex chars of `ContentHash`, remainder = filename.

- `object_path(hash)` — Full path to object file.
- `fan_out_dir(hash)` — Directory path (2-char prefix).
- `exists(hash)` — Delegates to `Path::exists()`.
- `store()` / `retrieve()` — `todo!()` stubs.

Tests validate: path structure, fan-out directory, exists returns false for missing.

---

## `storage/index` — In-Memory Object Index

**Fully implemented with 7 tests.**

`ObjectIndex` maps `HashMap<ContentHash, String>` (hash → kind label: `"blob"`, `"manifest"`, `"snapshot"`, `"delta"`):
- Full CRUD: `insert`, `lookup`, `remove`, `clear`, `contains`, `is_empty`, `count`, `iter`.

---

## `sync/transport` — Transport Protocol

**Fully implemented with tests.**

`Transport` enum: `Quic(addr)` and `Tcp(addr)`:
- `address()` / `protocol_name()` — Accessors.
- `connect()` — `todo!()` stub.

---

## `sync/push` / `sync/pull` — Sync Operations

Builder-pattern structs:

**`PushOperation`:** `tree_id`, `branch`, `remote`, `transport`, `force` flag. `execute()` stub.

**`PullOperation`:** `remote`, optional `branch`, `fast_forward_only`. `execute()` stub.

---

## `git/import` — Git Import Service

`GitImportService` with branch filtering and shallow mode:
- Uses `worktree_git::import::repo::GitRepo::open()` from sibling crate.
- Lists branches, filters if `branch_filter` is set.
- Commit walking + conversion is `todo!()`.

---

## `git/export` — Git Export Service

`ExportMode` enum: `Full`, `Squashed`, `Shallow(usize)`, `SingleTree`.

`GitExportService::export(tree_id, output, mode)` — `todo!()` stub.

---

## `git/remote` — Git Remote Management

`GitRemoteService` manages `(name, url)` pairs for a specific tree:
- `add_remote`, `remove_remote` — Validates duplicates/existence.
- `push`, `pull` — Validate remote exists first, then `todo!()`.

---

## `git/mirror` — Continuous Mirroring

`GitMirrorService` tracks active `MirrorEntry` triples `(tree_id, remote, branch)`:
- `start_mirror` — Prevents duplicates.
- `stop_mirror` — Removes entry.
- `is_mirroring`, `active_count` — Query state.

---

## `service/daemon` — Lifecycle Coordinator

`Daemon` uses `Arc<AtomicBool>` for thread-safe running state:
- `start()` — Guards against double-start. Will launch all subsystems.
- `stop()` — Guards against double-stop. Will gracefully shut down.
- `is_running()` — Atomic read.

---

## `service/health` — Health Monitoring

`HealthTracker` records `Instant::now()` at construction:
- `set_trees_watched(count)`, `record_snapshot()` — Update counters.
- `status()` — Produces `HealthStatus { uptime_secs, trees_watched, snapshots_created }`.

---

## `service/install` — Platform Service Installation

Platform-dispatched via `cfg!(target_os)`:
- **Linux:** systemd unit file installation.
- **macOS:** launchd plist installation.
- **Windows:** `sc.exe` service registration.
- All platform implementations are `todo!()` stubs.

---

## Implementation Maturity

| Component | Status |
|-----------|--------|
| `error` | ✅ Complete |
| `config::settings` | ✅ Complete |
| `watcher::fs` | ✅ Complete |
| `watcher::debounce` | ✅ Complete (with tests) |
| `auth::session` | ✅ Complete (with tests) |
| `auth::enforcer` | ✅ Complete (with 6 tests) |
| `storage::index` | ✅ Complete (with 7 tests) |
| `storage::disk` | 🔶 Partial (paths + exists only) |
| `storage::backend` | ✅ Trait defined |
| `sync::transport` | ✅ Complete (with tests) |
| `engine::rules` | ✅ Types complete |
| `engine::event` | 🔶 Types only (classify stub) |
| `engine::auto_commit` | 🔶 Structure only (evaluate stub) |
| `engine::auto_branch` | 🔶 Structure only (evaluate stub) |
| `api::grpc` | 🔶 Skeleton (start/stop stubs) |
| `api::handlers` | 🔶 All stubs |
| `git::*` | 🔶 Structures defined, logic stubs |
| `service::daemon` | 🔶 State management (start/stop stubs) |
| `service::health` | ✅ Complete |
| `service::install` | 🔶 All platform stubs |
| `sync::push/pull` | 🔶 Structures defined (execute stubs) |

---

## TODO

- [ ] Implement `DiskStorage::store` and `DiskStorage::retrieve` with file I/O
- [ ] Implement `classify_event` — inspect file extensions to categorize semantic events
- [ ] Implement `AutoCommitEngine::evaluate` — analyze events, decide on snapshot, generate message
- [ ] Implement `AutoBranchEngine::evaluate` — suggest branch names based on event patterns
- [ ] Implement `GrpcServer::start` — bind tonic gRPC service and serve RPCs
- [ ] Implement `GrpcServer::stop` — graceful shutdown
- [ ] Implement all API handlers (init, status, snapshot, branch)
- [ ] Implement `Daemon::start` — launch watcher, engine, API, and sync subsystems
- [ ] Implement `Daemon::stop` — graceful shutdown of all subsystems
- [ ] Implement `PushOperation::execute` — connect transport, negotiate objects, upload
- [ ] Implement `PullOperation::execute` — connect, download, integrate
- [ ] Implement platform service installers (systemd, launchd, Windows Service)
- [ ] Implement `GitImportService::import` — full commit walking + object conversion
- [ ] Implement `GitExportService::export` — snapshot → Git commit pipeline
- [ ] Add metrics collection and Prometheus endpoint
- [ ] Implement rate limiting for API requests
- [ ] Add graceful degradation when watcher encounters permission errors
- [ ] Implement object garbage collection for unreferenced blobs
- [ ] Add TLS/mTLS support for gRPC server
- [ ] Implement WebSocket event streaming for real-time UI updates