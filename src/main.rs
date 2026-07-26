mod api;
mod bcd;
mod catalog;
mod download;
mod install;
mod select;
mod tools;
mod update;

use std::path::PathBuf;
use anyhow::{bail, Context, Result};
use catalog::EsdFile;


fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("update") => {
            check_root()?;
            return update::run();
        }
        Some("--version") | Some("-v") | Some("version") => {
            println!("wifl {}", update::current_tag());
            return Ok(());
        }
        Some(other) => bail!("unknown command: {}  (try: update, --version)", other),
        None => {}
    }

    check_root()?;

    println!();
    println!("  \x1b[1mwifl\x1b[0m  windows image fetch + install  \x1b[90m{}\x1b[0m", update::current_tag());
    println!();

    info("initialising bundled tools…");
    let t = tools::Tools::setup()?;

    // ── 1. Windows version ─────────────────────────────────────────────────────
    info("fetching available releases…");
    let versions = api::fetch_versions()?;
    if versions.is_empty() { bail!("no versions returned from server"); }

    let ver_labels: Vec<String> = versions.iter()
        .map(|v| format!("Windows {}", v.number))
        .collect();
    let ver_idx = select::select("select Windows version", &ver_labels)?;
    let version = &versions[ver_idx];

    // ── 2. Build ───────────────────────────────────────────────────────────────
    if version.releases.is_empty() {
        bail!("no releases for Windows {}", version.number);
    }
    let build_labels: Vec<String> = version.releases.iter()
        .map(|r| format!("build {}   ({})", r.build, fmt_date(&r.date)))
        .collect();
    let build_idx = select::select("select build", &build_labels)?;
    let build = &version.releases[build_idx].build;

    // ── 3. Fetch catalog ───────────────────────────────────────────────────────
    info(&format!("fetching catalog for build {}…", build));
    let catalog = api::fetch_catalog(build)?;
    if catalog.is_empty() { bail!("catalog is empty for build {}", build); }

    // ── 4. Architecture ────────────────────────────────────────────────────────
    let mut arches: Vec<String> = catalog.iter()
        .map(|f| f.architecture.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    arches.sort();
    let arch_idx = select::select("select architecture", &arches)?;
    let arch = &arches[arch_idx];

    // ── 5. Language ────────────────────────────────────────────────────────────
    let mut seen = std::collections::HashSet::new();
    let mut lang_files: Vec<&EsdFile> = catalog.iter()
        .filter(|f| &f.architecture == arch)
        .filter(|f| seen.insert(f.language_code.clone()))
        .collect();
    lang_files.sort_by(|a, b| a.language.cmp(&b.language));
    let lang_labels: Vec<String> = lang_files.iter()
        .map(|f| format!("{}  ({})", f.language, f.language_code))
        .collect();
    let lang_idx  = select::select("select language", &lang_labels)?;
    let lang_code = &lang_files[lang_idx].language_code;

    // ── 6. ESD variant ─────────────────────────────────────────────────────────
    let candidates: Vec<&EsdFile> = catalog.iter()
        .filter(|f| &f.architecture == arch && &f.language_code == lang_code)
        .collect();
    let esd_file: &EsdFile = if candidates.len() == 1 {
        candidates[0]
    } else {
        let labels: Vec<String> = candidates.iter()
            .map(|f| format!("{}   {:.2} GiB", f.edition_label(), f.size_gb()))
            .collect();
        let idx = select::select("select edition group", &labels)?;
        candidates[idx]
    };

    // ── 7. Disk ────────────────────────────────────────────────────────────────
    let disks = install::list_disks(&t)?;
    if disks.is_empty() { bail!("no block devices found"); }
    let disk_labels: Vec<String> = disks.iter().map(|d| d.to_string()).collect();
    let disk_idx = select::select("select destination disk", &disk_labels)?;
    let disk = &disks[disk_idx];

    // ── 8. Confirm ─────────────────────────────────────────────────────────────
    println!();
    println!("  \x1b[33m·\x1b[0m  all data on /dev/{} will be destroyed", disk.name);
    println!();
    let confirm = vec!["yes — proceed".to_string(), "no  — abort".to_string()];
    if select::select("confirm?", &confirm)? != 0 {
        bail!("aborted");
    }

    // ── 9. Download ESD ────────────────────────────────────────────────────────
    println!();
    let esd_dir = esd_dir()?;
    info(&format!("ESD directory: {}", esd_dir.display()));
    let esd_path = esd_dir.join(&esd_file.filename);
    std::fs::create_dir_all(&esd_dir)
        .with_context(|| format!("create ESD directory {}", esd_dir.display()))?;
    download::ensure_esd(&esd_file.url, &esd_path, &esd_file.sha256, esd_file.size)
        .with_context(|| format!("download/verify {}", esd_path.display()))?;

    // ── 10. Select image index ─────────────────────────────────────────────────
    println!();
    info("reading image catalogue from ESD…");
    let images = install::list_images(&t, &esd_path)
        .with_context(|| format!("list images in {}", esd_path.display()))?;
    if images.is_empty() { bail!("no installable images found in ESD"); }
    let img_labels: Vec<String> = images.iter().map(|i| i.to_string()).collect();
    let img_idx = select::select("select edition to install", &img_labels)?;
    let image   = &images[img_idx];

    // ── 11. Install ────────────────────────────────────────────────────────────
    println!();
    install::install(&t, disk, &esd_path, image.index)
        .with_context(|| format!("install to /dev/{}", disk.name))?;

    println!();
    println!("  \x1b[1mdone\x1b[0m  reboot to continue Windows setup");
    println!();
    println!("  \x1b[33m·\x1b[0m  first boot may trigger Startup Repair — expected, let it complete");
    println!();
    Ok(())
}

fn esd_dir() -> Result<PathBuf> {
    // When running via `sudo`, HOME is often reset to /root but the ESD
    // should live in the invoking user's home directory.  Read SUDO_USER
    // and look up their home in /etc/passwd before falling back to HOME.
    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        if !sudo_user.is_empty() && sudo_user != "root" {
            if let Ok(home) = passwd_home(&sudo_user) {
                return Ok(PathBuf::from(home).join("Downloads").join("wifl"));
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    Ok(PathBuf::from(home).join("Downloads").join("wifl"))
}

fn passwd_home(username: &str) -> Result<String> {
    let passwd = std::fs::read_to_string("/etc/passwd")
        .context("read /etc/passwd")?;
    for line in passwd.lines() {
        let mut f = line.splitn(7, ':');
        let name = f.next().unwrap_or("");
        if name == username {
            // field order: name:password:uid:gid:gecos:home:shell
            let home = f.nth(4).unwrap_or("").to_string();
            if !home.is_empty() {
                return Ok(home);
            }
        }
    }
    bail!("user '{}' not found in /etc/passwd", username)
}

fn check_root() -> Result<()> {
    let out = std::process::Command::new("id").arg("-u").output()?;
    if String::from_utf8_lossy(&out.stdout).trim() != "0" {
        bail!("root required — run: sudo wifl");
    }
    Ok(())
}

fn info(msg: &str) {
    println!("  \x1b[32m·\x1b[0m  {}", msg);
}

fn fmt_date(d: &str) -> String {
    if d.len() == 8 {
        format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}
