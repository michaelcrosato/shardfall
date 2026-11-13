//! The splash: the emblem and the name over black for a moment when the game opens, then the
//! pavilion's plaza shows through as it fades (any key, click or tap ends it sooner). There is
//! no menu after it: the game and the engine's tools are places on the plaza (`crate::plaza`).

use egui::{Align2, Color32, FontId, Pos2, Stroke, Vec2};
use pav_core::t;

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);
const DIM: Color32 = Color32::from_rgb(170, 164, 152);
/// Seconds: fade in, hold, fade out to the plaza.
const IN: f32 = 0.6;
const HOLD: f32 = 1.6;
const OUT: f32 = 0.9;

/// The splash's state.
#[derive(Clone, Debug, Default)]
pub struct Splash {
    /// Seconds since it opened.
    pub age: f32,
}

impl Splash {
    /// Any key, click or tap: straight to the fade.
    pub fn skip(&mut self) {
        self.age = self.age.max(IN + HOLD);
    }

    /// It has faded out.
    pub fn done(&self) -> bool {
        self.age >= IN + HOLD + OUT
    }

    /// Still covering the plaza enough that input should end it rather than play.
    pub fn covering(&self) -> bool {
        self.age < IN + HOLD
    }

    /// Draws it; `dt` is the frame's seconds.
    pub fn ui(&mut self, ctx: &egui::Context, dt: f32) {
        self.age += dt.clamp(0.0, 0.25);
        let screen = ctx.content_rect();
        let a = if self.age < IN + HOLD { 1.0 } else { 1.0 - ((self.age - IN - HOLD) / OUT).clamp(0.0, 1.0) };
        if a <= 0.0 {
            return;
        }
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("splash")));
        p.rect_filled(screen, 0.0, Color32::from_black_alpha((255.0 * a) as u8));
        let show = (self.age / IN).clamp(0.0, 1.0) * a;
        let h = screen.height();
        let center = screen.center() - Vec2::new(0.0, h * 0.06);
        emblem(&p, center - Vec2::new(0.0, h * 0.16), h * 0.09, show, self.age);
        wordmark(&p, center, h * 0.13, show);
        let small = (h * 0.028).clamp(13.0, 26.0);
        p.text(
            center + Vec2::new(0.0, h * 0.12),
            Align2::CENTER_CENTER,
            t!("title.engine"),
            FontId::proportional(small),
            fade(DIM, show),
        );
    }
}

fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// The game's name, wide and gold with a dark edge.
fn wordmark(p: &egui::Painter, center: Pos2, size: f32, alpha: f32) {
    let name = t!("title.name");
    let font = FontId::proportional(size.max(18.0));
    for off in [Vec2::new(0.0, size * 0.04), Vec2::new(size * 0.02, size * 0.02)] {
        p.text(center + off, Align2::CENTER_CENTER, &name, font.clone(), Color32::from_black_alpha((200.0 * alpha) as u8));
    }
    p.text(center, Align2::CENTER_CENTER, name, font, fade(GOLD, alpha));
}

/// The emblem: three crystal shards falling, each a lit and a shadowed face, turning slowly.
fn emblem(p: &egui::Painter, center: Pos2, size: f32, alpha: f32, time: f32) {
    let shards = [(-0.62, 0.18, 0.55, -0.35), (0.0, -0.1, 1.0, 0.0), (0.6, 0.22, 0.6, 0.38)];
    for (k, (dx, dy, scale, tilt)) in shards.iter().enumerate() {
        let wobble = (time * 0.8 + k as f32 * 1.7).sin() * 0.04;
        let c = center + Vec2::new(dx * size, dy * size + wobble * size);
        let s = size * scale;
        let angle = tilt + wobble;
        let rot = |v: Vec2| Vec2::new(v.x * angle.cos() - v.y * angle.sin(), v.x * angle.sin() + v.y * angle.cos());
        let top = c + rot(Vec2::new(0.0, -s * 0.62));
        let bottom = c + rot(Vec2::new(0.0, s * 0.62));
        let left = c + rot(Vec2::new(-s * 0.26, -s * 0.05));
        let right = c + rot(Vec2::new(s * 0.26, s * 0.05));
        let lit = Color32::from_rgb(150, 220, 255);
        let dark = Color32::from_rgb(70, 110, 190);
        p.add(egui::Shape::convex_polygon(vec![top, right, bottom], fade(dark, alpha), Stroke::NONE));
        p.add(egui::Shape::convex_polygon(vec![top, bottom, left], fade(lit, alpha), Stroke::NONE));
        p.add(egui::Shape::closed_line(
            vec![top, right, bottom, left],
            Stroke::new(1.5, fade(Color32::from_rgb(220, 245, 255), alpha * 0.8)),
        ));
    }
}
