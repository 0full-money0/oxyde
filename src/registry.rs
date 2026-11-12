//! Le registre officiel de Horizon (horizon-rust/horizon-mods), lu comme le fait le launcher :
//! `index.json`, fichiers vérifiés par taille et SHA-256 avant d'arriver dans le dossier du jeu.
//! Doc : docs/api/mod-index.md et docs/mod-browser.md du dépôt Horizon.

use crate::{game, net};
use serde::Deserialize;
use std::path::Path;

pub const INDEX_URL: &str = "https://horizon-rust.github.io/horizon-mods/index.json";
const MAX_FILE: u64 = 64 * 1024 * 1024;

#[derive(Deserialize, Clone, Debug)]
pub struct Index {
    pub schema: u32,
    pub projects: Vec<Project>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Project {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: String,
    pub icon: Option<String>,
    pub downloads: Option<u64>,
    pub updated: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub versions: Vec<Version>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Version {
    pub version: String,
    pub game: Option<String>,
    pub file: FileRef,
}

#[derive(Deserialize, Clone, Debug)]
pub struct FileRef {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Revoked {
    pub id: String,
    pub sha256: Option<String>,
    #[serde(default)]
    pub reason: String,
}

fn cache_file() -> std::path::PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Oxyde").join("horizon-index.json")
}

fn parse(text: &str) -> Result<Index, String> {
    let idx: Index = serde_json::from_str(text).map_err(|e| format!("registre illisible : {e}"))?;
    if idx.schema > 1 {
        return Err("le registre Horizon a changé de format : mets Oxyde à jour".into());
    }
    Ok(idx)
}

/// Dernière copie gardée sur le disque : l'app l'affiche tout de suite au démarrage.
pub fn cached() -> Option<Index> {
    parse(&std::fs::read_to_string(cache_file()).ok()?).ok()
}

/// Télécharge l'index à jour et garde une copie.
pub fn fetch() -> Result<Index, String> {
    let resp = net::agent().get(INDEX_URL).call().map_err(|e| format!("registre Horizon injoignable : {e}"))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(resp.into_reader(), 32 * 1024 * 1024), &mut text)
        .map_err(|e| e.to_string())?;
    let idx = parse(&text)?;
    let f = cache_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, &text);
    Ok(idx)
}

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "mod" => "Contenu",
        "native-mod" => "Natif (Rust)",
        "resourcepack" => "Pack de ressources",
        "shader" => "Shader",
        "modpack" => "Modpack",
        _ => "Autre",
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

impl Index {
    /// Cherche la version Horizon officielle d'un mod Java (même id, même slug ou même nom).
    pub fn find_port(&self, java_id: &str, java_name: &str) -> Option<&Project> {
        let (id, name) = (norm(java_id), norm(java_name));
        if id.is_empty() && name.is_empty() {
            return None;
        }
        self.projects
            .iter()
            .filter(|p| p.kind == "mod" || p.kind == "native-mod")
            .find(|p| {
                let pid = norm(&p.id);
                let pname = norm(&p.name);
                (!id.is_empty() && (pid == id || pname == id)) || (!name.is_empty() && pname == name)
            })
    }

    fn revoked_reason(&self, p: &Project, sha: &str) -> Option<String> {
        self.revoked
            .iter()
            .find(|r| r.id == p.id && r.sha256.as_deref().is_none_or(|s| s == sha))
            .map(|r| r.reason.clone())
    }
}

fn trusted_url(u: &str) -> bool {
    u.starts_with("https://github.com/") && u.contains("/releases/download/")
}

/// Télécharge, vérifie et installe la dernière version d'un projet du registre.
pub fn install(index: &Index, p: &Project, game_dir: &Path) -> Result<String, String> {
    let v = p.versions.first().ok_or("aucune version publiée")?;
    let f = &v.file;
    if let Some(why) = index.revoked_reason(p, &f.sha256) {
        return Err(format!("retiré du registre Horizon : {why}"));
    }
    if !trusted_url(&f.url) {
        return Err("adresse de téléchargement refusée".into());
    }
    if !net::safe_name(&f.name) {
        return Err("nom de fichier refusé".into());
    }
    if f.size > MAX_FILE {
        return Err("fichier trop gros".into());
    }
    let bytes = net::download(&f.url, f.size.max(1))?;
    if bytes.len() as u64 != f.size || net::sha256_hex(&bytes) != f.sha256.to_lowercase() {
        return Err("le fichier téléchargé ne correspond pas au registre (taille ou SHA-256)".into());
    }
    let ext = Path::new(&f.name).extension().and_then(|e| e.to_str()).unwrap_or("");
    match p.kind.as_str() {
        "mod" => {
            let dir = game_dir.join("horizon-mods");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let dest = dir.join(format!("{}.hmod", p.id));
            write_checked(&dest, &bytes)?;
            match crate::pack::hmod_info(&dest) {
                Ok((id, _, _)) if id == p.id => {}
                _ => {
                    let _ = std::fs::remove_file(&dest);
                    return Err("le manifeste du mod ne correspond pas au registre".into());
                }
            }
            game::set_enabled(game_dir, &p.id, true)?;
            Ok(format!("{} {} installé et activé", p.name, v.version))
        }
        "native-mod" => {
            let dir = game_dir.join("horizon-native-mods");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let ext = if ext == "hmod" { "hmod" } else { "hnmod" };
            write_checked(&dir.join(format!("{}.{ext}", p.id)), &bytes)?;
            game::set_enabled(game_dir, &p.id, true)?;
            Ok(format!(
                "{} {} installé. Le launcher Horizon le compile au prochain lancement (la première fois prend quelques minutes).",
                p.name, v.version
            ))
        }
        "resourcepack" => {
            let dir = game_dir.join("resourcepacks");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            write_checked(&dir.join(&f.name), &bytes)?;
            Ok(format!("{} installé : active-le dans Options > Packs de ressources", p.name))
        }
        "shader" => {
            let dir = game_dir.join("shaderpacks");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            write_checked(&dir.join(&f.name), &bytes)?;
            Ok(format!("{} installé : choisis-le dans les options graphiques", p.name))
        }
        "modpack" => Err("installe les modpacks Horizon depuis le launcher Horizon (onglet Modpacks)".into()),
        k => Err(format!("type « {k} » pas encore pris en charge")),
    }
}

fn write_checked(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_and_finds_ports() {
        let json = r#"{"schema":1,"projects":[
          {"id":"appleskin","kind":"native-mod","name":"AppleSkin","versions":[{"version":"1","file":{"name":"a.hnmod","url":"https://github.com/x/y/releases/download/t/a.hnmod","size":1,"sha256":"00"}}]},
          {"id":"faithful","kind":"resourcepack","name":"Faithful","versions":[{"version":"1","file":{"name":"f.zip","url":"u","size":1,"sha256":"00"}}]}
        ],"revoked":[]}"#;
        let idx: Index = serde_json::from_str(json).unwrap();
        assert_eq!(idx.find_port("appleskin", "").unwrap().id, "appleskin");
        assert_eq!(idx.find_port("x", "Apple Skin").unwrap().id, "appleskin");
        assert!(idx.find_port("faithful", "").is_none());
        assert!(trusted_url("https://github.com/x/y/releases/download/t/a.hnmod"));
        assert!(!trusted_url("https://evil.example/a.hnmod"));
    }
}
