//! Le pipeline d'installation unique, utilisé par le magasin, les .jar déposés et les modpacks.
//!
//! Pour un mod Java, dans l'ordre :
//! 1. la version Horizon officielle du registre (complète, code Rust compris) ;
//! 2. le portage officiel de Horizon via son launcher (`mods install`), pour les mods qui ont une version 26.3 ;
//! 3. le convertisseur d'Oxyde, en dernier recours (contenu seulement, approximatif).

use crate::convert::{self, Options};
use crate::engine::Engine;
use crate::{modrinth, pack, registry};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Target {
    /// Projet du registre Horizon (id).
    Horizon(String),
    /// Mod Java sur Modrinth (slug ou id).
    Modrinth { slug: String, title: String },
    /// Fichier .jar local.
    Jar(PathBuf),
    /// Mod Horizon déjà prêt (.hmod).
    Hmod(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Native,
    Official,
    Oxyde,
    File,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Native => "Version Horizon officielle",
            Method::Official => "Porté par Horizon",
            Method::Oxyde => "Converti par Oxyde",
            Method::File => "Mod Horizon",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub method: Method,
    pub title: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Clone)]
pub struct Ctx {
    pub index: Option<Arc<registry::Index>>,
    pub launcher: Option<PathBuf>,
    pub game_dir: PathBuf,
    /// Profil du launcher Horizon où installer (None = profil principal).
    pub instance: Option<String>,
    pub out_dir: PathBuf,
    pub opts: Options,
}

impl Ctx {
    fn engine(&self) -> Option<Engine> {
        self.launcher.clone().map(|exe| Engine { exe })
    }
}

pub fn install(t: &Target, ctx: &Ctx, log: &mut dyn FnMut(&str)) -> Result<Outcome, String> {
    match t {
        Target::Horizon(id) => {
            let idx = ctx.index.as_ref().ok_or("registre Horizon pas encore chargé")?;
            let p = idx.projects.iter().find(|p| &p.id == id).ok_or_else(|| format!("« {id} » n'est pas dans le registre Horizon"))?;
            log(&format!("Téléchargement de {}…", p.name));
            let msg = registry::install(idx, p, &ctx.game_dir)?;
            Ok(Outcome { method: Method::Native, title: p.name.clone(), message: msg, details: vec![] })
        }
        Target::Modrinth { slug, title } => java(slug, title, None, ctx, log),
        Target::Jar(path) => {
            log("Identification du mod…");
            // 1. par empreinte (le fichier exact vient de Modrinth)
            // 2. sinon par le nom écrit dans le .jar (fichier CurseForge, autre version…)
            let found = modrinth::identify(path).or_else(|| {
                let (files, _) = convert::read_zip(path).ok()?;
                let meta = convert::read_meta(&files, path);
                log(&format!("Empreinte inconnue, recherche de « {} »…", meta.name));
                modrinth::find_by_name(&meta.name, &meta.id)
            });
            match found {
                Some(info) => {
                    log(&format!("Reconnu : {} (Modrinth)", info.title));
                    java(&info.slug, &info.title, Some(path), ctx, log)
                }
                None => {
                    log("Mod introuvable en ligne : conversion par Oxyde.");
                    oxyde(path, None, ctx, log)
                }
            }
        }
        Target::Hmod(path) => {
            let ids = pack::install_mods(std::slice::from_ref(path), &ctx.game_dir)?;
            Ok(Outcome { method: Method::File, title: ids.join(", "), message: "installé et activé".into(), details: vec![] })
        }
    }
}

fn java(slug: &str, title: &str, local: Option<&Path>, ctx: &Ctx, log: &mut dyn FnMut(&str)) -> Result<Outcome, String> {
    // 1. version Horizon officielle
    if let Some(idx) = &ctx.index {
        if let Some(p) = idx.find_port(slug, title) {
            log(&format!("Version Horizon officielle trouvée : {}", p.name));
            let msg = registry::install(idx, p, &ctx.game_dir)?;
            return Ok(Outcome {
                method: Method::Native,
                title: p.name.clone(),
                message: msg,
                details: vec!["Version complète réécrite en Rust pour Horizon, code compris.".into()],
            });
        }
    }
    // 2. portage officiel de Horizon
    if let Some(engine) = ctx.engine() {
        log("Analyse par le launcher Horizon…");
        match engine.check(slug) {
            Ok(c) if c.portable() => {
                log(&format!("Horizon peut le porter : {}", c.summary));
                let out = engine.install(slug, ctx.instance.as_deref(), &mut |l| log(l))?;
                let mut details: Vec<String> = c.ports.iter().map(|p| format!("✔ {p}")).collect();
                details.extend(c.skipped.iter().map(|s| format!("✖ {s}")));
                let first = out.lines().next().unwrap_or("").split(" (SHA").next().unwrap_or("").to_string();
                return Ok(Outcome {
                    method: Method::Official,
                    title: if c.name.is_empty() { title.to_string() } else { c.name.clone() },
                    message: if c.verdict == "partly" { format!("porté en partie et activé · {first}") } else { format!("porté et activé · {first}") },
                    details,
                });
            }
            Ok(c) => return Err(format!("{} : {}", if c.name.is_empty() { title } else { &c.name }, c.summary)),
            Err(e) => log(&format!("Pas de portage officiel ({e}). Conversion par Oxyde.")),
        }
    }
    // 3. convertisseur Oxyde
    let jar = match local {
        Some(p) => p.to_path_buf(),
        None => {
            log("Téléchargement du .jar sur Modrinth…");
            modrinth::download_jar(slug)?
        }
    };
    oxyde(&jar, Some(slug), ctx, log)
}

fn oxyde(jar: &Path, slug: Option<&str>, ctx: &Ctx, log: &mut dyn FnMut(&str)) -> Result<Outcome, String> {
    log("Conversion du contenu…");
    let rep = convert::convert_jar(jar, &ctx.out_dir, &ctx.opts)?;
    pack::install_mods(std::slice::from_ref(&rep.output), &ctx.game_dir)?;
    if let Some(slug) = slug {
        crate::game::record_source(&ctx.game_dir, &rep.meta.id, slug);
    }
    let mut details = vec![format!(
        "{} fichiers, {} bloc(s), {} objet(s), {} chemin(s) mis à jour",
        rep.kept, rep.blocks, rep.items, rep.renamed
    )];
    if rep.item_models > 0 {
        details.push(format!("{} définition(s) d'objet ajoutée(s)", rep.item_models));
    }
    if !rep.namespaces.is_empty() {
        details.push(format!("namespaces en plus : {}", rep.namespaces.join(", ")));
    }
    if rep.has_code {
        details.push("✖ le code Java du mod n'est pas porté".into());
    }
    details.extend(rep.warnings.iter().map(|w| format!("⚠ {w}")));
    if !rep.dropped.is_empty() {
        details.push(format!("{} fichier(s) ignoré(s)", rep.dropped.len()));
    }
    Ok(Outcome {
        method: Method::Oxyde,
        title: rep.meta.name.clone(),
        message: "contenu converti et activé (conversion approximative)".into(),
        details,
    })
}
