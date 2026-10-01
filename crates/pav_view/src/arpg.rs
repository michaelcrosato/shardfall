//! Drawing Shardfall: projectiles with light and trails, swing arcs and shockwave rings,
//! monster wind-up telegraphs on the ground, weapon swing trails, elite auras, ailment
//! particles and screen shake. Numbers and health bars are drawn by the app's HUD.

use std::collections::HashMap;

use glam::{Vec3, Vec4};
use pav_core::arpg::combat::{EffectKind, Rarity, Team};
use pav_core::arpg::{GameFrame, TeleShape};
use pav_core::frame::RenderFrame;
use pav_core::puppet::{ActKind, PuppetDef, WeaponKind};
use pav_render::mesh::{MeshData, Vertex};
use pav_render::scene::{self as rs, ParticleBurst, Scene, Style};

/// View-side memory between frames.
#[derive(Default)]
pub struct ArpgView {
    /// Last weapon tip per swinging character (trails run from there).
    tips: HashMap<u32, (Vec3, Vec3)>,
    /// Particle emission carried between frames (ailments, projectile trails).
    carry: HashMap<u64, f32>,
}

fn v3(c: [f32; 3]) -> Vec3 {
    Vec3::from(c)
}

/// A flat band (ring sector) on the plane y = `c.y`: radii r0..r1, angles a0..a1 (radians,
/// 0 = +Z, turning toward +X).
fn band(data: &mut MeshData, c: Vec3, r0: f32, r1: f32, a0: f32, a1: f32, shade: f32) {
    let segs = (((a1 - a0).abs() / 0.12).ceil() as u32).clamp(3, 96);
    let base = data.vertices.len() as u32;
    let col = [shade, shade, shade, 1.0];
    for i in 0..=segs {
        let a = a0 + (a1 - a0) * i as f32 / segs as f32;
        let d = Vec3::new(a.sin(), 0.0, a.cos());
        for r in [r0, r1] {
            data.vertices.push(Vertex { pos: (c + d * r).to_array(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color: col });
        }
    }
    for i in 0..segs {
        let k = base + i * 2;
        data.indices.extend_from_slice(&[k, k + 1, k + 2, k + 1, k + 3, k + 2]);
    }
}

/// A flat quad strip along `dir` from `from`: length and width.
fn strip(data: &mut MeshData, from: Vec3, dir: Vec3, length: f32, width: f32, shade: f32) {
    let side = Vec3::new(-dir.z, 0.0, dir.x) * width * 0.5;
    let base = data.vertices.len() as u32;
    let col = [shade, shade, shade, 1.0];
    for p in [from - side, from + side, from + dir * length - side, from + dir * length + side] {
        data.vertices.push(Vertex { pos: p.to_array(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color: col });
    }
    data.indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
}

fn decal(scene: &mut Scene, data: MeshData, color: Vec3, emissive: f32) {
    if data.indices.is_empty() {
        return;
    }
    scene.dynamic.push(rs::DynamicMesh {
        data,
        color,
        emissive,
        style: Style::Unlit,
        flags: rs::flags::NO_SHADOW | rs::flags::TWO_SIDED | rs::flags::NO_CUT | rs::flags::NO_RECEIVE_SHADOW,
        group: 1,
    });
}

fn burst(pos: Vec3, count: u32, color0: Vec4, color1: Vec4, f: impl FnOnce(&mut ParticleBurst)) -> ParticleBurst {
    let mut b = ParticleBurst { pos, count, color0, color1, additive: true, ..Default::default() };
    b.life = (0.25, 0.5);
    b.size = (0.12, 0.0);
    b.spread = 2.0;
    f(&mut b);
    b
}

/// Smooth pseudo-noise in -1..1.
fn noise(t: f32, seed: f32) -> f32 {
    ((t * 1.0 + seed).sin() * 0.5 + (t * 2.3 + seed * 1.7).sin() * 0.3 + (t * 5.1 + seed * 0.3).sin() * 0.2).clamp(-1.0, 1.0)
}

/// Camera shake offset for this frame.
pub fn shake_offset(g: &GameFrame, time: f32) -> Vec3 {
    let s = g.shake * g.shake * 0.45;
    if s < 1e-4 {
        return Vec3::ZERO;
    }
    Vec3::new(noise(time * 38.0, 0.0), noise(time * 41.0, 1.7) * 0.5, noise(time * 35.0, 3.1)) * s
}

impl ArpgView {
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &mut self,
        scene: &mut Scene,
        curr: &RenderFrame,
        g: &GameFrame,
        alpha: f32,
        time: f32,
        dt: f32,
        particles: bool,
        cam_fwd: Vec3,
    ) {
        let back = (1.0 - alpha) * curr.dt;
        // Projectiles: a glowing ball, a light, an ember trail.
        for (i, s) in g.shots.iter().enumerate() {
            let pos = s.pos - s.vel * back;
            let c = v3(s.color);
            if s.orbit.is_some() {
                // Orbiting blades: a spectral blade along the circle.
                let d = s.vel.normalize_or(Vec3::X);
                scene.sdfs.push(rs::SdfInstance {
                    a: pos - d * 0.32,
                    b: pos + d * 0.32,
                    ra: 0.05,
                    rb: 0.11,
                    color: c,
                    emissive: 2.5,
                    style: Style::Unlit,
                    flags: rs::flags::NO_SHADOW | rs::flags::NO_CUT,
                    group: 1,
                });
                if particles {
                    scene.particles.push(burst(pos, 1, c.extend(0.6), c.extend(0.0), |b| {
                        b.size = (0.1, 0.0);
                        b.life = (0.12, 0.2);
                        b.spread = 0.2;
                    }));
                }
                continue;
            }
            scene.sdfs.push(rs::SdfInstance {
                a: pos,
                b: pos,
                ra: s.radius,
                rb: s.radius,
                color: c,
                emissive: 3.0,
                style: Style::Unlit,
                flags: rs::flags::NO_SHADOW | rs::flags::NO_CUT,
                group: 1,
            });
            if i < 24 {
                scene.point_lights.push(rs::PointLight { position: pos, color: c * 1.6, radius: 4.5, shadows: false });
            }
            if particles {
                let key = 0x5000_0000_0000 + (s.owner.0 as u64) * 64 + i as u64;
                let carry = self.carry.entry(key).or_insert(0.0);
                *carry += dt * 60.0;
                let n = carry.floor();
                *carry -= n;
                if n >= 1.0 {
                    scene.particles.push(burst(pos, n as u32, c.extend(0.9), c.extend(0.0), |b| {
                        b.size = (s.radius * 0.9, 0.0);
                        b.life = (0.15, 0.3);
                        b.spread = 0.6;
                        b.vel = -s.vel * 0.05;
                    }));
                }
            }
        }
        // Ground effects: rings, swing arcs, flashes, delayed blasts.
        for e in &g.effects {
            let p = (e.t / e.dur.max(1e-3)).clamp(0.0, 1.0);
            let c = v3(e.color);
            match e.kind {
                EffectKind::Ring if e.angle >= 359.0 => {
                    let mut m = MeshData::default();
                    let r = e.radius * (0.25 + 0.75 * p.sqrt());
                    band(&mut m, e.pos + Vec3::Y * 0.06, (r - 0.35).max(0.0), r, 0.0, std::f32::consts::TAU, 1.0);
                    decal(scene, m, c, 2.5 * (1.0 - p));
                }
                EffectKind::Ring => {
                    // A swing arc at chest height: a crescent that fades as it sweeps.
                    let half = e.angle.to_radians() * 0.5;
                    let yaw = e.dir.x.atan2(e.dir.z);
                    let mut m = MeshData::default();
                    let r1 = e.radius;
                    band(&mut m, e.pos, r1 * (0.55 + 0.3 * p), r1, yaw - half, yaw + half, 1.0);
                    decal(scene, m, c, 1.8 * (1.0 - p));
                }
                EffectKind::Burst => {
                    scene.point_lights.push(rs::PointLight {
                        position: e.pos + Vec3::Y * 0.8,
                        color: c * 4.0 * (1.0 - p),
                        radius: e.radius * 3.0,
                        shadows: false,
                    });
                }
                EffectKind::Field => {
                    // Hurting ground: a dim patch with a bright rim; fire smoulders upward,
                    // cold falls as shards from above, poison bubbles.
                    let fade = ((e.dur - e.t) / 0.4).clamp(0.0, 1.0) * (e.t / 0.1).clamp(0.0, 1.0);
                    let flick = 0.8 + 0.2 * noise(time * 9.0, e.pos.x + e.pos.z);
                    let cold = c.z > c.x * 1.1;
                    let mut m = MeshData::default();
                    band(&mut m, e.pos + Vec3::Y * 0.03, 0.0, e.radius * 0.95, 0.0, std::f32::consts::TAU, 0.35);
                    decal(scene, m, c, 0.5 * fade * flick);
                    let mut rim = MeshData::default();
                    band(&mut rim, e.pos + Vec3::Y * 0.035, e.radius * 0.88, e.radius, 0.0, std::f32::consts::TAU, 1.0);
                    decal(scene, rim, c, 1.1 * fade);
                    if particles {
                        let key = 0x6000_0000_0000 + ((e.pos.x * 31.0) as i64 as u64) * 977 + (e.pos.z * 17.0) as i64 as u64;
                        let carry = self.carry.entry(key).or_insert(0.0);
                        *carry += dt * if cold { 26.0 } else { 14.0 } * fade * (e.radius / 1.5).max(1.0);
                        let n = carry.floor();
                        *carry -= n;
                        if n >= 1.0 {
                            let (from, c0, c1) = if cold {
                                (e.pos + Vec3::Y * 4.0, Vec4::new(0.85, 0.95, 1.0, 1.0), Vec4::new(0.6, 0.85, 1.0, 0.0))
                            } else {
                                (
                                    e.pos + Vec3::Y * 0.1,
                                    Vec4::new(c.x, c.y, c.z, 1.0) * 1.2,
                                    Vec4::new(c.x * 0.8, c.y * 0.4, c.z * 0.2, 0.0),
                                )
                            };
                            scene.particles.push(burst(from, n as u32, c0, c1, |b| {
                                b.area = Vec3::new(e.radius * 0.7, 0.05, e.radius * 0.7);
                                if cold {
                                    b.vel = Vec3::new(0.8, -9.0, 0.4);
                                    b.spread = 0.1;
                                    b.stretch = true;
                                    b.size = (0.07, 0.03);
                                    b.life = (0.4, 0.45);
                                } else {
                                    b.vel = Vec3::Y * 1.8;
                                    b.spread = 0.5;
                                    b.size = (0.14, 0.0);
                                    b.life = (0.3, 0.6);
                                }
                            }));
                        }
                    }
                }
                EffectKind::Meteor => {
                    // A burning rock falls along a slant onto the target circle.
                    let mut outline = MeshData::default();
                    band(&mut outline, e.pos + Vec3::Y * 0.04, e.radius - 0.15, e.radius, 0.0, std::f32::consts::TAU, 1.0);
                    decal(scene, outline, c, 1.4);
                    let mut fill = MeshData::default();
                    band(&mut fill, e.pos + Vec3::Y * 0.035, 0.0, e.radius * p.min(1.0), 0.0, std::f32::consts::TAU, 0.5);
                    decal(scene, fill, c, 0.7);
                    if p < 1.0 {
                        let fall = (1.0 - p).powf(1.3);
                        let rock = e.pos + Vec3::new(-e.dir.x * 5.0, 15.0, -e.dir.z * 5.0) * fall + Vec3::Y * 0.4;
                        let r = 0.45 + e.radius * 0.12;
                        scene.sdfs.push(rs::SdfInstance {
                            a: rock,
                            b: rock,
                            ra: r,
                            rb: r,
                            color: Vec3::new(1.0, 0.45, 0.12),
                            emissive: 3.0,
                            style: Style::Unlit,
                            flags: rs::flags::NO_SHADOW | rs::flags::NO_CUT,
                            group: 1,
                        });
                        scene.point_lights.push(rs::PointLight { position: rock, color: c * 3.0, radius: 9.0, shadows: false });
                        if particles {
                            scene.particles.push(burst(
                                rock,
                                3,
                                Vec4::new(1.0, 0.6, 0.2, 1.0),
                                Vec4::new(0.4, 0.2, 0.1, 0.0),
                                |b| {
                                    b.size = (r * 0.8, 0.1);
                                    b.life = (0.25, 0.5);
                                    b.spread = 0.5;
                                },
                            ));
                        }
                    }
                }
                // The hero's own quick blasts (fissures, rain): nothing before, a burst after.
                EffectKind::Delayed if e.angle < 0.0 => {
                    if e.t >= e.dur {
                        let q = ((e.t - e.dur) / 0.35).clamp(0.0, 1.0);
                        let mut m = MeshData::default();
                        let r = e.radius * (0.5 + 0.6 * q.sqrt());
                        band(&mut m, e.pos + Vec3::Y * 0.05, (r - 0.3).max(0.0), r, 0.0, std::f32::consts::TAU, 1.0);
                        decal(scene, m, c, 2.0 * (1.0 - q));
                        if q < 0.5 {
                            scene.point_lights.push(rs::PointLight {
                                position: e.pos + Vec3::Y * 0.6,
                                color: c * 2.5 * (1.0 - q * 2.0),
                                radius: e.radius * 2.5,
                                shadows: false,
                            });
                        }
                    }
                }
                EffectKind::Delayed => {
                    let mut outline = MeshData::default();
                    band(&mut outline, e.pos + Vec3::Y * 0.04, e.radius - 0.12, e.radius, 0.0, std::f32::consts::TAU, 1.0);
                    decal(scene, outline, c, 1.0);
                    let mut fill = MeshData::default();
                    band(&mut fill, e.pos + Vec3::Y * 0.035, 0.0, e.radius * p.min(1.0), 0.0, std::f32::consts::TAU, 0.55);
                    decal(scene, fill, c, 0.6);
                }
            }
        }
        // Monster wind-ups: where it will land, filling up until it does.
        for t in &g.telegraphs {
            let c = v3(t.color);
            let k = t.progress;
            let lift = Vec3::Y * 0.04;
            let mut outline = MeshData::default();
            let mut fill = MeshData::default();
            match t.shape {
                TeleShape::Circle { center, radius } => {
                    band(&mut outline, center + lift, radius - 0.1, radius, 0.0, std::f32::consts::TAU, 1.0);
                    band(&mut fill, center + lift * 0.9, 0.0, radius * k, 0.0, std::f32::consts::TAU, 0.6);
                }
                TeleShape::Arc { center, dir, range, angle } => {
                    let half = angle.to_radians() * 0.5;
                    let yaw = dir.x.atan2(dir.z);
                    band(&mut outline, center + lift, range - 0.1, range, yaw - half, yaw + half, 1.0);
                    band(&mut fill, center + lift * 0.9, 0.2, 0.2 + (range - 0.2) * k, yaw - half, yaw + half, 0.6);
                }
                TeleShape::Line { from, dir, length, width } => {
                    let side = Vec3::new(-dir.z, 0.0, dir.x);
                    strip(&mut outline, from + lift - side * width * 0.5, dir, length, 0.08, 1.0);
                    strip(&mut outline, from + lift + side * width * 0.5, dir, length, 0.08, 1.0);
                    strip(&mut fill, from + lift * 0.9, dir, length * k, width, 0.6);
                }
            }
            decal(scene, outline, c, 1.2 + k);
            decal(scene, fill, c, 0.5 + k);
        }
        // Elite auras and ailment particles.
        for a in &g.actors {
            if a.dead {
                continue;
            }
            if a.team == Team::Monster && a.rarity >= Rarity::Magic {
                let c = v3(a.rarity.color());
                let pulse = 0.8 + 0.2 * (time * 4.0 + a.id.0 as f32).sin();
                let mut m = MeshData::default();
                let r = a.radius * 1.35;
                band(&mut m, a.feet + Vec3::Y * 0.05, r - 0.07, r, 0.0, std::f32::consts::TAU, 1.0);
                decal(scene, m, c, 1.5 * pulse);
                if a.rarity >= Rarity::Rare {
                    scene.point_lights.push(rs::PointLight {
                        position: a.feet + Vec3::Y * 1.2,
                        color: c * 0.8,
                        radius: 3.5,
                        shadows: false,
                    });
                }
            }
            if !particles {
                continue;
            }
            let body = a.feet + Vec3::Y * a.height * 0.5;
            let kinds: [(bool, Vec4, Vec4, f32); 4] = [
                (a.ailments[1], Vec4::new(1.0, 0.55, 0.15, 1.0), Vec4::new(1.0, 0.2, 0.05, 0.0), 30.0),
                (a.ailments[3], Vec4::new(0.85, 0.95, 1.0, 1.0), Vec4::new(0.5, 0.8, 1.0, 0.0), 10.0),
                (a.ailments[4], Vec4::new(1.0, 0.95, 0.4, 1.0), Vec4::new(1.0, 0.9, 0.2, 0.0), 14.0),
                (a.ailments[5], Vec4::new(0.55, 0.95, 0.3, 1.0), Vec4::new(0.3, 0.7, 0.1, 0.0), 10.0),
            ];
            for (i, (on, c0, c1, rate)) in kinds.into_iter().enumerate() {
                if !on {
                    continue;
                }
                let key = (a.id.0 as u64) * 8 + i as u64;
                let carry = self.carry.entry(key).or_insert(0.0);
                *carry += dt * rate;
                let n = carry.floor();
                *carry -= n;
                if n >= 1.0 {
                    scene.particles.push(burst(body, n as u32, c0, c1, |b| {
                        b.area = Vec3::new(a.radius * 0.7, a.height * 0.35, a.radius * 0.7);
                        b.vel = Vec3::Y * if i == 0 { 1.6 } else { 0.4 };
                        b.spread = if i == 2 { 2.5 } else { 0.5 };
                        b.size = (if i == 0 { 0.16 } else { 0.08 }, 0.0);
                        b.stretch = i == 2;
                        b.life = (0.3, 0.6);
                    }));
                }
            }
        }
        // Loot: the item lying there, a rarity ring, and a pillar of light for rares and uniques.
        for (i, l) in g.loot.iter().enumerate() {
            let rc = v3(l.rarity.color());
            let ic = v3(l.color);
            let spin = l.age * 7.0 * if l.rest { 0.0 } else { 1.0 } + l.id as f32;
            let (half, r) = match l.slot {
                pav_core::arpg::items::Slot::Weapon => (0.42, 0.07),
                pav_core::arpg::items::Slot::Ring | pav_core::arpg::items::Slot::Amulet => (0.05, 0.1),
                pav_core::arpg::items::Slot::Body => (0.12, 0.22),
                _ => (0.1, 0.15),
            };
            let d = Vec3::new(spin.cos(), if l.rest { 0.0 } else { spin.sin() * 0.5 }, spin.sin()).normalize();
            let pos = l.pos + Vec3::Y * r;
            scene.sdfs.push(rs::SdfInstance {
                a: pos - d * half,
                b: pos + d * half,
                ra: r,
                rb: r * 0.8,
                color: ic,
                emissive: if l.rarity >= Rarity::Rare { 0.6 } else { 0.15 },
                style: Style::Lit,
                flags: rs::flags::NO_CUT,
                group: 1,
            });
            if l.rarity >= Rarity::Magic && l.rest {
                let pulse = 0.7 + 0.3 * (time * 3.0 + l.id as f32).sin();
                let mut m = MeshData::default();
                band(&mut m, l.pos + Vec3::Y * 0.03, 0.42, 0.5, 0.0, std::f32::consts::TAU, 1.0);
                decal(scene, m, rc, 1.4 * pulse);
            }
            if l.rarity >= Rarity::Rare {
                if i < 12 {
                    scene.point_lights.push(rs::PointLight {
                        position: l.pos + Vec3::Y * 1.0,
                        color: rc * 1.4,
                        radius: 4.0,
                        shadows: false,
                    });
                }
                if particles {
                    let key = 0x7000_0000_0000 + l.id as u64;
                    let carry = self.carry.entry(key).or_insert(0.0);
                    *carry += dt * if l.rarity == Rarity::Unique { 40.0 } else { 22.0 };
                    let n = carry.floor();
                    *carry -= n;
                    if n >= 1.0 {
                        scene.particles.push(burst(l.pos + Vec3::Y * 0.2, n as u32, rc.extend(0.8), rc.extend(0.0), |b| {
                            b.area = Vec3::new(0.08, 0.05, 0.08);
                            b.vel = Vec3::Y * 5.0;
                            b.spread = 0.04;
                            b.gravity = 0.0;
                            b.drag = 0.0;
                            b.size = (0.2, 0.05);
                            b.life = (0.8, 1.2);
                            b.stretch = true;
                        }));
                    }
                }
            }
        }
        // Gold: little glinting coins.
        for (i, p) in g.gold.iter().enumerate() {
            let glint = 1.0 + 1.5 * ((time * 5.0 + i as f32 * 1.7).sin().max(0.0)).powi(8);
            scene.sdfs.push(rs::SdfInstance {
                a: *p + Vec3::new(-0.06, 0.06, 0.0),
                b: *p + Vec3::new(0.06, 0.06, 0.0),
                ra: 0.07,
                rb: 0.07,
                color: Vec3::new(1.0, 0.8, 0.3),
                emissive: 0.4 * glint,
                style: Style::Lit,
                flags: rs::flags::NO_CUT,
                group: 1,
            });
        }
        // The spot the hero can use right now: a soft ring at its feet.
        if let Some(sp) = g.near.and_then(|i| g.spots.get(i)) {
            let mut m = MeshData::default();
            let r = 1.0 + 0.08 * (time * 4.0).sin();
            band(&mut m, sp.pos + Vec3::Y * 0.05, r - 0.08, r, 0.0, std::f32::consts::TAU, 1.0);
            decal(scene, m, Vec3::new(1.0, 0.85, 0.5), 1.4);
        }
        // Weapon swing trails: glowing streaks from the blade's last position.
        let mut live = Vec::new();
        for o in &curr.objects {
            let Some(p) = &o.puppet else { continue };
            let def: &PuppetDef = p.def.as_deref().unwrap_or(&curr.puppet_def);
            if def.weapon.kind == WeaponKind::None {
                continue;
            }
            let st = &p.state;
            let kind = ActKind::from_u8(st.act_kind);
            let striking = matches!(kind, ActKind::Slash | ActKind::Overhead | ActKind::Thrust | ActKind::Spin | ActKind::Leap)
                && st.act >= st.act_hit - 0.06
                && st.act <= st.act_hit + 0.2;
            if !striking || !particles {
                continue;
            }
            let feet = o.pos - Vec3::Y * p.feet_offset;
            let (_, span) = pav_core::puppet::pose_ex(def, st, p.rig.as_ref(), feet, cam_fwd);
            let Some((hand, tip)) = span else { continue };
            live.push(o.id.0);
            let (lh, lt) = self.tips.get(&o.id.0).copied().unwrap_or((hand, tip));
            self.tips.insert(o.id.0, (hand, tip));
            let wc = pav_core::Color::try_hex(&def.weapon.color).map(|c| Vec3::from(c.0)).unwrap_or(Vec3::ONE);
            let glow = wc.lerp(Vec3::new(0.9, 0.95, 1.0), 0.5) * (1.2 + def.weapon.glow);
            let n = 10;
            for i in 0..n {
                let t = i as f32 / n as f32;
                let along = 0.45 + 0.55 * ((i * 7) % n) as f32 / n as f32;
                let a = lh.lerp(lt, along);
                let b = hand.lerp(tip, along);
                scene.particles.push(burst(a.lerp(b, t), 1, glow.extend(0.85), glow.extend(0.0), |b| {
                    b.size = (0.09, 0.02);
                    b.life = (0.1, 0.18);
                    b.spread = 0.2;
                }));
            }
        }
        self.tips.retain(|k, _| live.contains(k));
        if self.carry.len() > 4096 {
            self.carry.clear();
        }
    }
}
