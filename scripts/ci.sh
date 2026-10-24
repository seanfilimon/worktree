#!/usr/bin/env bash
# Local equivalent of .github/workflows/ci.yml.
#
#   bash scripts/ci.sh                 # rust, go, web, admin (fail fast)
#   bash scripts/ci.sh --rust --go     # only the selected parts
#   bash scripts/ci.sh --no-web        # everything except web
#   bash scripts/ci.sh --all           # also run the e2e suite
#   bash scripts/ci.sh -k              # keep going after failures, summarize at the end
# want_* flags are read through eval in enabled():
# shellcheck disable=SC2034
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
EXE=""
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) EXE=".exe" ;; esac

usage() {
  cat <<'EOF'
Usage: scripts/ci.sh [PARTS...] [OPTIONS]

Parts (default: --rust --go --web --admin):
  --rust          cargo fmt --check, clippy -D warnings, cargo test
  --go            gofmt, go vet, go build, go test, proto drift, golangci-lint (if installed)
  --web           apps/web build + lint
  --admin         apps/admin lint, typecheck, test, build
  --e2e           scripts/e2e.sh (builds binaries first)
  --all           all of the above
  --no-<part>     skip a part (e.g. --no-web)

Options:
  -k, --keep-going  run every step, report failures at the end
  -h, --help        show this help
EOF
}

# Per-part state: "" = unset, 1 = selected, 0 = skipped (plain variables so
# this also runs on macOS's bash 3.2). They are read via eval in enabled().
want_rust=; want_go=; want_web=; want_admin=; want_e2e=
selected=0
keep_going=0
for arg in "$@"; do
  case "$arg" in
    --rust|--go|--web|--admin|--e2e) eval "want_${arg#--}=1"; selected=1 ;;
    --all) want_rust=1; want_go=1; want_web=1; want_admin=1; want_e2e=1; selected=1 ;;
    --no-rust|--no-go|--no-web|--no-admin|--no-e2e) eval "want_${arg#--no-}=0" ;;
    -k|--keep-going) keep_going=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "ci.sh: unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done
if [ "$selected" -eq 0 ]; then
  : "${want_rust:=1}" "${want_go:=1}" "${want_web:=1}" "${want_admin:=1}"
fi
enabled() { eval "[ \"\${want_$1:-0}\" = 1 ]"; }

if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; R=$'\033[31m'; Y=$'\033[33m'; N=$'\033[0m'; else B=; G=; R=; Y=; N=; fi
passed=(); failed=(); skipped=(); warned=()
soft_failed=0

# step NAME CMD...  -- run CMD, record result; abort unless --keep-going.
step() {
  local name="$1"; shift
  printf '\n%s==> %s%s\n' "$B" "$name" "$N"
  local start=$SECONDS
  soft_failed=0
  if "$@"; then
    if [ "$soft_failed" = 1 ]; then
      printf '%s[warn]%s %s failed (advisory, not counted)\n' "$Y" "$N" "$name"
      warned+=("$name")
    else
      printf '%s[ok]%s %s (%ss)\n' "$G" "$N" "$name" "$((SECONDS - start))"
      passed+=("$name")
    fi
  else
    printf '%s[FAIL]%s %s (%ss)\n' "$R" "$N" "$name" "$((SECONDS - start))"
    failed+=("$name")
    if [ "$keep_going" -eq 0 ]; then summary; exit 1; fi
  fi
}
skip() { printf '\n%s[skip]%s %s: %s\n' "$Y" "$N" "$1" "$2"; skipped+=("$1 ($2)"); }
need() { command -v "$1" >/dev/null 2>&1; }

summary() {
  printf '\n%s==> Summary%s\n' "$B" "$N"
  local s
  for s in "${passed[@]+"${passed[@]}"}"; do printf '  %s[ok]%s   %s\n' "$G" "$N" "$s"; done
  for s in "${skipped[@]+"${skipped[@]}"}"; do printf '  %s[skip]%s %s\n' "$Y" "$N" "$s"; done
  for s in "${warned[@]+"${warned[@]}"}"; do printf '  %s[warn]%s %s\n' "$Y" "$N" "$s"; done
  for s in "${failed[@]+"${failed[@]}"}"; do printf '  %s[FAIL]%s %s\n' "$R" "$N" "$s"; done
}

# ---------------------------------------------------------------- go helpers
go_in() { (cd "$ROOT/services/server-go" && "$@"); }
gofmt_check() {
  local out
  out="$(cd "$ROOT/services/server-go" && gofmt -l .)" || return 1
  if [ -n "$out" ]; then echo "gofmt needed on:"; echo "$out"; return 1; fi
}
# Regenerate the Go bindings and compare with the working tree; always
# restores the original files so the check has no side effects.
proto_drift() {
  local dir="$ROOT/services/server-go/internal/proto" tmp rc=0
  tmp="$(mktemp -d)"
  cp -R "$dir/." "$tmp/"
  if ! bash "$ROOT/scripts/gen-proto.sh"; then rc=1
  elif ! diff -r "$tmp" "$dir" >/dev/null; then
    echo "generated code differs from services/server-go/internal/proto:"
    diff -r "$tmp" "$dir" | head -40 || true
    echo "run 'bash scripts/gen-proto.sh' and commit the result"
    rc=1
  fi
  rm -rf "$dir" && mkdir -p "$dir" && cp -R "$tmp/." "$dir/"
  rm -rf "$tmp"
  return "$rc"
}
have_protoc_plugins() {
  local gobin; gobin="$(go env GOPATH)/bin"
  { need protoc-gen-go || [ -x "$gobin/protoc-gen-go" ] || [ -x "$gobin/protoc-gen-go.exe" ]; } &&
  { need protoc-gen-go-grpc || [ -x "$gobin/protoc-gen-go-grpc" ] || [ -x "$gobin/protoc-gen-go-grpc.exe" ]; }
}

# ---------------------------------------------------------------- npm helpers
npm_ready() {
  [ -d "$ROOT/node_modules" ] || step "npm ci" npm ci --no-audit --no-fund
}
ws_present() { [ -f "$ROOT/apps/$1/package.json" ]; }
npm_ws() { npm run "$2" --workspace="apps/$1" --if-present; }
# soft CMD... -- advisory: a failure is reported as [warn] but never fails CI.
soft() { "$@" || soft_failed=1; return 0; }

# ---------------------------------------------------------------- pipeline
if enabled rust; then
  if need cargo; then
    step "rust: fmt"    cargo fmt --all -- --check
    step "rust: clippy" cargo clippy --workspace --all-targets -- -D warnings
    step "rust: test"   cargo test --workspace
  else skip "rust" "cargo not found"; fi
fi

if enabled go; then
  if need go; then
    step "go: gofmt" gofmt_check
    step "go: vet"   go_in go vet ./...
    step "go: build" go_in go build ./...
    step "go: test"  go_in go test -count=1 ./...
    if have_protoc_plugins; then step "go: proto drift" proto_drift
    else skip "go: proto drift" "protoc-gen-go / protoc-gen-go-grpc not installed"; fi
    if need golangci-lint; then step "go: golangci-lint (advisory)" soft go_in golangci-lint run ./...
    else skip "go: golangci-lint" "not installed"; fi
  else skip "go" "go not found"; fi
fi

if enabled web || enabled admin; then
  if need npm; then npm_ready; fi
fi

if enabled web; then
  if ! need npm; then skip "web" "npm not found"
  elif ! ws_present web; then skip "web" "apps/web missing"
  else
    step "web: build" npm run build --workspace=apps/web
    # Advisory until apps/web replaces `next lint` (removed in Next.js 16).
    step "web: lint (advisory)" soft npm run lint --workspace=apps/web
  fi
fi

if enabled admin; then
  if ! need npm; then skip "admin" "npm not found"
  elif ! ws_present admin; then skip "admin" "apps/admin missing"
  else
    step "admin: lint"      npm_ws admin lint
    step "admin: typecheck" npm_ws admin typecheck
    step "admin: test"      npm_ws admin test
    step "admin: build"     npm run build --workspace=apps/admin
  fi
fi

if enabled e2e; then
  if [ ! -f "$ROOT/scripts/e2e.sh" ]; then skip "e2e" "scripts/e2e.sh missing"
  else
    step "e2e: build rust" cargo build --workspace --bins
    step "e2e: build go"   go_in go build -trimpath -o "$ROOT/target/go/worktree-server$EXE" .
    step "e2e: run"        bash "$ROOT/scripts/e2e.sh"
  fi
fi

summary
[ "${#failed[@]}" -eq 0 ]
