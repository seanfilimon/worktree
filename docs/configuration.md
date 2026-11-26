# Configuration

W0rkTree reads configuration from three places:

| Scope | Location | Owner |
|---|---|---|
| Worktree | [`.wt/config.toml`](#wtconfigtoml) | `worktree_sdk::config::WorktreeConfig` |
| User | [`$WT_HOME/`](#per-user-state-wt_home) (`credentials.toml`, `bg.toml`, `bg.json`, `bg.log`) | SDK remote client, daemon |
| Server | [flags / environment](#server-flags-and-environment) | `services/server-go/internal/config` |

---

## `.wt/config.toml`

Created by `wt init` and `wt clone`. Edit it with [`wt config`](./cli-reference.md#wt-config) or by
hand. All sections except `[worktree]` are optional and fall back to the defaults below.

```toml
[worktree]
name = "demo"
id = "0b6f6d8e-1f7a-4c39-9a51-3c2d7c1e4b20"
created_at = "2026-10-24T12:00:00Z"
default_branch = "main"

[user]
name = "Ada Lovelace"
email = "ada@example.com"

[auto_snapshot]
enabled = true
inactivity_secs = 30
max_changed_files = 50

[sync]
auto_stage = true
remote = "origin"

[remote.origin]
url = "http://localhost:7420"
tenant = "acme"
```

### Keys

Dotted keys are what `wt config get|set` accept.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `worktree.name` | string | directory name | Worktree name; also the root tree's name. |
| `worktree.id` | string (UUID) | generated | Stable worktree id. |
| `worktree.created_at` | RFC 3339 string | init time | Creation timestamp. |
| `worktree.default_branch` | string | `main` | Default branch of new trees. |
| `user.name` | string | `""` | Author name for snapshots. |
| `user.email` | string | `""` | Author email for snapshots. |
| `auto_snapshot.enabled` | bool | `true` | Whether the daemon auto-snapshots this worktree. |
| `auto_snapshot.inactivity_secs` | integer | `30` | Snapshot after this many seconds without file events. |
| `auto_snapshot.max_changed_files` | integer | `50` | Snapshot immediately once this many files have changed. |
| `sync.auto_stage` | bool | `true` | After an auto-snapshot, upload the working state as a staged snapshot. |
| `sync.remote` | string | `origin` | Remote used by the daemon. |
| `remote.<name>.url` | string | — | Server base URL, e.g. `http://localhost:7420`. gRPC and REST share it. |
| `remote.<name>.tenant` | string | — | Tenant slug that owns this worktree's trees on the server. |
| `storage.chunk_threshold` | size | `8MiB` (8388608) | New or modified files larger than this are stored as content-defined chunks. 1 MiB to 32 MiB; see [storage](#storage). |

**Snapshot author.** New snapshots use `user.name <user.email>` when set, else the username logged in
to the default remote's server, else the operating-system user.

Remotes are normally managed with `wt remote add|remove`.

### Storage

```toml
[storage]
chunk_threshold = "16MiB"
```

`storage.chunk_threshold` is the large-file threshold: a new or modified file larger than this many
bytes is stored as content-defined chunks instead of one blob (see
[storage format](./storage-format.md#large-files)). Sizes are a number of bytes or a number with a
unit (`KB`/`MB`/`GB` = powers of 1000, `KiB`/`MiB`/`GiB` = powers of 1024). Values outside
**1 MiB to 32 MiB** are rejected — by `wt config set` with an error naming the key and the range,
and in a hand-edited file when the worktree is opened (`wt doctor` reports it). The upper bound
keeps every single-blob file well below the 64 MiB per-object limit of the sync protocol. The
key exists only in `.wt/config.toml`: `--global` and `--tree` of a nested tree refuse it.

```bash
wt config set storage.chunk_threshold 16MiB    # stored as 16777216
wt config get storage.chunk_threshold          # 16777216
wt config unset storage.chunk_threshold        # back to 8 MiB
```

Changing the threshold never rewrites history and never makes an unchanged file look modified:
a file whose content equals its last snapshotted version keeps the form it was recorded in
(chunked or one blob), so `wt status` and `wt diff` stay clean. Only files you add or change
afterwards use the new value. Chunk sizes (256 KiB minimum, ~1 MiB average, 4 MiB maximum) are
fixed: they are part of the storage format, and changing them would re-chunk every modified large
file and stop deduplication against earlier versions.

Keep the setting the same in every clone of a tree: the same content stored under different
thresholds has different entry hashes, so two clones adding the same file would see different
versions of it. Lowering the threshold below 8 MiB makes more files chunked; the server's
merge-request merge reports a both-sides change to a chunked file as a conflict instead of merging
it as text, and server code search skips chunked files.

### Per-tree configuration

Each tree has metadata under `.wt/trees/<tree>/tree.toml` (name, id, path, default branch and the
tree's name on the server). Nested trees are marked by a `<path>/.wt-tree/` directory containing
`config.toml`, written by `wt tree add`. See [storage format](./storage-format.md).

### Ignore files

| Source | Notes |
|---|---|
| Built-ins | `.wt/`, `.wt-tree/`, `.git/`, `target/`, `node_modules/`, `.DS_Store`, `*.swp` |
| `.wt/ignore` | One gitignore-syntax pattern per line; `wt ignore add` appends here. |
| `.wtignore` | gitignore-syntax files anywhere in the worktree. |

`wt git import` converts `.gitignore` into `.wt/ignore`.

---

## Per-user state (`$WT_HOME`)

`$WT_HOME` defaults to `~/.wt` (`%USERPROFILE%\.wt` on Windows). Set it to isolate test runs —
**tests must always set `WT_HOME` to a temporary directory.**

| File | Written by | Contents |
|---|---|---|
| `credentials.toml` | `wt login`, `wt register`, `wt logout` | Bearer tokens per server URL. |
| `bg.toml` | `wt daemon watch` / `unwatch` | Worktrees the daemon watches. |
| `bg.json` | `worktree-bg` on startup (removed on clean shutdown) | `{"pid": u32, "port": u16, "version": "..."}` — how `wt` finds the daemon. |
| `bg.log` | `worktree-bg` | Daemon log. |

### `credentials.toml`

```toml
[servers."http://localhost:7420"]
username = "ada"
token = "wt_..."
```

Keys are normalized server URLs (scheme and host lower-cased, no trailing slash), so
`HTTP://LocalHost:7420/` and `http://localhost:7420` are the same server. The file holds bearer
tokens; on Unix it is created with mode `0600`. Remove an entry with `wt logout`.

### `bg.toml`

```toml
watched = [
  "/home/ada/src/demo",
  "C:\\Users\\ada\\src\\platform",
]
```

### Environment variables

| Variable | Used by | Effect |
|---|---|---|
| `WT_HOME` | SDK, CLI, daemon | Per-user state directory. |
| `WT_PASSWORD` | `wt login`, `wt register` | Password instead of an interactive prompt. |

---

## Server flags and environment

The Go server (`services/server-go`) takes each setting as a flag or an environment variable; flags
win.

| Flag | Environment | Default | Meaning |
|---|---|---|---|
| `--addr` | `WT_ADDR` | `:7420` | Listen address for gRPC, REST, SSE, metrics and the admin panel. |
| `--data` | `WT_DATA` | `./wt-data` | Data directory; holds `meta.db` (bbolt) and `objects/`. |
| `--admin-user` | `WT_ADMIN_USER` | `admin` | Username of the server admin account. |
| `--admin-password` | `WT_ADMIN_PASSWORD` | unset | If set, ensures the admin account exists with this password and server-admin rights. |
| `--admin-dir` | `WT_ADMIN_DIR` | unset | Directory with the built admin panel (`apps/admin/dist`), served at `/admin/`. |
| `--allow-registration` | `WT_ALLOW_REGISTRATION` | `true` | Allow self-service `POST /api/v1/auth/register`. |
| `--log-level` | `WT_LOG_LEVEL` | `info` | Minimum log level: `debug`, `info`, `warn`, `error`. |
| `--log-format` | `WT_LOG_FORMAT` | `text` | Structured log format: `text` or `json`. |
| `--cors-origins` | `WT_CORS_ORIGINS` | — | Extra browser origins allowed to call the REST API (comma-separated; `*` allows any). `http://localhost:5173` (Vite dev) is always allowed. |
| `--shutdown-timeout` | `WT_SHUTDOWN_TIMEOUT` | `15s` | Upper bound for graceful shutdown. |
| `--version` | — | — | Print the version and exit. |

Prefer `WT_ADMIN_PASSWORD` over `--admin-password` so the password does not appear in process
listings.

```bash
worktree-server --addr :7420 --data /var/lib/worktree \
  --admin-password "$ADMIN_PASSWORD" --admin-dir /opt/worktree/admin \
  --allow-registration=false
```

```powershell
$env:WT_DATA = "D:\wt-data"; $env:WT_ADMIN_PASSWORD = "change-me"
.\worktree-server.exe --admin-dir apps\admin\dist
```

See [server architecture](./server-architecture.md) for what lives in the data directory.

### Admin panel (development)

The Vite dev server proxies `/api`, `/healthz` and `/metrics` to `http://localhost:7420`; set
`WT_SERVER_URL` to point it elsewhere. See [admin panel](./admin-panel.md).
