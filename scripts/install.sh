#!/usr/bin/env bash
# Install W0rkTree (wt + worktree-bg, optionally worktree-server).
#
#   curl -fsSL https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.sh | bash
#   bash scripts/install.sh --version v0.7.0 --with-server
#   bash scripts/install.sh --base-url https://mirror.example.com/worktree/v0.7.0
#   bash scripts/install.sh --base-url ./dist          # archives from scripts/release.sh
#   bash scripts/install.sh --from-source              # cargo install from this checkout
#
# The archive for this OS/architecture is verified against SHA256SUMS (or
# the archive's own .sha256 file for older releases) before anything is
# installed. Nothing is installed when verification fails.
set -euo pipefail

REPO="${WT_REPO:-seanfilimon/worktree}"
VERSION="${WT_VERSION:-latest}"
INSTALL_DIR="${WT_INSTALL_DIR:-$HOME/.local/bin}"
BASE_URL="${WT_BASE_URL:-}"
TARGET="${WT_TARGET:-}"
WITH_SERVER=0
FROM_SOURCE=0

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  --version VERSION   release tag to install, e.g. v0.7.0 (default: latest)
  --dir DIR           install directory (default: \$HOME/.local/bin)
  --base-url URL      where the release archives and SHA256SUMS live:
                      https://..., file:///path or a local directory
                      (default: https://github.com/$REPO/releases/download/VERSION)
  --target TRIPLE     archive target to install (default: detected, e.g.
                      x86_64-unknown-linux-gnu)
  --with-server       also install worktree-server
  --from-source       build with cargo (and go for the server) instead of
                      downloading a release; uses this checkout if run from
                      one, otherwise https://github.com/$REPO
  --repo OWNER/NAME   GitHub repository (default: $REPO)
  -h, --help          show this help

Environment: WT_VERSION, WT_INSTALL_DIR, WT_BASE_URL, WT_TARGET and WT_REPO
mirror the options above.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir|--bin-dir) INSTALL_DIR="${2:?--dir needs a value}"; shift 2 ;;
    --dir=*|--bin-dir=*) INSTALL_DIR="${1#*=}"; shift ;;
    --base-url) BASE_URL="${2:?--base-url needs a value}"; shift 2 ;;
    --base-url=*) BASE_URL="${1#*=}"; shift ;;
    --target) TARGET="${2:?--target needs a value}"; shift 2 ;;
    --target=*) TARGET="${1#*=}"; shift ;;
    --with-server) WITH_SERVER=1; shift ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --repo) REPO="${2:?--repo needs a value}"; shift 2 ;;
    --repo=*) REPO="${1#*=}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "install.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1; }

# ------------------------------------------------------------ platform
os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Linux) os_part="unknown-linux-gnu"; ext="" ;;
  Darwin) os_part="apple-darwin"; ext="" ;;
  MINGW*|MSYS*|CYGWIN*) os_part="pc-windows-msvc"; ext=".exe" ;;
  *) die "unsupported OS: $os" ;;
esac
case "$arch" in
  x86_64|amd64) arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *) die "unsupported architecture: $arch" ;;
esac
# Rosetta: prefer native arm64 binaries on Apple silicon.
if [ "$os" = Darwin ] && [ "$arch_part" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
  arch_part="aarch64"
fi
[ -n "$TARGET" ] || TARGET="${arch_part}-${os_part}"
case "$TARGET" in
  *-windows-*) archive_ext="zip" ;;
  *) archive_ext="tar.gz" ;;
esac

BINS=(wt worktree-bg)
[ "$WITH_SERVER" = 1 ] && BINS+=(worktree-server)

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t worktree-install)"
trap 'rm -rf "$TMP"' EXIT

# ------------------------------------------------------------ helpers

# local_path URL: prints the filesystem path for file:// URLs and plain
# paths, fails for remote URLs.
local_path() {
  local u="$1"
  case "$u" in
    file://*)
      u="${u#file://}"
      u="${u#localhost}"
      # file:///C:/x -> C:/x (Git Bash / MSYS understands drive paths)
      case "$u" in /[A-Za-z]:/*) u="${u#/}" ;; esac
      printf '%s' "$u" ;;
    http://*|https://*) return 1 ;;
    *) printf '%s' "$u" ;;
  esac
}

# fetch URL DEST: download (or copy, for local sources) URL to DEST.
fetch() {
  local src
  if src="$(local_path "$1")"; then
    [ -f "$src" ] || return 1
    cp "$src" "$2"
    return 0
  fi
  local proto='=https'
  case "$1" in http://*) proto='=http,https' ;; esac
  if need curl; then curl --proto "$proto" --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
  elif need wget; then
    case "$1" in http://*) wget -q -O "$2" "$1" ;; *) wget -q --https-only -O "$2" "$1" ;; esac
  else die "need curl or wget"; fi
}

sha256_of() {
  if need sha256sum; then sha256sum "$1" | awk '{print $1}'
  elif need shasum; then shasum -a 256 "$1" | awk '{print $1}'
  elif need openssl; then openssl dgst -sha256 -r "$1" | awk '{print $1}'
  else die "need sha256sum, shasum or openssl to verify downloads"; fi
}

install_bin() { # SRC NAME
  mkdir -p "$INSTALL_DIR"
  if need install; then install -m 0755 "$1" "$INSTALL_DIR/$2"
  else cp "$1" "$INSTALL_DIR/$2" && chmod 0755 "$INSTALL_DIR/$2"; fi
  say "  installed $INSTALL_DIR/$2"
}

# ------------------------------------------------------------ install
install_release() {
  local base
  if [ -n "$BASE_URL" ]; then
    base="${BASE_URL%/}"
  elif [ "$VERSION" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
    base="https://github.com/$REPO/releases/download/$VERSION"
  fi
  case "$base" in http://*) say "warning: $base is plain HTTP; the checksum only detects corruption, not tampering" >&2 ;; esac

  # Pick the archive from SHA256SUMS (works for "latest" and for mirrors).
  local sums="$TMP/SHA256SUMS" archive="" expected=""
  if fetch "$base/SHA256SUMS" "$sums" 2>/dev/null; then
    local want_version="${VERSION#v}"
    local line name
    while IFS= read -r line || [ -n "$line" ]; do
      line="${line%$'\r'}"
      name="${line##* }"; name="${name#\*}"
      case "$name" in
        worktree-*-"$TARGET.$archive_ext") ;;
        *) continue ;;
      esac
      if [ "$VERSION" != latest ] && [ "$name" != "worktree-$want_version-$TARGET.$archive_ext" ]; then
        continue
      fi
      archive="$name"
      expected="$(printf '%s' "${line%% *}" | tr '[:upper:]' '[:lower:]')"
    done < "$sums"
    [ -n "$archive" ] || die "no $TARGET archive$([ "$VERSION" != latest ] && printf ' for %s' "$VERSION") listed in $base/SHA256SUMS"
  else
    # Older releases publish only per-archive .sha256 files.
    [ "$VERSION" != latest ] || die "could not fetch $base/SHA256SUMS (pass --version to use per-archive checksums)"
    archive="worktree-${VERSION#v}-$TARGET.$archive_ext"
    fetch "$base/$archive.sha256" "$TMP/$archive.sha256" ||
      die "could not fetch $base/SHA256SUMS or $base/$archive.sha256"
    expected="$(awk '{print $1; exit}' "$TMP/$archive.sha256" | tr '[:upper:]' '[:lower:]')"
  fi

  local name="${archive%."$archive_ext"}"
  local version="${name#worktree-}"; version="${version%-"$TARGET"}"
  say "Installing W0rkTree $version ($TARGET) into $INSTALL_DIR"
  fetch "$base/$archive" "$TMP/$archive" || die "download failed: $base/$archive"

  local actual
  actual="$(sha256_of "$TMP/$archive")"
  [ -n "$expected" ] && [ "$expected" = "$actual" ] ||
    die "checksum mismatch for $archive (expected ${expected:-nothing}, got $actual); nothing was installed"
  say "  checksum ok ($actual)"

  mkdir -p "$TMP/x"
  if [ "$archive_ext" = zip ]; then
    if need unzip; then unzip -q "$TMP/$archive" -d "$TMP/x"
    elif need powershell; then
      powershell -NoProfile -NonInteractive -Command \
        "Expand-Archive -Path '$(cygpath -w "$TMP/$archive")' -DestinationPath '$(cygpath -w "$TMP/x")' -Force"
    else die "unzip not found; on Windows use scripts/install.ps1"; fi
  else
    tar -xzf "$TMP/$archive" -C "$TMP/x"
  fi
  local bin bin_ext=""
  case "$TARGET" in *-windows-*) bin_ext=".exe" ;; esac
  for bin in "${BINS[@]}"; do
    [ -f "$TMP/x/$name/$bin$bin_ext" ] || die "$bin$bin_ext missing from $archive"
  done
  for bin in "${BINS[@]}"; do
    install_bin "$TMP/x/$name/$bin$bin_ext" "$bin$bin_ext"
  done
}

install_source() {
  need cargo || die "cargo not found (install Rust from https://rustup.rs)"
  local here src
  here="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd || true)"
  if [ -n "$here" ] && [ -f "$here/Cargo.toml" ] && [ -d "$here/crates/worktree-cli" ]; then
    src="$here"
  else
    need git || die "git not found (needed to fetch the source)"
    say "Cloning https://github.com/$REPO"
    local ref=()
    [ "$VERSION" != latest ] && ref=(--branch "$VERSION")
    git clone --depth 1 "${ref[@]+"${ref[@]}"}" "https://github.com/$REPO.git" "$TMP/src"
    src="$TMP/src"
  fi
  say "Building W0rkTree from source ($src) into $INSTALL_DIR"
  local locked=(); [ -f "$src/Cargo.lock" ] && locked=(--locked)
  local crate
  for crate in worktree-cli worktree-bg; do
    cargo install "${locked[@]+"${locked[@]}"}" --force --root "$TMP/cargo" --path "$src/crates/$crate"
  done
  install_bin "$TMP/cargo/bin/wt$ext" "wt$ext"
  install_bin "$TMP/cargo/bin/worktree-bg$ext" "worktree-bg$ext"
  if [ "$WITH_SERVER" = 1 ]; then
    need go || die "go not found (needed for --with-server --from-source)"
    (cd "$src/services/server-go" && CGO_ENABLED=0 go build -trimpath -o "$TMP/worktree-server$ext" .)
    install_bin "$TMP/worktree-server$ext" "worktree-server$ext"
  fi
}

if [ "$FROM_SOURCE" = 1 ]; then install_source; else install_release; fi

# ------------------------------------------------------------ PATH hint
path_dir="$INSTALL_DIR"
need cygpath && path_dir="$(cygpath -u "$INSTALL_DIR")"
case ":$PATH:" in
  *":$path_dir:"*|*":$path_dir/:"*) ;;
  *)
    say ""
    say "$path_dir is not on your PATH. Add it with:"
    case "${SHELL:-}" in
      */zsh)  say "  echo 'export PATH=\"$path_dir:\$PATH\"' >> ~/.zshrc && source ~/.zshrc" ;;
      */fish) say "  fish_add_path $path_dir" ;;
      *)      say "  echo 'export PATH=\"$path_dir:\$PATH\"' >> ~/.bashrc && source ~/.bashrc" ;;
    esac
    [ -n "$ext" ] && say "  (Windows: or run scripts/install.ps1 -AddToPath to update your user PATH)"
    ;;
esac
say ""
say "Done. Try: wt --help    (start the background daemon with: wt daemon start)"
