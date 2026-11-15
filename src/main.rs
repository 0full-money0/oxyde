#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod curseforge;
mod engine;
mod fx;
mod game;
mod installer;
mod modrinth;
mod net;
mod pack;
mod perf;
mod registry;
mod setup;
mod theme;
mod tour;
mod update;
mod widgets;

use convert::Options;
use eframe::egui::{self, Color32, RichText};
use installer::{Method, Target};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use widgets::*;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = cli(&args) {
        std::process::exit(code);
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oxyde")
            .with_inner_size(window_size())
            .with_min_inner_size([820.0, 540.0])
            .with_decorations(false) // barre de titre dessinée par Oxyde
            .with_transparent(true)
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()),
        // une taille imposée (enregistrement) ne doit pas être remplacée par celle de la dernière session
        persist_window: std::env::var("OXYDE_SIZE").is_err(),
        ..Default::default()
    };
    eframe::run_native("Oxyde", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

/// Taille de la fenêtre au démarrage (OXYDE_SIZE=1440x900 pour les enregistrements).
fn window_size() -> [f32; 2] {
    std::env::var("OXYDE_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([1120.0, 740.0])
}

/// Ligne de commande :
///   oxyde --convert <dossier-sortie> <mod.jar>...
///   oxyde --install <horizon|modrinth|jar|pack> <id, slug ou fichier>
fn cli(args: &[String]) -> Option<i32> {
    match args.get(1).map(String::as_str) {
        Some("--convert") if args.len() >= 4 => {
            let out = PathBuf::from(&args[2]);
            let mut code = 0;
            for jar in &args[3..] {
                match convert::convert_jar(Path::new(jar), &out, &Options::default()) {
                    Ok(r) => println!("OK  {} -> {} ({} fichiers)", jar, r.output.display(), r.kept),
                    Err(e) => {
                        code = 1;
                        println!("ERR {jar}: {e}")
                    }
                }
            }
            Some(code)
        }
        Some("--install") if args.len() == 4 => {
            let index = registry::fetch().ok().map(Arc::new);
            let game_dir = main_game_dir();
            let ctx = installer::Ctx {
                index,
                launcher: game::launcher_exe(),
                game_dir,
                instance: None,
                out_dir: std::env::temp_dir().join("oxyde-converted"),
                opts: Options::default(),
            };
            let mut log = |l: &str| println!("  {l}");
            let r = match args[2].as_str() {
                "horizon" => installer::install(&Target::Horizon(args[3].clone()), &ctx, &mut log).map(|o| vec![o]),
                "modrinth" => installer::install(&Target::Modrinth { slug: args[3].clone(), title: args[3].clone() }, &ctx, &mut log).map(|o| vec![o]),
                "jar" => installer::install(&Target::Jar(PathBuf::from(&args[3])), &ctx, &mut log).map(|o| vec![o]),
                "pack" => install_pack_file(Path::new(&args[3]), &ctx, &mut log).map(|(_, v)| v),
                _ => Err("source : horizon, modrinth, jar ou pack".into()),
            };
            match &r {
                Ok(v) => {
                    for o in v {
                        println!("OK  [{}] {} : {}", o.method.label(), o.title, o.message);
                        for d in &o.details {
                            println!("      {d}");
                        }
                    }
                }
                Err(e) => println!("ERR {e}"),
            }
            Some(if r.is_ok() { 0 } else { 1 })
        }
        // règle le profil principal (ou le dossier donné) pour ce PC
        Some("--optimize") => {
            let dir = args.get(2).map(PathBuf::from).unwrap_or_else(main_game_dir);
            let m = perf::detect(&dir);
            println!("PC  {} ({:?})", m.gpu, m.tier);
            let c = perf::optimize(&dir, &m);
            if c.is_empty() {
                println!("OK  déjà réglé");
            }
            for l in c {
                println!("OK  {l}");
            }
            Some(0)
        }
        Some("--profiles") => {
            let Some(e) = engine::Engine::find() else {
                println!("ERR launcher Horizon introuvable");
                return Some(1);
            };
            for i in e.instances().unwrap_or_default() {
                println!("{}\t{}\t{}\t{}", i.id, i.kind, i.name, i.home.display());
            }
            Some(0)
        }
        Some("--new-profile") if args.len() == 3 => {
            let r = engine::Engine::find().ok_or("launcher Horizon introuvable".to_string()).and_then(|e| e.create_instance(&args[2]));
            match r {
                Ok(i) => {
                    println!("OK  {} ({}) {}", i.name, i.id, i.home.display());
                    Some(0)
                }
                Err(e) => {
                    println!("ERR {e}");
                    Some(1)
                }
            }
        }
        // installe Oxyde (OXYDE_INSTALL_ROOT pour un dossier de test), puis --uninstall
        Some("--setup") => match setup::install(true) {
            Ok(p) => {
                println!("OK  {}", p.display());
                Some(0)
            }
            Err(e) => {
                println!("ERR {e}");
                Some(1)
            }
        },
        Some("--uninstall") => match setup::uninstall() {
            Ok(()) => {
                println!("OK");
                Some(0)
            }
            Err(e) => {
                println!("ERR {e}");
                Some(1)
            }
        },
        _ => None,
    }
}

/// Dossier de jeu de l'instance principale (celle du launcher Horizon, sinon le dossier par défaut).
fn main_game_dir() -> PathBuf {
    if let Some(e) = engine::Engine::find() {
        let list = e.instances().unwrap_or_default();
        if let Some(i) = list.iter().find(|i| i.kind == "default").or_else(|| list.iter().find(|i| i.selected)) {
            return i.home.clone();
        }
    }
    game::default_game_dir()
}

/// Installe un modpack : .oxypack/.hzmods (Oxyde), .mrpack ou zip CurseForge (portage officiel).
/// Renvoie (instance à jouer si c'est un modpack porté à part, résultats).
fn install_pack_file(path: &Path, ctx: &installer::Ctx, log: &mut dyn FnMut(&str)) -> Result<(Option<String>, Vec<installer::Outcome>), String> {
    if is_ext(path, pack::PACK_EXT) || is_ext(path, pack::LEGACY_EXT) {
        let p = pack::read_pack(path)?;
        log(&format!("Modpack « {} » : {} mods", p.name, p.mods.len()));
        let mut done = vec![];
        let mut failed = vec![];
        let n = p.mods.len();
        for (i, m) in p.mods.iter().enumerate() {
            log(&format!("[{}/{n}] {}", i + 1, m.name()));
            let target = match m {
                pack::Entry::Horizon { id, .. } => Ok(Target::Horizon(id.clone())),
                pack::Entry::Modrinth { slug, name } => Ok(Target::Modrinth { slug: slug.clone(), title: name.clone() }),
                pack::Entry::File { file, sha256, .. } => pack::extract(path, file, sha256).map(Target::Hmod),
            };
            match target.and_then(|t| installer::install(&t, ctx, &mut |l| log(&format!("    {l}")))) {
                Ok(o) => {
                    log(&format!("    ✔ {} : {}", o.method.label(), o.message));
                    done.push(o);
                }
                Err(e) => {
                    log(&format!("    ✖ {e}"));
                    failed.push(format!("{} : {e}", m.name()));
                }
            }
        }
        if done.is_empty() && !failed.is_empty() {
            return Err(format!("aucun mod installé ({})", failed.join(" ; ")));
        }
        if !failed.is_empty() {
            done.push(installer::Outcome { method: Method::File, title: "Non installés".into(), message: format!("{} mod(s)", failed.len()), details: failed });
        }
        return Ok((None, done));
    }
    // modpack Java : chaîne officielle de Horizon, dans son propre profil
    let launcher = ctx.launcher.clone().ok_or("Pour les modpacks Java (.mrpack, CurseForge), installe le launcher Horizon : il les porte.")?;
    let engine = engine::Engine { exe: launcher };
    log("Import du modpack par Horizon…");
    let id = engine.import_pack(path, &mut |l| log(l))?;
    let pack = engine.packs().unwrap_or_default().into_iter().find(|p| p.pack == id);
    let msg = pack.as_ref().map(|p| format!("{} mod(s) portés sur {}", p.ported_mods, p.portable)).unwrap_or_default();
    Ok((
        Some(format!("port:{id}")),
        vec![installer::Outcome {
            method: Method::Official,
            title: pack.map(|p| p.name).unwrap_or(id),
            message: format!("modpack porté par Horizon · {msg}"),
            details: vec!["Il a son propre profil : lance-le depuis l'onglet Modpacks.".into()],
        }],
    ))
}

// ---------------------------------------------------------------------------
// État
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
struct Settings {
    output_dir: PathBuf,
    game_dir: Option<PathBuf>,
    generate_registries: bool,
    auto_update: bool,
    /// Profil choisi (id du launcher Horizon).
    profile: Option<String>,
    cf_key: String,
    skip_setup: bool,
    /// Mods Modrinth que Horizon a refusés (slug -> raison) : ils ne sont plus proposés à l'installation.
    not_portable: std::collections::BTreeMap<String, String>,
    /// Régler le jeu selon le PC avant chaque partie (évite les plantages « mémoire vidéo pleine »).
    optimize: bool,
    /// Réduire Oxyde pendant la partie : il ne prend plus rien au jeu.
    minimize_on_play: bool,
    /// Fond animé (désactivable sur les petits PC).
    animations: bool,
    /// Image choisie pour chaque profil (id -> copie gardée par Oxyde).
    profile_images: std::collections::BTreeMap<String, PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
        Self {
            output_dir: base.join("Oxyde"),
            game_dir: None,
            generate_registries: true,
            auto_update: true,
            profile: None,
            cf_key: String::new(),
            skip_setup: false,
            not_portable: Default::default(),
            optimize: true,
            minimize_on_play: true,
            animations: true,
            profile_images: Default::default(),
        }
    }
}

fn imports_dir() -> PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("Oxyde").join("Imports")
}

/// Écran de lancement du jeu.
struct Launch {
    name: String,
    id: Option<String>,
    started: f64,
    /// Réglages faits pour ce PC, carte graphique, mods actifs.
    prep: Option<(Vec<String>, String, usize)>,
    result: Option<Result<(), String>>,
    done_at: Option<f64>,
}

/// Fenêtre de création / modification d'un profil.
struct ProfileEdit {
    id: Option<String>,
    name: String,
    color: usize,
    image: Option<PathBuf>,
    confirm_delete: bool,
    busy: bool,
}

/// Couleurs proposées pour les profils (reprises par le launcher Horizon).
const PROFILE_COLORS: [(u8, u8, u8); 8] = [(214, 118, 64), (186, 76, 60), (92, 182, 155), (72, 140, 172), (138, 110, 178), (210, 168, 72), (118, 150, 82), (128, 122, 112)];

fn color_hex(i: usize) -> String {
    let (r, g, b) = PROFILE_COLORS[i % PROFILE_COLORS.len()];
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Couleur d'un profil : celle du launcher si c'en est une, sinon tirée de son id.
fn profile_color_index(p: &engine::Instance) -> usize {
    if let Some(hex) = p.icon.as_deref().filter(|h| h.starts_with('#')) {
        if let Some(i) = (0..PROFILE_COLORS.len()).find(|&i| color_hex(i).eq_ignore_ascii_case(hex)) {
            return i;
        }
    }
    p.id.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % PROFILE_COLORS.len()
}

fn profile_color(i: usize) -> Color32 {
    let (r, g, b) = PROFILE_COLORS[i % PROFILE_COLORS.len()];
    Color32::from_rgb(r, g, b)
}

fn profiles_dir() -> PathBuf {
    dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from(".")).join("Oxyde").join("Profils")
}

/// Nom affiché d'un profil.
fn profile_label(p: &engine::Instance) -> String {
    if p.kind == "default" && p.name == "Horizon" {
        "Horizon (principal)".into()
    } else {
        p.name.trim().to_string()
    }
}

fn played(secs: u64) -> String {
    match secs {
        0 => "jamais lancé".into(),
        s if s < 3600 => format!("{} min de jeu", (s / 60).max(1)),
        s => format!("{} h {:02} de jeu", s / 3600, (s % 3600) / 60),
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Profiles,
    Mods,
    Packs,
    Settings,
}

/// Colonne de gauche du menu Mods : d'où viennent les mods Java.
#[derive(PartialEq, Clone, Copy)]
enum JavaSrc {
    Modrinth,
    CurseForge,
    Files,
}

/// Colonne de droite : ce qui est dans Horizon.
#[derive(PartialEq, Clone, Copy)]
enum RightView {
    Installed,
    Catalog,
}

enum Upd {
    Off,
    Checking,
    UpToDate,
    Available(update::Release),
    Installing,
    Error(String),
}

#[derive(Clone, PartialEq)]
enum TaskState {
    Running,
    Done,
    Failed,
}

#[derive(Clone)]
struct Task {
    title: String,
    state: TaskState,
    lines: Vec<String>,
    result: String,
    outcomes: Vec<installer::Outcome>,
    key: String,
}

type Tasks = Arc<Mutex<Vec<Task>>>;
/// Les entrées d'un modpack et les fichiers .hmod à intégrer.
type PackParts = (Vec<pack::Entry>, Vec<(String, Vec<u8>)>);

enum Msg {
    Registry(Result<registry::Index, String>),
    Modrinth { gen: u64, offset: u64, result: Result<modrinth::SearchResp, String> },
    Launcher { instances: Option<Vec<engine::Instance>>, mods: Option<Vec<engine::InstalledEntry>>, packs: Option<Vec<engine::PortedPack>> },
    TaskDone,
    Launched(Result<(), String>),
    CurseForge(Result<Vec<curseforge::Mod>, String>),
    SetupDone(Result<PathBuf, String>),
    /// Avant le lancement : réglages faits, carte graphique, nombre de mods actifs.
    LaunchPrep(Vec<String>, String, usize),
    /// Le jeu tourne (true) ; à sa fermeture, la raison d'un éventuel plantage.
    Game(bool),
    GameEnded(Option<String>),
    ProfileSaved(Result<(String, Option<PathBuf>, bool), String>),
    ProfileDeleted(Result<String, String>),
    Machine(perf::Machine),
    /// Recherche de modpacks Modrinth.
    MrPacks { gen: u64, offset: u64, result: Result<modrinth::SearchResp, String> },
    /// Un modpack ouvert : son contenu et le .mrpack téléchargé.
    MrPackOpened { slug: String, result: Result<(modrinth::PackInfo, PathBuf), String> },
}

/// Un modpack Modrinth ouvert dans Oxyde.
struct PackView {
    hit: modrinth::Hit,
    state: Option<Result<(modrinth::PackInfo, PathBuf), String>>,
}

struct App {
    s: Settings,
    tab: Tab,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    registry: Option<Arc<registry::Index>>,
    registry_err: Option<String>,
    // launcher Horizon
    tour: Option<tour::Tour>,
    launcher: Option<PathBuf>,
    instances: Vec<engine::Instance>,
    launcher_mods: Vec<engine::InstalledEntry>,
    packs: Vec<engine::PortedPack>,
    // menu Mods
    java_src: JavaSrc,
    right_view: RightView,
    cat_query: String,
    cf_hits: Vec<curseforge::Mod>,
    cf_loading: bool,
    cf_err: Option<String>,
    imports: Vec<PathBuf>,
    // profils, lancement, installation
    profile_menu: bool,
    launch: Option<Launch>,
    setup_open: bool,
    setup_desktop: bool,
    setup_state: Option<Result<PathBuf, String>>,
    setup_busy: bool,
    confirm_uninstall: bool,
    activity_window: bool,
    query: String,
    kind: String,
    shown: usize,
    mr_hits: Vec<modrinth::Hit>,
    mr_total: u64,
    mr_loading: bool,
    /// Numéro de la dernière recherche : une réponse plus ancienne est ignorée.
    mr_gen: u64,
    mr_err: Option<String>,
    // bibliothèque et modpacks
    library: Vec<game::InstalledMod>,
    confirm_remove: Option<PathBuf>,
    pack_name: String,
    pack_pick: HashSet<String>,
    // tâches
    tasks: Tasks,
    tasks_open: bool,
    // divers
    launching: bool,
    upd: Arc<Mutex<Upd>>,
    toast: Option<(String, bool)>,
    fx: fx::Fx,
    logo: egui::TextureHandle,
    started: Option<f64>,
    splash_done_at: Option<f64>,
    splash_gone: bool,
    prev_tab: Tab,
    tab_changed: f64,
    // profils et performances
    edit: Option<ProfileEdit>,
    // modpacks Modrinth
    packs_modrinth: bool,
    pk_query: String,
    pk_hits: Vec<modrinth::Hit>,
    pk_total: u64,
    pk_loading: bool,
    pk_gen: u64,
    pk_err: Option<String>,
    pk_open: Option<PackView>,
    profile_mods: std::collections::HashMap<String, usize>,
    machine: Option<perf::Machine>,
    game_running: bool,
    /// Réduire la fenêtre une fois l'écran de lancement fini.
    minimize_after_launch: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let s: Settings = cc.storage.and_then(|st| eframe::get_value(st, eframe::APP_KEY)).unwrap_or_default();
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = Self {
            s,
            tab: Tab::Home,
            tx,
            rx,
            registry: None,
            registry_err: None,
            tour: tour::Tour::from_env(),
            launcher: game::launcher_exe(),
            instances: vec![],
            launcher_mods: vec![],
            packs: vec![],
            java_src: JavaSrc::Modrinth,
            right_view: RightView::Installed,
            cat_query: String::new(),
            cf_hits: vec![],
            cf_loading: false,
            cf_err: None,
            imports: vec![],
            profile_menu: false,
            launch: None,
            setup_open: false,
            setup_desktop: true,
            setup_state: None,
            setup_busy: false,
            confirm_uninstall: false,
            activity_window: false,
            query: String::new(),
            kind: "all".into(),
            shown: 40,
            mr_hits: vec![],
            mr_total: 0,
            mr_loading: false,
            mr_gen: 0,
            mr_err: None,
            library: vec![],
            confirm_remove: None,
            pack_name: "Mon modpack".into(),
            pack_pick: HashSet::new(),
            tasks: Arc::new(Mutex::new(vec![])),
            tasks_open: true,
            launching: false,
            upd: Arc::new(Mutex::new(Upd::Off)),
            toast: None,
            fx: fx::Fx,
            logo: load_logo(&cc.egui_ctx),
            started: None,
            splash_done_at: None,
            splash_gone: std::env::var("OXYDE_TAB").is_ok(),
            prev_tab: Tab::Home,
            tab_changed: 0.0,
            edit: None,
            packs_modrinth: true,
            pk_query: String::new(),
            pk_hits: vec![],
            pk_total: 0,
            pk_loading: false,
            pk_gen: 0,
            pk_err: None,
            pk_open: None,
            profile_mods: Default::default(),
            machine: None,
            game_running: false,
            minimize_after_launch: false,
        };
        app.setup_open = setup::should_offer() && !app.s.skip_setup;
        app.tab = match std::env::var("OXYDE_TAB").as_deref() {
            Ok("mods") => {
                app.search_modrinth(&cc.egui_ctx, 0);
                Tab::Mods
            }
            Ok("files") => {
                app.java_src = JavaSrc::Files;
                Tab::Mods
            }
            Ok("catalog") => {
                app.right_view = RightView::Catalog;
                app.search_modrinth(&cc.egui_ctx, 0);
                Tab::Mods
            }
            Ok("packs") => {
                app.search_packs(&cc.egui_ctx, 0);
                Tab::Packs
            }
            Ok("profiles") => Tab::Profiles,
            Ok("settings") => Tab::Settings,
            Ok("setup") => {
                app.setup_open = true;
                Tab::Home
            }
            _ => Tab::Home,
        };
        app.scan_imports();
        if let Ok(f) = std::env::var("OXYDE_DEMO_TASK") {
            app.add_demo_task(&f);
        }
        // démonstrations pour les tests (version de développement seulement)
        #[cfg(debug_assertions)]
        {
            if let Ok(slug) = std::env::var("OXYDE_DEMO_REFUSED") {
                app.s.not_portable.insert(slug, format!("{}{}", installer::NOT_PORTABLE, installer::why_not("Can't be carried over: mod id")));
            }
            if std::env::var("OXYDE_DEMO_ACTIVITY").is_ok() {
                app.activity_window = true;
            }
            if let Ok(q) = std::env::var("OXYDE_DEMO_QUERY") {
                app.query = q;
                app.search_modrinth(&cc.egui_ctx, 0);
            }
        }
        if let Ok(slug) = std::env::var("OXYDE_DEMO_PACK") {
            app.tab = Tab::Packs;
            app.open_mrpack(&cc.egui_ctx, modrinth::Hit { slug: slug.clone(), title: slug, author: String::new(), downloads: 0, icon_url: None, categories: vec![] });
        }
        if std::env::var("OXYDE_DEMO_LAUNCH").is_ok() {
            app.play(&cc.egui_ctx, None);
        }
        if std::env::var("OXYDE_DEMO_EDIT").is_ok() {
            app.edit = Some(ProfileEdit { id: None, name: "Survie entre potes".into(), color: 2, image: None, confirm_delete: false, busy: false });
        }
        app.refresh_library();
        {
            let dir = main_game_dir();
            app.spawn(&cc.egui_ctx, move || Msg::Machine(perf::detect(&dir)));
        }
        app.load_registry(&cc.egui_ctx);
        app.refresh_launcher(&cc.egui_ctx);
        if app.s.auto_update {
            app.check_updates(cc.egui_ctx.clone());
        }
        app
    }

    fn add_demo_task(&mut self, title: &str) {
        let mut t = self.tasks.lock().unwrap();
        t.push(Task {
            title: "mcw-roofs-2.3.3.jar".into(),
            state: TaskState::Done,
            lines: vec![],
            result: "Porté par Horizon · porté en partie et activé · Macaw's Roofs 2.3.3".into(),
            outcomes: vec![],
            key: String::new(),
        });
        t.push(Task {
            title: "AppleSkin".into(),
            state: TaskState::Done,
            lines: vec![],
            result: "Version Horizon officielle · AppleSkin 0.1.3 installé".into(),
            outcomes: vec![],
            key: String::new(),
        });
        t.push(Task {
            title: "Supplementaries".into(),
            state: TaskState::Running,
            lines: vec!["Analyse par le launcher Horizon…".into()],
            result: String::new(),
            outcomes: vec![],
            key: String::new(),
        });
        t.push(Task {
            title: title.into(),
            state: TaskState::Done,
            lines: vec!["Identification du mod…".into(), "Analyse par le launcher Horizon…".into()],
            result: "porté et activé".into(),
            outcomes: vec![installer::Outcome {
                method: Method::Official,
                title: title.into(),
                message: "porté en partie et activé".into(),
                details: vec!["✔ 295 blocks".into(), "✔ 307 recipes".into(), "✖ 6 blocks with no vanilla block to copy (left out)".into()],
            }],
            key: String::new(),
        });
    }

    /// Les profils Horizon (pas les instances Minecraft Java du launcher).
    fn profiles(&self) -> Vec<&engine::Instance> {
        self.instances.iter().filter(|i| matches!(i.kind.as_str(), "default" | "own" | "port")).collect()
    }

    fn profile(&self) -> Option<&engine::Instance> {
        let list = self.profiles();
        self.s
            .profile
            .as_ref()
            .and_then(|id| list.iter().find(|i| &i.id == id).copied())
            .or_else(|| list.iter().find(|i| i.kind == "default").copied())
            .or_else(|| list.first().copied())
    }

    fn profile_name(&self) -> String {
        self.profile().map(profile_label).unwrap_or_else(|| "Horizon".into())
    }

    /// Vignette d'un profil : son image, sinon un dégradé à sa couleur avec son initiale.
    fn avatar(&self, ui: &mut egui::Ui, r: egui::Rect, p: &engine::Instance, radius: f32) {
        if let Some(img) = self.s.profile_images.get(&p.id).filter(|f| f.exists()) {
            ui.put(r, egui::Image::new(format!("file://{}", img.display())).fit_to_exact_size(r.size()).maintain_aspect_ratio(false).corner_radius(radius));
            return;
        }
        let c = profile_color(profile_color_index(p));
        fx::rounded_gradient(ui.painter(), r, radius, c, fx::darken(c, 0.45));
        let initial = profile_label(p).chars().next().unwrap_or('H').to_uppercase().to_string();
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::new(r.height() * 0.48, theme::heavy()), Color32::WHITE);
    }

    fn open_editor(&mut self, p: Option<&engine::Instance>) {
        self.profile_menu = false;
        self.edit = Some(match p {
            Some(p) => ProfileEdit { id: Some(p.id.clone()), name: p.name.clone(), color: profile_color_index(p), image: self.s.profile_images.get(&p.id).cloned(), confirm_delete: false, busy: false },
            None => ProfileEdit { id: None, name: String::new(), color: self.profiles().len() % PROFILE_COLORS.len(), image: None, confirm_delete: false, busy: false },
        });
    }

    fn game_dir(&self) -> PathBuf {
        if let Some(d) = &self.s.game_dir {
            return d.clone();
        }
        self.profile().map(|i| i.home.clone()).unwrap_or_else(game::default_game_dir)
    }

    fn select_profile(&mut self, id: String) {
        self.s.profile = Some(id);
        self.profile_menu = false;
        self.refresh_library();
        self.pack_pick.clear();
        self.toast = Some((format!("Profil : {}", self.profile_name()), true));
    }

    fn scan_imports(&mut self) {
        let mut v: Vec<PathBuf> = std::fs::read_dir(imports_dir()).map(|r| r.flatten().map(|e| e.path()).filter(|p| is_ext(p, "jar")).collect()).unwrap_or_default();
        v.sort();
        self.imports = v;
    }

    fn search_curseforge(&mut self, ctx: &egui::Context) {
        if self.s.cf_key.trim().is_empty() {
            return;
        }
        self.cf_loading = true;
        self.cf_err = None;
        let (key, q) = (self.s.cf_key.trim().to_string(), self.query.clone());
        self.spawn(ctx, move || Msg::CurseForge(curseforge::search(&key, &q, 0)));
    }

    fn ictx(&self) -> installer::Ctx {
        installer::Ctx {
            index: self.registry.clone(),
            launcher: self.launcher.clone(),
            game_dir: self.game_dir(),
            instance: self.profile().map(|p| p.id.clone()),
            out_dir: self.s.output_dir.clone(),
            opts: Options { generate_registries: self.s.generate_registries },
        }
    }

    fn spawn(&self, ctx: &egui::Context, f: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
    }

    fn load_registry(&mut self, ctx: &egui::Context) {
        self.registry_err = None;
        if self.registry.is_none() {
            self.registry = registry::cached().map(Arc::new);
        }
        self.spawn(ctx, || Msg::Registry(registry::fetch()));
    }

    fn refresh_launcher(&mut self, ctx: &egui::Context) {
        self.launcher = game::launcher_exe();
        let Some(exe) = self.launcher.clone() else { return };
        self.spawn(ctx, move || {
            let e = engine::Engine { exe };
            Msg::Launcher { instances: e.instances(), mods: e.installed(), packs: e.packs() }
        });
    }

    fn search_modrinth(&mut self, ctx: &egui::Context, offset: u64) {
        self.mr_loading = true;
        self.mr_err = None;
        let q = self.query.clone();
        self.mr_gen += 1;
        let gen = self.mr_gen;
        self.spawn(ctx, move || Msg::Modrinth { gen, offset, result: modrinth::search(&q, offset) });
    }

    fn search_packs(&mut self, ctx: &egui::Context, offset: u64) {
        self.pk_loading = true;
        self.pk_err = None;
        self.pk_gen += 1;
        let (gen, q) = (self.pk_gen, self.pk_query.clone());
        self.spawn(ctx, move || Msg::MrPacks { gen, offset, result: modrinth::search_packs(&q, offset) });
    }

    fn open_mrpack(&mut self, ctx: &egui::Context, hit: modrinth::Hit) {
        let slug = hit.slug.clone();
        self.pk_open = Some(PackView { hit, state: None });
        self.spawn(ctx, move || Msg::MrPackOpened { result: modrinth::open_pack(&slug), slug });
    }

    fn check_updates(&self, ctx: egui::Context) {
        if update::REPO.is_none() {
            return;
        }
        *self.upd.lock().unwrap() = Upd::Checking;
        let upd = self.upd.clone();
        std::thread::spawn(move || {
            let r = match update::check() {
                Ok(Some(rel)) => Upd::Available(rel),
                Ok(None) => Upd::UpToDate,
                Err(e) => Upd::Error(e),
            };
            *upd.lock().unwrap() = r;
            ctx.request_repaint();
        });
    }

    fn refresh_library(&mut self) {
        self.library = game::scan(&self.game_dir());
        self.profile_mods = self.profiles().iter().map(|p| (p.id.clone(), game::scan(&p.home).iter().filter(|m| m.enabled).count())).collect();
    }

    fn is_installed(&self, id: &str) -> bool {
        self.library.iter().any(|m| m.id == id)
    }

    fn is_busy(&self, key: &str) -> bool {
        self.tasks.lock().unwrap().iter().any(|t| t.key == key && t.state == TaskState::Running)
    }

    /// Lance une tâche en fond. `job` reçoit une fonction de journal et renvoie les résultats.
    fn task(
        &mut self,
        ctx: &egui::Context,
        title: String,
        key: String,
        job: impl FnOnce(&mut dyn FnMut(&str)) -> Result<Vec<installer::Outcome>, String> + Send + 'static,
    ) {
        if !key.is_empty() && self.is_busy(&key) {
            return;
        }
        let idx = {
            let mut t = self.tasks.lock().unwrap();
            t.push(Task { title, state: TaskState::Running, lines: vec![], result: String::new(), outcomes: vec![], key });
            t.len() - 1
        };
        self.tasks_open = true;
        let tasks = self.tasks.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut log = |l: &str| {
                if let Some(t) = tasks.lock().unwrap().get_mut(idx) {
                    t.lines.push(l.to_string());
                }
                ctx.request_repaint();
            };
            let r = job(&mut log);
            if let Some(t) = tasks.lock().unwrap().get_mut(idx) {
                match r {
                    Ok(o) => {
                        t.state = TaskState::Done;
                        t.result = o.first().map(|x| format!("{} · {}", x.method.label(), x.message)).unwrap_or_else(|| "terminé".into());
                        t.outcomes = o;
                    }
                    Err(e) => {
                        t.state = TaskState::Failed;
                        t.result = e;
                    }
                }
            }
            let _ = tx.send(Msg::TaskDone);
            ctx.request_repaint();
        });
    }

    fn install_target(&mut self, ctx: &egui::Context, title: String, key: String, target: Target) {
        let ictx = self.ictx();
        self.task(ctx, title, key, move |log| installer::install(&target, &ictx, log).map(|o| vec![o]));
    }

    fn install_pack(&mut self, ctx: &egui::Context, path: PathBuf) {
        let ictx = self.ictx();
        let title = format!("Modpack {}", file_name(&path));
        self.task(ctx, title, format!("p:{}", path.display()), move |log| install_pack_file(&path, &ictx, log).map(|(_, o)| o));
        self.tab = Tab::Packs;
    }

    fn add_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let dir = imports_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut jars = vec![];
        for p in paths {
            if p.is_dir() {
                jars.extend(std::fs::read_dir(&p).map(|rd| rd.flatten().map(|e| e.path()).filter(|x| is_ext(x, "jar")).collect::<Vec<_>>()).unwrap_or_default());
            } else if is_ext(&p, "jar") {
                jars.push(p);
            } else if is_ext(&p, "hmod") {
                self.install_target(ctx, file_name(&p), String::new(), Target::Hmod(p));
            } else if is_ext(&p, "zip") && pack::is_resource_pack(&p) {
                let dir = self.game_dir().join("resourcepacks");
                let dest = dir.join(file_name(&p));
                self.toast = Some(match std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(&p, &dest)) {
                    Ok(_) => (format!("Pack de ressources ajouté : {}. Active-le dans Options > Packs de ressources.", file_name(&p)), true),
                    Err(e) => (format!("Pack de ressources : {e}"), false),
                });
            } else if is_ext(&p, pack::PACK_EXT) || is_ext(&p, pack::LEGACY_EXT) || is_ext(&p, "mrpack") || is_ext(&p, "zip") {
                self.install_pack(ctx, p);
            } else {
                self.toast = Some((format!("Fichier non pris en charge : {}", file_name(&p)), false));
            }
        }
        if !jars.is_empty() {
            let n = jars.len();
            for j in jars {
                let dest = dir.join(file_name(&j));
                if j != dest {
                    let _ = std::fs::copy(&j, &dest);
                }
            }
            self.scan_imports();
            self.tab = Tab::Mods;
            self.java_src = JavaSrc::Files;
            self.toast = Some((format!("{n} fichier(s) ajouté(s) à « Mes fichiers » : clique sur Installer pour les porter dans Horizon."), true));
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Registry(Ok(i)) => self.registry = Some(Arc::new(i)),
                Msg::Registry(Err(e)) => {
                    if self.registry.is_none() {
                        self.registry_err = Some(e);
                    }
                }
                Msg::Modrinth { gen, .. } if gen != self.mr_gen => {}
                Msg::Modrinth { offset, result, .. } => {
                    self.mr_loading = false;
                    match result {
                        Ok(r) => {
                            if offset == 0 {
                                self.mr_hits = r.hits;
                            } else {
                                self.mr_hits.extend(r.hits);
                            }
                            self.mr_total = r.total_hits;
                        }
                        Err(e) => self.mr_err = Some(e),
                    }
                }
                Msg::Launcher { instances, mods, packs } => {
                    // une réponse manquante ne vide jamais ce qui est affiché
                    if let Some(i) = instances {
                        self.instances = i;
                    }
                    if let Some(m) = mods {
                        self.launcher_mods = m;
                    }
                    if let Some(p) = packs {
                        self.packs = p;
                    }
                    self.refresh_library();
                }
                Msg::TaskDone => {
                    // retenir les mods que Horizon refuse, pour ne plus proposer de les installer
                    for t in self.tasks.lock().unwrap().iter() {
                        if t.state == TaskState::Failed && t.result.starts_with(installer::NOT_PORTABLE) {
                            if let Some(slug) = t.key.strip_prefix("m:") {
                                self.s.not_portable.insert(slug.to_string(), t.result.clone());
                            }
                        }
                    }
                    self.refresh_library();
                    self.refresh_launcher(ctx);
                }
                Msg::Launched(r) => {
                    self.launching = false;
                    match &mut self.launch {
                        Some(l) => l.result = Some(r),
                        None => {
                            if let Err(e) = r {
                                self.toast = Some((e, false));
                            }
                        }
                    }
                }
                Msg::CurseForge(r) => {
                    self.cf_loading = false;
                    match r {
                        Ok(v) => self.cf_hits = v,
                        Err(e) => self.cf_err = Some(e),
                    }
                }
                Msg::SetupDone(r) => {
                    self.setup_busy = false;
                    self.setup_state = Some(r);
                }
                Msg::LaunchPrep(changes, gpu, mods) => {
                    if let Some(l) = &mut self.launch {
                        l.prep = Some((changes, gpu, mods));
                    }
                }
                Msg::Game(on) => self.game_running = on,
                Msg::GameEnded(crash) => {
                    self.game_running = false;
                    // Oxyde revient au premier plan quand on quitte le jeu
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if let Some(why) = crash {
                        self.toast = Some((why, false));
                    }
                    self.refresh_launcher(ctx);
                }
                Msg::ProfileSaved(r) => match r {
                    Ok((id, image, created)) => {
                        match image {
                            Some(img) => self.s.profile_images.insert(id.clone(), img),
                            None => self.s.profile_images.remove(&id),
                        };
                        self.edit = None;
                        if created {
                            self.s.profile = Some(id);
                            self.toast = Some(("Profil créé. Ajoute-lui des mods avec « Ajouter des mods ».".into(), true));
                        } else {
                            self.toast = Some(("Profil enregistré.".into(), true));
                        }
                        self.refresh_launcher(ctx);
                    }
                    Err(e) => {
                        if let Some(ed) = &mut self.edit {
                            ed.busy = false;
                        }
                        self.toast = Some((format!("Profil : {e}"), false));
                    }
                },
                Msg::ProfileDeleted(r) => match r {
                    Ok(id) => {
                        self.s.profile_images.remove(&id);
                        if self.s.profile.as_ref() == Some(&id) {
                            self.s.profile = None;
                        }
                        self.instances.retain(|i| i.id != id);
                        self.edit = None;
                        self.toast = Some(("Profil supprimé (son dossier est dans la Corbeille).".into(), true));
                        self.refresh_launcher(ctx);
                    }
                    Err(e) => self.toast = Some((format!("Suppression : {e}"), false)),
                },
                Msg::Machine(m) => self.machine = Some(m),
                Msg::MrPacks { gen, .. } if gen != self.pk_gen => {}
                Msg::MrPacks { offset, result, .. } => {
                    self.pk_loading = false;
                    match result {
                        Ok(r) => {
                            if offset == 0 {
                                self.pk_hits = r.hits;
                            } else {
                                self.pk_hits.extend(r.hits);
                            }
                            self.pk_total = r.total_hits;
                        }
                        Err(e) => self.pk_err = Some(e),
                    }
                }
                Msg::MrPackOpened { slug, result } => {
                    if let Some(v) = &mut self.pk_open {
                        if v.hit.slug == slug {
                            v.state = Some(result);
                        }
                    }
                }
            }
        }
    }

    fn play(&mut self, ctx: &egui::Context, instance: Option<String>) {
        let now = ctx.input(|i| i.time);
        let name = match &instance {
            Some(id) if id.starts_with("port:") => self.packs.iter().find(|p| format!("port:{}", p.pack) == *id).map(|p| p.name.clone()).unwrap_or_else(|| id.clone()),
            _ => self.profile_name(),
        };
        let instance = instance.or_else(|| self.profile().map(|p| p.id.clone()));
        if self.game_running {
            self.toast = Some(("Horizon est déjà lancé.".into(), true));
            return;
        }
        // pendant la visite guidée (trailer), on ne lance jamais le vrai jeu
        if std::env::var("OXYDE_TOUR").is_ok() || std::env::var("OXYDE_DEMO_LAUNCH").is_ok() {
            self.launch = Some(Launch { name, id: instance, started: now, prep: None, result: None, done_at: None });
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(700));
                let _ = tx.send(Msg::LaunchPrep(vec!["distance de rendu 64 → 10 chunks".into(), "images par seconde plafonnées à 60".into()], "Intel(R) UHD Graphics".into(), 4));
                ctx.request_repaint();
            });
            return;
        }
        let game_dir = instance.as_ref().and_then(|id| self.instances.iter().find(|i| &i.id == id)).map(|i| i.home.clone()).unwrap_or_else(|| self.game_dir());
        self.launcher = game::launcher_exe();
        match self.launcher.clone() {
            Some(exe) => {
                self.launching = true;
                self.minimize_after_launch = self.s.minimize_on_play;
                self.launch = Some(Launch { name, id: instance.clone(), started: now, prep: None, result: None, done_at: None });
                let optimize = self.s.optimize;
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let send = |m: Msg| {
                        let _ = tx.send(m);
                        ctx.request_repaint();
                    };
                    let machine = perf::detect(&game_dir);
                    let changes = if optimize { perf::optimize(&game_dir, &machine) } else { vec![] };
                    let mods = game::scan(&game_dir).iter().filter(|m| m.enabled).count();
                    send(Msg::LaunchPrep(changes, machine.gpu.clone(), mods));
                    let since = std::time::SystemTime::now();
                    let r = engine::Engine { exe }.play(instance.as_deref());
                    let ok = r.is_ok();
                    send(Msg::Launched(r));
                    if !ok {
                        return;
                    }
                    // attendre que le jeu apparaisse, puis qu'il se ferme
                    let mut seen = false;
                    for _ in 0..90 {
                        if perf::game_running() {
                            seen = true;
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                    if !seen {
                        return;
                    }
                    send(Msg::Game(true));
                    while perf::game_running() {
                        std::thread::sleep(std::time::Duration::from_secs(4));
                    }
                    let crash = perf::last_crash(&game_dir, since);
                    if crash.is_some() && optimize {
                        perf::optimize(&game_dir, &perf::Machine { tier: perf::Tier::Light, ..machine });
                    }
                    send(Msg::GameEnded(crash));
                });
            }
            None => self.toast = Some(("Le launcher Horizon n'est pas installé : clique sur « Installer Horizon » dans l'accueil.".into(), false)),
        }
    }

    fn tour_click(&mut self, ctx: &egui::Context, key: &str) {
        if let Some(tab) = key.strip_prefix("nav:") {
            self.tab = match tab {
                "mods" => Tab::Mods,
                "packs" => Tab::Packs,
                "profiles" => Tab::Profiles,
                "settings" => Tab::Settings,
                _ => Tab::Home,
            };
        } else if let Some(id) = key.strip_prefix("feat:") {
            let name = self.registry.as_ref().and_then(|r| r.projects.iter().find(|p| p.id == id)).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string());
            self.install_target(ctx, name, format!("h:{id}"), Target::Horizon(id.to_string()));
        } else if key == "seg:Modrinth" {
            self.java_src = JavaSrc::Modrinth;
            self.query = "macaw".into();
            self.search_modrinth(ctx, 0);
        } else if key == "seg:Catalogue officiel" {
            self.right_view = RightView::Catalog;
        } else if key == "pack_play" || key == "hero_play" {
            self.play(ctx, None);
        }
    }

    /// D'où vient chaque mod installé, pour un modpack léger (références plutôt que fichiers).
    fn pack_entries(&self) -> Result<PackParts, String> {
        let sources = game::sources(&self.game_dir());
        let mut entries = vec![];
        let mut files = vec![];
        for m in self.library.iter().filter(|m| self.pack_pick.contains(&m.id)) {
            let name = self.display_name(m);
            if self.registry.as_ref().is_some_and(|r| r.projects.iter().any(|p| p.id == m.id)) {
                entries.push(pack::Entry::Horizon { id: m.id.clone(), name });
                continue;
            }
            let folder = m.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            if let Some(e) = self.launcher_mods.iter().find(|e| e.source == "modrinth" && e.file.ends_with(&folder) && !e.slug.is_empty()) {
                entries.push(pack::Entry::Modrinth { slug: e.slug.clone(), name });
                continue;
            }
            if let Some(slug) = sources.get(&m.id) {
                entries.push(pack::Entry::Modrinth { slug: slug.clone(), name });
                continue;
            }
            if m.native {
                continue; // un mod natif hors registre ne se partage pas
            }
            let bytes = if m.path.is_dir() { pack::zip_folder(&m.path)? } else { std::fs::read(&m.path).map_err(|e| e.to_string())? };
            let file = format!("mods/{}.hmod", m.id);
            entries.push(pack::Entry::File { id: m.id.clone(), name, file: file.clone(), sha256: pack::sha256_hex(&bytes) });
            files.push((file, bytes));
        }
        Ok((entries, files))
    }

    fn display_name(&self, m: &game::InstalledMod) -> String {
        self.registry
            .as_ref()
            .and_then(|r| r.projects.iter().find(|p| p.id == m.id))
            .map(|p| p.name.clone())
            .or_else(|| {
                let folder = m.path.file_name()?.to_string_lossy().to_string();
                self.launcher_mods.iter().find(|e| e.file.ends_with(&folder)).map(|e| e.title.clone())
            })
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| m.name.clone())
    }

    // ------------------------------------------------------------ UI

    fn ui_topbar(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let mut profile_pill = egui::Rect::NOTHING;
        egui::TopBottomPanel::top("topbar")
            .exact_height(64.0)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(22, 21, 18)).stroke(egui::Stroke::new(1.0_f32, theme::LINE)))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let drag = ui.interact(rect, egui::Id::new("topbar_drag"), egui::Sense::click_and_drag());
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                let compact = rect.width() < 1180.0;
                ui.horizontal_centered(|ui| {
                    ui.add_space(16.0);
                    ui.add(egui::Image::new(egui::include_image!("../assets/icon.png")).fit_to_exact_size(egui::vec2(30.0, 30.0)));
                    if !compact {
                        ui.label(theme::heading("Oxyde", 19.0));
                    }
                    ui.add_space(if compact { 6.0 } else { 18.0 });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // boutons de fenêtre, dessinés à la main
                        for what in [2u8, 1, 0] {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(44.0, 64.0), egui::Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 0.0, if what == 2 { Color32::from_rgb(196, 43, 28) } else { Color32::from_rgb(46, 43, 38) });
                            }
                            let c = r.center();
                            let st = egui::Stroke::new(1.2_f32, theme::TEXT);
                            match what {
                                2 => {
                                    ui.painter().line_segment([c + egui::vec2(-5.0, -5.0), c + egui::vec2(5.0, 5.0)], st);
                                    ui.painter().line_segment([c + egui::vec2(-5.0, 5.0), c + egui::vec2(5.0, -5.0)], st);
                                }
                                1 => {
                                    ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(10.0, 10.0)), 1.5, st, egui::StrokeKind::Middle);
                                }
                                _ => {
                                    ui.painter().line_segment([c + egui::vec2(-5.0, 0.0), c + egui::vec2(5.0, 0.0)], st);
                                }
                            }
                            if resp.clicked() {
                                match what {
                                    0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                    1 => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                                    _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                }
                            }
                        }
                        ui.add_space(12.0);
                        // jouer
                        let label = if self.launching {
                            "Lancement…"
                        } else if self.game_running {
                            "En jeu"
                        } else {
                            "▶  Jouer"
                        };
                        let r = theme::primary_sized(ui, label, egui::vec2(if compact { 96.0 } else { 116.0 }, 40.0), 15.0, !self.launching && !self.game_running);
                        tour::mark(ui, "top_play", r.rect);
                        if r.clicked() {
                            self.play(ui.ctx(), None);
                        }
                        ui.add_space(10.0);
                        // profil
                        let name = self.profile_name();
                        let galley = ui.painter().layout_no_wrap(name.clone(), egui::FontId::new(13.5, theme::semi()), theme::TEXT);
                        let w = (galley.size().x + 70.0).min(if compact { 170.0 } else { 230.0 });
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::click());
                        profile_pill = r;
                        let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || self.profile_menu, 0.12);
                        ui.painter().rect_filled(r, 4.0, theme::SURFACE2);
                        ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0_f32, if hov > 0.5 { Color32::from_rgb(92, 84, 72) } else { theme::LINE }), egui::StrokeKind::Inside);
                        let av = egui::Rect::from_min_size(r.min + egui::vec2(6.0, 6.0), egui::vec2(28.0, 28.0));
                        match self.profile().cloned() {
                            Some(p) => self.avatar(ui, av, &p, 3.0),
                            None => fx::rounded_gradient(ui.painter(), av, 4.0, theme::ACCENT_A, theme::ACCENT_B),
                        }
                        let clip = egui::Rect::from_min_max(egui::pos2(av.right() + 8.0, r.top()), egui::pos2(r.right() - 22.0, r.bottom()));
                        ui.painter().with_clip_rect(clip).galley(egui::pos2(clip.left(), r.center().y - galley.size().y / 2.0), galley, theme::TEXT);
                        let chev = egui::pos2(r.right() - 13.0, r.center().y);
                        ui.painter().line(vec![chev + egui::vec2(-4.0, -2.0), chev + egui::vec2(0.0, 2.0), chev + egui::vec2(4.0, -2.0)], egui::Stroke::new(1.4_f32, theme::SUBTLE));
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Changer de profil").clicked() {
                            self.profile_menu = !self.profile_menu;
                        }
                        ui.add_space(10.0);
                        // navigation, dans la place qui reste
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let running = self.tasks.lock().unwrap().iter().filter(|t| t.state == TaskState::Running).count();
                            let mods_label = if running > 0 { format!("Mods · {running}") } else { "Mods".to_string() };
                            let tabs = [
                                (Tab::Home, "Accueil", "nav:home"),
                                (Tab::Profiles, "Profils", "nav:profiles"),
                                (Tab::Mods, mods_label.as_str(), "nav:mods"),
                                (Tab::Packs, "Modpacks", "nav:packs"),
                                (Tab::Settings, "Réglages", "nav:settings"),
                            ];
                            for (t, label, key) in tabs {
                                let sel = self.tab == t;
                                let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::new(14.0, theme::semi()), theme::TEXT);
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + if compact { 18.0 } else { 26.0 }, 38.0), egui::Sense::click());
                                tour::mark(ui, key, r);
                                let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || sel, 0.12);
                                if hov > 0.0 {
                                    ui.painter().rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, ((if sel { 12.0 } else { 7.0 }) * hov) as u8));
                                }
                                if sel {
                                    let bar = egui::Rect::from_center_size(egui::pos2(r.center().x, r.bottom() + 9.0), egui::vec2(r.width() - 18.0, 3.0));
                                    fx::rounded_gradient(ui.painter(), bar, 1.5, theme::ACCENT_A, theme::ACCENT_B);
                                }
                                ui.painter().galley(r.center() - galley.size() / 2.0, galley, if sel { theme::TEXT } else { theme::SUBTLE });
                                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    self.tab = t;
                                }
                            }
                        });
                    });
                });
            });
        if self.profile_menu {
            self.ui_profile_menu(ctx, profile_pill);
        }
    }

    fn ui_profile_menu(&mut self, ctx: &egui::Context, anchor: egui::Rect) {
        let mut pick = None;
        let mut create = false;
        let mut manage = false;
        let pos = egui::pos2((anchor.right() - 280.0).max(8.0), anchor.bottom() + 8.0);
        let area = egui::Area::new(egui::Id::new("profiles")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                .corner_radius(4.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [5, 5], blur: 0, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_width(280.0);
                    ui.label(theme::label_caps("Profils"));
                    let current = self.profile().map(|p| p.id.clone());
                    let list: Vec<engine::Instance> = self.profiles().into_iter().cloned().collect();
                    for p in &list {
                        let label = profile_label(p);
                        let n = self.profile_mods.get(&p.id).copied().unwrap_or(0);
                        let sub = format!("{} · {n} mod(s)", if p.kind == "port" { "modpack porté" } else if p.kind == "default" { "profil principal" } else { "profil" });
                        let sel = current.as_ref() == Some(&p.id);
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
                        if sel || resp.hovered() {
                            ui.painter().rect_filled(r, 4.0, if sel { theme::SURFACE2 } else { Color32::from_rgb(30, 29, 25) });
                        }
                        let av = egui::Rect::from_min_size(r.min + egui::vec2(8.0, 8.0), egui::vec2(28.0, 28.0));
                        self.avatar(ui, av, p, 3.0);
                        ui.painter().text(egui::pos2(av.right() + 10.0, r.top() + 14.0), egui::Align2::LEFT_CENTER, &label, egui::FontId::new(14.0, theme::semi()), theme::TEXT);
                        ui.painter().text(egui::pos2(av.right() + 10.0, r.top() + 31.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.5), theme::SUBTLE);
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            pick = Some(p.id.clone());
                        }
                    }
                    ui.separator();
                    if self.launcher.is_some() {
                        ui.horizontal(|ui| {
                            if theme::primary_sized(ui, "+ Nouveau profil", egui::vec2(140.0, 32.0), 13.5, true).clicked() {
                                create = true;
                            }
                            if ui.button("Gérer les profils").clicked() {
                                manage = true;
                            }
                        });
                    } else {
                        ui.label(RichText::new("Installe le launcher Horizon pour gérer des profils.").small().color(MUTED));
                    }
                });
        });
        // clic ailleurs : on ferme
        if ctx.input(|i| i.pointer.any_click()) && !area.response.contains_pointer() && !anchor.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) {
            self.profile_menu = false;
        }
        if let Some(id) = pick {
            self.select_profile(id);
        }
        if create {
            self.open_editor(None);
        }
        if manage {
            self.profile_menu = false;
            self.tab = Tab::Profiles;
        }
    }

    /// Bords redimensionnables (la fenêtre n'a pas de cadre Windows).
    fn resize_edges(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        use egui::viewport::ResizeDirection as D;
        let r = ctx.screen_rect();
        let t = 5.0;
        let zones = [
            (egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + t, r.bottom())), D::West, egui::CursorIcon::ResizeHorizontal),
            (egui::Rect::from_min_max(egui::pos2(r.right() - t, r.top()), r.right_bottom()), D::East, egui::CursorIcon::ResizeHorizontal),
            (egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - t), r.right_bottom()), D::South, egui::CursorIcon::ResizeVertical),
            (egui::Rect::from_min_max(egui::pos2(r.right() - 12.0, r.bottom() - 12.0), r.right_bottom()), D::SouthEast, egui::CursorIcon::ResizeNwSe),
        ];
        egui::Area::new(egui::Id::new("resize")).order(egui::Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
            for (i, (zone, dir, cursor)) in zones.into_iter().enumerate() {
                let resp = ui.interact(zone, egui::Id::new(("rz", i)), egui::Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                }
            }
        });
    }

    fn ui_update_banner(&mut self, ui: &mut egui::Ui) {
        let state = std::mem::replace(&mut *self.upd.lock().unwrap(), Upd::Off);
        let mut next = None;
        match &state {
            Upd::Available(rel) => {
                surface(ui, 12, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Oxyde {} est disponible", rel.version)).family(theme::semi()).color(Color32::WHITE));
                        if ui.add(egui::Button::new(RichText::new("Mettre à jour").color(Color32::WHITE)).fill(RUST)).clicked() {
                            next = Some(Upd::Installing);
                            let rel = rel.clone();
                            let upd = self.upd.clone();
                            let ctx = ui.ctx().clone();
                            std::thread::spawn(move || {
                                if let Err(e) = update::apply(&rel) {
                                    *upd.lock().unwrap() = Upd::Error(e);
                                    ctx.request_repaint();
                                }
                            });
                        }
                        if !rel.page.is_empty() {
                            ui.hyperlink_to("Nouveautés", &rel.page);
                        }
                    });
                });
            }
            Upd::Installing => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Téléchargement et vérification de la mise à jour…");
                });
            }
            _ => {}
        }
        let mut g = self.upd.lock().unwrap();
        if matches!(*g, Upd::Off) {
            *g = next.unwrap_or(state);
        }
    }

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        let t = ui.input(|i| i.time) as f32;
        let launcher = self.launcher.clone();

        // ---- grand bandeau : dégradé, lueur, île voxel qui flotte
        let w = ui.available_width();
        let hero_h = (ui.available_height() * 0.36).clamp(210.0, 300.0);
        let (hero, _) = ui.allocate_exact_size(egui::vec2(w, hero_h), egui::Sense::hover());
        let p = ui.painter_at(hero.expand(30.0));
        p.rect_filled(hero.translate(egui::vec2(0.0, 10.0)), 4.0, Color32::from_black_alpha(90));
        fx::rounded_gradient(&p, hero, 4.0, theme::SURFACE, theme::SURFACE);
        p.rect_filled(egui::Rect::from_min_size(hero.min, egui::vec2(4.0, hero.height())), 0.0, theme::ACCENT_A);
        fx::soft_glow(&p, hero.right_center() - egui::vec2(w * 0.22, 0.0), 260.0, Color32::from_rgba_unmultiplied(230, 110, 40, 70));
        p.rect_stroke(hero, 4.0, egui::Stroke::new(1.0_f32, theme::LINE), egui::StrokeKind::Inside);
        let s = (hero.height() / 15.0).min(22.0);
        fx::diorama(&p, hero.right_center() - egui::vec2(w * 0.2, s * 0.6), s, t);

        let inner = hero.shrink2(egui::vec2(34.0, 30.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(inner.min, egui::vec2(inner.width() * 0.58, inner.height()))), |ui| {
            ui.label(theme::label_caps("Horizon · Minecraft Rust Edition"));
            ui.label(theme::heading("Minecraft, réécrit en Rust.", 38.0));
            ui.label(RichText::new("Tes mods, tes modpacks et le jeu, au même endroit.").size(16.0).color(Color32::from_rgb(190, 182, 168)));
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                let label = if self.launching { "Lancement…" } else { "▶   Jouer" };
                let r = theme::primary_sized(ui, label, egui::vec2(200.0, 54.0), 19.0, !self.launching && launcher.is_some());
                tour::mark(ui, "hero_play", r.rect);
                let pulse = 0.5 + 0.5 * (t * 2.2).sin();
                fx::soft_glow(ui.painter(), r.rect.center(), 140.0, Color32::from_rgba_unmultiplied(255, 120, 40, (25.0 + 25.0 * pulse) as u8));
                if r.clicked() {
                    self.play(ui.ctx(), None);
                }
                ui.add_space(8.0);
                ui.vertical(|ui| match &launcher {
                    Some(exe) => {
                        ui.label(RichText::new("✔ Launcher Horizon prêt").color(GREEN));
                        if ui.small_button("Ouvrir le launcher").clicked() {
                            if let Err(e) = game::open_launcher(exe) {
                                self.toast = Some((e, false));
                            }
                        }
                    }
                    None => {
                        ui.label(RichText::new("✖ Horizon n'est pas installé").color(ORANGE));
                        ui.hyperlink_to("Installer Horizon (gratuit)", game::INSTALL_PAGE);
                    }
                });
            });
        });
        ui.advance_cursor_after_rect(hero);
        ui.add_space(22.0);

        ui.columns(3, |c| {
            stat(&mut c[0], fx::Icon::Mods, &self.library.len().to_string(), "mods installés");
            stat(&mut c[1], fx::Icon::Packs, &self.packs.len().to_string(), "modpacks prêts à jouer");
            let n = self.registry.as_ref().map(|r| r.projects.len().to_string()).unwrap_or_else(|| "…".into());
            stat(&mut c[2], fx::Icon::Store, &n, "projets dans le magasin");
        });

        // ---- tes profils, prêts à lancer
        let profiles: Vec<engine::Instance> = self.profiles().into_iter().take(4).cloned().collect();
        if !profiles.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                section(ui, "Tes profils");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Gérer les profils").clicked() {
                        self.tab = Tab::Profiles;
                    }
                });
            });
            let mut launch = None;
            let n = profiles.len().clamp(1, 4);
            ui.columns(n, |cols| {
                for (col, p) in cols.iter_mut().zip(profiles.iter()) {
                    surface(col, 14, |ui| {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                            self.avatar(ui, r, p, 3.0);
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(RichText::new(profile_label(p)).family(theme::semi()).color(theme::TEXT)).truncate());
                                let n = self.profile_mods.get(&p.id).copied().unwrap_or(0);
                                ui.label(RichText::new(format!("{n} mod(s) · {}", played(p.played_secs))).small().color(MUTED));
                            });
                        });
                        ui.add_space(4.0);
                        if theme::primary(ui, "▶  Jouer", egui::vec2(ui.available_width(), 34.0)).clicked() {
                            launch = Some(p.id.clone());
                        }
                    });
                }
            });
            if let Some(id) = launch {
                self.s.profile = Some(id.clone());
                self.refresh_library();
                self.play(ui.ctx(), Some(id));
            }
        }

        // ---- à la une : les mods Horizon les plus téléchargés
        let Some(idx) = self.registry.clone() else { return };
        section(ui, "À la une");
        let mut top: Vec<&registry::Project> = idx.projects.iter().filter(|p| p.kind == "native-mod" || p.kind == "mod").collect();
        top.sort_by_key(|p| std::cmp::Reverse(p.downloads.unwrap_or(0)));
        let mut install = None;
        let running: Vec<String> = self.tasks.lock().unwrap().iter().filter(|t| t.state == TaskState::Running).map(|t| t.key.clone()).collect();
        ui.columns(4, |cols| {
            for (col, p) in cols.iter_mut().zip(top.iter().take(4)) {
                let installed = self.library.iter().any(|m| m.id == p.id);
                let busy = running.contains(&format!("h:{}", p.id));
                surface(col, 14, |ui| {
                    if let Some(u) = p.icon.as_deref().filter(|u| u.starts_with("https://")) {
                        ui.add(egui::Image::new(u).fit_to_exact_size(egui::vec2(72.0, 72.0)).corner_radius(4.0));
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(&p.name).size(16.0).family(theme::semi()).color(Color32::WHITE));
                    let sum: String = p.summary.chars().take(70).collect();
                    ui.label(RichText::new(sum).small().color(MUTED));
                    ui.label(RichText::new(format!("⬇ {}", fmt_num(p.downloads.unwrap_or(0)))).small().color(theme::SUBTLE));
                    ui.add_space(4.0);
                    if installed {
                        ui.label(RichText::new("✔ Installé").color(GREEN));
                    } else if busy {
                        ui.spinner();
                    } else {
                        let b = theme::primary(ui, "Installer", egui::vec2(ui.available_width(), 32.0));
                        tour::mark(ui, &format!("feat:{}", p.id), b.rect);
                        if b.clicked() {
                            install = Some((p.name.clone(), p.id.clone()));
                        }
                    }
                });
            }
        });
        if let Some((name, id)) = install {
            self.install_target(ui.ctx(), name, format!("h:{id}"), Target::Horizon(id));
        }
    }

    fn ui_mods(&mut self, ui: &mut egui::Ui) {
        title(ui, "Mods", &format!("À gauche, les mods Java. Clique sur Installer : Oxyde les porte pour Horizon et les met dans le profil « {} ».", self.profile_name()));
        let gap = 14.0;
        let total = ui.available_width();
        let left = ((total - gap) * 0.55).floor();
        let right = total - gap - left;
        let h = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(left, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_max_width(left);
                self.ui_java_pane(ui);
            });
            ui.add_space(gap - ui.spacing().item_spacing.x);
            ui.allocate_ui_with_layout(egui::vec2(right, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_max_width(right);
                self.ui_horizon_pane(ui);
            });
        });
    }

    fn ui_java_pane(&mut self, ui: &mut egui::Ui) {
        pane(ui, |ui| {
            ui.label(theme::heading("Mods Java", 18.0));
            let srcs = [JavaSrc::Modrinth, JavaSrc::CurseForge, JavaSrc::Files];
            let cur = srcs.iter().position(|x| *x == self.java_src).unwrap_or(0);
            if let Some(i) = theme::pills(ui, &["Modrinth", "CurseForge", "Mes fichiers"], cur, "seg:") {
                let src = srcs[i];
                if self.java_src != src {
                    self.java_src = src;
                    if src == JavaSrc::Modrinth && self.mr_hits.is_empty() {
                        self.search_modrinth(ui.ctx(), 0);
                    }
                    if src == JavaSrc::CurseForge && self.cf_hits.is_empty() {
                        self.search_curseforge(ui.ctx());
                    }
                }
            }
            ui.add_space(4.0);
            match self.java_src {
                JavaSrc::Modrinth => self.ui_modrinth_list(ui),
                JavaSrc::CurseForge => self.ui_curseforge_list(ui),
                JavaSrc::Files => self.ui_files_list(ui),
            }
        });
    }

    fn search_bar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut go = false;
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            go |= ui.button("🔍").on_hover_text("Rechercher").clicked();
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Rechercher un mod…")
                    .desired_width(ui.available_width())
                    .margin(egui::vec2(12.0, 8.0))
                    .background_color(Color32::from_rgb(15, 14, 12)),
            );
            go |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        go
    }

    fn list_area(ui: &egui::Ui, salt: &str) -> egui::ScrollArea {
        let mut area = egui::ScrollArea::vertical().id_salt(salt.to_string()).auto_shrink([false, false]);
        if let Some(y) = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("tour_scroll"))) {
            area = area.vertical_scroll_offset(y);
        }
        area
    }

    fn ui_modrinth_list(&mut self, ui: &mut egui::Ui) {
        if self.search_bar(ui) {
            self.search_modrinth(ui.ctx(), 0);
        }
        if let Some(e) = &self.mr_err {
            ui.colored_label(RED, e);
        }
        let mut install = None;
        let mut more = false;
        Self::list_area(ui, "mr").show(ui, |ui| {
            let browsing = self.query.trim().is_empty();
            for h in &self.mr_hits {
                let port = self.registry.as_ref().and_then(|r| r.find_port(&h.slug, &h.title));
                let code_only = port.is_none() && h.categories.iter().any(|c| c == "library" || c == "optimization");
                if browsing && code_only {
                    continue;
                }
                let done = port.is_some_and(|p| self.is_installed(&p.id)) || self.launcher_mods.iter().any(|e| e.slug == h.slug) || self.is_installed(&convert::sanitize_id(&h.slug));
                let refused = self.s.not_portable.get(&h.slug);
                let action = if done {
                    RowAction::Done
                } else if self.is_busy(&format!("m:{}", h.slug)) {
                    RowAction::Busy
                } else if code_only {
                    RowAction::Info("code seulement")
                } else if refused.is_some() {
                    RowAction::Info("non portable")
                } else {
                    RowAction::Install
                };
                let badge = if port.is_some() {
                    "version Horizon"
                } else if code_only || refused.is_some() {
                    ""
                } else {
                    "mod Java"
                };
                let sub = format!("{} · ⬇ {}", h.author, fmt_num(h.downloads));
                let before = ui.cursor().min.y;
                if row(ui, h.icon_url.as_deref(), &h.title, &sub, badge, action) {
                    install = Some(h.clone());
                }
                if let Some(why) = refused {
                    let r = egui::Rect::from_min_max(egui::pos2(ui.min_rect().left(), before), egui::pos2(ui.min_rect().right(), ui.cursor().min.y));
                    ui.interact(r, egui::Id::new(("why", &h.slug)), egui::Sense::hover()).on_hover_text(why.as_str());
                }
            }
            if self.mr_loading {
                ui.spinner();
            } else if (self.mr_hits.len() as u64) < self.mr_total && ui.button("Afficher plus").clicked() {
                more = true;
            }
        });
        if let Some(h) = install {
            self.install_target(ui.ctx(), h.title.clone(), format!("m:{}", h.slug), Target::Modrinth { slug: h.slug.clone(), title: h.title.clone() });
        }
        if more {
            let off = self.mr_hits.len() as u64;
            self.search_modrinth(ui.ctx(), off);
        }
    }

    fn ui_curseforge_list(&mut self, ui: &mut egui::Ui) {
        if self.s.cf_key.trim().is_empty() {
            surface(ui, 14, |ui| {
                ui.label(RichText::new("CurseForge demande une clé (gratuite)").family(theme::semi()).color(Color32::WHITE));
                ui.label(
                    RichText::new("CurseForge n'ouvre son catalogue qu'aux applications qui ont une clé. Crée-en une gratuitement, colle-la dans Réglages, et les mods CurseForge apparaîtront ici.")
                        .color(MUTED),
                );
                ui.hyperlink_to("Obtenir une clé CurseForge", curseforge::KEY_PAGE);
                ui.add_space(4.0);
                ui.label(RichText::new("Sans clé : télécharge le .jar sur curseforge.com et glisse-le dans Oxyde, il ira dans « Mes fichiers ».").small().color(MUTED));
            });
            return;
        }
        if self.search_bar(ui) {
            self.search_curseforge(ui.ctx());
        }
        ui.label(RichText::new("Bêta").small().color(ORANGE));
        if let Some(e) = &self.cf_err {
            ui.colored_label(RED, e);
        }
        let mut install = None;
        Self::list_area(ui, "cf").show(ui, |ui| {
            for m in &self.cf_hits {
                let key = format!("cf:{}", m.id);
                let action = if self.is_busy(&key) { RowAction::Busy } else { RowAction::Install };
                let sub = format!("{} · ⬇ {}", m.authors.first().map(|a| a.name.as_str()).unwrap_or("?"), fmt_num(m.downloads as u64));
                let icon = m.logo.as_ref().and_then(|l| l.thumb.as_deref());
                if row(ui, icon, &m.name, &sub, "", action) {
                    install = Some(m.clone());
                }
            }
            if self.cf_loading {
                ui.spinner();
            }
        });
        if let Some(m) = install {
            let ictx = self.ictx();
            self.task(ui.ctx(), m.name.clone(), format!("cf:{}", m.id), move |log| {
                log("Téléchargement sur CurseForge…");
                let jar = curseforge::download(&m)?;
                installer::install(&Target::Jar(jar), &ictx, log).map(|o| vec![o])
            });
        }
    }

    fn ui_files_list(&mut self, ui: &mut egui::Ui) {
        drop_zone(ui, "Glisse tes .jar ici", "Fichiers Modrinth, CurseForge ou d'ailleurs");
        ui.horizontal(|ui| {
            if ui.button("Ajouter des .jar…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Mods Java", &["jar"]).pick_files() {
                    let ctx = ui.ctx().clone();
                    self.add_paths(&ctx, f);
                }
            }
            if ui.button("Ouvrir le dossier").clicked() {
                open_folder(&imports_dir());
            }
            let todo: Vec<PathBuf> = self.imports.iter().filter(|p| !self.is_busy(&format!("j:{}", p.display()))).cloned().collect();
            if !todo.is_empty() && ui.add(egui::Button::new(RichText::new("Tout installer").color(Color32::WHITE)).fill(RUST)).clicked() {
                for j in todo {
                    self.install_target(ui.ctx(), file_name(&j), format!("j:{}", j.display()), Target::Jar(j));
                }
            }
        });
        let mut install = None;
        let mut remove = None;
        let tasks = self.tasks.lock().unwrap().clone();
        Self::list_area(ui, "files").show(ui, |ui| {
            if self.imports.is_empty() {
                ui.label(RichText::new("Aucun fichier pour l'instant.").color(MUTED));
            }
            for j in &self.imports {
                let key = format!("j:{}", j.display());
                let last = tasks.iter().rev().find(|t| t.key == key);
                let action = match last.map(|t| &t.state) {
                    Some(TaskState::Running) => RowAction::Busy,
                    Some(TaskState::Done) => RowAction::Done,
                    _ => RowAction::Install,
                };
                let sub = match last {
                    Some(t) if t.state == TaskState::Failed => format!("✖ {}", t.result),
                    Some(t) if t.state == TaskState::Done => t.result.clone(),
                    _ => format!("{} Ko", std::fs::metadata(j).map(|m| m.len() / 1024).unwrap_or(0)),
                };
                if row(ui, None, &file_name(j), &sub, "", action) {
                    install = Some(j.clone());
                }
                if ui.small_button("Retirer de la liste").clicked() {
                    remove = Some(j.clone());
                }
            }
        });
        if let Some(j) = install {
            self.install_target(ui.ctx(), file_name(&j), format!("j:{}", j.display()), Target::Jar(j));
        }
        if let Some(j) = remove {
            let _ = std::fs::remove_file(&j);
            self.scan_imports();
        }
    }

    fn ui_horizon_pane(&mut self, ui: &mut egui::Ui) {
        pane(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::heading("Horizon", 18.0));
                ui.add(egui::Label::new(RichText::new(format!("· {}", self.profile_name())).size(12.0).color(MUTED)).truncate());
            });
            let installed = format!("Installés · {}", self.library.len());
            let cur = if self.right_view == RightView::Installed { 0 } else { 1 };
            match theme::pills(ui, &[installed.as_str(), "Catalogue officiel"], cur, "seg:") {
                Some(0) => self.right_view = RightView::Installed,
                Some(_) => self.right_view = RightView::Catalog,
                None => {}
            }
            ui.add_space(4.0);
            match self.right_view {
                RightView::Installed => self.ui_installed_list(ui),
                RightView::Catalog => self.ui_catalog_list(ui),
            }
        });
    }

    fn ui_installed_list(&mut self, ui: &mut egui::Ui) {
        if self.library.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Aucun mod dans ce profil").family(theme::semi()).color(Color32::WHITE));
                ui.label(RichText::new("Installe un mod Java à gauche, ou un mod du Catalogue officiel.").color(MUTED));
            });
            return;
        }
        let mut toggle = None;
        let mut remove = None;
        Self::list_area(ui, "inst").show(ui, |ui| {
            for m in &self.library {
                let name = self.display_name(m);
                let folder = m.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let entry = self.launcher_mods.iter().find(|e| e.file.ends_with(&folder));
                let (badge, origin) = if m.native {
                    ("Rust natif", "complet, code compris")
                } else if entry.is_some() {
                    ("porté par Horizon", "contenu porté, sans le code Java")
                } else {
                    ("converti", "contenu converti par Oxyde")
                };
                surface(ui, 10, |ui| {
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 42.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.confirm_remove.as_ref() == Some(&m.path) {
                            if ui.button(RichText::new("Supprimer").color(RED)).clicked() {
                                remove = Some((m.clone(), entry.map(|e| e.slug.clone())));
                            }
                            if ui.small_button("Annuler").clicked() {
                                self.confirm_remove = None;
                            }
                        } else if ui.small_button("Retirer").clicked() {
                            self.confirm_remove = Some(m.path.clone());
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if m.native {
                                ui.label(RichText::new("⚙").size(18.0).color(RUST));
                            } else {
                                let mut on = m.enabled;
                                if ui.checkbox(&mut on, "").on_hover_text("Activer / désactiver").changed() {
                                    toggle = Some((m.id.clone(), on));
                                }
                            }
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(RichText::new(&name).family(theme::semi()).color(Color32::WHITE)).truncate());
                                ui.horizontal(|ui| {
                                    chip(ui, badge);
                                    ui.add(egui::Label::new(RichText::new(format!("{} · {origin}", m.version)).small().color(MUTED)).truncate());
                                });
                            });
                        });
                    });
                });
            }
        });
        let dir = self.game_dir();
        if let Some((id, on)) = toggle {
            if let Err(e) = game::set_enabled(&dir, &id, on) {
                self.toast = Some((e, false));
            }
            self.refresh_library();
        }
        if let Some((m, slug)) = remove {
            self.confirm_remove = None;
            let r = match (slug.filter(|s| !s.is_empty()), self.launcher.clone()) {
                (Some(slug), Some(exe)) => engine::Engine { exe }.remove(&slug).map(|_| ()),
                _ => game::remove(&dir, &m),
            };
            self.toast = Some(match r {
                Ok(()) => (format!("{} supprimé.", m.name), true),
                Err(e) => (e, false),
            });
            self.refresh_library();
            self.refresh_launcher(ui.ctx());
        }
    }

    fn ui_catalog_list(&mut self, ui: &mut egui::Ui) {
        let Some(idx) = self.registry.clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.registry_err.clone().unwrap_or_else(|| "Chargement du registre Horizon…".into()));
            });
            return;
        };
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("kind").width(110.0).selected_text(if self.kind == "all" { "Tout" } else { registry::kind_label(&self.kind) }).show_ui(ui, |ui| {
                for k in ["all", "native-mod", "mod", "resourcepack", "shader", "modpack"] {
                    ui.selectable_value(&mut self.kind, k.to_string(), if k == "all" { "Tout" } else { registry::kind_label(k) });
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.cat_query).hint_text("Filtrer le catalogue…").desired_width(ui.available_width()).margin(egui::vec2(12.0, 8.0)).background_color(Color32::from_rgb(15, 14, 12)));
        });
        let q = self.cat_query.to_lowercase();
        let mut list: Vec<&registry::Project> = idx
            .projects
            .iter()
            .filter(|p| self.kind == "all" || p.kind == self.kind)
            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.summary.to_lowercase().contains(&q) || p.tags.iter().any(|t| t.contains(&q)))
            .collect();
        list.sort_by_key(|p| std::cmp::Reverse(p.downloads.unwrap_or(0)));
        let mut install = None;
        Self::list_area(ui, "cat").show(ui, |ui| {
            for p in list.iter().take(self.shown) {
                let action = if self.is_installed(&p.id) {
                    RowAction::Done
                } else if self.is_busy(&format!("h:{}", p.id)) {
                    RowAction::Busy
                } else if p.kind == "modpack" {
                    if self.launcher.is_some() { RowAction::Install } else { RowAction::Info("launcher requis") }
                } else {
                    RowAction::Install
                };
                let sub = format!("⬇ {} · {}", fmt_num(p.downloads.unwrap_or(0)), p.summary);
                if row(ui, p.icon.as_deref(), &p.name, &sub, registry::kind_label(&p.kind), action) {
                    install = Some((*p).clone());
                }
            }
            if list.len() > self.shown && ui.button("Afficher plus").clicked() {
                self.shown += 40;
            }
        });
        if let Some(p) = install {
            if p.kind == "modpack" {
                let exe = self.launcher.clone().unwrap();
                let id = p.id.clone();
                self.task(ui.ctx(), format!("Modpack {}", p.name), format!("h:{}", p.id), move |log| {
                    let out = engine::Engine { exe }.install_horizon_pack(&id, log)?;
                    Ok(vec![installer::Outcome { method: Method::Native, title: id, message: out, details: vec!["Choisis-le dans tes profils pour y jouer.".into()] }])
                });
            } else {
                self.install_target(ui.ctx(), p.name.clone(), format!("h:{}", p.id), Target::Horizon(p.id.clone()));
            }
        }
    }

    fn ui_packs(&mut self, ui: &mut egui::Ui) {
        title(ui, "Modpacks", "Ouvre les modpacks de Modrinth, regarde ce qu'ils contiennent, installe-les. Ou glisse celui d'un pote.");
        let sel = if self.packs_modrinth { 0 } else { 1 };
        if let Some(i) = theme::pills(ui, &["Modrinth", "Mes modpacks"], sel, "pk:") {
            self.packs_modrinth = i == 0;
            if self.packs_modrinth && self.pk_hits.is_empty() {
                self.search_packs(ui.ctx(), 0);
            }
        }
        ui.add_space(10.0);
        if self.packs_modrinth {
            self.ui_modrinth_packs(ui);
            return;
        }
        drop_zone(ui, "Glisse un modpack ici", ".oxypack (Oxyde), .mrpack (Modrinth) ou .zip (CurseForge)");
        ui.horizontal(|ui| {
            if ui.button("Choisir un modpack…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Modpacks", &[pack::PACK_EXT, pack::LEGACY_EXT, "mrpack", "zip"]).pick_file() {
                    let ctx = ui.ctx().clone();
                    self.add_paths(&ctx, vec![f]);
                }
            }
            if ui.button("Actualiser").clicked() {
                self.refresh_launcher(ui.ctx());
            }
        });
        ui.add_space(8.0);

        let mut play = None;
        if !self.packs.is_empty() {
            ui.label(RichText::new("Prêts à jouer").size(18.0).family(theme::semi()).color(Color32::WHITE));
            for p in &self.packs {
                surface(ui, 14, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&p.name).size(17.0).family(theme::semi()));
                            ui.label(RichText::new(format!("{} · {} mod(s) portés sur {}", p.summary, p.ported_mods, p.portable)).small().color(MUTED));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let b = theme::primary(ui, "▶  Jouer", egui::vec2(110.0, 36.0));
                            tour::mark(ui, "pack_play", b.rect);
                            if b.clicked() {
                                play = Some(format!("port:{}", p.pack));
                            }
                        });
                    });
                });
            }
        }
        if let Some(i) = play {
            self.play(ui.ctx(), Some(i));
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Créer un modpack à partager").size(18.0).family(theme::semi()).color(Color32::WHITE));
        ui.label(
            RichText::new("Oxyde fait un petit fichier qui liste tes mods : ton pote le glisse dans Oxyde et tout se télécharge. Seuls les mods introuvables en ligne sont mis dans le fichier.")
                .small()
                .color(MUTED),
        );
        if self.library.is_empty() {
            ui.label("Installe d'abord des mods (Magasin ou Importer).");
            return;
        }
        if self.pack_pick.is_empty() {
            self.pack_pick = self.library.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect();
        }
        egui::ScrollArea::vertical().max_height(220.0).id_salt("pick").show(ui, |ui| {
            let mods: Vec<(String, String)> = self.library.iter().map(|m| (m.id.clone(), self.display_name(m))).collect();
            for (id, name) in mods {
                let mut on = self.pack_pick.contains(&id);
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        self.pack_pick.insert(id);
                    } else {
                        self.pack_pick.remove(&id);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.pack_name).desired_width(220.0).margin(egui::vec2(10.0, 7.0)).background_color(Color32::from_rgb(15, 14, 12)));
            if ui.add_enabled(!self.pack_pick.is_empty(), egui::Button::new(RichText::new(format!("Créer ({} mods)", self.pack_pick.len())).color(Color32::WHITE)).fill(RUST)).clicked() {
                self.save_pack();
            }
        });
    }

    fn ui_modrinth_packs(&mut self, ui: &mut egui::Ui) {
        if self.pk_hits.is_empty() && !self.pk_loading && self.pk_err.is_none() {
            self.search_packs(ui.ctx(), 0);
        }
        ui.horizontal(|ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut self.pk_query).hint_text("Rechercher un modpack Modrinth…").desired_width(ui.available_width() - 60.0).margin(egui::vec2(12.0, 8.0)));
            let go = ui.add_sized(egui::vec2(46.0, 34.0), egui::Button::new("🔍")).clicked();
            if go || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.search_packs(ui.ctx(), 0);
            }
        });
        ui.label(RichText::new("Horizon porte ce qu'il peut de chaque mod (blocs, objets, recettes, textures) ; le code Java des mods ne tourne pas. Ouvre un modpack pour voir ce qu'il contient.").small().color(MUTED));
        ui.add_space(6.0);
        if let Some(e) = &self.pk_err {
            ui.label(RichText::new(e).color(RED));
        }
        let mut open = None;
        let busy: Vec<String> = self.tasks.lock().unwrap().iter().filter(|t| t.state == TaskState::Running).map(|t| t.key.clone()).collect();
        for h in &self.pk_hits {
            let installing = busy.iter().any(|k| k.contains(&h.slug));
            let sub = format!("{} · ⬇ {}", h.author, fmt_num(h.downloads));
            let badge = h.categories.iter().find(|c| matches!(c.as_str(), "fabric" | "forge" | "neoforge" | "quilt")).map(|s| s.as_str()).unwrap_or("");
            if row(ui, h.icon_url.as_deref(), &h.title, &sub, badge, if installing { RowAction::Busy } else { RowAction::Button("Ouvrir") }) {
                open = Some(h.clone());
            }
        }
        if self.pk_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Recherche sur Modrinth…").color(MUTED));
            });
        } else if (self.pk_hits.len() as u64) < self.pk_total && !self.pk_hits.is_empty() && ui.button("Voir plus").clicked() {
            let n = self.pk_hits.len() as u64;
            self.search_packs(ui.ctx(), n);
        }
        if let Some(h) = open {
            self.open_mrpack(ui.ctx(), h);
        }
    }

    /// Fenêtre d'un modpack Modrinth : version, chargeur, liste des mods, installation.
    fn ui_pack_view(&mut self, ctx: &egui::Context) {
        let Some(v) = &self.pk_open else { return };
        let screen = ctx.screen_rect();
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut install: Option<PathBuf> = None;
        let mut retry = false;
        egui::Area::new(egui::Id::new("pack_view")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let bg = ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(170));
            let card = egui::Rect::from_center_size(screen.center(), egui::vec2(640.0, 620.0).min(screen.size() - egui::vec2(32.0, 32.0)));
            ui.painter().rect_filled(card.translate(egui::vec2(6.0, 6.0)), theme::R, Color32::from_black_alpha(120));
            ui.painter().rect_filled(card, theme::R, theme::SURFACE);
            ui.painter().rect_stroke(card, theme::R, egui::Stroke::new(1.0_f32, theme::LINE), egui::StrokeKind::Inside);
            if bg.clicked() && !card.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) {
                close = true;
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(card.shrink(24.0)).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.horizontal(|ui| {
                    match v.hit.icon_url.as_deref().filter(|u| u.starts_with("https://")) {
                        Some(u) => {
                            ui.add(egui::Image::new(u).fit_to_exact_size(egui::vec2(64.0, 64.0)).corner_radius(theme::R));
                        }
                        None => {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, theme::R, theme::SURFACE2);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(theme::label_caps("Modpack Modrinth"));
                        ui.add(egui::Label::new(theme::heading(&v.hit.title, 24.0)).truncate());
                        if !v.hit.author.is_empty() {
                            ui.label(RichText::new(format!("par {} · ⬇ {}", v.hit.author, fmt_num(v.hit.downloads))).color(MUTED));
                        }
                    });
                });
                ui.add_space(12.0);
                match &v.state {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Téléchargement et lecture du modpack…").color(MUTED));
                        });
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("Impossible d'ouvrir ce modpack : {e}")).color(RED));
                        if ui.button("Réessayer").clicked() {
                            retry = true;
                        }
                    }
                    Some(Ok((info, path))) => {
                        let mut facts: Vec<String> = [("Version", &info.version), ("Minecraft", &info.minecraft), ("Chargeur", &info.loader)]
                            .iter()
                            .filter(|(_, v)| !v.is_empty())
                            .map(|(k, v)| format!("{k} {v}"))
                            .collect();
                        facts.push(format!("{} mods", info.mods.len()));
                        if info.resource_packs > 0 {
                            facts.push(format!("{} packs de ressources", info.resource_packs));
                        }
                        if info.shaders > 0 {
                            facts.push(format!("{} shaders", info.shaders));
                        }
                        ui.add(egui::Label::new(RichText::new(facts.join("   ·   ")).family(theme::semi()).color(Color32::from_rgb(230, 160, 120))).wrap());
                        ui.add_space(8.0);
                        ui.label(RichText::new("Ce qu'il contient").family(theme::semi()).color(theme::TEXT));
                        let list_h = (ui.available_height() - 120.0).max(80.0);
                        let list_w = ui.available_width() - 22.0;
                        egui::Frame::NONE.fill(Color32::from_rgb(21, 20, 18)).stroke(egui::Stroke::new(1.0_f32, theme::LINE)).corner_radius(theme::R).inner_margin(10.0).show(ui, |ui| {
                            ui.set_width(list_w);
                            egui::ScrollArea::vertical().max_height(list_h).max_width(list_w).auto_shrink([false, true]).show(ui, |ui| {
                                ui.set_max_width(list_w);
                                let cols = 2;
                                egui::Grid::new("pack_mods").num_columns(cols).spacing(egui::vec2(18.0, 6.0)).show(ui, |ui| {
                                    for (i, m) in info.mods.iter().enumerate() {
                                        ui.label(RichText::new(format!("•  {m}")).color(Color32::from_rgb(210, 204, 193)));
                                        if i % cols == cols - 1 {
                                            ui.end_row();
                                        }
                                    }
                                });
                            });
                        });
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Installer : Oxyde donne le modpack au launcher Horizon, qui télécharge chaque mod et porte ce qui peut l'être. Le modpack devient un profil à part, prêt à jouer.")
                                .small()
                                .color(MUTED),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if theme::primary_sized(ui, "Installer pour Horizon", egui::vec2(220.0, 42.0), 15.0, self.launcher.is_some()).clicked() {
                                install = Some(path.clone());
                            }
                            if ui.add(egui::Button::new("Fermer").min_size(egui::vec2(90.0, 42.0))).clicked() {
                                close = true;
                            }
                        });
                    }
                }
                if v.state.as_ref().is_none_or(|r| r.is_err()) {
                    ui.add_space(8.0);
                    if ui.button("Fermer").clicked() {
                        close = true;
                    }
                }
            });
        });
        if retry {
            let hit = v.hit.clone();
            self.open_mrpack(ctx, hit);
            return;
        }
        if let Some(p) = install {
            self.pk_open = None;
            self.install_pack(ctx, p);
            self.packs_modrinth = false;
            self.toast = Some(("Installation lancée : suis-la dans Activité. Le modpack apparaîtra dans « Mes modpacks » et dans tes profils.".into(), true));
            return;
        }
        if close {
            self.pk_open = None;
        }
    }

    fn save_pack(&mut self) {
        let (mods, files) = match self.pack_entries() {
            Ok(x) => x,
            Err(e) => {
                self.toast = Some((e, false));
                return;
            }
        };
        let file = format!("{}.{}", sanitize_file(&self.pack_name), pack::PACK_EXT);
        let Some(out) = rfd::FileDialog::new().set_file_name(&file).add_filter("Modpack Oxyde", &[pack::PACK_EXT]).save_file() else { return };
        let p = pack::Pack { format: 2, name: self.pack_name.trim().to_string(), version: "1.0.0".into(), description: String::new(), game: "26.3".into(), mods };
        let embedded = files.len();
        self.toast = Some(match pack::write_pack(&p, &files, &out) {
            Ok(()) => (
                format!("Modpack créé : {} ({} mods, {} intégré(s)). Envoie-le à tes potes : ils le glissent dans Oxyde.", file_name(&out), p.mods.len(), embedded),
                true,
            ),
            Err(e) => (e, false),
        });
    }

    fn ui_task_list(&mut self, ui: &mut egui::Ui, compact: bool) {
        let tasks = self.tasks.lock().unwrap().clone();
        if tasks.is_empty() {
            return;
        }
        if !compact {
            ui.horizontal(|ui| {
                if ui.small_button("Effacer les terminées").clicked() {
                    self.tasks.lock().unwrap().retain(|t| t.state == TaskState::Running);
                }
            });
        }
        egui::ScrollArea::vertical().id_salt(("tasks", compact)).auto_shrink([false, !compact]).max_height(if compact { 96.0 } else { f32::INFINITY }).show(ui, |ui| {
            for (i, t) in tasks.iter().enumerate().rev() {
                surface(ui, 10, |ui| {
                    ui.horizontal(|ui| {
                        match t.state {
                            TaskState::Running => {
                                ui.spinner();
                            }
                            TaskState::Done => {
                                ui.label(RichText::new("✔").color(GREEN));
                            }
                            TaskState::Failed => {
                                ui.label(RichText::new("✖").color(RED));
                            }
                        }
                        ui.label(RichText::new(&t.title).family(theme::semi()));
                        let line = match t.state {
                            TaskState::Running => t.lines.last().cloned().unwrap_or_default(),
                            _ => t.result.clone(),
                        };
                        let short: String = line.chars().take(110).collect();
                        ui.add(egui::Label::new(RichText::new(short).color(match t.state {
                            TaskState::Failed => RED,
                            TaskState::Done => Color32::from_rgb(210, 204, 193),
                            TaskState::Running => MUTED,
                        })).truncate());
                    });
                    if !compact && (!t.lines.is_empty() || !t.outcomes.is_empty()) {
                        egui::CollapsingHeader::new(RichText::new("Détails").small()).id_salt(("td", i)).show(ui, |ui| {
                            for o in &t.outcomes {
                                ui.label(RichText::new(format!("{} — {} : {}", o.method.label(), o.title, o.message)).color(Color32::WHITE));
                                for d in &o.details {
                                    ui.label(RichText::new(d).small().color(if d.starts_with('✖') { ORANGE } else { MUTED }));
                                }
                            }
                            for l in t.lines.iter().rev().take(40).rev() {
                                ui.label(RichText::new(l).small().monospace().color(MUTED));
                            }
                        });
                    }
                });
            }
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        title(ui, "Réglages", "Profil, sources, installation et mises à jour.");
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Horizon").family(theme::semi()).color(Color32::WHITE));
            match &self.launcher {
                Some(p) => ui.label(RichText::new(format!("✔ Launcher Horizon : {}", pretty_path(p))).color(GREEN)),
                None => ui.label(RichText::new("✖ Launcher Horizon introuvable : installe-le pour jouer, les profils et le portage officiel.").color(ORANGE)),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Profil : {}", self.profile_name()));
                ui.monospace(pretty_path(&self.game_dir()));
                if ui.small_button("Ouvrir le dossier").clicked() {
                    open_folder(&self.game_dir());
                }
            });
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Performances").family(theme::semi()).color(Color32::WHITE));
            match &self.machine {
                Some(m) => ui.label(
                    RichText::new(format!(
                        "Carte graphique : {} ({})",
                        m.gpu,
                        if m.tier == perf::Tier::Light { "puce intégrée : réglages légers" } else { "carte dédiée" }
                    ))
                    .color(MUTED),
                ),
                None => ui.label(RichText::new("Détection de la carte graphique…").color(MUTED)),
            };
            ui.checkbox(&mut self.s.optimize, "Optimiser le jeu pour ce PC avant chaque partie (recommandé : évite les plantages « mémoire vidéo pleine »)");
            ui.checkbox(&mut self.s.minimize_on_play, "Réduire Oxyde pendant la partie, et le rouvrir quand tu quittes le jeu");
            ui.checkbox(&mut self.s.animations, "Animations du fond d'Oxyde");
            if ui.button("Optimiser ce profil maintenant").clicked() {
                let dir = self.game_dir();
                let m = self.machine.clone().unwrap_or_else(|| perf::detect(&dir));
                let c = perf::optimize(&dir, &m);
                self.toast = Some(if c.is_empty() {
                    ("Ce profil est déjà réglé pour ton PC.".into(), true)
                } else {
                    (format!("Optimisé : {}.", c.join(", ")), true)
                });
            }
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("CurseForge (bêta)").family(theme::semi()).color(Color32::WHITE));
            ui.label(RichText::new("Pour parcourir CurseForge dans Oxyde, colle ta clé API (gratuite). Elle reste sur ce PC.").small().color(MUTED));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.s.cf_key).password(true).hint_text("Clé API CurseForge").desired_width(320.0));
                ui.hyperlink_to("Obtenir une clé", curseforge::KEY_PAGE);
            });
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Convertisseur Oxyde (dernier recours)").family(theme::semi()).color(Color32::WHITE));
            ui.horizontal_wrapped(|ui| {
                ui.label("Fichiers convertis :");
                ui.monospace(pretty_path(&self.s.output_dir));
                if ui.small_button("Changer").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.s.output_dir = d;
                    }
                }
            });
            ui.checkbox(&mut self.s.generate_registries, "Créer les blocs et objets (modèle vanilla deviné d'après le nom)");
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Installation d'Oxyde").family(theme::semi()).color(Color32::WHITE));
            if setup::is_installed_copy() {
                ui.label(RichText::new(format!("✔ Installé dans {}", pretty_path(&setup::install_dir()))).color(GREEN));
                if self.confirm_uninstall {
                    ui.horizontal(|ui| {
                        ui.label("Désinstaller Oxyde ? Tes mods et profils Horizon ne sont pas touchés.");
                        if ui.button(RichText::new("Désinstaller").color(RED)).clicked() {
                            match setup::uninstall() {
                                Ok(()) => std::process::exit(0),
                                Err(e) => self.toast = Some((e, false)),
                            }
                        }
                        if ui.small_button("Annuler").clicked() {
                            self.confirm_uninstall = false;
                        }
                    });
                } else if ui.small_button("Désinstaller Oxyde…").clicked() {
                    self.confirm_uninstall = true;
                }
            } else {
                ui.label(RichText::new("Oxyde tourne sans être installé.").color(MUTED));
                if ui.button("Installer Oxyde sur ce PC…").clicked() {
                    self.setup_open = true;
                }
            }
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Mises à jour").family(theme::semi()).color(Color32::WHITE));
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.s.auto_update, "Mettre à jour Oxyde automatiquement");
                if update::REPO.is_some() && ui.small_button("Vérifier maintenant").clicked() {
                    self.check_updates(ui.ctx().clone());
                }
                let s = match &*self.upd.lock().unwrap() {
                    Upd::Checking => "vérification…".to_string(),
                    Upd::UpToDate => "à jour ✔".to_string(),
                    Upd::Error(e) => e.clone(),
                    _ => String::new(),
                };
                ui.label(RichText::new(s).color(MUTED));
            });
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Créé par full_money").family(theme::semi()).color(Color32::WHITE));
            ui.label("Oxyde est gratuit et open source (licence MIT, © 2026 full_money). Projet non officiel, sans lien avec Mojang, Microsoft ou l'équipe Horizon.");
            ui.hyperlink_to("Code source et mises à jour", "https://github.com/0full-money0/oxyde");
        });
    }

    // ------------------------------------------------------------ notifications et activité flottantes
    fn ui_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, ok)) = self.toast.clone() else { return };
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0)).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(theme::SURFACE2)
                .stroke(egui::Stroke::new(1.0_f32, if ok { Color32::from_rgb(60, 120, 100) } else { Color32::from_rgb(130, 60, 50) }))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .shadow(egui::Shadow { offset: [5, 5], blur: 0, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.set_max_width((screen.width() - 120.0).min(720.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if ok { "✔" } else { "!" }).family(theme::heavy()).color(if ok { GREEN } else { RED }));
                        ui.add(egui::Label::new(RichText::new(&msg).color(theme::TEXT)).wrap());
                        if ui.small_button("OK").clicked() {
                            self.toast = None;
                        }
                    });
                });
        });
    }

    fn ui_activity(&mut self, ctx: &egui::Context) {
        let tasks = self.tasks.lock().unwrap().clone();
        if tasks.is_empty() {
            return;
        }
        let running = tasks.iter().filter(|t| t.state == TaskState::Running).count();
        egui::Area::new(egui::Id::new("activity")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0)).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0_f32, theme::LINE))
                .corner_radius(4.0)
                .inner_margin(12.0)
                .shadow(egui::Shadow { offset: [5, 5], blur: 0, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    let w = if self.tasks_open { 420.0 } else { 220.0 };
                    ui.set_width(w);
                    ui.set_max_width(w);
                    ui.horizontal(|ui| {
                        if running > 0 {
                            ui.spinner();
                        } else {
                            ui.label(RichText::new("✔").color(GREEN));
                        }
                        let label = if running > 0 { format!("{running} en cours") } else { "Activité".to_string() };
                        ui.label(RichText::new(label).family(theme::semi()).color(theme::TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(if self.tasks_open { "Réduire" } else { "Ouvrir" }).clicked() {
                                self.tasks_open = !self.tasks_open;
                            }
                        });
                    });
                    if self.tasks_open {
                        self.ui_task_list(ui, true);
                        ui.horizontal(|ui| {
                            if ui.small_button("Tout voir").clicked() {
                                self.activity_window = true;
                            }
                            if running == 0 && ui.small_button("Effacer").clicked() {
                                self.tasks.lock().unwrap().clear();
                            }
                        });
                    }
                });
        });
    }

    // ------------------------------------------------------------ écran de lancement : OXYDE × HORIZON
    fn ui_launch(&mut self, ctx: &egui::Context, now: f64) {
        let demo = std::env::var("OXYDE_TOUR").is_ok() || std::env::var("OXYDE_DEMO_LAUNCH").is_ok();
        let Some(l) = &mut self.launch else { return };
        let t = (now - l.started) as f32;
        if demo && l.result.is_none() && t > 2.6 {
            l.result = Some(Ok(()));
        }
        if l.result.is_some() && l.done_at.is_none() {
            l.done_at = Some(now);
        }
        let ok = matches!(l.result, Some(Ok(())));
        let since_done = l.done_at.map(|d| (now - d) as f32).unwrap_or(0.0);
        let hold = if std::env::var("OXYDE_SHOT").is_ok() { 60.0 } else { 2.4 };
        let fade_in = (t / 0.35).clamp(0.0, 1.0);
        let fade_out = if ok { 1.0 - ((since_done - hold) / 0.45).clamp(0.0, 1.0) } else { 1.0 };
        let alpha = fade_in * fade_out;
        if alpha <= 0.0 {
            self.launch = None;
            if ok && self.minimize_after_launch && !demo {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            self.minimize_after_launch = false;
            return;
        }
        let (name, id, prep, result) = (l.name.clone(), l.id.clone(), l.prep.clone(), l.result.clone());
        let inst = id.as_ref().and_then(|id| self.instances.iter().find(|i| &i.id == id)).cloned().or_else(|| self.profile().cloned());
        let name = inst.as_ref().filter(|i| !i.id.starts_with("port:") || id.is_some()).map(profile_label).unwrap_or(name);

        // étapes : profil, optimisation, mods, démarrage
        let s1 = t > 0.35;
        let s2 = s1 && prep.is_some() && t > 0.8;
        let s3 = s2 && t > 1.25;
        let s4 = ok;
        let mut raw = 0.06 + if s1 { 0.18 } else { 0.0 } + if s2 { 0.22 } else { 0.0 } + if s3 { 0.14 } else { 0.0 };
        if s4 {
            raw = 1.0;
        } else if s3 {
            raw += 0.32 * (1.0 - (-(t - 1.25) / 5.0).exp());
        }
        let progress = ctx.animate_value_with_time(egui::Id::new("launch_p"), raw, 0.5);

        let mut close = false;
        let mut open_launcher = false;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("launch")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, egui::Sense::click());
            ui.multiply_opacity(alpha);
            let p = ui.painter().clone();
            let tt = now as f32;
            p.rect_filled(screen, 0.0, theme::BG);
            // deux lueurs : la rouille d'Oxyde, le bleu froid de Horizon
            fx::soft_glow(&p, screen.center() + egui::vec2(-screen.width() * 0.22, -60.0 + (tt * 0.7).sin() * 14.0), 420.0, Color32::from_rgba_unmultiplied(255, 110, 40, 46));
            fx::soft_glow(&p, screen.center() + egui::vec2(screen.width() * 0.22, -40.0 + (tt * 0.6).cos() * 14.0), 420.0, Color32::from_rgba_unmultiplied(80, 120, 255, 34));

            let top = screen.center().y - 190.0;
            fx::lockup(&p, egui::pos2(screen.center().x, top), 1.0, 1.0, Some(&self.logo), tt);
            fx::spaced(&p, egui::pos2(screen.center().x, top + 52.0), egui::Align2::CENTER_CENTER, "LANCÉ PAR OXYDE  ·  MOTEUR HORIZON, MINECRAFT RUST EDITION", 11.0, theme::semi(), theme::SUBTLE, 2.0);

            // carte du profil
            let card = egui::Rect::from_center_size(egui::pos2(screen.center().x, top + 132.0), egui::vec2(480.0, 84.0));
            p.rect_filled(card.translate(egui::vec2(0.0, 8.0)), 4.0, Color32::from_black_alpha(90));
            p.rect_filled(card, 4.0, theme::SURFACE);
            p.rect_stroke(card, 4.0, egui::Stroke::new(1.0_f32, theme::LINE), egui::StrokeKind::Inside);
            let av = egui::Rect::from_min_size(card.min + egui::vec2(14.0, 14.0), egui::vec2(56.0, 56.0));
            match &inst {
                Some(i) => self.avatar(ui, av, i, 3.0),
                None => fx::rounded_gradient(&p, av, 4.0, theme::ACCENT_A, theme::ACCENT_B),
            }
            p.text(egui::pos2(av.right() + 16.0, card.top() + 30.0), egui::Align2::LEFT_CENTER, &name, egui::FontId::new(21.0, theme::semi()), Color32::WHITE);
            let sub = match &prep {
                Some((_, gpu, mods)) => format!("{mods} mod(s) actif(s)  ·  {gpu}"),
                None => "préparation du profil…".into(),
            };
            p.text(egui::pos2(av.right() + 16.0, card.top() + 56.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(13.0), theme::SUBTLE);

            // barre de progression en dégradé, avec un reflet qui glisse
            let bar = egui::Rect::from_center_size(egui::pos2(screen.center().x, card.bottom() + 34.0), egui::vec2(480.0, 6.0));
            p.rect_filled(bar, 3.0, Color32::from_rgb(52, 49, 43));
            let fill = egui::Rect::from_min_size(bar.min, egui::vec2((bar.width() * progress).max(6.0), bar.height()));
            fx::rounded_gradient(&p, fill, 3.0, theme::ACCENT_A, theme::ACCENT_B);
            if !s4 {
                let sx = fill.left() + ((tt * 0.7) % 1.0) * fill.width();
                fx::soft_glow(&p, egui::pos2(sx, bar.center().y), 26.0, Color32::from_rgba_unmultiplied(255, 220, 190, 120));
            }

            // étapes
            let opt_text = match &prep {
                Some((c, _, _)) if c.is_empty() => "Réglages déjà adaptés à ton PC".to_string(),
                Some((c, _, _)) => format!("Optimisé pour ton PC : {}", c.join(", ")),
                None if !self.s.optimize => "Optimisation désactivée".to_string(),
                None => "Optimisation pour ton PC".to_string(),
            };
            let mods_text = match &prep {
                Some((_, _, n)) => format!("{n} mod(s) vérifié(s)"),
                None => "Vérification des mods".to_string(),
            };
            let start_text = match &result {
                Some(Ok(())) => "Horizon est lancé · bon jeu !".to_string(),
                Some(Err(_)) => "Horizon n'a pas démarré".to_string(),
                None => "Démarrage de Horizon".to_string(),
            };
            let steps = [
                (s1, !s1, format!("Profil « {name} » prêt")),
                (s2, s1 && !s2, opt_text),
                (s3, s2 && !s3, mods_text),
                (s4, s3 && result.is_none(), start_text),
            ];
            let mut y = bar.bottom() + 30.0;
            let x = bar.left() + 4.0;
            for (i, (done, active, text)) in steps.iter().enumerate() {
                let c = egui::pos2(x + 8.0, y);
                let failed = i == 3 && matches!(result, Some(Err(_)));
                if failed {
                    p.circle_filled(c, 8.0, RED);
                    p.text(c, egui::Align2::CENTER_CENTER, "!", egui::FontId::new(11.0, theme::heavy()), Color32::WHITE);
                } else if *done {
                    p.circle_filled(c, 8.0, theme::ACCENT_B);
                    p.line(vec![c + egui::vec2(-3.5, 0.0), c + egui::vec2(-1.0, 2.6), c + egui::vec2(3.8, -2.8)], egui::Stroke::new(1.8_f32, Color32::WHITE));
                } else if *active {
                    fx::spinner(&p, c, 7.0, tt, theme::ACCENT_A);
                } else {
                    p.circle_stroke(c, 7.0, egui::Stroke::new(1.2_f32, Color32::from_rgb(72, 68, 60)));
                }
                let col = if failed {
                    RED
                } else if *done || *active {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(120, 113, 102)
                };
                let g = p.layout(text.clone(), egui::FontId::proportional(14.0), col, bar.width() - 30.0);
                let h = g.size().y;
                p.galley(egui::pos2(x + 26.0, y - 9.0), g, col);
                y += h.max(18.0) + 14.0;
            }

            if let Some(Err(e)) = &result {
                let r = egui::Rect::from_min_size(egui::pos2(bar.left(), y), egui::vec2(bar.width(), 120.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                    ui.label(RichText::new(e).color(theme::SUBTLE));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if theme::primary_sized(ui, "Ouvrir le launcher Horizon", egui::vec2(0.0, 36.0), 14.0, true).clicked() {
                            open_launcher = true;
                        }
                        if ui.button("Fermer").clicked() {
                            close = true;
                        }
                    });
                });
            }

            fx::spaced(&p, screen.center_bottom() - egui::vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, "OXYDE × HORIZON  ·  BY FULL_MONEY", 10.5, theme::semi(), Color32::from_rgb(100, 94, 85), 2.5);
        });
        if open_launcher {
            if let Some(exe) = &self.launcher {
                let _ = game::open_launcher(exe);
            }
            close = true;
        }
        if close {
            self.launch = None;
        }
    }

    // ------------------------------------------------------------ profils
    fn ui_profiles(&mut self, ui: &mut egui::Ui) {
        title(ui, "Profils", "Un profil, c'est une partie à part : ses mods, ses mondes, ses réglages. Donne-lui un nom et une image, ajoute-lui des mods, lance-le.");
        if self.launcher.is_none() {
            ui.label(RichText::new("Installe le launcher Horizon pour créer des profils.").color(ORANGE));
            ui.hyperlink_to("Installer Horizon (gratuit)", game::INSTALL_PAGE);
            return;
        }
        enum Act {
            Play(String),
            Mods(String),
            Share(String),
            Edit(String),
            Select(String),
            New,
        }
        let mut act = None;
        let list: Vec<engine::Instance> = self.profiles().into_iter().cloned().collect();
        let current = self.profile().map(|p| p.id.clone());
        let gap = 16.0;
        let w = ui.available_width();
        let cols = (((w + gap) / (300.0 + gap)).floor() as usize).max(1);
        let card_w = ((w - gap * (cols - 1) as f32) / cols as f32).floor();
        let card_h = 284.0;
        let idx: Vec<usize> = (0..=list.len()).collect();
        for chunk in idx.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &i in chunk {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(card_w, card_h), egui::Sense::click());
                    let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15);
                    let r = r.translate(egui::vec2(0.0, -3.0 * hov));
                    let p = ui.painter().clone();
                    if i == list.len() {
                        // carte « nouveau profil »
                        p.rect_filled(r, 4.0, if hov > 0.5 { theme::SURFACE2 } else { theme::SURFACE });
                        let col = if hov > 0.5 { theme::ACCENT_A } else { Color32::from_rgb(80, 75, 66) };
                        fx::dashed_rect(&p, r.shrink(1.0), 4.0, egui::Stroke::new(1.5_f32, col));
                        let c = r.center() - egui::vec2(0.0, 18.0);
                        p.circle_filled(c, 28.0, Color32::from_rgb(52, 49, 43));
                        p.line_segment([c - egui::vec2(10.0, 0.0), c + egui::vec2(10.0, 0.0)], egui::Stroke::new(2.4_f32, theme::ACCENT_A));
                        p.line_segment([c - egui::vec2(0.0, 10.0), c + egui::vec2(0.0, 10.0)], egui::Stroke::new(2.4_f32, theme::ACCENT_A));
                        p.text(c + egui::vec2(0.0, 50.0), egui::Align2::CENTER_CENTER, "Nouveau profil", egui::FontId::new(17.0, theme::semi()), theme::TEXT);
                        p.text(c + egui::vec2(0.0, 74.0), egui::Align2::CENTER_CENTER, "nom, image, mods à part", egui::FontId::proportional(12.5), theme::SUBTLE);
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            act = Some(Act::New);
                        }
                        continue;
                    }
                    let prof = &list[i];
                    // tout le contenu de la carte vit dans un sous-espace : il ne pousse pas la grille
                    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
                    let ui = &mut cui;
                    let sel = current.as_ref() == Some(&prof.id);
                    p.rect_filled(r.translate(egui::vec2(0.0, 8.0)), 4.0, Color32::from_black_alpha((50.0 + 50.0 * hov) as u8));
                    p.rect_filled(r, 4.0, theme::SURFACE);
                    let border = if sel { theme::ACCENT_A } else if hov > 0.5 { Color32::from_rgb(92, 84, 72) } else { theme::LINE };
                    p.rect_stroke(r, 4.0, egui::Stroke::new(if sel { 1.6_f32 } else { 1.0_f32 }, border), egui::StrokeKind::Inside);
                    // couverture
                    let cover = egui::Rect::from_min_size(r.min + egui::vec2(8.0, 8.0), egui::vec2(r.width() - 16.0, 112.0));
                    match self.s.profile_images.get(&prof.id).filter(|f| f.exists()) {
                        Some(img) => {
                            ui.put(cover, egui::Image::new(format!("file://{}", img.display())).fit_to_exact_size(cover.size()).maintain_aspect_ratio(false).corner_radius(4.0));
                        }
                        None => {
                            let c = profile_color(profile_color_index(prof));
                            fx::rounded_gradient(&p, cover, 4.0, c, fx::darken(c, 0.25));
                            fx::cover_pattern(&p, cover, c);
                        }
                    }
                    if sel {
                        let pill = egui::Rect::from_min_size(cover.min + egui::vec2(10.0, 10.0), egui::vec2(62.0, 22.0));
                        p.rect_filled(pill, 4.0, Color32::from_black_alpha(150));
                        p.text(pill.center(), egui::Align2::CENTER_CENTER, "ACTIF", egui::FontId::new(10.5, theme::heavy()), Color32::WHITE);
                    }
                    // vignette qui chevauche la couverture
                    let av = egui::Rect::from_min_size(egui::pos2(cover.left() + 14.0, cover.bottom() - 30.0), egui::vec2(56.0, 56.0));
                    p.rect_filled(av.expand(3.0), 4.0, theme::SURFACE);
                    self.avatar(ui, av, prof, 3.0);
                    let tx = av.right() + 12.0;
                    let name_clip = egui::Rect::from_min_max(egui::pos2(tx, cover.bottom()), egui::pos2(r.right() - 12.0, cover.bottom() + 30.0));
                    p.with_clip_rect(name_clip).text(egui::pos2(tx, cover.bottom() + 15.0), egui::Align2::LEFT_CENTER, profile_label(prof), egui::FontId::new(17.0, theme::semi()), Color32::WHITE);
                    let n = self.profile_mods.get(&prof.id).copied().unwrap_or(0);
                    let kind = match prof.kind.as_str() {
                        "default" => "principal",
                        "port" => "modpack porté",
                        _ => "profil",
                    };
                    p.text(egui::pos2(r.left() + 22.0, av.bottom() + 18.0), egui::Align2::LEFT_CENTER, format!("{kind}  ·  {n} mod(s)  ·  {}", played(prof.played_secs)), egui::FontId::proportional(12.5), theme::SUBTLE);
                    // boutons
                    let inner = egui::Rect::from_min_max(egui::pos2(r.left() + 14.0, av.bottom() + 36.0), egui::pos2(r.right() - 14.0, r.bottom() - 12.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                        if theme::primary_sized(ui, "▶  Jouer", egui::vec2(inner.width(), 38.0), 15.0, !self.game_running && !self.launching).clicked() {
                            act = Some(Act::Play(prof.id.clone()));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let bw = ((inner.width() - 12.0) / 3.0).floor();
                            let small = |ui: &mut egui::Ui, t: &str, tip: &str| ui.add_sized(egui::vec2(bw, 30.0), egui::Button::new(RichText::new(t).size(13.0))).on_hover_text(tip).clicked();
                            if small(ui, "+ Mods", "Ajouter des mods dans ce profil") {
                                act = Some(Act::Mods(prof.id.clone()));
                            }
                            if small(ui, "Partager", "Faire un modpack de ce profil pour tes potes") {
                                act = Some(Act::Share(prof.id.clone()));
                            }
                            if small(ui, "Modifier", "Nom, image, suppression") {
                                act = Some(Act::Edit(prof.id.clone()));
                            }
                        });
                    });
                    if act.is_none() && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !sel {
                        act = Some(Act::Select(prof.id.clone()));
                    }
                }
            });
            ui.add_space(gap);
        }
        let ctx = ui.ctx().clone();
        match act {
            Some(Act::Play(id)) => {
                self.s.profile = Some(id.clone());
                self.refresh_library();
                self.play(&ctx, Some(id));
            }
            Some(Act::Mods(id)) => {
                self.select_profile(id);
                self.tab = Tab::Mods;
                if self.mr_hits.is_empty() {
                    self.search_modrinth(&ctx, 0);
                }
            }
            Some(Act::Share(id)) => {
                self.select_profile(id);
                self.pack_name = self.profile_name();
                self.tab = Tab::Packs;
            }
            Some(Act::Edit(id)) => {
                let p = self.instances.iter().find(|i| i.id == id).cloned();
                self.open_editor(p.as_ref());
            }
            Some(Act::Select(id)) => self.select_profile(id),
            Some(Act::New) => self.open_editor(None),
            None => {}
        }
    }

    fn ui_profile_editor(&mut self, ctx: &egui::Context) {
        let Some(mut e) = self.edit.take() else { return };
        let screen = ctx.screen_rect();
        let mut keep = true;
        let mut save = false;
        let mut delete = false;
        let is_default = e.id.as_ref().and_then(|id| self.instances.iter().find(|i| &i.id == id)).is_some_and(|i| i.kind == "default");
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !e.busy {
            keep = false;
        }
        egui::Area::new(egui::Id::new("profile_edit")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let bg = ui.allocate_rect(screen, egui::Sense::click());
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(170));
            let card = egui::Rect::from_center_size(screen.center(), egui::vec2(520.0, 500.0).min(screen.size() - egui::vec2(32.0, 32.0)));
            ui.painter().rect_filled(card.translate(egui::vec2(0.0, 14.0)), 4.0, Color32::from_black_alpha(120));
            ui.painter().rect_filled(card, 4.0, theme::SURFACE);
            ui.painter().rect_stroke(card, 4.0, egui::Stroke::new(1.0_f32, theme::LINE), egui::StrokeKind::Inside);
            if bg.clicked() && !card.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())) && !e.busy {
                keep = false;
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(card.shrink(26.0)).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.label(theme::label_caps(if e.id.is_none() { "Nouveau profil" } else { "Modifier le profil" }));
                ui.add_space(8.0);
                // aperçu
                let (cover, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
                let c = profile_color(e.color);
                match e.image.as_ref().filter(|f| f.exists()) {
                    Some(img) => {
                        ui.put(cover, egui::Image::new(format!("file://{}", img.display())).fit_to_exact_size(cover.size()).maintain_aspect_ratio(false).corner_radius(4.0));
                    }
                    None => {
                        fx::rounded_gradient(ui.painter(), cover, 4.0, c, fx::darken(c, 0.25));
                        fx::cover_pattern(ui.painter(), cover, c);
                    }
                }
                let shown = if e.name.trim().is_empty() { "Nom du profil" } else { e.name.trim() };
                let p = ui.painter();
                p.text(cover.left_bottom() + egui::vec2(18.0, -22.0), egui::Align2::LEFT_CENTER, shown, egui::FontId::new(24.0, theme::heavy()), Color32::from_black_alpha(140));
                p.text(cover.left_bottom() + egui::vec2(16.0, -24.0), egui::Align2::LEFT_CENTER, shown, egui::FontId::new(24.0, theme::heavy()), Color32::WHITE);
                ui.add_space(16.0);
                ui.label(RichText::new("Nom").family(theme::semi()).color(theme::TEXT));
                let te = ui.add(egui::TextEdit::singleline(&mut e.name).hint_text("Ex. Survie entre potes").desired_width(f32::INFINITY).margin(egui::vec2(12.0, 9.0)).char_limit(40));
                if e.id.is_none() && !ui.ctx().memory(|m| m.has_focus(te.id)) && e.name.is_empty() {
                    te.request_focus();
                }
                if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !e.name.trim().is_empty() {
                    save = true;
                }
                ui.add_space(14.0);
                ui.label(RichText::new("Image").family(theme::semi()).color(theme::TEXT));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for i in 0..PROFILE_COLORS.len() {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                        let col = profile_color(i);
                        ui.painter().circle_filled(r.center(), 13.0, col);
                        if e.color == i && e.image.is_none() {
                            ui.painter().circle_stroke(r.center(), 15.5, egui::Stroke::new(2.0_f32, Color32::WHITE));
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            e.color = i;
                            e.image = None;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Choisir une image…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file() {
                            e.image = Some(f);
                        }
                    }
                    if e.image.is_some() && ui.button("Retirer l'image").clicked() {
                        e.image = None;
                    }
                    ui.label(RichText::new("PNG, JPG ou WebP").small().color(MUTED));
                });
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    let label = if e.busy {
                        "Enregistrement…"
                    } else if e.id.is_none() {
                        "Créer le profil"
                    } else {
                        "Enregistrer"
                    };
                    if theme::primary_sized(ui, label, egui::vec2(180.0, 42.0), 15.0, !e.busy && !e.name.trim().is_empty()).clicked() {
                        save = true;
                    }
                    if ui.add_enabled(!e.busy, egui::Button::new("Annuler").min_size(egui::vec2(90.0, 42.0))).clicked() {
                        keep = false;
                    }
                });
                if e.id.is_some() && !is_default {
                    ui.add_space(12.0);
                    if e.confirm_delete {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Supprimer ce profil ? Son dossier (mondes, mods) part dans la Corbeille.").color(RED));
                            if ui.button(RichText::new("Supprimer").color(RED)).clicked() {
                                delete = true;
                            }
                            if ui.small_button("Non").clicked() {
                                e.confirm_delete = false;
                            }
                        });
                    } else if ui.add(egui::Button::new(RichText::new("Supprimer ce profil…").color(RED)).frame(false)).clicked() {
                        e.confirm_delete = true;
                    }
                }
            });
        });
        if save && !e.busy && !e.name.trim().is_empty() {
            if let Some(exe) = self.launcher.clone() {
                e.busy = true;
                let (id, name, color, image) = (e.id.clone(), e.name.trim().to_string(), e.color, e.image.clone());
                let old_name = id.as_ref().and_then(|id| self.instances.iter().find(|i| &i.id == id)).map(|i| i.name.clone());
                self.spawn(ctx, move || {
                    let eng = engine::Engine { exe };
                    let r = (|| {
                        let (id, created) = match id {
                            Some(id) => {
                                if old_name.as_deref() != Some(name.as_str()) {
                                    eng.rename_instance(&id, &name)?;
                                }
                                (id, false)
                            }
                            None => (eng.create_instance(&name)?.id, true),
                        };
                        let _ = eng.set_icon(&id, &color_hex(color));
                        let img = match image {
                            Some(src) if !src.starts_with(profiles_dir()) => {
                                let dir = profiles_dir();
                                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                                let ext = src.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into());
                                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                                let dest = dir.join(format!("{}-{stamp}.{ext}", sanitize_file(&id)));
                                std::fs::copy(&src, &dest).map_err(|e| format!("image : {e}"))?;
                                Some(dest)
                            }
                            other => other,
                        };
                        Ok((id, img, created))
                    })();
                    Msg::ProfileSaved(r)
                });
            }
        }
        if delete {
            if let (Some(exe), Some(id)) = (self.launcher.clone(), e.id.clone()) {
                e.busy = true;
                self.spawn(ctx, move || Msg::ProfileDeleted(engine::Engine { exe }.delete_instance(&id).map(|_| id)));
            }
        }
        if keep {
            self.edit = Some(e);
        }
    }

    // ------------------------------------------------------------ installeur
    fn ui_setup(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        let now = ctx.input(|i| i.time) as f32;
        let mut later = false;
        let mut go = false;
        egui::Area::new(egui::Id::new("setup")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, egui::Sense::click());
            let p = ui.painter();
            p.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(19, 18, 16, 250));
            let card = egui::Rect::from_center_size(screen.center(), egui::vec2(640.0, 440.0).min(screen.size() - egui::vec2(40.0, 40.0)));
            p.rect_filled(card.translate(egui::vec2(0.0, 12.0)), 4.0, Color32::from_black_alpha(110));
            fx::rounded_gradient(p, card, 4.0, theme::SURFACE, theme::SURFACE);
            p.rect_stroke(card, 4.0, egui::Stroke::new(1.0_f32, theme::LINE), egui::StrokeKind::Inside);
            fx::diorama(p, egui::pos2(card.right() - 130.0, card.top() + 150.0), 17.0, now);
            ui.scope_builder(egui::UiBuilder::new().max_rect(card.shrink(40.0)), |ui| {
                ui.label(RichText::new("INSTALLATION").size(12.0).family(theme::semi()).color(theme::ACCENT_B));
                ui.label(RichText::new("Bienvenue dans Oxyde").size(32.0).family(theme::semi()).color(Color32::WHITE));
                ui.label(RichText::new("Mods, modpacks et lancement pour Horizon.").size(16.0).color(Color32::from_rgb(190, 182, 168)));
                ui.add_space(26.0);
                match &self.setup_state {
                    Some(Ok(_)) => {
                        ui.label(RichText::new("✔ Oxyde est installé. Il se relance depuis ton menu Démarrer…").size(16.0).color(GREEN));
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(format!("✖ Installation impossible : {e}")).color(RED));
                    }
                    None => {
                        ui.label(RichText::new(format!("Dossier : {}", pretty_path(&setup::install_dir()))).small().color(MUTED));
                        ui.label(RichText::new("Sans droits administrateur. Raccourci dans le menu Démarrer.").small().color(MUTED));
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.setup_desktop, "Créer un raccourci sur le bureau");
                    }
                }
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    if self.setup_state.as_ref().is_none_or(|r| r.is_err()) {
                        let label = if self.setup_busy { "Installation…" } else { "Installer Oxyde" };
                        if theme::primary_sized(ui, label, egui::vec2(220.0, 50.0), 17.0, !self.setup_busy).clicked() {
                            go = true;
                        }
                        ui.add_space(10.0);
                        if ui.add(egui::Button::new(RichText::new("Utiliser sans installer").color(MUTED)).frame(false)).clicked() {
                            later = true;
                        }
                    }
                });
            });
        });
        if go {
            self.setup_busy = true;
            let desktop = self.setup_desktop;
            self.spawn(ctx, move || Msg::SetupDone(setup::install(desktop)));
        }
        if later {
            self.setup_open = false;
            self.s.skip_setup = true;
        }
        if let Some(Ok(_)) = &self.setup_state {
            if !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("relaunch")).unwrap_or(false)) {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("relaunch"), true));
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(1600));
                    setup::launch_installed();
                    std::process::exit(0);
                });
            }
        }
    }
}

fn drop_zone(ui: &mut egui::Ui, big: &str, small: &str) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    egui::Frame::NONE
        .stroke(egui::Stroke::new(if hovering { 2.5_f32 } else { 1.5_f32 }, if hovering { RUST } else { Color32::from_gray(70) }))
        .corner_radius(4.0)
        .fill(if hovering { Color32::from_rgb(48, 36, 28) } else { Color32::from_rgb(27, 26, 23) })
        .inner_margin(24.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("⬇").size(28.0).color(RUST));
                ui.label(RichText::new(big).size(19.0).family(theme::semi()));
                ui.label(RichText::new(small).color(MUTED));
            });
        });
    ui.add_space(6.0);
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.add_paths(ctx, dropped);
        }

        let now = ctx.input(|i| i.time);
        let start = *self.started.get_or_insert(now);
        let elapsed = (now - start) as f32;
        // Oxyde ne consomme presque rien quand il n'est pas regardé ou que le jeu tourne
        let (focused, minimized) = ctx.input(|i| (i.viewport().focused.unwrap_or(true), i.viewport().minimized.unwrap_or(false)));
        let showing = self.launch.is_some() || !self.splash_gone || self.tour.is_some() || std::env::var("OXYDE_SHOT").is_ok() || std::env::var("OXYDE_REC").is_ok();
        let idle = !showing && (minimized || self.game_running || !focused || !self.s.animations);
        ctx.request_repaint_after(std::time::Duration::from_millis(if idle { 1000 } else { 33 }));
        self_screenshot(ctx, elapsed);
        self.fx.background(ctx, if self.s.animations && !self.game_running { 0.55 } else { 0.0 });
        if self.tab != self.prev_tab {
            self.prev_tab = self.tab;
            self.tab_changed = now;
        }

        self.ui_topbar(ctx);
        self.ui_toast(ctx);
        self.ui_activity(ctx);

        let enter = (((now - self.tab_changed) as f32) / 0.22).clamp(0.0, 1.0);
        let enter = 1.0 - (1.0 - enter).powi(3);
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(32, 24))).show(ctx, |ui| {
            ui.multiply_opacity(enter);
            ui.add_space((1.0 - enter) * 14.0);
            self.ui_update_banner(ui);
            match self.tab {
                Tab::Home => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_home(ui));
                }
                Tab::Mods => self.ui_mods(ui),
                Tab::Profiles => {
                    egui::ScrollArea::vertical().id_salt("profiles").show(ui, |ui| self.ui_profiles(ui));
                }
                Tab::Packs => {
                    egui::ScrollArea::vertical().id_salt("packs").show(ui, |ui| self.ui_packs(ui));
                }
                Tab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_settings(ui));
                }
            }
        });
        if self.activity_window {
            let mut open = true;
            let tasks_win = egui::Window::new("Activité").open(&mut open).default_size([640.0, 460.0]).collapsible(false);
            tasks_win.show(ctx, |ui| self.ui_task_list(ui, false));
            self.activity_window = open;
        }
        if self.edit.is_some() {
            self.ui_profile_editor(ctx);
        }
        if self.pk_open.is_some() {
            self.ui_pack_view(ctx);
        }
        self.ui_launch(ctx, now);
        if self.setup_open && self.splash_gone {
            self.ui_setup(ctx);
        }
        self.resize_edges(ctx);

        if self.splash_gone {
            if let Some(mut tour) = self.tour.take() {
                for key in tour.step(ctx, now) {
                    self.tour_click(ctx, key);
                }
                tour.draw(ctx, now);
                self.tour = Some(tour);
            }
        }

        if !self.splash_gone {
            let ready = self.registry.is_some() || self.registry_err.is_some();
            let target = 0.25 + if ready { 0.45 } else { 0.0 } + if elapsed > 0.8 { 0.30 } else { 0.0 };
            let progress = ctx.animate_value_with_time(egui::Id::new("splash_p"), target, 0.45);
            let status = if elapsed < 0.4 {
                "démarrage"
            } else if !ready {
                "lecture du registre horizon"
            } else if elapsed < 0.8 {
                "recherche du jeu"
            } else {
                "prêt"
            };
            if self.splash_done_at.is_none() && ((ready && elapsed > 1.1) || elapsed > 3.0) {
                self.splash_done_at = Some(now);
            }
            let alpha = match self.splash_done_at {
                Some(d) => 1.0 - ((now - d) as f32 / 0.45).clamp(0.0, 1.0),
                None => 1.0,
            };
            if alpha <= 0.0 {
                self.splash_gone = true;
                self.tab_changed = now;
            } else {
                self.fx.splash(ctx, progress, status, alpha, &self.logo);
            }
        }
    }
}
