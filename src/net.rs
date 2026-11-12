//! Réseau : un seul agent HTTP, téléchargements bornés et vérifiés.

use sha2::{Digest, Sha256, Sha512};
use std::io::Read;

pub const UA: &str = concat!(
    "0full-money0/oxyde/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/0full-money0/oxyde)"
);

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(120))
        .user_agent(UA)
        .build()
}

pub fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| format!("connexion impossible ({url}) : {e}"))?
        .into_json()
        .map_err(|e| format!("réponse illisible : {e}"))
}

pub fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| format!("téléchargement impossible : {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("téléchargement interrompu : {e}"))?;
    if buf.len() as u64 > limit {
        return Err("fichier trop gros".into());
    }
    Ok(buf)
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha512_hex(b: &[u8]) -> String {
    Sha512::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Encode une valeur de paramètre d'URL.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Nom de fichier sûr (pas de dossier, pas de caractère spécial).
pub fn safe_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 120
        && !n.starts_with('.')
        && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}
