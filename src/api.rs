use anyhow::{Context, Result};
use crate::catalog::{parse_catalog, parse_versions, EsdFile, WindowsVersion};

const VERSIONS_URL: &str = "https://worproject.com/dldserv/esd/getversions.php";
const CATALOG_URL:  &str = "https://worproject.com/dldserv/esd/getcatalog.php";

pub fn fetch_versions() -> Result<Vec<WindowsVersion>> {
    let body = reqwest::blocking::get(VERSIONS_URL)
        .context("fetch versions")?
        .text()?;
    parse_versions(&body)
}

pub fn fetch_catalog(build: &str) -> Result<Vec<EsdFile>> {
    let url  = format!("{}?build={}", CATALOG_URL, build);
    let body = reqwest::blocking::get(&url)
        .context("fetch catalog")?
        .text()?;
    parse_catalog(&body)
}
