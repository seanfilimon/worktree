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
- Request ID middleware.
- TLS 1.3 minimum-version configuration.
- Local content-addressed object storage with BLAKE3 verification.
- `POST /staged` staged snapshot upload endpoint.
- JSON staged snapshot index for local development.

Not implemented yet:

- Postgres metadata storage.
- Auth and tenant resolution.
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
```

Upload a staged snapshot object:

```bash
curl -X POST http://127.0.0.1:8080/staged \
  -H "Content-Type: application/json" \
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

## TLS 1.3

For local TLS testing, provide a certificate and key:

```bash
WT_SERVER_TLS_ENABLED=true \
WT_SERVER_TLS_CERT_FILE=cert.pem \
WT_SERVER_TLS_KEY_FILE=key.pem \
go run ./cmd/wt-server
```

The server sets `tls.Config.MinVersion` to TLS 1.3 when TLS is enabled.
