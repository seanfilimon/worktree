#!/usr/bin/env bash
# Install W0rkTree (wt + worktree-bg, optionally worktree-server).
#
#   curl -fsSL https://raw.githubusercontent.com/seanfilimon/worktree/main/scripts/install.sh | bash
#   bash scripts/install.sh --version v0.2.0 --with-server
#   bash scripts/install.sh --from-source          # cargo install from this checkout
#
# Release archives are verified against their published SHA-256 checksum
# before anything is installed.
set -euo pipefail

REPO="${WT_REPO:-seanfilimon/worktree}"
VERSION="${WT_VERSION:-latest}"
INSTALL_DIR="${WT_INSTALL_DIR:-$HOME/.local/bin}"
WITH_SERVER=0
FROM_SOURCE=0

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Options:
  --version VERSION   release tag to install, e.g. v0.2.0 (default: latest)
  --dir DIR           install directory (default: \$HOME/.local/bin)
  --with-server       also install worktree-server
  --from-source       build with cargo (and go for the server) instead of
                      downloading a release; uses this checkout if run from
                      one, otherwise https://github.com/$REPO
  --repo OWNER/NAME   GitHub repository (default: $REPO)
  -h, --help          show this help

Environment: WT_VERSION, WT_INSTALL_DIR, WT_REPO mirror the options above.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir) INSTALL_DIR="${2:?--dir needs a value}"; shift 2 ;;
    --dir=*) INSTALL_DIR="${1#*=}"; shift ;;
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
  Linux) os_part="unknown-linux-gnu"; ext=""; archive_ext="tar.gz" ;;
  Darwin) os_part="apple-darwin"; ext=""; archive_ext="tar.gz" ;;
  MINGW*|MSYS*|CYGWIN*) os_part="pc-windows-msvc"; ext=".exe"; archive_ext="zip" ;;
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
TARGET="${arch_part}-${os_part}"

BINS=(wt worktree-bg)
[ "$WITH_SERVER" = 1 ] && BINS+=(worktree-server)

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t worktree-install)"
trap 'rm -rf "$TMP"' EXIT

# ------------------------------------------------------------ helpers
download() { # URL DEST
  if need curl; then curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
  elif need wget; then wget -q --https-only -O "$2" "$1"
  else die "need curl or wget"; fi
}
sha256_of() {
  if need sha256sum; then sha256sum "$1" | awk '{print $1}'
  elif need shasum; then shasum -a 256 "$1" | awk '{print $1}'
  elif need openssl; then openssl dgst -sha256 -r "$1" | awk '{print $1}'
  else die "need sha256sum, shasum or openssl to verify downloads"; fi
}
resolve_latest() {
  local url
  if need curl; then
    url="$(curl --proto '=https' -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")"
  else
    url="$(wget -q --max-redirect=5 -S --spider "https://github.com/$REPO/releases/latest" 2>&1 | awk '/^  Location: /{print $2}' | tail -1)"
  fi
  url="${url%$'\r'}"
  case "$url" in */tag/*) printf '%s' "${url##*/tag/}" ;; *) die "could not resolve the latest release of $REPO" ;; esac
}
install_bin() { # SRC NAME
  mkdir -p "$INSTALL_DIR"
  if need install; then install -m 0755 "$1" "$INSTALL_DIR/$2"
  else cp "$1" "$INSTALL_DIR/$2" && chmod 0755 "$INSTALL_DIR/$2"; fi
  say "  installed $INSTALL_DIR/$2"
}

# ------------------------------------------------------------ install
install_release() {
  [ "$VERSION" = latest ] && VERSION="$(resolve_latest)"
  case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
  local name="worktree-${VERSION#v}-${TARGET}"
  local archive="${name}.${archive_ext}"
  local base="https://github.com/$REPO/releases/download/$VERSION"

  say "Installing W0rkTree $VERSION ($TARGET) into $INSTALL_DIR"
  download "$base/$archive" "$TMP/$archive" || die "download failed: $base/$archive"
  download "$base/$archive.sha256" "$TMP/$archive.sha256" || die "download failed: $base/$archive.sha256"

  local expected actual
  expected="$(awk '{print $1; exit}' "$TMP/$archive.sha256" | tr 'A-F' 'a-f')"
  actual="$(sha256_of "$TMP/$archive")"
  [ -n "$expected" ] && [ "$expected" = "$actual" ] ||
    die "checksum mismatch for $archive (expected $expected, got $actual)"
  say "  checksum ok ($actual)"

  mkdir -p "$TMP/x"
  if [ "$archive_ext" = zip ]; then
    if need unzip; then unzip -q "$TMP/$archive" -d "$TMP/x"
    else die "unzip not found; on Windows use scripts/install.ps1"; fi
  else
    tar -xzf "$TMP/$archive" -C "$TMP/x"
  fi
  local bin
  for bin in "${BINS[@]}"; do
    [ -f "$TMP/x/$name/$bin$ext" ] || die "$bin$ext missing from $archive"
    install_bin "$TMP/x/$name/$bin$ext" "$bin$ext"
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
