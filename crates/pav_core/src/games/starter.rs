//! The starter game, and the template for a new one: collect every shard while slimes chase
//! you round the walls. Three lives; a slime's touch sends everyone back to the start.
//!
//! To start a game of your own, copy this file and `starter.toml` to `<key>.rs` and
//! `<key>.toml`, change `KEY`, add its line to `GAMES` in `mod.rs`, then change the rules.
//! `docs/MAKE_A_GAME.md` walks through it.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Game, GameBox, Outcome};
use crate::color::Color;
use crate::entity::{Behavior, EntityId, Spawn};
use crate::frame::SimEvent;
use crate::input::InputFrame;
use crate::nav::NavGrid;
use crate::room::RoomDef;
use crate::shape::{Shape, Visual};
use crate::sim::Sim;

/// The level, a room file, built in (the browser has no files). Running from the repository,
/// `level_text` reads the file on disk instead, so an edit shows at the next `load`.
const LEVEL: &str = include_str!("starter.toml");

/// The rules' numbers: the level's `[game]` table, each with a default, so balancing is an
/// edit to the level and a `load` (no rebuild).
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub lives: u32,
    /// Slime speed as a fraction of a walker's top speed (each slime gets ±15%, from the seed).
    pub slime_speed: f32,
    /// Slimes chase a player within this walking distance (m).
    pub slime_sight: f32,
    /// Feet closer than this (m) catch the player.
    pub catch: f32,
    /// Feet closer than this (m) to a shard pick it up.
    pub pick: f32,
    /// Seconds the player is safe at the start and after a catch.
    pub grace: f32,
}

impl Default for Rules {
    fn default() -> Self {
        Self { lives: 3, slime_speed: 0.5, slime_sight: 9.0, catch: 0.8, pick: 0.9, grace: 2.0 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Starter {
    pub rules: Rules,
    /// Shards still on the floor.
    pub shards: Vec<EntityId>,
    pub total: usize,
    pub slimes: Vec<EntityId>,
    /// Each slime's start (feet) and speed.
    pub homes: Vec<Vec3>,
    pub speeds: Vec<f32>,
    pub lives: u32,
    /// Seconds the player is still safe from slimes.
    pub grace: f32,
    pub time: f32,
    pub outcome: Outcome,
    /// The walkable grid. It follows from the level alone, so it isn't saved: rebuilt when
    /// missing (after a snapshot load).
    #[serde(skip)]
    nav: Option<Arc<NavGrid>>,
}

/// Builds the level, places the shards on its `shard` markers and starts the game.
pub fn build(sim: &mut Sim, _rest: Option<&str>) -> Result<()> {
    let def = RoomDef::parse(&super::level_text(Starter::KEY, LEVEL)).map_err(|e| anyhow!("starter.toml: {e}"))?;
    let rules: Rules = toml::Value::Table(def.game.clone()).try_into().map_err(|e| anyhow!("starter.toml [game]: {e}"))?;
    crate::scenes::build_standalone_room(sim, Starter::KEY, def);
    let mut shards = Vec::new();
    for at in crate::scenes::markers(sim, "shard") {
        let mut look = Visual::new(Shape::Box { half: Vec3::splat(0.2) }, Color::hex("#7fe3ff"));
        look.emissive = 0.8;
        let shard = Spawn::new("shard", at + Vec3::Y * 0.7).visual(look).behavior(Behavior::Spin { speed: 2.5 });
        shards.push(sim.spawn(shard));
    }
    // Slimes come from the level's [[npc]] entries; the game drives them.
    let slimes: Vec<EntityId> = sim.state.entities.iter().filter(|e| e.name == "slime").map(|e| e.id).collect();
    let homes = slimes.iter().filter_map(|&id| sim.feet(id)).collect();
    // Randomness comes from the sim's seeded generator, so a seed always plays the same.
    let speeds = slimes.iter().map(|_| rules.slime_speed * sim.state.rng.range(0.85, 1.15)).collect();
    let game = Starter {
        total: shards.len(),
        shards,
        slimes,
        homes,
        speeds,
        lives: rules.lives,
        grace: rules.grace,
        rules,
        time: 0.0,
        outcome: Outcome::Playing,
        nav: None,
    };
    sim.state.play = Some(GameBox::new(game));
    Ok(())
}

impl Game for Starter {
    const KEY: &'static str = "starter";

    fn pre(
        &mut self,
        sim: &mut Sim,
        _input: &InputFrame,
        _dt: f32,
        _events: &mut Vec<SimEvent>,
    ) -> BTreeMap<EntityId, InputFrame> {
        let mut out = BTreeMap::new();
        if self.outcome != Outcome::Playing {
            // Decided: everyone stands still.
            for &id in sim.state.player.iter().chain(&self.slimes) {
                out.insert(id, InputFrame::default());
            }
            return out;
        }
        if self.nav.is_none() {
            self.nav = NavGrid::for_room(sim).map(Arc::new);
        }
        let Some(nav) = self.nav.clone() else { return out };
        // One flow field toward the player steers every slime round the walls.
        let player = sim.state.player.and_then(|id| sim.feet(id));
        let field = player.filter(|_| self.grace <= 0.0).and_then(|p| nav.flow(p, self.rules.slime_sight));
        for (&id, &speed) in self.slimes.iter().zip(&self.speeds) {
            let mut input = InputFrame::default();
            if let (Some(field), Some(at)) = (&field, sim.feet(id))
                && let Some(d) = field.dir(&nav, at)
            {
                input.move_dir = Vec2::new(d.x, d.z) * speed;
            }
            out.insert(id, input);
        }
        out
    }

    fn post(&mut self, sim: &mut Sim, dt: f32, events: &mut Vec<SimEvent>) {
        if self.outcome != Outcome::Playing {
            return;
        }
        self.time += dt;
        self.grace = (self.grace - dt).max(0.0);
        let Some(pid) = sim.state.player else { return };
        let Some(p) = sim.feet(pid) else { return };

        // Shards: walk over one to pick it up.
        let near: Vec<(EntityId, Vec3)> = self
            .shards
            .iter()
            .filter_map(|&id| sim.state.entities.get(id).map(|e| (id, e.pos)))
            .filter(|(_, at)| flat(*at - p) < self.rules.pick)
            .collect();
        for (id, at) in near {
            self.shards.retain(|&s| s != id);
            sim.despawn(id);
            events.push(SimEvent::Coin { pos: at });
            let left = self.shards.len();
            sim.say(if left == 1 { "1 shard to go".to_string() } else { format!("{left} shards to go") });
        }
        if self.shards.is_empty() {
            self.outcome = Outcome::Won;
            sim.say(format!("All shards in {:.1} s!", self.time));
            return;
        }

        // Slimes: a touch costs a life and sends everyone back to the start.
        let catch = self.rules.catch;
        let caught = self.grace <= 0.0 && self.slimes.iter().any(|&id| sim.feet(id).is_some_and(|s| flat(s - p) < catch));
        if caught {
            self.lives -= 1;
            events.push(SimEvent::Hit { pos: p + Vec3::Y, strength: 1.0, target: Some(pid) });
            if self.lives == 0 {
                self.outcome = Outcome::Lost;
                sim.say("Caught! Game over");
                return;
            }
            sim.say(format!("Caught! {} lives left", self.lives));
            let spawn = sim.state.spawn;
            sim.set_position(pid, spawn);
            for (&id, &home) in self.slimes.iter().zip(&self.homes) {
                sim.set_position(id, home);
            }
            self.grace = self.rules.grace;
        }
    }

    fn hud(&self, _sim: &Sim) -> Vec<String> {
        let mut lines =
            vec![format!("Shards {}/{}", self.total - self.shards.len(), self.total), format!("Lives {}", self.lives)];
        match self.outcome {
            Outcome::Won => lines.push(format!("You win! {:.1} s", self.time)),
            Outcome::Lost => lines.push("Game over".into()),
            Outcome::Playing => {}
        }
        lines
    }

    fn status(&self, sim: &Sim) -> serde_json::Value {
        let xz = |id: EntityId| sim.feet(id).map(|f| [round(f.x), round(f.z)]);
        json!({
            "outcome": self.outcome,
            "time": round(self.time),
            "shards_left": self.shards.len(),
            "total": self.total,
            "lives": self.lives,
            "grace": round(self.grace),
            "player": sim.state.player.and_then(xz),
            "shards": self.shards.iter().filter_map(|&id| sim.state.entities.get(id).map(|e| [round(e.pos.x), round(e.pos.z)])).collect::<Vec<_>>(),
            "slimes": self.slimes.iter().filter_map(|&id| xz(id)).collect::<Vec<_>>(),
        })
    }

    fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// Takes the shortest walk to a shard that keeps clear of slimes (cells near them count
    /// as walls); with every shard cut off, makes for the reachable spot farthest from them.
    fn bot(&self, sim: &Sim) -> InputFrame {
        let mut input = InputFrame::default();
        let Some(p) = sim.state.player.and_then(|id| sim.feet(id)) else { return input };
        let Some(mut grid) = self.nav.as_deref().cloned().or_else(|| NavGrid::for_room(sim)) else { return input };
        let slimes: Vec<Vec3> = self.slimes.iter().filter_map(|&id| sim.feet(id)).collect();
        if self.grace < 0.5 {
            for i in 0..grid.blocked.len() {
                if slimes.iter().any(|&s| flat(grid.center(i) - s) < 1.8) {
                    grid.blocked[i] = true;
                }
            }
        }
        let paths = self.shards.iter().filter_map(|&id| sim.state.entities.get(id)).filter_map(|e| grid.path(p, e.pos));
        let path = paths.min_by(|a, b| length(p, a).total_cmp(&length(p, b))).or_else(|| {
            let clearance = |i: usize| slimes.iter().map(|&s| flat(grid.center(i) - s)).fold(f32::MAX, f32::min);
            let reach = grid.flow(p, 6.0)?;
            let safest = (0..grid.blocked.len())
                .filter(|&i| reach.dist[i].is_finite())
                .max_by(|&a, &b| clearance(a).total_cmp(&clearance(b)))?;
            grid.path(p, grid.center(safest))
        });
        input.move_dir = path.map(|path| flat_dir(path[0] - p)).unwrap_or_default();
        input
    }
}

fn flat(v: Vec3) -> f32 {
    Vec2::new(v.x, v.z).length()
}

fn flat_dir(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z).normalize_or_zero()
}

/// Walking length of a path from `p`.
fn length(p: Vec3, path: &[Vec3]) -> f32 {
    let (mut at, mut sum) = (p, 0.0);
    for &q in path {
        sum += flat(q - at);
        at = q;
    }
    sum
}

fn round(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}
