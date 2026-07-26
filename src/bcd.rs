use std::{io::Write, path::Path, process::Command};
use anyhow::{bail, Context, Result};


/// Return the UEFI default boot filename for the given architecture.
/// uefi-ntfs looks for this file on the NTFS partition.
pub fn efi_boot_filename(arch: &str) -> &'static str {
    match arch {
        "arm64" => "bootaa64.efi",
        "x86"   => "bootia32.efi",
        _       => "bootx64.efi",
    }
}

/// Parse a GUID string into Windows mixed-endian bytes (bytes_le).
fn parse_guid(s: &str) -> Result<[u8; 16]> {
    let s = s.trim_matches(|c| c == '{' || c == '}');
    let parts: Vec<&str> = s.splitn(5, '-').collect();
    if parts.len() != 5 { bail!("invalid GUID: {}", s); }
    let a = u32::from_str_radix(parts[0], 16)?;
    let b = u16::from_str_radix(parts[1], 16)?;
    let c = u16::from_str_radix(parts[2], 16)?;
    let d = u16::from_str_radix(parts[3], 16)?;
    let e_str = parts[4];
    if e_str.len() != 12 { bail!("GUID part[4] wrong length: {}", e_str); }
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

fn make_gpt_device(part_guid: &str, disk_guid: &str) -> Result<[u8; 88]> {
    let mut data = [0u8; 88];
    data[16..20].copy_from_slice(&6u32.to_le_bytes());
    data[24..28].copy_from_slice(&0x48u32.to_le_bytes());
    data[32..48].copy_from_slice(&parse_guid(part_guid)?);
    data[64..80].copy_from_slice(&parse_guid(disk_guid)?);
    Ok(data)
}

pub fn get_partuuid(part_dev: &str) -> Result<String> {
    let out = Command::new("blkid")
        .args(["-o", "value", "-s", "PARTUUID", part_dev])
        .output().context("blkid PARTUUID")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { bail!("blkid returned no PARTUUID for {}", part_dev); }
    Ok(s)
}

pub fn get_diskuuid(disk_dev: &str) -> Result<String> {
    let out = Command::new("blkid")
        .args(["-o", "value", "-s", "PTUUID", disk_dev])
        .output().context("blkid PTUUID")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { bail!("blkid returned no PTUUID for {}", disk_dev); }
    Ok(s)
}

pub fn require_hivex_perl() -> Result<()> {
    let out = Command::new("perl")
        .args(["-e", "use Win::Hivex; print 1"])
        .output();
    match out {
        Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "1" => Ok(()),
        _ => bail!(
            "Perl module Win::Hivex not found\n  \
             · install: pacman -S hivex  |  apt install libhivex-perl\n  \
             Win::Hivex is required to create the Windows boot BCD"
        ),
    }
}

// Perl script that creates BCD objects using Win::Hivex API directly.
// Called as: perl <script> <bcd_path> <part_guid> <disk_guid>
const BCD_PERL: &str = r#"
use strict; use warnings;
use Win::Hivex;

sub guid_bytes {
    my ($g) = @_; $g =~ s/[{}]//g;
    my @p = split /-/, $g;
    my @b;
    push @b, reverse(map { chr(hex) } ($p[0]=~/../g));
    push @b, reverse(map { chr(hex) } ($p[1]=~/../g));
    push @b, reverse(map { chr(hex) } ($p[2]=~/../g));
    push @b,         map { chr(hex) } ($p[3]=~/../g);
    push @b,         map { chr(hex) } ($p[4]=~/../g);
    join('', @b);
}

sub u16 { join('', map { chr(ord($_))."\x00" } split(//, $_[0]))."\x00\x00" }
sub msz { u16("{$_[0]}")."\x00\x00" }
sub dev {
    my ($pg, $dg) = @_;
    my $d = "\x00"x88;
    substr($d,16,4)=pack('V',6); substr($d,24,4)=pack('V',0x48);
    substr($d,32,16)=guid_bytes($pg); substr($d,64,16)=guid_bytes($dg);
    $d
}

my ($bcd, $pg, $dg) = @ARGV;
my $BM = '9dea862c-5cdd-4e70-acc1-f32b344d4795';
my $BL = 'b012b84d-c47c-4ed5-b722-c0c42163e569';
my $GS = '6efb52bf-1766-41db-a6b3-0ee5eff72bd7';

my $h = Win::Hivex->open($bcd, write=>1) or die "open: $!\n";
my $root = $h->root; my $objs = $h->node_get_child($root,'Objects');
die "Objects key not found\n" unless $objs;

my $dev = dev($pg,$dg); my $ldr_b = guid_bytes($BL);

sub ae { my($p,$n,$t,$v)=@_; my $k=$h->node_add_child($p,$n); $h->node_set_value($k,{key=>'Element',t=>$t,value=>$v}); }

# Boot Manager
my $bm=$h->node_add_child($objs,"{$BM}");
{ my $d=$h->node_add_child($bm,'Description'); $h->node_set_value($d,{key=>'Type',t=>4,value=>pack('V',0x10100002)}); }
my $be=$h->node_add_child($bm,'Elements');
ae($be,'11000001',3,$dev);
ae($be,'14000006',7,msz($GS));
ae($be,'23000003',3,$ldr_b);
ae($be,'24000001',7,msz($BL));
ae($be,'25000004',11,pack('VV',30,0));

# Boot Loader
my $bl=$h->node_add_child($objs,"{$BL}");
{ my $d=$h->node_add_child($bl,'Description'); $h->node_set_value($d,{key=>'Type',t=>4,value=>pack('V',0x10200003)}); }
my $le=$h->node_add_child($bl,'Elements');
ae($le,'11000001',3,$dev);
ae($le,'12000002',1,u16('\\Windows\\System32\\winload.efi'));
ae($le,'12000004',1,u16('Windows 11'));
ae($le,'14000006',7,msz($GS));
ae($le,'21000001',3,$dev);
ae($le,'22000002',1,u16('\\Windows'));

$h->commit(undef);
print "ok\n";
"#;

pub fn create_bcd(
    template_src: &Path,
    bcd_dest: &Path,
    disk_dev: &str,
    win_part_dev: &str,
) -> Result<()> {
    require_hivex_perl()?;

    if !template_src.exists() {
        bail!("BCD-Template not found at {}", template_src.display());
    }

    let disk_guid    = get_diskuuid(disk_dev)?;
    let win_part_guid = get_partuuid(win_part_dev)?;

    // Verify device element parses correctly
    make_gpt_device(&win_part_guid, &disk_guid)?;

    std::fs::copy(template_src, bcd_dest)
        .with_context(|| format!("copy BCD-Template to {}", bcd_dest.display()))?;

    let script_path = std::env::temp_dir()
        .join(format!("wifl-bcd-{}.pl", std::process::id()));
    {
        let mut f = std::fs::File::create(&script_path)
            .context("create BCD perl script")?;
        f.write_all(BCD_PERL.as_bytes())?;
    }

    let out = Command::new("perl")
        .args([
            script_path.to_str().unwrap(),
            bcd_dest.to_str().unwrap(),
            &win_part_guid,
            &disk_guid,
        ])
        .output()
        .context("run BCD perl script")?;

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    if !out.status.success() || !stdout.trim().ends_with("ok") {
        bail!(
            "BCD creation failed\nstdout: {}\nstderr: {}",
            stdout.trim(), stderr.trim()
        );
    }

    Ok(())
}
