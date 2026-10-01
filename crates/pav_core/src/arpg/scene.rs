//! Game places: the town of Emberwatch (vendor, stash, portal, townsfolk) and the wave arena
//! (the Proving Grounds: a stone ring where monsters pour in, every wave a little harder).
//! G5 adds the levels and the endless depths.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::cmd::{Place, Spot, SpotKind};
use super::combat::Rarity;
use super::hero::Hero;
use super::{ArenaState, Game, feet_of, flat, spawn_monster_into};
use crate::color::Color;
use crate::entity::{BodyKind, EntityId, Spawn};
use crate::frame::SimEvent;
use crate::fxdef::{DistortDef, EmitterDef, LightDef};
use crate::puppet::{ActKind, PuppetDef, WeaponKind};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::Block;

/// Builds a place around a game in progress (travel).
pub fn build_place(sim: &mut Sim, place: Place, g: Game) {
    match place {
        Place::Town => build_town(sim, Some(g)),
        Place::Arena => build_arena_with(sim, Some(g)),
    }
}

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
    build_arena_with(sim, None);
}

fn build_arena_with(sim: &mut Sim, game: Option<Game>) {
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
    // A portal home in the north wall's alcove.
    portal(sim, Vec3::new(0.0, 0.0, -17.5));
    sim.state.spawn = Vec3::new(0.0, 0.12, 0.0);
    sim.spawn_player();
    match game {
        Some(g) => sim.resume_game(g),
        None => sim.start_game(Hero::default()),
    }
    if let Some(g) = sim.state.game.as_mut() {
        g.place = Place::Arena;
        g.arena = Some(ArenaState { wave: 0, next_in: 2.0, center: Vec3::ZERO });
        g.spots.push(Spot {
            kind: SpotKind::Portal,
            name: "Portal to Emberwatch".into(),
            pos: Vec3::new(0.0, 0.0, -17.5),
            reach: 2.6,
        });
        g.say("The Proving Grounds: survive the waves", 3.0);
    }
}

/// A shimmering portal: a ring of standing stones around a glowing pool.
fn portal(sim: &mut Sim, at: Vec3) {
    let st = &mut sim.state;
    let stone = Color::hex("#4a4650");
    for k in 0..10 {
        let a = k as f32 / 10.0 * std::f32::consts::TAU;
        let c = at + Vec3::new(a.cos(), 0.0, a.sin()) * 1.9;
        let h = if k % 2 == 0 { 1.5 } else { 1.0 };
        st.statics.add(
            &mut st.physics,
            Block::new(c + Vec3::new(-0.22, 0.0, -0.22), c + Vec3::new(0.22, h, 0.22), stone)
                .with_flags(crate::statics::block_flags::ROUNDED),
        );
    }
    let mut v = Visual::new(Shape::Cylinder { half_height: 0.03, radius: 1.45 }, Color::hex("#7fd8ff"));
    v.look = Look::Unlit;
    v.emissive = 2.2;
    v.light = Some(Box::new(LightDef {
        color: "#6ac8ff".into(),
        radius: 8.0,
        intensity: 2.6,
        pulse: 0.6,
        offset: Vec3::Y * 1.2,
        ..Default::default()
    }));
    v.particles = Some(Box::new(EmitterDef {
        preset: "magic".into(),
        color: "#9ae4ff".into(),
        area: Vec3::new(1.2, 0.05, 1.2),
        size: 1.2,
        ..Default::default()
    }));
    v.distortion =
        Some(Box::new(DistortDef { kind: "ripple".into(), radius: 1.6, strength: 0.35, period: 1.2, offset: Vec3::Y * 0.4 }));
    sim.spawn(Spawn::new("portal", at + Vec3::Y * 0.04).visual(v));
}

/// A lamp post with a warm light.
fn lamp(sim: &mut Sim, at: Vec3) {
    let st = &mut sim.state;
    st.statics.add(
        &mut st.physics,
        Block::new(at + Vec3::new(-0.09, 0.0, -0.09), at + Vec3::new(0.09, 2.6, 0.09), Color::hex("#2e2a28")),
    );
    let mut v = Visual::new(Shape::Sphere { radius: 0.2 }, Color::hex("#ffd38a"));
    v.look = Look::Unlit;
    v.emissive = 2.5;
    v.light =
        Some(Box::new(LightDef { color: "#ffb56a".into(), radius: 7.5, intensity: 1.8, flicker: 0.25, ..Default::default() }));
    sim.spawn(Spawn::new("lamp", at + Vec3::Y * 2.75).visual(v));
}

/// A house: walls, a door, a stepped roof.
fn house(sim: &mut Sim, min: Vec3, max: Vec3, wall: &str, roof: &str, door_side: f32) {
    let st = &mut sim.state;
    let h = 3.2;
    st.statics.add(&mut st.physics, Block::new(Vec3::new(min.x, 0.0, min.z), Vec3::new(max.x, h, max.z), Color::hex(wall)));
    let mut lo = Vec3::new(min.x - 0.3, h, min.z - 0.3);
    let mut hi = Vec3::new(max.x + 0.3, h + 0.5, max.z + 0.3);
    let rc = Color::hex(roof);
    for _ in 0..4 {
        st.statics.add(&mut st.physics, Block::new(lo, hi, rc));
        let shrink = Vec3::new(0.0, 0.0, (hi.z - lo.z) * 0.17);
        lo += shrink + Vec3::Y * 0.5;
        hi += -shrink + Vec3::Y * 0.5;
    }
    // A door and a lit window on the side facing the square.
    let cx = (min.x + max.x) * 0.5;
    let z = if door_side > 0.0 { max.z } else { min.z };
    let dz = door_side * 0.06;
    st.statics.add(
        &mut st.physics,
        Block::new(Vec3::new(cx - 0.55, 0.0, z.min(z + dz)), Vec3::new(cx + 0.55, 2.0, z.max(z + dz)), Color::hex("#4a3020")),
    );
    let mut win = Visual::new(Shape::Box { half: Vec3::new(0.4, 0.35, 0.03) }, Color::hex("#ffcf7a"));
    win.look = Look::Unlit;
    win.emissive = 1.6;
    sim.spawn(Spawn::new("window", Vec3::new(cx + 1.6, 1.7, z + dz)).visual(win));
}

/// Townsfolk: who they are and what they do when idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcRole {
    /// Hammers at the anvil; turns to greet the hero.
    Smith,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Npc {
    pub id: EntityId,
    pub role: NpcRole,
    pub name: String,
    /// Where they work and which way they face while working.
    pub facing: f32,
    pub work: Vec3,
    pub t: f32,
    pub greeted: bool,
}

/// Emberwatch: a small square at dusk with a smith, the stash and the portal out.
pub fn build_town(sim: &mut Sim, game: Option<Game>) {
    game_movement(sim);
    let half = 22.0f32;
    let st = &mut sim.state;
    // Cobbles: 2 m tiles in a few warm greys.
    let cobbles = ["#5e5650", "#58514c", "#625a52", "#5a534d"];
    let t = 2.0;
    let n = (half * 2.0 / t) as i32;
    for i in 0..n {
        for j in 0..n {
            let x0 = -half + i as f32 * t;
            let z0 = -half + j as f32 * t;
            let k = (crate::rng::hash3(77, i, 0, j) % 4) as usize;
            st.statics.add(
                &mut st.physics,
                Block::new(Vec3::new(x0, -0.5, z0), Vec3::new(x0 + t, 0.0, z0 + t), Color::hex(cobbles[k])),
            );
        }
    }
    // A low town wall around the square.
    let wall = Color::hex("#6b625a");
    for (min, max) in [
        (Vec3::new(-half - 1.0, 0.0, -half - 1.0), Vec3::new(half + 1.0, 1.4, -half)),
        (Vec3::new(-half - 1.0, 0.0, half), Vec3::new(half + 1.0, 1.4, half + 1.0)),
        (Vec3::new(-half - 1.0, 0.0, -half), Vec3::new(-half, 1.4, half)),
        (Vec3::new(half, 0.0, -half), Vec3::new(half + 1.0, 1.4, half)),
    ] {
        st.statics.add(&mut st.physics, Block::new(min, max, wall));
    }
    // The well in the middle.
    st.statics.add(
        &mut st.physics,
        Block::new(Vec3::new(-1.3, 0.0, -1.3), Vec3::new(1.3, 0.8, 1.3), Color::hex("#77706a"))
            .with_flags(crate::statics::block_flags::ROUNDED),
    );
    st.statics.add(&mut st.physics, Block::new(Vec3::new(-1.0, 0.8, -1.0), Vec3::new(1.0, 0.82, 1.0), Color::hex("#2a4a5a")));
    for x in [-1.1f32, 1.1] {
        st.statics
            .add(&mut st.physics, Block::new(Vec3::new(x - 0.1, 0.8, -0.1), Vec3::new(x + 0.1, 2.4, 0.1), Color::hex("#5a4030")));
    }
    st.statics.add(&mut st.physics, Block::new(Vec3::new(-1.4, 2.4, -0.7), Vec3::new(1.4, 2.6, 0.7), Color::hex("#7a3a2a")));
    // Houses around the square.
    house(sim, Vec3::new(-19.0, 0.0, 12.0), Vec3::new(-11.0, 0.0, 19.0), "#8a7a66", "#7a3a2a", -1.0);
    house(sim, Vec3::new(11.0, 0.0, 12.0), Vec3::new(19.0, 0.0, 19.0), "#7d7468", "#5a3a5a", -1.0);
    house(sim, Vec3::new(-19.0, 0.0, -19.0), Vec3::new(-12.0, 0.0, -12.0), "#857563", "#3a4a6a", 1.0);
    house(sim, Vec3::new(12.0, 0.0, -19.0), Vec3::new(19.0, 0.0, -12.0), "#8a7d6a", "#7a5a2a", 1.0);
    // The smithy (west): forge, anvil, the smith.
    let st = &mut sim.state;
    let forge = Vec3::new(-12.5, 0.0, 0.0);
    st.statics.add(
        &mut st.physics,
        Block::new(forge + Vec3::new(-1.2, 0.0, -1.4), forge + Vec3::new(0.6, 1.2, 1.4), Color::hex("#4a4040")),
    );
    st.statics.add(
        &mut st.physics,
        Block::new(forge + Vec3::new(-1.0, 1.2, -0.5), forge + Vec3::new(-0.2, 3.6, 0.5), Color::hex("#3e3636")),
    );
    // Awning over the smithy.
    for (x, z) in [(-14.4f32, -3.2f32), (-14.4, 3.2), (-8.6, -3.2), (-8.6, 3.2)] {
        st.statics.add(
            &mut st.physics,
            Block::new(Vec3::new(x - 0.12, 0.0, z - 0.12), Vec3::new(x + 0.12, 3.0, z + 0.12), Color::hex("#5a4030")),
        );
    }
    st.statics.add(&mut st.physics, Block::new(Vec3::new(-14.7, 3.0, -3.5), Vec3::new(-8.3, 3.2, 3.5), Color::hex("#8a4a2a")));
    let mut fire = Visual::new(Shape::Box { half: Vec3::new(0.5, 0.05, 0.9) }, Color::hex("#ff8a3a"));
    fire.look = Look::Unlit;
    fire.emissive = 2.5;
    fire.light = Some(Box::new(LightDef {
        color: "#ff8a3a".into(),
        radius: 10.0,
        intensity: 2.6,
        flicker: 0.5,
        shadows: true,
        offset: Vec3::Y * 0.8,
        ..Default::default()
    }));
    fire.particles =
        Some(Box::new(EmitterDef { preset: "fire".into(), area: Vec3::new(0.4, 0.05, 0.7), size: 0.9, ..Default::default() }));
    sim.spawn(Spawn::new("forge fire", forge + Vec3::new(-0.3, 1.25, 0.0)).visual(fire));
    let anvil = Vec3::new(-9.8, 0.0, 0.0);
    let st = &mut sim.state;
    st.statics.add(
        &mut st.physics,
        Block::new(anvil + Vec3::new(-0.25, 0.0, -0.2), anvil + Vec3::new(0.25, 0.55, 0.2), Color::hex("#3a3634")),
    );
    st.statics.add(
        &mut st.physics,
        Block::new(anvil + Vec3::new(-0.3, 0.55, -0.45), anvil + Vec3::new(0.3, 0.8, 0.45), Color::hex("#2a2a30")),
    );
    // Weapon rack.
    for k in 0..4 {
        let z = -2.2 + k as f32 * 0.35;
        st.statics.add(
            &mut st.physics,
            Block::new(Vec3::new(-14.1, 0.0, z - 0.03), Vec3::new(-14.0, 1.6, z + 0.03), Color::hex("#b8bcc4")),
        );
    }
    // The stash (east): an iron-bound chest under a lantern.
    let chest = Vec3::new(11.0, 0.0, 0.0);
    st.statics.add(
        &mut st.physics,
        Block::new(chest + Vec3::new(-0.8, 0.0, -0.55), chest + Vec3::new(0.8, 0.85, 0.55), Color::hex("#6a4426"))
            .with_flags(crate::statics::block_flags::ROUNDED),
    );
    st.statics.add(
        &mut st.physics,
        Block::new(chest + Vec3::new(-0.82, 0.55, -0.57), chest + Vec3::new(0.82, 0.65, 0.57), Color::hex("#c9a54a")),
    );
    lamp(sim, chest + Vec3::new(1.4, 0.0, -1.0));
    // Lamps around the square, the portal north.
    for p in [Vec3::new(-6.0, 0.0, 6.0), Vec3::new(6.0, 0.0, 6.0), Vec3::new(-6.0, 0.0, -8.0), Vec3::new(6.0, 0.0, -8.0)] {
        lamp(sim, p);
    }
    let portal_at = Vec3::new(0.0, 0.0, -16.0);
    portal(sim, portal_at);
    // The smith.
    let smith_feet = anvil + Vec3::new(-0.9, 0.0, 0.0);
    let look = PuppetDef {
        scale: 1.08,
        torso_radius: 0.23,
        shoulder_width: 0.25,
        skin: "#c98f6a".into(),
        shirt: "#5a3a22".into(),
        pants: "#3a302a".into(),
        shoes: "#2a2220".into(),
        weapon: crate::puppet::WeaponLook { kind: WeaponKind::Mace, color: "#5a5a60".into(), size: 0.8, ..Default::default() },
        ..Default::default()
    };
    let smith = sim.spawn_npc("Hilda", smith_feet, std::f32::consts::FRAC_PI_2, look, None, None);
    sim.state.spawn = Vec3::new(0.0, 0.05, 7.0);
    sim.spawn_player();
    if let Some(p) = sim.state.player {
        if let Some(ch) = sim.state.entities.get_mut(p).and_then(|e| e.character.as_mut()) {
            ch.facing = std::f32::consts::PI;
            ch.anim.facing = ch.facing;
        }
    }
    match game {
        Some(g) => sim.resume_game(g),
        None => sim.start_game(Hero::default()),
    }
    let mut g = sim.state.game.take().unwrap();
    g.place = Place::Town;
    g.spots.push(Spot { kind: SpotKind::Vendor, name: "Hilda the Smith".into(), pos: smith_feet, reach: 3.2 });
    g.spots.push(Spot { kind: SpotKind::Stash, name: "Stash".into(), pos: chest, reach: 2.6 });
    g.spots.push(Spot { kind: SpotKind::Portal, name: "Portal".into(), pos: portal_at, reach: 2.8 });
    g.npcs.push(Npc {
        id: smith,
        role: NpcRole::Smith,
        name: "Hilda".into(),
        facing: std::f32::consts::FRAC_PI_2,
        work: anvil + Vec3::Y * 0.8,
        t: 0.0,
        greeted: false,
    });
    g.restock(sim);
    g.say(Place::Town.name(), 2.5);
    sim.state.game = Some(g);
}

/// Townsfolk at work: the smith hammers (sparks on every blow) and turns to the hero.
pub fn update_npcs(g: &mut Game, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
    let hero = g.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0);
    let mut greet = None;
    for n in &mut g.npcs {
        let Some((feet, _)) = feet_of(sim, n.id) else { continue };
        let Some(ch) = sim.state.entities.get_mut(n.id).and_then(|e| e.character.as_mut()) else { continue };
        let near = hero.filter(|h| flat(*h - feet).length() < 4.5);
        match n.role {
            NpcRole::Smith => match near {
                Some(h) => {
                    // Stop, turn, nod.
                    let to = flat(h - feet);
                    ch.face = Some(to.x.atan2(to.z));
                    ch.anim.act_kind = 0;
                    ch.anim.act = 0.0;
                    n.t = 0.0;
                    if !n.greeted {
                        n.greeted = true;
                        greet = Some((feet, n.name.clone()));
                    }
                }
                None => {
                    n.greeted = false;
                    ch.face = Some(n.facing);
                    let period = 1.5;
                    let before = n.t;
                    n.t = (n.t + dt) % period;
                    ch.anim.act_kind = ActKind::Overhead.index();
                    ch.anim.act = n.t / period;
                    ch.anim.act_hit = 0.5;
                    ch.anim.act_side = 1.0;
                    if before < period * 0.5 && n.t >= period * 0.5 {
                        events.push(SimEvent::Strike { pos: n.work, power: 3.0, element: 0, crit: false });
                    }
                }
            },
        }
    }
    if let Some((at, name)) = greet {
        g.float_text(at + Vec3::Y * 2.3, format!("{name}: \"Need an edge on that blade?\""));
    }
}

const FAMILIES: &[(&str, u32)] =
    &[("ghoul", 5), ("skitterer", 4), ("spitter", 3), ("ashdrake", 2), ("bile_ooze", 2), ("bonecrusher", 1)];

/// Next wave when the arena is clear.
pub fn update_arena(g: &mut Game, sim: &mut Sim, dt: f32) {
    if g.place != Place::Arena {
        return;
    }
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
