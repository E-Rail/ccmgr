#!/usr/bin/env bash
# Installs the ccmgr prebuilt binary for macOS/Linux from GitHub Releases.
set -euo pipefail

REPO="E-Rail/ClaudeCode-Manager"
INSTALL_DIR="${CCMGR_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Darwin) plat="apple-darwin" ;;
  Linux) plat="unknown-linux-gnu" ;;
  *)
    echo "Unsupported OS: $os" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64|amd64) cpu="x86_64" ;;
  arm64|aarch64) cpu="aarch64" ;;
  *)
    echo "Unsupported architecture: $arch" >&2
    exit 1
    ;;
esac

target="${cpu}-${plat}"
asset="ccmgr-${target}.tar.gz"

echo "Looking up latest release..."
latest_tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep -m1 '"tag_name"' | cut -d'"' -f4)
if [ -z "$latest_tag" ]; then
  echo "Could not determine the latest release tag." >&2
  exit 1
fi

url="https://github.com/${REPO}/releases/download/${latest_tag}/${asset}"
echo "Downloading ${asset} (${latest_tag})..."

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

curl -fsSL "$url" -o "$tmpdir/$asset"
tar -xzf "$tmpdir/$asset" -C "$tmpdir"

mkdir -p "$INSTALL_DIR"
mv "$tmpdir/ccmgr" "$INSTALL_DIR/ccmgr"
chmod +x "$INSTALL_DIR/ccmgr"

echo "Installed ccmgr to $INSTALL_DIR/ccmgr"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "Note: $INSTALL_DIR is not on your PATH."
    echo "Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
