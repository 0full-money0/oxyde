//! Effets visuels : fond animé (blocs voxel qui dérivent, braises qui montent) et écran de chargement.
//! Tout est dessiné à la main avec le painter d'egui : aucune image externe.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Shape, Stroke};

const AUTHOR: &str = "by full_money";

struct Cube {
    x: f32,     // 0..1 de la largeur
    phase: f32, // 0..1 de la hauteur
    speed: f32, // px / s
    size: f32,
    alpha: f32,
    sway: f32,
}

struct Ember {
    x: f32,
    phase: f32,
    speed: f32,
    r: f32,
    flicker: f32,
}

pub struct Fx {
    cubes: Vec<Cube>,
    embers: Vec<Ember>,
}

/// Petit générateur pseudo-aléatoire déterministe (pas besoin de crate).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32) / (u32::MAX >> 1) as f32
    }
}

impl Default for Fx {
    fn default() -> Self {
        let mut r = Lcg(0x005E_ED0F_0A1D);
        let cubes = (0..26)
            .map(|_| {
                let depth = r.next();
                Cube {
                    x: r.next(),
                    phase: r.next(),
                    speed: 6.0 + depth * 18.0,
                    size: 10.0 + depth * 30.0,
                    alpha: 0.025 + depth * 0.075,
                    sway: r.next() * std::f32::consts::TAU,
                }
            })
            .collect();
        let embers = (0..70)
            .map(|_| Ember { x: r.next(), phase: r.next(), speed: 18.0 + r.next() * 45.0, r: 0.8 + r.next() * 1.8, flicker: r.next() * 10.0 })
            .collect();
        Self { cubes, embers }
    }
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Rectangle avec un dégradé vertical (haut -> bas).
fn vertical_gradient(r: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), top);
    m.colored_vertex(r.right_top(), top);
    m.colored_vertex(r.right_bottom(), bottom);
    m.colored_vertex(r.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    Shape::mesh(m)
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

/// Cube isométrique : dessus clair, côtés plus sombres.
fn cube(p: &egui::Painter, c: Pos2, s: f32, a: f32) {
    let w = s * 0.866;
    let top = vec![c + vec2(0.0, -s), c + vec2(w, -s * 0.5), c, c + vec2(-w, -s * 0.5)];
    let left = vec![c + vec2(-w, -s * 0.5), c, c + vec2(0.0, s), c + vec2(-w, s * 0.5)];
    let right = vec![c, c + vec2(w, -s * 0.5), c + vec2(w, s * 0.5), c + vec2(0.0, s)];
    p.add(Shape::convex_polygon(top, rgba(232, 124, 62, a), Stroke::NONE));
    p.add(Shape::convex_polygon(left, rgba(150, 68, 32, a), Stroke::NONE));
    p.add(Shape::convex_polygon(right, rgba(104, 46, 22, a), Stroke::NONE));
}

impl Fx {
    /// Fond animé sur toute la fenêtre, derrière tous les panneaux.
    pub fn background(&self, ctx: &egui::Context, strength: f32) {
        let rect = ctx.screen_rect();
        let t = ctx.input(|i| i.time) as f32;
        let p = ctx.layer_painter(egui::LayerId::background());
        p.rect_filled(rect, 0.0, Color32::from_rgb(14, 14, 17));
        // lueur chaude en bas de l'écran (dégradé lisse)
        let glow = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * 0.55), rect.right_bottom());
        p.add(vertical_gradient(glow, rgba(110, 40, 14, 0.0), rgba(110, 40, 14, 0.22 * strength)));
        let h = rect.height() + 80.0;
        for c in &self.cubes {
            let y = rect.bottom() + 40.0 - ((c.phase * h + t * c.speed) % h);
            let x = rect.left() + c.x * rect.width() + (t * 0.25 + c.sway).sin() * 14.0;
            cube(&p, pos2(x, y), c.size, c.alpha * strength);
        }
        for e in &self.embers {
            let y = rect.bottom() + 10.0 - ((e.phase * h + t * e.speed) % h);
            let x = rect.left() + e.x * rect.width() + (t * 0.9 + e.flicker).sin() * 6.0;
            let life = ((rect.bottom() - y) / rect.height()).clamp(0.0, 1.0);
            let a = (0.55 + 0.45 * (t * 6.0 + e.flicker).sin()) * (1.0 - life) * 0.8 * strength;
            p.circle_filled(pos2(x, y), e.r, rgba(255, 150, 70, a));
            p.circle_filled(pos2(x, y), e.r * 3.0, rgba(255, 110, 40, a * 0.12));
        }
    }

    /// Écran de démarrage. `progress` 0..1, `alpha` 1 = opaque (fondu de sortie).
    pub fn splash(&self, ctx: &egui::Context, progress: f32, status: &str, alpha: f32, logo: &egui::TextureHandle) {
        let rect = ctx.screen_rect();
        let t = ctx.input(|i| i.time) as f32;
        egui::Area::new(egui::Id::new("splash")).order(egui::Order::Foreground).fixed_pos(rect.min).interactable(alpha > 0.5).show(ctx, |ui| {
            ui.allocate_rect(rect, egui::Sense::click());
            let p = ui.painter();
            p.rect_filled(rect, 0.0, rgba(14, 14, 17, alpha));
            let c = rect.center() - vec2(0.0, 40.0);

            // logo qui flotte, avec un halo qui respire
            let bob = (t * 2.0).sin() * 5.0;
            let halo = 0.25 + 0.15 * (t * 2.5).sin();
            p.add(radial(c + vec2(0.0, bob), 150.0, rgba(222, 110, 40, halo * 0.55 * alpha), rgba(222, 110, 40, 0.0)));
            let s = 96.0;
            let img = Rect::from_center_size(c + vec2(0.0, bob), vec2(s, s));
            p.image(logo.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), rgba(255, 255, 255, alpha));

            // titre en lettres espacées
            let title = "O X Y D E";
            p.text(c + vec2(0.0, 92.0), egui::Align2::CENTER_CENTER, title, egui::FontId::monospace(34.0), rgba(240, 236, 230, alpha));
            p.text(
                c + vec2(0.0, 124.0),
                egui::Align2::CENTER_CENTER,
                "MODS · MODPACKS · LAUNCHER POUR HORIZON",
                egui::FontId::monospace(11.0),
                rgba(160, 150, 140, alpha),
            );

            // barre de chargement fine, avec un reflet qui glisse
            let bw = 300.0;
            let bar = Rect::from_center_size(c + vec2(0.0, 170.0), vec2(bw, 4.0));
            p.rect_filled(bar, 2.0, rgba(50, 46, 44, alpha));
            let fill = Rect::from_min_size(bar.min, vec2(bw * progress.clamp(0.0, 1.0), 4.0));
            p.rect_filled(fill, 2.0, rgba(222, 110, 40, alpha));
            let sx = bar.left() + ((t * 0.8) % 1.0) * fill.width();
            if fill.width() > 20.0 {
                p.rect_filled(Rect::from_center_size(pos2(sx, bar.center().y), vec2(24.0, 4.0)), 2.0, rgba(255, 200, 150, 0.5 * alpha));
            }
            p.text(c + vec2(0.0, 192.0), egui::Align2::CENTER_CENTER, status, egui::FontId::monospace(12.0), rgba(150, 145, 140, alpha));
            p.text(
                rect.center_bottom() - vec2(0.0, 22.0),
                egui::Align2::CENTER_CENTER,
                AUTHOR,
                egui::FontId::monospace(11.0),
                rgba(110, 105, 100, alpha),
            );
        });
    }
}

/// Liseré qui s'allume au survol d'un bloc (animation douce).
pub fn hover_outline(ui: &egui::Ui, rect: Rect, radius: f32) {
    let hovered = ui.rect_contains_pointer(rect);
    let k = ui.ctx().animate_bool_with_time(ui.id().with(("hov", rect.min.x as i32, rect.min.y as i32)), hovered, 0.15);
    if k > 0.0 {
        let c = Color32::from_rgb(
            (44.0 + (222.0 - 44.0) * k) as u8,
            (46.0 + (110.0 - 46.0) * k) as u8,
            (53.0 + (40.0 - 53.0) * k) as u8,
        );
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0_f32 + k * 0.5, c), egui::StrokeKind::Inside);
    }
}

// ---------------------------------------------------------------------------
// Profondeur : dégradés arrondis, diorama voxel, icônes
// ---------------------------------------------------------------------------

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Rectangle arrondi rempli d'un dégradé diagonal (haut-gauche -> bas-droite).
pub fn rounded_gradient(p: &egui::Painter, r: Rect, radius: f32, from: Color32, to: Color32) {
    let mut pts = vec![];
    let corners = [
        (r.right_bottom() - vec2(radius, radius), 0.0),
        (r.left_bottom() + vec2(radius, -radius), 90.0),
        (r.left_top() + vec2(radius, radius), 180.0),
        (r.right_top() + vec2(-radius, radius), 270.0),
    ];
    for (c, start) in corners {
        for i in 0..=8 {
            let a = (start + i as f32 * 90.0 / 8.0f32).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * radius);
        }
    }
    let mut m = egui::Mesh::default();
    let diag = (r.width() + r.height()).max(1.0);
    let col = |q: Pos2| lerp_color(from, to, ((q.x - r.left()) + (q.y - r.top())) / diag);
    m.colored_vertex(r.center(), col(r.center()));
    for q in &pts {
        m.colored_vertex(*q, col(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

pub fn soft_glow(p: &egui::Painter, c: Pos2, radius: f32, color: Color32) {
    p.add(radial(c, radius, color, Color32::TRANSPARENT));
}

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
    let ore = Color32::from_rgb(235, 120, 45);
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
    blocks.push((2, 3, 1, Color32::from_rgb(200, 100, 45), Color32::from_rgb(160, 75, 35), true));
    blocks.sort_by_key(|b| (b.0 + b.1, b.2, b.0));
    for (x, y, z, top, side, glow) in blocks {
        let c = pos(x, y, z);
        if glow {
            p.add(radial(c, s * 2.4, Color32::from_rgba_unmultiplied(255, 120, 40, (60.0 + 30.0 * (t * 3.0).sin()) as u8), Color32::TRANSPARENT));
        }
        block(p, c, s, top, side);
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Home,
    Store,
    Packs,
    Mods,
    Import,
    Settings,
}

/// Petites icônes vectorielles du menu.
pub fn icon(p: &egui::Painter, kind: Icon, c: Pos2, color: Color32) {
    let st = Stroke::new(1.8_f32, color);
    let pt = |x: f32, y: f32| c + vec2(x, y);
    match kind {
        Icon::Home => {
            p.add(Shape::line(vec![pt(-8.0, 0.0), pt(0.0, -7.0), pt(8.0, 0.0)], st));
            p.add(Shape::line(vec![pt(-6.0, -1.0), pt(-6.0, 7.0), pt(6.0, 7.0), pt(6.0, -1.0)], st));
            p.add(Shape::line(vec![pt(-2.0, 7.0), pt(-2.0, 2.0), pt(2.0, 2.0), pt(2.0, 7.0)], st));
        }
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
        Icon::Import => {
            p.add(Shape::line_segment([pt(0.0, -8.0), pt(0.0, 3.0)], st));
            p.add(Shape::line(vec![pt(-4.0, -1.0), pt(0.0, 3.0), pt(4.0, -1.0)], st));
            p.add(Shape::line(vec![pt(-8.0, 3.0), pt(-8.0, 7.0), pt(8.0, 7.0), pt(8.0, 3.0)], st));
        }
        Icon::Settings => {
            p.circle_stroke(c, 4.0, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.add(Shape::line_segment([c + d * 6.0, c + d * 8.5], st));
            }
        }
    }
}
