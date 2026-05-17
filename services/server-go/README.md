# worktree-server (Go)

**Remote multi-tenant server** for the W0rkTree VCS. Runs in the cloud
(or on-prem), holds canonical history, enforces IAM, handles chunk
routing, and aggregates staged snapshots from `worktree-bg` clients.

This is **not** the local daemon — that's `crates/worktree-bg/` (Rust).
The current `crates/worktree-server/` Rust crate is transitional and
slated for removal once this Go implementation reaches feature parity
(per the 2026-05-15 language-pivot decision: Rust for the local bg,
Go for the remote server).

## Status

Skeleton only — module declaration + stub `main.go` that prints
`worktree-server: not yet implemented (skeleton — WT-SRV-1)` and exits.
Real implementation work is queued:

- **WT-PROTO-1** — Adopt protobuf wire format (decision: Option A,
  shipped 2026-05-17). Schema at `crates/worktree-protocol/proto/sync.proto`.
  Go codegen lands at `internal/proto/syncpb/`.
- **WT-SRV-2** — Wire the codegen'd `SyncServiceServer` interface from
  `internal/proto/syncpb/sync_grpc.pb.go` into a real gRPC listener
  (blocked on WT-PROTO-1 — now unblocked).
- **WT-SRV-3** — Object store: content-addressable BLAKE3 + dedup +
  per-tenant namespacing per Server.md §14.
- **WT-SRV-4** — Single-tenant sync handlers: stage, push, pull,
  config-sync, tag-push.
- **WT-SRV-5** — `/health` + `/metrics` (Prometheus) endpoints per
  Server.md §20.

## Spec

`../../crates/worktree-protocol/specs/server/Server.md`

## Prerequisites

- Go 1.22 or later
- [`golangci-lint`](https://golangci-lint.run/) — install once, used
  by `scripts/ci.sh`
- [`buf`](https://buf.build) — protobuf workflow (lint + codegen). Install
  via `go install github.com/bufbuild/buf/cmd/buf@latest`.
- `protoc-gen-go` + `protoc-gen-go-grpc` (Go protobuf plugins). Install via
  `go install google.golang.org/protobuf/cmd/protoc-gen-go@latest` and
  `go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest`.
- (Optional) `protoc` itself, for direct invocations outside buf. Not
  required when using `buf generate`.

## Build + run

```bash
cd services/server-go
go build ./...
./server-go
```

Output:

```
worktree-server: not yet implemented (skeleton — WT-SRV-1)
```

## Regenerating protobuf code

The wire-protocol schema lives at
`../../crates/worktree-protocol/proto/sync.proto`. The Go bindings under
`internal/proto/syncpb/` are checked in and regenerated via:

```bash
cd ../../crates/worktree-protocol/proto
buf generate
```

(`buf.gen.yaml` in that directory drives the codegen.) Re-run after any
edit to `sync.proto`. CI runs the same step automatically and fails if
the checked-in `.pb.go` files differ from the regenerated output.

Buf lint enforces the conventions documented at
`../../crates/worktree-protocol/proto/CONVENTIONS.md`:

```bash
cd ../../crates/worktree-protocol/proto
buf lint
```
