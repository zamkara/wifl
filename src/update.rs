use std::io::{Read, Write};
use anyhow::{bail, Context, Result};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const CURRENT_TAG: &str = env!("WIFL_BUILD_TAG");
const CURRENT_TARGET: &str = env!("WIFL_TARGET");
const API: &str = "https://api.github.com/repos/zamkara/wifl/releases/latest";

pub fn current_tag() -> &'static str { CURRENT_TAG }

pub fn run() -> Result<()> {
    println!();
    println!("  · current   {}", CURRENT_TAG);

    if CURRENT_TAG == "dev" {
        println!("  \x1b[33m·\x1b[0m  dev build — update not available");
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent("wifl-updater/1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let body = client
        .get(API)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .context("fetch latest release")?
        .text()
        .context("read release response")?;

    let latest_tag = json_str(&body, "tag_name")
        .context("parse tag_name from GitHub response")?;

    println!("  · latest    {}", latest_tag);

    if latest_tag == CURRENT_TAG {
        println!("  \x1b[32m·\x1b[0m  already up to date");
        return Ok(());
    }

    let asset_name = format!("wifl-{}", CURRENT_TARGET);
    let url = asset_url(&body, &asset_name)
        .with_context(|| format!("asset '{}' not found in release {}", asset_name, latest_tag))?;

    println!("  \x1b[32m·\x1b[0m  downloading {}…", latest_tag);

    let current_exe = std::env::current_exe().context("resolve current executable")?;
    let tmp = current_exe.with_extension("_wifl_update");

    let mut resp = client.get(&url).send().context("download binary")?;
    if !resp.status().is_success() {
        bail!("download returned {}", resp.status());
    }

    {
        let mut f = std::fs::File::create(&tmp).context("create temp file")?;
        let mut buf = vec![0u8; 131_072];
        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 { break; }
            f.write_all(&buf[..n])?;
        }
    }

    #[cfg(unix)]
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;

    std::fs::rename(&tmp, &current_exe).context("replace binary")?;

    println!("  \x1b[32m·\x1b[0m  updated to {}  —  restart to apply", latest_tag);
    println!();
    Ok(())
}

fn json_str(json: &str, key: &str) -> Option<String> {
    let marker = format!("\"{}\":", key);
    let start  = json.find(&marker)? + marker.len();
    let rest   = json[start..].trim_start();
    if !rest.starts_with('"') { return None; }
    let inner = &rest[1..];
    let end   = inner.find('"')?;
    Some(inner[..end].to_string())
}

fn asset_url(json: &str, name: &str) -> Option<String> {
    let marker = format!("\"name\":\"{}\"", name);
    let pos    = json.find(&marker)?;
    json_str(&json[pos..], "browser_download_url")
}
