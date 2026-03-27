#!/bin/sh
set -e

REPO="chocksy/hubstaff-cli"
INSTALL_DIR="/usr/local/bin"
BINARY="hubstaff-cli"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)
    case "$ARCH" in
      x86_64|amd64) TARGET="x86_64-unknown-linux-musl" ;;
      aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
      *) echo "error: unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  darwin)
    case "$ARCH" in
      x86_64|amd64) TARGET="x86_64-apple-darwin" ;;
      aarch64|arm64) TARGET="aarch64-apple-darwin" ;;
      *) echo "error: unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *) echo "error: unsupported OS: $OS"; exit 1 ;;
esac

# Get latest version
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
if [ -z "$VERSION" ]; then
  echo "error: could not determine latest version"
  exit 1
fi

URL="https://github.com/$REPO/releases/download/$VERSION/$BINARY-$TARGET.tar.gz"

echo "Installing $BINARY $VERSION ($TARGET)..."

# Download and extract
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/$BINARY.tar.gz"
tar xzf "$TMPDIR/$BINARY.tar.gz" -C "$TMPDIR"

# Install
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"
else
  echo "Installing to $INSTALL_DIR (requires sudo)..."
  sudo mv "$TMPDIR/$BINARY" "$INSTALL_DIR/$BINARY"
fi

chmod +x "$INSTALL_DIR/$BINARY"

echo "Installed $BINARY $VERSION to $INSTALL_DIR/$BINARY"
echo ""
echo "Get started:"
echo "  $BINARY config set-pat YOUR_PERSONAL_TOKEN"
echo "  $BINARY login"
