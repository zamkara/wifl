# wifl

**w**indows **i**mage **f**etch + install

Fetches Windows ESD images from the [WOR Project](https://worproject.com) catalogue (direct Microsoft CDN) and installs them to a bare-metal disk — fully interactive, arrow-key driven, no configuration files.

---

## install

```sh
curl -fsSL https://raw.githubusercontent.com/zamkara/wifl/main/install.sh | sh
```

Detects your OS and architecture, downloads the latest release binary, places it at `/usr/local/bin/wifl`.

Or download manually from [releases](https://github.com/zamkara/wifl/releases):

| target | description |
|--------|-------------|
| `wifl-x86_64-unknown-linux-musl` | Linux x86_64 (any distro) |
| `wifl-aarch64-unknown-linux-musl` | Linux ARM64 (RPi 4+, ARM servers) |
| `wifl-armv7-unknown-linux-musleabihf` | Linux ARMv7 (RPi 3, older ARM) |
| `wifl-x86_64-apple-darwin` | macOS Intel |
| `wifl-aarch64-apple-darwin` | macOS Apple Silicon |

---

## usage

```sh
sudo wifl
```

Interactive flow:

1. select Windows version (10 / 11)
2. select build
3. select architecture
4. select language
5. select edition group (Consumer / Business Volume)
6. select destination disk — **all data will be erased**
7. confirm
8. download ESD (skipped if already cached and checksum matches)
9. select edition to install (Pro / Enterprise / Education / …)
10. partition, format, apply, configure boot

Navigate with `↑ ↓` or `j k`. Confirm with `Enter`. Abort with `Esc` or `q`.

ESD files are cached in `~/Documents/ESDs/` and verified by SHA-256 before use.

---

## system requirements

### linux

The pre-built Linux binaries are **self-contained** — all operational tools (wimlib-imagex, sgdisk, mkntfs, mkfs.fat, partprobe, efibootmgr, lsblk, fuser) are embedded and extracted to `/tmp` at runtime.

They are compiled against **musl libc**. On most distributions musl is already present; if not:

| distro | command |
|--------|---------|
| Arch Linux | `sudo pacman -S musl` |
| Debian / Ubuntu | `sudo apt install musl` |
| Fedora | `sudo dnf install musl-libc` |

Additional kernel requirements:
- **ntfs3** or **ntfs-3g** kernel module for post-apply mount (available in Linux 5.15+)
- **UEFI** firmware (legacy BIOS not supported)
- root access for disk operations

### macos

macOS binaries do **not** bundle tools. Install via Homebrew:

```sh
brew install wimlib gptfdisk ntfs-3g dosfstools
```

Note: full disk installation (NTFS write, EFI boot setup) is not supported on macOS — download and inspection only.

---

## build from source

```sh
cargo build --release
```

The build embeds tool binaries if the following environment variables point to them:

```
WIFL_WIMLIB_IMAGEX   path to wimlib-imagex binary
WIFL_SGDISK          path to sgdisk
WIFL_MKNTFS          path to mkntfs
WIFL_MKFS_FAT        path to mkfs.fat
WIFL_PARTPROBE       path to partprobe
WIFL_EFIBOOTMGR      path to efibootmgr
WIFL_LSBLK           path to lsblk
WIFL_FUSER           path to fuser
```

If a variable is unset, the tool is not embedded and wifl falls back to the system `PATH` at runtime.

### production build (Linux, all architectures)

The CI workflow (`ci/bundle-tools.sh`) collects static tool binaries from Alpine Linux containers and produces fully self-contained release binaries.

To replicate locally (requires Docker):

```sh
# x86_64
ci/bundle-tools.sh linux/amd64 \
    "https://wimlib.net/downloads/wimlib-1.14.4-x86_64-linux-bin.tar.gz" \
    ./bundled-tools

export WIFL_WIMLIB_IMAGEX=$PWD/bundled-tools/wimlib-imagex
export WIFL_SGDISK=$PWD/bundled-tools/sgdisk
export WIFL_MKNTFS=$PWD/bundled-tools/mkntfs
export WIFL_MKFS_FAT=$PWD/bundled-tools/mkfs.fat
export WIFL_PARTPROBE=$PWD/bundled-tools/partprobe
export WIFL_EFIBOOTMGR=$PWD/bundled-tools/efibootmgr
export WIFL_LSBLK=$PWD/bundled-tools/lsblk
export WIFL_FUSER=$PWD/bundled-tools/fuser

cargo build --release --target x86_64-unknown-linux-musl
```

---

## release cadence

Every push to `main` produces a release tagged `v{8-char commit hash}`. Binaries for all five targets are attached automatically.

---

## licence

MIT
