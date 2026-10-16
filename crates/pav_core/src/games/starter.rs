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

/// The level: a room file, embedded in the build (the browser has no files).
const LEVEL: &str = include_str!("starter.toml");

const LIVES: u32 = 3;
/// Slime speed as a fraction of a walker's top speed (each slime gets ±15%, from the seed).
const SLIME_SPEED: f32 = 0.5;
/// Slimes chase a player within this walking distance (m).
const SLIME_SIGHT: f32 = 9.0;
/// Feet closer than this (m) catch the player, or pick up a shard.
const CATCH: f32 = 0.8;
const PICK: f32 = 0.9;
/// Seconds the player is safe at the start and after a catch.
const GRACE: f32 = 2.0;

#[derive(Clone, Serialize, Deserialize)]
pub struct Starter {
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
    let def = RoomDef::parse(LEVEL).map_err(|e| anyhow!("starter.toml: {e}"))?;
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
    let homes = slimes.iter().filter_map(|&id| feet(sim, id)).collect();
    // Randomness comes from the sim's seeded generator, so a seed always plays the same.
    let speeds = slimes.iter().map(|_| SLIME_SPEED * sim.state.rng.range(0.85, 1.15)).collect();
    let game = Starter {
        total: shards.len(),
        shards,
        slimes,
        homes,
        speeds,
        lives: LIVES,
        grace: GRACE,
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
        let nav = self.nav.get_or_insert_with(|| Arc::new(nav_grid(sim))).clone();
        // One flow field toward the player steers every slime round the walls.
        let player = sim.state.player.and_then(|id| feet(sim, id));
        let field = player.filter(|_| self.grace <= 0.0).and_then(|p| nav.flow(p, SLIME_SIGHT));
        for (&id, &speed) in self.slimes.iter().zip(&self.speeds) {
            let mut input = InputFrame::default();
            if let (Some(field), Some(at)) = (&field, feet(sim, id))
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
        let Some(p) = feet(sim, pid) else { return };

        // Shards: walk over one to pick it up.
        let near: Vec<(EntityId, Vec3)> = self
            .shards
            .iter()
            .filter_map(|&id| sim.state.entities.get(id).map(|e| (id, e.pos)))
            .filter(|(_, at)| flat(*at - p) < PICK)
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
        let caught = self.grace <= 0.0 && self.slimes.iter().any(|&id| feet(sim, id).is_some_and(|s| flat(s - p) < CATCH));
        if caught {
            self.lives -= 1;
            events.push(SimEvent::Hit { pos: p + Vec3::Y, strength: 1.0 });
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
            self.grace = GRACE;
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
        let xz = |id: EntityId| feet(sim, id).map(|f| [round(f.x), round(f.z)]);
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
        let Some(p) = sim.state.player.and_then(|id| feet(sim, id)) else { return input };
        let mut grid = self.nav.as_deref().cloned().unwrap_or_else(|| nav_grid(sim));
        let slimes: Vec<Vec3> = self.slimes.iter().filter_map(|&id| feet(sim, id)).collect();
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

/// A character's feet (entity positions are capsule centres).
fn feet(sim: &Sim, id: EntityId) -> Option<Vec3> {
    let e = sim.state.entities.get(id)?;
    Some(e.pos - Vec3::Y * e.character.as_ref()?.height() * 0.5)
}

fn nav_grid(sim: &Sim) -> NavGrid {
    let w = &sim.state.world;
    let room = w.current_room.and_then(|i| w.rooms.get(i as usize));
    let (min, max) = room
        .map(|r| (Vec2::new(r.min.x, r.min.z), Vec2::new(r.max.x, r.max.z)))
        .unwrap_or((Vec2::splat(-20.0), Vec2::splat(20.0)));
    NavGrid::build(sim, min, max, 0.5, 0.4, &|_| false)
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
