#!/bin/sh
set -e

REPO="NetsoftHoldings/hubstaff-cli"
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

ARCHIVE="$BINARY-$TARGET.tar.gz"
URL="https://github.com/$REPO/releases/download/$VERSION/$ARCHIVE"
CHECKSUMS_URL="https://github.com/$REPO/releases/download/$VERSION/checksums-sha256.txt"

echo "Installing $BINARY $VERSION ($TARGET)..."

# Download archive and checksums
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "$TMPDIR/$ARCHIVE"
curl -fsSL "$CHECKSUMS_URL" -o "$TMPDIR/checksums-sha256.txt"

# Verify checksum
cd "$TMPDIR"
EXPECTED=$(grep "$ARCHIVE" checksums-sha256.txt | awk '{print $1}')
if [ -z "$EXPECTED" ]; then
  echo "warning: no checksum found for $ARCHIVE, skipping verification"
else
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL=$(sha256sum "$ARCHIVE" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL=$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')
  else
    echo "warning: no sha256sum or shasum found, skipping verification"
    ACTUAL="$EXPECTED"
  fi

  if [ "$ACTUAL" != "$EXPECTED" ]; then
    echo "error: checksum verification failed!"
    echo "  expected: $EXPECTED"
    echo "  actual:   $ACTUAL"
    echo ""
    echo "The downloaded file may have been tampered with."
    echo "Please report this at https://github.com/$REPO/issues"
    exit 1
  fi
  echo "Checksum verified."
fi

# Extract
tar xzf "$ARCHIVE"

# Install
if [ -w "$INSTALL_DIR" ]; then
  mv "$BINARY" "$INSTALL_DIR/$BINARY"
else
  echo "Installing to $INSTALL_DIR (requires sudo)..."
  sudo mv "$BINARY" "$INSTALL_DIR/$BINARY"
fi

chmod +x "$INSTALL_DIR/$BINARY"

echo "Installed $BINARY $VERSION to $INSTALL_DIR/$BINARY"
echo ""
echo "Get started:"
echo "  $BINARY config set-pat YOUR_PERSONAL_TOKEN"
echo "  $BINARY login"
