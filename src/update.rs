//! Mise à jour automatique depuis les GitHub Releases du dépôt qui a compilé l'app.
//!
//! - Le dépôt est fixé à la compilation (`HMC_REPO`, mis par GitHub Actions = `owner/repo`).
//! - L'app lit la dernière release, compare la version (semver), télécharge `HorizonModConverter.exe`
//!   et vérifie son SHA-256 avec `HorizonModConverter.exe.sha256` publié par la même build.
//! - Seules des URL https de github.com sont suivies. Le .exe est remplacé puis l'app redémarre.

use sha2::{Digest, Sha256};
use std::io::Read;

pub const REPO: Option<&str> = option_env!("HMC_REPO");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ASSET: &str = "Oxyde.exe";
const UA: &str = crate::net::UA;

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub exe_url: String,
    pub sha_url: String,
    pub page: String,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60)).user_agent(UA).build()
}

fn trusted(url: &str) -> bool {
    url.starts_with("https://github.com/") || url.starts_with("https://objects.githubusercontent.com/") || url.starts_with("https://api.github.com/")
}

/// `Ok(None)` : déjà à jour (ou build locale sans dépôt).
pub fn check() -> Result<Option<Release>, String> {
    let Some(repo) = REPO else { return Ok(None) };
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let v: serde_json::Value = agent()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("vérification des mises à jour impossible : {e}"))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let latest = semver::Version::parse(&tag).map_err(|_| format!("version de release illisible : {tag}"))?;
    let current = semver::Version::parse(VERSION).unwrap();
    if latest <= current {
        return Ok(None);
    }
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let find = |name: &str| {
        assets
            .iter()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(String::from)
    };
    let exe_url = find(ASSET).ok_or("la release n'a pas de Oxyde.exe")?;
    let sha_url = find(&format!("{ASSET}.sha256")).ok_or("la release n'a pas de fichier .sha256")?;
    if !trusted(&exe_url) || !trusted(&sha_url) {
        return Err("adresse de téléchargement non fiable".into());
    }
    Ok(Some(Release {
        version: tag,
        exe_url,
        sha_url,
        page: v["html_url"].as_str().unwrap_or("").to_string(),
    }))
}

fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| format!("téléchargement : {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader().take(limit).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Télécharge, vérifie et installe la nouvelle version, puis relance l'app.
pub fn apply(rel: &Release) -> Result<(), String> {
    let exe = download(&rel.exe_url, 200 * 1024 * 1024)?;
    let sha_text = String::from_utf8_lossy(&download(&rel.sha_url, 4096)?).to_string();
    let expected = sha_text.split_whitespace().next().unwrap_or("").to_lowercase();
    let actual: String = Sha256::digest(&exe).iter().map(|b| format!("{b:02x}")).collect();
    if expected.len() != 64 || expected != actual {
        return Err("le fichier téléchargé ne correspond pas à son SHA-256 : mise à jour annulée".into());
    }
    if exe.len() < 2 || &exe[..2] != b"MZ" {
        return Err("le fichier téléchargé n'est pas un exécutable Windows".into());
    }
    let tmp = std::env::temp_dir().join(format!("Oxyde-{}.exe", rel.version));
    std::fs::write(&tmp, &exe).map_err(|e| e.to_string())?;
    self_replace::self_replace(&tmp).map_err(|e| format!("remplacement impossible : {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(me).spawn().map_err(|e| e.to_string())?;
    std::process::exit(0);
}
