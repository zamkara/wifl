use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use crate::tui::Tui;

pub fn ensure_esd(url: &str, dest: &Path, expected_sha256: &str, size: u64, ui: &mut Tui) -> Result<()> {
    if dest.exists() {
        ui.step("verifying cached ESD…");
        if verify(dest, expected_sha256)? {
            ui.step_ok("checksum ok — skipping download");
            return Ok(());
        }
        ui.step("checksum mismatch — re-downloading");
        fs::remove_file(dest).context("remove stale ESD file")?;
    }

    ui.step(&format!("downloading  {:.2} GiB", size as f64 / 1_073_741_824.0));

    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()?;
    let mut resp = client.get(url).send().context("GET request")?;

    if !resp.status().is_success() {
        bail!("server returned {}", resp.status());
    }

    let mut file = fs::File::create(dest).context("create ESD file")?;
    let mut buf  = vec![0u8; 131_072];
    let mut done = 0u64;

    loop {
        let n = resp.read(&mut buf).context("read HTTP response")?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).context("write ESD file")?;
        done += n as u64;
        ui.progress(done, size);
    }
    ui.progress_done();

    ui.step("verifying sha256…");
    if !verify(dest, expected_sha256)? {
        fs::remove_file(dest).context("remove failed ESD")?;
        bail!("sha256 mismatch after download — file corrupted");
    }
    ui.step_ok("verified");

    Ok(())
}

fn verify(path: &Path, expected: &str) -> Result<bool> {
    let mut f   = fs::File::open(path)
        .with_context(|| format!("open for verify: {}", path.display()))?;
    let mut h   = Sha256::new();
    let mut buf = vec![0u8; 131_072];
    loop {
        let n = f.read(&mut buf).context("read during sha256")?;
        if n == 0 { break; }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()).eq_ignore_ascii_case(expected))
}
