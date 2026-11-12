//! Conversion d'un mod Java (.jar Fabric / Quilt / Forge / NeoForge) en mod de contenu Horizon (.hmod).
//!
//! Horizon n'exécute pas de code Java : seuls les fichiers `assets/` et `data/` passent.
//! Les règles appliquées ici suivent la doc de Horizon (docs/mods.md, docs/api/mod-files.md) :
//! - manifeste `horizon-mod.toml` (api 0.2, capability `registries`) ;
//! - fichiers seulement dans les namespaces du mod, plus les tags `minecraft`/`c` et les atlas vanilla ;
//! - chemins en `[a-z0-9_.-]`, 16 Mio max par fichier, 65 536 fichiers et 512 Mio au total ;
//! - blocs et objets déclarés dans `data/<id>/horizon/{block,item}/*.json` à partir d'un modèle vanilla.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_FILE: u64 = 16 * 1024 * 1024;
pub const MAX_FILES: usize = 65_536;
pub const MAX_TOTAL: u64 = 512 * 1024 * 1024;

/// Namespaces des loaders Java : leurs fichiers n'ont pas de sens dans Horizon.
const LOADER_NAMESPACES: &[&str] = &["forge", "neoforge", "fabric", "fabric-api", "quilt", "architectury"];

#[derive(Debug, Clone)]
pub struct Options {
    /// Génère les déclarations de blocs / objets à partir des modèles trouvés.
    pub generate_registries: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { generate_registries: true }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub loader: String,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub meta: ModMeta,
    pub output: PathBuf,
    pub kept: usize,
    pub renamed: usize,
    pub blocks: usize,
    pub items: usize,
    pub item_models: usize,
    pub has_code: bool,
    pub namespaces: Vec<String>,
    pub dropped: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Métadonnées
// ---------------------------------------------------------------------------

pub fn sanitize_id(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    let mut out: String = out.chars().take(64).collect();
    out = out.trim_end_matches('_').to_string();
    if out.is_empty() {
        "mod".to_string()
    } else {
        out
    }
}

fn is_valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn str_list(v: Option<&serde_json::Value>) -> Vec<String> {
    match v {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|x| match x {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(o) => o.get("name").and_then(|n| n.as_str()).map(String::from),
                _ => None,
            })
            .collect(),
        Some(serde_json::Value::Object(o)) => o.keys().cloned().collect(),
        _ => vec![],
    }
}

fn jsonc(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn manifest_version(mf: Option<&str>) -> Option<String> {
    mf?.lines()
        .find_map(|l| l.strip_prefix("Implementation-Version:"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_mods_toml(text: &str) -> Option<ModMeta> {
    let v: toml::Value = toml::from_str(text).ok()?;
    let m = v.get("mods")?.as_array()?.first()?;
    let s = |k: &str| m.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    Some(ModMeta {
        id: s("modId"),
        name: s("displayName"),
        version: s("version"),
        description: s("description"),
        authors: {
            let a = s("authors");
            if a.is_empty() { vec![] } else { vec![a] }
        },
        loader: String::new(),
    })
}

/// Lit les métadonnées du jar (Fabric, Quilt, NeoForge, Forge), avec repli sur le nom du fichier.
pub fn read_meta(files: &BTreeMap<String, Vec<u8>>, jar: &Path) -> ModMeta {
    let text = |p: &str| files.get(p).map(|b| String::from_utf8_lossy(b).into_owned());
    let mf = text("META-INF/MANIFEST.MF");
    let mut meta = ModMeta::default();

    if let Some(v) = text("fabric.mod.json").as_deref().and_then(jsonc) {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        meta = ModMeta {
            id: s("id"),
            name: s("name"),
            version: s("version"),
            description: s("description"),
            authors: str_list(v.get("authors")),
            loader: "Fabric".into(),
        };
    } else if let Some(v) = text("quilt.mod.json").as_deref().and_then(jsonc) {
        let ql = v.get("quilt_loader");
        let md = ql.and_then(|q| q.get("metadata"));
        let s = |o: Option<&serde_json::Value>, k: &str| {
            o.and_then(|o| o.get(k)).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        meta = ModMeta {
            id: s(ql, "id"),
            name: s(md, "name"),
            version: s(ql, "version"),
            description: s(md, "description"),
            authors: str_list(md.and_then(|m| m.get("contributors"))),
            loader: "Quilt".into(),
        };
    } else if let Some(m) = text("META-INF/neoforge.mods.toml").as_deref().and_then(parse_mods_toml) {
        meta = ModMeta { loader: "NeoForge".into(), ..m };
    } else if let Some(m) = text("META-INF/mods.toml").as_deref().and_then(parse_mods_toml) {
        meta = ModMeta { loader: "Forge".into(), ..m };
    } else if files.contains_key("pack.mcmeta") {
        meta.loader = "Data pack".into();
    } else {
        meta.loader = "Inconnu".into();
    }

    let stem = jar.file_stem().and_then(|s| s.to_str()).unwrap_or("mod").to_string();
    if meta.id.trim().is_empty() {
        // "create-1.21.1-6.0.4" -> "create"
        let base: Vec<&str> = stem
            .split(['-', '+', ' '])
            .take_while(|p| !p.chars().next().is_some_and(|c| c.is_ascii_digit()))
            .collect();
        meta.id = if base.is_empty() { stem.clone() } else { base.join("_") };
    }
    if meta.version.contains("${") || meta.version.trim().is_empty() {
        meta.version = manifest_version(mf.as_deref()).unwrap_or_else(|| "1.0.0".into());
    }
    if meta.name.trim().is_empty() {
        meta.name = meta.id.clone();
    }
    meta.description = meta.description.trim().to_string();
    meta
}

// ---------------------------------------------------------------------------
// Chemins
// ---------------------------------------------------------------------------

fn valid_part(p: &str) -> bool {
    !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-'))
}

/// Renomme les dossiers des anciennes versions (≤ 1.20) vers les noms de 1.21+ / 26.3.
fn upgrade_data_path(parts: &mut [String]) -> bool {
    if parts.len() < 4 || parts[0] != "data" {
        return false;
    }
    let renamed = match parts[2].as_str() {
        "recipes" => Some("recipe"),
        "loot_tables" => Some("loot_table"),
        "advancements" => Some("advancement"),
        "functions" => Some("function"),
        "structures" => Some("structure"),
        "predicates" => Some("predicate"),
        "item_modifiers" => Some("item_modifier"),
        _ => None,
    };
    let mut changed = false;
    if let Some(r) = renamed {
        parts[2] = r.to_string();
        changed = true;
    }
    if parts[2] == "tags" && parts.len() >= 5 {
        let t = match parts[3].as_str() {
            "blocks" => Some("block"),
            "items" => Some("item"),
            "entity_types" => Some("entity_type"),
            "fluids" => Some("fluid"),
            "game_events" => Some("game_event"),
            "functions" => Some("function"),
            _ => None,
        };
        if let Some(t) = t {
            parts[3] = t.to_string();
            changed = true;
        }
    }
    changed
}

enum Verdict {
    Keep(String, Option<String>), // chemin final, namespace propre au mod
    Drop(&'static str),
}

fn classify(path: &str) -> (Verdict, bool) {
    let mut parts: Vec<String> = path.split('/').map(String::from).collect();
    if parts.len() < 3 {
        return (Verdict::Drop("chemin trop court"), false);
    }
    if !parts[1..].iter().all(|p| valid_part(p)) {
        return (Verdict::Drop("nom de fichier invalide (majuscules, espaces ou caractères spéciaux)"), false);
    }
    let renamed = upgrade_data_path(&mut parts);
    let root = parts[0].as_str();
    let ns = parts[1].as_str();
    let sub = parts[2].as_str();
    let verdict = match ns {
        "minecraft" => {
            if (root == "data" && sub == "tags") || (root == "assets" && sub == "atlases") {
                Verdict::Keep(parts.join("/"), None)
            } else {
                Verdict::Drop("remplace un fichier vanilla (interdit par Horizon)")
            }
        }
        "c" => {
            if root == "data" && sub == "tags" {
                Verdict::Keep(parts.join("/"), None)
            } else {
                Verdict::Drop("namespace « c » réservé aux tags communs")
            }
        }
        "horizon" => Verdict::Drop("namespace « horizon » réservé"),
        n if LOADER_NAMESPACES.contains(&n) => Verdict::Drop("fichier propre au loader Java"),
        n => Verdict::Keep(parts.join("/"), Some(n.to_string())),
    };
    (verdict, renamed)
}

// ---------------------------------------------------------------------------
// Modèles vanilla pour les blocs / objets
// ---------------------------------------------------------------------------

pub fn block_template(name: &str) -> &'static str {
    let ends = |s: &str| name.ends_with(s);
    let has = |s: &str| name.contains(s);
    if ends("_stairs") { "minecraft:stone_stairs" }
    else if ends("_slab") { "minecraft:stone_slab" }
    else if ends("_wall") { "minecraft:cobblestone_wall" }
    else if ends("_fence_gate") { "minecraft:oak_fence_gate" }
    else if ends("_fence") { "minecraft:oak_fence" }
    else if ends("_trapdoor") { "minecraft:oak_trapdoor" }
    else if ends("_door") { "minecraft:oak_door" }
    else if ends("_pressure_plate") { "minecraft:stone_pressure_plate" }
    else if ends("_button") { "minecraft:stone_button" }
    else if ends("_pane") { "minecraft:glass_pane" }
    else if ends("_glass") { "minecraft:glass" }
    else if ends("_leaves") { "minecraft:oak_leaves" }
    else if ends("_sapling") { "minecraft:oak_sapling" }
    else if ends("_log") || ends("_wood") || ends("_stem") || ends("_hyphae") || ends("_pillar") { "minecraft:oak_log" }
    else if ends("_planks") { "minecraft:oak_planks" }
    else if has("deepslate") && ends("_ore") { "minecraft:deepslate_iron_ore" }
    else if ends("_ore") { "minecraft:iron_ore" }
    else if ends("_carpet") { "minecraft:white_carpet" }
    else if ends("_wool") { "minecraft:white_wool" }
    else if ends("_lamp") || has("lantern") { "minecraft:sea_lantern" }
    else if ends("_bricks") || ends("_brick") || ends("_tiles") { "minecraft:stone_bricks" }
    else if ends("_block") && (has("iron") || has("steel") || has("copper") || has("metal") || has("gold") || has("ingot")) { "minecraft:iron_block" }
    else if ends("_dirt") || ends("_soil") { "minecraft:dirt" }
    else if ends("_sand") { "minecraft:sand" }
    else { "minecraft:stone" }
}

pub fn item_template(name: &str) -> &'static str {
    let ends = |s: &str| name.ends_with(s);
    if ends("_sword") { "minecraft:iron_sword" }
    else if ends("_pickaxe") { "minecraft:iron_pickaxe" }
    else if ends("_axe") { "minecraft:iron_axe" }
    else if ends("_shovel") || ends("_spade") { "minecraft:iron_shovel" }
    else if ends("_hoe") { "minecraft:iron_hoe" }
    else if ends("_helmet") { "minecraft:iron_helmet" }
    else if ends("_chestplate") { "minecraft:iron_chestplate" }
    else if ends("_leggings") { "minecraft:iron_leggings" }
    else if ends("_boots") { "minecraft:iron_boots" }
    else if ends("_ingot") { "minecraft:iron_ingot" }
    else if ends("_nugget") { "minecraft:iron_nugget" }
    else if name.starts_with("raw_") { "minecraft:raw_iron" }
    else if ends("_bucket") { "minecraft:bucket" }
    else if ends("_stick") || ends("_rod") { "minecraft:stick" }
    else if ends("_seeds") { "minecraft:wheat_seeds" }
    else { "minecraft:diamond" }
}

// ---------------------------------------------------------------------------
// Manifeste
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct Manifest<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    description: &'a str,
    authors: &'a [String],
    api: &'a str,
    side: &'a str,
    capabilities: Vec<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    namespaces: Vec<String>,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Lit toutes les entrées d'une archive zip (jar) en mémoire.
type ZipContents = (BTreeMap<String, Vec<u8>>, Vec<(String, String)>);

pub fn read_zip(path: &Path) -> Result<ZipContents, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("impossible d'ouvrir le fichier : {e}"))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("ce n'est pas un .jar valide : {e}"))?;
    let mut files = BTreeMap::new();
    let mut dropped = Vec::new();
    for i in 0..zip.len() {
        let mut e = match zip.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                dropped.push((format!("#{i}"), format!("entrée illisible : {err}")));
                continue;
            }
        };
        if e.is_dir() {
            continue;
        }
        let name = e.name().replace('\\', "/");
        let interesting = name.starts_with("assets/")
            || name.starts_with("data/")
            || matches!(
                name.as_str(),
                "fabric.mod.json" | "quilt.mod.json" | "META-INF/mods.toml" | "META-INF/neoforge.mods.toml" | "META-INF/MANIFEST.MF" | "pack.mcmeta"
            );
        if name.ends_with(".class") {
            files.insert(name, Vec::new()); // on ne garde que la présence du code
            continue;
        }
        if !interesting {
            continue;
        }
        if e.size() > MAX_FILE {
            dropped.push((name, "fichier de plus de 16 Mio".into()));
            continue;
        }
        let mut buf = Vec::with_capacity(e.size() as usize);
        if let Err(err) = e.read_to_end(&mut buf) {
            dropped.push((name, format!("lecture impossible : {err}")));
            continue;
        }
        files.insert(name, buf);
    }
    Ok((files, dropped))
}

pub fn convert_jar(jar: &Path, out_dir: &Path, opts: &Options) -> Result<Report, String> {
    let (files, mut dropped) = read_zip(jar)?;
    let mut meta = read_meta(&files, jar);
    let has_code = files.keys().any(|k| k.ends_with(".class"));

    // 1. Tri des fichiers
    let mut kept: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut ns_count: BTreeMap<String, usize> = BTreeMap::new();
    let mut renamed = 0;
    for (path, bytes) in &files {
        if !(path.starts_with("assets/") || path.starts_with("data/")) {
            continue;
        }
        match classify(path) {
            (Verdict::Keep(p, ns), was_renamed) => {
                if kept.contains_key(&p) {
                    dropped.push((path.clone(), "doublon après mise à jour du chemin".into()));
                    continue;
                }
                if let Some(ns) = ns {
                    *ns_count.entry(ns).or_default() += 1;
                }
                if was_renamed {
                    renamed += 1;
                }
                kept.insert(p, bytes.clone());
            }
            (Verdict::Drop(reason), _) => dropped.push((path.clone(), reason.into())),
        }
    }

    if kept.is_empty() {
        return Err(if has_code {
            "ce mod ne contient que du code Java, rien ne peut être converti pour Horizon".into()
        } else {
            "aucun fichier assets/ ou data/ dans ce jar".into()
        });
    }

    // 2. Choix de l'identifiant : le namespace principal s'il diffère de l'id Java
    let mut id = sanitize_id(&meta.id);
    if !ns_count.contains_key(&id) {
        if let Some((best, _)) = ns_count.iter().filter(|(n, _)| is_valid_id(n)).max_by_key(|(_, c)| **c) {
            id = best.clone();
        }
    }
    meta.id = id.clone();
    let namespaces: Vec<String> = ns_count.keys().filter(|n| **n != id).cloned().collect();

    // 3. Déclarations de blocs et objets (Horizon ne peut pas lire le code Java qui les enregistre)
    let mut blocks = 0;
    let mut items = 0;
    let mut item_models = 0;
    if opts.generate_registries {
        let leaf = |p: &str, prefix: &str| -> Option<String> {
            let rest = p.strip_prefix(prefix)?.strip_suffix(".json")?;
            (!rest.contains('/')).then(|| rest.to_string())
        };
        let a = format!("assets/{id}/");
        let block_names: BTreeSet<String> =
            kept.keys().filter_map(|p| leaf(p, &format!("{a}blockstates/"))).collect();
        let item_defs: BTreeSet<String> = kept.keys().filter_map(|p| leaf(p, &format!("{a}items/"))).collect();
        let item_model_names: BTreeSet<String> =
            kept.keys().filter_map(|p| leaf(p, &format!("{a}models/item/"))).collect();
        let block_model_names: BTreeSet<String> =
            kept.keys().filter_map(|p| leaf(p, &format!("{a}models/block/"))).collect();

        let mut gen: Vec<(String, Vec<u8>)> = Vec::new();
        for b in &block_names {
            let path = format!("data/{id}/horizon/block/{b}.json");
            if kept.contains_key(&path) {
                continue;
            }
            let json = serde_json::json!({ "template": block_template(b), "item": true });
            gen.push((path, serde_json::to_vec_pretty(&json).unwrap()));
            blocks += 1;
        }
        let all_items: BTreeSet<&String> = item_defs.iter().chain(item_model_names.iter()).collect();
        for it in all_items {
            if block_names.contains(it) || it.ends_with("_spawn_egg") {
                continue;
            }
            let path = format!("data/{id}/horizon/item/{it}.json");
            if kept.contains_key(&path) {
                continue;
            }
            let json = serde_json::json!({ "template": item_template(it) });
            gen.push((path, serde_json::to_vec_pretty(&json).unwrap()));
            items += 1;
        }
        // Définitions d'objets (assets/<ns>/items/*.json), obligatoires depuis 1.21.4
        let need_defs = item_model_names.iter().chain(block_names.iter()).collect::<BTreeSet<_>>();
        for n in need_defs {
            if item_defs.contains(n) {
                continue;
            }
            let model = if item_model_names.contains(n) {
                format!("{id}:item/{n}")
            } else if block_model_names.contains(n) {
                format!("{id}:block/{n}")
            } else {
                continue;
            };
            let json = serde_json::json!({ "model": { "type": "minecraft:model", "model": model } });
            gen.push((format!("{a}items/{n}.json"), serde_json::to_vec_pretty(&json).unwrap()));
            item_models += 1;
        }
        kept.extend(gen);
    }

    // 4. Limites
    let mut warnings = Vec::new();
    if kept.len() > MAX_FILES {
        return Err(format!("trop de fichiers ({}, max {MAX_FILES})", kept.len()));
    }
    let total: u64 = kept.values().map(|b| b.len() as u64).sum();
    if total > MAX_TOTAL {
        return Err("contenu de plus de 512 Mio".into());
    }
    if has_code {
        warnings.push(
            "Ce mod contient du code Java : les comportements spéciaux (machines, interfaces, entités, événements) ne seront pas présents. Seuls les textures, modèles, recettes, tags, loot et structures sont convertis."
                .into(),
        );
    }
    if blocks + items > 0 {
        warnings.push(format!(
            "{blocks} bloc(s) et {items} objet(s) ont été déclarés avec un modèle vanilla deviné d'après leur nom : vérifie-les en jeu."
        ));
    }
    if meta.loader == "Data pack" {
        warnings.push("Ce jar est un data pack : ses fichiers minecraft/ ont été ignorés.".into());
    }

    // 5. Écriture du .hmod
    let has_data = kept.keys().any(|k| k.starts_with("data/"));
    let manifest = Manifest {
        id: &id,
        name: &meta.name,
        version: &meta.version,
        description: if meta.description.is_empty() { "Converti depuis un mod Java avec Oxyde." } else { &meta.description },
        authors: &meta.authors,
        api: "0.2",
        side: if has_data { "both" } else { "client" },
        capabilities: vec!["registries"],
        namespaces: namespaces.clone(),
    };
    let manifest_text = toml::to_string(&manifest).map_err(|e| e.to_string())?;

    std::fs::create_dir_all(out_dir).map_err(|e| format!("dossier de sortie : {e}"))?;
    let output = out_dir.join(format!("{id}.hmod"));
    write_hmod(&output, &manifest_text, &kept).map_err(|e| format!("écriture du .hmod : {e}"))?;

    Ok(Report {
        meta,
        output,
        kept: kept.len(),
        renamed,
        blocks,
        items,
        item_models,
        has_code,
        namespaces,
        dropped,
        warnings,
    })
}

fn write_hmod(out: &Path, manifest: &str, files: &BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    let tmp = out.with_extension("hmod.tmp");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut z = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("horizon-mod.toml", o)?;
        z.write_all(manifest.as_bytes())?;
        for (p, b) in files {
            z.start_file(p.as_str(), o)?;
            z.write_all(b)?;
        }
        z.finish()?;
    }
    std::fs::rename(&tmp, out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn make_jar(path: &Path, entries: &[(&str, &str)]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in entries {
            z.start_file(*n, o).unwrap();
            z.write_all(c.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn ids() {
        assert_eq!(sanitize_id("My-Cool.Mod"), "my_cool_mod");
        assert_eq!(sanitize_id("__"), "mod");
        assert_eq!(block_template("ruby_stairs"), "minecraft:stone_stairs");
        assert_eq!(item_template("ruby_pickaxe"), "minecraft:iron_pickaxe");
    }

    #[test]
    fn converts_fabric_jar() {
        let dir = std::env::temp_dir().join(format!("hmc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("ruby-1.0.jar");
        make_jar(
            &jar,
            &[
                ("fabric.mod.json", r#"{"id":"ruby","name":"Ruby","version":"1.2.3","authors":["Bob",{"name":"Ann"}]}"#),
                ("com/x/Ruby.class", "x"),
                ("assets/ruby/blockstates/ruby_block.json", "{}"),
                ("assets/ruby/models/block/ruby_block.json", "{}"),
                ("assets/ruby/models/item/ruby.json", "{}"),
                ("assets/ruby/textures/item/ruby.png", "png"),
                ("assets/ruby/lang/en_us.json", "{}"),
                ("assets/ruby/textures/Bad Name.png", "x"),
                ("assets/minecraft/textures/block/stone.png", "x"),
                ("data/ruby/recipes/ruby_block.json", "{}"),
                ("data/minecraft/tags/blocks/mineable/pickaxe.json", "{}"),
                ("data/forge/tags/items/gems.json", "{}"),
                ("data/other_ns/loot_tables/x.json", "{}"),
            ],
        );
        let r = convert_jar(&jar, &dir, &Options::default()).unwrap();
        assert_eq!(r.meta.id, "ruby");
        assert_eq!(r.meta.authors, vec!["Bob", "Ann"]);
        assert_eq!(r.blocks, 1);
        assert_eq!(r.items, 1);
        assert_eq!(r.namespaces, vec!["other_ns"]);
        assert!(r.has_code);
        assert_eq!(r.dropped.len(), 3);

        let (out, _) = read_zip(&r.output).unwrap();
        assert!(out.contains_key("data/ruby/recipe/ruby_block.json"));
        assert!(out.contains_key("data/minecraft/tags/block/mineable/pickaxe.json"));
        assert!(out.contains_key("data/ruby/horizon/block/ruby_block.json"));
        assert!(out.contains_key("data/ruby/horizon/item/ruby.json"));
        assert!(out.contains_key("assets/ruby/items/ruby.json"));
        assert!(out.contains_key("assets/ruby/items/ruby_block.json"));
        assert!(!out.contains_key("assets/minecraft/textures/block/stone.png"));
        let f = std::fs::File::open(&r.output).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut s = String::new();
        z.by_name("horizon-mod.toml").unwrap().read_to_string(&mut s).unwrap();
        let m: toml::Value = toml::from_str(&s).unwrap();
        assert_eq!(m["api"].as_str(), Some("0.2"));
        assert_eq!(m["side"].as_str(), Some("both"));
        assert_eq!(m["version"].as_str(), Some("1.2.3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn code_only_jar_is_refused() {
        let dir = std::env::temp_dir().join(format!("hmc-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("lib.jar");
        make_jar(&jar, &[("META-INF/neoforge.mods.toml", "[[mods]]\nmodId=\"lib\"\nversion=\"${file.jarVersion}\""), ("a/B.class", "x")]);
        assert!(convert_jar(&jar, &dir, &Options::default()).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
