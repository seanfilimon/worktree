#!/usr/bin/env bash
# Local development: Go server on 127.0.0.1:7420 + admin panel Vite dev
# server on http://localhost:5173 (proxies /api and /healthz to the server).
#
#   bash scripts/dev.sh               # server + admin
#   bash scripts/dev.sh --server-only
#   bash scripts/dev.sh --admin-only
#
# Env: WT_ADDR (default 127.0.0.1:7420), WT_DATA (default ./wt-data),
#      WT_ADMIN_USER (default admin), WT_ADMIN_PASSWORD (default "admin" for
#      local dev only), WT_LOG_LEVEL (default debug).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_server=1
run_admin=1
for arg in "$@"; do
  case "$arg" in
    --server-only) run_admin=0 ;;
    --admin-only) run_server=0 ;;
    -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "dev.sh: unknown argument: $arg" >&2; exit 2 ;;
  esac
done

is_windows=0
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) is_windows=1 ;; esac
exe=""; [ "$is_windows" = 1 ] && exe=".exe"

export WT_ADDR="${WT_ADDR:-127.0.0.1:7420}"
export WT_DATA="${WT_DATA:-$ROOT/wt-data}"
export WT_ADMIN_USER="${WT_ADMIN_USER:-admin}"
if [ -z "${WT_ADMIN_PASSWORD:-}" ]; then
  export WT_ADMIN_PASSWORD=admin
  echo "dev.sh: WT_ADMIN_PASSWORD not set; using the local-dev default 'admin'" >&2
fi
log_level="${WT_LOG_LEVEL:-debug}"

pids=()
# Stop a background job and everything it spawned. Native Windows processes
# (go-built server, node) are not in MSYS process groups, so use taskkill /T.
kill_tree() {
  local pid="$1" winpid
  kill -0 "$pid" 2>/dev/null || return 0
  if [ "$is_windows" = 1 ] && [ -r "/proc/$pid/winpid" ]; then
    winpid="$(cat "/proc/$pid/winpid")"
    taskkill //F //T //PID "$winpid" >/dev/null 2>&1 || true
  fi
  kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
}
cleanup() {
  trap - EXIT INT TERM
  local pid
  for pid in "${pids[@]+"${pids[@]}"}"; do kill_tree "$pid"; done
  wait 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Each job gets its own process group so kill_tree can signal its children.
set -m

if [ "$run_server" = 1 ]; then
  bin="$ROOT/target/go/worktree-server$exe"
  echo "==> building worktree-server -> $bin"
  (cd "$ROOT/services/server-go" && go build -o "$bin" .)
  mkdir -p "$WT_DATA"
  admin_dir="$ROOT/apps/admin/dist"
  args=(--addr "$WT_ADDR" --data "$WT_DATA" --log-level "$log_level")
  [ -f "$admin_dir/index.html" ] && args+=(--admin-dir "$admin_dir")
  echo "==> server on http://$WT_ADDR (data: $WT_DATA)"
  "$bin" "${args[@]}" &
  pids+=("$!")
fi

if [ "$run_admin" = 1 ]; then
  if [ ! -f "$ROOT/apps/admin/package.json" ]; then
    echo "dev.sh: apps/admin not found; running server only" >&2
  else
    [ -d "$ROOT/node_modules" ] || (cd "$ROOT" && npm ci --no-audit --no-fund)
    echo "==> admin dev server on http://localhost:5173"
    (cd "$ROOT" && exec npm run dev --workspace=apps/admin) &
    pids+=("$!")
  fi
fi

[ "${#pids[@]}" -gt 0 ] || { echo "dev.sh: nothing to run" >&2; exit 1; }
echo "==> running; press Ctrl+C to stop"

# Exit as soon as any process stops (portable replacement for `wait -n`).
while :; do
  for pid in "${pids[@]}"; do
    if ! kill -0 "$pid" 2>/dev/null; then
      rc=0; wait "$pid" || rc=$?
      echo "dev.sh: process $pid exited with status $rc; shutting down" >&2
      exit "$rc"
    fi
  done
  sleep 1
done
