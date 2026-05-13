# W0rkTree Go Server

This directory contains the planned production remote authority for W0rkTree.
It is intentionally separate from the Rust `worktree-server` prototype, which
currently mixes local bgprocess behavior with demo server endpoints.

## Current Status

Implemented:

- Go module skeleton.
- HTTP server with graceful shutdown.
- JSON logging through `log/slog`.
- `GET /health`.
- `GET /ready`.
- `GET /metrics` Prometheus-style request counters.
- Request ID middleware.
- TLS 1.3 minimum-version configuration.
- Production bearer-token middleware for protected endpoints.
- File-backed bearer credential loading via `WT_SERVER_AUTH_CREDENTIALS_PATH`.
- File-backed policy rules via `WT_SERVER_IAM_POLICY_PATH`.
- Static header-derived tenant/account principal support only for explicit `static-dev` mode.
- Local content-addressed object storage with BLAKE3 verification.
- `POST /staged` staged snapshot upload endpoint.
- `GET /staged` staged snapshot listing endpoint with tenant/worktree/branch filters.
- JSON staged snapshot index for local development.
- Postgres staged snapshot store selected by `WT_SERVER_DATABASE_URL`.
- Embedded migrations for staged schema, canonical schema, and audit events run seamlessly on both Windows and Linux environments.
- File-backed JSONL audit records for staged upload/list allow and deny decisions.
- Configurable staged upload object-size and object-count limits.
- IAM `Authorizer` seam with allow-all/deny-all test implementations and a default-deny policy authorizer.
- gRPC `SyncService` with `StageSnapshot` and `ListStagedSnapshots` protected by a bearer auth interceptor.
- Dockerfile and Docker Compose stack with Postgres and migration runner.

Not implemented yet:

- Full declarative `.wt/access/*.toml` and `.wt-tree/access/*.toml` policy parsing.
- JWT/OIDC identity and API-key lifecycle management beyond file/env bearer credentials.
- Branch push/pull and canonical branch history.
- WebSocket staged event fanout.

## Local Run

Requires Go 1.23 or newer.

```bash
cd server-go
go run ./cmd/wt-server
```

Then test:

```bash
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/ready
curl http://127.0.0.1:8080/metrics
```

The gRPC server listens on `WT_SERVER_GRPC_ADDR`, default `127.0.0.1:9877`.

## Docker Compose

```bash
cd server-go
docker compose up --build
```

Compose starts Postgres, runs migrations, and starts the server with:

- HTTP: `0.0.0.0:8080`
- gRPC: `0.0.0.0:9877`
- Postgres staged store through `WT_SERVER_DATABASE_URL`

Upload a staged snapshot object:

```bash
curl -X POST http://127.0.0.1:8080/staged \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer dev-secret" \
  -d '{
    "snapshot_id": "snap-1",
    "tenant": "acme",
    "worktree": "api",
    "tree_id": "tree-1",
    "branch": "main",
    "objects": [
      {
        "path": "README.md",
        "hash": "<blake3-hex-of-content>",
        "size": 12,
        "content": "<base64-content>"
      }
    ]
  }'
```

JSON `content` fields are decoded as base64 by Go. The server verifies `size` and BLAKE3 `hash`
before persisting the object. Staged metadata is written under
`.wt-server-go/staged/index.json` by default, or to Postgres when `WT_SERVER_DATABASE_URL` is set.
Duplicate staged uploads are idempotent only when the canonical staged identity and object refs match. Conflicting retries return a structured conflict instead of being silently ignored.

Staged upload and list decisions are appended to `.wt-server-go/audit/audit.jsonl` by default.
Override the path with `WT_SERVER_AUDIT_PATH`.

Staged uploads are bounded before object persistence. Defaults:

- `WT_SERVER_MAX_STAGED_OBJECT_BYTES=67108864`
- `WT_SERVER_MAX_STAGED_OBJECTS=1024`

Set either value to `0` to disable that specific development limit.

Protected endpoints use bearer-token auth by default. The simplest demo uses one env token:

```bash
WT_SERVER_AUTH_MODE=bearer \
WT_SERVER_AUTH_TOKEN=dev-secret \
WT_SERVER_AUTH_TENANT=acme \
WT_SERVER_AUTH_ACCOUNT=alice \
WT_SERVER_AUTH_SCOPES=staged:* \
go run ./cmd/wt-server
```

For multiple demo principals, use the checked-in example files:

```bash
WT_SERVER_AUTH_MODE=bearer \
WT_SERVER_AUTH_CREDENTIALS_PATH=examples/tokens.demo.json \
WT_SERVER_IAM_MODE=policy \
WT_SERVER_IAM_POLICY_PATH=examples/policy.demo.json \
go run ./cmd/wt-server
```

Then call protected endpoints with:

```bash
Authorization: Bearer dev-secret
```

Use `Authorization: Bearer viewer-secret` to verify that the viewer principal can list staged
snapshots but cannot create them.

List staged snapshots:

```bash
curl "http://127.0.0.1:8080/staged?worktree=api&branch=main" \
  -H "Authorization: Bearer dev-secret"
```

Inspect local audit records:

```bash
cat .wt-server-go/audit/audit.jsonl
```

## Rust Client Compatibility

Rust clients use:

- `WT_SERVER_URL` to choose the HTTP server, default `http://127.0.0.1:8080`.
- `WT_TENANT` to choose the staged snapshot tenant, default `default`.

The JSON upload field is `content`; the previous `content_base64` name is no longer part of the
compatibility contract.

## TLS 1.3

For local TLS testing, provide a certificate and key:

```bash
WT_SERVER_TLS_ENABLED=true \
WT_SERVER_TLS_CERT_FILE=cert.pem \
WT_SERVER_TLS_KEY_FILE=key.pem \
go run ./cmd/wt-server
```

The server sets `tls.Config.MinVersion` to TLS 1.3 when TLS is enabled.
