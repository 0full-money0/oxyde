//! Le jeu Horizon sur ce PC : dossier de jeu, launcher, lancement et mods installés.
//!
//! Chemins du launcher officiel (launcher/src/backend.rs et installer.rs du dépôt Horizon) :
//! - dossier du jeu : `HORIZON_HOME`, sinon `%APPDATA%\horizon` ;
//! - launcher : `%LOCALAPPDATA%\Programs\Horizon Launcher\horizon-launcher.exe`.

use std::io::Read;
use std::path::{Path, PathBuf};

pub const INSTALL_PAGE: &str = "https://playhorizon.dev/docs/install";

pub fn default_game_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("HORIZON_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("horizon")
}

pub fn launcher_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) { "horizon-launcher.exe" } else { "horizon-launcher" };
    let mut candidates = vec![];
    if let Some(l) = dirs::data_local_dir() {
        candidates.push(l.join("Programs").join("Horizon Launcher").join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|p| p.join(name)));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn open_launcher(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[derive(Clone, Debug)]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub native: bool,
    pub enabled: bool,
}

fn enabled_set(game_dir: &Path) -> std::collections::HashMap<String, bool> {
    std::fs::read_to_string(game_dir.join("horizon-mods.txt"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("mod.")?;
            let (id, v) = rest.split_once(':')?;
            Some((id.to_string(), v.trim() == "true"))
        })
        .collect()
}

fn folder_info(dir: &Path) -> Option<(String, String, String)> {
    let v: toml::Value = toml::from_str(&std::fs::read_to_string(dir.join("horizon-mod.toml")).ok()?).ok()?;
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = g("id");
    (!id.is_empty()).then(|| (id, g("name"), g("version")))
}

fn native_info(path: &Path) -> Option<(String, String)> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    let mut s = String::new();
    z.by_name("horizon-native.json").ok()?.read_to_string(&mut s).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    Some((v["id"].as_str()?.to_string(), v["version"].as_str().unwrap_or("").to_string()))
}

pub fn scan(game_dir: &Path) -> Vec<InstalledMod> {
    let on = enabled_set(game_dir);
    let mut out = vec![];
    let list = |d: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        v.sort();
        v
    };
    for p in list(&game_dir.join("horizon-mods")) {
        // un mod est un .hmod, ou un dossier qui contient horizon-mod.toml (ce que produit le portage officiel)
        let info = if p.is_dir() {
            folder_info(&p)
        } else if p.extension().and_then(|e| e.to_str()) == Some("hmod") {
            crate::pack::hmod_info(&p).ok()
        } else {
            None
        };
        if let Some((id, name, version)) = info {
            let enabled = on.get(&id).copied().unwrap_or(false);
            out.push(InstalledMod { name: if name.is_empty() { id.clone() } else { name }, id, version, path: p, native: false, enabled });
        }
    }
    for p in list(&game_dir.join("horizon-native-mods")) {
        if !matches!(p.extension().and_then(|e| e.to_str()), Some("hnmod" | "hmod")) {
            continue;
        }
        if let Some((id, version)) = native_info(&p) {
            out.push(InstalledMod { name: id.clone(), id, version, path: p, native: true, enabled: true });
        }
    }
    out
}

/// Écrit `mod.<id>:true|false` dans `horizon-mods.txt`, en gardant les autres lignes.
pub fn set_enabled(game_dir: &Path, id: &str, on: bool) -> Result<(), String> {
    std::fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
    let path = game_dir.join("horizon-mods.txt");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().map(String::from).collect();
    let key = format!("mod.{id}:");
    let line = format!("mod.{id}:{on}");
    match lines.iter_mut().find(|l| l.starts_with(&key)) {
        Some(l) => *l = line,
        None => lines.push(line),
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

pub fn remove(game_dir: &Path, m: &InstalledMod) -> Result<(), String> {
    if m.path.is_dir() {
        std::fs::remove_dir_all(&m.path).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(&m.path).map_err(|e| e.to_string())?;
    }
    if !m.native {
        let path = game_dir.join("horizon-mods.txt");
        if let Ok(old) = std::fs::read_to_string(&path) {
            let key = format!("mod.{}:", m.id);
            let mut text: String = old.lines().filter(|l| !l.starts_with(&key)).collect::<Vec<_>>().join("\n");
            text.push('\n');
            let _ = std::fs::write(&path, text);
        }
    }
    Ok(())
}

/// Mods Java convertis par Oxyde depuis Modrinth : id Horizon -> slug Modrinth (pour les modpacks).
pub fn sources(game_dir: &Path) -> std::collections::BTreeMap<String, String> {
    std::fs::read_to_string(game_dir.join("oxyde-sources.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn record_source(game_dir: &Path, id: &str, slug: &str) {
    let mut m = sources(game_dir);
    m.insert(id.to_string(), slug.to_string());
    if let Ok(t) = serde_json::to_string_pretty(&m) {
        let _ = std::fs::write(game_dir.join("oxyde-sources.json"), t);
    }
}
