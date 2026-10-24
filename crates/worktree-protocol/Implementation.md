# worktree-protocol: implementation notes

## Crate facts

- Edition 2021. A library only.
- Dependencies: `serde`, `blake3`, `bincode` 1, `chrono`, `thiserror`, `uuid`, `prost`/`prost-types` 0.13 and `tonic` 0.12.
- Build dependencies: `tonic-build` 0.12 and `protoc-bin-vendored` 3.
- `build.rs` sets `PROTOC` to the vendored binary unless it is already set, then compiles `proto/sync.proto` into `$OUT_DIR`. The generated code is not checked in. `src/proto/mod.rs` includes it as `proto::sync`, with a targeted `allow(clippy::result_large_err)` because tonic returns `Status` errors.
- Go bindings come from the same proto: run `bash scripts/gen-proto.sh`, which writes `services/server-go/internal/proto/syncpb`.

## Layout

```
src/
  lib.rs            module tree + re-exports of feature::* (diff, merge, wire, ...)
  proto/            generated wire types (sync) + conversions.rs
  core/             hash.rs (ContentHash), id.rs (UUID ids), error.rs
  object/           blob, manifest, snapshot, branch, tag, delta, staged,
                    merge_request, release, reflog, dependency, tree
  iam/              account, tenant, team, role, permission, scope, policy,
                    session, engine
  access/           resource, tree_access, branch_access
  config/           worktree_config, tree_config, hierarchy
  feature/          ignore, diff/, merge/, wire/, compat/, licensing,
                    large_file, audit, archive, sync_protocol
```

## Key behaviours (the contract other crates rely on)

### `core::hash`

- `ContentHash` is the 32-byte BLAKE3 hash.
- Serde writes a hex string in human-readable formats and raw bytes in binary formats.
- `FromStr` and `from_hex` accept exactly 64 ASCII hex digits, in either case. They reject sign characters and never panic on multi-byte input.
- Added helpers: `from_slice`, `is_zero`, `short_hex` and `AsRef<[u8]>`.
- `hash_file` streams the file through `hash_reader`, so large files are not loaded into memory.

### `core::id`

UUID newtypes. `Display` and `FromStr` use the hyphenated UUID form.

### `proto::conversions`

- Domain to wire uses `From`. Wire to domain uses `TryFrom<…, Error = ConversionError>`, which is `#[non_exhaustive]`.
- Legacy messages (Have/Want, Push/Pull, Stage, AccessConfig, SyncState, SyncEnvelope) convert to and from `feature::sync_protocol`. The `snapshots` field of `PushRequest` and `PullResponse` has no domain counterpart: it is left empty going to the wire and dropped coming back.
- Helpers for content-addressed objects and records:
  - **`ContentHash`:** `From<&ContentHash>`, `TryFrom<&wire::ContentHash>`, `to_domain`, `to_hex`.
  - **`ObjectData`:**
    - constructors `new(kind, data)` and `blob(data)`, both of which compute the hash
    - `object_kind` (rejects UNSPECIFIED)
    - `verify`, which checks the hash against the data
    - `decode_manifest`
    - `From<Blob>` and `TryFrom<ObjectData> for Blob`, which checks the kind and the hash
  - **`ManifestRecord` / `ManifestEntry`:**
    - `TryFrom<&Manifest>`. Directories are dropped and symlinks rejected. Entries are sorted by path *string*, which is the wire order and differs from `Path` ordering.
    - `from_entries` (canonicalising), `validate`, `content_hash`, `encode_object_bytes`, `decode_object_bytes`, `to_object` and `into_manifest(tree_id)`
    - `validate_manifest_path` is the path-traversal guard. It rejects empty, absolute, `.`/`..` and empty segments, backslashes, NUL and drive prefixes.
  - **`SnapshotRecord`:** `from_snapshot(&Snapshot, branch)`, `validate`, `snapshot_id`, `parent_ids`, `to_snapshot_with_author`, and `TryFrom` (only when the author string is an account id).
  - **`TreeInfo`:** `tree_id_domain` and `tenant_id_domain`.
  - **`BranchInfo`:** `from_branch`, `tip_id` and `updated_at_utc`.
  - **`StagedInfo`:** `from_staged`, `to_staged`, `From<StagedSnapshot>`.
  - **`TagRecord`:** `from_tag`, `to_tag(tree_id)`, `From<Tag>`.
- Timestamps must have `0 <= nanos < 1e9`.

### `feature::ignore`

Gitignore semantics:
- Comments, `\#` and `\!` escapes, and trailing-space trimming unless the space is escaped.
- `!` negation.
- A trailing `/` makes a pattern directory-only.
- A leading or middle `/` anchors the pattern. Without one, it matches the basename at any depth.
- `*`, `?` and `[...]` (with `!`/`^` negation, ranges and `[:class:]`). None of them crosses `/`.
- `**` as a whole segment matches across directories: leading, middle, and trailing (trailing means "everything inside").

How the engine evaluates a path:
- It checks every parent directory first. If a parent is excluded, the path is excluded and nothing can re-include it.
- Then it checks the path itself.

The layers:
- **Built-ins:** `.wt/`, `.wt-tree/`, `.git/`. These are hard and cannot be negated.
- **Root patterns:** last match wins.
- **Tree patterns:** a tree negation can only undo a tree-level exclusion.

Other notes:
- `default_ignores()` holds soft defaults such as `node_modules/`, `target/` and `*.swp`.
- Use `is_ignored_path(path, is_dir)` or `is_ignored_dir` for directories. `is_ignored(path)` treats a trailing `/` as marking a directory.
- `matching_pattern` explains which pattern decided the result, and `patterns()` lists every pattern.

### `iam::engine::AccessEngine::evaluate`

1. Deny if `request.account_id` differs from `account.id`, or if the account is inactive.
2. **RBAC.** A team role grants a permission if any permission it holds `implies` the requested one, and the role's tenant covers the scope. `TenantAdmin` implies everything except `GlobalAdmin`; `TreeAdmin` implies the tree operations. `GlobalAdmin` allows everywhere. `Role::owner` deliberately does *not* hold it.
3. **ABAC.** Only enabled policies of the *request's tenant* are considered. Each one must also match on scope, subject, permission and conditions. Conditions see the request attributes overlaid with the account attributes, and the account wins.
4. Any matching Deny wins, whatever its priority. Otherwise the result is Allow if RBAC or an Allow policy matched, and Deny if not.

`Session::refresh` and `touch` refuse sessions that have expired by status or by time.

### `access`

- Tree and branch ACLs: deny overrides allow, and the default is deny.
- `BranchProtection::can_push` and `can_merge` respect the `Public` and `AllAuthenticated` wildcards. An empty list means unrestricted.

### `feature::licensing`

- Path rules are directory prefixes, with an optional trailing `*`/`**`.
- They match on segment boundaries, so `src` does not match `src2`.
- `*` or `**` alone matches everything.
- The longest prefix wins.
- Grants use the same matcher.

### `feature::diff` / `object::delta`

- `compute_diff` reports adds, modifies, deletes, renames (by hash) and optional copies.
- Each delta records the resulting entry kind and executable flag (`Delta::new_kind`/`new_executable`), so `Patch::apply(old, compute_diff(old, new)) == new`.
- Deltas without that metadata keep the existing mode.

### `feature::wire`

- The header is 13 bytes: magic, version, flags and payload length, followed by the bincode payload.
- `encode*` rejects payloads over `u32::MAX`.
- The length-prefixed peek only reads the header from inside the frame.
- The flags are recorded but not acted on yet (no compression or checksum).

### `feature::compat`

- `GitHash` serialises as hex in human-readable formats, which also accepts the legacy byte array, and as raw 20 bytes in bincode (unchanged).
- The bidirectional indexes (`InMemoryHashIndex`, `ObjectMappingIndex`, `RefMappingIndex`) keep both directions consistent when a mapping is replaced or removed.

## Known gaps

- `Manifest::add_entry` does not deduplicate paths, and `compute_hash` uses `to_string_lossy` for paths.
- Merge conflict types cannot resolve a conflict by accepting the deletion. `MergeResult.outcome` does not change after the conflicts are resolved.
- Deny-policy conditions that read a missing attribute evaluate to false (fail open), which is the documented behaviour. Write deny conditions with that in mind.
- A tree-level license default can loosen the root default, and license `paths`/`grants` are not carried into `ResolvedConfig`.
- Nothing stops a tenant admin from creating a custom role that holds `GlobalAdmin`. Role management APIs must refuse it.
