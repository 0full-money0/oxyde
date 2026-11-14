#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod engine;
mod fx;
mod game;
mod installer;
mod modrinth;
mod net;
mod pack;
mod registry;
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
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([820.0, 540.0])
            .with_decorations(false) // barre de titre dessinée par Oxyde
            .with_transparent(true)
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native("Oxyde", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
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
        _ => None,
    }
}

/// Dossier de jeu de l'instance principale (celle du launcher Horizon, sinon le dossier par défaut).
fn main_game_dir() -> PathBuf {
    if let Some(e) = engine::Engine::find() {
        if let Some(i) = e.instances().into_iter().find(|i| i.kind == "default").or_else(|| e.instances().into_iter().find(|i| i.selected)) {
            return i.home;
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
    let pack = engine.packs().into_iter().find(|p| p.pack == id);
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
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
        Self { output_dir: base.join("Oxyde"), game_dir: None, generate_registries: true, auto_update: true }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Store,
    Packs,
    Library,
    Import,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum Source {
    Horizon,
    Modrinth,
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
    Modrinth { offset: u64, result: Result<modrinth::SearchResp, String> },
    Launcher { instances: Vec<engine::Instance>, mods: Vec<engine::InstalledEntry>, packs: Vec<engine::PortedPack> },
    TaskDone,
    Launched(Result<(), String>),
}

struct App {
    s: Settings,
    tab: Tab,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    registry: Option<Arc<registry::Index>>,
    registry_err: Option<String>,
    // launcher Horizon
    launcher: Option<PathBuf>,
    instances: Vec<engine::Instance>,
    launcher_mods: Vec<engine::InstalledEntry>,
    packs: Vec<engine::PortedPack>,
    // magasin
    source: Source,
    query: String,
    kind: String,
    shown: usize,
    mr_hits: Vec<modrinth::Hit>,
    mr_total: u64,
    mr_loading: bool,
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
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let s: Settings = cc.storage.and_then(|st| eframe::get_value(st, eframe::APP_KEY)).unwrap_or_default();
        apply_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = Self {
            s,
            tab: Tab::Home,
            tx,
            rx,
            registry: None,
            registry_err: None,
            launcher: game::launcher_exe(),
            instances: vec![],
            launcher_mods: vec![],
            packs: vec![],
            source: Source::Horizon,
            query: String::new(),
            kind: "all".into(),
            shown: 40,
            mr_hits: vec![],
            mr_total: 0,
            mr_loading: false,
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
            fx: fx::Fx::default(),
            logo: load_logo(&cc.egui_ctx),
            started: None,
            splash_done_at: None,
            splash_gone: std::env::var("OXYDE_TAB").is_ok(),
            prev_tab: Tab::Home,
            tab_changed: 0.0,
        };
        app.tab = match std::env::var("OXYDE_TAB").as_deref() {
            Ok("store") => Tab::Store,
            Ok("modrinth") => {
                app.source = Source::Modrinth;
                app.search_modrinth(&cc.egui_ctx, 0);
                Tab::Store
            }
            Ok("packs") => Tab::Packs,
            Ok("mods") => Tab::Library,
            Ok("import") => Tab::Import,
            Ok("settings") => Tab::Settings,
            _ => Tab::Home,
        };
        if let Ok(f) = std::env::var("OXYDE_DEMO_TASK") {
            app.add_demo_task(&f);
        }
        app.refresh_library();
        app.load_registry(&cc.egui_ctx);
        app.refresh_launcher(&cc.egui_ctx);
        if app.s.auto_update {
            app.check_updates(cc.egui_ctx.clone());
        }
        app
    }

    fn add_demo_task(&mut self, title: &str) {
        self.tasks.lock().unwrap().push(Task {
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

    fn game_dir(&self) -> PathBuf {
        if let Some(d) = &self.s.game_dir {
            return d.clone();
        }
        self.instances
            .iter()
            .find(|i| i.kind == "default")
            .or_else(|| self.instances.iter().find(|i| i.selected))
            .map(|i| i.home.clone())
            .unwrap_or_else(game::default_game_dir)
    }

    fn ictx(&self) -> installer::Ctx {
        installer::Ctx {
            index: self.registry.clone(),
            launcher: self.launcher.clone(),
            game_dir: self.game_dir(),
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
            Msg::Launcher { instances: e.instances(), mods: e.installed(None), packs: e.packs() }
        });
    }

    fn search_modrinth(&mut self, ctx: &egui::Context, offset: u64) {
        self.mr_loading = true;
        self.mr_err = None;
        let q = self.query.clone();
        self.spawn(ctx, move || Msg::Modrinth { offset, result: modrinth::search(&q, offset) });
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
        for p in paths {
            if p.is_dir() {
                let mut jars: Vec<PathBuf> = std::fs::read_dir(&p).map(|rd| rd.flatten().map(|e| e.path()).filter(|x| is_ext(x, "jar")).collect()).unwrap_or_default();
                jars.sort();
                if jars.is_empty() {
                    self.toast = Some((format!("Aucun .jar dans {}", p.display()), false));
                }
                for j in jars {
                    self.install_target(ctx, file_name(&j), format!("j:{}", j.display()), Target::Jar(j));
                }
            } else if is_ext(&p, "jar") {
                self.install_target(ctx, file_name(&p), format!("j:{}", p.display()), Target::Jar(p));
            } else if is_ext(&p, "hmod") {
                self.install_target(ctx, file_name(&p), String::new(), Target::Hmod(p));
            } else if is_ext(&p, pack::PACK_EXT) || is_ext(&p, pack::LEGACY_EXT) || is_ext(&p, "mrpack") || is_ext(&p, "zip") {
                self.install_pack(ctx, p);
            } else {
                self.toast = Some((format!("Fichier non pris en charge : {}", file_name(&p)), false));
            }
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
                Msg::Modrinth { offset, result } => {
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
                    self.instances = instances;
                    self.launcher_mods = mods;
                    self.packs = packs;
                    self.refresh_library();
                }
                Msg::TaskDone => {
                    self.refresh_library();
                    self.refresh_launcher(ctx);
                }
                Msg::Launched(r) => {
                    self.launching = false;
                    self.toast = Some(match r {
                        Ok(()) => ("Horizon démarre…".into(), true),
                        Err(e) => (e, false),
                    });
                }
            }
        }
    }

    fn play(&mut self, ctx: &egui::Context, instance: Option<String>) {
        self.launcher = game::launcher_exe();
        match self.launcher.clone() {
            Some(exe) => {
                self.launching = true;
                self.spawn(ctx, move || Msg::Launched(engine::Engine { exe }.play(instance.as_deref())));
            }
            None => self.toast = Some(("Le launcher Horizon n'est pas installé : clique sur « Installer Horizon » dans l'accueil.".into(), false)),
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

    fn ui_titlebar(&mut self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        egui::TopBottomPanel::top("titlebar").exact_height(38.0).frame(egui::Frame::NONE.fill(Color32::from_rgb(13, 13, 16))).show(ctx, |ui| {
            let rect = ui.max_rect();
            let drag = ui.interact(rect, egui::Id::new("titlebar_drag"), egui::Sense::click_and_drag());
            if drag.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui.painter().text(rect.left_center() + egui::vec2(16.0, 0.0), egui::Align2::LEFT_CENTER, "OXYDE", egui::FontId::monospace(13.0), Color32::from_rgb(200, 190, 180));
            ui.painter().text(rect.left_center() + egui::vec2(74.0, 0.0), egui::Align2::LEFT_CENTER, "· Horizon", egui::FontId::monospace(12.0), MUTED);
            // boutons fenêtre
            let w = 46.0;
            for (i, what) in [2u8, 1, 0].iter().enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(rect.right() - w * (i as f32 + 1.0), rect.top()), egui::vec2(w, rect.height()));
                let resp = ui.interact(r, egui::Id::new(("winbtn", i)), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, if *what == 2 { Color32::from_rgb(196, 43, 28) } else { Color32::from_rgb(40, 40, 46) });
                }
                // icônes dessinées à la main (la police n'a pas ces caractères)
                let c = r.center();
                let st = egui::Stroke::new(1.2_f32, Color32::from_rgb(225, 225, 230));
                match what {
                    2 => {
                        ui.painter().line_segment([c + egui::vec2(-5.0, -5.0), c + egui::vec2(5.0, 5.0)], st);
                        ui.painter().line_segment([c + egui::vec2(-5.0, 5.0), c + egui::vec2(5.0, -5.0)], st);
                    }
                    1 => {
                        if maximized {
                            ui.painter().rect_stroke(egui::Rect::from_center_size(c + egui::vec2(-1.5, 1.5), egui::vec2(8.0, 8.0)), 1.0, st, egui::StrokeKind::Middle);
                            ui.painter().line(vec![c + egui::vec2(-2.0, -3.5), c + egui::vec2(3.5, -3.5), c + egui::vec2(3.5, 2.0)], st);
                        } else {
                            ui.painter().rect_stroke(egui::Rect::from_center_size(c, egui::vec2(10.0, 10.0)), 1.0, st, egui::StrokeKind::Middle);
                        }
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
        });
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

    fn ui_nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(210.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(Color32::from_rgba_premultiplied(20, 21, 24, 240)).inner_margin(egui::Margin::symmetric(14, 18)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../assets/icon.png")).fit_to_exact_size(egui::vec2(34.0, 34.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Oxyde").size(22.0).strong().color(RUST));
                        ui.label(RichText::new(format!("v{}", update::VERSION)).small().weak());
                    });
                });
                ui.add_space(22.0);
                let running = self.tasks.lock().unwrap().iter().filter(|t| t.state == TaskState::Running).count();
                for (t, label, ic) in [
                    (Tab::Home, "Accueil".to_string(), fx::Icon::Home),
                    (Tab::Store, "Magasin".to_string(), fx::Icon::Store),
                    (Tab::Packs, "Modpacks".to_string(), fx::Icon::Packs),
                    (Tab::Library, "Mes mods".to_string(), fx::Icon::Mods),
                    (Tab::Import, if running > 0 { format!("Importer  ·  {running}") } else { "Importer".to_string() }, fx::Icon::Import),
                    (Tab::Settings, "Réglages".to_string(), fx::Icon::Settings),
                ] {
                    let sel = self.tab == t;
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
                    let fill = if sel {
                        Color32::from_rgb(40, 32, 28)
                    } else if resp.hovered() {
                        Color32::from_rgb(32, 34, 39)
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);
                    if sel {
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 9.0), egui::vec2(3.0, 20.0)), 2.0, RUST);
                    }
                    let col = if sel { Color32::WHITE } else { MUTED };
                    fx::icon(ui.painter(), ic, rect.left_center() + egui::vec2(24.0, 0.0), if sel { RUST } else { col });
                    ui.painter().text(rect.left_center() + egui::vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(16.0), col);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.tab = t;
                    }
                    ui.add_space(2.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let label = if self.launching { "Lancement…" } else { "▶  Jouer" };
                    let b = egui::Button::new(RichText::new(label).size(18.0).strong().color(Color32::WHITE)).fill(RUST).corner_radius(10.0).min_size(egui::vec2(ui.available_width(), 46.0));
                    let resp = ui.add_enabled(!self.launching, b);
                    let t = ui.input(|i| i.time) as f32;
                    let pulse = 0.5 + 0.5 * (t * 2.2).sin();
                    for i in 1..=4 {
                        let a = ((0.22 - i as f32 * 0.045) * pulse * 255.0) as u8;
                        ui.painter().rect_stroke(
                            resp.rect.expand(i as f32 * 2.0),
                            10.0 + i as f32 * 2.0,
                            egui::Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(222, 110, 40, a)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if resp.clicked() {
                        self.play(ui.ctx(), None);
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("by full_money").small().monospace().color(Color32::from_rgb(95, 92, 90)));
                });
            });
    }

    fn ui_update_banner(&mut self, ui: &mut egui::Ui) {
        let state = std::mem::replace(&mut *self.upd.lock().unwrap(), Upd::Off);
        let mut next = None;
        match &state {
            Upd::Available(rel) => {
                surface(ui, 12, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Oxyde {} est disponible", rel.version)).strong().color(Color32::WHITE));
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
        let (hero, _) = ui.allocate_exact_size(egui::vec2(w, 300.0), egui::Sense::hover());
        let p = ui.painter_at(hero.expand(30.0));
        p.rect_filled(hero.translate(egui::vec2(0.0, 10.0)), 22.0, Color32::from_black_alpha(90));
        fx::rounded_gradient(&p, hero, 20.0, Color32::from_rgb(58, 30, 18), Color32::from_rgb(16, 15, 19));
        fx::soft_glow(&p, hero.right_center() - egui::vec2(w * 0.22, 0.0), 260.0, Color32::from_rgba_unmultiplied(230, 110, 40, 70));
        p.rect_stroke(hero, 20.0, egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 160, 100, 40)), egui::StrokeKind::Inside);
        let s = (hero.height() / 15.0).min(22.0);
        fx::diorama(&p, hero.right_center() - egui::vec2(w * 0.2, s * 0.6), s, t);

        let inner = hero.shrink2(egui::vec2(34.0, 30.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(inner.min, egui::vec2(inner.width() * 0.58, inner.height()))), |ui| {
            ui.label(RichText::new("HORIZON").size(13.0).strong().color(Color32::from_rgb(240, 160, 110)));
            ui.label(RichText::new("Minecraft, réécrit en Rust.").size(34.0).strong().color(Color32::WHITE));
            ui.label(RichText::new("Tes mods, tes modpacks et le jeu, au même endroit.").size(16.0).color(Color32::from_rgb(210, 200, 195)));
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                let label = if self.launching { "Lancement…" } else { "▶   JOUER" };
                let b = egui::Button::new(RichText::new(label).size(20.0).strong().color(Color32::WHITE))
                    .fill(RUST)
                    .corner_radius(12.0)
                    .min_size(egui::vec2(210.0, 56.0));
                let r = ui.add_enabled(!self.launching && launcher.is_some(), b);
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
                        ui.add(egui::Image::new(u).fit_to_exact_size(egui::vec2(72.0, 72.0)).corner_radius(14.0));
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(&p.name).size(16.0).strong().color(Color32::WHITE));
                    let sum: String = p.summary.chars().take(70).collect();
                    ui.label(RichText::new(sum).small().color(MUTED));
                    ui.label(RichText::new(format!("⬇ {}", fmt_num(p.downloads.unwrap_or(0)))).small().color(Color32::from_rgb(200, 140, 100)));
                    ui.add_space(4.0);
                    if installed {
                        ui.label(RichText::new("✔ Installé").color(GREEN));
                    } else if busy {
                        ui.spinner();
                    } else if ui.add(egui::Button::new(RichText::new("Installer").strong().color(Color32::WHITE)).fill(RUST).min_size(egui::vec2(ui.available_width(), 32.0))).clicked() {
                        install = Some((p.name.clone(), p.id.clone()));
                    }
                });
            }
        });
        if let Some((name, id)) = install {
            self.install_target(ui.ctx(), name, format!("h:{id}"), Target::Horizon(id));
        }
    }

    fn ui_store(&mut self, ui: &mut egui::Ui) {
        title(ui, "Magasin", "Mods vérifiés par l'équipe Horizon, et mods Java de Modrinth portés automatiquement.");
        ui.horizontal(|ui| {
            if segment(ui, self.source == Source::Horizon, "Horizon (officiel)") {
                self.source = Source::Horizon;
            }
            if segment(ui, self.source == Source::Modrinth, "Modrinth (mods Java)") {
                self.source = Source::Modrinth;
                if self.mr_hits.is_empty() && !self.mr_loading {
                    self.search_modrinth(ui.ctx(), 0);
                }
            }
        });
        let mut submit = false;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Rechercher un mod…").desired_width(380.0).margin(egui::vec2(12.0, 9.0)).background_color(Color32::from_rgb(31, 33, 38)));
            if r.changed() {
                self.shown = 40;
            }
            submit = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if self.source == Source::Horizon {
                egui::ComboBox::from_id_salt("kind").selected_text(if self.kind == "all" { "Tout" } else { registry::kind_label(&self.kind) }).show_ui(ui, |ui| {
                    for k in ["all", "mod", "native-mod", "resourcepack", "shader", "modpack"] {
                        let label = if k == "all" { "Tout" } else { registry::kind_label(k) };
                        if ui.selectable_value(&mut self.kind, k.to_string(), label).clicked() {
                            self.shown = 40;
                        }
                    }
                });
            } else if ui.button("Rechercher").clicked() {
                submit = true;
            }
        });
        if self.source == Source::Modrinth && submit {
            self.search_modrinth(ui.ctx(), 0);
        }
        ui.separator();
        match self.source {
            Source::Horizon => self.ui_store_horizon(ui),
            Source::Modrinth => self.ui_store_modrinth(ui),
        }
    }

    fn ui_store_horizon(&mut self, ui: &mut egui::Ui) {
        let Some(idx) = self.registry.clone() else {
            if let Some(e) = self.registry_err.clone() {
                ui.colored_label(RED, format!("Registre Horizon indisponible : {e}"));
                if ui.button("Réessayer").clicked() {
                    self.load_registry(ui.ctx());
                }
            } else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Chargement du registre Horizon…");
                });
            }
            return;
        };
        let q = self.query.to_lowercase();
        let mut list: Vec<&registry::Project> = idx
            .projects
            .iter()
            .filter(|p| self.kind == "all" || p.kind == self.kind)
            .filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.summary.to_lowercase().contains(&q) || p.id.contains(&q) || p.tags.iter().any(|t| t.contains(&q)))
            .collect();
        list.sort_by_key(|p| std::cmp::Reverse(p.downloads.unwrap_or(0)));
        ui.label(RichText::new(format!("{} résultat(s)", list.len())).color(MUTED));
        let mut to_install = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for p in list.iter().take(self.shown) {
                let installed = self.is_installed(&p.id);
                let busy = self.is_busy(&format!("h:{}", p.id));
                let v = p.versions.first();
                let meta = format!(
                    "par {} · ⬇ {} · {}{}{}{}",
                    if p.authors.is_empty() { "?".to_string() } else { p.authors.join(", ") },
                    fmt_num(p.downloads.unwrap_or(0)),
                    v.map(|v| format!("v{}", v.version)).unwrap_or_default(),
                    v.and_then(|v| v.game.as_ref()).map(|g| format!(" · MC {g}")).unwrap_or_default(),
                    if p.license.is_empty() { String::new() } else { format!(" · {}", p.license) },
                    p.updated.as_ref().map(|d| format!(" · {d}")).unwrap_or_default(),
                );
                if p.kind == "modpack" {
                    // les modpacks du registre s'installent comme profil par le launcher
                    if card(ui, p.icon.as_deref(), &p.name, &p.summary, &meta, "Modpack", false, busy, self.launcher.is_some()) {
                        to_install = Some(("mp", (*p).clone()));
                    }
                } else if card(ui, p.icon.as_deref(), &p.name, &p.summary, &meta, registry::kind_label(&p.kind), installed, busy, true) {
                    to_install = Some(("h", (*p).clone()));
                }
            }
            if list.len() > self.shown && ui.button("Afficher plus").clicked() {
                self.shown += 40;
            }
        });
        if let Some((what, p)) = to_install {
            if what == "mp" {
                let exe = self.launcher.clone().unwrap();
                let id = p.id.clone();
                self.task(ui.ctx(), format!("Modpack {}", p.name), format!("h:{}", p.id), move |log| {
                    let out = engine::Engine { exe }.install_horizon_pack(&id, log)?;
                    Ok(vec![installer::Outcome { method: Method::Native, title: id, message: out, details: vec!["Lance-le depuis l'onglet Modpacks.".into()] }])
                });
            } else {
                self.install_target(ui.ctx(), p.name.clone(), format!("h:{}", p.id), Target::Horizon(p.id.clone()));
            }
        }
    }

    fn ui_store_modrinth(&mut self, ui: &mut egui::Ui) {
        let engine_note = if self.launcher.is_some() {
            "Version Horizon officielle si elle existe, sinon portage officiel par Horizon, sinon conversion par Oxyde."
        } else {
            "Version Horizon officielle si elle existe, sinon conversion par Oxyde. Installe le launcher Horizon pour le portage officiel, plus complet."
        };
        ui.label(RichText::new(engine_note).small().color(MUTED));
        if let Some(e) = &self.mr_err {
            ui.colored_label(RED, e);
        }
        let mut to_install = None;
        let mut more = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for h in &self.mr_hits {
                let port = self.registry.as_ref().and_then(|r| r.find_port(&h.slug, &h.title));
                let installed = port.is_some_and(|p| self.is_installed(&p.id))
                    || self.launcher_mods.iter().any(|e| e.slug == h.slug)
                    || self.is_installed(&convert::sanitize_id(&h.slug));
                let busy = self.is_busy(&format!("m:{}", h.slug));
                let code_only = port.is_none() && h.categories.iter().any(|c| c == "library" || c == "optimization");
                let badge = if port.is_some() {
                    "Version Horizon complète"
                } else if code_only {
                    "Code Java uniquement"
                } else if self.launcher.is_some() {
                    "Porté par Horizon"
                } else {
                    "Converti (contenu)"
                };
                let meta = format!("par {} · ⬇ {}", h.author, fmt_num(h.downloads));
                if card(ui, h.icon_url.as_deref(), &h.title, &h.description, &meta, badge, installed, busy, !code_only) {
                    to_install = Some(h.clone());
                }
            }
            if self.mr_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Recherche sur Modrinth…");
                });
            } else if (self.mr_hits.len() as u64) < self.mr_total && ui.button("Afficher plus").clicked() {
                more = true;
            }
        });
        if let Some(h) = to_install {
            self.install_target(ui.ctx(), h.title.clone(), format!("m:{}", h.slug), Target::Modrinth { slug: h.slug.clone(), title: h.title.clone() });
        }
        if more {
            let off = self.mr_hits.len() as u64;
            self.search_modrinth(ui.ctx(), off);
        }
    }

    fn ui_packs(&mut self, ui: &mut egui::Ui) {
        title(ui, "Modpacks", "Un pote t'envoie un modpack ? Glisse-le ici : Oxyde télécharge, porte et installe tout.");
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
            ui.label(RichText::new("Prêts à jouer").size(18.0).strong().color(Color32::WHITE));
            for p in &self.packs {
                surface(ui, 14, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&p.name).size(17.0).strong());
                            ui.label(RichText::new(format!("{} · {} mod(s) portés sur {}", p.summary, p.ported_mods, p.portable)).small().color(MUTED));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(RichText::new("▶ Jouer").strong().color(Color32::WHITE)).fill(RUST).min_size(egui::vec2(110.0, 36.0))).clicked() {
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
        ui.label(RichText::new("Créer un modpack à partager").size(18.0).strong().color(Color32::WHITE));
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
            ui.add(egui::TextEdit::singleline(&mut self.pack_name).desired_width(220.0).margin(egui::vec2(10.0, 7.0)).background_color(Color32::from_rgb(31, 33, 38)));
            if ui.add_enabled(!self.pack_pick.is_empty(), egui::Button::new(RichText::new(format!("Créer ({} mods)", self.pack_pick.len())).color(Color32::WHITE)).fill(RUST)).clicked() {
                self.save_pack();
            }
        });
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

    fn ui_library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mes mods").size(28.0).strong().color(Color32::WHITE));
            ui.add_space(12.0);
            if ui.button("Actualiser").clicked() {
                self.refresh_library();
                self.refresh_launcher(ui.ctx());
            }
            if ui.button("Ouvrir le dossier").clicked() {
                open_folder(&self.game_dir().join("horizon-mods"));
            }
        });
        ui.label(RichText::new(format!("Dossier du jeu : {}", pretty_path(&self.game_dir()))).small().color(MUTED));
        ui.add_space(6.0);
        if self.library.is_empty() {
            ui.label("Aucun mod installé pour l'instant. Va dans le Magasin, ou glisse tes .jar dans la fenêtre.");
            return;
        }
        let mut toggle = None;
        let mut remove = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for m in &self.library {
                let name = self.display_name(m);
                let folder = m.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let entry = self.launcher_mods.iter().find(|e| e.file.ends_with(&folder));
                let origin = if m.native {
                    "natif (Rust)"
                } else if entry.is_some() {
                    "porté par Horizon"
                } else {
                    "contenu"
                };
                surface(ui, 12, |ui| {
                    ui.horizontal(|ui| {
                        if m.native {
                            ui.label(RichText::new("⚙").size(18.0));
                        } else {
                            let mut on = m.enabled;
                            if ui.checkbox(&mut on, "").on_hover_text("Activer / désactiver").changed() {
                                toggle = Some((m.id.clone(), on));
                            }
                        }
                        ui.label(RichText::new(name).strong());
                        ui.label(RichText::new(format!("{} · {}", m.version, origin)).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.confirm_remove.as_ref() == Some(&m.path) {
                                if ui.button(RichText::new("Confirmer").color(RED)).clicked() {
                                    remove = Some((m.clone(), entry.map(|e| e.slug.clone())));
                                }
                                if ui.button("Annuler").clicked() {
                                    self.confirm_remove = None;
                                }
                            } else if ui.button("Supprimer").clicked() {
                                self.confirm_remove = Some(m.path.clone());
                            }
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

    fn ui_import(&mut self, ui: &mut egui::Ui) {
        title(ui, "Importer", "Glisse tes mods Java : Oxyde les reconnaît et les installe, prêts à jouer.");
        drop_zone(ui, "Glisse tes .jar ici", "ou un dossier « mods », un .hmod ou un modpack, n'importe où dans la fenêtre");
        ui.horizontal(|ui| {
            if ui.button("Choisir des fichiers…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Mods et modpacks", &["jar", "hmod", pack::PACK_EXT, pack::LEGACY_EXT, "mrpack", "zip"]).pick_files() {
                    let ctx = ui.ctx().clone();
                    self.add_paths(&ctx, f);
                }
            }
            if ui.button("Choisir un dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    let ctx = ui.ctx().clone();
                    self.add_paths(&ctx, vec![d]);
                }
            }
        });
        ui.add_space(6.0);
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Ce qu'Oxyde fait pour chaque mod Java").strong().color(Color32::WHITE));
            for (n, t) in [
                ("1", "Version Horizon officielle : réécrite en Rust, complète, code compris."),
                ("2", "Portage officiel par Horizon (launcher) : blocs, objets, recettes, loot, textures en 26.3, avec rapport précis."),
                ("3", "Sinon, conversion du contenu par Oxyde (approximative)."),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(n).strong().color(RUST));
                    ui.label(RichText::new(t).color(Color32::from_rgb(200, 202, 210)));
                });
            }
            ui.label(RichText::new("Le code Java d'un mod (machines, menus spéciaux, IA de mobs) ne tourne pas dans Horizon : seul le pont Java prévu par Horizon le permettra.").small().color(MUTED));
        });
        ui.add_space(6.0);
        self.ui_task_list(ui, false);
    }

    fn ui_task_list(&mut self, ui: &mut egui::Ui, compact: bool) {
        let tasks = self.tasks.lock().unwrap().clone();
        if tasks.is_empty() {
            return;
        }
        if !compact {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Activité").size(18.0).strong().color(Color32::WHITE));
                if ui.small_button("Effacer les terminées").clicked() {
                    self.tasks.lock().unwrap().retain(|t| t.state == TaskState::Running);
                }
            });
        }
        egui::ScrollArea::vertical().id_salt(("tasks", compact)).auto_shrink([false, !compact]).max_height(if compact { 170.0 } else { f32::INFINITY }).show(ui, |ui| {
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
                        ui.label(RichText::new(&t.title).strong());
                        let line = match t.state {
                            TaskState::Running => t.lines.last().cloned().unwrap_or_default(),
                            _ => t.result.clone(),
                        };
                        let short: String = line.chars().take(110).collect();
                        ui.label(RichText::new(short).color(match t.state {
                            TaskState::Failed => RED,
                            TaskState::Done => Color32::from_rgb(200, 202, 210),
                            TaskState::Running => MUTED,
                        }));
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
        title(ui, "Réglages", "Dossiers, moteur, conversion et mises à jour.");
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Moteur").strong().color(Color32::WHITE));
            match &self.launcher {
                Some(p) => ui.label(RichText::new(format!("✔ Portage officiel Horizon : {}", pretty_path(p))).color(GREEN)),
                None => ui.label(RichText::new("✖ Launcher Horizon introuvable : Oxyde utilise son propre convertisseur.").color(ORANGE)),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label("Dossier du jeu :");
                ui.monospace(pretty_path(&self.game_dir()));
                if ui.small_button("Changer").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title("Dossier du jeu Horizon (celui qui contient options.txt)").pick_folder() {
                        if !pack::looks_like_game_dir(&d) {
                            self.toast = Some(("Ce dossier ne contient pas options.txt : vérifie que c'est le dossier du jeu Horizon.".into(), false));
                        }
                        self.s.game_dir = Some(d);
                        self.refresh_library();
                    }
                }
                if self.s.game_dir.is_some() && ui.small_button("Automatique").clicked() {
                    self.s.game_dir = None;
                    self.refresh_library();
                }
            });
        });
        surface(ui, 14, |ui| {
            ui.label(RichText::new("Convertisseur Oxyde (dernier recours)").strong().color(Color32::WHITE));
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
            ui.label(RichText::new("Mises à jour").strong().color(Color32::WHITE));
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
            ui.label(RichText::new("Créé par full_money").strong().color(Color32::WHITE));
            ui.label("Oxyde est gratuit et open source (licence MIT, © 2026 full_money). Projet non officiel, sans lien avec Mojang, Microsoft ou l'équipe Horizon.");
            ui.hyperlink_to("Code source et mises à jour", "https://github.com/0full-money0/oxyde");
            ui.hyperlink_to("Horizon", "https://playhorizon.dev/");
        });
    }
}

fn drop_zone(ui: &mut egui::Ui, big: &str, small: &str) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    egui::Frame::NONE
        .stroke(egui::Stroke::new(if hovering { 2.5_f32 } else { 1.5_f32 }, if hovering { RUST } else { Color32::from_gray(70) }))
        .corner_radius(12.0)
        .fill(if hovering { Color32::from_rgb(45, 30, 22) } else { Color32::from_rgba_premultiplied(22, 23, 27, 235) })
        .inner_margin(24.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("⬇").size(28.0).color(RUST));
                ui.label(RichText::new(big).size(19.0).strong());
                ui.label(RichText::new(small).color(MUTED));
            });
        });
    ui.add_space(6.0);
}

fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    for f in style.text_styles.values_mut() {
        f.size *= 1.08;
    }
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(12, 13, 15);
    v.selection.bg_fill = RUST;
    v.hyperlink_color = Color32::from_rgb(240, 150, 90);
    let r = egui::CornerRadius::same(8);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 42, 48);
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 42, 48);
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 55, 62);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(70, 74, 84));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(220, 222, 228));
    style.visuals = v;
    style.spacing.interact_size.y = 30.0;
    ctx.set_style(style);
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
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        self_screenshot(ctx, elapsed);
        self.fx.background(ctx, 1.0);
        if self.tab != self.prev_tab {
            self.prev_tab = self.tab;
            self.tab_changed = now;
        }

        self.ui_titlebar(ctx);
        self.ui_nav(ctx);

        if let Some((msg, ok)) = self.toast.clone() {
            egui::TopBottomPanel::bottom("toast").frame(egui::Frame::NONE.fill(PANEL).inner_margin(10.0)).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(if ok { GREEN } else { RED }, msg);
                    if ui.small_button("OK").clicked() {
                        self.toast = None;
                    }
                });
            });
        }
        // activité en bas, sauf dans l'onglet Importer qui l'affiche en grand
        let has_tasks = !self.tasks.lock().unwrap().is_empty();
        if has_tasks && self.tab != Tab::Import {
            egui::TopBottomPanel::bottom("activity").frame(egui::Frame::NONE.fill(Color32::from_rgba_premultiplied(18, 19, 22, 245)).inner_margin(egui::Margin::symmetric(16, 8))).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let running = self.tasks.lock().unwrap().iter().filter(|t| t.state == TaskState::Running).count();
                    let label = if running > 0 { format!("Activité · {running} en cours") } else { "Activité".to_string() };
                    if ui.add(egui::Button::new(RichText::new(label).strong()).frame(false)).clicked() {
                        self.tasks_open = !self.tasks_open;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(if self.tasks_open { "Réduire" } else { "Voir" }).clicked() {
                            self.tasks_open = !self.tasks_open;
                        }
                        if ui.small_button("Tout voir").clicked() {
                            self.tab = Tab::Import;
                        }
                    });
                });
                if self.tasks_open {
                    self.ui_task_list(ui, true);
                }
            });
        }

        let enter = (((now - self.tab_changed) as f32) / 0.22).clamp(0.0, 1.0);
        let enter = 1.0 - (1.0 - enter).powi(3);
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(28, 22))).show(ctx, |ui| {
            ui.multiply_opacity(enter);
            ui.add_space((1.0 - enter) * 14.0);
            self.ui_update_banner(ui);
            match self.tab {
                Tab::Home => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_home(ui));
                }
                Tab::Store => self.ui_store(ui),
                Tab::Packs => {
                    egui::ScrollArea::vertical().id_salt("packs").show(ui, |ui| self.ui_packs(ui));
                }
                Tab::Library => self.ui_library(ui),
                Tab::Import => self.ui_import(ui),
                Tab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_settings(ui));
                }
            }
        });
        self.resize_edges(ctx);

        if !self.splash_gone {
            let ready = self.registry.is_some() || self.registry_err.is_some();
            let target = 0.25 + if ready { 0.45 } else { 0.0 } + if elapsed > 1.3 { 0.30 } else { 0.0 };
            let progress = ctx.animate_value_with_time(egui::Id::new("splash_p"), target, 0.45);
            let status = if elapsed < 0.4 {
                "démarrage"
            } else if !ready {
                "lecture du registre horizon"
            } else if elapsed < 1.3 {
                "recherche du jeu"
            } else {
                "prêt"
            };
            if self.splash_done_at.is_none() && ((ready && elapsed > 1.7) || elapsed > 4.0) {
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
