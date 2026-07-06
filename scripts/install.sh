#!/usr/bin/env bash
# W0rkTree installer for Linux/macOS.
#
# Builds the requested components from source (release profile) and installs
# them into a user-level bin directory.
#
#   Components:
#     cli     -> wt + worktree-bg      (the CLI and its background daemon)
#     server  -> worktree-server      (the remote multi-tenant server)
#     all     -> everything (default)
#
# Usage:
#   ./scripts/install.sh [--component all|cli|server] [--prefix DIR] [--uninstall]
#
# Default prefix is ~/.local (binaries land in ~/.local/bin).
set -euo pipefail

COMPONENT="all"
PREFIX="${HOME}/.local"
UNINSTALL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --component) COMPONENT="$2"; shift 2 ;;
    --prefix)    PREFIX="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help)   grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

case "$COMPONENT" in
  all|cli|server) ;;
  *) echo "invalid --component '$COMPONENT' (all|cli|server)" >&2; exit 2 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="${PREFIX}/bin"

BINARIES=()
PACKAGES=()
if [[ "$COMPONENT" == "all" || "$COMPONENT" == "cli" ]]; then
  BINARIES+=("wt" "worktree-bg")
  PACKAGES+=("-p" "worktree-cli" "-p" "worktree-bg")
fi
if [[ "$COMPONENT" == "all" || "$COMPONENT" == "server" ]]; then
  BINARIES+=("worktree-server")
  PACKAGES+=("-p" "worktree-server")
fi

if [[ "$UNINSTALL" == 1 ]]; then
  echo ">> Uninstalling from ${BIN_DIR}"
  for bin in "${BINARIES[@]}"; do
    if [[ -f "${BIN_DIR}/${bin}" ]]; then
      rm -f "${BIN_DIR}/${bin}"
      echo "   removed ${bin}"
    fi
  done
  echo "Uninstall complete."
  exit 0
fi

echo "W0rkTree Installer"
echo "=================="

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust first: https://rustup.rs" >&2
  exit 1
fi

echo ">> Building release binaries (${COMPONENT})..."
(cd "$REPO_ROOT" && cargo build --release "${PACKAGES[@]}")

echo ">> Installing to ${BIN_DIR}"
mkdir -p "$BIN_DIR"
for bin in "${BINARIES[@]}"; do
  install -m 755 "${REPO_ROOT}/target/release/${bin}" "${BIN_DIR}/${bin}"
  echo "   installed ${bin}"
done

echo ""
echo "Installation complete."

case ":$PATH:" in
  *":${BIN_DIR}:"*) ;;
  *)
    echo ""
    echo "NOTE: ${BIN_DIR} is not on your PATH. Add it with:"
    echo "  echo 'export PATH=\"${BIN_DIR}:\$PATH\"' >> ~/.$(basename "${SHELL:-bash}")rc"
    ;;
esac

if [[ "$COMPONENT" == "all" || "$COMPONENT" == "cli" ]]; then
  echo ""
  echo "Next steps:"
  echo "  wt init             # initialize a worktree in the current directory"
  echo "  wt server start     # start the background daemon (auto-snapshots)"
  echo "  wt --help           # everything else"
fi
