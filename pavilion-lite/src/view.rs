//! From the world to a picture: camera placement, entities and puppets, projectiles and
//! sparks, plus the game's own drawing (`Draw`: HUD text, bars, labels, extra shapes).

use glam::{Quat, Vec3};

use crate::entity::{Look, Shape};
use crate::puppet;
use crate::render::{self, Cam, Image, Item, Lighting, Overlay, Prim, Scene};
use crate::sim::Game;
use crate::util::Color;
use crate::world::World;

/// The HUD canvas is 360 units tall; its width follows the window (640 at 16:9). Text `size`
/// is the letter height in those units (8 = small, 16 = big, 24 = title).
pub const HUD_HEIGHT: f32 = 360.0;

enum Hud {
    Text { x: f32, y: f32, size: f32, color: Color, text: String, center: bool },
    Rect { x: f32, y: f32, w: f32, h: f32, color: Color, alpha: f32 },
}

/// What `Game::draw` can add to a frame. Colours are "#rrggbb" strings.
pub struct Draw {
    pub(crate) items: Vec<Item>,
    hud: Vec<Hud>,
    labels: Vec<(Vec3, String, Color, f32)>,
    bars: Vec<(Vec3, f32, Color)>,
    /// HUD canvas width (height is always 360).
    pub width: f32,
    pub height: f32,
}

impl Draw {
    pub fn new(aspect: f32) -> Self {
        Self {
            items: Vec::new(),
            hud: Vec::new(),
            labels: Vec::new(),
            bars: Vec::new(),
            width: HUD_HEIGHT * aspect,
            height: HUD_HEIGHT,
        }
    }
    /// Text with its top-left corner at (x, y) on the HUD canvas.
    pub fn text(&mut self, x: f32, y: f32, size: f32, color: &str, text: &str) {
        self.hud.push(Hud::Text { x, y, size, color: Color::hex(color), text: text.into(), center: false });
    }
    /// Text centred horizontally on the canvas.
    pub fn title(&mut self, y: f32, size: f32, color: &str, text: &str) {
        self.hud.push(Hud::Text { x: self.width * 0.5, y, size, color: Color::hex(color), text: text.into(), center: true });
    }
    /// A filled rectangle (alpha 0..1).
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: &str, alpha: f32) {
        self.hud.push(Hud::Rect { x, y, w, h, color: Color::hex(color), alpha });
    }
    /// A meter: dark background, `frac` (0..1) filled.
    pub fn bar(&mut self, x: f32, y: f32, w: f32, h: f32, frac: f32, color: &str) {
        self.rect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, "#101216", 0.8);
        self.rect(x, y, w * frac.clamp(0.0, 1.0), h, color, 1.0);
    }
    /// Text floating over a world point.
    pub fn label(&mut self, pos: Vec3, text: &str, color: &str) {
        self.labels.push((pos, text.into(), Color::hex(color), 8.0));
    }
    /// A small health bar floating over a world point.
    pub fn health(&mut self, pos: Vec3, frac: f32, color: &str) {
        self.bars.push((pos, frac.clamp(0.0, 1.0), Color::hex(color)));
    }
    pub fn sphere(&mut self, center: Vec3, radius: f32, color: &str, look: Look) {
        self.shape(Prim::Cone { a: center, b: center, ra: radius, rb: radius }, color, look);
    }
    pub fn cube(&mut self, center: Vec3, size: Vec3, color: &str, look: Look) {
        self.shape(Prim::Box { center, rot: Quat::IDENTITY, half: size * 0.5 }, color, look);
    }
    /// A rod from `a` to `b` (aim lines, beams, sticks).
    pub fn line(&mut self, a: Vec3, b: Vec3, radius: f32, color: &str, look: Look) {
        self.shape(Prim::Cone { a, b, ra: radius, rb: radius }, color, look);
    }
    /// A flat glowing ring on the ground (area warnings, selection circles).
    pub fn ring(&mut self, center: Vec3, radius: f32, color: &str) {
        let n = (radius * 6.0).clamp(12.0, 48.0) as usize;
        let at = |k: usize| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            center + Vec3::new(a.cos() * radius, 0.03, a.sin() * radius)
        };
        for k in 0..n {
            self.line(at(k), at(k + 1), 0.05, color, Look::Glow);
        }
    }
    pub fn shape(&mut self, prim: Prim, color: &str, look: Look) {
        self.items.push(Item { prim, color: Color::hex(color), look, shadow: look != Look::Glow });
    }
}

/// Camera smoothing for the window (captures snap straight to the target).
#[derive(Clone, Debug, Default)]
pub struct Follow {
    pub target: Vec3,
    init: bool,
}

impl Follow {
    pub fn update(&mut self, w: &World, dt: f32) {
        let goal = focus(w);
        let lag = w.camera.lag;
        if !self.init || lag <= 0.0 || self.target.distance(goal) > 30.0 {
            self.target = goal;
            self.init = true;
        } else {
            self.target += (goal - self.target) * (1.0 - (-dt / lag.max(1e-3)).exp());
        }
    }
}

/// What the camera looks at: the followed entity (default the player) or `camera.target`.
pub fn focus(w: &World) -> Vec3 {
    let id = w.camera.follow.or(w.player);
    id.and_then(|i| w.get(i)).map(|e| e.pos).unwrap_or(w.camera.target)
}

/// The renderer camera for a world and a focus point.
pub fn camera(w: &World, target: Vec3) -> Cam {
    let c = &w.camera;
    let (yaw, tilt) = (c.yaw.to_radians(), c.tilt.clamp(0.0, 90.0).to_radians());
    let fh = Vec3::new(yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let fwd = (fh * tilt.cos() + Vec3::NEG_Y * tilt.sin()).normalize();
    let up = (fh * tilt.sin() + Vec3::Y * tilt.cos()).normalize();
    let mut look = target + Vec3::Y * c.height;
    if c.shake > 0.0 {
        let t = w.time();
        look += (right * (t * 53.0).sin() + up * (t * 41.0).cos()) * c.shake * 0.25;
    }
    let fov = c.fov.clamp(1.0, 170.0).to_radians();
    let half = if c.ortho { c.distance * (fov * 0.5).tan() } else { (fov * 0.5).tan() };
    Cam {
        eye: look - fwd * c.distance,
        fwd,
        up,
        right,
        half,
        ortho: c.ortho,
        near: if c.ortho { 0.05 } else { 0.1 },
        far: c.distance * 4.0 + 300.0,
    }
}

/// Every visible entity, shot and spark as render items.
pub fn world_items(w: &World, out: &mut Vec<Item>) {
    let white = Color::WHITE;
    for e in w.entities.values() {
        if !e.visible {
            continue;
        }
        let flash = e.flash > 0.0;
        if let (Some(p), Some(ch)) = (&e.puppet, &e.character) {
            for part in puppet::pose(p, &ch.anim, e.pos) {
                let color = if flash { white } else { part.color };
                out.push(Item {
                    prim: Prim::Cone { a: part.a, b: part.b, ra: part.ra, rb: part.rb },
                    color,
                    look: p.look,
                    shadow: true,
                });
            }
            continue;
        }
        let center = e.center();
        let prim = match (e.shape, &e.character) {
            (_, Some(ch)) => {
                let hh = ch.half_height();
                Prim::Cone {
                    a: e.pos + Vec3::Y * ch.radius,
                    b: e.pos + Vec3::Y * (ch.radius + hh * 2.0),
                    ra: ch.radius,
                    rb: ch.radius,
                }
            }
            (Shape::Box { half }, _) => Prim::Box { center, rot: e.rot, half },
            (Shape::Sphere { radius }, _) => Prim::Cone { a: center, b: center, ra: radius, rb: radius },
            (Shape::Capsule { half_height, radius }, _) => {
                let d = e.rot * Vec3::Y * half_height;
                Prim::Cone { a: center - d, b: center + d, ra: radius, rb: radius }
            }
            (Shape::Cylinder { half_height, radius }, _) => Prim::Cylinder { center, rot: e.rot, half_height, radius },
        };
        out.push(Item { prim, color: if flash { white } else { e.color }, look: e.look, shadow: true });
    }
    for s in &w.shots {
        out.push(Item {
            prim: Prim::Cone { a: s.pos, b: s.pos, ra: s.radius, rb: s.radius },
            color: s.color,
            look: Look::Glow,
            shadow: false,
        });
    }
    for p in &w.particles {
        let r = p.size * (p.life / p.max_life.max(1e-3)).sqrt();
        out.push(Item { prim: Prim::Cone { a: p.pos, b: p.pos, ra: r, rb: r }, color: p.color, look: Look::Glow, shadow: false });
    }
}

/// Builds the scene: world, game drawing and `extra` (tool marks) on top.
pub fn scene(w: &World, game: &dyn Game, target: Vec3, width: usize, height: usize, extra: Option<Draw>) -> Scene {
    let aspect = width as f32 / height.max(1) as f32;
    let cam = camera(w, target);
    let mut items = Vec::with_capacity(w.entities.len() * 2);
    world_items(w, &mut items);
    let mut d = Draw::new(aspect);
    game.draw(w, &mut d);
    let mut overlay = Vec::new();
    let k = height as f32 / HUD_HEIGHT;
    for dr in [Some(d), extra].into_iter().flatten() {
        items.extend(dr.items);
        for (pos, frac, color) in dr.bars {
            if let Some((x, y)) = cam.project(pos, width, height) {
                let (bw, bh) = ((24.0 * k).round() as i32, (3.0 * k).max(2.0).round() as i32);
                let (x, y) = (x as i32 - bw / 2, y as i32);
                overlay.push(Overlay::Rect {
                    x: x - 1,
                    y: y - 1,
                    w: bw + 2,
                    h: bh + 2,
                    color: Color::hex("#101216"),
                    alpha: 0.8,
                });
                overlay.push(Overlay::Rect { x, y, w: (bw as f32 * frac) as i32, h: bh, color, alpha: 1.0 });
            }
        }
        for (pos, text, color, size) in dr.labels {
            if let Some((x, y)) = cam.project(pos, width, height) {
                let scale = (size / 8.0 * k).max(1.0);
                let tw = crate::font::width(&text, scale);
                overlay.push(Overlay::Text { x: x as i32 - tw / 2, y: (y - 4.0 * scale) as i32, scale, color, text });
            }
        }
        for h in dr.hud {
            match h {
                Hud::Text { x, y, size, color, text, center } => {
                    let scale = (size / 8.0 * k).max(0.75);
                    let mut px = (x * k) as i32;
                    if center {
                        px -= crate::font::width(&text, scale) / 2;
                    }
                    overlay.push(Overlay::Text { x: px, y: (y * k) as i32, scale, color, text });
                }
                Hud::Rect { x, y, w: rw, h: rh, color, alpha } => {
                    overlay.push(Overlay::Rect {
                        x: (x * k) as i32,
                        y: (y * k) as i32,
                        w: (rw * k).ceil() as i32,
                        h: (rh * k).ceil() as i32,
                        color,
                        alpha,
                    });
                }
            }
        }
    }
    let env = &w.env;
    let shadow_radius = if cam.ortho { cam.half * aspect * 1.15 } else { (w.camera.distance * 1.1).clamp(12.0, 70.0) };
    let light = Lighting {
        sky: env.sky,
        horizon: env.horizon,
        sun_dir: env.sun_dir(),
        sun: env.sun_color.scale(env.sun),
        ambient: env.ambient,
        fog: env.fog,
        shadows: env.shadows,
        outlines: env.outlines,
        shadow_center: target,
        shadow_radius,
    };
    Scene { cam, light, items, overlay }
}

/// Renders the world as the game camera sees it, snapped to its target.
pub fn snapshot(w: &World, game: &dyn Game, width: usize, height: usize, ssaa: usize, extra: Option<Draw>) -> Image {
    let s = scene(w, game, focus(w), width, height, extra);
    render::render(&s, width, height, ssaa)
}
