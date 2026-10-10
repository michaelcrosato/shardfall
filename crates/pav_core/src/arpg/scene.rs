//! Game places: the town of Emberwatch (vendor, stash, portal, townsfolk) and the wave arena
//! (the Proving Grounds: a stone ring where monsters pour in, every wave a little harder).
//! G5 adds the levels and the endless depths.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::cmd::{Place, Spot, SpotKind};
use super::combat::Rarity;
use super::hero::Hero;
use super::{ArenaState, Game, feet_of, flat};
use crate::color::Color;
use crate::entity::{BodyKind, EntityId, Spawn};
use crate::frame::SimEvent;
use crate::fxdef::{DistortDef, EmitterDef, LightDef};
use crate::moves::MoveId;
use crate::params::ChoiceParam;
use crate::puppet::{PuppetDef, WeaponKind};
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;
use crate::statics::Block;

/// Builds a place around a game in progress (travel).
pub fn build_place(sim: &mut Sim, place: Place, g: Game) {
    match place {
        Place::Town => build_town(sim, Some(g)),
        Place::Arena => build_arena_with(sim, Some(g)),
        Place::Lab => build_lab(sim, Some(g)),
        Place::Level(n) => super::world::build_level(sim, n, Some(g)),
    }
}

/// A creature on show in the Menagerie.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exhibit {
    pub entity: EntityId,
    /// "family:<key>" or "genome:<seed>".
    pub source: String,
    pub level: u32,
    pub spot: usize,
}

const LAB_COLS: usize = 5;
const LAB_ROWS: usize = 4;
const LAB_LEVEL: u32 = 20;

fn pedestal_pos(i: usize) -> Vec3 {
    let (c, r) = (i % LAB_COLS, i / LAB_COLS);
    Vec3::new((c as f32 - (LAB_COLS - 1) as f32 * 0.5) * 6.0, 0.35, -4.0 - r as f32 * 6.5)
}

/// The Menagerie: a long hall of pedestals with creatures on show, a portal home.
pub fn build_lab(sim: &mut Sim, game: Option<Game>) {
    game_movement(sim);
    let st = &mut sim.state;
    let (w, front, back) = (17.0f32, 6.0f32, -30.0f32);
    let tiles = ["#3a3842", "#34323c"];
    let mut x = -w;
    let mut i = 0;
    while x < w {
        let mut z = back;
        while z < front {
            st.statics.add(
                &mut st.physics,
                Block::new(Vec3::new(x, -0.5, z), Vec3::new(x + 2.0, 0.0, z + 2.0), Color::hex(tiles[i % 2])),
            );
            z += 2.0;
            i += 1;
        }
        x += 2.0;
        i += 1;
    }
    let wall = Color::hex("#4a4654");
    for (min, max) in [
        (Vec3::new(-w - 1.0, 0.0, back - 1.0), Vec3::new(w + 1.0, 3.5, back)),
        (Vec3::new(-w - 1.0, 0.0, front), Vec3::new(w + 1.0, 1.2, front + 1.0)),
        (Vec3::new(-w - 1.0, 0.0, back), Vec3::new(-w, 3.5, front)),
        (Vec3::new(w, 0.0, back), Vec3::new(w + 1.0, 3.5, front)),
    ] {
        st.statics.add(&mut st.physics, Block::new(min, max, wall));
    }
    // Pedestals with lights between them.
    for i in 0..LAB_COLS * LAB_ROWS {
        let p = pedestal_pos(i);
        let st = &mut sim.state;
        st.statics.add(
            &mut st.physics,
            Block::new(p + Vec3::new(-1.2, -0.35, -1.2), p + Vec3::new(1.2, 0.0, 1.2), Color::hex("#6a6474"))
                .with_flags(crate::statics::block_flags::ROUNDED),
        );
        if i % 2 == 0 {
            let mut v = Visual::new(Shape::Sphere { radius: 0.12 }, Color::hex("#bfe0ff"));
            v.look = Look::Unlit;
            v.emissive = 2.0;
            v.light = Some(Box::new(LightDef { color: "#bfd8ff".into(), radius: 8.0, intensity: 1.4, ..Default::default() }));
            sim.spawn(Spawn::new("lab light", p + Vec3::new(3.0, 3.2, 0.0)).visual(v));
        }
    }
    portal(sim, Vec3::new(0.0, 0.0, 3.5));
    sim.state.spawn = Vec3::new(0.0, 0.05, 1.0);
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
    g.place = Place::Lab;
    g.spots.push(Spot {
        kind: SpotKind::Portal,
        name: "Portal".into(),
        pos: Vec3::new(0.0, 0.0, 3.5),
        reach: 2.8,
        info: Vec::new(),
    });
    stock_lab(&mut g, sim, false);
    g.say("The Menagerie: every creature here was grown from a seed", 3.5);
    sim.state.game = Some(g);
}

/// Fills the pedestals: the designed families first, then creatures grown from seeds.
pub fn stock_lab(g: &mut Game, sim: &mut Sim, reroll: bool) {
    let d = super::data::data();
    for ex in std::mem::take(&mut g.exhibits) {
        sim.despawn(ex.entity);
    }
    g.spots.retain(|s| s.kind != SpotKind::Exhibit);
    if reroll || g.lab_seed == 0 {
        g.lab_seed = g.lab_seed.wrapping_add(sim.state.rng.next_u32() as u64 + 1);
    }
    let families: Vec<String> = if reroll { Vec::new() } else { d.families.iter().map(|f| f.key.clone()).collect() };
    for i in 0..LAB_COLS * LAB_ROWS {
        let (source, spec, card) = match families.get(i) {
            Some(k) => {
                let f = d.family(k).unwrap();
                let card = vec![
                    format!("Designed family '{}'", f.key),
                    format!("{} body · {:?} brain", f.body.name(), f.archetype),
                    format!("Skills: {}", f.skills.join(", ")),
                ];
                (format!("family:{k}"), f.spec(), card)
            }
            None => {
                let seed = g.lab_seed.wrapping_mul(1000).wrapping_add(i as u64);
                let Ok(gn) = super::genome::Genome::generate(&d, seed, LAB_LEVEL, &Default::default()) else { continue };
                let parts: Vec<&str> = gn.puppet.parts.iter().map(|p| p.kind.name()).collect();
                let card = vec![
                    format!("Genome seed {seed}"),
                    format!("{} body · {} ({:?} brain) · {}", gn.body.name(), gn.archetype, gn.brain, gn.element.name()),
                    format!(
                        "Size {:.2} · life x{:.2} · damage x{:.2} · speed x{:.2}",
                        gn.puppet.scale, gn.life, gn.damage, gn.speed
                    ),
                    format!("Parts: {}", if parts.is_empty() { "none".to_string() } else { parts.join(", ") }),
                    format!("Skills: {}", gn.skills.join(", ")),
                ];
                (format!("genome:{seed}"), gn.spec(&d), card)
            }
        };
        let at = pedestal_pos(i);
        let id = sim.spawn_npc(&spec.name, at, 0.0, spec.puppet.clone(), None, None);
        let spot = g.spots.len();
        g.spots.push(Spot { kind: SpotKind::Exhibit, name: spec.name.clone(), pos: at, reach: 2.6, info: card });
        g.exhibits.push(Exhibit { entity: id, source, level: LAB_LEVEL, spot });
    }
    g.inv_changed();
}

/// Lets an exhibit off its pedestal: it becomes a real monster, hunting the hero.
pub fn release_exhibit(g: &mut Game, sim: &mut Sim, spot: usize) -> Result<(), String> {
    let d = super::data::data();
    let i = g.exhibits.iter().position(|e| e.spot == spot).ok_or("nothing there")?;
    let ex = g.exhibits.remove(i);
    let spec = if let Some(k) = ex.source.strip_prefix("family:") {
        d.family(k).ok_or("unknown family")?.spec()
    } else {
        let seed: u64 = ex.source.strip_prefix("genome:").and_then(|s| s.parse().ok()).ok_or("bad exhibit")?;
        super::genome::Genome::generate(&d, seed, ex.level, &Default::default())?.spec(&d)
    };
    let at = sim.state.entities.get(ex.entity).map(|e| e.pos).unwrap_or(pedestal_pos(spot));
    sim.despawn(ex.entity);
    if let Some(s) = g.spots.get_mut(spot) {
        s.reach = 0.0;
        s.name = String::new();
    }
    let level = g.hero.level.max(1);
    let pack = g.next_pack;
    g.next_pack += 1;
    let id = super::spawn_spec_into(sim, g, &spec, level, Rarity::Normal, Vec3::new(at.x, 0.4, at.z), pack)
        .ok_or("could not spawn")?;
    if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
        b.aggro = true;
    }
    g.say(format!("{} is loose!", spec.name), 2.0);
    Ok(())
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

pub(super) fn brazier(sim: &mut Sim, pos: Vec3) {
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
            info: Vec::new(),
        });
        g.say("The Proving Grounds: survive the waves", 3.0);
    }
}

/// A shimmering portal: a ring of standing stones around a glowing pool.
pub(crate) fn portal(sim: &mut Sim, at: Vec3) -> EntityId {
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
    v.emissive = 0.9;
    v.light = Some(Box::new(LightDef {
        color: "#6ac8ff".into(),
        radius: 8.0,
        intensity: 1.8,
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
    sim.spawn(Spawn::new("portal", at + Vec3::Y * 0.04).visual(v))
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
    /// Hammers at the anvil (sparks on every blow); sells gear.
    Smith,
    /// Flips a coin at the card table; sells mystery items by slot.
    Gambler,
    /// Stirs the cauldron; brews potion upgrades.
    Alchemist,
    /// Guards the portal; knows how deep you've been.
    Captain,
    /// Walks a round of the square, stops to look about.
    Villager,
    /// Follows the hero about town.
    Dog,
}

impl NpcRole {
    /// What they say when the hero comes near.
    fn lines(self) -> &'static [&'static str] {
        match self {
            NpcRole::Smith => {
                &["Need an edge on that blade?", "Fresh steel, still warm.", "Bring me something rare, I pay well."]
            }
            NpcRole::Gambler => &[
                "Feeling lucky? Pick a slot.",
                "Every box holds something. Sometimes something wonderful.",
                "The house usually wins. Usually.",
            ],
            NpcRole::Alchemist => &["A stronger brew for the depths, dear?", "Drink deep, come back alive.", "Mind the fumes."],
            NpcRole::Captain => &[
                "The portal takes you to any depth you've reached.",
                "Twelve levels down. Past them the Depths never end.",
                "Watch the gates. Watch the floor. Watch everything.",
            ],
            NpcRole::Villager => {
                &["Good day!", "Did you hear something below?", "Lovely evening for a descent.", "Mind the dog."]
            }
            NpcRole::Dog => &["Woof!", "*wags*", "*sniffs your boots*"],
        }
    }
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
    /// Walkers: the points of their round, which one is next, how long to stand about.
    #[serde(default)]
    pub route: Vec<Vec3>,
    #[serde(default)]
    pub leg: usize,
    #[serde(default)]
    pub wait: f32,
    /// A thing they handle (the gambler's coin).
    #[serde(default)]
    pub prop: Option<EntityId>,
}

impl Npc {
    fn new(id: EntityId, role: NpcRole, name: &str, facing: f32, work: Vec3) -> Self {
        Self {
            id,
            role,
            name: name.into(),
            facing,
            work,
            t: 0.0,
            greeted: false,
            route: Vec::new(),
            leg: 0,
            wait: 0.0,
            prop: None,
        }
    }
}

/// Gambling: a mystery item of a slot for this much gold.
pub fn gamble_price(level: u32, slot: super::items::Slot) -> u64 {
    let l = level as f32;
    let base = 40.0 + 8.0 * l + 0.35 * l * l;
    (base * if slot == super::items::Slot::Weapon { 1.6 } else { 1.0 }).round() as u64
}

/// Brews: 0 = one more potion (up to 6), 1 = stronger potions (+20%, five times). None = maxed.
pub fn brew_price(hero: &Hero, kind: u8) -> Option<u64> {
    match kind {
        0 if hero.potion_max < 6 => Some(150 * 2u64.pow(hero.potion_max.saturating_sub(3))),
        1 => {
            let n = (hero.mods.get(super::stats::Stat::PotionInc) / 20.0).round() as u64;
            (n < 5).then(|| 200 * (n + 1) * (n + 1))
        }
        _ => None,
    }
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
    // Posts of the smithy (open to the sky so the camera sees the smith at work).
    for z in [-3.2f32, 3.2] {
        st.statics.add(
            &mut st.physics,
            Block::new(Vec3::new(-14.5, 0.0, z - 0.12), Vec3::new(-14.26, 2.6, z + 0.12), Color::hex("#5a4030")),
        );
    }
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
    let table = Vec3::new(9.0, 0.0, -7.0);
    gamble_table(sim, table);
    let cauldron = Vec3::new(-9.0, 0.0, 8.5);
    cauldron_station(sim, cauldron);
    // The captain's post by the portal: a banner and a brazier.
    let post = Vec3::new(3.6, 0.0, -13.2);
    let st = &mut sim.state;
    st.statics.add(
        &mut st.physics,
        Block::new(post + Vec3::new(1.0, 0.0, -0.08), post + Vec3::new(1.16, 3.4, 0.08), Color::hex("#4a3a2a")),
    );
    st.statics.add(
        &mut st.physics,
        Block::new(post + Vec3::new(1.16, 1.9, -0.03), post + Vec3::new(1.96, 3.3, 0.03), Color::hex("#8a2a24")),
    );
    brazier(sim, post + Vec3::new(-1.6, 0.0, 0.6));
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
    let folk = townsfolk(sim, table, cauldron, post);
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
    g.spots.push(Spot { kind: SpotKind::Vendor, name: "Hilda the Smith".into(), pos: smith_feet, reach: 3.2, info: Vec::new() });
    g.spots.push(Spot { kind: SpotKind::Stash, name: "Stash".into(), pos: chest, reach: 2.6, info: Vec::new() });
    g.spots.push(Spot { kind: SpotKind::Portal, name: "Portal".into(), pos: portal_at, reach: 2.8, info: Vec::new() });
    g.npcs.push(Npc::new(smith, NpcRole::Smith, "Hilda", std::f32::consts::FRAC_PI_2, anvil + Vec3::Y * 0.8));
    g.spots.push(Spot {
        kind: SpotKind::Gamble,
        name: "Odo the Gambler".into(),
        pos: table + Vec3::new(0.0, 0.0, 1.2),
        reach: 3.0,
        info: Vec::new(),
    });
    g.spots.push(Spot {
        kind: SpotKind::Alchemist,
        name: "Mother Wren".into(),
        pos: cauldron + Vec3::new(1.2, 0.0, -0.6),
        reach: 3.0,
        info: Vec::new(),
    });
    g.npcs.extend(folk);
    // The Hall of Heroes (south): every playable character on a pedestal.
    super::hall::build(sim, &mut g);
    g.restock(sim);
    g.say(Place::Town.name(), 2.5);
    sim.state.game = Some(g);
}

/// Townsfolk at work: the smith hammers (sparks on every blow), the gambler flips a coin, the
/// alchemist stirs, the captain keeps watch, villagers walk their rounds and the dog follows
/// you. Anyone working turns to greet the hero who comes close.
pub fn update_npcs(g: &mut Game, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
    let hero = g.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0);
    let mut greet = Vec::new();
    let deepest = g.hero.max_depth;
    for n in &mut g.npcs {
        let Some((feet, _)) = feet_of(sim, n.id) else { continue };
        let walker = matches!(n.role, NpcRole::Villager | NpcRole::Dog);
        let reach = if n.role == NpcRole::Dog { 2.2 } else { 4.5 };
        let near = hero.filter(|h| flat(*h - feet).length() < reach);
        let Some(ch) = sim.state.entities.get_mut(n.id).and_then(|e| e.character.as_mut()) else { continue };
        if let Some(h) = near {
            if !walker || ch.vel.length() < 0.3 {
                let to = flat(h - feet);
                ch.face = Some(to.x.atan2(to.z));
            }
            if !walker {
                ch.anim.set_action(MoveId::NONE, 0.0, 0.0, 1.0);
                n.t = 0.0;
            }
            if !n.greeted {
                n.greeted = true;
                if n.role != NpcRole::Dog {
                    // A bow, hand on the chest (a captured greeting on the upper body).
                    let bow = crate::clips::find_cached(TOWN_GREETING);
                    if bow != 0 {
                        ch.anim.play_clip(bow, crate::clips::UPPER | crate::clips::ONCE, 1.0);
                    }
                }
                let lines = n.role.lines();
                let i = sim.state.rng.below(lines.len() as u32) as usize;
                let mut line = lines[i].to_string();
                if n.role == NpcRole::Captain && i == 0 && deepest > 0 {
                    line = format!("You've been as deep as level {deepest}. The portal remembers.");
                }
                greet.push((feet, n.name.clone(), line));
            }
            if !walker {
                continue;
            }
        } else if hero.is_some_and(|h| flat(h - feet).length() > reach + 2.0) {
            n.greeted = false;
        }
        match n.role {
            NpcRole::Smith => {
                ch.face = Some(n.facing);
                let period = 1.5;
                let before = n.t;
                n.t = (n.t + dt) % period;
                ch.anim.set_action(MoveId::of("overhead"), n.t / period, 0.5, 1.0);
                if before < period * 0.5 && n.t >= period * 0.5 {
                    events.push(SimEvent::Strike { pos: n.work, power: 3.0, element: 0, crit: false });
                }
            }
            NpcRole::Gambler => {
                // A coin flick every few seconds, the coin spinning up and back into the hand.
                ch.face = Some(n.facing);
                let period = 3.2;
                n.t = (n.t + dt) % period;
                let flick = (n.t / 0.5).min(1.0);
                let mv = if n.t < 0.5 { MoveId::of("flick") } else { MoveId::NONE };
                ch.anim.set_action(mv, flick, 0.4, 1.0);
                if let Some(c) = n.prop.and_then(|c| sim.state.entities.get_mut(c)) {
                    let s = ((n.t - 0.2) / 1.0).clamp(0.0, 1.0);
                    c.pos = n.work + Vec3::Y * (4.0 * 1.1 * s * (1.0 - s));
                    c.rot = glam::Quat::from_rotation_x(s * 18.0);
                }
            }
            NpcRole::Alchemist => {
                // Stirring: the ladle sweeps one way, then back.
                ch.face = Some(n.facing);
                let period = 2.4;
                let before = n.t;
                n.t = (n.t + dt) % (period * 2.0);
                let half = n.t % period;
                ch.anim.set_action(MoveId::of("stir"), half / period, 0.05, if n.t < period { 1.0 } else { -1.0 });
                if before < period && n.t >= period {
                    events.push(SimEvent::Spell { pos: n.work, element: 4 });
                }
            }
            NpcRole::Captain => {
                ch.face = Some(n.facing + (n.t * 0.4).sin() * 0.5);
                n.t += dt;
            }
            NpcRole::Villager => {
                ch.anim.set_action(MoveId::NONE, 0.0, 0.0, 1.0);
                if n.wait > 0.0 {
                    // Standing about, looking around.
                    n.t += dt;
                    ch.face = Some(n.facing + (n.t * 0.9).sin() * 0.9);
                } else {
                    ch.face = None;
                }
            }
            NpcRole::Dog => {
                n.t += dt;
                if ch.vel.length() > 0.3 {
                    ch.face = None;
                }
            }
        }
    }
    for (at, name, line) in greet {
        g.float_text(at + Vec3::Y * 2.3, format!("{name}: \"{line}\""));
    }
}

/// Walkers decide where to go before characters move: villagers walk their round at a
/// stroll, the dog trots after the hero (and runs when left behind).
pub(crate) fn npc_inputs(
    g: &mut Game,
    sim: &mut Sim,
    dt: f32,
    out: &mut std::collections::BTreeMap<EntityId, crate::input::InputFrame>,
) {
    let hero = g.hero_id.and_then(|h| feet_of(sim, h)).map(|f| f.0);
    for n in &mut g.npcs {
        if !matches!(n.role, NpcRole::Villager | NpcRole::Dog) {
            continue;
        }
        let Some((feet, _)) = feet_of(sim, n.id) else { continue };
        let mut mv = Vec3::ZERO;
        let mut haste = -0.6;
        match n.role {
            NpcRole::Villager => {
                let blocked = hero.is_some_and(|h| flat(h - feet).length() < 1.3);
                if n.wait > 0.0 {
                    n.wait -= dt;
                } else if let Some(t) = n.route.get(n.leg).copied() {
                    let d = flat(t - feet);
                    if d.length() < 0.6 {
                        n.leg = (n.leg + 1) % n.route.len();
                        n.wait = sim.state.rng.range(1.5, 4.5);
                        n.facing = d.x.atan2(d.z);
                        n.t = 0.0;
                    } else if !blocked {
                        mv = d.normalize();
                    }
                }
            }
            _ => {
                if let Some(h) = hero {
                    let d = flat(h - feet);
                    let dist = d.length();
                    if dist > 2.4 {
                        mv = d / dist;
                    }
                    haste = if dist > 7.0 { 0.25 } else { -0.25 };
                }
            }
        }
        let base = sim.config.movement.speed.max(0.1);
        if let Some(ch) = sim.state.entities.get_mut(n.id).and_then(|e| e.character.as_mut()) {
            // A villager with a captured walk walks at its pace, so the feet keep to the ground.
            if n.role == NpcRole::Villager {
                if let Some(pace) = ch.puppet.as_deref().and_then(crate::clips::walk_pace) {
                    haste = pace / base - 1.0;
                }
            }
            ch.haste = haste;
        }
        out.insert(n.id, crate::input::InputFrame { move_dir: glam::Vec2::new(mv.x, mv.z), ..Default::default() });
    }
}

/// The card table: green cloth, cards, stools, a lantern.
fn gamble_table(sim: &mut Sim, at: Vec3) {
    let st = &mut sim.state;
    let wood = Color::hex("#5a3a24");
    st.statics.add(&mut st.physics, Block::new(at + Vec3::new(-0.9, 0.0, -0.45), at + Vec3::new(0.9, 0.75, 0.45), wood));
    st.statics.add(
        &mut st.physics,
        Block::new(at + Vec3::new(-0.85, 0.75, -0.4), at + Vec3::new(0.85, 0.78, 0.4), Color::hex("#2a6a3a")),
    );
    for (x, z) in [(-0.6f32, 0.1f32), (-0.35, -0.05), (0.4, 0.12)] {
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(x - 0.07, 0.78, z - 0.1), at + Vec3::new(x + 0.07, 0.8, z + 0.1), Color::hex("#f2ece0"))
                .with_flags(crate::statics::block_flags::GHOST),
        );
    }
    for x in [-0.5f32, 0.5] {
        st.statics.add(
            &mut st.physics,
            Block::new(at + Vec3::new(x - 0.2, 0.0, 0.75), at + Vec3::new(x + 0.2, 0.45, 1.1), wood.scale(0.8)),
        );
    }
    lamp(sim, at + Vec3::new(2.3, 0.0, 0.7));
}

/// The alchemist's cauldron over a fire, green and bubbling, and her shelf of bottles.
fn cauldron_station(sim: &mut Sim, at: Vec3) {
    let mut pot = Visual::new(Shape::Cylinder { half_height: 0.42, radius: 0.72 }, Color::hex("#2a2a30"));
    pot.look = Look::Lit;
    sim.spawn(Spawn::new("cauldron", at + Vec3::Y * 0.55).visual(pot).body(BodyKind::Fixed));
    let mut brew = Visual::new(Shape::Cylinder { half_height: 0.02, radius: 0.62 }, Color::hex("#6adf5a"));
    brew.look = Look::Unlit;
    brew.emissive = 1.2;
    brew.light = Some(Box::new(LightDef {
        color: "#7aff6a".into(),
        radius: 6.0,
        intensity: 1.6,
        pulse: 0.4,
        offset: Vec3::Y * 0.6,
        ..Default::default()
    }));
    brew.particles = Some(Box::new(EmitterDef {
        preset: "bubbles".into(),
        color: "#b0ffa0".into(),
        area: Vec3::new(0.45, 0.05, 0.45),
        rate: 10.0,
        ..Default::default()
    }));
    sim.spawn(Spawn::new("brew", at + Vec3::Y * 0.98).visual(brew));
    let mut fire = Visual::new(Shape::Sphere { radius: 0.05 }, Color::hex("#ff8a3a"));
    fire.look = Look::Unlit;
    fire.light =
        Some(Box::new(LightDef { color: "#ff8a3a".into(), radius: 4.0, intensity: 1.2, flicker: 0.6, ..Default::default() }));
    fire.particles =
        Some(Box::new(EmitterDef { preset: "fire".into(), area: Vec3::new(0.35, 0.02, 0.35), size: 0.6, ..Default::default() }));
    sim.spawn(Spawn::new("cauldron fire", at + Vec3::Y * 0.08).visual(fire));
    // A shelf with bottles.
    let st = &mut sim.state;
    let shelf = at + Vec3::new(-2.2, 0.0, 1.6);
    st.statics.add(
        &mut st.physics,
        Block::new(shelf + Vec3::new(-0.9, 0.0, -0.2), shelf + Vec3::new(0.9, 1.6, 0.2), Color::hex("#4a3424")),
    );
    let colors = ["#c84a4a", "#4a7ac8", "#6ac85a", "#c8a84a", "#a85ac8"];
    for (k, c) in colors.iter().enumerate() {
        let mut b = Visual::new(Shape::Cylinder { half_height: 0.12, radius: 0.07 }, Color::hex(c));
        b.look = Look::Unlit;
        b.emissive = 0.6;
        let x = -0.7 + k as f32 * 0.35;
        sim.spawn(Spawn::new("bottle", shelf + Vec3::new(x, 1.73, -0.25)).visual(b));
    }
}

/// The gambler, the alchemist, the captain, two villagers and the dog.
fn townsfolk(sim: &mut Sim, table: Vec3, cauldron: Vec3, post: Vec3) -> Vec<Npc> {
    use std::f32::consts::PI;
    let biped = |skin: &str, shirt: &str, pants: &str, scale: f32, weapon: WeaponKind, wcolor: &str| PuppetDef {
        scale,
        skin: skin.into(),
        shirt: shirt.into(),
        pants: pants.into(),
        shoes: "#2a2220".into(),
        weapon: crate::puppet::WeaponLook { kind: weapon, color: wcolor.into(), size: 0.75, ..Default::default() },
        ..Default::default()
    };
    let mut out = Vec::new();
    // Odo behind his table, facing the square (south).
    let odo_at = table + Vec3::new(0.0, 0.0, -0.95);
    let odo =
        sim.spawn_npc("Odo", odo_at, 0.0, biped("#d8a878", "#6a2a5a", "#2a2a3a", 0.98, WeaponKind::None, "#000000"), None, None);
    let mut coin = Visual::new(Shape::Cylinder { half_height: 0.012, radius: 0.07 }, Color::hex("#ffd24a"));
    coin.look = Look::Unlit;
    coin.emissive = 1.2;
    let coin_id = sim.spawn(Spawn::new("coin", odo_at + Vec3::new(0.25, 1.25, 0.25)).visual(coin));
    let mut n = Npc::new(odo, NpcRole::Gambler, "Odo", 0.0, odo_at + Vec3::new(0.25, 1.25, 0.25));
    n.prop = Some(coin_id);
    out.push(n);
    // Mother Wren at the cauldron, ladle in hand.
    let wren_at = cauldron + Vec3::new(1.05, 0.0, -0.35);
    let mut look = biped("#e8c0a0", "#3a5a3a", "#3a2a2a", 0.9, WeaponKind::Staff, "#6a4a2a");
    look.weapon.size = 0.55;
    look.gear.helm = crate::puppet::HelmKind::Hood;
    look.gear.helm_color = "#3e5e46".into();
    let wren = sim.spawn_npc("Mother Wren", wren_at, -PI * 0.5 - 0.3, look, None, None);
    out.push(Npc::new(wren, NpcRole::Alchemist, "Mother Wren", -PI * 0.5 - 0.3, cauldron + Vec3::Y * 1.0));
    // Captain Brannoc by the portal, spear grounded.
    let mut look = biped("#c99a7a", "#5a5a66", "#3a3a44", 1.1, WeaponKind::Spear, "#a8a8b0");
    look.gear.helm = crate::puppet::HelmKind::Cap;
    let cap = sim.spawn_npc("Captain Brannoc", post, 0.0, look, None, None);
    out.push(Npc::new(cap, NpcRole::Captain, "Captain Brannoc", 0.0, post));
    // Villagers on their rounds.
    let rounds = [
        (
            vec![Vec3::new(4.5, 0.0, 4.0), Vec3::new(-4.5, 0.0, 4.0), Vec3::new(-4.5, 0.0, -4.5), Vec3::new(4.5, 0.0, -4.5)],
            "Tomas",
            "#b08060",
            "#5a6a8a",
        ),
        (
            vec![
                Vec3::new(14.0, 0.0, 9.0),
                Vec3::new(3.0, 0.0, 10.5),
                Vec3::new(-7.0, 0.0, 4.0),
                Vec3::new(2.5, 0.0, -9.5),
                Vec3::new(13.0, 0.0, -4.0),
            ],
            "Elsie",
            "#e0b090",
            "#8a5a3a",
        ),
    ];
    for ((route, name, skin, shirt), (idle, walk)) in rounds.into_iter().zip(VILLAGER_CLIPS) {
        let start = route[0];
        // When they stop on their rounds they stand like people do (captured idles), and they
        // walk in their own styles (100STYLE captures) at those walks' own pace.
        let mut look = biped(skin, shirt, "#3a3028", 0.95, WeaponKind::None, "#000000");
        look.idle_clip = idle.into();
        look.walk_clip = walk.into();
        let id = sim.spawn_npc(name, start, 0.0, look, None, None);
        let mut n = Npc::new(id, NpcRole::Villager, name, 0.0, start);
        n.route = route;
        n.leg = 1;
        n.wait = 1.0;
        out.push(n);
    }
    // Biscuit the dog.
    let mut dog = PuppetDef::preset(crate::puppet::BodyPlan::Quadruped);
    dog.scale = 0.72;
    dog.skin = "#c89a5a".into();
    dog.shirt = "#d8aa6a".into();
    dog.accent = "#f2e2c8".into();
    dog.tail_length = 0.5;
    let id = sim.spawn_npc("Biscuit", Vec3::new(2.0, 0.0, 9.0), PI, dog, None, None);
    out.push(Npc::new(id, NpcRole::Dog, "Biscuit", PI, Vec3::ZERO));
    out
}

/// The greeting townsfolk give the hero (anim/*.json).
pub const TOWN_GREETING: &str = "MESH2MOTION/Greeting";
/// The villagers' idles and walks: Tomas strolls with his hands clasped behind his back, Elsie
/// bounces along swinging her arms.
pub const VILLAGER_CLIPS: [(&str, &str); 2] =
    [("MESH2MOTION/Idle Listening", "STYLE100/ArmsBehindBack_Walk"), ("MESH2MOTION/Idle_A", "STYLE100/Elated_Walk")];

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
    let d = super::data::data();
    let wave = ar.wave;
    let level = 1 + (wave - 1) / 2;
    // Every tenth wave: a boss (the designed ones in order, then generated ones forever).
    if wave.is_multiple_of(10) {
        let n = wave / 10 - 1;
        let key = match d.bosses.get(n as usize) {
            Some(b) => b.key.clone(),
            None => format!("gen:{}", sim.state.seed.wrapping_mul(7919).wrapping_add(wave as u64)),
        };
        g.spawn_boss(sim, &key, level + 1, ar.center + Vec3::new(0.0, 0.0, -9.0));
        return;
    }
    let packs = (1 + wave / 2).min(6);
    let total: u32 = FAMILIES.iter().map(|f| f.1).sum();
    for p in 0..packs {
        let rng = &mut sim.state.rng;
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let center = ar.center + Vec3::new(ang.cos(), 0.0, ang.sin()) * rng.range(12.0, 17.0);
        let rarity = if wave.is_multiple_of(5) && p == 0 {
            Rarity::Rare
        } else if wave >= 3 && rng.f32() < 0.25 {
            Rarity::Magic
        } else {
            Rarity::Normal
        };
        // From wave 4 on, some packs are newly generated creatures.
        let generated = wave >= 4 && rng.f32() < 0.35;
        let (spec, pack_k, big) = if generated {
            let seed = sim.state.rng.next_u32() as u64 | ((wave as u64) << 32);
            match super::genome::Genome::generate(&d, seed, level, &Default::default()) {
                Ok(gn) => {
                    let big = gn.puppet.scale > 1.3 || gn.archetype == "tank";
                    (gn.spec(&d), gn.pack, big)
                }
                Err(_) => continue,
            }
        } else {
            let rng = &mut sim.state.rng;
            let mut pick = rng.below(total);
            let fam = FAMILIES
                .iter()
                .find(|f| {
                    if pick < f.1 {
                        true
                    } else {
                        pick -= f.1;
                        false
                    }
                })
                .map(|f| f.0)
                .unwrap_or("ghoul");
            let Some(spec) = d.family(fam).map(|f| f.spec()) else { continue };
            let k = if fam == "skitterer" { 2.0 } else { 1.0 };
            (spec, k, fam == "bonecrusher")
        };
        let count =
            if big || rarity == Rarity::Rare { 1 } else { (((2 + (wave / 2).min(4)) as f32) * pack_k).round().max(1.0) as u32 };
        let pack = g.next_pack;
        g.next_pack += 1;
        for i in 0..count {
            let a = i as f32 * 2.4;
            let at = center + Vec3::new(a.cos(), 0.0, a.sin()) * (0.8 + i as f32 * 0.35);
            let at = Vec3::new(at.x.clamp(-18.5, 18.5), 0.0, at.z.clamp(-18.5, 18.5));
            if let Some(id) = super::spawn_spec_into(sim, g, &spec, level, rarity, at, pack) {
                if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
                    b.aggro = true;
                }
            }
        }
    }
}
