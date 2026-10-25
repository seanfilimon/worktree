#!/bin/sh
# Container health check for worktree-server: GET /healthz on the local
# listener. The port comes from WT_ADDR (default :7420) and the scheme from
# WT_TLS_CERT (TLS on => https, certificate not verified because it is
# usually issued for the public name, not 127.0.0.1).
# Override everything with WT_HEALTHCHECK_URL.
set -eu

url="${WT_HEALTHCHECK_URL:-}"
if [ -z "$url" ]; then
  addr="${WT_ADDR:-:7420}"
  port="${addr##*:}"
  scheme=http
  [ -n "${WT_TLS_CERT:-}" ] && scheme=https
  url="$scheme://127.0.0.1:${port:-7420}/healthz"
fi

exec curl --fail --silent --show-error --insecure --max-time 4 --output /dev/null "$url"
