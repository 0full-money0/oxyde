//! Modpacks Oxyde (`.oxypack`) et installation de mods dans le dossier du jeu.
//!
//! Un `.oxypack` est un petit zip :
//! - `pack.json` : nom, version et liste des mods ;
//! - chaque mod est une référence (registre Horizon ou Modrinth : il est retéléchargé et reporté chez ton ami)
//!   ou, seulement s'il ne vient d'aucun des deux, un fichier `mods/<id>.hmod` intégré (vérifié par SHA-256).
//!
//! L'ancien format `.hzmods` (v1, tous les mods intégrés) se lit toujours.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const PACK_EXT: &str = "oxypack";
pub const LEGACY_EXT: &str = "hzmods";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum Entry {
    Horizon { id: String, name: String },
    Modrinth { slug: String, name: String },
    File { id: String, name: String, file: String, sha256: String },
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::Horizon { name, .. } | Entry::Modrinth { name, .. } | Entry::File { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub format: u32,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub game: String,
    pub mods: Vec<Entry>,
}

// ancien format v1
#[derive(Deserialize)]
struct V1Mod {
    id: String,
    #[serde(default)]
    name: String,
    file: String,
    sha256: String,
}
#[derive(Deserialize)]
struct V1 {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    mods: Vec<V1Mod>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Lit l'id, le nom et la version dans le `horizon-mod.toml` d'un .hmod.
pub fn hmod_info(path: &Path) -> Result<(String, String, String), String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut s = String::new();
    z.by_name("horizon-mod.toml")
        .map_err(|_| format!("{} n'a pas de horizon-mod.toml", path.display()))?
        .read_to_string(&mut s)
        .map_err(|e| e.to_string())?;
    let v: toml::Value = toml::from_str(&s).map_err(|e| e.to_string())?;
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let id = g("id");
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(format!("{} : id invalide", path.display()));
    }
    Ok((id, g("name"), g("version")))
}

/// Un mod installé en dossier (portage officiel) devient un .hmod en mémoire.
pub fn zip_folder(dir: &Path) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut stack = vec![dir.to_path_buf()];
        let mut files = vec![];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    files.push(p);
                }
            }
        }
        files.sort();
        for p in files {
            let rel = p.strip_prefix(dir).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            z.start_file(rel, o).map_err(|e| e.to_string())?;
            z.write_all(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

/// Écrit un .oxypack. `files` donne le contenu des entrées `File` (id -> octets du .hmod).
pub fn write_pack(pack: &Pack, files: &[(String, Vec<u8>)], out: &Path) -> Result<(), String> {
    let tmp = out.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut z = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("pack.json", o).map_err(|e| e.to_string())?;
        z.write_all(serde_json::to_string_pretty(pack).unwrap().as_bytes()).map_err(|e| e.to_string())?;
        z.start_file("README.txt", o).map_err(|e| e.to_string())?;
        z.write_all(
            format!(
                "Modpack Oxyde \"{}\" ({} mods) for Horizon (Minecraft Rust Edition).\r\n\r\nTo install it: open Oxyde (https://github.com/0full-money0/oxyde) and drop this file in the window.\r\nOxyde downloads, ports and installs every mod for you.\r\n",
                pack.name,
                pack.mods.len()
            )
            .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        for (file, bytes) in files {
            z.start_file(file.as_str(), stored).map_err(|e| e.to_string())?;
            z.write_all(bytes).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())
}

pub fn read_pack(path: &Path) -> Result<Pack, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("modpack invalide : {e}"))?;
    let mut s = String::new();
    z.by_name("pack.json").map_err(|_| "modpack invalide : pas de pack.json".to_string())?.read_to_string(&mut s).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&s).map_err(|e| format!("pack.json invalide : {e}"))?;
    match v["format"].as_u64() {
        Some(1) => {
            let old: V1 = serde_json::from_value(v).map_err(|e| e.to_string())?;
            Ok(Pack {
                format: 2,
                name: old.name,
                version: old.version,
                description: old.description,
                game: "26.3".into(),
                mods: old
                    .mods
                    .into_iter()
                    .map(|m| Entry::File { name: if m.name.is_empty() { m.id.clone() } else { m.name }, id: m.id, file: format!("mods/{}", m.file), sha256: m.sha256 })
                    .collect(),
            })
        }
        Some(2) => serde_json::from_value(v).map_err(|e| format!("pack.json invalide : {e}")),
        Some(n) => Err(format!("format de modpack {n} non pris en charge : mets Oxyde à jour")),
        None => Err("pack.json invalide : pas de format".into()),
    }
}

fn safe_entry_path(f: &str) -> bool {
    let Some(name) = f.strip_prefix("mods/") else { return false };
    name.ends_with(".hmod") && name.len() <= 80 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.') && !name.starts_with('.')
}

/// Sort un mod intégré dans un fichier temporaire, après vérification de son SHA-256.
pub fn extract(pack: &Path, file: &str, sha256: &str) -> Result<PathBuf, String> {
    if !safe_entry_path(file) {
        return Err(format!("nom de fichier refusé dans le modpack : {file}"));
    }
    let f = std::fs::File::open(pack).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    z.by_name(file).map_err(|_| format!("{file} manque dans le modpack"))?.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if sha256_hex(&bytes) != sha256 {
        return Err(format!("{file} est corrompu (SHA-256 différent)"));
    }
    let dir = std::env::temp_dir().join("oxyde-pack");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(file.trim_start_matches("mods/"));
    std::fs::write(&out, bytes).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Copie des .hmod dans `horizon-mods/` et les active.
pub fn install_mods(mods: &[PathBuf], game_dir: &Path) -> Result<Vec<String>, String> {
    let mods_dir = game_dir.join("horizon-mods");
    std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for m in mods {
        let (id, _, _) = hmod_info(m)?;
        std::fs::copy(m, mods_dir.join(format!("{id}.hmod"))).map_err(|e| e.to_string())?;
        crate::game::set_enabled(game_dir, &id, true)?;
        ids.push(id);
    }
    Ok(ids)
}

/// Un dossier de jeu Horizon contient `options.txt` (ou déjà `horizon-mods/`).
pub fn looks_like_game_dir(p: &Path) -> bool {
    p.join("options.txt").is_file() || p.join("horizon-mods").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{convert_jar, tests::make_jar, Options};

    #[test]
    fn pack_roundtrip_v2() {
        let dir = std::env::temp_dir().join(format!("oxy-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("alpha.jar");
        make_jar(&jar, &[("fabric.mod.json", r#"{"id":"alpha","version":"1.0"}"#), ("assets/alpha/textures/item/x.png", "png")]);
        let hmod = convert_jar(&jar, &dir.join("out"), &Options::default()).unwrap().output;
        let bytes = std::fs::read(&hmod).unwrap();
        let pack = Pack {
            format: 2,
            name: "Mon pack".into(),
            version: "1.0.0".into(),
            description: String::new(),
            game: "26.3".into(),
            mods: vec![
                Entry::Horizon { id: "zoom".into(), name: "Zoom".into() },
                Entry::Modrinth { slug: "macaws-doors".into(), name: "Macaw's Doors".into() },
                Entry::File { id: "alpha".into(), name: "alpha".into(), file: "mods/alpha.hmod".into(), sha256: sha256_hex(&bytes) },
            ],
        };
        let out = dir.join("p.oxypack");
        write_pack(&pack, &[("mods/alpha.hmod".into(), bytes)], &out).unwrap();
        let back = read_pack(&out).unwrap();
        assert_eq!(back.mods, pack.mods);
        let Entry::File { file, sha256, .. } = &back.mods[2] else { panic!() };
        let x = extract(&out, file, sha256).unwrap();
        let game = dir.join("game");
        assert_eq!(install_mods(&[x], &game).unwrap(), vec!["alpha"]);
        assert!(std::fs::read_to_string(game.join("horizon-mods.txt")).unwrap().contains("mod.alpha:true"));
        assert!(extract(&out, "mods/../evil.hmod", sha256).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
