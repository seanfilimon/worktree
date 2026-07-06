#!/usr/bin/env bash
set -euo pipefail
echo "Running Worktree CI pipeline..."
echo ""
echo "Step 1: Check formatting..."
cargo fmt --all -- --check
echo ""
echo "Step 2: Run clippy..."
cargo clippy --workspace -- -D warnings
echo ""
echo "Step 3: Run tests..."
cargo test --workspace
echo ""
echo "Step 4: Build release..."
cargo build --release
echo ""
echo "Step 5: Lint proto schema (buf, skipped if not installed)..."
if command -v buf >/dev/null 2>&1; then
  (cd crates/worktree-protocol/proto && buf lint)
else
  echo "buf not installed — skipping proto lint (CI runs it via the buf action)."
fi
echo ""
echo "CI pipeline complete."
