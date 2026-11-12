#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod pack;
mod update;

use convert::{Options, Report};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const GREEN: Color32 = Color32::from_rgb(80, 200, 120);
const RED: Color32 = Color32::from_rgb(235, 90, 90);
const ORANGE: Color32 = Color32::from_rgb(240, 170, 60);
const ACCENT: Color32 = Color32::from_rgb(110, 160, 255);

fn main() -> eframe::Result<()> {
    // Mode ligne de commande (utile pour les scripts) :
    //   HorizonModConverter.exe --convert <dossier-sortie> <mod.jar>...
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

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Horizon Mod Converter")
            .with_inner_size([820.0, 680.0])
            .with_min_inner_size([560.0, 460.0])
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native("Horizon Mod Converter", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Settings {
    output_dir: PathBuf,
    game_dir: Option<PathBuf>,
    generate_registries: bool,
    auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let base = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
        Self {
            output_dir: base.join("Horizon Mod Converter"),
            game_dir: None,
            generate_registries: true,
            auto_update: true,
        }
    }
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

struct App {
    settings: Settings,
    jobs: Vec<Job>,
    rx: Option<Receiver<(usize, Result<Report, String>)>>,
    pack_name: String,
    pending_pack: Option<(PathBuf, pack::PackInfo)>,
    upd: Arc<Mutex<Upd>>,
    toast: Option<(String, bool)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        for f in style.text_styles.values_mut() {
            f.size *= 1.1;
        }
        cc.egui_ctx.set_style(style);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let upd = Arc::new(Mutex::new(Upd::Off));
        let app = Self {
            settings,
            jobs: vec![],
            rx: None,
            pack_name: "Mon modpack".into(),
            pending_pack: None,
            upd,
            toast: None,
        };
        if app.settings.auto_update {
            app.check_updates(cc.egui_ctx.clone());
        }
        app
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

    fn busy(&self) -> bool {
        self.rx.is_some()
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
                self.toast = Some(("Ce fichier est déjà un mod Horizon (.hmod) : rien à convertir.".into(), true));
            } else {
                self.toast = Some((format!("Fichier non pris en charge : {}", file_name(&p)), false));
            }
        }
    }

    fn add_jar(&mut self, p: PathBuf) {
        if !self.jobs.iter().any(|j| j.path == p) {
            self.jobs.push(Job { path: p, status: Status::Pending });
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
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
        let (tx, rx): (Sender<_>, Receiver<_>) = channel();
        self.rx = Some(rx);
        let out = self.settings.output_dir.clone();
        let opts = Options { generate_registries: self.settings.generate_registries };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (i, p) in todo {
                let r = convert::convert_jar(&p, &out, &opts);
                if tx.send((i, r)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    fn poll(&mut self) {
        let mut finished = false;
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok((i, r)) => {
                        if let Some(j) = self.jobs.get_mut(i) {
                            j.status = match r {
                                Ok(rep) => Status::Done(Box::new(rep)),
                                Err(e) => Status::Failed(e),
                            };
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished {
            self.rx = None;
            let ok = self.converted().len();
            let ko = self.jobs.iter().filter(|j| matches!(j.status, Status::Failed(_))).count();
            self.toast = Some((format!("Terminé : {ok} mod(s) converti(s), {ko} impossible(s)."), ko == 0));
        }
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

    fn pick_game_dir(&mut self) -> bool {
        if let Some(d) = rfd::FileDialog::new()
            .set_title("Dossier de jeu Horizon (celui qui contient options.txt)")
            .pick_folder()
        {
            if !pack::looks_like_game_dir(&d) {
                self.toast = Some((
                    "Ce dossier ne contient pas options.txt : vérifie que c'est bien le dossier de jeu Horizon (lance le jeu une fois avant).".into(),
                    false,
                ));
            }
            self.settings.game_dir = Some(d);
            true
        } else {
            false
        }
    }

    fn game_dir(&mut self) -> Option<PathBuf> {
        if self.settings.game_dir.is_none() && !self.pick_game_dir() {
            return None;
        }
        self.settings.game_dir.clone()
    }

    // ---------------------------------------------------------------- UI

    fn ui_update_banner(&mut self, ui: &mut egui::Ui) {
        let state = std::mem::replace(&mut *self.upd.lock().unwrap(), Upd::Off);
        let mut next = None;
        match &state {
            Upd::Available(rel) => {
                egui::Frame::group(ui.style()).fill(Color32::from_rgb(30, 50, 90)).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Nouvelle version {} disponible", rel.version)).strong());
                        if ui.button("⬇ Mettre à jour maintenant").clicked() {
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
                            ui.hyperlink_to("Voir les nouveautés", &rel.page);
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
            Upd::Error(e) => {
                ui.colored_label(ORANGE, format!("Mise à jour : {e}"));
            }
            _ => {}
        }
        let mut guard = self.upd.lock().unwrap();
        if matches!(*guard, Upd::Off) {
            *guard = next.unwrap_or(state);
        }
    }

    fn ui_drop_zone(&mut self, ui: &mut egui::Ui) {
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let stroke = if hovering { egui::Stroke::new(2.5_f32, ACCENT) } else { egui::Stroke::new(1.5_f32, Color32::from_gray(90)) };
        egui::Frame::NONE
            .stroke(stroke)
            .corner_radius(10.0)
            .fill(if hovering { Color32::from_rgb(28, 38, 60) } else { Color32::from_gray(28) })
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("📦").size(40.0));
                    ui.label(RichText::new("Glisse tes mods Java (.jar) ici").size(22.0).strong());
                    ui.label("ou un dossier « mods » entier, ou un modpack .hzmods à installer");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let w: f32 = 360.0;
                        ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                        if ui.button("📄 Choisir des .jar").clicked() {
                            if let Some(files) = rfd::FileDialog::new().add_filter("Mods Java", &["jar"]).add_filter("Modpack", &[pack::PACK_EXT]).pick_files() {
                                self.add_paths(files);
                            }
                        }
                        if ui.button("📁 Choisir un dossier").clicked() {
                            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.add_paths(vec![d]);
                            }
                        }
                    });
                });
            });
    }

    fn ui_pending_pack(&mut self, ui: &mut egui::Ui) {
        let Some((path, info)) = self.pending_pack.clone() else { return };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("Modpack « {} »", info.name)).size(18.0).strong());
            if !info.description.is_empty() {
                ui.label(&info.description);
            }
            for m in &info.mods {
                ui.label(format!("  • {} {}", if m.name.is_empty() { &m.id } else { &m.name }, m.version));
            }
            ui.horizontal(|ui| {
                if ui.button(RichText::new("✔ Installer dans Horizon").strong()).clicked() {
                    if let Some(g) = self.game_dir() {
                        self.toast = Some(match pack::install_pack(&path, &g) {
                            Ok(ids) => (format!("{} mod(s) installé(s) et activé(s). Relance Horizon pour jouer avec.", ids.len()), true),
                            Err(e) => (e, false),
                        });
                        self.pending_pack = None;
                    }
                }
                if ui.button("Annuler").clicked() {
                    self.pending_pack = None;
                }
            });
        });
    }

    fn ui_jobs(&mut self, ui: &mut egui::Ui) {
        let mut remove = None;
        for (i, job) in self.jobs.iter().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match &job.status {
                        Status::Pending => ui.label(RichText::new("●").color(Color32::GRAY)),
                        Status::Working => ui.spinner(),
                        Status::Done(_) => ui.label(RichText::new("✔").color(GREEN)),
                        Status::Failed(_) => ui.label(RichText::new("✖").color(RED)),
                    };
                    ui.label(RichText::new(file_name(&job.path)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !matches!(job.status, Status::Working) && ui.small_button("✕").on_hover_text("Retirer").clicked() {
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
                        ui.label(format!(
                            "{} ({}{}) → {} — {} fichiers, {} bloc(s), {} objet(s)",
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
                            egui::CollapsingHeader::new(format!("Détails ({} fichier(s) ignoré(s))", r.dropped.len()))
                                .id_salt(("details", i))
                                .show(ui, |ui| {
                                    if r.renamed > 0 {
                                        ui.label(format!("{} chemin(s) mis à jour vers le format 26.3.", r.renamed));
                                    }
                                    if r.item_models > 0 {
                                        ui.label(format!("{} définition(s) d'objet ajoutée(s) (assets/…/items/).", r.item_models));
                                    }
                                    if !r.namespaces.is_empty() {
                                        ui.label(format!("Namespaces en plus : {}", r.namespaces.join(", ")));
                                    }
                                    egui::ScrollArea::vertical().max_height(160.0).id_salt(("drop", i)).show(ui, |ui| {
                                        for (p, why) in r.dropped.iter().take(500) {
                                            ui.label(RichText::new(format!("{p} — {why}")).small().weak());
                                        }
                                        if r.dropped.len() > 500 {
                                            ui.label(format!("… et {} autres", r.dropped.len() - 500));
                                        }
                                    });
                                });
                        }
                    }
                }
            });
        }
        if let Some(i) = remove {
            if !self.busy() {
                self.jobs.remove(i);
            }
        }
    }

    fn ui_actions(&mut self, ui: &mut egui::Ui) {
        let pending = self.jobs.iter().filter(|j| matches!(j.status, Status::Pending | Status::Failed(_))).count();
        ui.horizontal_wrapped(|ui| {
            let label = if self.busy() { "Conversion en cours…".to_string() } else { format!("▶ Convertir ({pending})") };
            if ui
                .add_enabled(pending > 0 && !self.busy(), egui::Button::new(RichText::new(label).size(17.0).strong()).fill(Color32::from_rgb(40, 90, 170)))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.start(&ctx);
            }
            if ui.add_enabled(!self.jobs.is_empty() && !self.busy(), egui::Button::new("Vider la liste")).clicked() {
                self.jobs.clear();
            }
        });

        let done = self.converted();
        if done.is_empty() || self.busy() {
            return;
        }
        ui.separator();
        ui.label(RichText::new(format!("{} mod(s) prêt(s)", done.len())).size(17.0).strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("📂 Ouvrir le dossier").clicked() {
                open_folder(&self.settings.output_dir);
            }
            if ui.button("🎮 Installer dans Horizon").on_hover_text("Copie les .hmod dans horizon-mods/ et les active").clicked() {
                if let Some(g) = self.game_dir() {
                    self.toast = Some(match pack::install_mods(&done, &g) {
                        Ok(ids) => (format!("{} mod(s) installé(s) et activé(s). Relance Horizon.", ids.len()), true),
                        Err(e) => (e, false),
                    });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Modpack à partager :");
            ui.add(egui::TextEdit::singleline(&mut self.pack_name).desired_width(200.0));
            if ui.button("🎁 Créer le modpack (.hzmods)").clicked() {
                let file = format!("{}.{}", sanitize_file(&self.pack_name), pack::PACK_EXT);
                if let Some(out) = rfd::FileDialog::new()
                    .set_directory(&self.settings.output_dir)
                    .set_file_name(&file)
                    .add_filter("Modpack", &[pack::PACK_EXT])
                    .save_file()
                {
                    self.toast = Some(match pack::create_pack(&self.pack_name, "", &done, &out) {
                        Ok(p) => (format!("Modpack créé : {}. Envoie ce fichier à tes amis : ils le glissent dans l'app pour l'installer.", file_name(&p)), true),
                        Err(e) => (e, false),
                    });
                }
            }
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("⚙ Réglages").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Dossier de sortie :");
                ui.monospace(self.settings.output_dir.display().to_string());
                if ui.small_button("Changer").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.settings.output_dir = d;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Dossier de jeu Horizon :");
                match &self.settings.game_dir {
                    Some(d) => ui.monospace(d.display().to_string()),
                    None => ui.label(RichText::new("pas encore choisi").weak()),
                };
                if ui.small_button("Changer").clicked() {
                    self.pick_game_dir();
                }
            });
            ui.checkbox(&mut self.settings.generate_registries, "Créer les blocs et objets automatiquement (modèle vanilla deviné d'après le nom)");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.settings.auto_update, "Chercher les mises à jour au démarrage");
                if update::REPO.is_some() && ui.small_button("Vérifier maintenant").clicked() {
                    self.check_updates(ui.ctx().clone());
                }
                let s = match &*self.upd.lock().unwrap() {
                    Upd::Checking => "vérification…",
                    Upd::UpToDate => "à jour ✔",
                    _ => "",
                };
                ui.label(RichText::new(s).weak());
            });
        });
        egui::CollapsingHeader::new("ℹ Ce qui est converti").show(ui, |ui| {
            ui.label("Horizon (Minecraft Rust Edition) ne peut pas exécuter le code Java des mods Forge, NeoForge, Fabric ou Quilt.");
            ui.label("L'app récupère tout ce qui n'est pas du code : textures, modèles, sons, langues, recettes, tags, tables de loot, avancements, structures et génération de monde, puis crée un mod de contenu Horizon (.hmod).");
            ui.label("Les mods de décoration, de minerais, de nourriture ou de structures marchent bien. Les mods techniques (machines, interfaces, nouveaux mobs) ne gardent que leur contenu visuel et leurs recettes.");
            ui.hyperlink_to("Documentation des mods Horizon", "https://playhorizon.dev/docs/mods");
        });
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && !self.busy() {
            self.add_paths(dropped);
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Horizon Mod Converter").size(22.0).strong().color(ACCENT));
                ui.label(RichText::new(format!("v{}", update::VERSION)).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Java → Horizon (Rust)").weak());
                });
            });
            self.ui_update_banner(ui);
            ui.add_space(4.0);
        });

        if let Some((msg, ok)) = self.toast.clone() {
            egui::TopBottomPanel::bottom("toast").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(if ok { GREEN } else { RED }, msg);
                    if ui.small_button("OK").clicked() {
                        self.toast = None;
                    }
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.ui_drop_zone(ui);
                self.ui_pending_pack(ui);
                if !self.jobs.is_empty() {
                    ui.add_space(4.0);
                    self.ui_actions(ui);
                    self.ui_jobs(ui);
                }
                ui.add_space(8.0);
                self.ui_settings(ui);
            });
        });
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
    if t.is_empty() { "modpack".into() } else { t }
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
