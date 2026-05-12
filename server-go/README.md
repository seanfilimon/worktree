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
- Optional static bearer-token middleware for protected endpoints.
- Request-scoped tenant/account principal headers.
- Local content-addressed object storage with BLAKE3 verification.
- `POST /staged` staged snapshot upload endpoint.
- `GET /staged` staged snapshot listing endpoint with tenant/worktree/branch filters.
- JSON staged snapshot index for local development.
- File-backed JSONL audit records for staged upload/list allow and deny decisions.

Not implemented yet:

- Postgres metadata storage.
- IAM policy evaluation.
- gRPC sync service.

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

Upload a staged snapshot object:

```bash
curl -X POST http://127.0.0.1:8080/staged \
  -H "Content-Type: application/json" \
  -H "X-WT-Tenant: acme" \
  -H "X-WT-Account: alice" \
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
`.wt-server-go/staged/index.json` by default.

Staged upload and list decisions are appended to `.wt-server-go/audit/audit.jsonl` by default.
Override the path with `WT_SERVER_AUDIT_PATH`.

Set `WT_SERVER_AUTH_TOKEN` to require a static bearer token for protected endpoints:

```bash
WT_SERVER_AUTH_TOKEN=dev-secret go run ./cmd/wt-server
```

Then call protected endpoints with:

```bash
Authorization: Bearer dev-secret
```

List staged snapshots:

```bash
curl "http://127.0.0.1:8080/staged?worktree=api&branch=main" \
  -H "Authorization: Bearer dev-secret" \
  -H "X-WT-Tenant: acme"
```

Inspect local audit records:

```bash
cat .wt-server-go/audit/audit.jsonl
```

## TLS 1.3

For local TLS testing, provide a certificate and key:

```bash
WT_SERVER_TLS_ENABLED=true \
WT_SERVER_TLS_CERT_FILE=cert.pem \
WT_SERVER_TLS_KEY_FILE=key.pem \
go run ./cmd/wt-server
```

The server sets `tls.Config.MinVersion` to TLS 1.3 when TLS is enabled.
