//! The level mechanics at work. Each one is a handful of pieces (`Feature`s) placed by the
//! level builder and a rule run every tick: shrines hand out boons, kegs blow up (and set each
//! other off), spike plates fire on a beat, rift gates fold the map, windways carry everyone,
//! totems ward monsters, lava burns, ice slides (and shatters frozen monsters), floors crumble
//! (whoever falls is gone), wells light the dark, cursed chests call their keepers, and time
//! bubbles slow monsters to a crawl. Monsters obey the same rules as the hero, which is what
//! makes the mechanics worth exploiting.

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::combat::{Buff, BuffKind, Damage, Effect, EffectKind, Rarity, Team};
use super::levelgen::{Layout, Rect, RoomRole};
use super::stats::{Mods, Stat};
use super::world::{LevelPlan, Mechanic, Mood, PackInfo};
use super::{Game, Place, TeleShape, Telegraph, feet_of, flat, refresh_hero};
use crate::color::Color;
use crate::entity::{EntityId, Spawn};
use crate::frame::SimEvent;
use crate::fxdef::{EmitterDef, LightDef};
use crate::rng::Rng;
use crate::shape::{Look, Shape, Visual};
use crate::sim::Sim;

/// What a shrine grants (for 15 s; walking through the same kind again adds time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boon {
    Might,
    Fury,
    Swiftness,
    Warding,
    Fortune,
    Clarity,
}

impl Boon {
    pub const ALL: [Boon; 6] = [Boon::Might, Boon::Fury, Boon::Swiftness, Boon::Warding, Boon::Fortune, Boon::Clarity];
    pub fn name(self) -> &'static str {
        match self {
            Boon::Might => "Might",
            Boon::Fury => "Fury",
            Boon::Swiftness => "Swiftness",
            Boon::Warding => "Warding",
            Boon::Fortune => "Fortune",
            Boon::Clarity => "Clarity",
        }
    }
    pub fn color(self) -> &'static str {
        match self {
            Boon::Might => "#ff5a3a",
            Boon::Fury => "#ffa42a",
            Boon::Swiftness => "#6aff9a",
            Boon::Warding => "#6ab8ff",
            Boon::Fortune => "#ffe25a",
            Boon::Clarity => "#c07aff",
        }
    }
    pub fn mods(self) -> Mods {
        let m = Mods::default();
        match self {
            Boon::Might => m.with(Stat::DamageMore, 50.0),
            Boon::Fury => m.with(Stat::AttackSpeed, 35.0).with(Stat::CastSpeed, 35.0),
            Boon::Swiftness => m.with(Stat::MoveSpeed, 35.0).with(Stat::DodgeRecovery, 60.0),
            Boon::Warding => m.with(Stat::DamageTaken, 40.0).with(Stat::LifeLeech, 3.0),
            Boon::Fortune => m.with(Stat::ItemRarity, 120.0).with(Stat::GoldFind, 120.0),
            Boon::Clarity => m.with(Stat::CooldownRecovery, 60.0).with(Stat::ManaCost, 60.0),
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            Boon::Might => "50% more damage",
            Boon::Fury => "35% faster attacks and casts",
            Boon::Swiftness => "35% faster, quicker dodges",
            Boon::Warding => "40% less damage taken, life leech",
            Boon::Fortune => "far better loot",
            Boon::Clarity => "60% faster cooldowns, cheaper skills",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChestState {
    Closed,
    Waves,
    Open,
}

/// One piece of a mechanic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Shrine {
        boon: Boon,
        used: bool,
    },
    /// The keg's actor is the feature's entity; `fuse` > 0 counts down to a chain blast.
    Keg {
        fuse: f32,
    },
    Spikes {
        half: f32,
        period: f32,
        phase: f32,
        up: bool,
        spikes: Vec<EntityId>,
    },
    /// `to`: the partner gate (feature index).
    Gate {
        to: usize,
        cd: f32,
    },
    Wind {
        min: Vec2,
        max: Vec2,
        dir: Vec3,
        chevrons: Vec<EntityId>,
    },
    /// A monster-team actor (the feature's entity) warding monsters around it.
    Totem,
    Lava {
        radius: f32,
        tick: f32,
    },
    Ice {
        min: Vec2,
        max: Vec2,
    },
    Crumble {
        min: Vec2,
        max: Vec2,
    },
    Well {
        lit: bool,
    },
    Chest {
        state: ChestState,
        wave: u32,
        keepers: Vec<EntityId>,
        spot: usize,
        t: f32,
    },
    Bubble {
        radius: f32,
        hands: Vec<EntityId>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub kind: FeatureKind,
    pub pos: Vec3,
    pub room: usize,
    /// The main entity (visual or actor) and extra decoration that goes with it.
    pub entity: Option<EntityId>,
    pub deco: Vec<EntityId>,
}

/// A level in progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelState {
    pub depth: u32,
    pub label: String,
    pub name: String,
    pub about: String,
    pub endless: bool,
    pub mechanics: Vec<Mechanic>,
    pub mood: Mood,
    pub accent: String,
    pub monster_level: u32,
    pub pack: PackInfo,
    pub seed: u64,
    pub layout: Layout,
    pub features: Vec<Feature>,
    /// Rooms the hero has been in (the map's fog).
    pub seen: Vec<bool>,
    pub boss: Option<EntityId>,
    pub boss_name: String,
    pub has_boss: bool,
    pub exit_open: bool,
    pub exit_spot: usize,
    pub seal: Option<EntityId>,
    /// Last firm ground under the hero (falls put you back here).
    pub safe: Vec3,
    pub lantern: Option<EntityId>,
    pub time: f32,
    pub pulse: f32,
    /// Where one can walk (built on first use from the level's blocks), and the way to the
    /// hero from everywhere near them (rebuilt when the hero moves to another cell).
    #[serde(skip)]
    pub nav: Option<std::sync::Arc<crate::nav::NavGrid>>,
    #[serde(skip)]
    pub flow: Option<std::sync::Arc<crate::nav::FlowField>>,
}

impl LevelState {
    pub fn new(plan: &LevelPlan, layout: Layout, seed: u64, exit_spot: usize) -> Self {
        let n = layout.rooms.len();
        Self {
            depth: plan.depth,
            label: plan.label(),
            name: plan.name.clone(),
            about: plan.about.clone(),
            endless: plan.endless,
            mechanics: plan.mechanics.clone(),
            mood: plan.mood(),
            accent: plan.theme.accent.clone(),
            monster_level: plan.monster_level,
            pack: PackInfo::of(plan),
            seed,
            layout,
            features: Vec::new(),
            seen: vec![false; n],
            boss: None,
            boss_name: String::new(),
            has_boss: plan.boss.is_some(),
            exit_open: true,
            exit_spot,
            seal: None,
            safe: Vec3::ZERO,
            lantern: None,
            time: 0.0,
            pulse: 0.0,
            nav: None,
            flow: None,
        }
    }

    /// The level's navigation grid (built once from its blocks and props).
    pub fn nav_grid(
        &mut self,
        sim: &Sim,
        actors: &std::collections::BTreeMap<EntityId, super::combat::Actor>,
    ) -> std::sync::Arc<crate::nav::NavGrid> {
        if let Some(n) = &self.nav {
            return n.clone();
        }
        let b = self.layout.bounds();
        let grid = crate::nav::NavGrid::build(sim, b.min - Vec2::splat(3.0), b.max + Vec2::splat(3.0), 0.5, 0.45, &|id| {
            actors.contains_key(&id)
        });
        let grid = std::sync::Arc::new(grid);
        self.nav = Some(grid.clone());
        grid
    }

    /// The way to the hero from everywhere within 40 m (cached per hero cell).
    pub fn hero_flow(
        &mut self,
        sim: &Sim,
        actors: &std::collections::BTreeMap<EntityId, super::combat::Actor>,
        hero: Vec3,
    ) -> Option<std::sync::Arc<crate::nav::FlowField>> {
        let grid = self.nav_grid(sim, actors);
        let cell = grid.nearest_open(hero)?;
        if let Some(f) = self.flow.as_ref().filter(|f| f.goal == cell) {
            return Some(f.clone());
        }
        let f = std::sync::Arc::new(grid.flow(hero, 40.0)?);
        self.flow = Some(f.clone());
        Some(f)
    }

    fn inside(min: Vec2, max: Vec2, p: Vec3) -> bool {
        p.x >= min.x && p.x <= max.x && p.z >= min.y && p.z <= max.y
    }

    /// Standing on ice?
    pub fn on_ice(&self, p: Vec3) -> bool {
        p.y > -0.4 && self.features.iter().any(|f| matches!(f.kind, FeatureKind::Ice { min, max } if Self::inside(min, max, p)))
    }

    pub fn on_crumble(&self, p: Vec3) -> bool {
        self.features.iter().any(|f| matches!(f.kind, FeatureKind::Crumble { min, max } if Self::inside(min, max, p)))
    }

    /// The room a point is in (rooms only, not corridors).
    pub fn room_at(&self, p: Vec3) -> Option<usize> {
        let q = Vec2::new(p.x, p.z);
        self.layout.rooms.iter().position(|r| r.rect.shrink(-0.5).contains(q))
    }
}

/// Damage of one element for a level hazard.
fn hazard(element: usize, amount: f32, source: Option<EntityId>, knockback: f32) -> Damage {
    let mut a = [0.0; 5];
    a[element] = amount;
    Damage {
        amount: a,
        crit: false,
        ailment: [0.0; 5],
        ailment_mult: 1.0,
        knockback,
        source,
        skill: 0,
        attack: false,
        melee: false,
    }
}

/// How hard a hazard hits a monster: a share of its life, less for the strong.
fn monster_share(r: Rarity) -> f32 {
    match r {
        Rarity::Normal => 1.0,
        Rarity::Magic => 0.8,
        Rarity::Rare => 0.45,
        Rarity::Unique => 0.1,
    }
}

/// Adds a timed buff, or tops up the one already there (no recompute needed then).
fn give_buff(g: &mut Game, sim: &mut Sim, id: EntityId, name: &str, kind: BuffKind, time: f32, mods: Mods, cap: f32) {
    let Some(a) = g.actors.get_mut(&id) else { return };
    if let Some(b) = a.buffs.iter_mut().find(|b| b.name == name) {
        b.time = (b.time.max(0.0) + if cap > time { time } else { 0.0 }).clamp(time, cap.max(time));
        return;
    }
    a.buffs.push(Buff { kind, name: name.to_string(), time, mods });
    if Some(id) == g.hero_id {
        refresh_hero(sim, g, false);
    } else {
        a.recompute();
    }
}

fn set_pos(sim: &mut Sim, id: EntityId, p: Vec3, rot: Option<Quat>) {
    if let Some(e) = sim.state.entities.get_mut(id) {
        e.pos = p;
        if let Some(r) = rot {
            e.rot = r;
        }
    }
}

/// Every tick in a level.
pub(crate) fn update_level(g: &mut Game, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
    let Some(mut lv) = g.level.take() else { return };
    lv.time += dt;
    lv.pulse += dt;
    let hero = g.hero_id.and_then(|h| {
        let a = g.actors.get(&h)?;
        if a.dead {
            return None;
        }
        Some((h, feet_of(sim, h)?.0))
    });
    if let Some((_, hf)) = hero {
        if let Some(r) = lv.room_at(hf) {
            lv.seen[r] = true;
        }
    }
    falls(g, sim, &mut lv, events);
    if let Some((h, hf)) = hero {
        let grounded = sim.state.entities.get(h).and_then(|e| e.character.as_ref()).is_some_and(|c| c.grounded);
        if grounded && hf.y > -0.15 && !lv.on_crumble(hf) {
            lv.safe = hf + Vec3::Y * 0.05;
        }
    }
    if lv.mechanics.contains(&Mechanic::FrozenLake) {
        let ids: Vec<EntityId> = g.actors.keys().copied().collect();
        for id in ids {
            let Some((f, _)) = feet_of(sim, id) else { continue };
            let ice = lv.on_ice(f);
            if let Some(ch) = sim.state.entities.get_mut(id).and_then(|e| e.character.as_mut()) {
                ch.slide = if ice { 0.9 } else { 0.0 };
            }
        }
    }
    if lv.mechanics.contains(&Mechanic::LightlessDeep) {
        lantern(sim, &mut lv, hero.map(|h| h.1));
    }
    let pulse = lv.pulse > 1.6;
    if pulse {
        lv.pulse = 0.0;
    }
    for i in 0..lv.features.len() {
        feature(g, sim, &mut lv, i, hero, dt, pulse, events);
    }
    // The boss: when it falls, the way down opens.
    if let Some(b) = lv.boss {
        if g.actors.get(&b).is_none_or(|a| a.dead) {
            lv.boss = None;
            lv.exit_open = true;
            if let Some(s) = lv.seal.take() {
                sim.despawn(s);
            }
            let at = g.spots.get(lv.exit_spot).map(|s| s.pos).unwrap_or(Vec3::ZERO);
            // A pillar of light over the way down, seen from across the level.
            let mut v = Visual::new(Shape::Cylinder { half_height: 7.0, radius: 0.35 }, Color::hex("#ffe2a0"));
            v.look = Look::Unlit;
            v.emissive = 1.6;
            v.light = Some(Box::new(LightDef {
                color: "#ffd27a".into(),
                radius: 14.0,
                intensity: 2.4,
                pulse: 0.4,
                ..Default::default()
            }));
            v.particles = Some(Box::new(EmitterDef {
                preset: "magic".into(),
                color: "#fff0c0".into(),
                area: Vec3::new(0.5, 3.0, 0.5),
                rate: 30.0,
                ..Default::default()
            }));
            sim.spawn(Spawn::new("~beacon", at + Vec3::Y * 7.0).visual(v));
            events.push(SimEvent::Blast { pos: at, element: 3 });
            g.say(format!("{} is slain. The way down is open.", lv.boss_name), 3.5);
        }
    }
    g.level = Some(lv);
}

/// Whoever falls through the floor: monsters are gone (the hero gets the credit), the hero
/// climbs back out where the ground was firm, a little hurt.
fn falls(g: &mut Game, sim: &mut Sim, lv: &mut LevelState, events: &mut Vec<SimEvent>) {
    let ids: Vec<EntityId> = g.actors.iter().filter(|(_, a)| !a.dead).map(|(id, _)| *id).collect();
    for id in ids {
        let Some((f, _)) = feet_of(sim, id) else { continue };
        if f.y > -2.5 {
            continue;
        }
        if Some(id) == g.hero_id {
            sim.set_position(id, lv.safe);
            let a = g.actors.get_mut(&id).unwrap();
            let loss = a.sheet.life_max * 0.1;
            a.life = (a.life - loss).max(1.0);
            a.iframes = a.iframes.max(1.0);
            g.float_text(lv.safe + Vec3::Y * 2.2, "Fell!");
            events.push(SimEvent::Land { pos: lv.safe, speed: 6.0 });
        } else {
            sim.set_position(id, Vec3::new(f.x, 0.05, f.z));
            let hero = g.hero_id;
            if let Some(a) = g.actors.get_mut(&id) {
                if a.team == Team::Monster {
                    a.last_hit = hero;
                }
            }
            g.float_text(Vec3::new(f.x, 1.8, f.z), "Fell!");
            g.kill(sim, id, events);
        }
    }
}

fn lantern(sim: &mut Sim, lv: &mut LevelState, hero: Option<Vec3>) {
    let Some(hf) = hero else { return };
    let at = hf + Vec3::Y * 2.6;
    match lv.lantern {
        Some(id) if sim.state.entities.get(id).is_some() => set_pos(sim, id, at, None),
        _ => {
            let mut v = Visual::new(Shape::Sphere { radius: 0.01 }, Color::hex("#000000"));
            v.look = Look::Unlit;
            v.light = Some(Box::new(LightDef {
                color: "#ffd8a0".into(),
                radius: 11.0,
                intensity: 2.3,
                flicker: 0.15,
                shadows: true,
                ..Default::default()
            }));
            lv.lantern = Some(sim.spawn(Spawn::new("~lantern", at).visual(v)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn feature(
    g: &mut Game,
    sim: &mut Sim,
    lv: &mut LevelState,
    i: usize,
    hero: Option<(EntityId, Vec3)>,
    dt: f32,
    pulse: bool,
    events: &mut Vec<SimEvent>,
) {
    let pos = lv.features[i].pos;
    let near = |r: f32| hero.filter(|(_, h)| flat(*h - pos).length() < r && (h.y - pos.y).abs() < 2.0);
    match lv.features[i].kind.clone() {
        FeatureKind::Shrine { boon, used } => {
            if used {
                return;
            }
            let Some((h, _)) = near(1.7) else { return };
            lv.features[i].kind = FeatureKind::Shrine { boon, used: true };
            give_buff(g, sim, h, boon.name(), BuffKind::Custom, 15.0, boon.mods(), 30.0);
            g.float_text(pos + Vec3::Y * 2.4, format!("Shrine of {}: {}", boon.name(), boon.text()));
            events.push(SimEvent::Spell { pos, element: 3 });
            g.effects.push(ring(pos, 3.0, Color::hex(boon.color()).0, 0.6));
            if let Some(e) = lv.features[i].entity.and_then(|e| sim.state.entities.get_mut(e)) {
                if let Some(v) = e.visual.as_mut() {
                    v.emissive = 0.15;
                    v.light = None;
                    v.particles = None;
                    v.color = Color::hex("#5a5a60");
                }
            }
        }
        FeatureKind::Keg { fuse } => {
            let Some(id) = lv.features[i].entity else { return };
            if g.actors.get(&id).is_none_or(|a| a.dead) {
                explode(g, sim, lv, i, events);
            } else if fuse > 0.0 {
                if fuse - dt <= 0.0 {
                    explode(g, sim, lv, i, events);
                } else {
                    lv.features[i].kind = FeatureKind::Keg { fuse: fuse - dt };
                }
            }
        }
        FeatureKind::Spikes { half, period, phase, up, spikes } => {
            let cyc = (lv.time + phase).rem_euclid(period);
            let rise = period - 0.5;
            let now_up = cyc >= rise;
            let warn = cyc >= rise - 0.9;
            if now_up && !up {
                // Spikes! Everyone on the plate.
                let hit: Vec<EntityId> = g
                    .actors
                    .iter()
                    .filter(|(_, a)| !a.dead && (a.team == Team::Hero || (a.team == Team::Monster && a.family != "totem")))
                    .map(|(id, _)| *id)
                    .collect();
                for id in hit {
                    let Some((f, _)) = feet_of(sim, id) else { continue };
                    let d = f - pos;
                    if d.x.abs() > half + 0.3 || d.z.abs() > half + 0.3 || f.y > 1.0 {
                        continue;
                    }
                    strike(g, sim, id, 0, 0.1, 0.4, pos, 2.5, events);
                }
                events.push(SimEvent::Slam { pos, radius: half });
            }
            let h = if now_up {
                1.0
            } else if warn {
                0.12
            } else {
                0.0
            };
            for (k, s) in spikes.iter().enumerate() {
                let (a, b) = ((k / 3) as f32 - 1.0, (k % 3) as f32 - 1.0);
                set_pos(sim, *s, pos + Vec3::new(a * 1.2, -0.6 + h * 0.9, b * 1.2), None);
            }
            lv.features[i].kind = FeatureKind::Spikes { half, period, phase, up: now_up, spikes };
        }
        FeatureKind::Gate { to, cd } => {
            let cd = (cd - dt).max(0.0);
            lv.features[i].kind = FeatureKind::Gate { to, cd };
            if cd > 0.0 || to >= lv.features.len() {
                return;
            }
            let Some((h, _)) = near(1.0) else { return };
            let dest = lv.features[to].pos;
            let room = lv.layout.rooms[lv.features[to].room].rect.center();
            let out = (Vec2::new(room.x, room.y) - Vec2::new(dest.x, dest.z)).normalize_or(Vec2::X);
            let land = dest + Vec3::new(out.x, 0.0, out.y) * 2.4 + Vec3::Y * 0.05;
            sim.set_position(h, land);
            lv.safe = land;
            lv.features[i].kind = FeatureKind::Gate { to, cd: 1.5 };
            if let FeatureKind::Gate { cd, .. } = &mut lv.features[to].kind {
                *cd = 1.5;
            }
            events.push(SimEvent::Spell { pos, element: 3 });
            events.push(SimEvent::Spell { pos: dest, element: 3 });
            g.effects.push(ring(dest, 2.5, [0.7, 0.45, 1.0], 0.5));
            lv.seen[lv.features[to].room] = true;
        }
        FeatureKind::Wind { min, max, dir, chevrons } => {
            // Chevrons stream down the strip with the wind.
            let c = (min + max) * 0.5;
            let along_x = dir.x.abs() > 0.5;
            let len = if along_x { max.x - min.x } else { max.y - min.y };
            let n = (chevrons.len() / 2).max(1);
            let yaw = dir.x.atan2(dir.z);
            for k in 0..n {
                let s = (lv.time * 4.5 + k as f32 * len / n as f32).rem_euclid(len) - len * 0.5;
                let tip = Vec3::new(c.x, 0.03, c.y) + dir * s;
                for (j, sd) in [-1.0f32, 1.0].iter().enumerate() {
                    let r = Quat::from_rotation_y(yaw + sd * 0.7);
                    let arm = r * Vec3::Z;
                    if let Some(id) = chevrons.get(k * 2 + j) {
                        set_pos(sim, *id, tip - arm * 0.45, Some(r));
                    }
                }
            }
        }
        FeatureKind::Totem => {
            let Some(id) = lv.features[i].entity else { return };
            if g.actors.get(&id).is_none_or(|a| a.dead) {
                for d in std::mem::take(&mut lv.features[i].deco) {
                    sim.despawn(d);
                }
                lv.features[i].entity = None;
                events.push(SimEvent::Break { pos: pos + Vec3::Y });
                return;
            }
            let near: Vec<EntityId> = g
                .actors
                .iter()
                .filter(|(o, a)| **o != id && !a.dead && a.team == Team::Monster && a.family != "totem")
                .filter_map(|(o, _)| {
                    let (f, _) = feet_of(sim, *o)?;
                    (flat(f - pos).length() < 8.0).then_some(*o)
                })
                .collect();
            for o in near {
                give_buff(g, sim, o, "Warded", BuffKind::Ward, 0.5, Mods::default().with(Stat::DamageTaken, 60.0), 0.5);
            }
            if pulse {
                g.effects.push(ring(pos, 8.0, Color::hex(&lv.accent).0, 0.9));
            }
            if let Some(eye) = lv.features[i].deco.last() {
                set_pos(sim, *eye, pos + Vec3::Y * (2.35 + 0.08 * (lv.time * 2.0).sin()), None);
            }
        }
        FeatureKind::Lava { radius, tick } => {
            let t = tick + dt;
            if t < 0.25 {
                lv.features[i].kind = FeatureKind::Lava { radius, tick: t };
                return;
            }
            lv.features[i].kind = FeatureKind::Lava { radius, tick: 0.0 };
            let ids: Vec<EntityId> =
                g.actors.iter().filter(|(_, a)| !a.dead && a.team != Team::Neutral).map(|(id, _)| *id).collect();
            for id in ids {
                let Some((f, _)) = feet_of(sim, id) else { continue };
                if flat(f - pos).length() < radius && f.y < 0.6 {
                    strike(g, sim, id, 1, 0.04, 0.09, f, 0.0, events);
                }
            }
        }
        FeatureKind::Ice { .. } | FeatureKind::Crumble { .. } => {}
        FeatureKind::Well { lit } => {
            if lit {
                return;
            }
            let Some((h, _)) = near(1.9) else { return };
            lv.features[i].kind = FeatureKind::Well { lit: true };
            if let Some(e) = lv.features[i].entity.and_then(|e| sim.state.entities.get_mut(e)) {
                if let Some(v) = e.visual.as_mut() {
                    v.color = Color::hex("#ffb05a");
                    v.emissive = 2.6;
                    v.light = Some(Box::new(LightDef {
                        color: "#ff9a4a".into(),
                        radius: 15.0,
                        intensity: 2.6,
                        flicker: 0.4,
                        offset: Vec3::Y * 1.2,
                        ..Default::default()
                    }));
                    v.particles = Some(Box::new(EmitterDef {
                        preset: "fire".into(),
                        size: 1.4,
                        area: Vec3::new(0.4, 0.05, 0.4),
                        ..Default::default()
                    }));
                }
            }
            // The flare burns the monsters around it; the hero burns brighter.
            let ids: Vec<EntityId> =
                g.actors.iter().filter(|(_, a)| !a.dead && a.team == Team::Monster).map(|(id, _)| *id).collect();
            for id in ids {
                let Some((f, _)) = feet_of(sim, id) else { continue };
                if flat(f - pos).length() < 7.5 {
                    strike(g, sim, id, 1, 0.0, 0.4, pos, 3.0, events);
                }
            }
            let kindled = Mods::default().with(Stat::DamageMore, 25.0).with(Stat::MoveSpeed, 10.0);
            give_buff(g, sim, h, "Kindled", BuffKind::Custom, 30.0, kindled, 45.0);
            g.effects.push(ring(pos, 7.5, [1.0, 0.6, 0.25], 0.6));
            events.push(SimEvent::Blast { pos, element: 1 });
            g.float_text(pos + Vec3::Y * 2.0, "The well blazes: Kindled");
        }
        FeatureKind::Chest { state, wave, keepers, spot, t } => chest(g, sim, lv, i, state, wave, keepers, spot, t, dt, events),
        FeatureKind::Bubble { radius, hands } => {
            for (k, hnd) in hands.iter().enumerate() {
                let (speed, len) = if k == 0 { (0.6, radius * 0.55) } else { (0.07, radius * 0.35) };
                let a = lv.time * speed;
                let r = Quat::from_rotation_y(a);
                set_pos(sim, *hnd, pos + Vec3::Y * 0.06 + r * Vec3::Z * len * 0.5, Some(r));
            }
            let inside = |f: Vec3| flat(f - pos).length() < radius && f.y < 2.0;
            let ids: Vec<EntityId> = g.actors.iter().filter(|(_, a)| !a.dead).map(|(id, _)| *id).collect();
            for id in ids {
                let Some((f, _)) = feet_of(sim, id) else { continue };
                if !inside(f) {
                    continue;
                }
                if Some(id) == g.hero_id {
                    let m = Mods::default().with(Stat::MoveSpeed, 30.0).with(Stat::AttackSpeed, 20.0).with(Stat::CastSpeed, 20.0);
                    give_buff(g, sim, id, "Quickened", BuffKind::Haste, 0.3, m, 0.3);
                } else if let Some(a) = g.actors.get_mut(&id).filter(|a| a.team == Team::Monster) {
                    // Slow time: they move, swing and recover at a crawl.
                    a.ailments.chill = (a.ailments.chill.0.max(0.65), a.ailments.chill.1.max(0.25));
                    let lag = dt * if a.rarity == Rarity::Unique { 0.3 } else { 0.6 };
                    if let Some(c) = a.cast.as_mut() {
                        c.t = (c.t - lag).max(0.0);
                    }
                    for cd in &mut a.cooldowns {
                        if cd.1 > 0.0 {
                            cd.1 += lag;
                        }
                    }
                }
            }
            for s in &mut g.shots {
                if s.team == Team::Monster && s.orbit.is_none() && inside(s.pos) {
                    s.pos -= s.vel * dt * 0.65;
                    s.life += dt * 0.65;
                }
            }
        }
    }
}

/// A hazard hit: monsters lose `share` (times their rarity factor) of their life with the hero
/// credited; the hero loses `hero` of theirs. Element index as in `Damage::amount`.
#[allow(clippy::too_many_arguments)]
fn strike(
    g: &mut Game,
    sim: &mut Sim,
    id: EntityId,
    element: usize,
    hero: f32,
    share: f32,
    from: Vec3,
    knockback: f32,
    events: &mut Vec<SimEvent>,
) {
    let Some(a) = g.actors.get(&id) else { return };
    let (amount, source) = match a.team {
        Team::Hero => (a.sheet.life_max * hero, None),
        Team::Monster => (a.sheet.life_max * share * monster_share(a.rarity) / (1.0 - resist(a, element)), g.hero_id),
        Team::Neutral => (a.sheet.life_max, g.hero_id),
    };
    if amount <= 0.0 {
        return;
    }
    let dmg = hazard(element, amount, source, knockback);
    g.hit(sim, id, &dmg, from, events);
}

/// The target's resistance to an element (hazards pay it back so they hit as advertised).
fn resist(a: &super::combat::Actor, element: usize) -> f32 {
    if element == 0 { 0.0 } else { (a.sheet.res[element] / 100.0).clamp(0.0, 0.75) }
}

fn ring(pos: Vec3, radius: f32, color: [f32; 3], dur: f32) -> Effect {
    Effect { kind: EffectKind::Ring, pos, radius, t: 0.0, dur, color, team: Team::Neutral, dmg: None, dir: Vec3::X, angle: 360.0 }
}

/// A keg goes up: everything near is hurt (monsters badly), nearby kegs catch.
fn explode(g: &mut Game, sim: &mut Sim, lv: &mut LevelState, i: usize, events: &mut Vec<SimEvent>) {
    let pos = lv.features[i].pos;
    if let Some(id) = lv.features[i].entity.take() {
        g.actors.remove(&id);
        sim.despawn(id);
    }
    for d in std::mem::take(&mut lv.features[i].deco) {
        sim.despawn(d);
    }
    lv.features[i].kind = FeatureKind::Keg { fuse: -1.0 };
    let radius = 4.5;
    let ids: Vec<EntityId> = g.actors.iter().filter(|(_, a)| !a.dead).map(|(id, _)| *id).collect();
    for id in ids {
        let Some((f, _)) = feet_of(sim, id) else { continue };
        if flat(f - pos).length() > radius {
            continue;
        }
        if g.actors[&id].team == Team::Neutral {
            // Another keg: it goes up a moment later.
            if let Some(k) = lv.features.iter_mut().find(|k| k.entity == Some(id)) {
                if let FeatureKind::Keg { fuse } = &mut k.kind {
                    if *fuse == 0.0 {
                        *fuse = 0.14;
                    }
                }
            }
            continue;
        }
        strike(g, sim, id, 1, 0.12, 0.75, pos, 4.0, events);
    }
    events.push(SimEvent::Explosion { pos, radius });
    events.push(SimEvent::Blast { pos, element: 1 });
    g.effects.push(ring(pos, radius, [1.0, 0.55, 0.2], 0.45));
    g.effects.push(Effect {
        kind: EffectKind::Burst,
        pos: pos + Vec3::Y * 0.5,
        radius: 2.0,
        t: 0.0,
        dur: 0.3,
        color: [1.0, 0.6, 0.25],
        team: Team::Neutral,
        dmg: None,
        dir: Vec3::X,
        angle: 360.0,
    });
    g.shake = (g.shake + 0.5 * sim.config.difficulty.shake).min(1.5);
}

#[allow(clippy::too_many_arguments)]
fn chest(
    g: &mut Game,
    sim: &mut Sim,
    lv: &mut LevelState,
    i: usize,
    state: ChestState,
    mut wave: u32,
    mut keepers: Vec<EntityId>,
    spot: usize,
    mut t: f32,
    dt: f32,
    events: &mut Vec<SimEvent>,
) {
    if state != ChestState::Waves {
        return;
    }
    let pos = lv.features[i].pos;
    keepers.retain(|k| g.actors.get(k).is_some_and(|a| !a.dead));
    t -= dt;
    let mut state = state;
    if keepers.is_empty() && t <= 0.0 {
        if wave < 3 {
            wave += 1;
            let level = lv.monster_level + 1;
            let mut rng = Rng::new(sim.state.rng.next_u32() as u64 ^ lv.seed);
            let packs: &[Rarity] = match wave {
                1 => &[Rarity::Normal, Rarity::Magic],
                2 => &[Rarity::Magic, Rarity::Magic, Rarity::Rare],
                _ => &[Rarity::Rare, Rarity::Rare, Rarity::Magic],
            };
            for (k, r) in packs.iter().enumerate() {
                let a = k as f32 / packs.len() as f32 * std::f32::consts::TAU + rng.range(0.0, 1.0);
                let at = pos + Vec3::new(a.cos(), 0.0, a.sin()) * 6.0 + Vec3::Y * 0.1;
                let at = clamp_to_room(&lv.layout, lv.features[i].room, at);
                keepers.extend(super::world::spawn_pack(g, sim, &mut rng, &lv.pack, at, level, *r, true));
                events.push(SimEvent::Spell { pos: at, element: 4 });
            }
            g.say(format!("The keepers come: wave {wave} of 3"), 2.0);
            t = 1.0;
        } else {
            state = ChestState::Open;
            open_chest_loot(g, sim, lv, i, events);
            if let Some(s) = g.spots.get_mut(spot) {
                s.name = "Opened Chest".into();
                s.reach = 0.0;
            }
        }
    }
    lv.features[i].kind = FeatureKind::Chest { state, wave, keepers, spot, t };
}

fn clamp_to_room(layout: &Layout, room: usize, p: Vec3) -> Vec3 {
    let r = layout.rooms[room].rect.shrink(1.5);
    Vec3::new(p.x.clamp(r.min.x, r.max.x), p.y, p.z.clamp(r.min.y, r.max.y))
}

fn open_chest_loot(g: &mut Game, sim: &mut Sim, lv: &mut LevelState, i: usize, events: &mut Vec<SimEvent>) {
    let pos = lv.features[i].pos;
    if let Some(e) = lv.features[i].entity.and_then(|e| sim.state.entities.get_mut(e)) {
        e.pos = pos + Vec3::new(0.0, 1.25, -0.45);
        e.rot = Quat::from_rotation_x(-1.1);
        if let Some(v) = e.visual.as_mut() {
            v.color = Color::hex("#ffd27a");
            v.light = Some(Box::new(LightDef { color: "#ffcf6a".into(), radius: 8.0, intensity: 2.2, ..Default::default() }));
            v.particles = Some(Box::new(EmitterDef {
                preset: "magic".into(),
                color: "#ffe9a0".into(),
                area: Vec3::new(0.6, 0.2, 0.4),
                ..Default::default()
            }));
        }
    }
    let level = lv.monster_level + 2;
    let n = 3 + (lv.depth / 4).min(5);
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let at = pos + Vec3::new(a.cos(), 0.0, a.sin()) * 1.4;
        let r = if k == 0 && sim.state.rng.f32() < 0.35 { Rarity::Unique } else { Rarity::Rare };
        g.drop_loot(sim, at, level, r, events);
    }
    events.push(SimEvent::LevelUp { pos });
    g.say("The vault opens!", 2.5);
}

/// The hero asks to open a cursed chest (interact).
pub(crate) fn open_chest(g: &mut Game, spot: usize) -> Result<(), String> {
    let lv = g.level.as_mut().ok_or("no chest here")?;
    let f = lv
        .features
        .iter_mut()
        .find(|f| matches!(f.kind, FeatureKind::Chest { spot: s, .. } if s == spot))
        .ok_or("no chest here")?;
    match &mut f.kind {
        FeatureKind::Chest { state, t, .. } if *state == ChestState::Closed => {
            *state = ChestState::Waves;
            *t = 0.6;
        }
        _ => return Err("already opened".into()),
    }
    if let Some(s) = g.spots.get_mut(spot) {
        s.name = "Cursed Chest (keepers!)".into();
        s.reach = 0.0;
    }
    g.say("The curse wakes...", 1.5);
    Ok(())
}

/// The hero takes the way down.
pub(crate) fn use_exit(g: &mut Game) -> Result<(), String> {
    let lv = g.level.as_ref().ok_or("no way down here")?;
    if !lv.exit_open {
        return Err(format!("Sealed while {} lives", lv.boss_name));
    }
    let next = lv.depth + 1;
    if lv.has_boss && next > g.hero.max_depth {
        g.hero.bonus_points += 1;
        g.say("A boss level conquered: +1 passive point", 3.0);
    }
    g.hero.max_depth = g.hero.max_depth.max(next);
    g.inv_changed();
    g.travel = Some(Place::Level(next).code());
    Ok(())
}

// ---------------------------------------------------------------------- what the view needs

#[derive(Clone, Debug)]
pub struct MapRoom {
    pub rect: Rect,
    pub seen: bool,
    pub role: RoomRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Portal,
    Exit,
    Shrine,
    Gate,
    Chest,
    Well,
    Totem,
}

#[derive(Clone, Debug)]
pub struct MapMark {
    pub pos: Vec2,
    pub kind: MarkKind,
    /// Still useful (unused shrine, unlit well, closed chest, open exit).
    pub active: bool,
}

/// The level for the HUD: card, map and marks.
#[derive(Clone, Debug)]
pub struct LevelView {
    pub depth: u32,
    pub label: String,
    pub name: String,
    pub about: String,
    pub endless: bool,
    /// Mechanic names with their one-line hints.
    pub mechanics: Vec<(String, String)>,
    pub mood: Mood,
    pub rooms: Vec<MapRoom>,
    pub corridors: Vec<Rect>,
    pub marks: Vec<MapMark>,
    pub exit_open: bool,
    pub boss_name: String,
    pub time: f32,
}

impl LevelState {
    pub fn view(&self, g: &Game) -> LevelView {
        let mut marks = Vec::new();
        let p2 = |p: Vec3| Vec2::new(p.x, p.z);
        for s in &g.spots {
            match s.kind {
                super::SpotKind::Portal => marks.push(MapMark { pos: p2(s.pos), kind: MarkKind::Portal, active: true }),
                super::SpotKind::Exit => marks.push(MapMark { pos: p2(s.pos), kind: MarkKind::Exit, active: self.exit_open }),
                _ => {}
            }
        }
        for f in &self.features {
            if !self.seen.get(f.room).copied().unwrap_or(false) {
                continue;
            }
            let (kind, active) = match &f.kind {
                FeatureKind::Shrine { used, .. } => (MarkKind::Shrine, !used),
                FeatureKind::Gate { .. } => (MarkKind::Gate, true),
                FeatureKind::Chest { state, .. } => (MarkKind::Chest, *state != ChestState::Open),
                FeatureKind::Well { lit } => (MarkKind::Well, !lit),
                FeatureKind::Totem => (MarkKind::Totem, f.entity.is_some()),
                _ => continue,
            };
            marks.push(MapMark { pos: p2(f.pos), kind, active });
        }
        LevelView {
            depth: self.depth,
            label: self.label.clone(),
            name: self.name.clone(),
            about: self.about.clone(),
            endless: self.endless,
            mechanics: self.mechanics.iter().map(|m| (m.name().to_string(), m.hint().to_string())).collect(),
            mood: self.mood.clone(),
            rooms: self
                .layout
                .rooms
                .iter()
                .enumerate()
                .map(|(i, r)| MapRoom { rect: r.rect, seen: self.seen[i], role: r.role })
                .collect(),
            corridors: self
                .layout
                .corridors
                .iter()
                .filter(|c| self.seen[c.rooms.0] || self.seen[c.rooms.1])
                .map(|c| c.rect)
                .collect(),
            marks,
            exit_open: self.exit_open,
            boss_name: self.boss_name.clone(),
            time: self.time,
        }
    }

    /// Spike plates about to fire.
    pub fn telegraphs(&self) -> Vec<Telegraph> {
        let mut out = Vec::new();
        for f in &self.features {
            if let FeatureKind::Spikes { half, period, phase, .. } = &f.kind {
                let cyc = (self.time + phase).rem_euclid(*period);
                let rise = period - 0.5;
                if cyc >= rise - 0.9 && cyc < rise {
                    let progress = (cyc - (rise - 0.9)) / 0.9;
                    out.push(Telegraph {
                        shape: TeleShape::Line {
                            from: f.pos - Vec3::X * *half,
                            dir: Vec3::X,
                            length: half * 2.0,
                            width: half * 2.0,
                        },
                        progress,
                        color: [1.0, 0.3, 0.15],
                    });
                }
            }
        }
        out
    }
}
