//! Visite guidée (OXYDE_TOUR=1) : un curseur dessiné parcourt l'app et clique, pour le trailer.
//! Les éléments visés s'enregistrent avec `mark()` quand ils sont dessinés.

use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Shape, Stroke};

/// (instant en secondes depuis le début de la visite, élément visé, clic)
pub const SCRIPT: &[(f32, &str, bool)] = &[
    (0.0, "", false),
    (1.4, "hero_play", false),
    (3.0, "feat:fps_overlay", true),
    (5.2, "nav:store", true),
    (9.0, "seg:Modrinth (mods Java)", true),
    (13.2, "nav:import", true),
    (16.6, "nav:packs", true),
    (18.2, "pack_play", true),
    (21.0, "nav:library", true),
    (24.0, "nav:home", true),
    (26.5, "", false),
];

pub fn mark(ui: &egui::Ui, key: &str, rect: Rect) {
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("tour", key)), rect));
}

fn rect_of(ctx: &egui::Context, key: &str) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("tour", key))))
}

pub struct Tour {
    pub t0: Option<f64>,
    fired: Vec<bool>,
    ripples: Vec<(Pos2, f64)>,
}

impl Tour {
    pub fn from_env() -> Option<Tour> {
        std::env::var("OXYDE_TOUR").ok().map(|_| Tour { t0: None, fired: vec![false; SCRIPT.len()], ripples: vec![] })
    }

    fn pos(ctx: &egui::Context, key: &str) -> Pos2 {
        let r = ctx.screen_rect();
        if key.is_empty() {
            return pos2(r.right() - 220.0, r.bottom() - 160.0);
        }
        rect_of(ctx, key).map(|x| x.center()).unwrap_or(r.center())
    }

    /// Position du curseur : il reste sur l'élément, puis glisse vers le suivant pendant ~0,9 s.
    fn cursor(&self, ctx: &egui::Context, t: f32) -> Pos2 {
        let i = SCRIPT.iter().rposition(|k| k.0 <= t).unwrap_or(0);
        let a = Self::pos(ctx, SCRIPT[i].1);
        let Some(next) = SCRIPT.get(i + 1) else { return a };
        let b = Self::pos(ctx, next.1);
        let dur = (next.0 - SCRIPT[i].0).min(0.95);
        let k = ((t - (next.0 - dur)) / dur).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        a + (b - a) * k
    }

    /// Avance la visite. Renvoie les clics à exécuter maintenant (clé de l'élément).
    pub fn step(&mut self, ctx: &egui::Context, now: f64) -> Vec<&'static str> {
        let t0 = *self.t0.get_or_insert(now);
        let t = (now - t0) as f32;
        let mut clicks = vec![];
        for (i, (at, key, click)) in SCRIPT.iter().enumerate() {
            if *click && !self.fired[i] && t >= *at + 0.15 {
                self.fired[i] = true;
                clicks.push(*key);
                self.ripples.push((Self::pos(ctx, key), now));
            }
        }
        // défilement automatique du magasin pendant la visite
        let scroll = if (6.0..8.6).contains(&t) {
            Some((t - 6.0) * 95.0)
        } else if (10.2..12.8).contains(&t) {
            Some((t - 10.2) * 70.0)
        } else {
            None
        };
        ctx.data_mut(|d| match scroll {
            Some(y) => d.insert_temp(egui::Id::new("tour_scroll"), y),
            None => d.remove::<f32>(egui::Id::new("tour_scroll")),
        });
        clicks
    }

    pub fn draw(&mut self, ctx: &egui::Context, now: f64) {
        let Some(t0) = self.t0 else { return };
        let t = (now - t0) as f32;
        let p = self.cursor(ctx, t);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tour_cursor")));
        // ondes de clic
        self.ripples.retain(|(_, at)| now - at < 0.6);
        let mut pressed = false;
        for (c, at) in &self.ripples {
            let k = ((now - at) / 0.6) as f32;
            pressed |= k < 0.2;
            let a = ((1.0 - k) * 200.0) as u8;
            painter.circle_stroke(*c, 8.0 + k * 34.0, Stroke::new(3.0_f32 * (1.0 - k) + 0.5, Color32::from_rgba_unmultiplied(255, 140, 60, a)));
            painter.circle_filled(*c, 6.0 * (1.0 - k), Color32::from_rgba_unmultiplied(255, 190, 120, a));
        }
        // flèche de souris
        let s = if pressed { 0.85 } else { 1.0 } * 1.25;
        let pts: Vec<Pos2> = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.2, 19.8), (9.8, 18.6), (6.9, 12.2), (12.2, 12.2)]
            .iter()
            .map(|(x, y)| p + vec2(*x, *y) * s)
            .collect();
        let shadow: Vec<Pos2> = pts.iter().map(|q| *q + vec2(2.0, 3.0)).collect();
        painter.add(Shape::convex_polygon(shadow, Color32::from_black_alpha(90), Stroke::NONE));
        painter.add(Shape::Path(egui::epaint::PathShape::closed_line(pts.clone(), Stroke::new(1.5_f32, Color32::BLACK))));
        // remplissage (polygone non convexe : deux morceaux convexes)
        painter.add(Shape::convex_polygon(vec![pts[0], pts[1], pts[2], pts[5], pts[6]], Color32::WHITE, Stroke::NONE));
        painter.add(Shape::convex_polygon(vec![pts[2], pts[3], pts[4], pts[5]], Color32::WHITE, Stroke::NONE));
        painter.add(Shape::Path(egui::epaint::PathShape::closed_line(pts, Stroke::new(1.2_f32, Color32::from_rgb(20, 20, 24)))));
        let _ = Rect::NOTHING;
    }
}
