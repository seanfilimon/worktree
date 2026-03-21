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
echo "CI pipeline complete."
