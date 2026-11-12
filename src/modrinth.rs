//! Recherche et téléchargement de mods Java sur Modrinth (API publique v2).

use crate::net;
use serde::Deserialize;
use std::path::PathBuf;

const API: &str = "https://api.modrinth.com/v2";
const MAX_JAR: u64 = 256 * 1024 * 1024;

#[derive(Deserialize, Clone, Debug)]
pub struct SearchResp {
    pub hits: Vec<Hit>,
    pub total_hits: u64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Hit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub downloads: u64,
    pub icon_url: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct MVersion {
    #[serde(default)]
    game_versions: Vec<String>,
    files: Vec<MFile>,
}

#[derive(Deserialize, Clone, Debug)]
struct MFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    hashes: Hashes,
}

#[derive(Deserialize, Clone, Debug)]
struct Hashes {
    sha512: Option<String>,
}

pub fn search(query: &str, offset: u64) -> Result<SearchResp, String> {
    let facets = r#"[["project_type:mod"],["categories:fabric","categories:neoforge","categories:forge","categories:quilt"]]"#;
    let url = format!(
        "{API}/search?query={}&facets={}&limit=20&offset={offset}&index={}",
        net::urlencode(query),
        net::urlencode(facets),
        if query.trim().is_empty() { "downloads" } else { "relevance" }
    );
    net::get_json(&url)
}

/// Télécharge le .jar de la version la plus adaptée (26.3 si elle existe, sinon la plus récente).
pub fn download_jar(project_id: &str) -> Result<PathBuf, String> {
    let versions: Vec<MVersion> = net::get_json(&format!("{API}/project/{}/version", net::urlencode(project_id)))?;
    let v = versions
        .iter()
        .find(|v| v.game_versions.iter().any(|g| g == "26.3"))
        .or_else(|| versions.first())
        .ok_or("aucune version téléchargeable sur Modrinth")?;
    let f = v
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| v.files.first())
        .ok_or("aucun fichier dans cette version")?;
    if !f.url.starts_with("https://cdn.modrinth.com/") {
        return Err("adresse de téléchargement refusée".into());
    }
    if !net::safe_name(&f.filename) || !f.filename.ends_with(".jar") {
        return Err("ce projet ne publie pas de .jar".into());
    }
    let bytes = net::download(&f.url, MAX_JAR)?;
    if let Some(h) = &f.hashes.sha512 {
        if net::sha512_hex(&bytes) != h.to_lowercase() {
            return Err("le .jar téléchargé est corrompu (SHA-512 différent)".into());
        }
    }
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Oxyde").join("jars");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&f.filename);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path)
}
