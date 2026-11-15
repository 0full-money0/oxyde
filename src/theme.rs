//! Identité visuelle d'Oxyde : le cuivre qui s'oxyde.
//! Pierre sombre et chaude, cuivre mat, vert-de-gris. Aplats, angles nets comme des blocs,
//! boutons avec un relief de bloc. Aucun dégradé, aucun halo.

use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Sense, Vec2};

pub const BG: Color32 = Color32::from_rgb(19, 18, 16);
pub const SURFACE: Color32 = Color32::from_rgb(27, 26, 23);
pub const SURFACE2: Color32 = Color32::from_rgb(36, 34, 30);
pub const LINE: Color32 = Color32::from_rgb(52, 49, 43);
pub const TEXT: Color32 = Color32::from_rgb(237, 231, 220);
pub const SUBTLE: Color32 = Color32::from_rgb(154, 146, 133);
/// Cuivre.
pub const ACCENT_A: Color32 = Color32::from_rgb(214, 118, 64);
/// Vert-de-gris (le cuivre oxydé).
pub const ACCENT_B: Color32 = Color32::from_rgb(92, 182, 155);
/// Rayon des angles : presque carré, comme un bloc.
pub const R: f32 = 4.0;

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
    v.window_corner_radius = egui::CornerRadius::same(4);
    v.window_shadow = egui::Shadow { offset: [6, 6], blur: 0, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [4, 4], blur: 0, spread: 0, color: Color32::from_black_alpha(110) };
    v.extreme_bg_color = Color32::from_rgb(15, 14, 12);
    v.faint_bg_color = SURFACE2;
    v.selection.bg_fill = Color32::from_rgb(150, 82, 44);
    v.selection.stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.hyperlink_color = ACCENT_B;
    v.override_text_color = None;
    let r = egui::CornerRadius::same(3);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.weak_bg_fill = SURFACE2;
    v.widgets.inactive.bg_fill = SURFACE2;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, LINE);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 43, 38);
    v.widgets.hovered.bg_fill = Color32::from_rgb(46, 43, 38);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(92, 84, 72));
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(56, 52, 46);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT_A);
    style.visuals = v;
    ctx.set_style(style);
}

/// Bouton principal : aplat de cuivre, avec le relief d'un bloc (arête sombre en bas).
pub fn primary(ui: &mut egui::Ui, label: &str, min: Vec2) -> egui::Response {
    primary_sized(ui, label, min, 14.5, true)
}

pub fn primary_sized(ui: &mut egui::Ui, label: &str, min: Vec2, size: f32, enabled: bool) -> egui::Response {
    let ink = if enabled { Color32::from_rgb(255, 247, 238) } else { Color32::from_rgb(140, 132, 120) };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(size, semi()), ink);
    let want = egui::vec2((galley.size().x + 32.0).max(min.x), (galley.size().y + 16.0).max(min.y));
    let (rect, resp) = ui.allocate_exact_size(want, if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.08);
    let pressed = enabled && resp.is_pointer_button_down_on();
    let p = ui.painter();
    let edge = 3.0;
    let (face, side) = if enabled { (lighten(ACCENT_A, hover * 0.1), Color32::from_rgb(140, 70, 34)) } else { (Color32::from_rgb(58, 54, 48), Color32::from_rgb(40, 37, 33)) };
    let face_rect = if pressed { rect.with_min_y(rect.top() + edge) } else { rect.with_max_y(rect.bottom() - edge) };
    if !pressed {
        p.rect_filled(rect, R, side);
    }
    p.rect_filled(face_rect, R, face);
    // liseré clair en haut : la lumière tombe sur l'arête du bloc
    p.line_segment(
        [face_rect.left_top() + egui::vec2(R, 1.0), face_rect.right_top() + egui::vec2(-R, 1.0)],
        egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 230, 200, if enabled { 70 } else { 15 })),
    );
    p.galley(face_rect.center() - galley.size() / 2.0, galley, ink);
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

/// Onglets côte à côte dans un rail. Renvoie l'index cliqué.
pub fn pills(ui: &mut egui::Ui, labels: &[&str], selected: usize, key_prefix: &str) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::NONE.fill(Color32::from_rgb(22, 21, 18)).stroke(egui::Stroke::new(1.0_f32, LINE)).corner_radius(R).inner_margin(3.0).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.horizontal(|ui| {
            for (i, label) in labels.iter().enumerate() {
                let sel = i == selected;
                let galley = ui.painter().layout_no_wrap((*label).to_owned(), FontId::new(13.5, semi()), TEXT);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 26.0, 30.0), Sense::click());
                crate::tour::mark(ui, &format!("{key_prefix}{label}"), rect);
                let p = ui.painter();
                if sel {
                    p.rect_filled(rect, 3.0, SURFACE2);
                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom()), 0.0, ACCENT_A);
                } else if resp.hovered() {
                    p.rect_filled(rect, 3.0, Color32::from_rgb(30, 29, 25));
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
    RichText::new(text.to_uppercase()).family(semi()).size(11.0).extra_letter_spacing(1.6).color(ACCENT_B)
}
