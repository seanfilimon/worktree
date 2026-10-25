#!/usr/bin/env bash
# Build W0rkTree release archives and SHA256SUMS.
#
#   bash scripts/release.sh                  # host target + admin panel
#   bash scripts/release.sh --all            # every target whose Rust toolchain is installed
#   bash scripts/release.sh --target aarch64-apple-darwin --skip-admin
#   bash scripts/release.sh --admin-only     # just the admin panel archive
#
# Per target the archive worktree-<version>-<target>.{tar.gz,zip} holds
#   wt, worktree-bg     (cargo build --release --target <target>)
#   worktree-server     (go build, GOOS/GOARCH, CGO_ENABLED=0)
#   LICENSE, README.md
# plus worktree-admin-<version>.tar.gz (apps/admin/dist), a <file>.sha256 next
# to every archive and SHA256SUMS over all of them. The version comes from
# [workspace.package] in Cargo.toml.
#
# Targets (Rust triple -> GOOS/GOARCH):
#   x86_64-unknown-linux-gnu  aarch64-unknown-linux-gnu
#   x86_64-apple-darwin       aarch64-apple-darwin
#   x86_64-pc-windows-msvc    aarch64-pc-windows-msvc
# A target is built only when its Rust standard library is installed
# (rustup target add <target>) and, for cross builds, a working linker is
# available; others are skipped with a warning (an error with --strict).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ALL_TARGETS=(
  x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
  x86_64-apple-darwin aarch64-apple-darwin
  x86_64-pc-windows-msvc aarch64-pc-windows-msvc
)

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [OPTIONS]

Options:
  --target TRIPLE   build this target (repeatable; default: the host target)
  --all             build every supported target with an installed toolchain
  --out DIR         output directory (default: dist)
  --version VER     override the version (default: Cargo.toml workspace version)
  --skip-admin      do not build the admin panel archive
  --admin-only      only build the admin panel archive
  --skip-build      package existing build outputs (target/<triple>/release, target/go)
  --no-sums         do not write SHA256SUMS (per-archive .sha256 files are still written)
  --strict          fail instead of skipping a target that cannot be built
  --target-dir DIR  cargo target directory (default: $CARGO_TARGET_DIR or target/)
  -h, --help        show this help
EOF
}

say() { printf '==> %s\n' "$*"; }
warn() { printf 'release.sh: warning: %s\n' "$*" >&2; }
die() { printf 'release.sh: error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1; }

targets=()
all=0
out="dist"
version=""
skip_admin=0
admin_only=0
skip_build=0
no_sums=0
strict=0
target_dir="${CARGO_TARGET_DIR:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --target) targets+=("${2:?--target needs a value}"); shift 2 ;;
    --target=*) targets+=("${1#*=}"); shift ;;
    --all) all=1; shift ;;
    --out) out="${2:?--out needs a value}"; shift 2 ;;
    --out=*) out="${1#*=}"; shift ;;
    --version) version="${2:?--version needs a value}"; shift 2 ;;
    --version=*) version="${1#*=}"; shift ;;
    --skip-admin) skip_admin=1; shift ;;
    --admin-only) admin_only=1; shift ;;
    --skip-build) skip_build=1; shift ;;
    --no-sums) no_sums=1; shift ;;
    --strict) strict=1; shift ;;
    --target-dir) target_dir="${2:?--target-dir needs a value}"; shift 2 ;;
    --target-dir=*) target_dir="${1#*=}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "release.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# ------------------------------------------------------------ version
cargo_version() {
  # First `version = "..."` after the [workspace.package] header.
  awk '
    /^\[workspace\.package\]/ { in_pkg = 1; next }
    /^\[/ { in_pkg = 0 }
    in_pkg && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
  ' Cargo.toml
}
[ -n "$version" ] || version="$(cargo_version)"
version="${version#v}"
[ -n "$version" ] || die "could not read the workspace version from Cargo.toml"
printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$' ||
  die "version '$version' is not SemVer"

# ------------------------------------------------------------ paths / tools
native() { if need cygpath; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
case "$out" in /*|[A-Za-z]:*) ;; *) out="$ROOT/$out" ;; esac
mkdir -p "$out"
out="$(cd "$out" && pwd)"
TARGET_DIR="${target_dir:-$ROOT/target}"
case "$TARGET_DIR" in /*|[A-Za-z]:*) ;; *) TARGET_DIR="$ROOT/$TARGET_DIR" ;; esac
export CARGO_TARGET_DIR="$TARGET_DIR"
STAGE="$TARGET_DIR/release-stage"

sha256_line() { # FILE -> "<hash>  <basename>"
  local f="$1" h
  if need sha256sum; then h="$(sha256sum "$f" | awk '{print $1}')"
  elif need shasum; then h="$(shasum -a 256 "$f" | awk '{print $1}')"
  elif need openssl; then h="$(openssl dgst -sha256 -r "$f" | awk '{print $1}')"
  else die "need sha256sum, shasum or openssl"; fi
  printf '%s  %s\n' "$h" "$(basename "$f")"
}

host_target() {
  if need rustc; then
    rustc -vV | awk '/^host: /{print $2}'
  else
    die "rustc not found"
  fi
}

installed_targets=""
if need rustup; then installed_targets="$(rustup target list --installed 2>/dev/null || true)"; fi
target_installed() {
  [ "$1" = "$(host_target)" ] && return 0
  [ -z "$installed_targets" ] && return 1
  printf '%s\n' "$installed_targets" | grep -qx "$1"
}

go_platform() { # TRIPLE -> "GOOS GOARCH EXT ARCHIVE_EXT"
  local arch os
  case "$1" in
    x86_64-*) arch=amd64 ;;
    aarch64-*) arch=arm64 ;;
    *) return 1 ;;
  esac
  case "$1" in
    *-linux-*) os=linux; printf '%s %s "" tar.gz\n' "$os" "$arch" ;;
    *-apple-darwin) os=darwin; printf '%s %s "" tar.gz\n' "$os" "$arch" ;;
    *-windows-*) os=windows; printf '%s %s .exe zip\n' "$os" "$arch" ;;
    *) return 1 ;;
  esac
}

# make_zip DIR_PARENT NAME ARCHIVE: zip DIR_PARENT/NAME into ARCHIVE.
make_zip() {
  local parent="$1" name="$2" archive="$3"
  rm -f "$archive"
  if need zip; then
    (cd "$parent" && zip -qr -X "$archive" "$name")
  elif need 7z; then
    (cd "$parent" && 7z a -tzip -bso0 -bsp0 "$(native "$archive")" "$name" >/dev/null)
  elif [ -x /c/Windows/System32/tar.exe ]; then
    # Windows' bsdtar writes zip archives with -a.
    (cd "$parent" && /c/Windows/System32/tar.exe -a -cf "$(native "$archive")" "$name")
  elif need powershell; then
    powershell -NoProfile -NonInteractive -Command \
      "Compress-Archive -Path '$(native "$parent/$name")' -DestinationPath '$(native "$archive")' -Force"
  else
    die "need zip, 7z, Windows tar or PowerShell to create $archive"
  fi
}

# make_tgz DIR_PARENT NAME ARCHIVE: tar+gzip with executable binaries even
# when the build host's filesystem has no POSIX modes (Windows).
make_tgz() {
  local parent="$1" name="$2" archive="$3"
  rm -f "$archive"
  if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    local tarfile="${archive%.gz}"
    rm -f "$tarfile"
    local owner=(--owner=0 --group=0 --numeric-owner)
    (
      cd "$parent"
      tar -cf "$tarfile" "${owner[@]}" --mode=0755 --no-recursion "$name"
      local f
      for f in "$name"/*; do
        case "$(basename "$f")" in
          wt|worktree-bg|worktree-server|*.exe) tar -rf "$tarfile" "${owner[@]}" --mode=0755 "$f" ;;
          *) tar -rf "$tarfile" "${owner[@]}" --mode=u=rwX,go=rX "$f" ;;
        esac
      done
    )
    gzip -9 -n -f "$tarfile"
  else
    (cd "$parent" && tar -czf "$archive" "$name")
  fi
}

# ------------------------------------------------------------ builds
built_archives=()

build_target() {
  local target="$1" plat goos goarch ext aext
  plat="$(go_platform "$target")" || die "unsupported target: $target"
  eval "set -- $plat"
  goos="$1"; goarch="$2"; ext="$3"; aext="$4"

  local rust_dir="$TARGET_DIR/$target/release"
  local go_out="$TARGET_DIR/go/$goos-$goarch/worktree-server$ext"
  if [ "$skip_build" = 0 ]; then
    if ! target_installed "$target"; then
      local msg="Rust target $target is not installed (rustup target add $target)"
      [ "$strict" = 1 ] && die "$msg"
      warn "$msg; skipping"
      return 0
    fi
    say "cargo build --release --target $target (wt, worktree-bg)"
    local locked=(); [ -f Cargo.lock ] && locked=(--locked)
    if ! cargo build --release "${locked[@]}" --target "$target" -p worktree-cli -p worktree-bg; then
      [ "$strict" = 1 ] && die "cargo build failed for $target"
      warn "cargo build failed for $target (missing cross linker?); skipping"
      return 0
    fi
    say "go build worktree-server ($goos/$goarch)"
    mkdir -p "$(dirname "$go_out")"
    (cd services/server-go &&
      CGO_ENABLED=0 GOOS="$goos" GOARCH="$goarch" go build -trimpath \
        -ldflags "-s -w -X main.version=$version" \
        -o "$(native "$go_out")" .)
  fi

  local name="worktree-$version-$target"
  local stage="$STAGE/$name"
  rm -rf "$stage"
  mkdir -p "$stage"
  local bin
  for bin in wt worktree-bg; do
    [ -f "$rust_dir/$bin$ext" ] || die "missing $rust_dir/$bin$ext"
    cp "$rust_dir/$bin$ext" "$stage/"
  done
  [ -f "$go_out" ] || die "missing $go_out"
  cp "$go_out" "$stage/"
  cp LICENSE README.md "$stage/"
  chmod 0755 "$stage"/wt"$ext" "$stage"/worktree-bg"$ext" "$stage"/worktree-server"$ext" 2>/dev/null || true

  local archive="$out/$name.$aext"
  say "package $(basename "$archive")"
  if [ "$aext" = zip ]; then make_zip "$STAGE" "$name" "$archive"; else make_tgz "$STAGE" "$name" "$archive"; fi
  sha256_line "$archive" > "$archive.sha256"
  built_archives+=("$archive")
}

build_admin() {
  local dist="apps/admin/dist"
  if [ "$skip_build" = 0 ]; then
    need npm || die "npm not found (needed for the admin panel; use --skip-admin)"
    if [ ! -d node_modules ]; then
      say "npm ci"
      npm ci --no-audit --no-fund
    fi
    say "npm run build --workspace=apps/admin"
    npm run build --workspace=apps/admin
  fi
  [ -f "$dist/index.html" ] || die "$dist/index.html missing (build the admin panel first)"
  local name="worktree-admin-$version"
  local archive="$out/$name.tar.gz"
  say "package $(basename "$archive")"
  rm -rf "${STAGE:?}/$name"
  mkdir -p "$STAGE/$name"
  cp -R "$dist"/. "$STAGE/$name/"
  make_tgz "$STAGE" "$name" "$archive"
  sha256_line "$archive" > "$archive.sha256"
  built_archives+=("$archive")
}

# ------------------------------------------------------------ main
say "W0rkTree $version -> $out"
if [ "$admin_only" = 0 ]; then
  if [ "$all" = 1 ]; then
    targets=("${ALL_TARGETS[@]}")
  elif [ "${#targets[@]}" -eq 0 ]; then
    targets=("$(host_target)")
  fi
  for t in "${targets[@]}"; do
    supported=0
    for s in "${ALL_TARGETS[@]}"; do [ "$s" = "$t" ] && supported=1; done
    [ "$supported" = 1 ] || die "unsupported target: $t (supported: ${ALL_TARGETS[*]})"
    build_target "$t"
  done
fi
[ "$skip_admin" = 1 ] || build_admin

[ "${#built_archives[@]}" -gt 0 ] || die "nothing was built"

if [ "$no_sums" = 0 ]; then
  # Cover every archive in the output directory (earlier runs included),
  # so per-target invocations can share one dist/.
  (
    cd "$out"
    : > SHA256SUMS.tmp
    for f in worktree-*.tar.gz worktree-*.zip; do
      [ -f "$f" ] && sha256_line "$f" >> SHA256SUMS.tmp
    done
    LC_ALL=C sort -k2 SHA256SUMS.tmp > SHA256SUMS
    rm -f SHA256SUMS.tmp
  )
  say "SHA256SUMS"
  cat "$out/SHA256SUMS"
fi
say "done: ${#built_archives[@]} archive(s) in $out"
