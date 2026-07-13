#!/bin/bash
set -e

# Configuration
REPO="kgantsov/fsz"
BINARY_NAME="fsz"
INSTALL_DIR="/usr/local/bin"

# 1. Fetch latest release tag dynamically from GitHub API
echo "Checking GitHub for the latest release..."
VERSION=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | awk -F '"' '/"tag_name":/ {print $4}')

if [ -z "$VERSION" ]; then
    echo "Error: Could not retrieve the latest version from GitHub API."
    exit 1
fi

echo "Latest version found: $VERSION"

# 2. Detect OS and Architecture
ARCH=$(uname -m)
OS=$(uname -s)

case "$ARCH" in
    arm64 | aarch64) ARCH="aarch64" ;;
    x86_64 | amd64) ARCH="x86_64" ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

case "$OS" in
    Darwin) TARGET="fsz-${ARCH}-apple-darwin" ;;
    Linux) TARGET="fsz-${ARCH}-unknown-linux-musl" ;;
    *)
        echo "Unsupported operating system: $OS"
        exit 1
        ;;
esac

URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARGET}"
TMP_DIR=$(mktemp -d)
TMP_BIN="${TMP_DIR}/${BINARY_NAME}"

echo "Downloading ${BINARY_NAME} ${VERSION} for ${OS} ${ARCH}..."
if ! curl -fsSL "$URL" -o "$TMP_BIN"; then
    echo "Error: Failed to download binary from $URL"
    exit 1
fi

# 3. Make executable
chmod +x "$TMP_BIN"

# 4. Strip macOS Gatekeeper Quarantine flag
if [ "$OS" = "Darwin" ]; then
    echo "Bypassing macOS Gatekeeper quarantine..."
    xattr -d com.apple.quarantine "$TMP_BIN" 2>/dev/null || true
fi

# 5. Move to installation directory
echo "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$TMP_BIN" "${INSTALL_DIR}/${BINARY_NAME}"
else
    echo "Elevated permissions required to write to ${INSTALL_DIR}"
    sudo mv "$TMP_BIN" "${INSTALL_DIR}/${BINARY_NAME}"
fi

# Clean up
rm -rf "$TMP_DIR"

echo "Successfully installed ${BINARY_NAME}! Run '${BINARY_NAME} --help' to verify."
