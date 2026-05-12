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

Not implemented yet:

- Postgres metadata storage.
- Content-addressed object storage.
- Auth and tenant resolution.
- IAM policy evaluation.
- Staged snapshot upload.
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

## TLS 1.3

For local TLS testing, provide a certificate and key:

```bash
WT_SERVER_TLS_ENABLED=true \
WT_SERVER_TLS_CERT_FILE=cert.pem \
WT_SERVER_TLS_KEY_FILE=key.pem \
go run ./cmd/wt-server
```

The server sets `tls.Config.MinVersion` to TLS 1.3 when TLS is enabled.
