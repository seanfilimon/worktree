#!/usr/bin/env bash
# Railway helpers for the W0rkTree website and docs (apps/web).
#
# Normal deploys need none of this: Railway's GitHub integration deploys
# pushes to main (after the "Docs site checks" workflow passes, with Wait
# for CI on). Use `deploy` only for manual or emergency deploys.
#
#   bash scripts/deploy-docs.sh setup    # one-time: service, variables, domain
#   bash scripts/deploy-docs.sh check    # lint (incl. link check) and build locally
#   bash scripts/deploy-docs.sh deploy   # manual: check, then upload and deploy
#   bash scripts/deploy-docs.sh deploy --no-check
#
# Railway builds apps/web/Dockerfile from the repository root, configured by
# apps/web/railway.json (see docs/deploy-docs-site.md).
#
# Env: RAILWAY_DOCS_SERVICE (default "docs"), RAILWAY_ENVIRONMENT (default:
#      the linked environment, or the one a project token belongs to),
#      RAILWAY_TOKEN (project token) or a prior `railway login` and
#      `railway link`, RAILWAY_CLI_VERSION (used when `railway` is not on
#      PATH; default 5.66.0).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SERVICE="${RAILWAY_DOCS_SERVICE:-docs}"
CLI_VERSION="${RAILWAY_CLI_VERSION:-5.66.0}"
PORT=8080

usage() {
  sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

railway() {
  if command -v railway >/dev/null 2>&1; then
    command railway "$@"
  else
    npx --yes "@railway/cli@${CLI_VERSION}" "$@"
  fi
}

env_args=()
if [[ -n "${RAILWAY_ENVIRONMENT:-}" ]]; then
  env_args=(--environment "$RAILWAY_ENVIRONMENT")
fi

check() {
  echo "==> npm ci"
  npm ci --no-audit --no-fund
  echo "==> lint apps/web (eslint + link check)"
  npm run lint --workspace=apps/web
  echo "==> build apps/web"
  npm run build --workspace=apps/web
}

require_project() {
  if [[ -z "${RAILWAY_TOKEN:-}" ]] && ! railway status >/dev/null 2>&1; then
    echo "error: no Railway project. Run 'railway login' and 'railway link' (or 'railway init')," >&2
    echo "       or export RAILWAY_TOKEN with a project token." >&2
    exit 1
  fi
}

setup() {
  require_project
  if railway variable list --service "$SERVICE" ${env_args[@]+"${env_args[@]}"} --kv >/dev/null 2>&1; then
    echo "==> service '$SERVICE' exists"
  else
    echo "==> creating service '$SERVICE'"
    railway add --service "$SERVICE"
  fi

  echo "==> setting variables"
  railway variable set --service "$SERVICE" ${env_args[@]+"${env_args[@]}"} --skip-deploys \
    "PORT=$PORT" \
    "RAILWAY_DOCKERFILE_PATH=apps/web/Dockerfile" \
    "NEXT_TELEMETRY_DISABLED=1"

  if [[ -n "$(railway domain list --service "$SERVICE" ${env_args[@]+"${env_args[@]}"} 2>/dev/null | grep -E '[a-z0-9-]+\.[a-z]' || true)" ]]; then
    echo "==> domain already configured:"
    railway domain list --service "$SERVICE" ${env_args[@]+"${env_args[@]}"}
  else
    echo "==> generating a railway.app domain on port $PORT"
    railway domain --service "$SERVICE" ${env_args[@]+"${env_args[@]}"} --port "$PORT"
  fi

  cat <<EOF

Finish in the Railway dashboard (service '$SERVICE' > Settings):
  Source:                                connect seanfilimon/worktree, branch main
  Source > Root Directory:               leave empty (the repository root)
  Config-as-code > Railway Config File:  /apps/web/railway.json
  Wait for CI:                           on, once the CI workflow is green
Railway then deploys every push to main that changes the site.
EOF
}

deploy() {
  local run_check=1
  for arg in "$@"; do
    case "$arg" in
      --no-check) run_check=0 ;;
      *) echo "unknown option: $arg" >&2; usage 2 ;;
    esac
  done
  ((run_check)) && check
  require_project

  local message="deploy-docs"
  if git rev-parse --short HEAD >/dev/null 2>&1; then
    message="$(git rev-parse --short HEAD) $(git log -1 --format=%s | cut -c1-72)"
  fi
  echo "==> railway up --service $SERVICE"
  # --ci streams the build logs and exits non-zero when the build fails.
  railway up --ci --service "$SERVICE" ${env_args[@]+"${env_args[@]}"} --message "$message"
}

case "${1:-}" in
  setup) shift; setup "$@" ;;
  check) shift; check "$@" ;;
  deploy) shift; deploy "$@" ;;
  -h | --help | help | "") usage 0 ;;
  *) echo "unknown command: $1" >&2; usage 2 ;;
esac
