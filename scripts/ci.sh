#!/usr/bin/env bash
set -euo pipefail
echo "Running Worktree CI pipeline..."
echo ""
echo "Step 1: Check formatting (Rust)..."
cargo fmt --all -- --check
echo ""
echo "Step 2: Run clippy (Rust)..."
cargo clippy --workspace -- -D warnings
echo ""
echo "Step 3: Run tests (Rust)..."
cargo test --workspace
echo ""
echo "Step 4: Build release (Rust)..."
cargo build --release
echo ""
echo "Step 5: Lint proto schema (buf)..."
(cd crates/worktree-protocol/proto && buf lint)
echo ""
echo "Step 6: Regenerate Go protobuf code + verify in-sync with .proto..."
(cd crates/worktree-protocol/proto && buf generate)
test -z "$(cd services/server-go && git diff --stat internal/proto/syncpb/ 2>/dev/null || true)" || {
  echo "ERROR: services/server-go/internal/proto/syncpb/ is out of sync with proto/sync.proto."
  echo "       Run \`cd crates/worktree-protocol/proto && buf generate\` and commit."
  exit 1
}
echo ""
echo "Step 7: Check formatting (Go)..."
(cd services/server-go && test -z "$(gofmt -l .)")
echo ""
echo "Step 8: Run vet (Go)..."
(cd services/server-go && go vet ./...)
echo ""
echo "Step 9: Run golangci-lint (Go)..."
(cd services/server-go && golangci-lint run ./...)
echo ""
echo "Step 10: Run tests (Go)..."
(cd services/server-go && go test ./...)
echo ""
echo "Step 11: Build (Go)..."
(cd services/server-go && go build ./...)
echo ""
echo "CI pipeline complete."
