//! Modpacks partageables (`.hzmods`) et installation dans le dossier de jeu Horizon.
//!
//! Le format officiel `.hpack` de Horizon ne contient pas les mods : il les télécharge depuis le registre.
//! Des mods convertis sur ta machine n'y sont pas, donc l'app utilise son propre format, simple :
//! un zip avec `pack.json` (nom, version, liste des mods et leur SHA-256) et `mods/<id>.hmod`.
//! À l'installation, chaque mod est vérifié par son SHA-256 puis copié dans `horizon-mods/`
//! et activé dans `horizon-mods.txt`, comme le fait le jeu (`mod.<id>:true`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const PACK_EXT: &str = "hzmods";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackMod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub file: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub format: u32,
    pub name: String,
    pub version: String,
    pub description: String,
    pub game: String,
    pub mods: Vec<PackMod>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let h = Sha256::digest(bytes);
    h.iter().map(|b| format!("{b:02x}")).collect()
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

pub fn create_pack(name: &str, description: &str, mods: &[PathBuf], out: &Path) -> Result<PathBuf, String> {
    if mods.is_empty() {
        return Err("aucun mod à mettre dans le pack".into());
    }
    let mut info = PackInfo {
        format: 1,
        name: name.trim().to_string(),
        version: "1.0.0".into(),
        description: description.trim().to_string(),
        game: "26.3".into(),
        mods: vec![],
    };
    let mut payload = Vec::new();
    for m in mods {
        let (id, mname, version) = hmod_info(m)?;
        if info.mods.iter().any(|x| x.id == id) {
            continue;
        }
        let bytes = std::fs::read(m).map_err(|e| e.to_string())?;
        let file = format!("{id}.hmod");
        info.mods.push(PackMod { id, name: mname, version, file: file.clone(), sha256: sha256_hex(&bytes) });
        payload.push((file, bytes));
    }
    let tmp = out.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut z = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("pack.json", o).map_err(|e| e.to_string())?;
        z.write_all(serde_json::to_string_pretty(&info).unwrap().as_bytes()).map_err(|e| e.to_string())?;
        z.start_file("LISEZ-MOI.txt", o).map_err(|e| e.to_string())?;
        z.write_all(
            format!(
                "Modpack Horizon « {} » ({} mods).\r\n\r\nPour l'installer : ouvre Horizon Mod Converter et glisse ce fichier dans la fenêtre,\r\npuis clique sur « Installer ».\r\n\r\nÀ la main : copie les fichiers du dossier mods/ dans le dossier horizon-mods/ de ton jeu Horizon\r\n(le dossier qui contient options.txt), puis active-les dans Options > Mods.\r\n",
                info.name,
                info.mods.len()
            )
            .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        for (file, bytes) in payload {
            // les .hmod sont déjà compressés
            z.start_file(format!("mods/{file}"), stored).map_err(|e| e.to_string())?;
            z.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        z.finish().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    Ok(out.to_path_buf())
}

pub fn read_pack(path: &Path) -> Result<PackInfo, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("pack invalide : {e}"))?;
    let mut s = String::new();
    z.by_name("pack.json").map_err(|_| "pack invalide : pas de pack.json".to_string())?.read_to_string(&mut s).map_err(|e| e.to_string())?;
    let info: PackInfo = serde_json::from_str(&s).map_err(|e| format!("pack.json invalide : {e}"))?;
    if info.format != 1 {
        return Err(format!("format de pack {} non pris en charge : mets l'app à jour", info.format));
    }
    Ok(info)
}

fn safe_file_name(f: &str) -> bool {
    f.ends_with(".hmod")
        && f.len() <= 80
        && f.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && !f.starts_with('.')
}

/// Installe un pack : vérifie chaque mod puis le copie et l'active. Retourne les ids installés.
pub fn install_pack(pack: &Path, game_dir: &Path) -> Result<Vec<String>, String> {
    let info = read_pack(pack)?;
    let f = std::fs::File::open(pack).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mods_dir = game_dir.join("horizon-mods");
    std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    for m in &info.mods {
        if !safe_file_name(&m.file) {
            return Err(format!("nom de fichier refusé dans le pack : {}", m.file));
        }
        let mut bytes = Vec::new();
        z.by_name(&format!("mods/{}", m.file))
            .map_err(|_| format!("{} manque dans le pack", m.file))?
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if sha256_hex(&bytes) != m.sha256 {
            return Err(format!("{} est corrompu (SHA-256 différent)", m.file));
        }
        std::fs::write(mods_dir.join(&m.file), &bytes).map_err(|e| e.to_string())?;
        done.push(m.id.clone());
    }
    enable_mods(game_dir, &done)?;
    Ok(done)
}

/// Copie des .hmod dans `horizon-mods/` et les active.
pub fn install_mods(mods: &[PathBuf], game_dir: &Path) -> Result<Vec<String>, String> {
    let mods_dir = game_dir.join("horizon-mods");
    std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for m in mods {
        let (id, _, _) = hmod_info(m)?;
        std::fs::copy(m, mods_dir.join(format!("{id}.hmod"))).map_err(|e| e.to_string())?;
        ids.push(id);
    }
    enable_mods(game_dir, &ids)?;
    Ok(ids)
}

/// Écrit `mod.<id>:true` dans `horizon-mods.txt`, en gardant les autres lignes.
pub fn enable_mods(game_dir: &Path, ids: &[String]) -> Result<(), String> {
    let path = game_dir.join("horizon-mods.txt");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().map(String::from).collect();
    for id in ids {
        let key = format!("mod.{id}:");
        let line = format!("mod.{id}:true");
        match lines.iter_mut().find(|l| l.starts_with(&key)) {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| e.to_string())
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
    fn pack_roundtrip() {
        let dir = std::env::temp_dir().join(format!("hmc-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut outs = vec![];
        for id in ["alpha", "beta"] {
            let jar = dir.join(format!("{id}.jar"));
            let fmj = format!(r#"{{"id":"{id}","version":"1.0"}}"#);
            let tex = format!("assets/{id}/textures/item/x.png");
            make_jar(&jar, &[("fabric.mod.json", fmj.as_str()), (tex.as_str(), "png")]);
            outs.push(convert_jar(&jar, &dir.join("out"), &Options::default()).unwrap().output);
        }
        let pack = dir.join("p.hzmods");
        create_pack("Mon pack", "", &outs, &pack).unwrap();
        let info = read_pack(&pack).unwrap();
        assert_eq!(info.mods.len(), 2);
        let game = dir.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("horizon-mods.txt"), "mod.alpha:false\nmod.zzz:true\n").unwrap();
        let ids = install_pack(&pack, &game).unwrap();
        assert_eq!(ids, vec!["alpha", "beta"]);
        let txt = std::fs::read_to_string(game.join("horizon-mods.txt")).unwrap();
        assert_eq!(txt, "mod.alpha:true\nmod.zzz:true\nmod.beta:true\n");
        assert!(game.join("horizon-mods/beta.hmod").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }
}
