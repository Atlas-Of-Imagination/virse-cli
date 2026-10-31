#!/bin/sh
# 参考 kitkit-cli 的安装入口；只安装 virse，不复制包内其他文件。
set -eu

REPO=${VIRSE_REPO:-Atlas-Of-Imagination/virse-cli}
VERSION=${VIRSE_VERSION:-latest}
INSTALL_DIR=${VIRSE_INSTALL_DIR:-}
TARGET=${VIRSE_TARGET:-}

die() { printf 'virse install: %s\n' "$*" >&2; exit 1; }
usage() {
  cat <<EOF
Install virse from GitHub Releases.
Usage: install.sh [--version VERSION] [--dir PATH] [--target TARGET] [--repo OWNER/REPO]
Defaults: latest stable release; a writable bin directory in PATH, then ~/.local/bin.
Environment: VIRSE_VERSION, VIRSE_INSTALL_DIR, VIRSE_TARGET, VIRSE_REPO.
Private releases: install GitHub CLI and run gh auth login (or set GH_TOKEN).
Linux defaults to musl; use --target x86_64-unknown-linux-gnu for the GNU build.
EOF
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --version|--dir|--install-dir|--target|--repo)
      [ "$#" -ge 2 ] && [ -n "$2" ] || die "$1 requires a value"
      case "$1" in
        --version) VERSION=$2 ;;
        --dir|--install-dir) INSTALL_DIR=$2 ;;
        --target) TARGET=$2 ;;
        --repo) REPO=$2 ;;
      esac
      shift 2 ;;
    *) die "unknown option: $1" ;;
  esac
done

USE_GH=false
if command -v gh >/dev/null 2>&1 && gh auth status --hostname github.com >/dev/null 2>&1; then
  USE_GH=true
fi

download() {
  if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die 'curl or wget is required'
  fi
}
path_contains() { case ":${PATH:-}:" in *":$1:"*) return 0 ;; *) return 1 ;; esac; }

if [ -z "$TARGET" ]; then
  case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    arm64|aarch64) arch=aarch64 ;;
    *) die 'unsupported CPU architecture; expected x64 or ARM64' ;;
  esac
  case "$(uname -s)" in
    Darwin) TARGET=$arch-apple-darwin ;;
    Linux) TARGET=$arch-unknown-linux-musl ;;
    *) die 'unsupported OS; use install.ps1 on Windows' ;;
  esac
fi
case "$TARGET" in
  x86_64-apple-darwin|aarch64-apple-darwin|x86_64-unknown-linux-musl|aarch64-unknown-linux-musl|x86_64-unknown-linux-gnu) ;;
  *) die "unsupported target: $TARGET" ;;
esac

TMP_INSTALL=$(mktemp -d "${TMPDIR:-/tmp}/virse-install.XXXXXX")
PENDING=''
cleanup() {
  [ -z "$PENDING" ] || rm -f "$PENDING"
  rm -rf "$TMP_INSTALL"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

if [ "$VERSION" = latest ]; then
  if [ "$USE_GH" = true ]; then
    VERSION=$(gh release view --repo "$REPO" --json tagName --jq .tagName)
  else
    download "https://api.github.com/repos/$REPO/releases/latest" "$TMP_INSTALL/release.json"
    VERSION=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TMP_INSTALL/release.json" | head -n 1)
  fi
  [ -n "$VERSION" ] || die 'could not resolve latest release'
fi
case "$VERSION" in v*) ;; *) VERSION=v$VERSION ;; esac
case "$VERSION" in *[!A-Za-z0-9.+-]*|v) die 'invalid version tag' ;; esac

STEM=virse-$VERSION-$TARGET
ARCHIVE=$STEM.tar.gz
BASE_URL=https://github.com/$REPO/releases/download/$VERSION
printf 'Installing virse %s for %s\n' "$VERSION" "$TARGET"
if [ "$USE_GH" = true ]; then
  gh release download "$VERSION" --repo "$REPO" --pattern "$ARCHIVE" --pattern "$STEM.sha256" --dir "$TMP_INSTALL"
  mv "$TMP_INSTALL/$STEM.sha256" "$TMP_INSTALL/checksum"
else
  download "$BASE_URL/$ARCHIVE" "$TMP_INSTALL/$ARCHIVE"
  download "$BASE_URL/$STEM.sha256" "$TMP_INSTALL/checksum"
fi
expected=$(awk -v name="$ARCHIVE" '$2 == name || $2 == "*" name {print $1}' "$TMP_INSTALL/checksum")
[ "${#expected}" -eq 64 ] || die 'missing or invalid archive checksum'
case "$expected" in *[!a-fA-F0-9]*) die 'invalid SHA-256 checksum' ;; esac
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$TMP_INSTALL/$ARCHIVE" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$TMP_INSTALL/$ARCHIVE" | awk '{print $1}')
else
  die 'sha256sum or shasum is required'
fi
[ "$(printf '%s' "$expected" | tr A-F a-f)" = "$actual" ] || die 'SHA-256 mismatch; existing installation unchanged'

# 只提取根目录下的二进制，不安装文档或其他附带文件。
tar -xzf "$TMP_INSTALL/$ARCHIVE" -C "$TMP_INSTALL" virse
[ -f "$TMP_INSTALL/virse" ] && [ ! -L "$TMP_INSTALL/virse" ] || die 'archive must contain a regular virse file'

if [ -z "$INSTALL_DIR" ]; then
  [ -n "${HOME:-}" ] || die 'HOME is not set; pass --dir'
  for candidate in "$HOME/.local/bin" "$HOME/bin" /opt/homebrew/bin /usr/local/bin; do
    if path_contains "$candidate" && [ -d "$candidate" ] && [ -w "$candidate" ]; then
      INSTALL_DIR=$candidate
      break
    fi
  done
  INSTALL_DIR=${INSTALL_DIR:-$HOME/.local/bin}
fi
mkdir -p "$INSTALL_DIR" || die 'cannot create install directory; choose a writable --dir'
INSTALL_DIR=$(cd "$INSTALL_DIR" && pwd -P)
DEST=$INSTALL_DIR/virse
[ ! -d "$DEST" ] || die "$DEST is a directory"
PENDING=$(mktemp "$INSTALL_DIR/.virse-install.XXXXXX")
cp "$TMP_INSTALL/virse" "$PENDING"
chmod 0755 "$PENDING"
mv -f "$PENDING" "$DEST"
PENDING=''
printf 'Installed virse to %s\n' "$DEST"
if ! path_contains "$INSTALL_DIR"; then
  quoted=$(printf '%s' "$INSTALL_DIR" | sed "s/'/'\\\\''/g")
  printf "Add this to your shell profile, then open a new terminal:\n  export PATH='%s':\"\$PATH\"\n" "$quoted"
fi
