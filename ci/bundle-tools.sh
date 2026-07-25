#!/bin/sh
# Collect static tool binaries from an Alpine container for the given docker platform.
# Usage: bundle-tools.sh <docker-platform> <wimlib-url> <out-dir>
# Called by the GitHub Actions workflow.
set -e

PLATFORM="$1"
WIMLIB_URL="$2"
OUT="$3"

mkdir -p "$OUT"

# --- wimlib-imagex -----------------------------------------------------------
if [ -n "$WIMLIB_URL" ]; then
    curl -fsSL "$WIMLIB_URL" \
        | tar xz --wildcards "*/wimlib-imagex" --strip-components=1 -C "$OUT/"
else
    # Pull from Alpine (available for all arches)
    docker run --rm --platform "$PLATFORM" \
        -v "$OUT:/out" \
        alpine:latest sh -c '
            apk add --no-scripts wimlib
            cp "$(which wimlib-imagex)" /out/wimlib-imagex
        '
fi

# --- other tools -------------------------------------------------------------
docker run --rm --platform "$PLATFORM" \
    -v "$OUT:/out" \
    alpine:latest sh -c '
        apk add --no-scripts gptfdisk ntfs-3g ntfs-3g-progs dosfstools parted efibootmgr util-linux psmisc
        # Helper: find binary and copy to /out
        grab() {
            name="$1"
            bin=$(which "$name" 2>/dev/null \
                || find /usr/sbin /sbin /usr/bin /bin -name "$name" 2>/dev/null | head -1)
            [ -n "$bin" ] || { echo "ERROR: $name not found"; exit 1; }
            cp "$bin" "/out/$name"
        }
        grab sgdisk
        grab mkntfs
        grab mkfs.fat
        grab partprobe
        grab efibootmgr
        grab lsblk
        grab fuser
    '

chmod +x "$OUT"/*
echo "bundled tools in $OUT:"
ls -lh "$OUT"
