use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use anyhow::{bail, Context, Result};
use crate::tools::Tools;

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub name:  String,
    pub size:  String,
    pub model: String,
}

impl std::fmt::Display for DiskInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "/dev/{}  {}  {}", self.name, self.size, self.model)
    }
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub index: u32,
    pub name:  String,
}

impl std::fmt::Display for ImageInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]  {}", self.index, self.name)
    }
}

pub fn list_disks(tools: &Tools) -> Result<Vec<DiskInfo>> {
    let out = Command::new(&tools.lsblk)
        .args(["-d", "-o", "NAME,SIZE,MODEL", "-e", "7,11", "--noheadings"])
        .output()
        .context("lsblk")?;

    let mut disks = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut cols = line.splitn(3, char::is_whitespace).filter(|s| !s.is_empty());
        if let Some(name) = cols.next() {
            let size  = cols.next().unwrap_or("?").to_string();
            let model = cols.next().unwrap_or("").trim().to_string();
            disks.push(DiskInfo { name: name.to_string(), size, model });
        }
    }
    Ok(disks)
}

pub fn list_images(tools: &Tools, esd: &Path) -> Result<Vec<ImageInfo>> {
    let out = Command::new(&tools.wimlib)
        .args(["info", esd.to_str().unwrap()])
        .output()
        .context("wimlib-imagex info")?;

    let text = String::from_utf8_lossy(&out.stdout);
    let mut images = Vec::new();
    let mut cur_index: Option<u32>  = None;
    let mut cur_name:  Option<String> = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Index:") {
            if let Some(i) = cur_index.take() {
                if let Some(n) = cur_name.take() {
                    images.push(ImageInfo { index: i, name: n });
                }
            }
            cur_index = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("Name:") {
            cur_name = Some(rest.trim().to_string());
        }
    }
    if let (Some(i), Some(n)) = (cur_index, cur_name) {
        images.push(ImageInfo { index: i, name: n });
    }

    Ok(images
        .into_iter()
        .filter(|img| {
            !img.name.contains("Windows PE")
                && !img.name.contains("Windows Setup")
                && !img.name.contains("Setup Media")
        })
        .collect())
}

fn part(disk: &str, n: u8) -> String {
    if disk.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("/dev/{}p{}", disk, n)
    } else {
        format!("/dev/{}{}", disk, n)
    }
}

pub fn install(tools: &Tools, disk: &DiskInfo, esd: &Path, image_index: u32) -> Result<()> {
    let disk_dev = format!("/dev/{}", disk.name);
    let efi_part = part(&disk.name, 1);
    let msr_part = part(&disk.name, 2);
    let win_part = part(&disk.name, 3);
    let mnt_win  = "/mnt/windows";
    let mnt_efi  = "/mnt/winefi";

    // ── cleanup ───────────────────────────────────────────────────────────────
    step("clearing existing mounts");
    let _ = Command::new("umount").args(["-Rl", mnt_win]).output();
    let _ = Command::new("umount").args(["-Rl", mnt_efi]).output();
    for dev in [&disk_dev, &efi_part, &msr_part, &win_part] {
        let _ = Command::new(&tools.fuser).args(["-km", dev.as_str()]).output();
        let _ = Command::new("umount").args(["-Rl", dev.as_str()]).output();
        let _ = Command::new("swapoff").arg(dev).output();
    }

    // ── partition ─────────────────────────────────────────────────────────────
    step("partitioning disk  (GPT · UEFI)");
    run(&tools.sgdisk, &["--zap-all", &disk_dev])?;
    run(&tools.sgdisk, &[
        "-n", "1:0:+1G",  "-t", "1:ef00", "-c", "1:EFI System",
        "-n", "2:0:+16M", "-t", "2:0c01", "-c", "2:Microsoft Reserved",
        "-n", "3:0:0",    "-t", "3:0700", "-c", "3:Windows",
        &disk_dev,
    ])?;

    step("waiting for kernel to register partitions");
    run(&tools.partprobe, &[&disk_dev])?;
    let _ = Command::new("udevadm").args(["settle", "--timeout=15"]).status();

    for part_dev in [&efi_part, &win_part] {
        for _ in 0..15 {
            if Path::new(part_dev).exists() { break; }
            std::thread::sleep(Duration::from_secs(1));
        }
        if !Path::new(part_dev).exists() {
            bail!("device node never appeared: {}", part_dev);
        }
    }

    // ── format ────────────────────────────────────────────────────────────────
    step("formatting EFI → FAT32");
    run(&tools.mkfs_fat, &["-F32", "-n", "EFI", &efi_part])?;

    step("formatting Windows → NTFS");
    run(&tools.mkntfs, &["-f", "-L", "Windows", &win_part])?;
    let _ = Command::new("udevadm").args(["settle", "--timeout=10"]).status();

    // ── apply ─────────────────────────────────────────────────────────────────
    step(&format!(
        "applying image [{}] to {}  (10–20 min — do not interrupt)",
        image_index, win_part
    ));
    run_inherit(&tools.wimlib, &[
        "apply",
        esd.to_str().unwrap(),
        &image_index.to_string(),
        &win_part,
    ])?;

    // ── mount for boot file copy ───────────────────────────────────────────────
    step("mounting partitions");
    std::fs::create_dir_all(mnt_win)?;
    std::fs::create_dir_all(mnt_efi)?;

    let ntfs_ok = ["-t ntfs3", "-t ntfs-3g"].iter().any(|opts| {
        let mut cmd = Command::new("mount");
        for o in opts.split_whitespace() { cmd.arg(o); }
        cmd.arg(&win_part)
            .arg(mnt_win)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    });

    if !ntfs_ok {
        bail!("failed to mount {} as NTFS — check: dmesg | tail -20", win_part);
    }

    run_sys("mount", &["-t", "vfat", &efi_part, mnt_efi])?;

    // ── boot files ────────────────────────────────────────────────────────────
    step("staging EFI boot files");
    let boot_src    = PathBuf::from(mnt_win).join("Windows/Boot");
    let efi_ms_boot = PathBuf::from(mnt_efi).join("EFI/Microsoft/Boot");
    let efi_boot    = PathBuf::from(mnt_efi).join("EFI/Boot");

    if !boot_src.join("EFI").exists() {
        bail!("Windows/Boot/EFI not found in applied image");
    }

    std::fs::create_dir_all(&efi_ms_boot)?;
    std::fs::create_dir_all(&efi_boot)?;

    run_sys("cp", &[
        "-r",
        &format!("{}/.", boot_src.join("EFI").display()),
        efi_ms_boot.to_str().unwrap(),
    ])?;
    run_sys("cp", &[
        boot_src.join("EFI/bootmgfw.efi").to_str().unwrap(),
        efi_boot.join("bootx64.efi").to_str().unwrap(),
    ])?;

    let bcd = boot_src.join("BCD");
    if bcd.exists() {
        run_sys("cp", &[bcd.to_str().unwrap(), efi_ms_boot.join("BCD").to_str().unwrap()])?;
    }
    for dir in ["Fonts", "Resources"] {
        let src = boot_src.join(dir);
        if src.exists() {
            let _ = run_sys("cp", &["-r", src.to_str().unwrap(), efi_ms_boot.to_str().unwrap()]);
        }
    }

    // ── NVRAM ─────────────────────────────────────────────────────────────────
    step("registering UEFI boot entry");

    if let Ok(out) = Command::new(&tools.efibootmgr).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.to_lowercase().contains("windows boot manager") {
                if let Some(num) = line
                    .strip_prefix("Boot")
                    .and_then(|s| s.split(|c: char| !c.is_ascii_hexdigit()).next())
                {
                    let _ = Command::new(&tools.efibootmgr)
                        .args(["-b", num.trim(), "-B"])
                        .output();
                }
            }
        }
    }

    let ok = Command::new(&tools.efibootmgr)
        .args([
            "--create",
            "--disk", &disk_dev,
            "--part", "1",
            "--label", "Windows Boot Manager",
            "--loader", r"\EFI\Microsoft\Boot\bootmgfw.efi",
        ])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        println!("  \x1b[32m·\x1b[0m  boot entry created");
    } else {
        println!("  \x1b[33m·\x1b[0m  efibootmgr failed — add entry manually in firmware setup");
    }

    // ── unmount ───────────────────────────────────────────────────────────────
    step("flushing & unmounting");
    let _ = Command::new("sync").output();
    let _ = Command::new(&tools.fuser).args(["-km", mnt_win]).output();
    let _ = Command::new(&tools.fuser).args(["-km", mnt_efi]).output();
    std::thread::sleep(Duration::from_secs(1));
    let _ = Command::new("umount").args(["-R",  mnt_win]).output();
    let _ = Command::new("umount").args(["-Rl", mnt_win]).output();
    let _ = Command::new("umount").args(["-R",  mnt_efi]).output();
    let _ = Command::new("umount").args(["-Rl", mnt_efi]).output();

    Ok(())
}

fn step(msg: &str) {
    println!("\n  \x1b[32m·\x1b[0m  {}", msg);
}

fn run(prog: impl AsRef<OsStr>, args: &[&str]) -> Result<()> {
    let prog = prog.as_ref();
    let status = Command::new(prog)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("spawn {:?}", prog))?;
    if !status.success() {
        bail!("{:?} failed", prog);
    }
    Ok(())
}

fn run_inherit(prog: impl AsRef<OsStr>, args: &[&str]) -> Result<()> {
    let prog = prog.as_ref();
    let status = Command::new(prog)
        .args(args)
        .status()
        .with_context(|| format!("spawn {:?}", prog))?;
    if !status.success() {
        bail!("{:?} failed", prog);
    }
    Ok(())
}

// For system tools always in PATH (mount, cp, etc.)
fn run_sys(prog: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(prog)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("spawn {}", prog))?;
    if !status.success() {
        bail!("{} failed", prog);
    }
    Ok(())
}
