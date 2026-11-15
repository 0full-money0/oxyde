//! Identité visuelle d'Oxyde : police Inter, palette, boutons dégradés, onglets en pilule.

use crate::fx;
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Sense, Vec2};

pub const BG: Color32 = Color32::from_rgb(9, 9, 12);
pub const SURFACE: Color32 = Color32::from_rgb(18, 18, 24);
pub const SURFACE2: Color32 = Color32::from_rgb(25, 25, 33);
pub const LINE: Color32 = Color32::from_rgb(38, 38, 48);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 241);
pub const SUBTLE: Color32 = Color32::from_rgb(139, 139, 154);
pub const ACCENT_A: Color32 = Color32::from_rgb(255, 138, 42);
pub const ACCENT_B: Color32 = Color32::from_rgb(255, 61, 90);

pub fn semi() -> FontFamily {
    FontFamily::Name("semi".into())
}

pub fn heavy() -> FontFamily {
    FontFamily::Name("heavy".into())
}

/// Charge Inter (embarquée dans l'exe). Les polices d'egui restent en secours pour les symboles.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let add = |f: &mut egui::FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.to_owned(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
    };
    add(&mut fonts, "inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(&mut fonts, "inter-semi", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    add(&mut fonts, "inter-heavy", include_bytes!("../assets/fonts/Inter-ExtraBold.ttf"));
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| {
        let mut v = vec![first.to_owned()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, with("inter"));
    fonts.families.insert(semi(), with("inter-semi"));
    fonts.families.insert(heavy(), with("inter-heavy"));
    ctx.set_fonts(fonts);
}

pub fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.text_styles = [
        (egui::TextStyle::Small, FontId::proportional(12.0)),
        (egui::TextStyle::Body, FontId::proportional(14.5)),
        (egui::TextStyle::Button, FontId::new(14.0, semi())),
        (egui::TextStyle::Heading, FontId::new(26.0, heavy())),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.window_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.window_corner_radius = egui::CornerRadius::same(16);
    v.extreme_bg_color = Color32::from_rgb(14, 14, 19);
    v.faint_bg_color = SURFACE2;
    v.selection.bg_fill = Color32::from_rgb(255, 100, 60);
    v.hyperlink_color = ACCENT_A;
    v.override_text_color = None;
    let r = egui::CornerRadius::same(10);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.weak_bg_fill = SURFACE2;
    v.widgets.inactive.bg_fill = SURFACE2;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 34, 44);
    v.widgets.hovered.bg_fill = Color32::from_rgb(34, 34, 44);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(64, 64, 78));
    v.widgets.active.weak_bg_fill = Color32::from_rgb(40, 40, 52);
    style.visuals = v;
    ctx.set_style(style);
}

/// Bouton principal : dégradé orange → rouge, texte blanc.
pub fn primary(ui: &mut egui::Ui, label: &str, min: Vec2) -> egui::Response {
    primary_sized(ui, label, min, 14.5, true)
}

pub fn primary_sized(ui: &mut egui::Ui, label: &str, min: Vec2, size: f32, enabled: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(size, semi()), Color32::WHITE);
    let want = egui::vec2((galley.size().x + 32.0).max(min.x), (galley.size().y + 16.0).max(min.y));
    let (rect, resp) = ui.allocate_exact_size(want, if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.12);
    let radius = (rect.height() / 2.0).min(14.0);
    let p = ui.painter();
    // halo seulement pour les grands boutons (pas dans les listes)
    if enabled && rect.height() >= 40.0 {
        fx::soft_glow(p, rect.center() + egui::vec2(0.0, 6.0), rect.width() * 0.5, Color32::from_rgba_unmultiplied(255, 90, 60, (24.0 + 36.0 * hover) as u8));
    } else if enabled && hover > 0.0 {
        fx::soft_glow(p, rect.center(), rect.width() * 0.45, Color32::from_rgba_unmultiplied(255, 90, 60, (30.0 * hover) as u8));
    }
    let (a, b) = if enabled {
        (lighten(ACCENT_A, hover * 0.12), lighten(ACCENT_B, hover * 0.12))
    } else {
        (Color32::from_rgb(70, 60, 58), Color32::from_rgb(60, 50, 54))
    };
    fx::rounded_gradient(p, rect, radius, a, b);
    p.line_segment(
        [rect.left_top() + egui::vec2(radius, 1.0), rect.right_top() + egui::vec2(-radius, 1.0)],
        egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 60)),
    );
    p.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn lighten(c: Color32, k: f32) -> Color32 {
    let l = |x: u8| (x as f32 + (255.0 - x as f32) * k) as u8;
    Color32::from_rgb(l(c.r()), l(c.g()), l(c.b()))
}

/// Onglets en pilule dans un rail. Renvoie l'index cliqué.
pub fn pills(ui: &mut egui::Ui, labels: &[&str], selected: usize, key_prefix: &str) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::NONE.fill(Color32::from_rgb(16, 16, 22)).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(12.0).inner_margin(4.0).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.horizontal(|ui| {
            for (i, label) in labels.iter().enumerate() {
                let sel = i == selected;
                let galley = ui.painter().layout_no_wrap((*label).to_owned(), FontId::new(13.5, semi()), TEXT);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 26.0, 30.0), Sense::click());
                crate::tour::mark(ui, &format!("{key_prefix}{label}"), rect);
                let p = ui.painter();
                if sel {
                    p.rect_filled(rect, 9.0, SURFACE2);
                    p.rect_stroke(rect, 9.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(60, 60, 74)), egui::StrokeKind::Inside);
                    let bar = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 2.0), egui::vec2(18.0, 2.5));
                    fx::rounded_gradient(p, bar, 1.2, ACCENT_A, ACCENT_B);
                } else if resp.hovered() {
                    p.rect_filled(rect, 9.0, Color32::from_rgb(22, 22, 30));
                }
                let col = if sel { TEXT } else { SUBTLE };
                p.galley(rect.center() - galley.size() / 2.0 - egui::vec2(0.0, 1.0), galley, col);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    clicked = Some(i);
                }
            }
        });
    });
    clicked
}

pub fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text).family(heavy()).size(size).color(TEXT)
}

pub fn label_caps(text: &str) -> RichText {
    RichText::new(text.to_uppercase()).family(semi()).size(11.5).color(Color32::from_rgb(255, 150, 100))
}
