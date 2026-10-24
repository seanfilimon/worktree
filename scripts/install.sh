#!/usr/bin/env bash
set -euo pipefail
echo "Worktree Installer"
echo "==================="
echo ""
echo "Building from source..."
cargo build --release
echo ""
echo "Installing binaries..."
# TODO: Copy binaries to appropriate locations
# TODO: Register background service
echo "Installation complete."
