//! Shardfall HUD: life and mana orbs, the skill bar with cooldown sweeps, experience, floating
//! damage numbers and monster health bars (projected from the world), banners, the death screen
//! and the difficulty sliders for the pause menu.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Shape, Stroke, Vec2 as EVec2};
use glam::{Mat4, Vec3};
use pav_core::arpg::combat::{FloatKind, Rarity, Team};
use pav_core::arpg::data::{Behavior, Element, data};
use pav_core::arpg::mechanics::{LevelView, MarkKind};
use pav_core::arpg::{Difficulty, GameFrame, HeroHud};

use crate::input::Device;

/// World -> screen (egui points).
pub struct Projector {
    pub vp: Mat4,
    pub size: EVec2,
}

impl Projector {
    pub fn to_screen(&self, p: Vec3) -> Option<Pos2> {
        let c = self.vp * p.extend(1.0);
        if c.w <= 0.01 {
            return None;
        }
        let n = c.truncate() / c.w;
        if n.x.abs() > 1.2 || n.y.abs() > 1.2 {
            return None;
        }
        Some(Pos2::new((n.x * 0.5 + 0.5) * self.size.x, (0.5 - n.y * 0.5) * self.size.y))
    }
}

fn rgb(c: [f32; 3], a: f32) -> Color32 {
    let to = |x: f32| (x.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(to(c[0]), to(c[1]), to(c[2]), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

fn element_color(e: u8, a: f32) -> Color32 {
    rgb(Element::ALL[(e as usize).min(4)].color(), a)
}

fn short(v: f32) -> String {
    if v >= 1.0e6 {
        format!("{:.1}M", v / 1.0e6)
    } else if v >= 1.0e4 {
        format!("{:.0}k", v / 1.0e3)
    } else {
        format!("{:.0}", v.max(1.0))
    }
}

/// A liquid orb: dark glass, filled from the bottom.
fn orb(p: &egui::Painter, c: Pos2, r: f32, frac: f32, fill: Color32, label: &str) {
    p.circle_filled(c, r + 3.0, Color32::from_rgb(28, 24, 22));
    p.circle_filled(c, r, Color32::from_rgb(18, 16, 20));
    let frac = frac.clamp(0.0, 1.0);
    if frac > 0.0 {
        let yline = c.y + r - 2.0 * r * frac;
        let s = ((yline - c.y) / r).clamp(-1.0, 1.0);
        let (a0, a1) = (s.asin(), std::f32::consts::PI - s.asin());
        let n = 40;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect();
        p.add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    }
    // Glass highlight.
    p.circle_filled(c + EVec2::new(-r * 0.35, -r * 0.4), r * 0.22, Color32::from_white_alpha(28));
    p.circle_stroke(c, r, Stroke::new(2.0, Color32::from_rgb(120, 98, 70)));
    p.text(
        c + EVec2::new(0.0, r * 0.15),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        Color32::from_white_alpha(230),
    );
}

/// A tiny vector icon for a skill.
fn skill_icon(p: &egui::Painter, rect: Rect, key: &str, color: Color32) {
    let d = data();
    let Some(id) = d.skill_id(key) else { return };
    let s = d.skill(id);
    let c = rect.center();
    let r = rect.width() * 0.32;
    let st = Stroke::new(3.0, color);
    match s.behavior {
        Behavior::Melee if s.knockback >= 2.5 => {
            // Heavy arc.
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = -2.4 + 1.8 * i as f32 / 12.0;
                    c + EVec2::new(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(4.0, color)));
            p.line_segment([c, c + EVec2::new(r * 0.6, r * 0.6)], st);
        }
        Behavior::Melee => {
            p.line_segment([c + EVec2::new(-r, r), c + EVec2::new(r, -r)], st);
            p.line_segment([c + EVec2::new(-r * 0.5, r * 0.1), c + EVec2::new(-r * 0.1, r * 0.5)], st);
        }
        Behavior::Leap | Behavior::Slam => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    c + EVec2::new(-r + 2.0 * r * t, r * 0.6 - (t * std::f32::consts::PI).sin() * r * 1.3)
                })
                .collect();
            p.add(Shape::line(pts, st));
            p.circle_stroke(c + EVec2::new(r, r * 0.6), r * 0.3, st);
        }
        Behavior::Dash | Behavior::Charge => {
            for k in 0..2 {
                let x = -r * 0.6 + k as f32 * r * 0.8;
                p.line_segment([c + EVec2::new(x, -r * 0.7), c + EVec2::new(x + r * 0.6, 0.0)], st);
                p.line_segment([c + EVec2::new(x + r * 0.6, 0.0), c + EVec2::new(x, r * 0.7)], st);
            }
        }
        Behavior::Projectile => {
            p.circle_filled(c + EVec2::new(r * 0.35, -r * 0.35), r * 0.45, color);
            p.line_segment([c + EVec2::new(-r, r), c + EVec2::new(0.0, 0.0)], Stroke::new(2.0, color));
        }
        Behavior::Nova => {
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let v = EVec2::new(a.cos(), a.sin()) * r;
                p.line_segment([c - v, c + v], st);
            }
        }
        Behavior::Channel => {
            // A spinning arc with an arrowhead.
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = 0.3 + 5.2 * i as f32 / 20.0;
                    c + EVec2::new(a.cos(), a.sin()) * r * (0.55 + 0.45 * i as f32 / 20.0)
                })
                .collect();
            let tip = *pts.last().unwrap();
            p.add(Shape::line(pts, st));
            p.line_segment([tip, tip + EVec2::new(-r * 0.4, -r * 0.1)], st);
            p.line_segment([tip, tip + EVec2::new(-r * 0.05, r * 0.4)], st);
        }
        Behavior::Wave => {
            let pts: Vec<Pos2> = (0..=6)
                .map(|i| c + EVec2::new(-r + 2.0 * r * i as f32 / 6.0, if i % 2 == 0 { r * 0.4 } else { -r * 0.4 }))
                .collect();
            p.add(Shape::line(pts, st));
        }
        Behavior::Buff => {
            for k in 0..3 {
                let rr = r * (0.35 + 0.3 * k as f32);
                let pts: Vec<Pos2> = (0..=10)
                    .map(|i| {
                        let a = -0.8 + 1.6 * i as f32 / 10.0;
                        c + EVec2::new(-r * 0.5 + a.cos() * rr, a.sin() * rr)
                    })
                    .collect();
                p.add(Shape::line(pts, Stroke::new(2.5, color)));
            }
        }
        Behavior::Meteor => {
            p.circle_filled(c + EVec2::new(r * 0.35, r * 0.35), r * 0.45, color);
            for k in 0..3 {
                let o = EVec2::new(k as f32 * 0.25 - 0.25, -(k as f32) * 0.25 + 0.25) * r;
                p.line_segment(
                    [c + o + EVec2::new(r * 0.1, r * 0.1), c + o + EVec2::new(-r * 0.8, -r * 0.8)],
                    Stroke::new(2.0, color),
                );
            }
        }
        Behavior::Field => {
            p.line_segment([c + EVec2::new(-r, -r * 0.5), c + EVec2::new(r, -r * 0.5)], st);
            for k in 0..4 {
                let x = -r * 0.75 + k as f32 * r * 0.5;
                p.circle_filled(c + EVec2::new(x, r * (0.1 + 0.35 * (k % 2) as f32)), 2.5, color);
            }
        }
        Behavior::Blink => {
            p.circle_stroke(c + EVec2::new(-r * 0.6, r * 0.4), r * 0.3, Stroke::new(2.0, color));
            p.circle_filled(c + EVec2::new(r * 0.6, -r * 0.4), r * 0.3, color);
            for k in 0..3 {
                let t0 = 0.25 + k as f32 * 0.2;
                let a = c + EVec2::new(-r * 0.6, r * 0.4) + EVec2::new(r * 1.2, -r * 0.8) * t0;
                let b = c + EVec2::new(-r * 0.6, r * 0.4) + EVec2::new(r * 1.2, -r * 0.8) * (t0 + 0.1);
                p.line_segment([a, b], Stroke::new(2.0, color));
            }
        }
        Behavior::Rain => {
            for k in 0..5 {
                let x = -r + k as f32 * r * 0.5;
                let y = if k % 2 == 0 { -r * 0.3 } else { r * 0.1 };
                p.line_segment(
                    [c + EVec2::new(x, y - r * 0.4), c + EVec2::new(x - r * 0.15, y + r * 0.2)],
                    Stroke::new(2.0, color),
                );
            }
        }
    }
}

const SLOT_KEYS: [&str; 6] = ["LMB", "RMB", "Q", "E", "R", "F"];
const SLOT_PAD: [&str; 6] = ["X", "Y", "B", "RB", "LB", "RT"];

fn skill_bar(p: &egui::Painter, h: &HeroHud, center_bottom: Pos2, device: Device) {
    let size = 50.0;
    let gap = 6.0;
    let n = h.slots.len() as f32;
    let width = n * size + (n - 1.0) * gap;
    let x0 = center_bottom.x - width * 0.5;
    let y0 = center_bottom.y - size;
    for (i, s) in h.slots.iter().enumerate() {
        let rect = Rect::from_min_size(Pos2::new(x0 + i as f32 * (size + gap), y0), EVec2::splat(size));
        p.rect_filled(rect.expand(2.0), 6.0, Color32::from_rgb(30, 26, 24));
        p.rect_filled(rect, 5.0, Color32::from_rgb(44, 40, 46));
        let col = if s.spell { element_color(s.element, 1.0) } else { Color32::from_rgb(230, 222, 205) };
        skill_icon(p, rect, &s.key, if s.affordable { col } else { Color32::from_rgb(110, 60, 60) });
        if s.cooldown > 0.0 && s.cooldown_max > 0.0 {
            // Cooldown: a dark sweep and the seconds left.
            let f = (s.cooldown / s.cooldown_max).clamp(0.0, 1.0);
            let c = rect.center();
            let r = size * 0.72;
            let n = 32;
            let mut pts = vec![c];
            for k in 0..=n {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * f * k as f32 / n as f32;
                pts.push(c + EVec2::new(a.cos(), a.sin()) * r);
            }
            let clip = p.with_clip_rect(rect);
            for w in pts.windows(2).skip(1) {
                clip.add(Shape::convex_polygon(vec![c, w[0], w[1]], Color32::from_black_alpha(160), Stroke::NONE));
            }
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                format!("{:.1}", s.cooldown),
                FontId::proportional(15.0),
                Color32::WHITE,
            );
        }
        if !s.affordable {
            p.rect_filled(rect, 5.0, Color32::from_rgba_unmultiplied(60, 0, 0, 90));
        }
        let key = if device == Device::Gamepad { SLOT_PAD[i] } else { SLOT_KEYS[i] };
        p.text(
            rect.left_top() + EVec2::new(4.0, 2.0),
            Align2::LEFT_TOP,
            key,
            FontId::proportional(10.0),
            Color32::from_white_alpha(200),
        );
        p.rect_stroke(rect, 5.0, Stroke::new(1.0, Color32::from_rgb(120, 98, 70)), egui::StrokeKind::Outside);
    }
}

/// The whole in-game HUD.
pub fn hud(ctx: &egui::Context, g: &GameFrame, proj: &Projector, device: Device, big_map: bool) {
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("arpg_hud")));
    let screen = ctx.content_rect();
    if let Some(l) = &g.level {
        level_hud(&p, g, l, proj, screen, big_map);
    }
    // The boss bar.
    if let Some(b) = &g.boss {
        let w = 640.0f32.min(screen.width() * 0.6);
        let r = Rect::from_center_size(screen.center_top() + EVec2::new(0.0, 64.0), EVec2::new(w, 14.0));
        p.rect_filled(r.expand(3.0), 4.0, Color32::from_black_alpha(220));
        let mut fill = r;
        fill.set_width(w * b.life);
        p.rect_filled(fill, 3.0, Color32::from_rgb(190, 60, 20));
        for m in &b.marks {
            let x = r.left() + w * m;
            p.line_segment(
                [Pos2::new(x, r.top() - 2.0), Pos2::new(x, r.bottom() + 2.0)],
                Stroke::new(2.0, Color32::from_rgb(255, 210, 120)),
            );
        }
        p.rect_stroke(r, 3.0, Stroke::new(1.5, Color32::from_rgb(200, 150, 80)), egui::StrokeKind::Outside);
        p.text(
            r.center_top() - EVec2::new(0.0, 6.0),
            Align2::CENTER_BOTTOM,
            &b.name,
            FontId::proportional(20.0),
            Color32::from_rgb(255, 160, 70),
        );
        if !b.title.is_empty() {
            p.text(
                r.center_bottom() + EVec2::new(0.0, 5.0),
                Align2::CENTER_TOP,
                &b.title,
                FontId::proportional(12.0),
                Color32::from_white_alpha(170),
            );
        }
    }
    // Monster health bars and names.
    for a in &g.actors {
        if a.dead || a.team != Team::Monster || (!a.aggro && a.life >= 1.0) || (a.rarity == Rarity::Unique && g.boss.is_some()) {
            continue;
        }
        let Some(s) = proj.to_screen(a.feet + Vec3::Y * (a.height + 0.45)) else { continue };
        let w = match a.rarity {
            Rarity::Normal => 44.0,
            Rarity::Magic => 60.0,
            Rarity::Rare => 84.0,
            Rarity::Unique => 120.0,
        };
        let r = Rect::from_center_size(s, EVec2::new(w, 6.0));
        p.rect_filled(r.expand(1.5), 2.0, Color32::from_black_alpha(190));
        let mut fill = r;
        fill.set_width(w * a.life);
        p.rect_filled(fill, 1.5, Color32::from_rgb(200, 40, 34));
        if a.rarity >= Rarity::Magic {
            let mut name = a.name.clone();
            if !a.affixes.is_empty() {
                name = format!("{} ({})", name, a.affixes.join(", "));
            }
            p.text(
                r.center_top() - EVec2::new(0.0, 3.0),
                Align2::CENTER_BOTTOM,
                name,
                FontId::proportional(12.0),
                rgb(a.rarity.color(), 1.0),
            );
        }
        let ail = [[0.8, 0.1, 0.1], [1.0, 0.5, 0.1], [0.5, 0.8, 1.0], [0.8, 0.95, 1.0], [1.0, 0.95, 0.3], [0.5, 0.9, 0.2]];
        let mut x = r.left();
        for (i, on) in a.ailments.iter().enumerate() {
            if *on {
                p.circle_filled(Pos2::new(x + 3.0, r.bottom() + 6.0), 3.0, rgb(ail[i], 1.0));
                x += 8.0;
            }
        }
    }
    // Floating numbers.
    for f in &g.floaters {
        let Some(s) = proj.to_screen(f.pos) else { continue };
        let fade = (1.0 - (f.age - 0.6).max(0.0) / 0.5).clamp(0.0, 1.0);
        let pop = 1.0 + (0.15 - f.age).max(0.0) * 3.0;
        let (text, color, size) = match f.kind {
            FloatKind::Damage(e) => (short(f.value), element_color(e, fade), 15.0),
            FloatKind::Crit(e) => (format!("{}!", short(f.value)), element_color(e, fade).gamma_multiply(1.2), 22.0),
            FloatKind::Heal => {
                (format!("+{}", short(f.value)), Color32::from_rgba_unmultiplied(90, 230, 110, (fade * 255.0) as u8), 17.0)
            }
            FloatKind::Gold => {
                (format!("+{} gold", short(f.value)), Color32::from_rgba_unmultiplied(255, 205, 70, (fade * 255.0) as u8), 14.0)
            }
            FloatKind::Xp => {
                (format!("+{} xp", short(f.value)), Color32::from_rgba_unmultiplied(170, 140, 255, (fade * 255.0) as u8), 13.0)
            }
            FloatKind::Text => (f.text.clone(), Color32::from_white_alpha((fade * 235.0) as u8), 15.0),
        };
        let font = FontId::proportional(size * pop);
        p.text(
            s + EVec2::new(1.5, 1.5),
            Align2::CENTER_CENTER,
            &text,
            font.clone(),
            Color32::from_black_alpha((fade * 200.0) as u8),
        );
        p.text(s, Align2::CENTER_CENTER, text, font, color);
    }
    let Some(h) = &g.hero else { return };
    // Low life and death.
    let low = 1.0 - (h.life / h.life_max.max(1.0)) / 0.3;
    if low > 0.0 || h.dead {
        let a = if h.dead { 0.45 } else { low.min(1.0) * 0.25 };
        let edge = Color32::from_rgba_unmultiplied(120, 0, 0, (a * 255.0) as u8);
        let t = 60.0;
        for r in [
            Rect::from_min_max(screen.left_top(), Pos2::new(screen.right(), screen.top() + t)),
            Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - t), screen.right_bottom()),
            Rect::from_min_max(screen.left_top(), Pos2::new(screen.left() + t, screen.bottom())),
            Rect::from_min_max(Pos2::new(screen.right() - t, screen.top()), screen.right_bottom()),
        ] {
            p.rect_filled(r, 0.0, edge);
        }
    }
    // Bottom: orbs, skills, experience.
    let bottom = screen.center_bottom() - EVec2::new(0.0, 34.0);
    let r = 52.0;
    orb(
        &p,
        bottom + EVec2::new(-250.0, -r + 6.0),
        r,
        h.life / h.life_max.max(1.0),
        Color32::from_rgb(178, 26, 30),
        &format!("{}/{}", h.life.max(0.0).round(), h.life_max.round()),
    );
    orb(
        &p,
        bottom + EVec2::new(250.0, -r + 6.0),
        r,
        h.mana / h.mana_max.max(1.0),
        Color32::from_rgb(40, 70, 190),
        &format!("{}/{}", h.mana.round(), h.mana_max.round()),
    );
    skill_bar(&p, h, bottom + EVec2::new(0.0, 0.0), device);
    // Potion charges and dodge.
    let pot = bottom + EVec2::new(-250.0 + r + 12.0, -12.0);
    for i in 0..h.potion_max {
        let c = pot + EVec2::new(i as f32 * 14.0, 0.0);
        p.circle_filled(c, 5.5, if i < h.potions { Color32::from_rgb(210, 40, 50) } else { Color32::from_rgb(55, 40, 40) });
    }
    p.text(
        pot + EVec2::new(0.0, 10.0),
        Align2::LEFT_TOP,
        if device == Device::Gamepad { "D-pad up" } else { "1 potion" },
        FontId::proportional(10.0),
        Color32::from_white_alpha(170),
    );
    if h.dodge_cd > 0.0 {
        p.text(
            bottom + EVec2::new(250.0 - r - 12.0, -12.0),
            Align2::RIGHT_CENTER,
            format!("dodge {:.1}", h.dodge_cd),
            FontId::proportional(11.0),
            Color32::from_white_alpha(180),
        );
    }
    let xp_rect = Rect::from_min_size(Pos2::new(screen.center().x - 300.0, screen.bottom() - 16.0), EVec2::new(600.0, 6.0));
    p.rect_filled(xp_rect.expand(1.0), 2.0, Color32::from_black_alpha(200));
    let mut xf = xp_rect;
    xf.set_width(600.0 * (h.xp / h.xp_next.max(1.0)).clamp(0.0, 1.0) as f32);
    p.rect_filled(xf, 2.0, Color32::from_rgb(150, 120, 255));
    // Top left: who, level, gold, the arena wave.
    let mut text = format!("{}   Level {}   {} gold   {} kills", h.name, h.level, h.gold, h.kills);
    if g.wave > 0 {
        text += &format!("   Wave {}  ({} left)", g.wave, g.monsters);
    }
    p.text(Pos2::new(14.0, 12.0), Align2::LEFT_TOP, text, FontId::proportional(15.0), Color32::from_rgb(240, 228, 200));
    // Banners.
    if let Some(m) = &g.message {
        let c = screen.center_top() + EVec2::new(0.0, 120.0);
        p.text(c + EVec2::new(2.0, 2.0), Align2::CENTER_CENTER, m, FontId::proportional(30.0), Color32::from_black_alpha(200));
        p.text(c, Align2::CENTER_CENTER, m, FontId::proportional(30.0), Color32::from_rgb(255, 225, 160));
    }
    if h.level_flash < 2.0 {
        let a = (1.0 - h.level_flash / 2.0).clamp(0.0, 1.0);
        p.text(
            screen.center() - EVec2::new(0.0, 60.0),
            Align2::CENTER_CENTER,
            "LEVEL UP",
            FontId::proportional(44.0 + 10.0 * a),
            Color32::from_rgba_unmultiplied(255, 215, 120, (a * 255.0) as u8),
        );
    }
    if h.dead {
        p.text(
            screen.center(),
            Align2::CENTER_CENTER,
            format!("You fell   {:.0}", h.respawn.ceil().max(0.0)),
            FontId::proportional(36.0),
            Color32::from_rgb(230, 80, 70),
        );
    }
}

/// The level: its card (top left), the intro banner, and the map (top right; M for a big one).
/// The map turns with the camera, shows only rooms the hero has been in, and marks shrines,
/// gates, chests, wells, totems, the portal and the way down.
fn level_hud(p: &egui::Painter, g: &GameFrame, l: &LevelView, proj: &Projector, screen: Rect, big: bool) {
    // Card.
    let mut y = 34.0;
    p.text(
        Pos2::new(14.0, y),
        Align2::LEFT_TOP,
        format!("{} · {}", l.label, l.name),
        FontId::proportional(14.0),
        if l.endless { Color32::from_rgb(205, 175, 255) } else { Color32::from_rgb(255, 214, 150) },
    );
    y += 18.0;
    for (name, hint) in &l.mechanics {
        p.text(
            Pos2::new(16.0, y),
            Align2::LEFT_TOP,
            format!("{name}: {hint}"),
            FontId::proportional(11.5),
            Color32::from_white_alpha(170),
        );
        y += 14.0;
    }
    if !l.exit_open && !l.boss_name.is_empty() {
        p.text(
            Pos2::new(16.0, y + 2.0),
            Align2::LEFT_TOP,
            format!("The way down is sealed: slay {}", l.boss_name),
            FontId::proportional(11.5),
            Color32::from_rgb(255, 120, 90),
        );
    }
    // Intro banner for the first seconds.
    if l.time < 6.0 {
        let a = ((6.0 - l.time) / 1.5).clamp(0.0, 1.0) * (l.time / 0.4).clamp(0.0, 1.0);
        let c = screen.center_top() + EVec2::new(0.0, 190.0);
        let gold = Color32::from_rgba_unmultiplied(255, 220, 160, (a * 255.0) as u8);
        let shadow = Color32::from_black_alpha((a * 200.0) as u8);
        p.text(c + EVec2::new(2.0, 2.0), Align2::CENTER_CENTER, &l.name, FontId::proportional(40.0), shadow);
        p.text(c, Align2::CENTER_CENTER, &l.name, FontId::proportional(40.0), gold);
        let sub = Color32::from_rgba_unmultiplied(235, 225, 210, (a * 230.0) as u8);
        p.text(c + EVec2::new(0.0, 34.0), Align2::CENTER_CENTER, &l.about, FontId::proportional(16.0), sub);
    }
    // The map: turned like the camera (world axes projected around the hero).
    let Some(hero) = g.actors.iter().find(|a| a.team == Team::Hero) else { return };
    let (Some(o), Some(ex), Some(ez)) =
        (proj.to_screen(hero.feet), proj.to_screen(hero.feet + Vec3::X), proj.to_screen(hero.feet + Vec3::Z))
    else {
        return;
    };
    let ax = (ex - o).normalized();
    let az = (ez - o).normalized();
    let (size, center) = if big {
        let s = (screen.height() * 0.7).min(screen.width() * 0.6);
        (s, screen.center())
    } else {
        (210.0, Pos2::new(screen.right() - 125.0, 125.0))
    };
    let area = Rect::from_center_size(center, EVec2::splat(size));
    p.rect_filled(area, 8.0, Color32::from_black_alpha(if big { 150 } else { 120 }));
    // Scale: the whole level fits the big map; the minimap shows 90 m around the hero.
    let (mut lo, mut hi) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN));
    for r in &l.rooms {
        lo = lo.min(r.rect.min);
        hi = hi.max(r.rect.max);
    }
    let (focus, span) =
        if big { ((lo + hi) * 0.5, (hi - lo).max_element() * 1.05) } else { (glam::Vec2::new(hero.feet.x, hero.feet.z), 90.0) };
    let k = size / span.max(1.0);
    let to = |x: f32, z: f32| -> Pos2 { center + (ax * (x - focus.x) + az * (z - focus.y)) * k };
    let clip = p.with_clip_rect(area.shrink(2.0));
    let quad = |r: &pav_core::arpg::levelgen::Rect, fill: Color32, stroke: Stroke| {
        let pts = vec![to(r.min.x, r.min.y), to(r.max.x, r.min.y), to(r.max.x, r.max.y), to(r.min.x, r.max.y)];
        clip.add(Shape::convex_polygon(pts, fill, stroke));
    };
    for c in &l.corridors {
        quad(c, Color32::from_rgba_unmultiplied(150, 140, 120, 150), Stroke::NONE);
    }
    for r in l.rooms.iter().filter(|r| r.seen) {
        quad(&r.rect, Color32::from_rgba_unmultiplied(120, 110, 95, 140), Stroke::new(1.2, Color32::from_rgb(210, 190, 150)));
    }
    for m in &l.marks {
        let at = to(m.pos.x, m.pos.y);
        let (c, r) = match m.kind {
            MarkKind::Portal => (Color32::from_rgb(110, 200, 255), 4.5),
            MarkKind::Exit => {
                if m.active {
                    (Color32::from_rgb(255, 210, 110), 6.0)
                } else {
                    (Color32::from_rgb(230, 60, 50), 6.0)
                }
            }
            MarkKind::Shrine => (Color32::from_rgb(120, 255, 200), 3.5),
            MarkKind::Gate => (Color32::from_rgb(180, 120, 255), 4.0),
            MarkKind::Chest => (Color32::from_rgb(170, 90, 255), 4.0),
            MarkKind::Well => (Color32::from_rgb(255, 140, 60), 3.5),
            MarkKind::Totem => (Color32::from_rgb(150, 230, 90), 3.0),
        };
        let c = if m.active { c } else { c.gamma_multiply(0.35) };
        if m.kind == MarkKind::Exit {
            clip.add(Shape::convex_polygon(
                vec![at + EVec2::new(0.0, -r), at + EVec2::new(r, 0.0), at + EVec2::new(0.0, r), at + EVec2::new(-r, 0.0)],
                c,
                Stroke::new(1.0, Color32::BLACK),
            ));
        } else {
            clip.circle(at, r, c, Stroke::new(1.0, Color32::from_black_alpha(200)));
        }
    }
    // Monsters the hero is fighting, and the hero.
    for a in g.actors.iter().filter(|a| a.team == Team::Monster && !a.dead && a.aggro) {
        let r = if a.rarity >= Rarity::Rare { 2.6 } else { 1.6 };
        clip.circle_filled(to(a.feet.x, a.feet.z), r, Color32::from_rgb(230, 70, 60));
    }
    let h = to(hero.feet.x, hero.feet.z);
    clip.circle(h, 4.0, Color32::WHITE, Stroke::new(1.5, Color32::BLACK));
    p.rect_stroke(area, 8.0, Stroke::new(1.0, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
    if !big {
        p.text(
            area.center_bottom() + EVec2::new(0.0, 4.0),
            Align2::CENTER_TOP,
            "M: map",
            FontId::proportional(10.0),
            Color32::from_white_alpha(130),
        );
    }
}

/// Difficulty sliders (pause menu).
pub fn difficulty_ui(ui: &mut egui::Ui, d: &mut Difficulty) {
    ui.label(RichText::new("Difficulty (play-testing)").strong());
    ui.horizontal(|ui| {
        for (name, k) in [("Story", 0.5f32), ("Normal", 1.0), ("Hard", 1.6), ("Brutal", 2.5), ("Nightmare", 4.0)] {
            if ui.button(name).clicked() {
                d.enemy_damage = k;
                d.enemy_life = k;
                d.enemy_speed = 1.0 + (k - 1.0) * 0.12;
                d.player_damage = 1.0;
                d.player_life = 1.0;
            }
        }
    });
    egui::Grid::new("difficulty").num_columns(2).show(ui, |ui| {
        for (label, v, max) in [
            ("Hero damage", &mut d.player_damage, 10.0),
            ("Hero life", &mut d.player_life, 10.0),
            ("Monster damage", &mut d.enemy_damage, 10.0),
            ("Monster life", &mut d.enemy_life, 10.0),
            ("Monster speed", &mut d.enemy_speed, 3.0),
            ("Hit-stop", &mut d.hitstop, 3.0),
            ("Screen shake", &mut d.shake, 3.0),
        ] {
            ui.label(label);
            ui.add(egui::Slider::new(v, 0.0..=max).step_by(0.05));
            ui.end_row();
        }
    });
}
