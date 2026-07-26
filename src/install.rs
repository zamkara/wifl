use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use anyhow::{bail, Context, Result};
use crate::{bcd, tools::Tools, tui::Tui};

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

pub fn install(tools: &Tools, disk: &DiskInfo, esd: &Path, image_index: u32, ui: &Tui) -> Result<()> {
    let disk_dev      = format!("/dev/{}", disk.name);
    let uefi_ntfs_part = part(&disk.name, 1);  // tiny FAT12 — uefi-ntfs bridge
    let msr_part       = part(&disk.name, 2);  // Microsoft Reserved
    let win_part       = part(&disk.name, 3);  // Windows NTFS (boot files + OS)
    let mnt_win        = "/mnt/windows";

    // Check requirements early so we don't waste time if something is missing
    bcd::require_hivexregedit()?;

    let uefi_ntfs_img = Tools::uefi_ntfs_img();
    if uefi_ntfs_img.is_empty() {
        bail!(
            "uefi-ntfs.img not bundled in this build.\n  \
             Rebuild with: WIFL_UEFI_NTFS=/path/to/uefi-ntfs.img cargo build\n  \
             Get the image from: https://github.com/pbatard/uefi-ntfs/releases"
        );
    }

    // ── cleanup ───────────────────────────────────────────────────────────────
    ui.step("clearing existing mounts");
    let _ = Command::new("umount").args(["-Rl", mnt_win]).output();
    for dev in [&disk_dev, &uefi_ntfs_part, &msr_part, &win_part] {
        let _ = Command::new(&tools.fuser).args(["-km", dev.as_str()]).output();
        let _ = Command::new("umount").args(["-Rl", dev.as_str()]).output();
        let _ = Command::new("swapoff").arg(dev).output();
    }

    // ── partition ─────────────────────────────────────────────────────────────
    // Layout:
    //   sda1  2 MiB  EFI System — uefi-ntfs.img written directly (FAT12)
    //   sda2  16 MiB Microsoft Reserved
    //   sda3  rest   Windows NTFS — OS + boot files + BCD all on one partition
    //
    // Boot chain: UEFI → sda1 EFI/Boot/bootaa64.efi (uefi-ntfs) →
    //             NTFS driver → sda3 EFI/Boot/bootaa64.efi (bootmgfw.efi) →
    //             BCD → winload.efi → Windows
    ui.step("partitioning disk  (GPT · UEFI:NTFS layout)");
    {
        use std::io::Write as _;
        let mut child = Command::new(&tools.sfdisk)
            .args(["--label", "gpt", "--wipe", "always", "--wipe-partitions", "always", &disk_dev])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("spawn sfdisk")?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(
                b"name=\"UEFI:NTFS\",          size=2MiB,  type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n\
                  name=\"Microsoft Reserved\",  size=16MiB, type=E3C9E316-0B5C-4DB8-817D-F92DF00215AE\n\
                  name=\"Windows\",                         type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7\n",
            )?;
        }
        if !child.wait().context("sfdisk wait")?.success() {
            bail!("sfdisk failed");
        }
    }

    ui.step("waiting for kernel to register partitions");
    run(&tools.partprobe, &[&disk_dev])?;
    let _ = Command::new("udevadm").args(["settle", "--timeout=15"]).status();

    for part_dev in [&uefi_ntfs_part, &win_part] {
        for _ in 0..15 {
            if Path::new(part_dev).exists() { break; }
            std::thread::sleep(Duration::from_secs(1));
        }
        if !Path::new(part_dev).exists() {
            bail!("device node never appeared: {}", part_dev);
        }
    }

    // ── write uefi-ntfs.img to sda1 ──────────────────────────────────────────
    ui.step("writing uefi-ntfs boot bridge to EFI partition");
    {
        let tmp_img = std::env::temp_dir()
            .join(format!("wifl-uefi-ntfs-{}.img", std::process::id()));
        std::fs::write(&tmp_img, uefi_ntfs_img)
            .context("write uefi-ntfs.img to temp")?;

        let status = Command::new("dd")
            .args([
                &format!("if={}", tmp_img.display()),
                &format!("of={}", uefi_ntfs_part),
                "bs=4M",
                "conv=fdatasync",
                "status=none",
            ])
            .status()
            .context("dd uefi-ntfs.img")?;
        let _ = std::fs::remove_file(&tmp_img);
        if !status.success() {
            bail!("dd uefi-ntfs.img failed");
        }
    }

    // ── format Windows NTFS ───────────────────────────────────────────────────
    ui.step("formatting Windows → NTFS");
    run(&tools.mkntfs, &["-f", "-L", "Windows", &win_part])?;
    let _ = Command::new("udevadm").args(["settle", "--timeout=10"]).status();

    // ── apply Windows image ───────────────────────────────────────────────────
    ui.step(&format!(
        "applying image [{}] to {}  (10–20 min — do not interrupt)",
        image_index, win_part
    ));
    run_inherit(&tools.wimlib, &[
        "apply",
        esd.to_str().unwrap(),
        &image_index.to_string(),
        &win_part,
    ])?;

    // ── mount Windows NTFS ────────────────────────────────────────────────────
    ui.step("mounting Windows partition");
    std::fs::create_dir_all(mnt_win)
        .with_context(|| format!("create mount point {}", mnt_win))?;

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

    // ── set up EFI directory structure on NTFS ────────────────────────────────
    // Both EFI\Boot\bootaa64.efi (entry point for uefi-ntfs) and
    // EFI\Microsoft\Boot\bootmgfw.efi live on the same NTFS partition.
    // This eliminates the need for a separate FAT32 EFI partition entirely.
    ui.step("staging EFI boot files on NTFS partition");
    let boot_src    = PathBuf::from(mnt_win).join("Windows/Boot");
    let efi_boot    = PathBuf::from(mnt_win).join("EFI/Boot");
    let efi_ms_boot = PathBuf::from(mnt_win).join("EFI/Microsoft/Boot");

    if !boot_src.join("EFI").exists() {
        bail!("Windows/Boot/EFI not found in applied image — image may be incomplete");
    }

    // If the applied image left a non-directory at \EFI (rare but possible),
    // remove it so create_dir_all does not hit ENOTDIR.
    let efi_root = PathBuf::from(mnt_win).join("EFI");
    if efi_root.exists() && !efi_root.is_dir() {
        std::fs::remove_file(&efi_root)
            .with_context(|| format!("remove stale file at {}", efi_root.display()))?;
    }

    std::fs::create_dir_all(&efi_boot)
        .with_context(|| format!("create {}", efi_boot.display()))?;
    std::fs::create_dir_all(&efi_ms_boot)
        .with_context(|| format!("create {}", efi_ms_boot.display()))?;

    let bootmgfw = boot_src.join("EFI/bootmgfw.efi");
    if !bootmgfw.exists() {
        bail!("bootmgfw.efi not found in Windows/Boot/EFI/");
    }

    // EFI\Boot\bootaa64.efi — uefi-ntfs chain-loads this from the NTFS partition
    run_sys("cp", &[
        bootmgfw.to_str().unwrap(),
        efi_boot.join(bcd::EFI_BOOT_FILENAME).to_str().unwrap(),
    ])?;

    // EFI\Microsoft\Boot\bootmgfw.efi — canonical Windows Boot Manager location
    run_sys("cp", &[
        bootmgfw.to_str().unwrap(),
        efi_ms_boot.join("bootmgfw.efi").to_str().unwrap(),
    ])?;

    // Fonts and Resources (needed for boot menu display)
    for dir in ["Fonts", "Resources"] {
        let src = boot_src.join(dir);
        if src.exists() {
            let _ = run_sys("cp", &["-r", src.to_str().unwrap(), efi_ms_boot.to_str().unwrap()]);
        }
    }

    // ── create BCD on NTFS ────────────────────────────────────────────────────
    // BCD lives on the same NTFS partition as Windows.
    // All device elements point to sda3 (one partition GUID for everything).
    ui.step("creating Windows Boot Configuration Data (BCD)");
    let template = PathBuf::from(mnt_win).join("Windows/System32/config/BCD-Template");
    let bcd_dest  = efi_ms_boot.join("BCD");
    bcd::create_bcd(&template, &bcd_dest, &disk_dev, &win_part)?;

    // ── NVRAM boot entry ──────────────────────────────────────────────────────
    // Register sda1 (the uefi-ntfs FAT12 partition) as the UEFI boot entry.
    // UEFI will load EFI\Boot\bootaa64.efi from sda1, which is the uefi-ntfs
    // bridge that then loads bootmgfw.efi from the NTFS partition.
    ui.step("registering UEFI boot entry");

    if let Ok(out) = Command::new(&tools.efibootmgr).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.to_lowercase().contains("windows") {
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
            "--label", "Windows (UEFI:NTFS)",
            "--loader", r"\EFI\Boot\bootaa64.efi",
        ])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if ok {
        ui.step_ok("boot entry created");
    } else {
        ui.info(&format!(
            "efibootmgr failed — UEFI should auto-discover EFI\\Boot\\{}",
            bcd::EFI_BOOT_FILENAME
        ));
    }

    // ── flush & unmount ───────────────────────────────────────────────────────
    ui.step("flushing & unmounting");
    let _ = Command::new("sync").output();
    let _ = Command::new(&tools.fuser).args(["-km", mnt_win]).output();
    std::thread::sleep(Duration::from_secs(1));
    let _ = Command::new("umount").args(["-R",  mnt_win]).output();
    let _ = Command::new("umount").args(["-Rl", mnt_win]).output();
    ui.step_ok("done");

    Ok(())
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
