# Contribution Progress

## Go Server — Implementation Progress

Target: production remote authority in `server-go/`, built from the language-neutral protocol
contract in `docs/protocol-spec.md` and the deployment sequence in `roadmap.md`.

### Step 1 — `docs(protocol): connect server roadmap to protocol contract`
**Files:** `docs/protocol-spec.md`, `roadmap.md`
- Connected the consolidated protocol spec to the Go server roadmap.
- Established that protocol behavior must be specified before Rust/Go implementation.
- Added explicit cross-language compatibility expectations.

### Step 2 — `feat(server-go): scaffold remote server runtime`
**Files:** `server-go/`
- Added a standalone Go module for the production remote server.
- Added standard-library HTTP server setup with graceful shutdown.
- Added JSON logging through `log/slog`.
- Added request ID middleware.
- Added `GET /health` and `GET /ready`.
- Added TLS 1.3 minimum-version configuration.

### Step 3 — `feat(server-go): add staged object upload`
**Files:**
- `server-go/internal/storage/object.go`
- `server-go/internal/staged/store.go`
- `server-go/internal/httpapi/staged.go`
- `server-go/README.md`
- `docs/protocol-spec.md`
- `docs/server-architecture.md`
- `roadmap.md`

- Added local content-addressed object storage with BLAKE3 verification.
- Added `POST /staged` compatibility endpoint.
- Added JSON staged metadata persistence for local development.
- Added tests for object verification and staged upload behavior.
- Updated docs to describe the Go endpoint as the current compatibility bridge.

**Result:** `go test ./...` passes in `server-go` with workspace-local Go cache settings.

## Local Demo — Implementation Progress

Target: full local demo (CLI → bgprocess → server on localhost). No remote server needed.

### Step 1 — `feat(server): implement DiskStorage store/retrieve`
**File:** `crates/worktree-server/src/storage/disk.rs`
- `store()`: creates fan-out dir (`objects/XX/`) then writes object bytes atomically
- `retrieve()`: reads bytes by content hash; returns `ServerError::Storage` on miss
- Added 2 new tests: round-trip store→retrieve, missing-object error
- Added `tempfile = "3"` to `[dev-dependencies]` in `worktree-server/Cargo.toml`

**Also fixed:** pre-existing compile break in `auth/enforcer.rs` — `Scope::Tree` variant grew
a `TenantId` arg but 3 test call sites weren't updated. Moved `TenantId` import into
`#[cfg(test)]` to keep production code warning-free.

### Step 2 — `feat(server): implement classify_event`
**File:** `crates/worktree-server/src/engine/event.rs`
- Classifies `DebouncedEvent` → `SemanticEvent` by filename / path prefix:
  - `Cargo.toml`, `package.json`, `go.mod`, etc. → `DependencyChange`
  - `.wt/` or `.wt-tree/` path components, `.editorconfig`, `.gitignore`, etc. → `ConfigChange`
  - Everything else → `CodeChange`
- Uses `TreeId::nil()` placeholder (tree registry not yet wired)
- Added 6 unit tests covering all three variants + path membership check

**Result:** `cargo test -p worktree-server` → 32/32 pass, 0 warnings in server crate.

### Step 3 — `feat(sync): upload auto-snapshots as staged snapshots`
**Files:**
- `crates/worktree-sdk/src/engine/sync.rs`
- `crates/worktree-server/src/lib.rs`
- `crates/worktree-server/src/api/handlers.rs`
- `crates/worktree-server/src/storage/staged.rs`

- Added SDK staged sync:
  - `push()` now routes to `push_latest_staged()`
  - `push_staged(&engine, snapshot_id)` uploads one local snapshot to `POST /staged`
  - Computes added/modified/deleted paths relative to the parent snapshot
  - Uploads added/modified file bytes as base64 and verifies local BLAKE3 before send
- Added server `POST /staged` endpoint:
  - Validates uploaded paths are relative
  - Decodes base64 content
  - Verifies size and BLAKE3 hash
  - Stores content-addressed objects in server `DiskStorage`
  - Stores snapshot metadata as a server object
  - Records a protocol `StagedSnapshot` in a server-side staged index
- Added `StagedStore`:
  - Persists `StagedIndex` at server storage root under `staged/index.json`
  - Keeps staged visibility state separate from SDK `.wt/state.json`
- Wired bgprocess auto-snapshot path:
  - After `create_snapshot()` succeeds in `watcher_loop_blocking`, it synchronously calls
    `worktree_sdk::engine::sync::push_staged(&engine, &snap.id)`
  - Sync failures are logged without killing the watcher loop

**Go IAM/server rewrite note:** The Rust endpoint is intentionally a compatibility bridge. The
client contract is now `POST /staged`, and IAM remains server-side; bgprocess does not enforce
permissions. The Go server can replace the endpoint implementation without changing the SDK call
shape.

**Also fixed while verifying workspace:**
- Added `crates/worktree-git/build.rs` to link `advapi32` on Windows for libgit2 tests.
- Fixed protocol JSON serialization for `InMemoryHashIndex` by serializing as mappings rather
  than JSON object keys.
- Fixed directory-only ignore matching for `.wt/`, `.git/`, `node_modules/`, etc.

**Result:** `cargo fmt --all` and `cargo test --workspace` pass.

---

## Fixes

### `fix(git): sync InMemoryHashIndex with refactored HashIndex trait`
**File:** `crates/worktree-git/src/hash_index/store.rs`
- Rewrote `HashIndex` impl to match current trait signature
- `insert` now returns `bool` (dropped `Result`/associated error type)
- Renamed `lookup_by_git` → `get_blake3`, `lookup_by_content` → `get_sha1`
- Added missing `remove_by_blake3`, `remove_by_sha1`, `len`
- Fixed `HashMapping` field refs: `git_hash`/`content_hash` → `sha1`/`blake3`
- Updated tests; added coverage for remove operations and idempotent insert

**Also:** Added note to `CLAUDE.md` to keep `worktree-git` local impls in sync with protocol trait changes.
