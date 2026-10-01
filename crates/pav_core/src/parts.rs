//! Attachments: horns, antlers, spikes, crests, tusks, mandibles, shell plates, extra eyes,
//! floating orbs and wings that go on *any* body plan. Each body builder reports a few anchor
//! points (head, back line, shoulders); the parts are drawn from those, so a spider can grow
//! antlers and a blob can have wings. This is the "Spore" layer of the monster genome.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::color::Color;

crate::choice_enum! {
    /// Kinds of attachment.
    #[derive(Default)]
    pub enum AttachKind {
        #[default]
        Horns => "horns",
        Antlers => "antlers",
        Spikes => "spikes",
        Crest => "crest",
        Tusks => "tusks",
        Mandibles => "mandibles",
        Plates => "plates",
        Eyes => "eyes",
        Orbs => "orbs",
        Wings => "wings",
    }
}

impl AttachKind {
    pub const ALL: [AttachKind; 10] = [
        AttachKind::Horns,
        AttachKind::Antlers,
        AttachKind::Spikes,
        AttachKind::Crest,
        AttachKind::Tusks,
        AttachKind::Mandibles,
        AttachKind::Plates,
        AttachKind::Eyes,
        AttachKind::Orbs,
        AttachKind::Wings,
    ];
}

/// One attachment on a puppet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Attach {
    pub kind: AttachKind,
    /// Size multiplier (1 = normal).
    pub size: f32,
    /// How many (spikes, plates, eyes, orbs; 0 = the kind's usual number).
    pub count: u32,
    /// Colour ("" = the puppet's accent colour).
    pub color: String,
    /// Shine (orbs and eyes glow by default).
    pub glow: f32,
}

impl Default for Attach {
    fn default() -> Self {
        Self { kind: AttachKind::Horns, size: 1.0, count: 0, color: String::new(), glow: 0.0 }
    }
}

/// Where parts attach, in the space the body builder works in.
#[derive(Clone, Debug)]
pub struct Anchors {
    pub head: Vec3,
    pub head_r: f32,
    /// The head's forward, the body's up and right.
    pub fwd: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// Points along the back (front to rear), the back's thickness, and which way is "out of
    /// the back" (up for creatures, backward for bipeds).
    pub back: Vec<Vec3>,
    pub back_r: f32,
    pub back_out: Vec3,
    pub shoulders: [Vec3; 2],
    pub center: Vec3,
    /// Overall size (puppet scale).
    pub k: f32,
}

/// A point `t` (0..1) along a polyline.
fn along(pts: &[Vec3], t: f32) -> Vec3 {
    match pts.len() {
        0 => Vec3::ZERO,
        1 => pts[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            pts[i].lerp(pts[i + 1], x - i as f32)
        }
    }
}

/// Draws the attachments. `push(a, b, ra, rb, colour, glow)` adds one capsule.
pub fn attach(parts: &[Attach], accent: Color, a: &Anchors, time: f32, push: &mut dyn FnMut(Vec3, Vec3, f32, f32, Color, f32)) {
    let (f, u, r) = (a.fwd, a.up, a.right);
    let hr = a.head_r.max(0.06 * a.k);
    for p in parts {
        let c = Color::try_hex(&p.color).unwrap_or(accent);
        let s = p.size.max(0.1);
        let g = p.glow;
        match p.kind {
            AttachKind::Horns => {
                for sd in [-1.0f32, 1.0] {
                    let base = a.head + u * hr * 0.6 + r * sd * hr * 0.55;
                    let mid = base + (u * 0.75 + r * sd * 0.35 - f * 0.2) * hr * s;
                    let tip = mid + (u * 0.35 - f * 0.55 + r * sd * 0.1) * hr * s;
                    push(base, mid, hr * 0.2 * s.sqrt(), hr * 0.13 * s.sqrt(), c, g);
                    push(mid, tip, hr * 0.13 * s.sqrt(), hr * 0.03, c, g);
                }
            }
            AttachKind::Antlers => {
                for sd in [-1.0f32, 1.0] {
                    let base = a.head + u * hr * 0.75 + r * sd * hr * 0.4;
                    let mid = base + (u * 0.8 + r * sd * 0.6) * hr * s;
                    let tip = mid + (u * 0.9 + r * sd * 0.3 - f * 0.2) * hr * s;
                    let w = hr * 0.06 * s.sqrt();
                    push(base, mid, w * 1.2, w, c, g);
                    push(mid, tip, w, w * 0.4, c, g);
                    // Tines off the beam.
                    for t in [0.55f32, 1.0] {
                        let at = if t < 1.0 { base.lerp(mid, t) } else { mid };
                        push(at, at + (f * 0.45 + u * 0.7 + r * sd * 0.1) * hr * s * 0.55, w * 0.8, w * 0.25, c, g);
                    }
                }
            }
            AttachKind::Spikes => {
                let n = if p.count > 0 { p.count } else { 5 };
                for i in 0..n {
                    let t = (i as f32 + 0.5) / n as f32;
                    let at = along(&a.back, t) + a.back_out * a.back_r * 0.75;
                    let len = a.back_r * (0.55 + 0.6 * (t * std::f32::consts::PI).sin()) * s;
                    let tip = at + (a.back_out * 0.85 - f * 0.35).normalize() * len;
                    push(at, tip, a.back_r * 0.16 * s.sqrt(), a.back_r * 0.02, c, g);
                }
            }
            AttachKind::Crest => {
                let n = if p.count > 0 { p.count } else { 6 };
                let start = a.head + u * hr * 0.8;
                for i in 0..n {
                    let t = i as f32 / (n - 1).max(1) as f32;
                    let at = start.lerp(along(&a.back, 0.4) + a.back_out * a.back_r * 0.8, t);
                    let h = hr * (0.9 - 0.5 * t) * s;
                    push(at, at + (u * 0.9 - f * 0.4).normalize() * h, hr * 0.08, hr * 0.03, c, g);
                }
            }
            AttachKind::Tusks => {
                for sd in [-1.0f32, 1.0] {
                    let base = a.head + f * hr * 0.7 - u * hr * 0.35 + r * sd * hr * 0.35;
                    let mid = base + (f * 0.6 - u * 0.1 + r * sd * 0.2) * hr * s;
                    let tip = mid + (f * 0.25 + u * 0.55) * hr * s;
                    push(base, mid, hr * 0.13 * s.sqrt(), hr * 0.1, c, g);
                    push(mid, tip, hr * 0.1, hr * 0.02, c, g);
                }
            }
            AttachKind::Mandibles => {
                // Pincers that snap open and shut.
                let open = 0.35 + 0.25 * (time * 5.0).sin().abs();
                for sd in [-1.0f32, 1.0] {
                    let base = a.head + f * hr * 0.75 + r * sd * hr * 0.45 - u * hr * 0.2;
                    let mid = base + (f * 0.6 + r * sd * open) * hr * s;
                    let tip = mid + (f * 0.45 - r * sd * 0.5) * hr * s;
                    push(base, mid, hr * 0.12, hr * 0.09, c, g);
                    push(mid, tip, hr * 0.09, hr * 0.02, c, g);
                }
            }
            AttachKind::Plates => {
                let n = if p.count > 0 { p.count } else { 4 };
                for i in 0..n {
                    let t = (i as f32 + 0.5) / n as f32;
                    let at = along(&a.back, t) + a.back_out * a.back_r * 0.62;
                    let w = a.back_r * 0.6 * s;
                    push(at - r * w * 0.6, at + r * w * 0.6, w * 0.55, w * 0.55, c, g);
                }
            }
            AttachKind::Eyes => {
                let n = if p.count > 0 { p.count } else { 4 };
                for i in 0..n {
                    let ang = (i as f32 / n as f32 - 0.5) * 2.2;
                    let dir = (f * 0.8 + u * (0.45 + 0.15 * (i % 2) as f32) + r * ang * 0.5).normalize();
                    let at = a.head + dir * hr * 0.95;
                    let er = hr * 0.14 * s;
                    push(at, at, er, er, c, g.max(1.2));
                }
            }
            AttachKind::Orbs => {
                let n = if p.count > 0 { p.count } else { 3 };
                for i in 0..n {
                    let ang = time * 1.6 + i as f32 * std::f32::consts::TAU / n as f32;
                    let ring = a.back_r * 2.2 + 0.25 * a.k;
                    let at = a.center
                        + u * (a.back_r * 1.8 + 0.08 * (time * 3.0 + i as f32).sin() * a.k)
                        + (r * ang.cos() + f * ang.sin()) * ring;
                    let or = 0.09 * a.k * s;
                    push(at, at, or, or, c, g.max(2.0));
                }
            }
            AttachKind::Wings => {
                // A fan of four bones with membrane strips stretched between them, flapping.
                let flap = (time * 6.0).sin() * 0.4;
                let k = a.k;
                for sd in [-1.0f32, 1.0] {
                    let root = a.shoulders[if sd < 0.0 { 0 } else { 1 }];
                    let out = (r * sd * (0.9 + flap * 0.3) + u * (0.55 + flap)).normalize();
                    let tips: Vec<Vec3> = (0..4)
                        .map(|i| {
                            let back = i as f32 * 0.32;
                            let dir = (out - f * back - u * 0.18 * i as f32).normalize();
                            dir * (0.62 - 0.08 * i as f32) * k * s
                        })
                        .collect();
                    for t in &tips {
                        push(root, root + *t, 0.024 * k, 0.01 * k, c, g);
                    }
                    let skin = c.scale(0.7);
                    for w in tips.windows(2) {
                        push(root + w[0], root + w[1], 0.012 * k, 0.012 * k, c, g);
                        for t in [0.3f32, 0.55, 0.8] {
                            push(root + w[0] * t, root + w[1] * t, 0.03 * k * t, 0.03 * k * t, skin, g * 0.5);
                        }
                    }
                }
            }
        }
    }
}
