#!/bin/sh
# Collect tool binaries from an Alpine container for the given docker platform.
# Usage: bundle-tools.sh <docker-platform> <wimlib-url> <out-dir>
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
    docker run --rm --platform "$PLATFORM" \
        -v "$OUT:/out" \
        alpine:latest sh -c '
            apk add --no-scripts wimlib
            cp "$(find /usr/bin /bin -name wimlib-imagex | head -1)" /out/wimlib-imagex
        '
fi

# --- other tools -------------------------------------------------------------
docker run --rm --platform "$PLATFORM" \
    -v "$OUT:/out" \
    alpine:latest sh -c '
        apk add --no-scripts gptfdisk ntfs-3g ntfs-3g-progs dosfstools parted efibootmgr util-linux psmisc

        grab() {
            name="$1"
            # search common Alpine bin locations + full usr tree
            bin=$(find /usr/local/sbin /usr/local/bin /usr/sbin /usr/bin /sbin /bin \
                       -name "$name" 2>/dev/null | head -1)
            if [ -z "$bin" ]; then
                # last resort — full search (slow but reliable)
                bin=$(find / -xdev -name "$name" 2>/dev/null | head -1)
            fi
            if [ -z "$bin" ]; then
                echo "ERROR: $name not found — installed files:"
                apk info -L gptfdisk 2>/dev/null | grep -i "$name" || true
                exit 1
            fi
            echo "  $name -> $bin"
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
echo "bundled tools:"
ls -lh "$OUT"
