use std::{io::Write, path::Path, process::Command};
use anyhow::{bail, Context, Result};

// Well-known BCD GUIDs
const BOOT_MGR_GUID:    &str = "9dea862c-5cdd-4e70-acc1-f32b344d4795";
const BOOT_LDR_GUID:    &str = "b012b84d-c47c-4ed5-b722-c0c42163e569";
const GLOBAL_SETTINGS:  &str = "6efb52bf-1766-41db-a6b3-0ee5eff72bd7";

/// ARM64 UEFI boot filename — uefi-ntfs loads this from the NTFS partition.
pub const EFI_BOOT_FILENAME: &str = "bootaa64.efi";

/// Parse a GUID string into Windows mixed-endian bytes (bytes_le):
/// first 3 components little-endian, last 2 big-endian.
fn parse_guid(s: &str) -> Result<[u8; 16]> {
    let s = s.trim_matches(|c| c == '{' || c == '}');
    let parts: Vec<&str> = s.splitn(5, '-').collect();
    if parts.len() != 5 {
        bail!("invalid GUID: {}", s);
    }
    let a = u32::from_str_radix(parts[0], 16)
        .with_context(|| format!("GUID part[0]: {}", parts[0]))?;
    let b = u16::from_str_radix(parts[1], 16)
        .with_context(|| format!("GUID part[1]: {}", parts[1]))?;
    let c = u16::from_str_radix(parts[2], 16)
        .with_context(|| format!("GUID part[2]: {}", parts[2]))?;
    let d = u16::from_str_radix(parts[3], 16)
        .with_context(|| format!("GUID part[3]: {}", parts[3]))?;
    let e_str = parts[4];
    if e_str.len() != 12 {
        bail!("GUID part[4] wrong length: {}", e_str);
    }
    let e_hi = u32::from_str_radix(&e_str[0..8], 16)?;
    let e_lo = u16::from_str_radix(&e_str[8..12], 16)?;

    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&a.to_le_bytes());
    buf[4..6].copy_from_slice(&b.to_le_bytes());
    buf[6..8].copy_from_slice(&c.to_le_bytes());
    buf[8..10].copy_from_slice(&d.to_be_bytes());
    buf[10..14].copy_from_slice(&e_hi.to_be_bytes());
    buf[14..16].copy_from_slice(&e_lo.to_be_bytes());
    Ok(buf)
}

/// Build the 88-byte GPT device element for a BCD entry.
/// Layout (from analysing real Windows BCDs):
///   [0..16)  : outer header (zeros)
///   [16..20) : device type = 6 (GPT partition)
///   [20..24) : flags = 0
///   [24..28) : inner struct size = 0x48
///   [28..32) : padding = 0
///   [32..48) : partition GUID (mixed-endian)
///   [48..64) : zeros
///   [64..80) : disk GUID (mixed-endian)
///   [80..88) : zeros
fn make_gpt_device(part_guid: &str, disk_guid: &str) -> Result<[u8; 88]> {
    let mut data = [0u8; 88];
    data[16..20].copy_from_slice(&6u32.to_le_bytes());
    data[24..28].copy_from_slice(&0x48u32.to_le_bytes());
    data[32..48].copy_from_slice(&parse_guid(part_guid)?);
    data[64..80].copy_from_slice(&parse_guid(disk_guid)?);
    Ok(data)
}

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(",")
}

fn utf16le_null(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    v.push(0); v.push(0);
    v
}

fn path_hex(s: &str) -> String {
    bytes_to_hex(&utf16le_null(s))
}

fn guid_hex(guid: &str) -> String {
    bytes_to_hex(&parse_guid(guid).unwrap())
}

/// REG_MULTI_SZ containing a single GUID string like {xxxxxxxx-...}
fn multi_sz_one_guid(guid: &str) -> String {
    let s = format!("{{{}}}", guid);
    let mut v = utf16le_null(&s);
    v.push(0); v.push(0); // list terminator (second null string)
    bytes_to_hex(&v)
}

/// Query blkid for the partition UUID (PARTUUID) of a partition device.
pub fn get_partuuid(part_dev: &str) -> Result<String> {
    let out = Command::new("blkid")
        .args(["-o", "value", "-s", "PARTUUID", part_dev])
        .output()
        .context("blkid PARTUUID")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        bail!("blkid returned no PARTUUID for {} — run partprobe first?", part_dev);
    }
    Ok(s)
}

/// Query blkid for the disk GUID (PTUUID) of the whole disk device.
pub fn get_diskuuid(disk_dev: &str) -> Result<String> {
    let out = Command::new("blkid")
        .args(["-o", "value", "-s", "PTUUID", disk_dev])
        .output()
        .context("blkid PTUUID")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        bail!("blkid returned no PTUUID for {}", disk_dev);
    }
    Ok(s)
}

/// Check that hivexregedit is available.
pub fn require_hivexregedit() -> Result<()> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    for seg in path_var.split(':') {
        if std::path::Path::new(seg).join("hivexregedit").exists() {
            return Ok(());
        }
    }
    bail!(
        "'hivexregedit' not found\n  \
         · install: pacman -S hivex  |  apt install libhivex-perl\n  \
         hivexregedit is required to create the Windows boot BCD"
    );
}

/// Create `\EFI\Microsoft\Boot\BCD` on the Windows NTFS partition.
///
/// - `template_src`: path to `Windows\System32\config\BCD-Template` on the
///   mounted Windows partition (used as the hive base).
/// - `bcd_dest`: destination path (e.g. `/mnt/windows/EFI/Microsoft/Boot/BCD`).
/// - `disk_dev`: host path of the whole disk (e.g. `/dev/sda`).
/// - `win_part_dev`: host path of the Windows NTFS partition (e.g. `/dev/sda3`).
pub fn create_bcd(
    template_src: &Path,
    bcd_dest: &Path,
    disk_dev: &str,
    win_part_dev: &str,
) -> Result<()> {
    require_hivexregedit()?;

    if !template_src.exists() {
        bail!(
            "BCD-Template not found at {}\n  \
             The applied Windows image may be incomplete.",
            template_src.display()
        );
    }

    let disk_guid    = get_diskuuid(disk_dev)?;
    let win_part_guid = get_partuuid(win_part_dev)?;

    let device = make_gpt_device(&win_part_guid, &disk_guid)?;
    let dev_hex = bytes_to_hex(&device);

    let ldr_guid_bytes = guid_hex(BOOT_LDR_GUID);
    let global_inherit = multi_sz_one_guid(GLOBAL_SETTINGS);

    // Generate the .reg file that hivexregedit will merge into the BCD-Template.
    // Notes:
    //  - Boot Manager (9dea862c-...) type 0x10100002
    //    · 11000001: device (where bootmgfw.efi lives — same NTFS partition)
    //    · 23000003: default object (GUID of boot loader, 16 bytes binary)
    //    · 24000001: display order (same GUID, 16 bytes binary)
    //    · 25000004: timeout in seconds (QWORD = 30s)
    //    · 14000006 is NOT written here — template's inherit list is preserved
    //
    //  - Windows Boot Loader (b012b84d-...) type 0x10200003
    //    · 11000001: device (NTFS partition)
    //    · 12000002: path to winload.efi (UTF-16LE binary)
    //    · 12000004: description "Windows 11" (UTF-16LE binary)
    //    · 14000006: inherit list → globalsettings GUID (REG_MULTI_SZ)
    //    · 21000001: OS device (same NTFS partition)
    //    · 22000002: system root \Windows (UTF-16LE binary)
    let reg = format!(
        "Windows Registry Editor Version 5.00\r\n\
        \r\n\
        [Objects\\{{{BM}}}\\Description]\r\n\
        \"Type\"=dword:10100002\r\n\
        \r\n\
        [Objects\\{{{BM}}}\\Elements\\11000001]\r\n\
        \"Element\"=hex:{dev}\r\n\
        \r\n\
        [Objects\\{{{BM}}}\\Elements\\23000003]\r\n\
        \"Element\"=hex:{ldr}\r\n\
        \r\n\
        [Objects\\{{{BM}}}\\Elements\\24000001]\r\n\
        \"Element\"=hex:{ldr}\r\n\
        \r\n\
        [Objects\\{{{BM}}}\\Elements\\25000004]\r\n\
        \"Element\"=hex(b):1e,00,00,00,00,00,00,00\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Description]\r\n\
        \"Type\"=dword:10200003\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\11000001]\r\n\
        \"Element\"=hex:{dev}\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\12000002]\r\n\
        \"Element\"=hex:{winload}\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\12000004]\r\n\
        \"Element\"=hex:{desc}\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\14000006]\r\n\
        \"Element\"=hex(7):{inherit}\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\21000001]\r\n\
        \"Element\"=hex:{dev}\r\n\
        \r\n\
        [Objects\\{{{BL}}}\\Elements\\22000002]\r\n\
        \"Element\"=hex:{sysroot}\r\n\
        ",
        BM      = BOOT_MGR_GUID,
        BL      = BOOT_LDR_GUID,
        dev     = dev_hex,
        ldr     = ldr_guid_bytes,
        winload = path_hex(r"\Windows\System32\winload.efi"),
        desc    = path_hex("Windows 11"),
        inherit = global_inherit,
        sysroot = path_hex(r"\Windows"),
    );

    // Copy BCD-Template → destination (this is the hive we will merge into)
    std::fs::copy(template_src, bcd_dest)
        .with_context(|| format!("copy BCD-Template to {}", bcd_dest.display()))?;

    // Write .reg to a temp file next to the BCD
    let reg_path = bcd_dest.with_extension("reg");
    {
        let mut f = std::fs::File::create(&reg_path)
            .context("create BCD .reg temp file")?;
        f.write_all(reg.as_bytes())?;
    }

    // Merge .reg into BCD hive
    let status = Command::new("hivexregedit")
        .args([
            "--merge",
            bcd_dest.to_str().unwrap(),
            "--encoding", "UTF-8",
            reg_path.to_str().unwrap(),
        ])
        .status()
        .context("hivexregedit")?;

    let _ = std::fs::remove_file(&reg_path);

    if !status.success() {
        bail!("hivexregedit failed — check the .reg content / hive compatibility");
    }

    Ok(())
}
