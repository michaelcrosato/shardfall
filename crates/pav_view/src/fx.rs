//! Visual effects from simulation data: particle presets for object emitters and game events,
//! object lights (flicker, pulse) and screen distortion sources.

use glam::{Vec3, Vec4};
use pav_core::color::Color;
use pav_core::fxdef::{DistortDef, EmitterDef, LightDef};
use pav_render::{self as rs, DistortKind, Distortion, ParticleBurst};

fn rgba(c: Vec3, a: f32) -> Vec4 {
    c.extend(a)
}

/// A preset's template burst (count 0) and its default rate (particles per second).
pub fn preset(name: &str) -> (ParticleBurst, f32) {
    let b = ParticleBurst::default();
    match name {
        "smoke" => (
            ParticleBurst {
                area: Vec3::new(0.2, 0.05, 0.2),
                vel: Vec3::new(0.0, 0.9, 0.0),
                spread: 0.3,
                gravity: -0.3,
                drag: 0.6,
                life: (2.2, 3.6),
                size: (0.15, 0.75),
                color0: Vec4::new(0.22, 0.22, 0.24, 0.55),
                color1: Vec4::new(0.45, 0.45, 0.48, 0.0),
                additive: false,
                turbulence: 1.0,
                ..b
            },
            14.0,
        ),
        "steam" => (
            ParticleBurst {
                area: Vec3::new(0.15, 0.02, 0.15),
                vel: Vec3::new(0.0, 1.6, 0.0),
                spread: 0.4,
                gravity: -0.5,
                drag: 1.0,
                life: (1.0, 1.8),
                size: (0.1, 0.6),
                color0: Vec4::new(0.9, 0.92, 0.95, 0.45),
                color1: Vec4::new(1.0, 1.0, 1.0, 0.0),
                additive: false,
                turbulence: 1.5,
                ..b
            },
            20.0,
        ),
        "sparks" => (
            ParticleBurst {
                vel: Vec3::new(0.0, 3.5, 0.0),
                spread: 3.0,
                gravity: 9.8,
                drag: 0.3,
                life: (0.4, 1.0),
                size: (0.05, 0.015),
                color0: Vec4::new(6.0, 3.2, 1.0, 1.0),
                color1: Vec4::new(2.0, 0.4, 0.1, 0.0),
                stretch: true,
                bounce: 0.35,
                ..b
            },
            40.0,
        ),
        "embers" => (
            ParticleBurst {
                area: Vec3::new(0.3, 0.05, 0.3),
                vel: Vec3::new(0.0, 1.2, 0.0),
                spread: 0.6,
                gravity: -0.4,
                drag: 0.5,
                life: (1.5, 3.0),
                size: (0.035, 0.0),
                color0: Vec4::new(5.0, 1.6, 0.3, 1.0),
                color1: Vec4::new(1.5, 0.15, 0.0, 0.0),
                turbulence: 2.5,
                ..b
            },
            12.0,
        ),
        "fountain" => (
            ParticleBurst {
                area: Vec3::new(0.05, 0.0, 0.05),
                vel: Vec3::new(0.0, 6.5, 0.0),
                spread: 0.9,
                gravity: 9.8,
                life: (1.3, 1.7),
                size: (0.09, 0.06),
                color0: Vec4::new(0.55, 0.78, 1.1, 0.85),
                color1: Vec4::new(0.45, 0.65, 1.0, 0.0),
                additive: false,
                stretch: true,
                bounce: 0.1,
                ..b
            },
            90.0,
        ),
        "snow" => (
            ParticleBurst {
                area: Vec3::new(6.0, 0.0, 6.0),
                vel: Vec3::new(0.0, -1.0, 0.0),
                spread: 0.25,
                life: (6.0, 8.0),
                size: (0.05, 0.05),
                color0: Vec4::new(1.0, 1.0, 1.0, 0.95),
                color1: Vec4::new(1.0, 1.0, 1.0, 0.4),
                additive: false,
                turbulence: 0.8,
                bounce: 0.0,
                ..b
            },
            60.0,
        ),
        "rain" => (
            ParticleBurst {
                area: Vec3::new(6.0, 0.0, 6.0),
                vel: Vec3::new(0.3, -14.0, 0.0),
                spread: 0.3,
                life: (0.8, 1.0),
                size: (0.02, 0.02),
                color0: Vec4::new(0.7, 0.8, 1.0, 0.55),
                color1: Vec4::new(0.7, 0.8, 1.0, 0.35),
                additive: false,
                stretch: true,
                bounce: 0.0,
                ..b
            },
            300.0,
        ),
        "fireflies" => (
            ParticleBurst {
                area: Vec3::new(3.0, 1.0, 3.0),
                spread: 0.3,
                life: (3.0, 6.0),
                size: (0.08, 0.06),
                color0: Vec4::new(3.0, 3.2, 0.8, 1.0),
                color1: Vec4::new(2.0, 2.6, 0.4, 0.0),
                turbulence: 2.2,
                ..b
            },
            6.0,
        ),
        "magic" => (
            ParticleBurst {
                area: Vec3::new(0.3, 0.1, 0.3),
                vel: Vec3::new(0.0, 1.0, 0.0),
                spread: 1.4,
                gravity: -0.6,
                drag: 0.8,
                life: (0.8, 1.5),
                size: (0.08, 0.0),
                color0: Vec4::new(1.6, 0.7, 3.2, 1.0),
                color1: Vec4::new(0.3, 0.9, 3.0, 0.0),
                turbulence: 3.0,
                ..b
            },
            40.0,
        ),
        "dust" => (
            ParticleBurst {
                area: Vec3::new(0.3, 0.02, 0.3),
                vel: Vec3::new(0.0, 0.4, 0.0),
                spread: 1.2,
                gravity: 0.5,
                drag: 2.0,
                life: (0.6, 1.2),
                size: (0.1, 0.35),
                color0: Vec4::new(0.75, 0.68, 0.58, 0.5),
                color1: Vec4::new(0.8, 0.75, 0.68, 0.0),
                additive: false,
                ..b
            },
            20.0,
        ),
        "bubbles" => (
            ParticleBurst {
                area: Vec3::new(0.4, 0.05, 0.4),
                vel: Vec3::new(0.0, 1.0, 0.0),
                spread: 0.2,
                gravity: -0.8,
                drag: 0.8,
                life: (1.5, 2.5),
                size: (0.05, 0.08),
                color0: Vec4::new(0.8, 0.95, 1.2, 0.5),
                color1: Vec4::new(0.9, 1.0, 1.2, 0.0),
                additive: false,
                turbulence: 1.5,
                ..b
            },
            15.0,
        ),
        "confetti" => (
            ParticleBurst {
                vel: Vec3::new(0.0, 6.0, 0.0),
                spread: 3.5,
                gravity: 4.0,
                drag: 1.2,
                life: (2.5, 3.5),
                size: (0.06, 0.06),
                color0: Vec4::new(1.0, 0.4, 0.6, 1.0),
                color1: Vec4::new(1.0, 0.8, 0.3, 0.8),
                additive: false,
                turbulence: 2.0,
                bounce: 0.0,
                ..b
            },
            30.0,
        ),
        // fire
        _ => (
            ParticleBurst {
                area: Vec3::new(0.22, 0.05, 0.22),
                vel: Vec3::new(0.0, 1.5, 0.0),
                spread: 0.45,
                gravity: -1.5,
                drag: 1.0,
                life: (0.45, 0.9),
                size: (0.3, 0.04),
                color0: Vec4::new(4.0, 1.7, 0.45, 0.9),
                color1: Vec4::new(1.2, 0.2, 0.05, 0.0),
                turbulence: 1.5,
                ..b
            },
            70.0,
        ),
    }
}

/// The burst an object's emitter makes this frame (`count` new particles), floor at `floor`.
pub fn emitter_burst(def: &EmitterDef, pos: Vec3, count: u32) -> ParticleBurst {
    let (mut b, _) = preset(&def.preset);
    b.count = count;
    b.pos = pos + def.offset;
    if def.area != Vec3::ZERO {
        b.area = def.area;
    }
    b.vel *= def.speed;
    b.spread *= def.speed;
    b.size = (b.size.0 * def.size, b.size.1 * def.size);
    b.life = (b.life.0 * def.life, b.life.1 * def.life);
    if let Some(c) = Color::try_hex(&def.color) {
        let c = Vec3::from(c.0);
        let k0 = b.color0.truncate().max_element().max(0.01);
        let k1 = b.color1.truncate().max_element().max(0.01);
        b.color0 = rgba(c * k0, b.color0.w);
        b.color1 = rgba(c * k1 * 0.6, b.color1.w);
    }
    // Bouncy presets land on the emitter's own height.
    if b.stretch || b.bounce > 0.0 && b.gravity > 0.0 {
        b.floor = Some(pos.y);
    }
    b
}

/// Particles per second for an emitter.
pub fn emitter_rate(def: &EmitterDef) -> f32 {
    if def.rate > 0.0 { def.rate } else { preset(&def.preset).1 }
}

/// An object's light at time `t` (flicker and pulse are smooth pseudo-noise).
pub fn light(def: &LightDef, pos: Vec3, t: f32, seed: f32) -> rs::PointLight {
    let mut k = def.intensity;
    if def.flicker > 0.0 {
        let n = (t * 13.0 + seed).sin() * 0.5 + (t * 7.3 + seed * 1.7).sin() * 0.3 + (t * 23.0 + seed * 0.3).sin() * 0.2;
        k *= 1.0 - def.flicker * 0.35 * (n * 0.5 + 0.5);
    }
    if def.pulse > 0.0 {
        k *= 0.55 + 0.45 * (t * def.pulse * std::f32::consts::TAU + seed).sin();
    }
    let c = Color::try_hex(&def.color).map(|c| Vec3::from(c.0)).unwrap_or(Vec3::ONE);
    rs::PointLight { position: pos + def.offset, color: c * k, radius: def.radius, shadows: def.shadows }
}

/// An object's distortion at time `t`.
pub fn distortion(def: &DistortDef, pos: Vec3, t: f32) -> Distortion {
    let kind = match def.kind.as_str() {
        "lens" => DistortKind::Lens,
        "ripple" => DistortKind::Ripple,
        "ring" | "shockwave" => DistortKind::Ring,
        _ => DistortKind::Haze,
    };
    let progress = if kind == DistortKind::Ring { (t / def.period.max(0.1)).fract() } else { 0.0 };
    Distortion { pos: pos + def.offset, radius: def.radius, strength: def.strength, kind, progress }
}

/// Particles for game events.
pub fn event_bursts(e: &pav_core::frame::SimEvent, out: &mut Vec<ParticleBurst>) {
    use pav_core::frame::SimEvent as E;
    let with = |name: &str, pos: Vec3, count: u32, f: &dyn Fn(&mut ParticleBurst)| {
        let (mut b, _) = preset(name);
        b.pos = pos;
        b.count = count;
        f(&mut b);
        b
    };
    match e {
        E::Explosion { pos, radius } => {
            let r = *radius;
            out.push(with("sparks", *pos, 70, &|b| {
                b.spread = 7.0 * r.max(0.5);
                b.vel = Vec3::Y * 2.0;
                b.floor = Some(pos.y - 0.3);
            }));
            out.push(with("fire", *pos, 45, &|b| {
                b.area = Vec3::splat(r * 0.4);
                b.spread = 2.5 * r;
                b.size = (0.35 * r.max(0.6), 0.05);
                b.life = (0.25, 0.55);
            }));
            out.push(with("smoke", *pos + Vec3::Y * 0.3, 22, &|b| {
                b.area = Vec3::splat(r * 0.5);
                b.spread = 1.2;
                b.size = (0.3, 1.1);
            }));
        }
        E::Hit { pos, .. } => out.push(with("sparks", *pos, 16, &|b| {
            b.spread = 3.5;
            b.vel = Vec3::Y;
            b.life = (0.2, 0.45);
        })),
        E::Splash { pos } => out.push(with("fountain", *pos + Vec3::Y * 0.2, 40, &|b| {
            b.vel = Vec3::Y * 3.0;
            b.spread = 2.2;
            b.floor = None;
            b.life = (0.5, 0.8);
        })),
        E::Break { pos } => out.push(with("dust", *pos, 24, &|_| {})),
        E::Respawn { pos } => out.push(with("magic", *pos + Vec3::Y * 0.6, 50, &|b| {
            b.area = Vec3::new(0.3, 0.8, 0.3);
        })),
        E::Bounce { pos } => out.push(with("dust", *pos, 10, &|b| b.spread = 2.0)),
        E::Land { pos, speed } if *speed > 8.0 => out.push(with("dust", *pos, 12, &|b| b.spread = 2.0)),
        _ => {}
    }
}
