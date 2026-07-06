#!/usr/bin/env bash
# W0rkTree installer for Linux/macOS.
#
# Two modes:
#   build (default)  — compile from source with cargo (requires Rust)
#   --from-release   — download prebuilt binaries from GitHub Releases
#                      (optionally followed by a tag, e.g. v0.1.0; defaults
#                      to the latest release). SHA-256 verified.
#
#   Components:
#     cli     -> wt + worktree-bg      (the CLI and its background daemon)
#     server  -> worktree-server      (the remote multi-tenant server)
#     all     -> everything (default)
#
# Usage:
#   ./scripts/install.sh [--component all|cli|server] [--prefix DIR]
#                        [--from-release [TAG]] [--uninstall]
#
# Default prefix is ~/.local (binaries land in ~/.local/bin).
set -euo pipefail

REPO="seanfilimon/worktree"
COMPONENT="all"
PREFIX="${HOME}/.local"
UNINSTALL=0
FROM_RELEASE=0
RELEASE_TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --component) COMPONENT="$2"; shift 2 ;;
    --prefix)    PREFIX="$2"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    --from-release)
      FROM_RELEASE=1; shift
      if [[ $# -gt 0 && "$1" != -* ]]; then RELEASE_TAG="$1"; shift; fi
      ;;
    -h|--help)   grep '^#' "$0" | sed 's/^#!.*$//; s/^# \{0,1\}//'; exit 0 ;;
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
mkdir -p "$BIN_DIR"

release_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}-${arch}" in
    Linux-x86_64)              echo "x86_64-unknown-linux-gnu" ;;
    Darwin-arm64)              echo "aarch64-apple-darwin" ;;
    Darwin-x86_64)             echo "x86_64-apple-darwin" ;;
    *) echo "unsupported platform for prebuilt binaries: ${os}/${arch} (build from source instead)" >&2; return 1 ;;
  esac
}

install_from_release() {
  local target asset url tmp
  target="$(release_target)"
  asset="w0rktree-${target}.tar.gz"
  if [[ -n "$RELEASE_TAG" ]]; then
    url="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/${asset}"
  else
    url="https://github.com/${REPO}/releases/latest/download/${asset}"
  fi

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  echo ">> Downloading ${url}"
  curl -fsSL -o "${tmp}/${asset}" "$url"
  curl -fsSL -o "${tmp}/${asset}.sha256" "${url}.sha256"

  echo ">> Verifying checksum"
  (cd "$tmp" && {
    if command -v sha256sum >/dev/null; then sha256sum -c "${asset}.sha256"; else shasum -a 256 -c "${asset}.sha256"; fi
  })

  echo ">> Installing to ${BIN_DIR}"
  tar -xzf "${tmp}/${asset}" -C "$tmp"
  for bin in "${BINARIES[@]}"; do
    install -m 755 "${tmp}/w0rktree-${target}/${bin}" "${BIN_DIR}/${bin}"
    echo "   installed ${bin}"
  done
}

install_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. Install Rust (https://rustup.rs) or use --from-release." >&2
    exit 1
  fi
  echo ">> Building release binaries (${COMPONENT})..."
  (cd "$REPO_ROOT" && cargo build --release "${PACKAGES[@]}")
  echo ">> Installing to ${BIN_DIR}"
  for bin in "${BINARIES[@]}"; do
    install -m 755 "${REPO_ROOT}/target/release/${bin}" "${BIN_DIR}/${bin}"
    echo "   installed ${bin}"
  done
}

if [[ "$FROM_RELEASE" == 1 ]]; then
  install_from_release
else
  install_from_source
fi

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
