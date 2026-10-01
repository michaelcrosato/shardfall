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
