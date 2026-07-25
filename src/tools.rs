use std::{fs, path::{Path, PathBuf}};
use anyhow::{bail, Context, Result};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static WIMLIB_IMAGEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wimlib_imagex"));
static SGDISK:        &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sgdisk"));
static MKNTFS:        &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mkntfs"));
static MKFS_FAT:      &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mkfs_fat"));
static PARTPROBE:     &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/partprobe"));
static EFIBOOTMGR:    &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/efibootmgr"));
static LSBLK:         &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lsblk"));
static FUSER:         &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fuser"));

pub struct Tools {
    _dir:           PathBuf,
    pub wimlib:     PathBuf,
    pub sgdisk:     PathBuf,
    pub mkntfs:     PathBuf,
    pub mkfs_fat:   PathBuf,
    pub partprobe:  PathBuf,
    pub efibootmgr: PathBuf,
    pub lsblk:      PathBuf,
    pub fuser:      PathBuf,
}

impl Drop for Tools {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self._dir);
    }
}

impl Tools {
    pub fn setup() -> Result<Self> {
        let dir = std::env::temp_dir()
            .join(format!("wifl-{}", std::process::id()));
        fs::create_dir_all(&dir).context("create tool dir")?;

        Ok(Self {
            wimlib:     slot(&dir, "wimlib-imagex", WIMLIB_IMAGEX,
                "wimlib: pacman -S wimlib  |  apt install wimlib  |  brew install wimlib")?,
            sgdisk:     slot(&dir, "sgdisk",        SGDISK,
                "gptfdisk: pacman -S gptfdisk  |  apt install gdisk  |  brew install gptfdisk")?,
            mkntfs:     slot(&dir, "mkntfs",        MKNTFS,
                "ntfsprogs: pacman -S ntfs-3g  |  apt install ntfs-3g")?,
            mkfs_fat:   slot(&dir, "mkfs.fat",      MKFS_FAT,
                "dosfstools: pacman -S dosfstools  |  apt install dosfstools")?,
            partprobe:  slot(&dir, "partprobe",     PARTPROBE,
                "parted: pacman -S parted  |  apt install parted")?,
            efibootmgr: slot(&dir, "efibootmgr",   EFIBOOTMGR,
                "efibootmgr: pacman -S efibootmgr  |  apt install efibootmgr")?,
            lsblk:      slot(&dir, "lsblk",         LSBLK,
                "util-linux: pacman -S util-linux  |  apt install util-linux")?,
            fuser:      slot(&dir, "fuser",          FUSER,
                "psmisc: pacman -S psmisc  |  apt install psmisc")?,
            _dir: dir,
        })
    }
}

fn slot(dir: &Path, name: &str, data: &[u8], hint: &str) -> Result<PathBuf> {
    if !data.is_empty() {
        let p = dir.join(name);
        fs::write(&p, data).with_context(|| format!("extract {}", name))?;
        #[cfg(unix)]
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod {}", name))?;
        return Ok(p);
    }
    which_or_bail(name, hint)
}

fn which_or_bail(name: &str, hint: &str) -> Result<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for segment in path_var.split(':') {
        let p = PathBuf::from(segment).join(name);
        if p.exists() {
            return Ok(p);
        }
    }
    bail!("'{}' not found\n  · install: {}", name, hint);
}
