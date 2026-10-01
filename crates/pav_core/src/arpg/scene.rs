//! Game scenes. G1: the wave arena (a stone ring with braziers; monsters pour in, every wave a
//! little harder) - later the town's Proving Grounds.

use glam::Vec3;

use super::combat::Rarity;
use super::hero::Hero;
use super::{ArenaState, Game, spawn_monster_into};
use crate::color::Color;
use crate::entity::{BodyKind, Spawn};
use crate::fxdef::{EmitterDef, LightDef};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::Block;

/// Movement settings for the game: run fast, no jumping (Space dodges).
pub fn game_movement(sim: &mut Sim) {
    let m = &mut sim.config.movement;
    m.model = crate::character::MovementModel::Instant;
    m.speed = 6.2;
    m.allow_jump = false;
    m.ledge_grab = false;
    m.hit_stun = 0.0;
    m.face_aim = false;
}

fn brazier(sim: &mut Sim, pos: Vec3) {
    let mut v = Visual::new(Shape::Cylinder { half_height: 0.5, radius: 0.32 }, Color::hex("#3a3532"));
    v.look = Look::Lit;
    v.light = Some(Box::new(LightDef {
        color: "#ff9a4a".into(),
        radius: 9.0,
        intensity: 2.2,
        flicker: 0.6,
        offset: Vec3::Y * 1.2,
        ..Default::default()
    }));
    v.particles = Some(Box::new(EmitterDef { preset: "fire".into(), offset: Vec3::Y * 0.6, size: 0.8, ..Default::default() }));
    sim.spawn(Spawn::new("brazier", pos + Vec3::Y * 0.5).visual(v).body(BodyKind::Fixed));
}

/// The wave arena: a 40 m stone floor in a ring of walls, pillars and braziers.
pub fn build_arena(sim: &mut Sim) {
    game_movement(sim);
    let half = 20.0f32;
    let st = &mut sim.state;
    let a = Color::hex("#5b5652");
    let b = Color::hex("#545050");
    let t = 4.0;
    let n = (half * 2.0 / t) as i32;
    for i in 0..n {
        for j in 0..n {
            let x0 = -half + i as f32 * t;
            let z0 = -half + j as f32 * t;
            let c = if (i + j) % 2 == 0 { a } else { b };
            st.statics.add(&mut st.physics, Block::new(Vec3::new(x0, -0.5, z0), Vec3::new(x0 + t, 0.0, z0 + t), c));
        }
    }
    let wall = Color::hex("#6e6660");
    let h = 2.2;
    for (min, max) in [
        (Vec3::new(-half - 1.0, 0.0, -half - 1.0), Vec3::new(half + 1.0, h, -half)),
        (Vec3::new(-half - 1.0, 0.0, half), Vec3::new(half + 1.0, h, half + 1.0)),
        (Vec3::new(-half - 1.0, 0.0, -half), Vec3::new(-half, h, half)),
        (Vec3::new(half, 0.0, -half), Vec3::new(half + 1.0, h, half)),
    ] {
        st.statics.add(&mut st.physics, Block::new(min, max, wall));
    }
    // Pillars in a ring (cover from spitters) and a raised dais in the middle.
    let pillar = Color::hex("#7a726a");
    for k in 0..8 {
        let ang = k as f32 / 8.0 * std::f32::consts::TAU + 0.39;
        let c = Vec3::new(ang.cos(), 0.0, ang.sin()) * 11.0;
        st.statics.add(&mut st.physics, Block::new(c + Vec3::new(-0.7, 0.0, -0.7), c + Vec3::new(0.7, 3.2, 0.7), pillar));
    }
    st.statics.add(&mut st.physics, Block::new(Vec3::new(-2.5, 0.0, -2.5), Vec3::new(2.5, 0.12, 2.5), Color::hex("#6a5f55")));
    for c in [Vec3::new(-15.0, 0.0, -15.0), Vec3::new(15.0, 0.0, -15.0), Vec3::new(-15.0, 0.0, 15.0), Vec3::new(15.0, 0.0, 15.0)]
    {
        brazier(sim, c);
    }
    sim.state.spawn = Vec3::new(0.0, 0.12, 0.0);
    sim.spawn_player();
    sim.start_game(Hero::default());
    if let Some(g) = sim.state.game.as_mut() {
        g.arena = Some(ArenaState { wave: 0, next_in: 2.0, center: Vec3::ZERO });
        g.say("The Arena: survive the waves", 3.0);
    }
}

const FAMILIES: &[(&str, u32)] =
    &[("ghoul", 5), ("skitterer", 4), ("spitter", 3), ("ashdrake", 2), ("bile_ooze", 2), ("bonecrusher", 1)];

/// Next wave when the arena is clear.
pub fn update_arena(g: &mut Game, sim: &mut Sim, dt: f32) {
    let Some(mut ar) = g.arena.take() else { return };
    if g.monsters_alive() == 0 {
        ar.next_in -= dt;
        if ar.next_in <= 0.0 {
            ar.wave += 1;
            spawn_wave(g, sim, &ar);
            ar.next_in = 3.0;
            g.say(format!("Wave {}", ar.wave), 2.0);
        }
    }
    g.arena = Some(ar);
}

fn spawn_wave(g: &mut Game, sim: &mut Sim, ar: &ArenaState) {
    let wave = ar.wave;
    let level = 1 + (wave - 1) / 2;
    let packs = (1 + wave / 2).min(6);
    let total: u32 = FAMILIES.iter().map(|f| f.1).sum();
    for p in 0..packs {
        let rng = &mut sim.state.rng;
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let center = ar.center + Vec3::new(ang.cos(), 0.0, ang.sin()) * rng.range(12.0, 17.0);
        let mut pick = rng.below(total);
        let fam = FAMILIES.iter().find(|f| {
            if pick < f.1 {
                true
            } else {
                pick -= f.1;
                false
            }
        });
        let fam = fam.map(|f| f.0).unwrap_or("ghoul");
        let rarity = if wave.is_multiple_of(5) && p == 0 {
            Rarity::Rare
        } else if wave >= 3 && rng.f32() < 0.25 {
            Rarity::Magic
        } else {
            Rarity::Normal
        };
        let count = match (fam, rarity) {
            ("bonecrusher", _) => 1,
            (_, Rarity::Rare) => 1,
            ("skitterer", _) => 4 + wave.min(6),
            _ => 2 + (wave / 2).min(4),
        };
        let pack = g.next_pack;
        g.next_pack += 1;
        for i in 0..count {
            let a = i as f32 * 2.4;
            let at = center + Vec3::new(a.cos(), 0.0, a.sin()) * (0.8 + i as f32 * 0.35);
            let at = Vec3::new(at.x.clamp(-18.5, 18.5), 0.0, at.z.clamp(-18.5, 18.5));
            if let Some(id) = spawn_monster_into(sim, g, fam, level, rarity, at, pack) {
                if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
                    b.aggro = true;
                }
            }
        }
    }
}
