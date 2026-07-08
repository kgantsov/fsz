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

# 2. Detect Architecture
ARCH=$(uname -m)
OS=$(uname -s)

if [ "$OS" != "Darwin" ]; then
    echo "This installer script only supports macOS."
    exit 1
fi

if [ "$ARCH" = "arm64" ]; then
    TARGET="fsz-aarch64-apple-darwin"
elif [ "$ARCH" = "x86_64" ]; then
    TARGET="fsz-x86_64-apple-darwin"
else
    echo "Unsupported architecture: $ARCH"
    exit 1
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARGET}"
TMP_DIR=$(mktemp -d)
TMP_BIN="${TMP_DIR}/${BINARY_NAME}"

echo "Downloading ${BINARY_NAME} ${VERSION} for ${ARCH}..."
if ! curl -fsSL "$URL" -o "$TMP_BIN"; then
    echo "Error: Failed to download binary from $URL"
    exit 1
fi

# 3. Make executable
chmod +x "$TMP_BIN"

# 4. Strip macOS Gatekeeper Quarantine flag
echo "Bypassing macOS Gatekeeper quarantine..."
xattr -d com.apple.quarantine "$TMP_BIN" 2>/dev/null || true

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
