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
    pub slug: String,
    pub title: String,
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

#[derive(Deserialize, Clone, Debug)]
struct VersionFile {
    project_id: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ProjectInfo {
    pub slug: String,
    pub title: String,
}

/// Retrouve le projet Modrinth d'un .jar grâce à son empreinte SHA-1.
pub fn identify(jar: &std::path::Path) -> Option<ProjectInfo> {
    let bytes = std::fs::read(jar).ok()?;
    let sha1 = net::sha1_hex(&bytes);
    let vf: VersionFile = net::get_json(&format!("{API}/version_file/{sha1}?algorithm=sha1")).ok()?;
    net::get_json(&format!("{API}/project/{}", net::urlencode(&vf.project_id))).ok()
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Retrouve un projet Modrinth par le nom et l'id lus dans un .jar (fichier venu d'ailleurs).
pub fn find_by_name(name: &str, mod_id: &str) -> Option<ProjectInfo> {
    let (n, id) = (norm(name), norm(mod_id));
    let hits = search(name, 0).ok()?.hits;
    hits.into_iter()
        .find(|h| norm(&h.title) == n || norm(&h.slug) == id || norm(&h.slug) == n)
        .map(|h| ProjectInfo { slug: h.slug, title: h.title })
}

// ---------------------------------------------------------------------------
// Modpacks Modrinth (.mrpack) : recherche, lecture, téléchargement
// ---------------------------------------------------------------------------

const MAX_PACK: u64 = 512 * 1024 * 1024;

#[derive(Deserialize, Clone, Debug)]
struct PackVersion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    files: Vec<MFile>,
}

#[derive(Deserialize, Clone, Debug)]
struct MrIndex {
    #[serde(default)]
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize, Clone, Debug)]
struct MrFile {
    path: String,
}

/// Ce qu'il y a dans un modpack Modrinth.
#[derive(Clone, Debug, Default)]
pub struct PackInfo {
    pub version: String,
    pub minecraft: String,
    pub loader: String,
    pub mods: Vec<String>,
    pub resource_packs: usize,
    pub shaders: usize,
}

pub fn search_packs(query: &str, offset: u64) -> Result<SearchResp, String> {
    let facets = r#"[["project_type:modpack"]]"#;
    let url = format!(
        "{API}/search?query={}&facets={}&limit=20&offset={offset}&index={}",
        net::urlencode(query),
        net::urlencode(facets),
        if query.trim().is_empty() { "downloads" } else { "relevance" }
    );
    net::get_json(&url)
}

/// Joli nom d'un fichier de mod : « sodium-fabric-0.5.8+mc1.20.1.jar » → « sodium fabric ».
fn mod_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.trim_end_matches(".jar").trim_end_matches(".disabled");
    let mut words = vec![];
    for w in stem.split(['-', '_', ' ', '+']) {
        if w.chars().next().is_some_and(|c| c.is_ascii_digit()) || w.starts_with("mc1") || w.starts_with("v1") || w.starts_with("v0") {
            break;
        }
        if !w.is_empty() && (words.is_empty() || !matches!(w.to_lowercase().as_str(), "fabric" | "forge" | "neoforge" | "quilt" | "mc")) {
            words.push(w);
        }
    }
    if words.is_empty() {
        stem.to_string()
    } else {
        words.join(" ")
    }
}

/// Lit le manifeste d'un .mrpack déjà téléchargé.
pub fn read_mrpack(path: &std::path::Path) -> Result<PackInfo, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("modpack illisible : {e}"))?;
    let index: MrIndex = {
        let entry = z.by_name("modrinth.index.json").map_err(|_| "ce fichier n'est pas un modpack Modrinth")?;
        serde_json::from_reader(entry).map_err(|e| format!("manifeste illisible : {e}"))?
    };
    let mut info = PackInfo {
        minecraft: index.dependencies.get("minecraft").cloned().unwrap_or_default(),
        loader: ["fabric-loader", "forge", "neoforge", "quilt-loader"]
            .iter()
            .find_map(|k| index.dependencies.get(*k).map(|v| format!("{} {v}", k.trim_end_matches("-loader"))))
            .unwrap_or_default(),
        ..Default::default()
    };
    // mods listés + mods inclus directement dans le pack (overrides/mods)
    let mut mods: Vec<String> = index.files.iter().filter(|f| f.path.starts_with("mods/")).map(|f| mod_name(&f.path)).collect();
    for i in 0..z.len() {
        if let Ok(e) = z.by_index(i) {
            let n = e.name();
            if (n.starts_with("overrides/mods/") || n.starts_with("client-overrides/mods/")) && n.ends_with(".jar") {
                mods.push(mod_name(n));
            }
        }
    }
    info.resource_packs = index.files.iter().filter(|f| f.path.starts_with("resourcepacks/")).count();
    info.shaders = index.files.iter().filter(|f| f.path.starts_with("shaderpacks/")).count();
    mods.sort_by_key(|m| m.to_lowercase());
    mods.dedup();
    info.mods = mods;
    Ok(info)
}

/// Télécharge la dernière version d'un modpack (.mrpack vérifié) et lit ce qu'il contient.
pub fn open_pack(slug: &str) -> Result<(PackInfo, PathBuf), String> {
    let versions: Vec<PackVersion> = net::get_json(&format!("{API}/project/{}/version", net::urlencode(slug)))?;
    let v = versions.first().ok_or("ce modpack n'a aucune version")?;
    let f = v.files.iter().find(|f| f.primary).or_else(|| v.files.first()).ok_or("aucun fichier dans cette version")?;
    if !f.url.starts_with("https://cdn.modrinth.com/") {
        return Err("adresse de téléchargement refusée".into());
    }
    if !net::safe_name(&f.filename) || !f.filename.ends_with(".mrpack") {
        return Err("cette version ne publie pas de .mrpack".into());
    }
    let dir = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Oxyde").join("mrpacks");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&f.filename);
    if !path.exists() {
        let bytes = net::download(&f.url, MAX_PACK)?;
        if let Some(h) = &f.hashes.sha512 {
            if net::sha512_hex(&bytes) != h.to_lowercase() {
                return Err("le modpack téléchargé est corrompu (SHA-512 différent)".into());
            }
        }
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    }
    let mut info = read_mrpack(&path)?;
    info.version = if v.version_number.is_empty() { v.name.clone() } else { v.version_number.clone() };
    if info.minecraft.is_empty() {
        info.minecraft = v.game_versions.first().cloned().unwrap_or_default();
    }
    if info.loader.is_empty() {
        info.loader = v.loaders.join(", ");
    }
    Ok((info, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_mod_names() {
        assert_eq!(mod_name("mods/sodium-fabric-0.5.8+mc1.20.1.jar"), "sodium");
        assert_eq!(mod_name("mods/fabric-api-0.100.jar"), "fabric api");
        assert_eq!(mod_name("mods/create-1.20.1-0.5.1.f.jar"), "create");
        assert_eq!(mod_name("overrides/mods/Xaeros_Minimap_24.0_Fabric_1.20.jar"), "Xaeros Minimap");
    }
}
