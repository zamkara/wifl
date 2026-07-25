#!/bin/sh
# Collect tool binaries from an Alpine container for the given docker platform.
# Usage: bundle-tools.sh <docker-platform> <out-dir>
set -e

PLATFORM="$1"
OUT="$2"

mkdir -p "$OUT"

docker run --rm --platform "$PLATFORM" \
    -v "$OUT:/out" \
    alpine:latest sh -c '
        set -e
        apk add --no-scripts wimlib gptfdisk ntfs-3g ntfs-3g-progs dosfstools parted efibootmgr util-linux psmisc

        # Locate binary via apk package database (reliable on Alpine regardless of PATH)
        pkg_find() {
            pkg="$1"; bin="$2"
            # apk info -L lists paths without leading /
            path=$(apk info -L "$pkg" 2>/dev/null | grep -E "(^|/)${bin}$" | head -1)
            if [ -n "$path" ]; then
                echo "/$path"
                return 0
            fi
            # fallback: direct lookup in common dirs
            for d in /usr/bin /usr/sbin /bin /sbin /usr/local/bin /usr/local/sbin; do
                [ -f "$d/$bin" ] && echo "$d/$bin" && return 0
            done
            return 1
        }

        grab() {
            name="$1"; pkg="${2:-$1}"
            bin=$(pkg_find "$pkg" "$name") || {
                echo "ERROR: $name not found in package $pkg"
                echo "package contents:"
                apk info -L "$pkg" 2>/dev/null || true
                exit 1
            }
            echo "  $name  <-  $bin"
            cp "$bin" "/out/$name"
        }

        grab wimlib-imagex wimlib
        grab sgdisk        gptfdisk
        grab mkntfs        ntfs-3g-progs
        grab mkfs.fat      dosfstools
        grab partprobe     parted
        grab efibootmgr    efibootmgr
        grab lsblk         lsblk
        grab fuser         psmisc
    '

chmod +x "$OUT"/*
echo "bundled tools:"
ls -lh "$OUT"
