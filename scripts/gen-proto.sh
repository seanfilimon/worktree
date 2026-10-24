#!/usr/bin/env bash
# Regenerate Go bindings for crates/worktree-protocol/proto/sync.proto.
# Uses $PROTOC if set, otherwise the protoc vendored by the
# protoc-bin-vendored crate (run `cargo build -p worktree-protocol` once first).
# Requires protoc-gen-go v1.36.11 and protoc-gen-go-grpc v1.6.2 on PATH
# (or in $(go env GOPATH)/bin).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$(go env GOPATH)/bin:$PATH"
if [ -z "${PROTOC:-}" ]; then
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) plat="win32"; exe="protoc.exe" ;;
    Darwin) plat="macos-$( [ "$(uname -m)" = arm64 ] && echo aarch_64 || echo x86_64 )"; exe="protoc" ;;
    *) plat="linux-$( [ "$(uname -m)" = aarch64 ] && echo aarch_64 || echo x86_64 )"; exe="protoc" ;;
  esac
  VDIR="$(ls -d "$HOME"/.cargo/registry/src/*/protoc-bin-vendored-"$plat"-* | sort | tail -1)"
  PROTOC="$VDIR/bin/$exe"
  INCLUDE="$VDIR/include"
else
  INCLUDE="$(dirname "$PROTOC")/../include"
fi
OUT="$ROOT/services/server-go/internal/proto/syncpb"
cd "$ROOT/crates/worktree-protocol/proto"
"$PROTOC" -I . -I "$INCLUDE" \
  --go_out="$OUT" --go_opt=paths=source_relative \
  --go-grpc_out="$OUT" --go-grpc_opt=paths=source_relative \
  sync.proto
echo "generated Go bindings in $OUT"
