#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod fx;
mod game;
mod modrinth;
mod net;
mod pack;
mod registry;
mod update;

use convert::{Options, Report};
use eframe::egui::{self, Color32, RichText};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const GREEN: Color32 = Color32::from_rgb(80, 200, 120);
const RED: Color32 = Color32::from_rgb(235, 90, 90);
const ORANGE: Color32 = Color32::from_rgb(240, 170, 60);
const RUST: Color32 = Color32::from_rgb(222, 110, 40);
const BG: Color32 = Color32::from_rgb(17, 18, 21);
const PANEL: Color32 = Color32::from_rgb(24, 25, 29);
const CARD: Color32 = Color32::from_rgba_premultiplied(27, 28, 33, 232);
const BORDER: Color32 = Color32::from_rgb(44, 46, 53);
const MUTED: Color32 = Color32::from_rgb(150, 154, 165);

fn main() -> eframe::Result<()> {
    // Ligne de commande : oxyde.exe --convert <dossier-sortie> <mod.jar>...
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--convert" {
        let out = PathBuf::from(&args[2]);
        let mut failed = false;
        for jar in &args[3..] {
            match convert::convert_jar(Path::new(jar), &out, &Options::default()) {
                Ok(r) => println!("OK  {} -> {} ({} fichiers)", jar, r.output.display(), r.kept),
                Err(e) => {
                    failed = true;
                    println!("ERR {jar}: {e}")
                }
            }
        }
        std::process::exit(if failed { 1 } else { 0 });
    }
    // oxyde.exe --install <horizon|modrinth> <id ou slug> <dossier-du-jeu>
    if args.len() == 5 && args[1] == "--install" {
        let dir = PathBuf::from(&args[4]);
        let r = cli_install(&args[2], &args[3], &dir);
        match &r {
            Ok(m) => println!("OK  {m}"),
            Err(e) => println!("ERR {e}"),
        }
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oxyde")
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([760.0, 520.0])
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native("Oxyde", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

fn cli_install(source: &str, id: &str, dir: &Path) -> Result<String, String> {
    let idx = registry::fetch()?;
    match source {
        "horizon" => {
            let p = idx.projects.iter().find(|p| p.id == id).ok_or("projet introuvable dans le registre")?;
            registry::install(&idx, p, dir)
        }
        "modrinth" => {
            if let Some(p) = idx.find_port(id, "") {
                return registry::install(&idx, p, dir).map(|m| format!("version Horizon officielle : {m}"));
            }
            let jar = modrinth::download_jar(id)?;
            let out = dir.join("oxyde-converted");
            let rep = convert::convert_jar(&jar, &out, &Options::default())?;
            pack::install_mods(std::slice::from_ref(&rep.output), dir)?;
            Ok(format!("{} converti ({} fichiers, {} blocs, {} objets)", rep.meta.name, rep.kept, rep.blocks, rep.items))
        }
        _ => Err("source : horizon ou modrinth".into()),
    }
}

// ---------------------------------------------------------------------------
// État
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
struct Settings {
    output_dir: PathBuf,
    game_dir: PathBuf,
    generate_registries: bool,
    auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
        Self {
            output_dir: base.join("Oxyde"),
            game_dir: game::default_game_dir(),
            generate_registries: true,
            auto_update: true,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Store,
    Library,
    Convert,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum Source {
    Horizon,
    Modrinth,
}

enum Status {
    Pending,
    Working,
    Done(Box<Report>),
    Failed(String),
}

struct Job {
    path: PathBuf,
    status: Status,
}

enum Upd {
    Off,
    Checking,
    UpToDate,
    Available(update::Release),
    Installing,
    Error(String),
}

enum Msg {
    Registry(Result<registry::Index, String>),
    Modrinth { offset: u64, result: Result<modrinth::SearchResp, String> },
    Installed { key: String, result: Result<String, String> },
    Converted(usize, Box<Result<Report, String>>),
    ConvertDone,
    Launched(Result<(), String>),
}

struct App {
    s: Settings,
    tab: Tab,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    // registre Horizon
    registry: Option<Arc<registry::Index>>,
    registry_err: Option<String>,
    // magasin
    source: Source,
    query: String,
    kind: String,
    shown: usize,
    mr_hits: Vec<modrinth::Hit>,
    mr_total: u64,
    mr_loading: bool,
    mr_err: Option<String>,
    mr_query_sent: Option<String>,
    installing: HashSet<String>,
    // bibliothèque
    library: Vec<game::InstalledMod>,
    confirm_remove: Option<PathBuf>,
    // conversion
    jobs: Vec<Job>,
    converting: bool,
    pack_name: String,
    pending_pack: Option<(PathBuf, pack::PackInfo)>,
    // divers
    launching: bool,
    upd: Arc<Mutex<Upd>>,
    toast: Option<(String, bool)>,
    // animations
    fx: fx::Fx,
    logo: egui::TextureHandle,
    started: Option<f64>,
    splash_done_at: Option<f64>,
    splash_gone: bool,
    prev_tab: Tab,
    tab_changed: f64,
    launcher: Option<PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let s: Settings = cc.storage.and_then(|st| eframe::get_value(st, eframe::APP_KEY)).unwrap_or_default();
        let mut style = (*cc.egui_ctx.style()).clone();
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
        cc.egui_ctx.set_style(style);

        let (tx, rx) = channel();
        let mut app = Self {
            s,
            tab: Tab::Home,
            tx,
            rx,
            registry: None,
            registry_err: None,
            source: Source::Horizon,
            query: String::new(),
            kind: "all".into(),
            shown: 40,
            mr_hits: vec![],
            mr_total: 0,
            mr_loading: false,
            mr_err: None,
            mr_query_sent: None,
            installing: HashSet::new(),
            library: vec![],
            confirm_remove: None,
            jobs: vec![],
            converting: false,
            pack_name: "Mon modpack".into(),
            pending_pack: None,
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
            launcher: game::launcher_exe(),
        };
        // Onglet de départ (OXYDE_TAB=store|mods|convert|settings), pratique pour les captures d'écran.
        app.tab = match std::env::var("OXYDE_TAB").as_deref() {
            Ok("store") => Tab::Store,
            Ok("modrinth") => {
                app.source = Source::Modrinth;
                app.search_modrinth(&cc.egui_ctx, 0);
                Tab::Store
            }
            Ok("mods") => Tab::Library,
            Ok("convert") => Tab::Convert,
            Ok("settings") => Tab::Settings,
            _ => Tab::Home,
        };
        app.refresh_library();
        app.load_registry(&cc.egui_ctx);
        if app.s.auto_update {
            app.check_updates(cc.egui_ctx.clone());
        }
        app
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

    fn search_modrinth(&mut self, ctx: &egui::Context, offset: u64) {
        self.mr_loading = true;
        self.mr_err = None;
        let q = self.query.clone();
        self.mr_query_sent = Some(q.clone());
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
        self.library = game::scan(&self.s.game_dir);
    }

    fn is_installed(&self, id: &str) -> bool {
        self.library.iter().any(|m| m.id == id)
    }

    fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Registry(Ok(i)) => self.registry = Some(Arc::new(i)),
                Msg::Registry(Err(e)) => {
                    // Une copie locale suffit pour continuer hors ligne.
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
                Msg::Installed { key, result } => {
                    self.installing.remove(&key);
                    self.refresh_library();
                    self.toast = Some(match result {
                        Ok(m) => (m, true),
                        Err(e) => (e, false),
                    });
                }
                Msg::Converted(i, r) => {
                    if let Some(j) = self.jobs.get_mut(i) {
                        j.status = match *r {
                            Ok(rep) => Status::Done(Box::new(rep)),
                            Err(e) => Status::Failed(e),
                        };
                    }
                }
                Msg::ConvertDone => {
                    self.converting = false;
                    let ok = self.converted().len();
                    let ko = self.jobs.iter().filter(|j| matches!(j.status, Status::Failed(_))).count();
                    self.toast = Some((format!("Conversion terminée : {ok} mod(s) prêt(s), {ko} impossible(s)."), ko == 0));
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

    // ------------------------------------------------------------ actions

    fn play(&mut self, ctx: &egui::Context) {
        self.launcher = game::launcher_exe();
        match self.launcher.clone() {
            Some(exe) => {
                self.launching = true;
                self.spawn(ctx, move || Msg::Launched(game::play(&exe)));
            }
            None => {
                self.toast = Some(("Le launcher Horizon n'est pas installé : clique sur « Installer Horizon ».".into(), false));
            }
        }
    }

    fn install_registry(&mut self, ctx: &egui::Context, p: registry::Project) {
        let Some(idx) = self.registry.clone() else { return };
        let key = format!("h:{}", p.id);
        if !self.installing.insert(key.clone()) {
            return;
        }
        let dir = self.s.game_dir.clone();
        self.spawn(ctx, move || Msg::Installed { key, result: registry::install(&idx, &p, &dir) });
    }

    /// Modrinth : version Horizon officielle si elle existe, sinon téléchargement + conversion + installation.
    fn install_modrinth(&mut self, ctx: &egui::Context, h: modrinth::Hit) {
        let key = format!("m:{}", h.project_id);
        if !self.installing.insert(key.clone()) {
            return;
        }
        let idx = self.registry.clone();
        let dir = self.s.game_dir.clone();
        let out = self.s.output_dir.clone();
        let opts = Options { generate_registries: self.s.generate_registries };
        self.spawn(ctx, move || {
            let result = (|| {
                if let Some(idx) = &idx {
                    if let Some(p) = idx.find_port(&h.slug, &h.title) {
                        let msg = registry::install(idx, p, &dir)?;
                        return Ok(format!("Version Horizon officielle (complète) : {msg}"));
                    }
                }
                let jar = modrinth::download_jar(&h.project_id)?;
                let rep = convert::convert_jar(&jar, &out, &opts)?;
                pack::install_mods(std::slice::from_ref(&rep.output), &dir)?;
                let mut msg = format!("{} converti et installé ({} fichiers, {} bloc(s), {} objet(s))", rep.meta.name, rep.kept, rep.blocks, rep.items);
                if rep.has_code {
                    msg.push_str(". Contenu seulement : le code Java du mod ne tourne pas dans Horizon.");
                }
                Ok(msg)
            })();
            Msg::Installed { key, result }
        });
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            if p.is_dir() {
                let mut jars: Vec<PathBuf> = std::fs::read_dir(&p)
                    .map(|rd| rd.flatten().map(|e| e.path()).filter(|x| is_ext(x, "jar")).collect())
                    .unwrap_or_default();
                jars.sort();
                if jars.is_empty() {
                    self.toast = Some((format!("Aucun .jar dans {}", p.display()), false));
                }
                for j in jars {
                    self.add_jar(j);
                }
            } else if is_ext(&p, "jar") {
                self.add_jar(p);
            } else if is_ext(&p, pack::PACK_EXT) {
                match pack::read_pack(&p) {
                    Ok(info) => self.pending_pack = Some((p, info)),
                    Err(e) => self.toast = Some((e, false)),
                }
            } else if is_ext(&p, "hmod") {
                self.toast = Some(match pack::install_mods(&[p], &self.s.game_dir) {
                    Ok(ids) => (format!("{} installé et activé.", ids.join(", ")), true),
                    Err(e) => (e, false),
                });
                self.refresh_library();
            } else {
                self.toast = Some((format!("Fichier non pris en charge : {}", file_name(&p)), false));
            }
        }
        self.tab = Tab::Convert;
    }

    fn add_jar(&mut self, p: PathBuf) {
        if !self.jobs.iter().any(|j| j.path == p) {
            self.jobs.push(Job { path: p, status: Status::Pending });
        }
    }

    fn start_convert(&mut self, ctx: &egui::Context) {
        let todo: Vec<(usize, PathBuf)> = self
            .jobs
            .iter_mut()
            .enumerate()
            .filter(|(_, j)| matches!(j.status, Status::Pending | Status::Failed(_)))
            .map(|(i, j)| {
                j.status = Status::Working;
                (i, j.path.clone())
            })
            .collect();
        if todo.is_empty() {
            return;
        }
        self.converting = true;
        let tx = self.tx.clone();
        let out = self.s.output_dir.clone();
        let opts = Options { generate_registries: self.s.generate_registries };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (i, p) in todo {
                let _ = tx.send(Msg::Converted(i, Box::new(convert::convert_jar(&p, &out, &opts))));
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::ConvertDone);
            ctx.request_repaint();
        });
    }

    fn converted(&self) -> Vec<PathBuf> {
        self.jobs
            .iter()
            .filter_map(|j| match &j.status {
                Status::Done(r) => Some(r.output.clone()),
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------ UI

    fn ui_nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav").exact_width(210.0).resizable(false).frame(egui::Frame::NONE.fill(Color32::from_rgba_premultiplied(20, 21, 24, 240)).inner_margin(egui::Margin::symmetric(14, 18))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../assets/icon.png")).fit_to_exact_size(egui::vec2(34.0, 34.0)));
                ui.vertical(|ui| {
                    ui.label(RichText::new("Oxyde").size(22.0).strong().color(RUST));
                    ui.label(RichText::new(format!("v{}", update::VERSION)).small().weak());
                });
            });
            ui.add_space(22.0);
            for (t, label) in [
                (Tab::Home, "Accueil"),
                (Tab::Store, "Magasin"),
                (Tab::Library, "Mes mods"),
                (Tab::Convert, "Convertir"),
                (Tab::Settings, "Réglages"),
            ] {
                let sel = self.tab == t;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
                let fill = if sel {
                    Color32::from_rgb(40, 32, 28)
                } else if resp.hovered() {
                    Color32::from_rgb(32, 34, 39)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 8.0, fill);
                if sel {
                    let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 9.0), egui::vec2(3.0, 20.0));
                    ui.painter().rect_filled(bar, 2.0, RUST);
                }
                ui.painter().text(
                    rect.left_center() + egui::vec2(16.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(16.0),
                    if sel { Color32::WHITE } else { MUTED },
                );
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.tab = t;
                }
                ui.add_space(2.0);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let label = if self.launching { "Lancement…" } else { "▶  Jouer" };
                let b = egui::Button::new(RichText::new(label).size(18.0).strong().color(Color32::WHITE))
                    .fill(RUST)
                    .corner_radius(10.0)
                    .min_size(egui::vec2(ui.available_width(), 46.0));
                let resp = ui.add_enabled(!self.launching, b);
                // halo qui respire autour de Jouer
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
                    self.play(ui.ctx());
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
                egui::Frame::group(ui.style()).fill(Color32::from_rgb(70, 40, 20)).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Nouvelle version {} d'Oxyde disponible", rel.version)).strong());
                        if ui.button("⬇ Mettre à jour").clicked() {
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
        title(ui, "Bienvenue sur Oxyde", "Mods, modpacks et lancement pour Horizon, Minecraft réécrit en Rust.");

        let launcher = self.launcher.clone();
        surface(ui, 20, |ui| {
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new("▶  Jouer à Horizon").size(20.0).strong().color(Color32::WHITE))
                    .fill(RUST)
                    .corner_radius(10.0)
                    .min_size(egui::vec2(260.0, 54.0));
                if ui.add_enabled(!self.launching && launcher.is_some(), b).clicked() {
                    self.play(ui.ctx());
                }
                ui.vertical(|ui| match &launcher {
                    Some(exe) => {
                        ui.label(RichText::new("✔ Launcher Horizon trouvé").color(GREEN));
                        if ui.small_button("Ouvrir le launcher Horizon").clicked() {
                            if let Err(e) = game::open_launcher(exe) {
                                self.toast = Some((e, false));
                            }
                        }
                    }
                    None => {
                        ui.label(RichText::new("✖ Horizon n'est pas installé sur ce PC").color(ORANGE));
                        ui.hyperlink_to("⬇ Installer Horizon (gratuit)", game::INSTALL_PAGE);
                    }
                });
            });
            ui.add_space(8.0);
            ui.label(RichText::new("La première fois, ouvre le launcher Horizon pour te connecter avec ton compte Microsoft (il faut posséder Minecraft Java).").small().color(MUTED));
        });

        ui.add_space(12.0);
        ui.columns(3, |c| {
            stat(&mut c[0], &self.library.len().to_string(), "mods installés");
            let count = |k: &str| self.registry.as_ref().map(|r| r.projects.iter().filter(|p| p.kind == k).count().to_string()).unwrap_or_else(|| "…".into());
            stat(&mut c[1], &count("native-mod"), "mods natifs Horizon (complets)");
            let n = self.registry.as_ref().map(|r| r.projects.len().to_string()).unwrap_or_else(|| "…".into());
            stat(&mut c[2], &n, "projets dans le magasin");
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Parcourir le magasin").size(15.0)).min_size(egui::vec2(0.0, 36.0))).clicked() {
                self.tab = Tab::Store;
            }
            if ui.add(egui::Button::new(RichText::new("Convertir mes .jar").size(15.0)).min_size(egui::vec2(0.0, 36.0))).clicked() {
                self.tab = Tab::Convert;
            }
            if ui.add(egui::Button::new(RichText::new("Ouvrir le dossier du jeu").size(15.0)).min_size(egui::vec2(0.0, 36.0))).clicked() {
                open_folder(&self.s.game_dir);
            }
        });
    }

    fn ui_store(&mut self, ui: &mut egui::Ui) {
        title(ui, "Magasin", "Mods vérifiés par l'équipe Horizon, et mods Java de Modrinth.");
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
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Rechercher un mod…")
                    .desired_width(380.0)
                    .margin(egui::vec2(12.0, 9.0))
                    .background_color(Color32::from_rgb(31, 33, 38)),
            );
            if r.changed() {
                self.shown = 40;
            }
            submit = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if self.source == Source::Horizon {
                egui::ComboBox::from_id_salt("kind")
                    .selected_text(if self.kind == "all" { "Tout" } else { registry::kind_label(&self.kind) })
                    .show_ui(ui, |ui| {
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
            .filter(|p| {
                q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.summary.to_lowercase().contains(&q)
                    || p.id.contains(&q)
                    || p.tags.iter().any(|t| t.contains(&q))
            })
            .collect();
        list.sort_by_key(|p| std::cmp::Reverse(p.downloads.unwrap_or(0)));
        ui.label(RichText::new(format!("{} résultat(s)", list.len())).color(MUTED));
        let mut to_install = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for p in list.iter().take(self.shown) {
                let installed = self.is_installed(&p.id);
                let busy = self.installing.contains(&format!("h:{}", p.id));
                let v = p.versions.first();
                let meta = format!(
                    "par {} · ⬇ {} · {}{}{}{}",
                    if p.authors.is_empty() { "?".to_string() } else { p.authors.join(", ") },
                    fmt_num(p.downloads.unwrap_or(0)),
                    v.map(|v| format!("v{}", v.version)).unwrap_or_default(),
                    v.and_then(|v| v.game.as_ref()).map(|g| format!(" · MC {g}")).unwrap_or_default(),
                    if p.license.is_empty() { String::new() } else { format!(" · {}", p.license) },
                    p.updated.as_ref().map(|d| format!(" · {d}")).unwrap_or_default()
                );
                let action = card(ui, p.icon.as_deref(), &p.name, &p.summary, &meta, registry::kind_label(&p.kind), installed, busy, p.kind != "modpack");
                if action {
                    to_install = Some((*p).clone());
                }
            }
            if list.len() > self.shown && ui.button("Afficher plus").clicked() {
                self.shown += 40;
            }
        });
        if let Some(p) = to_install {
            self.install_registry(ui.ctx(), p);
        }
    }

    fn ui_store_modrinth(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Mods Java de Modrinth. Oxyde installe la version Horizon officielle quand elle existe ; sinon il convertit le contenu du mod (textures, blocs, recettes…), sans son code Java.")
                .small()
                .weak(),
        );
        if let Some(e) = &self.mr_err {
            ui.colored_label(RED, e);
        }
        let mut to_install = None;
        let mut more = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for h in &self.mr_hits {
                let port = self.registry.as_ref().and_then(|r| r.find_port(&h.slug, &h.title));
                let installed = port.is_some_and(|p| self.is_installed(&p.id)) || self.is_installed(&convert::sanitize_id(&h.slug));
                let busy = self.installing.contains(&format!("m:{}", h.project_id));
                let code_only = port.is_none() && h.categories.iter().any(|c| c == "library" || c == "optimization");
                let badge = if port.is_some() {
                    "Version Horizon complète"
                } else if code_only {
                    "Code Java uniquement"
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
            self.install_modrinth(ui.ctx(), h);
        }
        if more {
            let off = self.mr_hits.len() as u64;
            self.search_modrinth(ui.ctx(), off);
        }
    }

    fn ui_library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mes mods").size(28.0).strong().color(Color32::WHITE));
            ui.add_space(12.0);
            if ui.button("Actualiser").clicked() {
                self.refresh_library();
            }
            if ui.button("Ouvrir le dossier").clicked() {
                open_folder(&self.s.game_dir.join("horizon-mods"));
            }
        });
        ui.label(RichText::new(format!("Dossier du jeu : {}", self.s.game_dir.display())).small().weak());
        ui.separator();
        if self.library.is_empty() {
            ui.label("Aucun mod installé pour l'instant. Va dans le Magasin ou convertis tes .jar.");
        }
        let mut toggle = None;
        let mut remove = None;
        egui::ScrollArea::vertical().max_height(ui.available_height() - 170.0).auto_shrink([false, true]).show(ui, |ui| {
            for m in &self.library {
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
                        let name = self.registry.as_ref().and_then(|r| r.projects.iter().find(|p| p.id == m.id)).map(|p| p.name.clone()).unwrap_or_else(|| m.name.clone());
                        ui.label(RichText::new(name).strong());
                        ui.label(RichText::new(format!("{} · {}", m.version, if m.native { "natif (Rust)" } else { "contenu" })).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.confirm_remove.as_ref() == Some(&m.path) {
                                if ui.button(RichText::new("Confirmer").color(RED)).clicked() {
                                    remove = Some(m.clone());
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
        if let Some((id, on)) = toggle {
            if let Err(e) = game::set_enabled(&self.s.game_dir, &id, on) {
                self.toast = Some((e, false));
            }
            self.refresh_library();
        }
        if let Some(m) = remove {
            self.confirm_remove = None;
            self.toast = Some(match game::remove(&self.s.game_dir, &m) {
                Ok(()) => (format!("{} supprimé.", m.name), true),
                Err(e) => (e, false),
            });
            self.refresh_library();
        }

        ui.separator();
        ui.label(RichText::new("Modpack à partager").size(18.0).strong());
        let content: Vec<PathBuf> = self.library.iter().filter(|m| !m.native && m.enabled).map(|m| m.path.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.pack_name).desired_width(200.0));
            let b = ui.add_enabled(!content.is_empty(), egui::Button::new(format!("Créer le modpack ({} mods activés)", content.len())));
            if b.clicked() {
                self.save_pack(&content);
            }
            if ui.button("Installer un modpack…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Modpack Oxyde", &[pack::PACK_EXT]).pick_file() {
                    self.add_paths(vec![f]);
                    self.tab = Tab::Convert;
                }
            }
        });
        ui.label(RichText::new("Les mods natifs viennent du registre Horizon : tes amis les installent depuis le Magasin.").small().weak());
    }

    fn save_pack(&mut self, mods: &[PathBuf]) {
        let file = format!("{}.{}", sanitize_file(&self.pack_name), pack::PACK_EXT);
        if let Some(out) = rfd::FileDialog::new().set_file_name(&file).add_filter("Modpack Oxyde", &[pack::PACK_EXT]).save_file() {
            self.toast = Some(match pack::create_pack(&self.pack_name, "", mods, &out) {
                Ok(p) => (format!("Modpack créé : {}. Envoie-le à tes amis : ils le glissent dans Oxyde.", file_name(&p)), true),
                Err(e) => (e, false),
            });
        }
    }

    fn ui_convert(&mut self, ui: &mut egui::Ui) {
        title(ui, "Convertir des mods Java", "Oxyde installe la version Horizon officielle si elle existe, sinon il convertit le contenu du mod.");
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        egui::Frame::NONE
            .stroke(egui::Stroke::new(if hovering { 2.5_f32 } else { 1.5_f32 }, if hovering { RUST } else { Color32::from_gray(80) }))
            .corner_radius(12.0)
            .fill(if hovering { Color32::from_rgb(45, 30, 22) } else { PANEL })
            .inner_margin(26.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("⬇").size(30.0).color(RUST));
                    ui.label(RichText::new("Glisse tes mods Java (.jar) ici").size(20.0).strong());
                    ui.label(RichText::new("ou un dossier « mods », un modpack .hzmods ou un mod Horizon .hmod").color(MUTED));
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 340.0).max(0.0) / 2.0);
                        if ui.button("Choisir des fichiers").clicked() {
                            if let Some(f) = rfd::FileDialog::new().add_filter("Mods et modpacks", &["jar", pack::PACK_EXT, "hmod"]).pick_files() {
                                self.add_paths(f);
                            }
                        }
                        if ui.button("Choisir un dossier").clicked() {
                            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.add_paths(vec![d]);
                            }
                        }
                    });
                });
            });

        // Modpack déposé
        if let Some((path, info)) = self.pending_pack.clone() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("Modpack « {} » ({} mods)", info.name, info.mods.len())).size(17.0).strong());
                for m in &info.mods {
                    ui.label(format!("  • {} {}", if m.name.is_empty() { &m.id } else { &m.name }, m.version));
                }
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("✔ Installer").strong()).clicked() {
                        self.toast = Some(match pack::install_pack(&path, &self.s.game_dir) {
                            Ok(ids) => (format!("{} mod(s) installé(s) et activé(s).", ids.len()), true),
                            Err(e) => (e, false),
                        });
                        self.pending_pack = None;
                        self.refresh_library();
                    }
                    if ui.button("Annuler").clicked() {
                        self.pending_pack = None;
                    }
                });
            });
        }

        if self.jobs.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Oxyde cherche d'abord la version Horizon officielle du mod (complète, code compris). Sinon il convertit son contenu : textures, modèles, sons, recettes, tags, loot, structures, blocs et objets. Le code Java (machines, menus spéciaux, IA des mobs) ne peut pas tourner dans Horizon.").weak());
            return;
        }

        let pending = self.jobs.iter().filter(|j| matches!(j.status, Status::Pending | Status::Failed(_))).count();
        let done = self.converted();
        ui.horizontal_wrapped(|ui| {
            let label = if self.converting { "Conversion…".to_string() } else { format!("▶ Convertir ({pending})") };
            if ui.add_enabled(pending > 0 && !self.converting, egui::Button::new(RichText::new(label).size(16.0).strong()).fill(RUST)).clicked() {
                self.start_convert(ui.ctx());
            }
            if ui.add_enabled(!done.is_empty() && !self.converting, egui::Button::new("Installer dans Horizon")).clicked() {
                self.toast = Some(match pack::install_mods(&done, &self.s.game_dir) {
                    Ok(ids) => (format!("{} mod(s) installé(s) et activé(s).", ids.len()), true),
                    Err(e) => (e, false),
                });
                self.refresh_library();
            }
            if ui.add_enabled(!done.is_empty() && !self.converting, egui::Button::new("Créer un modpack")).clicked() {
                self.save_pack(&done);
            }
            if ui.add_enabled(!done.is_empty(), egui::Button::new("Ouvrir le dossier")).clicked() {
                open_folder(&self.s.output_dir);
            }
            if ui.add_enabled(!self.converting, egui::Button::new("Vider")).clicked() {
                self.jobs.clear();
            }
        });

        let mut remove = None;
        let mut install_port = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, job) in self.jobs.iter().enumerate() {
                surface(ui, 12, |ui| {
                    ui.horizontal(|ui| {
                        match &job.status {
                            Status::Pending => ui.label(RichText::new("●").color(Color32::GRAY)),
                            Status::Working => ui.spinner(),
                            Status::Done(_) => ui.label(RichText::new("✔").color(GREEN)),
                            Status::Failed(_) => ui.label(RichText::new("✖").color(RED)),
                        };
                        ui.label(RichText::new(file_name(&job.path)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !matches!(job.status, Status::Working) && ui.small_button("✕").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    match &job.status {
                        Status::Pending => {
                            ui.label(RichText::new("En attente").weak());
                        }
                        Status::Working => {
                            ui.label("Conversion…");
                        }
                        Status::Failed(e) => {
                            ui.colored_label(RED, e);
                        }
                        Status::Done(r) => {
                            if let Some(p) = self.registry.as_ref().and_then(|x| x.find_port(&r.meta.id, &r.meta.name)) {
                                ui.horizontal_wrapped(|ui| {
                                    ui.colored_label(GREEN, format!("★ Version Horizon officielle disponible : {} ({})", p.name, registry::kind_label(&p.kind)));
                                    if self.is_installed(&p.id) {
                                        ui.label("installée ✔");
                                    } else if ui.button("Installer la version complète").clicked() {
                                        install_port = Some(p.clone());
                                    }
                                });
                            }
                            ui.label(format!(
                                "{} ({}{}) → {} · {} fichiers, {} bloc(s), {} objet(s)",
                                r.meta.name,
                                r.meta.loader,
                                if r.has_code { ", avec code Java" } else { "" },
                                file_name(&r.output),
                                r.kept,
                                r.blocks,
                                r.items
                            ));
                            for w in &r.warnings {
                                ui.colored_label(ORANGE, format!("⚠ {w}"));
                            }
                            if !r.dropped.is_empty() || r.renamed > 0 || !r.namespaces.is_empty() {
                                egui::CollapsingHeader::new(format!("Détails ({} fichier(s) ignoré(s))", r.dropped.len())).id_salt(("d", i)).show(ui, |ui| {
                                    if r.renamed > 0 {
                                        ui.label(format!("{} chemin(s) mis à jour vers le format 26.3.", r.renamed));
                                    }
                                    if r.item_models > 0 {
                                        ui.label(format!("{} définition(s) d'objet ajoutée(s).", r.item_models));
                                    }
                                    if !r.namespaces.is_empty() {
                                        ui.label(format!("Namespaces en plus : {}", r.namespaces.join(", ")));
                                    }
                                    for (p, why) in r.dropped.iter().take(300) {
                                        ui.label(RichText::new(format!("{p} — {why}")).small().weak());
                                    }
                                });
                            }
                        }
                    }
                });
            }
        });
        if let Some(i) = remove {
            if !self.converting {
                self.jobs.remove(i);
            }
        }
        if let Some(p) = install_port {
            self.install_registry(ui.ctx(), p);
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        title(ui, "Réglages", "Dossiers, conversion et mises à jour.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Dossier du jeu Horizon :");
            ui.monospace(self.s.game_dir.display().to_string());
            if ui.small_button("Changer").clicked() {
                if let Some(d) = rfd::FileDialog::new().set_title("Dossier du jeu Horizon (celui qui contient options.txt)").pick_folder() {
                    if !pack::looks_like_game_dir(&d) {
                        self.toast = Some(("Ce dossier ne contient pas options.txt : vérifie que c'est le dossier du jeu Horizon.".into(), false));
                    }
                    self.s.game_dir = d;
                    self.refresh_library();
                }
            }
            if ui.small_button("Par défaut").clicked() {
                self.s.game_dir = game::default_game_dir();
                self.refresh_library();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Mods convertis :");
            ui.monospace(self.s.output_dir.display().to_string());
            if ui.small_button("Changer").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.s.output_dir = d;
                }
            }
        });
        ui.checkbox(&mut self.s.generate_registries, "Créer les blocs et objets des mods convertis (modèle vanilla deviné d'après le nom)");
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
            ui.label(RichText::new(s).weak());
        });
        ui.separator();
        ui.label(RichText::new("À propos").strong());
        ui.label(RichText::new("Créé par full_money").strong());
        ui.label("Oxyde est gratuit et open source (licence MIT, © 2026 full_money). Projet non officiel, sans lien avec Mojang, Microsoft ou l'équipe Horizon.");
        ui.hyperlink_to("Code source et mises à jour", "https://github.com/0full-money0/oxyde");
        ui.hyperlink_to("Horizon", "https://playhorizon.dev/");
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.s);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && !self.converting {
            self.add_paths(dropped);
        }

        // ---- temps, écran de démarrage, transitions
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
        if !self.installing.is_empty() {
            egui::TopBottomPanel::bottom("busy").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Installation en cours ({})…", self.installing.len()));
                });
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
                Tab::Library => self.ui_library(ui),
                Tab::Convert => self.ui_convert(ui),
                Tab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_settings(ui));
                }
            }
        });

        // ---- écran de démarrage
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

/// OXYDE_SHOT=<fichier.png> et OXYDE_SHOT_AT=<secondes> : l'app enregistre une capture d'elle-même puis se ferme.
/// Sert aux captures d'écran du README et aux tests.
fn self_screenshot(ctx: &egui::Context, elapsed: f32) {
    let Ok(path) = std::env::var("OXYDE_SHOT") else { return };
    let at: f32 = std::env::var("OXYDE_SHOT_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let asked = egui::Id::new("shot_asked");
    if elapsed >= at && !ctx.data(|d| d.get_temp::<bool>(asked).unwrap_or(false)) {
        ctx.data_mut(|d| d.insert_temp(asked, true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    let shot = ctx.input(|i| {
        i.raw.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(img) = shot {
        let [w, h] = img.size;
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
            let _ = buf.save(&path);
        }
        std::process::exit(0);
    }
}

fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8()).unwrap_or_default();
    let size = [img.width() as usize, img.height() as usize];
    ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
}

// ---------------------------------------------------------------------------
// Composants
// ---------------------------------------------------------------------------

/// Carte d'un projet ; renvoie true si on clique sur Installer.
#[allow(clippy::too_many_arguments)]
fn card(ui: &mut egui::Ui, icon: Option<&str>, title: &str, desc: &str, meta: &str, badge: &str, installed: bool, busy: bool, installable: bool) -> bool {
    let mut clicked = false;
    let resp = egui::Frame::NONE.fill(CARD).stroke(egui::Stroke::new(1.0_f32, BORDER)).corner_radius(12.0).inner_margin(12.0).outer_margin(egui::Margin { bottom: 8, ..Default::default() }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let size = egui::vec2(56.0, 56.0);
            match icon {
                Some(u) if u.starts_with("https://") => {
                    ui.add(egui::Image::new(u).fit_to_exact_size(size).corner_radius(8.0));
                }
                _ => {
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(r, 8.0, Color32::from_rgb(60, 45, 35));
                    let letter = title.chars().next().unwrap_or('?').to_uppercase().to_string();
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(26.0), RUST);
                }
            }
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 150.0).max(120.0));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(title).size(17.0).strong());
                    egui::Frame::NONE.fill(Color32::from_rgb(52, 36, 28)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(7, 2)).show(ui, |ui| {
                        ui.label(RichText::new(badge).small().color(Color32::from_rgb(245, 175, 125)));
                    });
                });
                let d: String = desc.chars().take(160).collect();
                ui.label(RichText::new(d).color(Color32::from_rgb(195, 198, 206)));
                ui.label(RichText::new(meta).small().color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if installed {
                    ui.label(RichText::new("✔ Installé").color(GREEN));
                } else if busy {
                    ui.spinner();
                } else if installable {
                    if ui.add(egui::Button::new(RichText::new("Installer").strong().color(Color32::WHITE)).fill(RUST).min_size(egui::vec2(110.0, 36.0))).clicked() {
                        clicked = true;
                    }
                } else {
                    ui.label(RichText::new(if badge == "Modpack" { "Via le launcher Horizon" } else { "Rien à convertir" }).small().color(MUTED));
                }
            });
        });
    });
    let mut r = resp.response.rect;
    r.max.y -= 8.0;
    fx::hover_outline(ui, r, 12.0);
    clicked
}

fn stat(ui: &mut egui::Ui, big: &str, small: &str) {
    surface(ui, 16, |ui| {
        ui.label(RichText::new(big).size(28.0).strong().color(Color32::WHITE));
        ui.label(RichText::new(small).color(MUTED));
    });
}

/// Bloc arrondi avec bordure fine, pleine largeur.
fn surface(ui: &mut egui::Ui, pad: i8, add: impl FnOnce(&mut egui::Ui)) {
    let resp = egui::Frame::NONE
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(12.0)
        .inner_margin(pad)
        .outer_margin(egui::Margin { bottom: 8, ..Default::default() })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let mut r = resp.response.rect;
    r.max.y -= 8.0;
    fx::hover_outline(ui, r, 12.0);
}

fn title(ui: &mut egui::Ui, t: &str, sub: &str) {
    ui.label(RichText::new(t).size(28.0).strong().color(Color32::WHITE));
    ui.label(RichText::new(sub).size(15.0).color(MUTED));
    ui.add_space(14.0);
}

/// Bouton de sélection de source.
fn segment(ui: &mut egui::Ui, selected: bool, label: &str) -> bool {
    let b = egui::Button::new(RichText::new(label).size(15.0).color(if selected { Color32::WHITE } else { MUTED }))
        .fill(if selected { Color32::from_rgb(52, 36, 28) } else { CARD })
        .stroke(egui::Stroke::new(1.0_f32, if selected { RUST } else { BORDER }))
        .corner_radius(18.0)
        .min_size(egui::vec2(0.0, 34.0));
    ui.add(b).clicked()
}

fn fmt_num(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1} k", n as f64 / 1e3).replace('.', ","),
        _ => format!("{:.1} M", n as f64 / 1e6).replace('.', ","),
    }
}

fn is_ext(p: &Path, ext: &str) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn sanitize_file(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let t = t.trim().to_string();
    if t.is_empty() {
        "modpack".into()
    } else {
        t
    }
}

fn open_folder(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}
