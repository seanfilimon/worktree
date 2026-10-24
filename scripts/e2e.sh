#!/usr/bin/env bash
# Build every component and run the W0rkTree end-to-end suite
# (crates/worktree-e2e). Extra arguments are passed to the test binary,
# e.g. `bash scripts/e2e.sh local_workflow` or `bash scripts/e2e.sh --nocapture`.
#
# Env: WT_E2E_THREADS (default 4); failing scenarios keep their temp dir
#      (WT_E2E_KEEP=1 keeps all, WT_E2E_KEEP=0 none);
#      CARGO_TARGET_DIR is honored.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Native (Windows) paths when running under Git Bash / MSYS.
native() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi
}

EXE=""
case "$(uname -s 2>/dev/null || echo unknown)" in
  MINGW* | MSYS* | CYGWIN*) EXE=".exe" ;;
esac

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
case "$TARGET_DIR" in
  /* | [A-Za-z]:*) ;;
  *) TARGET_DIR="$ROOT/$TARGET_DIR" ;;
esac
BIN_DIR="$TARGET_DIR/debug"

echo "==> cargo build -p worktree-cli -p worktree-bg"
cargo build -p worktree-cli -p worktree-bg

echo "==> go build services/server-go"
mkdir -p "$BIN_DIR"
SERVER_BIN="$BIN_DIR/worktree-server$EXE"
(cd services/server-go && CGO_ENABLED=0 go build -o "$(native "$SERVER_BIN")" .)

export WT_BIN WT_BG_BIN WT_SERVER_BIN
WT_BIN="$(native "$BIN_DIR/wt$EXE")"
WT_BG_BIN="$(native "$BIN_DIR/worktree-bg$EXE")"
WT_SERVER_BIN="$(native "$SERVER_BIN")"
export WT_E2E=1

echo "==> WT_E2E=1 cargo test -p worktree-e2e -- --test-threads=${WT_E2E_THREADS:-4} $*"
cargo test -p worktree-e2e -- --test-threads="${WT_E2E_THREADS:-4}" "$@"
