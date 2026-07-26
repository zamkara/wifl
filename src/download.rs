use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

pub fn ensure_esd(url: &str, dest: &Path, expected_sha256: &str, size: u64) -> Result<()> {
    if dest.exists() {
        step("file present — verifying sha256…");
        if verify(dest, expected_sha256)? {
            step("checksum ok");
            return Ok(());
        }
        step("checksum mismatch — re-downloading");
        fs::remove_file(dest).context("remove stale ESD file")?;
    }

    step(&format!("downloading  {:.2} GiB", size as f64 / 1_073_741_824.0));

    let client   = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()?;
    let mut resp = client.get(url).send().context("GET request")?;

    if !resp.status().is_success() {
        bail!("server returned {}", resp.status());
    }

    let pb = ProgressBar::new(size);
    pb.set_style(
        ProgressStyle::with_template(
            "  {bar:44.cyan/238}  {bytes:>10} / {total_bytes}  eta {eta}",
        )?
        .progress_chars("█▓░"),
    );

    let mut file = fs::File::create(dest).context("create file")?;
    let mut buf  = vec![0u8; 131_072];
    let mut done = 0u64;

    loop {
        let n = resp.read(&mut buf).context("read HTTP response")?;
        if n == 0 { break; }
        file.write_all(&buf[..n]).context("write ESD file")?;
        done += n as u64;
        pb.set_position(done);
    }

    pb.finish_and_clear();

    step("verifying sha256…");
    if !verify(dest, expected_sha256)? {
        fs::remove_file(dest)?;
        bail!("sha256 mismatch after download");
    }
    step("verified");

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

fn step(msg: &str) {
    println!("  \x1b[32m·\x1b[0m  {}", msg);
}
