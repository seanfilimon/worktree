# `worktree-protocol`

Shared protocol and domain types for W0rkTree. Every Rust crate in the
workspace (`worktree-sdk`, `worktree-cli`, `worktree-bg`, `worktree-git`)
depends on it, and its `proto/sync.proto` is the Rust⇄Go contract with the
Go server in `services/server-go`.

It is a pure library: no I/O apart from `core::hash::hash_file`, no async
runtime, and no binaries.

## What's in it

| Module | Contents |
|---|---|
| `proto::sync` | tonic/prost code generated from `proto/sync.proto` (`worktree.sync.v1`): messages, `SyncServiceClient`, `SyncServiceServer` |
| `proto::conversions` | Conversions between the domain and wire types, plus validation helpers (`ConversionError`, `validate_manifest_path`) |
| `core` | `ContentHash` (BLAKE3-256), typed UUID ids (`TreeId`, `SnapshotId`, …), `ProtocolError` |
| `object` | Blobs, manifests, snapshots, branches, tags, deltas, staged snapshots, merge requests, releases, reflog, dependencies |
| `iam` | Accounts, tenants, teams, roles (RBAC), permissions, scopes, policies (ABAC), sessions, and the `AccessEngine` |
| `access` | Per-tree and per-branch ACLs (deny overrides allow), branch protection |
| `config` | The `.wt/config.toml` and `.wt-tree/config.toml` models and the ceiling-model resolver |
| `feature::ignore` | Ignore engine with `.gitignore` semantics |
| `feature::{diff, merge}` | Manifest diff (renames and copies), patch apply, merge result and conflict types |
| `feature::wire` | Framed bincode envelope (magic, version, flags, length) |
| `feature::compat` | Git SHA-1 ⇄ BLAKE3 maps and ref maps for `worktree-git` |
| `feature::{licensing, large_file, audit, archive, sync_protocol}` | License path rules and grants, chunking config, audit events, archive options, legacy sync message types |

## Wire protocol (`proto/sync.proto`)

- The object hash is the BLAKE3-256 of the exact object bytes. `ContentHash.value` is the 32 raw bytes.
- A BLOB object is the raw file bytes. A MANIFEST object is a protobuf-encoded `ManifestRecord`. Its entries are sorted by path string (byte-wise), unique, `/`-separated and relative.
- Snapshots travel as `SnapshotRecord`, not as objects. Their ids are UUID v4 strings.

The helpers in `proto::conversions` follow these rules:

```rust
use worktree_protocol::proto::sync::{ManifestRecord, ObjectData};

let blob = ObjectData::blob(bytes);                      // computes the hash
let manifest = ManifestRecord::from_entries(entries);    // sorts and dedups
let obj = manifest.to_object();                          // MANIFEST ObjectData
assert_eq!(obj.verify()?, manifest.content_hash());
let decoded = obj.decode_manifest()?;                    // checks kind, hash, paths, order
```

Receivers must call `validate()` (or `decode_manifest` / `into_manifest`)
before writing files. It rejects absolute paths, `..`, backslashes and drive
prefixes.

### Code generation

- **Rust:** `build.rs` runs `tonic-build` at compile time using the `protoc`
  from `protoc-bin-vendored`, so nothing needs installing. Set `PROTOC` to
  use a different binary.
- **Go:** `bash scripts/gen-proto.sh` regenerates
  `services/server-go/internal/proto/syncpb/`. It uses the same vendored
  `protoc`, so run `cargo build -p worktree-protocol` once first.

`sync.proto` is a frozen contract. Changes must be additive, using new field
numbers.

## Ignore rules

`feature::ignore::IgnoreEngine` implements gitignore semantics: negation,
anchoring, `*`, `?`, `[...]`, `**`, directory-only patterns, and "a file
inside an excluded directory cannot be re-included".

There are three layers:
- **Hard built-ins:** `.wt/`, `.wt-tree/`, `.git/`. These can never be re-included.
- **Root patterns:** last match wins.
- **Tree patterns:** these can only add exclusions.

`is_ignored(path)` treats a path as a directory only if it ends in `/`. Use
`is_ignored_path(path, is_dir)` or `is_ignored_dir` when walking a tree.

## Access engine

`iam::engine::AccessEngine::evaluate` is a pure function:
- An explicit Deny policy always wins, whatever its priority.
- Anything not matched is denied.
- `TenantAdmin` and `TreeAdmin` imply the permissions below them.
- Policies apply only inside their own tenant.
- Account attributes take precedence over request attributes.
- Only `GlobalAdmin` crosses tenants. The built-in `Owner` role does not hold it.

## Development

```bash
cargo test   -p worktree-protocol
cargo clippy -p worktree-protocol --all-targets -- -D warnings
cargo fmt    -p worktree-protocol
```

`Implementation.md` covers the per-module details.
