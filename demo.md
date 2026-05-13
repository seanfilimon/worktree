# W0rktree Demo

This document covers two demo paths:

- Local Rust loop: CLI -> bgprocess -> local Rust HTTP server -> SDK -> disk storage.
- Go server staged-sync loop: Rust SDK/bgprocess -> Go REST or gRPC server -> BLAKE3 object store -> file or Postgres staged metadata.

---

## Current Go Server Demo

The Go server is now the production-boundary demo for staged snapshots. It accepts the same
language-neutral staged snapshot contract from Rust clients, authenticates protected REST and gRPC
requests with bearer-token principals, verifies objects, authorizes the operation through the
policy-backed IAM seam, writes audit records, and stores staged metadata durably when Postgres is
configured.

### Start With Docker Compose

```powershell
Set-Location C:\Users\admin\Desktop\Programming\worktree\server-go
docker compose up --build
```

This starts:

| Service | Purpose |
|---|---|
| `postgres` | Durable staged snapshot metadata store |
| `migrate` | Applies `server-go/migrations/*.sql` |
| `server` | Go HTTP server on `8080` and gRPC server on `9877` |

The compose demo configures a development bearer principal:

| Variable | Demo value |
|---|---|
| `WT_SERVER_AUTH_MODE` | `bearer` |
| `WT_SERVER_AUTH_TOKEN` | `dev-secret` |
| `WT_SERVER_AUTH_TENANT` | `acme` |
| `WT_SERVER_AUTH_ACCOUNT` | `alice` |
| `WT_SERVER_AUTH_SCOPES` | `staged:*` |
| `WT_SERVER_IAM_MODE` | `policy` |

### Run Without Docker

```powershell
Set-Location C:\Users\admin\Desktop\Programming\worktree\server-go
$env:WT_SERVER_DATABASE_URL = "postgres://worktree:worktree@localhost:5432/worktree?sslmode=disable"
$env:WT_SERVER_GRPC_ADDR = "127.0.0.1:9877"
$env:WT_SERVER_AUTH_MODE = "bearer"
$env:WT_SERVER_AUTH_TOKEN = "dev-secret"
$env:WT_SERVER_AUTH_TENANT = "acme"
$env:WT_SERVER_AUTH_ACCOUNT = "alice"
$env:WT_SERVER_AUTH_SCOPES = "staged:*"
$env:WT_SERVER_IAM_MODE = "policy"
go run ./cmd/wt-server
```

If `WT_SERVER_DATABASE_URL` is unset, the server uses the development file store under
`.wt-server-go/staged/index.json`.

For a multi-principal demo, replace the single env token with file-backed demo credentials and
policy rules:

```powershell
$env:WT_SERVER_AUTH_CREDENTIALS_PATH = "examples/tokens.demo.json"
$env:WT_SERVER_IAM_POLICY_PATH = "examples/policy.demo.json"
Remove-Item Env:WT_SERVER_AUTH_TOKEN -ErrorAction SilentlyContinue
go run ./cmd/wt-server
```

`dev-secret` authenticates `alice` with `staged:*`; `viewer-secret` authenticates a viewer who can
list staged snapshots but is denied `staged:create`.

### Configure The Rust Client

```powershell
$env:WT_SERVER_URL = "http://127.0.0.1:8080"
$env:WT_TENANT = "acme"
```

The Rust staged request now sends:

```json
{
  "snapshot_id": "snap-1",
  "tenant": "acme",
  "worktree": "root",
  "tree_id": "root",
  "branch": "main",
  "objects": [
    {
      "path": "README.md",
      "hash": "<blake3-hex>",
      "size": 12,
      "content": "<base64-json-content>"
    }
  ]
}
```

`content` replaces the old `content_base64` field. Go decodes JSON bytes from base64 and verifies
the declared size and BLAKE3 hash before persisting.

### Verify HTTP

```powershell
Invoke-RestMethod http://127.0.0.1:8080/health
Invoke-RestMethod http://127.0.0.1:8080/ready
Invoke-RestMethod http://127.0.0.1:8080/metrics
$headers = @{ Authorization = "Bearer dev-secret" }
Invoke-RestMethod "http://127.0.0.1:8080/api/pull" -Headers $headers
```

### Verify The Production Boundary

The implemented slices now demonstrate:

| Area | Implemented behavior |
|---|---|
| Rust compatibility | `CanonicalPushReq` matches the Go JSON contract and uses `WT_SERVER_URL` / `WT_TENANT` |
| Object integrity | Server verifies 64-character BLAKE3 hash, content bytes, and declared size |
| Staged metadata | File store for dev, Postgres store when `WT_SERVER_DATABASE_URL` is set |
| Idempotency | Duplicate staged uploads are idempotent on tenant/worktree/tree/branch/snapshot plus canonical object refs; conflicting retries return `409 Conflict` over REST and `AlreadyExists` over gRPC |
| Auth | Protected REST endpoints require a bearer token; gRPC uses an auth interceptor and bearer metadata |
| IAM seam | Default-deny policy authorizer checks `staged:create` and `staged:list`; optional JSON policy files demonstrate allow/deny behavior; `AllowAllAuthorizer` is test/dev-only |
| Audit | Staged allow/deny decisions are recorded |
| gRPC | `SyncService.StageSnapshot` and `SyncService.ListStagedSnapshots` on port `9877` |
| Packaging | Dockerfile and Compose stack with Postgres and migrations |

---

## Local Rust Demo

Demo of local end-to-end stack: CLI -> bgprocess -> HTTP server -> SDK -> disk storage.
Everything runs on one machine. No remote, no TLS, no production auth.

## What Was Built

| Component | Before | After |
|---|---|---|
| Object storage (`disk.rs`) | stub — no disk writes | BLAKE3 fan-out layout, store/retrieve working |
| Event classification (`event.rs`) | types defined, no logic | classifies Cargo.toml, .editorconfig, .rs, etc. |
| Auto-commit engine (`auto_commit.rs`) | `todo!()` | threshold logic + semantic message generation |
| API handlers (`handlers.rs`) | `todo!()` | 4 handlers wired to SDK (init/status/snapshot/branch) |
| HTTP server (`lib.rs`) | `run()` returned immediately | Axum on `127.0.0.1:9876`, 5 routes, watcher task |
| Daemon (`service/daemon.rs`) | `todo!()` | delegates to `run()` |
| CLI server start/stop | wrote own PID, spawned nothing | spawns actual `worktree-server` binary, kills by PID |
| SDK push/pull (`sync.rs`) | returned fake local data | staged push now posts to Go-compatible `/api/push`; pull uses `/api/pull` |

---

## Setup

```powershell
# From repo root — build both binaries (wt + worktree-server go to target/release/)
cargo build --release

# IMPORTANT: use explicit variable — do NOT use bare 'wt' command on Windows
# because Windows Terminal has its own 'wt.exe' that intercepts it.
$wt = "C:\Users\admin\Desktop\Programming\worktree\target\release\wt.exe"

# Create fresh demo directory
$demo = "$env:TEMP\wt-demo"
New-Item -ItemType Directory -Force $demo | Out-Null
Set-Location $demo
```

> **Windows Terminal conflict:** `wt` is also Windows Terminal's launcher binary.
> Running bare `wt` will open a new terminal tab, not the CLI.
> Always use `& $wt <command>` or the full path during this demo.

---

## Demo Script

### 0 — Authentication and Real-Time Setup

1. Login to the remote server to retrieve a 7-day JWT:
   ```bash
   wt auth login --secret dev-secret
   ```

2. Open a dedicated terminal pane for the real-time team dashboard:
   ```bash
   wt staged --watch
   ```
   *(Leave this running. As your teammates type, snapshots will appear here instantly via WebSocket.)*

### 1 — Initialize a worktree

```powershell
& $wt init .
```

**Expected output:**
```
  Initializing worktree at '.'...
✓ Worktree repository initialized at 'C:\Users\...\wt-demo'
  Location     C:\Users\...\wt-demo
  Default branch  main
  Default tree    root
```

**What happened:** Created `.wt/` with objects/, refs/branches/, state.json, config.toml, ignore.
This is the local store — not inside the repo (unlike `.git/`).

---

### 2 — Start the server

```powershell
& $wt server start
```

**Expected output:**
```
✓ Server started (PID 12345)
  Address  http://127.0.0.1:9876
  Log      C:\..\.wt\cache\server.log
  Watch logs: wt server logs  |  Stop: wt server stop
```

**What happened:** CLI found `worktree-server.exe` next to itself (via `current_exe().parent()`),
spawned it detached with stdout+stderr piped to `.wt/cache/server.log`, wrote the real PID to `.wt/cache/bgprocess.pid`.

**Verify server is alive:**
```powershell
Invoke-RestMethod http://127.0.0.1:9876/health
# → @{status=ok}
```

**Open second terminal to watch live logs:**
```powershell
& $wt server logs
# Streams .wt\cache\server.log live. Ctrl-C to stop.
```

---

### 3 — Create some files

```powershell
New-Item -ItemType Directory src
"fn main() { println!(\"Hello W0rktree\"); }" | Set-Content src/main.rs
"[package]`nname = `"demo`"`nversion = `"0.1.0`"" | Set-Content Cargo.toml
```

---

### 4 — Check status

```powershell
& $wt status
```

**Expected output:**
```
── Worktree Status ──────────────────
  Tree       root
  Branch     main
  Snapshots  0

⚠ 2 change(s) detected:

  + Cargo.toml
  + src/main.rs
```

**What happened:** Walks the working directory, hashes each file with BLAKE3, diffs against
last snapshot's file list. No staging area — all untracked files show as added.

---

### 5 — Watch bgprocess auto-snapshot (live)

Edit a file — the watcher detects it and auto-creates a snapshot within ~500ms:

```powershell
"fn main() { println!(\"Auto-snapshot test\"); }" | Set-Content src/main.rs
```

**In the `wt server logs` terminal:**
```
bgprocess: auto-snapshot a3f8b2c1 on branch main — auto-snapshot: 1 file(s) changed
```

**What happened:** `FileSystemWatcher` (notify crate) fired an event → `Debouncer` collapsed rapid
events → `classify_event()` → `AutoCommitEngine::evaluate()` returned `Some(msg)` →
`create_snapshot()` called → snapshot written to `.wt/state.json`. Zero CLI interaction required.

---

### 6 — Create a snapshot (manual / optional)

Because W0rktree continuously auto-snapshots your work, manual snapshots often return `no changes to snapshot` if the daemon already saved your state. Manual snapshots are primarily used to tag a specific moment in the continuous stream with a human-readable message.

```powershell
& $wt snapshot -m "initial commit"
# If the daemon already captured all your changes, this will safely say "no changes to snapshot".
```

**Expected output:**
```
  Creating snapshot on tree '(current)': "initial commit"
✓ Snapshot created: a1b2c3d4
  ID         a1b2c3d4-...
  Message    initial commit
  Author     <your username>
  Timestamp  2026-05-11T...
  Tree       root
  Branch     main
  Files      2
```

**What happened:** SDK collected all files, BLAKE3-hashed each, created a `SnapshotState`
(UUID, author, timestamp, file list with hashes), wrote to `.wt/state.json`, advanced
branch tip. History is append-only — no rebase, no reset.

---

### 7 — Staged Visibility (Real-Time Team Collaboration)

Open a **second terminal** and run the watch command to see teammates' unpushed snapshots streaming in live via WebSockets:

```powershell
# In terminal 2
& $wt staged --watch
```

**In terminal 1:**
Make a quick edit.

```powershell
"fn update() {}" >> src/main.rs
```

**What happens:**
1. The background process automatically creates a snapshot (`auto-snapshot`).
2. It pushes it to `/staged` on the server.
3. The server broadcasts it via WebSockets.
4. Terminal 2 instantly shows the new staged snapshot from your teammate.

---

### 8 — Local Branching and 3-Way Snapshot Merges (Embracing Auto-Snapshots)

Demonstrate the local 3-way merge resolution algorithm. In W0rktree, the background daemon auto-snapshots every file change instantly. We don't even need manual `wt snapshot` commands to build the DAG graph!

```powershell
# 1. Branch out and make a feature
& $wt branch create feature-a
& $wt branch switch feature-a
"fn feature() { println!('Feature A'); }" >> src/feature.rs
# (Wait ~500ms. The background daemon auto-snapshots this directly into the feature-a branch history!)

# 2. Switch back and make a parallel change on main
& $wt branch switch main
"fn other() { println!('Other'); }" >> src/other.rs
# (Wait ~500ms. The daemon auto-snapshots this into main!)

# 3. Merge the parallel timelines locally!
& $wt merge feature-a
```

**Expected output:**
```
  Merging branch 'feature-a' (strategy: auto)
✓ Merged branch 'feature-a' into current branch
  Snapshot   <uuid>
  Files merged  3
```

**What happened:** The SDK traversed the snapshot DAG (Breadth-First Search) to find the Most Recent Common Ancestor (MRCA), evaluated the 3-way difference matrix, detected no conflicts, updated the physical files on disk, and created a merge snapshot.

---

### 9 — Pause, Resume, and Backfill (The Core Sync Loop)

Demonstrate how the decoupled background queue handles pauses and automatically catches up.

1. **Pause Sync**
```powershell
& $wt sync pause
```
*Expected: `.wt/cache/sync_paused` is created. Background push thread will now skip uploads.*

2. **Make changes while paused**
```powershell
"More changes" >> file1.txt
```
*Wait ~1 second for the auto-snapshot to trigger.*

3. **Verify the snapshot was created but not pushed**
Check the `worktree-server` logs. You should see `bgprocess: auto-snapshot ...` but NO successful upload message.

4. **Resume Sync**
```powershell
& $wt sync resume
```
*Expected: The bgprocess detects the file deletion and triggers a backfill.*

5. **Verify Backfill**
Check the `worktree-server` logs. You should see:
`bgprocess: sync resumed, backfilling...`
followed by the successful staged upload of the snapshots created while paused.

### 10 — Show server status via CLI

```powershell
& $wt server status
```

**Expected output:**
```
── Server Status ──────────────────
  Worktree         wt-demo
  Status           running
  PID              12345
  Address          http://127.0.0.1:9876
  Auto-sync        enabled
  Trees            1
  Total snapshots  2
```

---

### 11 — Stop the server

```powershell
& $wt server stop
```

**Expected output:**
```
✓ Server stopped (PID 12345)
```

**What happened:** Read PID from `.wt/cache/bgprocess.pid`, ran `taskkill /F /PID 12345`,
deleted the PID file.

**Verify server is gone:**
```powershell
Invoke-RestMethod http://127.0.0.1:9876/health
# → Connection refused
```

## Architecture Notes (for technical questions)

### Two-runtime boundary — where it lives in code
- **bgprocess** (local daemon): `worktree-server` binary — watcher, auto-commit engine, HTTP
- **SDK** (`worktree-sdk`): pure local I/O, no network — used by both CLI and server handlers
- Server handlers call SDK to operate on disk. This is a **demo compromise** — production server
  would have independent object storage and not share the SDK's local engine.

### Content-addressed storage layout
```
.wt/
├── objects/
│   ├── a1/              ← first 2 hex chars of BLAKE3 hash
│   │   └── b2c3...      ← remaining 62 chars = full file
│   └── ...
├── refs/branches/main   ← branch tip pointer
└── state.json           ← full tree/snapshot/branch state
```
Same fan-out as Git's object store. Max 256 top-level dirs. Random-access is O(1) filesystem lookup.

### Auto-snapshot pipeline
```
FileSystemWatcher (notify crate)
    ↓  raw OS events (std::sync::mpsc)
Debouncer (500ms window)
    ↓  deduplicated DebouncedEvents
classify_event()
    ↓  SemanticEvent: CodeChange / DependencyChange / ConfigChange
AutoCommitEngine::evaluate()
    ↓  threshold check + priority (dep > config > code)
    → Some("auto-snapshot: dependency changes")  ← triggers snapshot
    ↓
snapshot::create_snapshot()
    ↓
Push Queue (MPSC)  ← Decoupled from watcher thread
    ↓
Background Worker Thread  ← Handles network I/O
    ↓
sync::push_staged()
```

### Why blocking HTTP in SDK (not async)
SDK has no Tokio runtime — it's a pure sync library used by 20 CLI commands.
Adding `reqwest::blocking` keeps every call site as `fn push()` not `async fn push()`.
Zero changes to CLI code.

While the SDK is blocking, the **bgprocess** decouples this by spawning a dedicated background worker thread and communicating via an MPSC channel. This ensures that even if a large blob upload takes several seconds, the filesystem watcher thread remains responsive and continues to track new changes.

The bgprocess also tracks the `remote_tip` of each branch. If sync is paused, snapshots are queued locally. When sync is resumed (e.g. `wt sync resume`), the bgprocess detects the deletion of the `sync_paused` sentinel and triggers a "backfill" operation to upload all missing snapshots since the last known `remote_tip`.

### Binary discovery
`wt server start` uses `std::env::current_exe().parent()` to find `worktree-server.exe`
as a sibling binary. Works automatically when both are built to `target/release/` or
installed together. No hardcoded paths, no `$PATH` search.

---

## Likely Questions

**Q: "Does the server enforce IAM?"**
The Rust local demo endpoints are open. The Go server now has the IAM enforcement seam wired into
REST and gRPC staged create/list through `Authorizer`, with `AllowAllAuthorizer` as the default
development implementation and `DenyAllAuthorizer` available for tests. Full policy parsing and
JWT/API-key identity are still planned.

**Q: "What happens on concurrent snapshots?"**
State file is written atomically (temp file + rename). Concurrent CLI + server calls to
`create_snapshot` on the same tree will serialize via OS file rename atomicity. No distributed
locking — single machine only.

**Q: "Why did the server call the SDK? Shouldn't server have its own storage?"**
Yes, in production. For the local demo the server is essentially a thin HTTP wrapper around
the same SDK the CLI uses. The `root_path` in each request tells the server which directory
to open. This wouldn't work across machines — intentional limitation of this milestone.

**Q: "How does the watcher trigger auto-snapshots?"**
The watcher loop runs in a background thread inside `worktree-server`. File events flow through
`Debouncer` → `classify_event()` → `AutoCommitEngine::evaluate()`. When evaluate returns
`Some(msg)`, `create_snapshot()` is called immediately via the SDK. Snapshots appear in
`.wt/state.json` and the log shows `bgprocess: auto-snapshot <id> on branch <name> — <msg>`.
`SdkError::NoChanges` is swallowed silently (normal when files haven't changed between ticks).

**Q: "What's the BLAKE3 manifest hash in the snapshot response?"**
Concatenation of all file content hashes (already BLAKE3), hashed again. Gives a single
fingerprint of the entire tree state. Any file change = different manifest hash. This is
the root of the content-addressed DAG.

**Q: "What's still needed before a real multi-user demo?"**
1. JWT/API-key identity and real IAM policy evaluation
2. WebSocket push for staged visibility
3. Branch push to finalize staged snapshots into canonical history
4. Tenant quota accounting and retention cleanup

---

## What to Show if Something Breaks

| Symptom | Likely cause | Fix |
|---|---|---|
| `Server may already be running` | Old PID file left over | `Remove-Item .wt\cache\bgprocess.pid` |
| `worktree-server binary not found` | Binary not in same dir as wt | Run from `target/release/` or add to PATH |
| Windows Terminal opens instead of CLI | Bare `wt` hits Windows Terminal binary | Use `& $wt` not bare `wt` |
| `Connection refused` on push | Server not started | `& $wt server start` first |
| `No changes to snapshot` on second snapshot | Files didn't change | Edit a file first |
| `Not a worktree` | Not in initialized directory | `& $wt init .` first |
