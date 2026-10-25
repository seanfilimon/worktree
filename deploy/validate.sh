#!/usr/bin/env bash
# Validate the packaging assets with whatever tools this machine has.
#
#   bash deploy/validate.sh            # run every available check
#   bash deploy/validate.sh --strict   # fail when a preferred tool is missing
#                                      # (docker, helm, shellcheck, actionlint)
#
# Checks (each skipped with a note when its tool is unavailable):
#   shell       bash -n / sh -n on the scripts, shellcheck when installed
#   powershell  parser check of scripts/*.ps1 (pwsh or powershell)
#   yaml/json   compose file, workflows, chart metadata and values (Node + js-yaml)
#   compose     docker compose config for every profile
#   dockerfile  hadolint when installed
#   helm        helm lint + helm template over deploy/helm/check/cases,
#               else the Go renderer in deploy/helm/check
#   workflows   actionlint when installed
#   systemd     systemd-analyze verify when available, else a section check
#   launchd     plutil -lint when available, else an XML well-formedness check
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
strict=0
[ "${1:-}" = "--strict" ] && strict=1

need() { command -v "$1" >/dev/null 2>&1; }
pass=(); fail=(); skip=()
ok() { pass+=("$1"); printf '  ok    %s\n' "$1"; }
bad() { fail+=("$1"); printf '  FAIL  %s\n' "$1"; }
skipped() {
  skip+=("$1"); printf '  skip  %s (%s)\n' "$1" "$2"
  if [ "$strict" = 1 ] && [ "${3:-}" = preferred ]; then fail+=("$1 (missing tool)"); fi
}
run() { # NAME CMD...
  local name="$1"; shift
  local log
  log="$(mktemp)"
  if "$@" >"$log" 2>&1; then ok "$name"; else bad "$name"; sed 's/^/        /' "$log"; fi
  rm -f "$log"
}

SHELL_SCRIPTS=(scripts/install.sh scripts/release.sh deploy/validate.sh)
POSIX_SCRIPTS=(deploy/docker/healthcheck.sh)

echo "==> shell"
for f in "${SHELL_SCRIPTS[@]}"; do run "bash -n $f" bash -n "$f"; done
for f in "${POSIX_SCRIPTS[@]}"; do run "sh -n $f" sh -n "$f"; done
if need shellcheck; then
  run "shellcheck" shellcheck -x "${SHELL_SCRIPTS[@]}" "${POSIX_SCRIPTS[@]}"
else
  skipped "shellcheck" "not installed" preferred
fi

echo "==> powershell"
ps=""
if need pwsh; then ps=pwsh; elif need powershell; then ps=powershell; fi
if [ -n "$ps" ]; then
  for f in scripts/install.ps1; do
    p="$f"; need cygpath && p="$(cygpath -w "$ROOT/$f")"
    run "parse $f" "$ps" -NoProfile -NonInteractive -Command \
      "\$e=\$null; [void][System.Management.Automation.Language.Parser]::ParseFile('$p',[ref]\$null,[ref]\$e); if (\$e) { \$e | ForEach-Object { Write-Output (\$_.Extent.StartLineNumber.ToString() + ': ' + \$_.Message) }; exit 1 }"
  done
else
  skipped "powershell parse" "no pwsh/powershell"
fi

echo "==> yaml/json"
if need node; then
  run "parse YAML/JSON" node deploy/validate-files.mjs
else
  skipped "parse YAML/JSON" "node not installed"
fi

echo "==> compose"
if need docker && docker compose version >/dev/null 2>&1; then
  for profile in "" s3 smtp; do
    args=(-f deploy/docker-compose.yml)
    [ -n "$profile" ] && args+=(--profile "$profile")
    run "docker compose config ${profile:-default}" env WT_ADMIN_PASSWORD=validate docker compose "${args[@]}" config --quiet
  done
else
  skipped "docker compose config" "docker compose not installed" preferred
fi

echo "==> dockerfile"
if need hadolint; then
  run "hadolint" hadolint deploy/docker/Dockerfile deploy/docker/Dockerfile.cli
else
  skipped "hadolint" "not installed"
fi

echo "==> helm"
chart=deploy/helm/worktree
if need helm; then
  run "helm lint" helm lint --strict "$chart"
  for c in deploy/helm/check/cases/*.yaml; do
    if grep -q '^# expect-error:' "$c"; then
      run "helm template $(basename "$c") (must fail)" sh -c "! helm template wt '$chart' -n worktree -f '$c' >/dev/null 2>&1"
    else
      run "helm template $(basename "$c")" sh -c "helm template wt '$chart' -n worktree -f '$c' >/dev/null"
    fi
  done
else
  skipped "helm lint" "helm not installed; using the Go renderer" preferred
fi
if need go; then
  run "chartcheck (Go renderer)" sh -c "cd deploy/helm/check && go vet . && go test -count=1 . && go run . -chart ../worktree -cases cases"
else
  skipped "chartcheck" "go not installed"
fi

echo "==> workflows"
if need actionlint; then
  run "actionlint" actionlint
else
  skipped "actionlint" "not installed: go install github.com/rhysd/actionlint/cmd/actionlint@v1.7.12" preferred
fi

echo "==> systemd"
for u in deploy/systemd/*.service; do
  run "sections $(basename "$u")" sh -c "grep -q '^\[Unit\]' '$u' && grep -q '^\[Service\]' '$u' && grep -q '^\[Install\]' '$u' && grep -q '^ExecStart=' '$u'"
done
if need systemd-analyze; then
  # verify insists that ExecStart exists: check copies that run /bin/true.
  unit_tmp="$(mktemp -d)"
  # (The bg unit is a user template; it needs a user manager to verify.)
  sed 's#^ExecStart=.*#ExecStart=/bin/true#' deploy/systemd/worktree-server.service > "$unit_tmp/worktree-server.service"
  run "systemd-analyze verify worktree-server.service" systemd-analyze verify --man=no "$unit_tmp/worktree-server.service"
  rm -rf "$unit_tmp"
else
  skipped "systemd-analyze verify" "not available"
fi

echo "==> launchd"
for p in deploy/launchd/*.plist; do
  if need plutil; then
    run "plutil -lint $(basename "$p")" plutil -lint "$p"
  elif need node; then
    run "xml $(basename "$p")" node deploy/validate-files.mjs --plist "$p"
  else
    skipped "plist $(basename "$p")" "no plutil/node"
  fi
done

echo
printf 'passed %d, failed %d, skipped %d\n' "${#pass[@]}" "${#fail[@]}" "${#skip[@]}"
[ "${#fail[@]}" -eq 0 ]
