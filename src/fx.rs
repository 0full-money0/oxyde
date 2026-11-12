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
