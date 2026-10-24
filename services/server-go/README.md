# worktree-server (Go)

The W0rkTree **remote server**: canonical history, multi-tenant IAM, branch
protection and merge requests, license enforcement, staged (team-visible WIP)
snapshots, an audit log, and the admin panel — all on **one port**.

```
 wt / worktree-bg (Rust) ──gRPC h2c──┐
                                     ├──► :7420  worktree-server ──► bbolt meta.db + objects/
 apps/admin (browser) ──REST + SSE───┘
```

* **gRPC** (`worktree.sync.v1.SyncService`, plus `grpc.health.v1.Health`) over
  HTTP/2 cleartext with prior knowledge (h2c). Requests with HTTP/2 and a
  `Content-Type: application/grpc*` header are routed to gRPC; everything else
  goes to the HTTP API. Max message size is 64 MiB in both directions.
* **REST** JSON API under `/api/v1`, a **Server-Sent Events** stream at
  `/api/v1/events`, `/healthz`, Prometheus `/metrics`, and the static admin
  SPA at `/admin/`.

This is not the local daemon — that is `crates/worktree-bg` (Rust).

## Build and run

Requires Go 1.25+ (pure Go, no cgo).

```bash
cd services/server-go
go build -o worktree-server -ldflags "-X main.version=$(git describe --tags --always)" .
WT_ADMIN_PASSWORD=change-me ./worktree-server --data ./wt-data
```

On startup the server prints

```
worktree-server version 0.2.0-dev
worktree-server listening on [::]:7420
```

to stdout (scripts wait for the `worktree-server listening on` line) and
writes structured logs to stderr. `SIGINT`/`SIGTERM` trigger a graceful
shutdown: health checks switch to `NOT_SERVING`, HTTP/2 connections get
`GOAWAY`, in-flight requests and RPCs finish within `--shutdown-timeout`, then
remaining connections are closed.

To serve the admin panel, build it and point the server at the output:

```bash
(cd ../../apps/admin && npm install && npm run build)
./worktree-server --admin-dir ../../apps/admin/dist
```

During panel development run `npm run dev` in `apps/admin` instead; the Vite
dev server (`http://localhost:5173`) is always allowed by CORS.

Exit codes: `0` clean shutdown (also `--version`, `-h`), `1` runtime error,
`2` invalid flags or configuration.

## Configuration

Precedence: **flag > environment variable > default**.

| Flag | Env | Default | Description |
|---|---|---|---|
| `--addr` | `WT_ADDR` | `:7420` | Listen address (gRPC + HTTP). |
| `--data` | `WT_DATA` | `./wt-data` | Data directory: `meta.db` (bbolt) and `objects/` (content-addressed BLAKE3 store). Created if missing. |
| `--admin-user` | `WT_ADMIN_USER` | `admin` | Server admin username. |
| `--admin-password` | `WT_ADMIN_PASSWORD` | — | If set, the admin account is created (or promoted to server admin) at startup. Prefer the env var: flags are visible in process listings. |
| `--admin-dir` | `WT_ADMIN_DIR` | — | Static admin build served at `/admin/` (index.html fallback). Without it `/admin/` explains how to build the panel. |
| `--allow-registration` | `WT_ALLOW_REGISTRATION` | `true` | Allow self-service `POST /api/v1/auth/register`. |
| `--log-level` | `WT_LOG_LEVEL` | `info` | `debug`, `info`, `warn`, `error`. |
| `--log-format` | `WT_LOG_FORMAT` | `text` | `text` or `json`. |
| `--cors-origins` | `WT_CORS_ORIGINS` | — | Comma-separated extra browser origins (`scheme://host[:port]`, or `*`). `http://localhost:5173` is always allowed. |
| `--shutdown-timeout` | `WT_SHUTDOWN_TIMEOUT` | `15s` | Graceful shutdown budget. |
| `--version` | — | — | Print the version and exit. |

## REST API

All endpoints are JSON; field names are the `json` tags in
[`internal/domain/types.go`](internal/domain/types.go). Authenticate with
`Authorization: Bearer <token>` (from login/register or an API token).

Errors use one envelope:

```json
{"error": {"code": "not_found", "message": "tree acme/web not found"}}
```

| Status | Code |
|---|---|
| 400 | `bad_request` (validation, malformed JSON, unknown fields) |
| 401 | `unauthenticated` |
| 403 | `forbidden`, or a policy code: `branch_protection`, `access_denied`, `license_violation`, `review_required`, `quota_exceeded` |
| 404 | `not_found` |
| 405 | `method_not_allowed` |
| 409 | `conflict` (already exists, concurrent update, last owner, ...) |
| 413 | `payload_too_large` (JSON bodies are limited to 1 MiB) |
| 415 | `unsupported_media_type` (body must be `application/json`) |
| 500 | `internal` (details are logged, never returned) |

Every response carries an `X-Request-Id` (an incoming well-formed one is
propagated) that also appears in the logs.

```
GET    /healthz                                    {status, version, uptime_seconds}
GET    /metrics                                    Prometheus text format

POST   /api/v1/auth/register        {username,email,password}   201 {token, account}
POST   /api/v1/auth/login           {username,password}         {token, account}
POST   /api/v1/auth/logout                                      204 (revokes the calling token)
GET    /api/v1/auth/me                                          {account, memberships:[{tenant, role}]}
GET    /api/v1/auth/tokens                                      [api_token]
POST   /api/v1/auth/tokens          {name, ttl_hours}           {token, api_token}   (ttl_hours 0 = no expiry)
DELETE /api/v1/auth/tokens/{id}                                 204

GET    /api/v1/tenants                                          [tenant] (yours; all for server admins)
POST   /api/v1/tenants              {slug,name}                 201 tenant (org, caller = owner)
GET    /api/v1/tenants/{tenant}                                 tenant           ({tenant} = slug or id)
GET    /api/v1/tenants/{tenant}/members                         [{account, role}]
PUT    /api/v1/tenants/{tenant}/members/{username} {role}       {account, role}
DELETE /api/v1/tenants/{tenant}/members/{username}              204 (members may remove themselves)
GET    /api/v1/tenants/{tenant}/trees                           [treeSummary] (readable trees only)
POST   /api/v1/tenants/{tenant}/trees {name,description,visibility,default_branch}  201 tree

GET    /api/v1/trees/{tenant}/{tree}                            {tree, branch_count, snapshot_count, staged_count, open_mr_count}
PATCH  /api/v1/trees/{tenant}/{tree} {description?,visibility?,default_branch?}  tree
DELETE /api/v1/trees/{tenant}/{tree}                            204
GET    /api/v1/trees/{tenant}/{tree}/branches                   [branch]
GET    /api/v1/trees/{tenant}/{tree}/snapshots?branch=&limit=50 [snapshot] newest first (default branch by default)
GET    /api/v1/trees/{tenant}/{tree}/snapshots/{id}             {snapshot, files:[{path,hash,size,executable}]}
GET    /api/v1/trees/{tenant}/{tree}/blob/{snapshot}/{path...}  raw bytes (Range, ETag; ?download for attachment)
GET    /api/v1/trees/{tenant}/{tree}/tags                       [tag]
GET    /api/v1/trees/{tenant}/{tree}/staged?branch=             [staged]
GET    /api/v1/trees/{tenant}/{tree}/protections                [protection]
PUT    /api/v1/trees/{tenant}/{tree}/protections  (BranchProtection)  protection
DELETE /api/v1/trees/{tenant}/{tree}/protections?pattern=       204
GET    /api/v1/trees/{tenant}/{tree}/licenses                   [license rule]
PUT    /api/v1/trees/{tenant}/{tree}/licenses     (LicenseRule) license rule
DELETE /api/v1/trees/{tenant}/{tree}/licenses?path_glob=        204
GET    /api/v1/trees/{tenant}/{tree}/merge-requests?state=      [mr]
POST   /api/v1/trees/{tenant}/{tree}/merge-requests {title,description,source_branch,target_branch}  201 mr
GET    /api/v1/trees/{tenant}/{tree}/merge-requests/{n}         mr
POST   /api/v1/trees/{tenant}/{tree}/merge-requests/{n}/approve|merge|close  mr

GET    /api/v1/audit?tenant=&action=&limit=&since=              [audit event] (tenant admin, or server admin without tenant)
GET    /api/v1/events?tenant=&tree=&types=                      text/event-stream
GET    /api/v1/admin/stats                                      ServerStats (server admin)
GET    /api/v1/admin/accounts                                   [account] (server admin)
PATCH  /api/v1/admin/accounts/{username} {is_admin?,disabled?}  account (server admin)
GET    /admin/*                                                 admin SPA
```

Notes:

* Authorization is decided by the IAM service (`tree:read`, `tree:admin`,
  `tenant:admin`, ...). Public trees are readable anonymously; internal trees
  by any logged-in account.
* Blobs are served with `X-Content-Type-Options: nosniff` and a sandboxing
  CSP; HTML, SVG, XML and JavaScript files are served as `text/plain` so tree
  content can never run in the server's origin. License rules are enforced on
  read (`403 license_violation`).
* Only owners (or server admins) can grant or revoke the `owner` role, and a
  tenant always keeps at least one owner.

### Server-Sent Events

`GET /api/v1/events` streams `domain.Event`s:

```
: connected
retry: 5000

event: branch.pushed
data: {"type":"branch.pushed","tenant_id":"...","tree_id":"...","actor":"alice","payload":{...},"at":"..."}

: heartbeat
```

Filters: `tree` (requires `tenant`; needs `tree:read`), `tenant` (needs
`tenant:read`), neither (server admins only). `types` is an optional
comma-separated list of event types. A heartbeat comment is sent every 15 s;
the stream ends when the client disconnects or the server shuts down.
The browser `EventSource` API cannot set headers, so browser clients need a
`fetch`-based stream reader to send the `Authorization` header.

### Metrics

`/metrics` (unauthenticated, Prometheus text format 0.0.4):

* `worktree_http_requests_total{route,code}` — `route` is the matched
  pattern, e.g. `GET /api/v1/trees/{tenant}/{tree}`
* `worktree_http_request_duration_seconds{route}` — histogram
* `worktree_http_requests_in_flight`
* `worktree_tenants`, `worktree_accounts`, `worktree_trees`,
  `worktree_branches`, `worktree_snapshots`, `worktree_staged_snapshots`,
  `worktree_objects`, `worktree_storage_bytes` — refreshed at most every 10 s
* `worktree_build_info{version,goversion}`, `worktree_uptime_seconds`,
  `go_goroutines`, `go_memstats_heap_alloc_bytes`, `go_memstats_sys_bytes`

## Layout

| Package | Role |
|---|---|
| `main.go` | Flag parsing, signals, exit codes. |
| `internal/config` | `Load(args)` — flags + `WT_*` env, validation. |
| `internal/server` | `Run(ctx, cfg)` — wires storage and services, single h2c listener, graceful shutdown. |
| `internal/httpapi` | REST, SSE, metrics, admin SPA, middleware (request id, logging, recovery, CORS). |
| `internal/syncsvc` | gRPC `SyncService` implementation. |
| `internal/domain` | Shared types and service interfaces (the contract between packages). |
| `internal/meta`, `internal/objstore` | bbolt metadata store, content-addressed object store. |
| `internal/iam`, `internal/review`, `internal/license`, `internal/staged`, `internal/events`, `internal/audit` | Domain services. |

## Development

```bash
go build ./... && go vet ./... && go test ./...
WT_INTEGRATION=1 go test ./internal/server/ -run Integration   # boots the full server
```

The integration test starts the real server on `127.0.0.1:0` with a temporary
data directory, checks `/healthz`, admin login, gRPC `Ping` over h2c and
graceful shutdown.

### Regenerating protobuf code

The schema lives at `../../crates/worktree-protocol/proto/sync.proto`; the Go
bindings in `internal/proto/syncpb/` are checked in. Regenerate with:

```bash
bash ../../scripts/gen-proto.sh
```
