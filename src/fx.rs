//! Effets visuels : fond animé (blocs voxel qui dérivent, braises qui montent) et écran de chargement.
//! Tout est dessiné à la main avec le painter d'egui : aucune image externe.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Shape, Stroke};

const AUTHOR: &str = "BY FULL_MONEY";

#[derive(Default)]
pub struct Fx;

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Disque avec un dégradé du centre vers le bord.
fn radial(c: Pos2, radius: f32, inner: Color32, outer: Color32) -> Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(c, inner);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * radius, outer);
    }
    for i in 1..=n as u32 {
        m.add_triangle(0, i, i + 1);
    }
    Shape::mesh(m)
}

impl Fx {
    /// Fond de la fenêtre : aplat de pierre et une trame de blocs de 32 px, presque invisible. Rien ne bouge.
    pub fn background(&self, ctx: &egui::Context, strength: f32) {
        let rect = ctx.screen_rect();
        let p = ctx.layer_painter(egui::LayerId::background());
        p.rect_filled(rect, 0.0, crate::theme::BG);
        if strength <= 0.0 {
            return;
        }
        let line = Color32::from_rgba_unmultiplied(255, 240, 220, 2);
        let step = 48.0;
        let mut x = rect.left() + step;
        while x < rect.right() {
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, line));
            x += step;
        }
        let mut y = rect.top() + step;
        while y < rect.bottom() {
            p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0_f32, line));
            y += step;
        }
    }

    /// Écran de démarrage. `progress` 0..1, `alpha` 1 = opaque (fondu de sortie).
    pub fn splash(&self, ctx: &egui::Context, progress: f32, status: &str, alpha: f32, logo: &egui::TextureHandle) {
        let rect = ctx.screen_rect();
        let t = ctx.input(|i| i.time) as f32;
        egui::Area::new(egui::Id::new("splash")).order(egui::Order::Foreground).fixed_pos(rect.min).interactable(alpha > 0.5).show(ctx, |ui| {
            ui.allocate_rect(rect, egui::Sense::click());
            let p = ui.painter().clone();
            p.rect_filled(rect, 0.0, rgba(19, 18, 16, alpha));
            let c = rect.center() - vec2(0.0, 40.0);

            let _ = t;
            let s = 88.0;
            let img = Rect::from_center_size(c, vec2(s, s));
            p.rect_filled(img.translate(vec2(5.0, 5.0)), 4.0, rgba(0, 0, 0, 0.45 * alpha));
            p.image(logo.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), rgba(255, 255, 255, alpha));

            // la marque : OXYDE × HORIZON
            lockup(&p, c + vec2(0.0, 96.0), 0.8, alpha, None, t);
            spaced(&p, c + vec2(0.0, 130.0), egui::Align2::CENTER_CENTER, "MODS  ·  PROFILS  ·  MODPACKS  ·  LANCEMENT", 10.5, crate::theme::semi(), rgba(160, 150, 140, alpha), 2.0);

            // barre de chargement en segments, comme une barre d'XP
            let n = 24;
            let seg = 12.0;
            let gap = 3.0;
            let bw = n as f32 * (seg + gap) - gap;
            let left = c.x - bw / 2.0;
            let lit = (progress.clamp(0.0, 1.0) * n as f32).round() as usize;
            for i in 0..n {
                let r = Rect::from_min_size(pos2(left + i as f32 * (seg + gap), c.y + 166.0), vec2(seg, 6.0));
                p.rect_filled(r, 0.0, if i < lit { rgba(214, 118, 64, alpha) } else { rgba(52, 49, 43, alpha) });
            }
            spaced(&p, c + vec2(0.0, 192.0), egui::Align2::CENTER_CENTER, &status.to_uppercase(), 10.5, crate::theme::semi(), rgba(150, 145, 140, alpha), 1.5);
            spaced(&p, rect.center_bottom() - vec2(0.0, 22.0), egui::Align2::CENTER_CENTER, AUTHOR, 10.5, crate::theme::semi(), rgba(110, 105, 100, alpha), 2.0);
        });
    }
}

/// Liseré qui s'allume au survol d'un bloc (animation douce).
pub fn hover_outline(ui: &egui::Ui, rect: Rect, radius: f32) {
    let hovered = ui.rect_contains_pointer(rect);
    let k = ui.ctx().animate_bool_with_time(ui.id().with(("hov", rect.min.x as i32, rect.min.y as i32)), hovered, 0.15);
    if k > 0.0 {
        let c = Color32::from_rgb(
            (52.0 + (214.0 - 52.0) * k) as u8,
            (49.0 + (118.0 - 49.0) * k) as u8,
            (43.0 + (64.0 - 43.0) * k) as u8,
        );
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0_f32 + k * 0.5, c), egui::StrokeKind::Inside);
    }
}

// ---------------------------------------------------------------------------
// Profondeur : dégradés arrondis, diorama voxel, icônes
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Rectangle arrondi rempli d'un dégradé diagonal (haut-gauche -> bas-droite).
/// Ancien dégradé : désormais un aplat de la première couleur (angles de bloc).
pub fn rounded_gradient(p: &egui::Painter, r: Rect, radius: f32, from: Color32, _to: Color32) {
    p.rect_filled(r, radius.min(crate::theme::R), from);
}

/// Les halos ont disparu de l'identité : on garde la fonction pour ne rien casser.
pub fn soft_glow(_p: &egui::Painter, _c: Pos2, _radius: f32, _color: Color32) {}

fn shade(c: Color32, k: f32) -> Color32 {
    Color32::from_rgb((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8)
}

/// Un bloc isométrique opaque avec ses trois faces et un liseré de lumière.
fn block(p: &egui::Painter, c: Pos2, s: f32, top: Color32, side: Color32) {
    let w = s * 0.866;
    let t = vec![c + vec2(0.0, -s), c + vec2(w, -s * 0.5), c, c + vec2(-w, -s * 0.5)];
    let l = vec![c + vec2(-w, -s * 0.5), c, c + vec2(0.0, s), c + vec2(-w, s * 0.5)];
    let r = vec![c, c + vec2(w, -s * 0.5), c + vec2(w, s * 0.5), c + vec2(0.0, s)];
    p.add(Shape::convex_polygon(l, shade(side, 0.78), Stroke::NONE));
    p.add(Shape::convex_polygon(r, shade(side, 0.58), Stroke::NONE));
    p.add(Shape::convex_polygon(t.clone(), top, Stroke::NONE));
    p.add(Shape::line(vec![t[3], t[0], t[1]], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28))));
}

/// Petite île de blocs qui flotte : herbe, terre, pierre, minerai brûlant, arbre et eau.
pub fn diorama(p: &egui::Painter, center: Pos2, s: f32, t: f32) {
    let bob = (t * 1.1).sin() * 6.0;
    let center = center + vec2(0.0, bob);
    // ombre portée
    p.add(radial(center + vec2(0.0, s * 6.2 - bob), s * 5.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120), Color32::TRANSPARENT));
    let w = s * 0.866;
    let pos = |gx: i32, gy: i32, gz: i32| center + vec2((gx - gy) as f32 * w, (gx + gy) as f32 * s * 0.5 - gz as f32 * s);
    let grass = Color32::from_rgb(104, 176, 72);
    let dirt = Color32::from_rgb(134, 92, 60);
    let stone = Color32::from_rgb(120, 122, 128);
    let water = Color32::from_rgb(70, 130, 200);
    let ore = Color32::from_rgb(92, 182, 155);
    let log = Color32::from_rgb(110, 80, 50);
    let leaf = Color32::from_rgb(70, 140, 60);
    let n = 5;
    let land = |x: i32, y: i32| !((x == 0 || x == n - 1) && (y == 0 || y == n - 1));
    let is_water = |x: i32, y: i32| (x == 3 && y == 1) || (x == 3 && y == 2) || (x == 4 && y == 2);
    let mut blocks: Vec<(i32, i32, i32, Color32, Color32, bool)> = vec![];
    for x in 0..n {
        for y in 0..n {
            if !land(x, y) {
                continue;
            }
            let glow = (x == 1 && y == 3) || (x == 2 && y == 4);
            blocks.push((x, y, -1, if glow { ore } else { stone }, if glow { ore } else { stone }, glow));
            if is_water(x, y) {
                blocks.push((x, y, 0, water, Color32::from_rgb(50, 95, 160), false));
            } else {
                blocks.push((x, y, 0, grass, dirt, false));
            }
        }
    }
    // arbre
    for z in 1..=2 {
        blocks.push((1, 1, z, log, log, false));
    }
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)] {
        blocks.push((1 + dx, 1 + dy, 3, leaf, shade(leaf, 0.9), false));
    }
    blocks.push((1, 1, 4, leaf, shade(leaf, 0.9), false));
    // bloc de rouille posé sur l'herbe
    blocks.push((2, 3, 1, Color32::from_rgb(214, 118, 64), Color32::from_rgb(170, 90, 48), true));
    blocks.sort_by_key(|b| (b.0 + b.1, b.2, b.0));
    for (x, y, z, top, side, glow) in blocks {
        let c = pos(x, y, z);
        let _ = glow;
        block(p, c, s, top, side);
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Store,
    Packs,
    Mods,
}

/// Petites icônes vectorielles du menu.
pub fn icon(p: &egui::Painter, kind: Icon, c: Pos2, color: Color32) {
    let st = Stroke::new(1.8_f32, color);
    let pt = |x: f32, y: f32| c + vec2(x, y);
    match kind {
        Icon::Store => {
            p.add(Shape::closed_line(vec![pt(-7.0, -3.0), pt(7.0, -3.0), pt(6.0, 7.0), pt(-6.0, 7.0)], st));
            p.add(Shape::line(vec![pt(-3.0, -3.0), pt(-3.0, -6.0), pt(3.0, -6.0), pt(3.0, -3.0)], st));
        }
        Icon::Packs => {
            for dy in [-4.0, 0.0, 4.0] {
                p.add(Shape::closed_line(vec![pt(0.0, dy - 4.0), pt(8.0, dy), pt(0.0, dy + 4.0), pt(-8.0, dy)], st));
            }
        }
        Icon::Mods => {
            let (w, s) = (7.0, 8.0);
            p.add(Shape::closed_line(vec![pt(0.0, -s), pt(w, -s * 0.5), pt(w, s * 0.5), pt(0.0, s), pt(-w, s * 0.5), pt(-w, -s * 0.5)], st));
            p.add(Shape::line(vec![pt(-w, -s * 0.5), pt(0.0, 0.0), pt(w, -s * 0.5)], st));
            p.add(Shape::line_segment([pt(0.0, 0.0), pt(0.0, s)], st));
        }
    }
}

// ---------------------------------------------------------------------------
// Marque OXYDE × HORIZON, texte espacé, petits éléments dessinés
// ---------------------------------------------------------------------------

pub fn darken(c: Color32, k: f32) -> Color32 {
    shade(c, k)
}

fn spaced_galley(p: &egui::Painter, text: &str, size: f32, family: egui::FontFamily, color: Color32, spacing: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, egui::TextFormat { font_id: egui::FontId::new(size, family), color, extra_letter_spacing: spacing, ..Default::default() });
    p.layout_job(job)
}

/// Texte aux lettres espacées, placé comme `Painter::text`.
#[allow(clippy::too_many_arguments)]
pub fn spaced(p: &egui::Painter, pos: Pos2, align: egui::Align2, text: &str, size: f32, family: egui::FontFamily, color: Color32, spacing: f32) {
    let g = spaced_galley(p, text, size, family, color, spacing);
    let r = align.anchor_size(pos, g.size());
    p.galley(r.min, g, color);
}

/// « [logo] OXYDE × HORIZON », centré sur `center`.
pub fn lockup(p: &egui::Painter, center: Pos2, scale: f32, alpha: f32, logo: Option<&egui::TextureHandle>, t: f32) {
    let size = 40.0 * scale;
    let white = rgba(245, 242, 238, alpha);
    let cold = rgba(154, 146, 133, alpha);
    let a = spaced_galley(p, "OXYDE", size, crate::theme::heavy(), white, 6.0 * scale);
    let x = spaced_galley(p, "×", size * 0.8, crate::theme::semi(), rgba(214, 118, 64, alpha), 0.0);
    let b = spaced_galley(p, "HORIZON", size, crate::theme::heavy(), cold, 6.0 * scale);
    let logo_w = if logo.is_some() { size * 1.35 + 18.0 * scale } else { 0.0 };
    let gap = 22.0 * scale;
    let total = logo_w + a.size().x + gap + x.size().x + gap + b.size().x;
    let mut cx = center.x - total / 2.0;
    if let Some(l) = logo {
        let s = size * 1.35;
        let r = Rect::from_min_size(pos2(cx, center.y - s / 2.0), vec2(s, s));
        let _ = t;
        p.rect_filled(r.translate(vec2(4.0, 4.0)), 4.0, rgba(0, 0, 0, 0.45 * alpha));
        p.image(l.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), rgba(255, 255, 255, alpha));
        cx += logo_w;
    }
    let y = |g: &egui::Galley| center.y - g.size().y / 2.0;
    let (ya, yx, yb) = (y(&a), y(&x), y(&b));
    let (wa, wx) = (a.size().x, x.size().x);
    p.galley(pos2(cx, ya), a, white);
    cx += wa + gap;
    p.galley(pos2(cx, yx), x, white);
    cx += wx + gap;
    p.galley(pos2(cx, yb), b, cold);
}

/// Petit anneau qui tourne (chargement).
pub fn spinner(p: &egui::Painter, c: Pos2, r: f32, t: f32, color: Color32) {
    p.circle_stroke(c, r, Stroke::new(1.6_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25)));
    let start = t * 5.0;
    let pts: Vec<Pos2> = (0..=16).map(|i| {
        let a = start + i as f32 / 16.0 * 4.0;
        c + vec2(a.cos(), a.sin()) * r
    }).collect();
    p.add(Shape::line(pts, Stroke::new(2.0_f32, color)));
}

/// Contour en pointillés d'un rectangle arrondi.
pub fn dashed_rect(p: &egui::Painter, r: Rect, radius: f32, stroke: Stroke) {
    let mut pts = vec![];
    let corners = [
        (r.right_bottom() - vec2(radius, radius), 0.0),
        (r.left_bottom() + vec2(radius, -radius), 90.0),
        (r.left_top() + vec2(radius, radius), 180.0),
        (r.right_top() + vec2(-radius, radius), 270.0),
    ];
    for (c, start) in corners {
        for i in 0..=6 {
            let a = (start + i as f32 * 15.0f32).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * radius);
        }
    }
    pts.push(pts[0]);
    p.extend(Shape::dashed_line(&pts, stroke, 7.0, 5.0));
}

/// Motif de blocs sur la couverture d'un profil sans image.
pub fn cover_pattern(p: &egui::Painter, r: Rect, c: Color32) {
    let p = p.with_clip_rect(r.shrink(1.0));
    let s = r.height() * 0.2;
    let light = Color32::from_rgba_unmultiplied(255, 255, 255, 30);
    for (i, (fx_, fy)) in [(0.78, 0.42), (0.9, 0.7), (0.66, 0.78), (0.95, 0.2)].iter().enumerate() {
        let center = pos2(r.left() + r.width() * fx_, r.top() + r.height() * fy);
        let k = 1.0 - i as f32 * 0.18;
        block(&p, center, s * k, shade(c, 1.15).gamma_multiply(0.9), shade(c, 0.8));
        p.line_segment([center + vec2(0.0, -s * k), center + vec2(s * k * 0.866, -s * k * 0.5)], Stroke::new(1.0_f32, light));
    }
}
