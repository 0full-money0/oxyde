//! Composants visuels réutilisables : couleurs, cartes, titres, boutons.

use crate::fx;
use eframe::egui::{self, Color32, RichText};
use std::path::Path;

pub const GREEN: Color32 = Color32::from_rgb(80, 200, 120);
pub const RED: Color32 = Color32::from_rgb(235, 90, 90);
pub const ORANGE: Color32 = Color32::from_rgb(240, 170, 60);
pub const RUST: Color32 = Color32::from_rgb(222, 110, 40);
pub const BG: Color32 = Color32::from_rgb(17, 18, 21);
pub const PANEL: Color32 = Color32::from_rgb(24, 25, 29);
pub const CARD: Color32 = Color32::from_rgba_premultiplied(27, 28, 33, 232);
pub const BORDER: Color32 = Color32::from_rgb(44, 46, 53);
pub const MUTED: Color32 = Color32::from_rgb(150, 154, 165);

pub fn self_screenshot(ctx: &egui::Context, elapsed: f32) {
    record(ctx, elapsed);
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

/// Enregistrement vidéo (pour le trailer) : OXYDE_REC=<dossier>, OXYDE_REC_FROM et OXYDE_REC_SECS en secondes.
/// Chaque image est enregistrée avec son horodatage (ms) ; ffmpeg en fait une vidéo à vitesse réelle.
fn record(ctx: &egui::Context, elapsed: f32) {
    let Ok(dir) = std::env::var("OXYDE_REC") else { return };
    let from: f32 = std::env::var("OXYDE_REC_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let secs: f32 = std::env::var("OXYDE_REC_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(5.0);
    ctx.request_repaint();
    if elapsed >= from && elapsed <= from + secs {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(((elapsed - from) * 1000.0) as u64)));
    }
    let shots: Vec<(u64, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { image, user_data, .. } => {
                    let ms = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied().unwrap_or(0);
                    Some((ms, image.clone()))
                }
                _ => None,
            })
            .collect()
    });
    for (ms, img) in shots {
        let dir = dir.clone();
        std::thread::spawn(move || {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = image::DynamicImage::ImageRgba8(buf).to_rgb8().save(format!("{dir}/f{ms:07}.bmp"));
            }
        });
    }
    if elapsed > from + secs + 1.0 {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::process::exit(0);
    }
}

/// Défilement automatique pendant un enregistrement (OXYDE_REC_SCROLL = pixels par seconde).
pub fn rec_scroll(elapsed: f32) -> Option<f32> {
    let speed: f32 = std::env::var("OXYDE_REC_SCROLL").ok()?.parse().ok()?;
    let from: f32 = std::env::var("OXYDE_REC_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Some(((elapsed - from).max(0.0)) * speed)
}

pub fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8()).unwrap_or_default();
    let size = [img.width() as usize, img.height() as usize];
    ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
}

// ---------------------------------------------------------------------------
// Composants
// ---------------------------------------------------------------------------

/// Carte d'un projet ; renvoie true si on clique sur Installer.
#[allow(clippy::too_many_arguments)]
pub fn card(ui: &mut egui::Ui, icon: Option<&str>, title: &str, desc: &str, meta: &str, badge: &str, installed: bool, busy: bool, installable: bool) -> bool {
    let mut clicked = false;
    let resp = egui::Frame::NONE
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(14.0)
        .inner_margin(14.0)
        .shadow(egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(120) })
        .outer_margin(egui::Margin { bottom: 10, ..Default::default() })
        .show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let size = egui::vec2(60.0, 60.0);
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
    r.max.y -= 10.0;
    top_light(ui, r);
    fx::hover_outline(ui, r, 14.0);
    clicked
}

pub fn stat(ui: &mut egui::Ui, icon: fx::Icon, big: &str, small: &str) {
    surface(ui, 16, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
            fx::rounded_gradient(ui.painter(), r, 12.0, Color32::from_rgb(232, 124, 52), Color32::from_rgb(150, 55, 20));
            fx::icon(ui.painter(), icon, r.center(), Color32::WHITE);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(big).size(26.0).strong().color(Color32::WHITE));
                ui.label(RichText::new(small).color(MUTED));
            });
        });
    });
}

/// Liseré clair en haut d'un bloc : donne l'impression d'une lumière venant d'au-dessus.
pub fn top_light(ui: &egui::Ui, r: egui::Rect) {
    let y = r.top() + 1.0;
    ui.painter().line_segment([egui::pos2(r.left() + 14.0, y), egui::pos2(r.right() - 14.0, y)], egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)));
}

/// Petit titre de section en capitales espacées.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    let spaced: String = text.to_uppercase().chars().flat_map(|c| [c, ' ']).collect();
    ui.label(RichText::new(spaced.trim_end()).size(12.0).strong().color(Color32::from_rgb(200, 140, 100)));
    ui.add_space(2.0);
}

/// Bloc arrondi avec bordure fine, pleine largeur.
pub fn surface(ui: &mut egui::Ui, pad: i8, add: impl FnOnce(&mut egui::Ui)) {
    let resp = egui::Frame::NONE
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(14.0)
        .inner_margin(pad)
        .shadow(egui::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(120) })
        .outer_margin(egui::Margin { bottom: 10, ..Default::default() })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let mut r = resp.response.rect;
    r.max.y -= 10.0;
    top_light(ui, r);
    fx::hover_outline(ui, r, 14.0);
}

pub fn title(ui: &mut egui::Ui, t: &str, sub: &str) {
    let r = ui.label(RichText::new(t).size(30.0).strong().color(Color32::WHITE)).rect;
    let bar = egui::Rect::from_min_size(egui::pos2(r.left(), r.bottom() + 3.0), egui::vec2(42.0, 3.0));
    fx::rounded_gradient(ui.painter(), bar, 1.5, RUST, Color32::from_rgb(255, 180, 90));
    ui.add_space(8.0);
    ui.label(RichText::new(sub).size(15.0).color(MUTED));
    ui.add_space(14.0);
}

/// Bouton de sélection de source.
pub fn segment(ui: &mut egui::Ui, selected: bool, label: &str) -> bool {
    let b = egui::Button::new(RichText::new(label).size(15.0).color(if selected { Color32::WHITE } else { MUTED }))
        .fill(if selected { Color32::from_rgb(52, 36, 28) } else { CARD })
        .stroke(egui::Stroke::new(1.0_f32, if selected { RUST } else { BORDER }))
        .corner_radius(18.0)
        .min_size(egui::vec2(0.0, 34.0));
    let r = ui.add(b);
    crate::tour::mark(ui, &format!("seg:{label}"), r.rect);
    r.clicked()
}

pub fn fmt_num(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1} k", n as f64 / 1e3).replace('.', ","),
        _ => format!("{:.1} M", n as f64 / 1e6).replace('.', ","),
    }
}

pub fn is_ext(p: &Path, ext: &str) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn sanitize_file(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    let t = t.trim().to_string();
    if t.is_empty() {
        "modpack".into()
    } else {
        t
    }
}

pub fn open_folder(p: &Path) {
    let _ = std::fs::create_dir_all(p);
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

/// Chemin lisible : le dossier personnel devient « ~ » (pas de nom d'utilisateur à l'écran).
pub fn pretty_path(p: &Path) -> String {
    let s = p.display().to_string().replace('/', "\\");
    if let Some(home) = dirs::home_dir() {
        let h = home.display().to_string();
        if let Some(rest) = s.strip_prefix(&h) {
            return format!("~{rest}");
        }
    }
    s
}
