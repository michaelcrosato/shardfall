//! Moments (52in1 Session Protocol v1, "Moments"; docs/AI_HUB.md): short stretches of the game,
//! each a goal cleared or failed in seconds. The hub deals three in a row and loads the game
//! afresh for each; every copy builds the same moment from the round's seed ([`moment_sim`]),
//! holds it until the start, and reports whether its player cleared it. Each is a scene too
//! (`moment/<id>`), so the tools and tests play them like any place.
//!
//! A moment is the Proving Grounds with one goal and a fixed hero (the same on every copy): no
//! portal, no waves of its own, no loot. The game decides the outcome inside the simulation
//! ([`update`]), so replays and every copy agree on it; the app reports it.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::combat::Rarity;
use super::hero::Hero;
use super::{Game, data::data};
use crate::Sim;
use crate::rng::Rng;

/// What clears a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Goal {
    /// Every monster down.
    Defeat,
    /// Still standing when the time runs out.
    Survive,
    /// Every pile of gold picked up.
    Collect,
}

/// One of the game's moments, as the hub's `hub:moment` tag names it.
#[derive(Clone, Copy, Debug)]
pub struct MomentDef {
    /// The moment's name in the game (lowercase letters, digits and hyphens).
    pub id: &'static str,
    /// The protocol's verb: what the player does.
    pub verb: &'static str,
    /// How long it lasts.
    pub seconds: u32,
    /// What clears it, in English, as the hub's tag says it (at most 60 characters).
    pub goal_en: &'static str,
    pub goal: Goal,
    /// The hero's level: strong enough to clear it by playing well, never by standing still.
    pub hero_level: u32,
}

/// Every moment, in the order the hub's tags list them (`web/hub.json` has the same ids).
pub const MOMENTS: &[MomentDef] = &[
    MomentDef {
        id: "pack",
        verb: "defeat",
        seconds: 25,
        goal_en: "Defeat every monster in the pack",
        goal: Goal::Defeat,
        hero_level: 8,
    },
    MomentDef {
        id: "champion",
        verb: "defeat",
        seconds: 30,
        goal_en: "Slay the champion and its guards",
        goal: Goal::Defeat,
        hero_level: 10,
    },
    MomentDef {
        id: "swarm",
        verb: "survive",
        seconds: 20,
        goal_en: "Stay alive while the swarm closes in",
        goal: Goal::Survive,
        hero_level: 6,
    },
    MomentDef {
        id: "gold",
        verb: "collect",
        seconds: 20,
        goal_en: "Grab every pile of gold before time runs out",
        goal: Goal::Collect,
        hero_level: 6,
    },
];

pub fn def(id: &str) -> Option<&'static MomentDef> {
    MOMENTS.iter().find(|m| m.id == id)
}

/// A moment in play (`Game::moment`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MomentState {
    pub id: String,
    pub goal: Goal,
    /// Seconds of play left; it counts once play begins (the world holds still before).
    pub left: f32,
    pub seconds: f32,
    /// Cleared (true) or failed (false), once decided: the world then stops for good.
    pub outcome: Option<bool>,
    /// Swarm: seconds until the next pack comes in.
    #[serde(default)]
    pub next_pack: f32,
    /// Collect: piles at the start.
    #[serde(default)]
    pub total: u32,
}

/// What the HUD and the app see of a moment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MomentView {
    pub id: String,
    pub goal: Goal,
    pub left: f32,
    pub seconds: f32,
    pub outcome: Option<bool>,
    /// Defeat: monsters still standing. Collect: piles still on the ground.
    pub remaining: u32,
    /// Collect: piles at the start.
    pub total: u32,
}

/// The families a moment draws its monsters from (all of them quick to read on screen).
const PACK: &[&str] = &["ghoul", "skitterer", "spitter", "bile_ooze"];
const SWARM: &[&str] = &["ghoul", "skitterer"];
const GUARDS: &[&str] = &["ghoul", "spitter"];
const CHAMPIONS: &[&str] = &["bonecrusher", "ashdrake", "ghoul"];
/// Swarm: seconds between packs.
const SWARM_EVERY: f32 = 2.0;

/// The same numbers on every copy: the round's seed and the moment's name.
fn rng_for(id: &str, seed: u64) -> Rng {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    Rng::new(seed ^ h)
}

/// Moment `id` from `seed`, in a world of its own (the hub's rounds).
pub fn moment_sim(id: &str, seed: u64) -> Result<Sim, String> {
    let mut sim = Sim::empty(seed);
    build_moment(&mut sim, id)?;
    Ok(sim)
}

/// Builds moment `id` into an empty `sim` from its seed: the Proving Grounds, a fresh hero and
/// what the goal needs (the scene `moment/<id>`).
pub fn build_moment(sim: &mut Sim, id: &str) -> Result<(), String> {
    let m = def(id).ok_or_else(|| {
        let ids: Vec<&str> = MOMENTS.iter().map(|m| m.id).collect();
        format!("no moment '{id}' (moments: {})", ids.join(", "))
    })?;
    let seed = sim.state.seed;
    let d = data();
    let mut hero = Hero::new_character(&d, super::round::SEAT_CHARACTERS[0]);
    hero.level = m.hero_level;
    hero.potions = hero.potion_max;
    let mut g = Game::new(hero);
    g.round_seed = Some(seed);
    super::scene::build_arena_with(sim, Some(g));
    sim.state.scene = format!("moment/{id}");
    sim.recording.scene = sim.state.scene.clone();
    let mut g = sim.state.game.take().ok_or("the arena has no game")?;
    // A moment is one goal: no waves of the arena's own, no way home, nothing to pick up but
    // what the goal asks for.
    g.arena = None;
    g.spots.clear();
    g.message = None;
    g.loot_rate = 0.0;
    super::refresh_hero(sim, &mut g, true);
    let mut rng = rng_for(id, seed);
    let mut st = MomentState {
        id: id.to_string(),
        goal: m.goal,
        left: m.seconds as f32,
        seconds: m.seconds as f32,
        outcome: None,
        next_pack: 0.0,
        total: 0,
    };
    let level = m.hero_level;
    match id {
        "pack" => {
            // One pack around the hero, a few steps away.
            let fam = PACK[rng.below(PACK.len() as u32) as usize];
            let n = if fam == "skitterer" { 8 } else { 6 };
            let ang = rng.range(0.0, std::f32::consts::TAU);
            let center = Vec3::new(ang.cos(), 0.0, ang.sin()) * 7.0;
            spawn_group(sim, &mut g, fam, level, Rarity::Normal, center, n, true);
        }
        "champion" => {
            let fam = CHAMPIONS[rng.below(CHAMPIONS.len() as u32) as usize];
            let ang = rng.range(0.0, std::f32::consts::TAU);
            let center = Vec3::new(ang.cos(), 0.0, ang.sin()) * 8.0;
            // A champion a little below the hero, so a good fight wins it in time.
            let champion_level = level.saturating_sub(2).max(1);
            spawn_group(sim, &mut g, fam, champion_level, Rarity::Rare, center, 1, true);
            let guards = GUARDS[rng.below(GUARDS.len() as u32) as usize];
            spawn_group(sim, &mut g, guards, level, Rarity::Normal, center * 1.15, 3, true);
        }
        "swarm" => {
            // The first packs come in from two sides; more follow every few seconds (`update`).
            for k in 0..2 {
                let ang = rng.range(0.0, std::f32::consts::TAU) + k as f32 * std::f32::consts::PI;
                let fam = SWARM[rng.below(SWARM.len() as u32) as usize];
                spawn_group(sim, &mut g, fam, level + 4, Rarity::Normal, ring(ang, 15.0), 6, true);
            }
            st.next_pack = SWARM_EVERY;
        }
        "gold" => {
            // Piles scattered round the arena (clear of the pillars), with ghouls between them.
            let n = 6;
            for k in 0..n {
                let ang = (k as f32 + rng.range(0.15, 0.85)) / n as f32 * std::f32::consts::TAU;
                let r = if k % 2 == 0 { rng.range(5.5, 8.5) } else { rng.range(13.5, 16.0) };
                let pos = ring(ang, r) + Vec3::Y * 0.05;
                g.gold.push(super::loot::GoldPile { amount: 25, pos, vel: Vec3::ZERO, rest: true, age: 1.0 });
            }
            st.total = n;
            let ang = rng.range(0.0, std::f32::consts::TAU);
            spawn_group(sim, &mut g, "ghoul", level, Rarity::Normal, ring(ang, 12.0), 3, false);
        }
        _ => {}
    }
    g.moment = Some(st);
    sim.state.game = Some(g);
    Ok(())
}

/// A point on a ring round the arena's middle.
fn ring(angle: f32, r: f32) -> Vec3 {
    Vec3::new(angle.cos() * r, 0.0, angle.sin() * r)
}

/// `n` monsters of family `fam` in a loose cluster at `center`, inside the arena's walls.
#[allow(clippy::too_many_arguments)]
fn spawn_group(sim: &mut Sim, g: &mut Game, fam: &str, level: u32, rarity: Rarity, center: Vec3, n: u32, aggro: bool) {
    let d = data();
    let Some(spec) = d.family(fam).map(|f| f.spec()) else { return };
    let pack = g.next_pack;
    g.next_pack += 1;
    for i in 0..n {
        let a = i as f32 * 2.4;
        let at = center + Vec3::new(a.cos(), 0.0, a.sin()) * (0.8 + i as f32 * 0.35);
        let at = Vec3::new(at.x.clamp(-18.5, 18.5), 0.0, at.z.clamp(-18.5, 18.5));
        if let Some(id) = super::spawn_spec_into(sim, g, &spec, level, rarity, at, pack) {
            if let Some(b) = g.actors.get_mut(&id).and_then(|a| a.brain.as_mut()) {
                b.aggro = aggro;
            }
        }
    }
}

/// One tick of a moment: its clock, more of the swarm, and whether it is cleared or failed.
pub(crate) fn update(g: &mut Game, sim: &mut Sim, dt: f32) {
    let Some(mut st) = g.moment.take() else { return };
    if st.outcome.is_none() {
        st.left = (st.left - dt).max(0.0);
        let hero_down = g.hero_id.and_then(|h| g.actors.get(&h)).is_none_or(|a| a.dead);
        if st.goal == Goal::Survive && st.left > 2.0 {
            st.next_pack -= dt;
            if st.next_pack <= 0.0 {
                st.next_pack = SWARM_EVERY;
                let mut rng = rng_for(&st.id, sim.state.seed ^ ((st.seconds - st.left) * 10.0) as u64);
                let fam = SWARM[rng.below(SWARM.len() as u32) as usize];
                let ang = rng.range(0.0, std::f32::consts::TAU);
                let level = def(&st.id).map(|m| m.hero_level).unwrap_or(1);
                spawn_group(sim, g, fam, level + 4, Rarity::Normal, ring(ang, 16.0), 6, true);
            }
        }
        st.outcome = if hero_down {
            Some(false)
        } else {
            match st.goal {
                Goal::Defeat if g.monsters_alive() == 0 => Some(true),
                Goal::Collect if g.gold.is_empty() => Some(true),
                _ if st.left <= 0.0 => Some(st.goal == Goal::Survive),
                _ => None,
            }
        };
        if let Some(won) = st.outcome {
            let line = if won { "moment.cleared" } else { "moment.failed" };
            g.say(crate::t!(line), 600.0);
        }
    }
    g.moment = Some(st);
}

impl MomentState {
    pub fn view(&self, g: &Game) -> MomentView {
        let remaining = match self.goal {
            Goal::Defeat => g.monsters_alive() as u32,
            Goal::Collect => g.gold.len() as u32,
            Goal::Survive => 0,
        };
        MomentView {
            id: self.id.clone(),
            goal: self.goal,
            left: self.left,
            seconds: self.seconds,
            outcome: self.outcome,
            remaining,
            total: self.total,
        }
    }
}
