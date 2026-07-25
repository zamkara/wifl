#!/bin/sh
# one-line installer:
#   curl -fsSL https://raw.githubusercontent.com/zamkara/wifl/main/install.sh | sh
set -e

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64)          ARCH_SLUG="x86_64"  ;;
    aarch64|arm64)   ARCH_SLUG="aarch64" ;;
    armv7*|armhf)    ARCH_SLUG="armv7"   ;;
    *) echo "unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux)  TARGET="${ARCH_SLUG}-unknown-linux-musl" ;;
    darwin) TARGET="${ARCH_SLUG}-apple-darwin"        ;;
    *)      echo "unsupported OS: $OS"; exit 1 ;;
esac

API="https://api.github.com/repos/zamkara/wifl/releases/latest"
TAG=$(curl -fsSL "$API" | grep '"tag_name"' | sed 's/.*"tag_name": *"\(.*\)".*/\1/')
URL="https://github.com/zamkara/wifl/releases/download/${TAG}/wifl-${TARGET}"

DEST="${1:-/usr/local/bin/wifl}"

echo "· wifl ${TAG}  →  ${DEST}"
curl -fsSL "$URL" -o /tmp/wifl-install
chmod +x /tmp/wifl-install

if [ "$(id -u)" = "0" ]; then
    mv /tmp/wifl-install "$DEST"
else
    sudo mv /tmp/wifl-install "$DEST"
fi

echo "· done — run: sudo wifl"
