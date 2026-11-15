//! CurseForge (bêta) : recherche et téléchargement avec la clé API gratuite de l'utilisateur.
//! CurseForge n'autorise l'accès à son API qu'avec une clé (console.curseforge.com).
//! Les .jar téléchargés passent ensuite par le même pipeline qu'un .jar déposé.

use crate::net;
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;

const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
const MODS_CLASS: u32 = 6;
pub const KEY_PAGE: &str = "https://console.curseforge.com/";

#[derive(Deserialize, Clone, Debug)]
pub struct Resp {
    pub data: Vec<Mod>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Mod {
    pub id: u64,
    pub name: String,
    #[serde(default, rename = "downloadCount")]
    pub downloads: f64,
    pub logo: Option<Logo>,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default, rename = "latestFiles")]
    pub files: Vec<File>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Logo {
    #[serde(rename = "thumbnailUrl")]
    pub thumb: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Author {
    pub name: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct File {
    #[serde(rename = "fileName")]
    pub name: String,
    #[serde(rename = "downloadUrl")]
    pub url: Option<String>,
    #[serde(default, rename = "gameVersions")]
    pub versions: Vec<String>,
}

fn get(key: &str, url: &str) -> Result<ureq::Response, String> {
    net::agent().get(url).set("x-api-key", key).set("Accept", "application/json").call().map_err(|e| match e {
        ureq::Error::Status(403, _) => "clé CurseForge refusée : vérifie-la dans les Réglages".to_string(),
        e => format!("CurseForge injoignable : {e}"),
    })
}

pub fn search(key: &str, query: &str, offset: u64) -> Result<Vec<Mod>, String> {
    let url = format!(
        "{API}/mods/search?gameId={MINECRAFT}&classId={MODS_CLASS}&searchFilter={}&sortField=6&sortOrder=desc&pageSize=20&index={offset}",
        net::urlencode(query)
    );
    let r: Resp = get(key, &url)?.into_json().map_err(|e| format!("réponse CurseForge illisible : {e}"))?;
    Ok(r.data)
}

/// Télécharge le .jar le plus adapté (26.3 si possible). Certains auteurs interdisent le téléchargement hors CurseForge.
pub fn download(m: &Mod) -> Result<PathBuf, String> {
    let f = m
        .files
        .iter()
        .find(|f| f.versions.iter().any(|v| v == "26.3") && f.name.ends_with(".jar"))
        .or_else(|| m.files.iter().find(|f| f.name.ends_with(".jar")))
        .ok_or("aucun .jar publié pour ce mod")?;
    let url = f.url.clone().ok_or_else(|| {
        format!("l'auteur de {} interdit le téléchargement hors du site CurseForge : télécharge le .jar sur curseforge.com et glisse-le dans Oxyde", m.name)
    })?;
    if !url.starts_with("https://edge.forgecdn.net/") && !url.starts_with("https://mediafilez.forgecdn.net/") {
        return Err("adresse de téléchargement refusée".into());
    }
    if !net::safe_name(&f.name) {
        return Err("nom de fichier refusé".into());
    }
    let resp = net::agent().get(&url).call().map_err(|e| format!("téléchargement impossible : {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader().take(256 * 1024 * 1024).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Oxyde").join("jars");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&f.name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}
