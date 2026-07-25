use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct WindowsVersion {
    pub number:   String,
    pub releases: Vec<Release>,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub build: String,
    pub date:  String,
}

#[derive(Debug, Clone)]
pub struct EsdFile {
    pub filename:      String,
    pub language_code: String,
    pub language:      String,
    #[allow(dead_code)]
    pub editions:      Vec<String>,
    pub architecture:  String,
    pub size:          u64,
    pub sha256:        String,
    pub url:           String,
}

impl EsdFile {
    pub fn edition_label(&self) -> String {
        if self.filename.contains("CLIENTBUSINESS_VOL") {
            "Business Volume (Enterprise / EnterpriseN)".into()
        } else if self.filename.contains("CLIENTCHINA") {
            "China Retail".into()
        } else {
            "Consumer Retail (Pro / Home / Education / …)".into()
        }
    }

    pub fn size_gb(&self) -> f64 {
        self.size as f64 / 1_073_741_824.0
    }
}

pub fn parse_versions(xml: &str) -> Result<Vec<WindowsVersion>> {
    let doc = roxmltree::Document::parse(xml).context("parse versions XML")?;
    let mut versions = Vec::new();

    for ver in doc.descendants().filter(|n| n.has_tag_name("version")) {
        let number = ver.attribute("number").unwrap_or("").to_string();
        let mut releases = Vec::new();

        for rel in ver.descendants().filter(|n| n.has_tag_name("release")) {
            let build = rel.attribute("build").unwrap_or("").to_string();
            let date  = child_text(&rel, "date");
            if !build.is_empty() {
                releases.push(Release { build, date });
            }
        }

        if !number.is_empty() {
            versions.push(WindowsVersion { number, releases });
        }
    }

    Ok(versions)
}

pub fn parse_catalog(xml: &str) -> Result<Vec<EsdFile>> {
    let doc = roxmltree::Document::parse(xml).context("parse catalog XML")?;
    let mut files = Vec::new();

    for file_node in doc.descendants().filter(|n| n.has_tag_name("File")) {
        let filename      = child_text(&file_node, "FileName");
        let language_code = child_text(&file_node, "LanguageCode");
        let language      = child_text(&file_node, "Language");
        let architecture  = child_text(&file_node, "Architecture");
        let sha256        = child_text(&file_node, "Sha256");
        let url           = child_text(&file_node, "FilePath");
        let edition_str   = child_text(&file_node, "Edition");
        let size          = child_text(&file_node, "Size").parse::<u64>().unwrap_or(0);

        if filename.is_empty() || url.is_empty() || architecture.is_empty() {
            continue;
        }

        let editions = edition_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        files.push(EsdFile {
            filename,
            language_code,
            language,
            editions,
            architecture,
            size,
            sha256,
            url,
        });
    }

    Ok(files)
}

fn child_text(node: &roxmltree::Node, tag: &str) -> String {
    node.descendants()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string()
}
