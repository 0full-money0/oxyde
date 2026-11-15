//! Le moteur officiel : le launcher Horizon en ligne de commande (`horizon-launcher`).
//! Il porte les mods Java avec la chaîne officielle de Horizon (horizon-port-pack) :
//! blocs copiés sur le bloc vanilla le plus proche, recettes et loot mis à jour en 26.3, rapport précis.
//! Doc : docs/launcher-commands.md et docs/mod-browser.md du dépôt Horizon.

use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Engine {
    pub exe: PathBuf,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Check {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub skipped: Vec<String>,
}

impl Check {
    /// Le mod a quelque chose à porter (« partly » ou mieux).
    pub fn portable(&self) -> bool {
        !matches!(self.verdict.as_str(), "no" | "")
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct InstalledEntry {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub file: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Instance {
    #[serde(default)]
    pub kind: String,
    pub home: PathBuf,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PortedPack {
    pub pack: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub portable: u32,
    #[serde(default)]
    pub ported_mods: u32,
}

/// Lit soit un tableau JSON, soit des objets JSON à la suite.
fn parse_many<T: serde::de::DeserializeOwned>(text: &str) -> Vec<T> {
    let t = text.trim();
    if t.starts_with('[') {
        return serde_json::from_str(t).unwrap_or_default();
    }
    serde_json::Deserializer::from_str(t).into_iter::<T>().filter_map(Result::ok).collect()
}

impl Engine {
    pub fn find() -> Option<Engine> {
        crate::game::launcher_exe().map(|exe| Engine { exe })
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(&self.exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // pas de fenêtre console
        }
        c.stdin(Stdio::null());
        c
    }

    /// Lance une commande, envoie chaque ligne de sortie à `line`, coupe après `timeout`.
    fn run(&self, args: &[&str], timeout: Duration, line: &mut dyn FnMut(&str)) -> Result<String, String> {
        let mut child = self
            .cmd()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("launcher Horizon : {e}"))?;
        let err = child.stderr.take().unwrap();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                s.push_str(&l);
                s.push('\n');
            }
            s
        });
        let out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let start = Instant::now();
        let mut all = String::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(l) => {
                    line(&l);
                    all.push_str(&l);
                    all.push('\n');
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                return Err("le launcher Horizon met trop de temps à répondre".into());
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let err = err_thread.join().unwrap_or_default();
        if status.success() {
            Ok(all)
        } else {
            let msg = err
                .lines()
                .chain(all.lines())
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("erreur inconnue")
                .trim()
                .trim_start_matches("error:")
                .trim_start_matches("Error:")
                .trim()
                .to_string();
            Err(msg)
        }
    }

    /// Commande silencieuse, réessayée une fois (le launcher peut être occupé un instant).
    fn quiet(&self, args: &[&str], timeout: Duration) -> Result<String, String> {
        self.run(args, timeout, &mut |_| {}).or_else(|_| {
            std::thread::sleep(Duration::from_millis(600));
            self.run(args, timeout, &mut |_| {})
        })
    }

    /// Lecture JSON : `None` si le launcher n'a pas répondu (on garde alors l'affichage précédent).
    fn list<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Option<Vec<T>> {
        self.quiet(args, Duration::from_secs(60)).ok().map(|o| parse_many(&o))
    }

    /// Ce que la chaîne officielle peut porter d'un mod Modrinth (26.3).
    pub fn check(&self, slug: &str) -> Result<Check, String> {
        let out = self.quiet(&["mods", "check", slug, "--source", "modrinth", "--json"], Duration::from_secs(240))?;
        serde_json::from_str(out.trim()).map_err(|_| out.lines().last().unwrap_or("réponse illisible").to_string())
    }

    /// Porte et installe un mod Modrinth dans l'instance choisie (par défaut la principale).
    pub fn install(&self, slug: &str, instance: Option<&str>, line: &mut dyn FnMut(&str)) -> Result<String, String> {
        let mut args = vec!["mods", "install", slug, "--source", "modrinth"];
        if let Some(i) = instance {
            args.extend(["--instance", i]);
        }
        self.run(&args, Duration::from_secs(600), line)
    }

    /// Modpack du registre Horizon (.hpack) : installé par le launcher comme profil à part.
    pub fn install_horizon_pack(&self, id: &str, line: &mut dyn FnMut(&str)) -> Result<String, String> {
        let out = self.run(&["mods", "install", id, "--source", "horizon"], Duration::from_secs(1800), line)?;
        Ok(out.lines().next().unwrap_or("installé").to_string())
    }

    pub fn remove(&self, slug: &str) -> Result<String, String> {
        self.quiet(&["mods", "remove", slug], Duration::from_secs(120))
    }

    pub fn installed(&self) -> Option<Vec<InstalledEntry>> {
        self.list(&["mods", "list", "--json"])
    }

    pub fn instances(&self) -> Option<Vec<Instance>> {
        self.list(&["instance", "list", "--json"])
    }

    pub fn packs(&self) -> Option<Vec<PortedPack>> {
        self.list(&["modpack", "list", "--json"])
    }

    /// Importe puis porte un modpack Java (.mrpack, zip CurseForge ou dossier d'instance). Renvoie son id.
    pub fn import_pack(&self, path: &Path, line: &mut dyn FnMut(&str)) -> Result<String, String> {
        let before: Vec<String> = self.packs().unwrap_or_default().into_iter().map(|p| p.pack).collect();
        let p = path.to_string_lossy().to_string();
        self.run(&["modpack", "import", &p], Duration::from_secs(900), line)?;
        let after = self.packs().unwrap_or_default();
        let id = after
            .iter()
            .find(|x| !before.contains(&x.pack))
            .or_else(|| after.last())
            .map(|x| x.pack.clone())
            .ok_or("le modpack n'a pas été importé")?;
        line("Téléchargement et portage des mods…");
        self.run(&["modpack", "port", &id], Duration::from_secs(3600), line)?;
        Ok(id)
    }

    pub fn play(&self, instance: Option<&str>) -> Result<(), String> {
        let mut c = self.cmd();
        c.arg("play");
        if let Some(i) = instance {
            c.args(["--instance", i]);
        }
        let mut child = c.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("impossible de lancer Horizon : {e}"))?;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(Some(status)) = child.try_wait() {
                if status.success() {
                    return Ok(());
                }
                let mut err = String::new();
                if let Some(s) = child.stderr.take() {
                    for l in BufReader::new(s).lines().map_while(Result::ok) {
                        err = l;
                    }
                }
                return Err(if err.trim().is_empty() {
                    "Horizon s'est arrêté au démarrage. Ouvre le launcher Horizon pour te connecter avec ton compte Microsoft.".into()
                } else {
                    format!("Horizon : {}", err.trim())
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_shapes() {
        let one = r#"{"pack":"a","name":"A"}"#;
        let two = r#"{"pack":"a","name":"A"}
{"pack":"b","name":"B"}"#;
        let arr = r#"[{"pack":"a","name":"A"},{"pack":"b","name":"B"}]"#;
        assert_eq!(parse_many::<PortedPack>(one).len(), 1);
        assert_eq!(parse_many::<PortedPack>(two).len(), 2);
        assert_eq!(parse_many::<PortedPack>(arr).len(), 2);
        let c: Check = serde_json::from_str(r#"{"verdict":"no","summary":"x"}"#).unwrap();
        assert!(!c.portable());
    }
}
