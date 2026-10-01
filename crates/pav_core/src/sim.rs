//! The simulation: fixed-tick, headless, snapshot-able, rewindable.

use std::path::Path;

use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::character::{self, Action, BombParams, Character, MovementParams, RADIUS};
use crate::choice_enum;
use crate::color::Color;
use crate::entity::{Behavior, BodyKind, Bomb, Entities, Entity, EntityId, Spawn};
use crate::frame::{PuppetFrame, RenderFrame, RenderObject, SimEvent};
use crate::history::{History, Replay};
use crate::input::InputFrame;
use crate::params::{ChoiceParam, ParamVisitor, Tunable, nested};
use crate::physics::{EventCollector, PhysicsState, entity_tag};
use crate::puppet::PuppetDef;
use crate::rng::Rng;
use crate::shape::{Look, Shape, Visual};
use crate::statics::{StaticWorld, block_flags};
use crate::terrain::TerrainParams;
use crate::world::World;

choice_enum! {
    pub enum TickRate { Hz60 => "60", Hz120 => "120", Hz240 => "240" }
}

impl TickRate {
    pub fn hz(self) -> u32 {
        match self {
            TickRate::Hz60 => 60,
            TickRate::Hz120 => 120,
            TickRate::Hz240 => 240,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimConfig {
    pub tick_rate: TickRate,
    /// World gravity for physics props (m/s², positive = down).
    pub gravity: f32,
    /// How far back rewind can go (seconds).
    pub history_seconds: f32,
    pub movement: MovementParams,
    pub bombs: BombParams,
    pub puppet: PuppetDef,
    pub terrain: TerrainParams,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            tick_rate: TickRate::Hz60,
            gravity: 9.81,
            history_seconds: 30.0,
            movement: MovementParams::default(),
            bombs: BombParams::default(),
            puppet: PuppetDef::default(),
            terrain: TerrainParams::default(),
        }
    }
}

/// Only the simulation-level fields (groups are visited by `visit_groups`).
impl Tunable for SimConfig {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.tick_rate.visit_choice(v, "tick_rate", "Simulation ticks per second");
        v.float("gravity", &mut self.gravity, 0.0, 40.0, "Gravity for physics props (m/s²)");
        v.float("history_seconds", &mut self.history_seconds, 0.0, 300.0, "Rewind window (s); 0 disables");
    }
}

impl SimConfig {
    /// Visits every simulation tunable as top-level groups: sim, movement, bombs, puppet.
    pub fn visit_groups(&mut self, v: &mut dyn ParamVisitor) {
        nested(v, "sim", self);
        nested(v, "movement", &mut self.movement);
        nested(v, "bombs", &mut self.bombs);
        nested(v, "puppet", &mut self.puppet);
        nested(v, "world", &mut self.terrain);
    }
}

/// Everything that changes during play. Cloning it is a snapshot.
#[derive(Clone, Serialize, Deserialize)]
pub struct SimState {
    pub tick: u64,
    pub seed: u64,
    pub rng: Rng,
    pub physics: PhysicsState,
    pub entities: Entities,
    pub statics: StaticWorld,
    pub scene: String,
    pub focus: Vec3,
    pub player: Option<EntityId>,
    pub spawn: Vec3,
    pub world: World,
}

impl SimState {
    /// Rough memory footprint (for the history budget and stats).
    pub fn approx_bytes(&self) -> usize {
        self.physics.colliders.len() * 700 + self.physics.bodies.len() * 600 + self.entities.len() * 400 + 4096
    }
}

pub struct Sim {
    pub config: SimConfig,
    pub state: SimState,
    pub history: History,
    /// Inputs since the scene was built (for replays and crash reports).
    pub recording: Replay,
    pipeline: PhysicsPipeline,
    events: Vec<SimEvent>,
    /// True while re-simulating (rewind): no events, no recording.
    replaying: bool,
    /// Shared copy of `config` for frames (refreshed when it changes).
    config_arc: std::sync::Arc<SimConfig>,
}

const MAX_RECORDING_TICKS: u64 = 60 * 60 * 60;

impl Sim {
    /// An empty world.
    pub fn empty(seed: u64) -> Self {
        let config = SimConfig::default();
        let dt = 1.0 / config.tick_rate.hz() as f32;
        Self {
            state: SimState {
                tick: 0,
                seed,
                rng: Rng::new(seed),
                physics: PhysicsState::new(dt),
                entities: Entities::default(),
                statics: StaticWorld::default(),
                scene: String::new(),
                focus: Vec3::ZERO,
                player: None,
                spawn: Vec3::ZERO,
                world: World::default(),
            },
            config,
            history: History::default(),
            recording: Replay { seed, ..Default::default() },
            pipeline: PhysicsPipeline::new(),
            events: Vec::new(),
            replaying: false,
            config_arc: std::sync::Arc::new(SimConfig::default()),
        }
    }

    /// A world populated by the named built-in scene.
    pub fn new(scene: &str, seed: u64) -> Result<Self> {
        let mut sim = Self::empty(seed);
        crate::scenes::build(&mut sim, scene)?;
        sim.state.scene = scene.to_string();
        sim.recording.scene = scene.to_string();
        Ok(sim)
    }

    /// Rebuilds the same scene and seed, keeping the configuration.
    pub fn reset(&mut self) -> Result<()> {
        let mut fresh = Sim::new(&self.state.scene, self.state.seed)?;
        fresh.config = self.config.clone();
        *self = fresh;
        Ok(())
    }

    pub fn dt(&self) -> f32 {
        1.0 / self.config.tick_rate.hz() as f32
    }

    pub fn time(&self) -> f64 {
        self.state.tick as f64 * self.dt() as f64
    }

    pub fn snapshot(&self) -> SimState {
        self.state.clone()
    }

    pub fn restore(&mut self, s: SimState) {
        self.state = s;
        self.pipeline = PhysicsPipeline::new();
    }

    pub fn spawn(&mut self, s: Spawn) -> EntityId {
        let id = self.state.entities.alloc_id();
        let body = match s.body {
            BodyKind::None => None,
            kind => {
                let builder = match kind {
                    BodyKind::Fixed => RigidBodyBuilder::fixed(),
                    BodyKind::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
                    _ => RigidBodyBuilder::dynamic(),
                }
                .pose(Pose::from_parts(s.pos, s.rot));
                let shape = s.visual.as_ref().map(|v| v.shape).unwrap_or(Shape::Sphere { radius: 0.25 });
                let collider = shape
                    .collider()
                    .density(s.density as Real)
                    .friction(s.friction as Real)
                    .restitution(s.restitution as Real)
                    .user_data(entity_tag(id.0));
                let (b, _) = self.state.physics.insert(builder, collider);
                Some(b)
            }
        };
        self.state.entities.map.insert(
            id,
            Entity {
                id,
                name: if s.name.is_empty() { format!("entity{}", id.0) } else { s.name },
                pos: s.pos,
                rot: s.rot,
                body_kind: s.body,
                body,
                visual: s.visual,
                behavior: s.behavior,
                character: None,
                bomb: None,
                lifetime: None,
                region: s.region,
                material: crate::entity::Material { density: s.density, friction: s.friction, restitution: s.restitution },
            },
        );
        if !self.replaying {
            self.events.push(SimEvent::Spawned { id, pos: s.pos });
        }
        id
    }

    /// Creates a character (kinematic capsule + puppet) standing at `feet`.
    pub fn spawn_character(&mut self, name: &str, feet: Vec3) -> EntityId {
        let id = self.state.entities.alloc_id();
        let ch = Character::new();
        let center = feet + Vec3::Y * (ch.height() * 0.5);
        let body = RigidBodyBuilder::kinematic_position_based().translation(center);
        let collider = ColliderBuilder::capsule_y(ch.posture.half_height() as Real, RADIUS as Real)
            .friction(0.0)
            .user_data(entity_tag(id.0));
        let (b, _) = self.state.physics.insert(body, collider);
        self.state.entities.map.insert(
            id,
            Entity {
                id,
                name: name.to_string(),
                pos: center,
                rot: Quat::IDENTITY,
                body_kind: BodyKind::Kinematic,
                body: Some(b),
                visual: None,
                behavior: Behavior::None,
                character: Some(Box::new(ch)),
                bomb: None,
                lifetime: None,
                region: None,
                material: Default::default(),
            },
        );
        id
    }

    /// Spawns the player at the scene's spawn point and makes the camera follow it.
    pub fn spawn_player(&mut self) -> EntityId {
        let id = self.spawn_character("player", self.state.spawn);
        self.state.player = Some(id);
        id
    }

    pub fn player(&self) -> Option<&Entity> {
        self.state.player.and_then(|id| self.state.entities.get(id))
    }

    pub fn despawn(&mut self, id: EntityId) -> bool {
        match self.state.entities.map.remove(&id) {
            Some(e) => {
                if let Some(b) = e.body {
                    self.state.physics.remove_body(b);
                }
                if self.state.player == Some(id) {
                    self.state.player = None;
                }
                true
            }
            None => false,
        }
    }

    /// Moves an entity (teleport), resetting its velocity. For characters `pos` is the feet.
    pub fn set_position(&mut self, id: EntityId, pos: Vec3) -> bool {
        let Some(e) = self.state.entities.map.get_mut(&id) else { return false };
        let center = match &mut e.character {
            Some(ch) => {
                ch.vel = Vec3::ZERO;
                ch.climbing = None;
                pos + Vec3::Y * (ch.height() * 0.5)
            }
            None => pos,
        };
        e.pos = center;
        if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
            b.set_translation(center, true);
            if b.is_kinematic() {
                b.set_next_kinematic_translation(center);
            }
            b.set_linvel(Vector::ZERO, true);
            b.set_angvel(Vector::ZERO, true);
        }
        true
    }

    /// Advances one fixed tick, recording the input for rewind and replays.
    pub fn step(&mut self, input: &InputFrame) {
        if self.history.enabled && self.config.history_seconds > 0.0 {
            self.history.window = (self.config.history_seconds * self.config.tick_rate.hz() as f32) as u64;
            self.history.record(&self.state, input);
        }
        if self.recording.ticks() < MAX_RECORDING_TICKS {
            self.recording.push(input);
        }
        self.step_inner(input);
    }

    fn step_inner(&mut self, input: &InputFrame) {
        let dt = self.dt();
        self.state.physics.params.dt = dt as Real;
        self.state.physics.gravity = Vector::new(0.0, -self.config.gravity as Real, 0.0);
        self.run_behaviors(dt);

        // Characters.
        let ids: Vec<EntityId> = self.state.entities.iter().filter(|e| e.character.is_some()).map(|e| e.id).collect();
        let mut actions = Vec::new();
        let mut events = Vec::new();
        let idle = InputFrame::default();
        for id in ids {
            let inp = if Some(id) == self.state.player { input } else { &idle };
            character::tick(
                &mut self.state,
                id,
                inp,
                &self.config.movement,
                &self.config.bombs,
                &self.config.puppet,
                self.config.gravity,
                dt,
                &mut events,
                &mut actions,
            );
        }
        for a in actions {
            match a {
                Action::ThrowBomb { from, vel, owner } => {
                    self.throw_bomb(from, vel, owner);
                    events.push(SimEvent::Throw { pos: from });
                }
            }
        }

        // Bombs and debris.
        let mut blasts = Vec::new();
        let mut expired = Vec::new();
        let settle_after = self.config.bombs.fuse - self.config.bombs.flight_time - 0.05;
        for e in self.state.entities.map.values_mut() {
            if let Some(b) = &mut e.bomb {
                b.fuse -= dt;
                if b.fuse <= 0.0 {
                    blasts.push((e.id, e.pos, b.radius));
                } else if b.fuse < settle_after {
                    // After the throw arc, bombs stop where they landed instead of rolling away.
                    if let Some(body) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                        let v = body.linvel();
                        body.set_linvel(Vec3::new(v.x * 0.8, v.y, v.z * 0.8), false);
                        body.set_angvel(body.angvel() * 0.8, false);
                    }
                }
            }
            if let Some(l) = &mut e.lifetime {
                *l = l.saturating_sub(1);
                if *l == 0 {
                    expired.push(e.id);
                }
            }
        }
        for (id, pos, radius) in blasts {
            self.despawn(id);
            self.explode(pos, radius, &mut events);
        }
        for id in expired {
            self.despawn(id);
        }

        let collector = EventCollector::default();
        self.state.physics.step(&mut self.pipeline, &collector);
        self.sync_from_physics();
        if let Some(p) = self.player() {
            self.state.focus = p.pos;
        }
        self.update_room_tracking(&mut events);
        if self.state.tick % 15 == 0 {
            self.update_streaming(2);
        }
        self.state.tick += 1;
        if !self.replaying {
            self.events.extend(events);
        }
    }

    fn throw_bomb(&mut self, from: Vec3, vel: Vec3, owner: EntityId) {
        let mut v = Visual::new(Shape::Sphere { radius: 0.2 }, Color::hex("#2b2d35"));
        v.look = Look::Lit;
        let id = self.spawn(Spawn::new("bomb", from).visual(v).body(BodyKind::Dynamic).restitution(0.15));
        if let Some(e) = self.state.entities.get_mut(id) {
            e.bomb = Some(Bomb { fuse: self.config.bombs.fuse, radius: self.config.bombs.radius, owner: Some(owner) });
            if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                b.set_linvel(vel, true);
                b.enable_ccd(true);
            }
        }
    }

    /// Removes destructible blocks in the radius, throws debris and pushes things away.
    pub fn explode(&mut self, pos: Vec3, radius: f32, events: &mut Vec<SimEvent>) {
        let hit = self.state.statics.blocks_in_sphere(pos, radius);
        let mut debris = Vec::new();
        for r in hit {
            if !self.state.statics.get(r).is_some_and(|b| b.has(block_flags::DESTRUCTIBLE)) {
                continue;
            }
            let st = &mut self.state;
            if let Some(b) = st.statics.destroy(&mut st.physics, r) {
                debris.push((b.center(), b.half(), b.color));
            }
        }
        for (c, half, color) in debris {
            let n = 3 + self.state.rng.below(3);
            for _ in 0..n {
                let rng = &mut self.state.rng;
                let off = Vec3::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y), rng.range(-half.z, half.z));
                let size = Vec3::new(rng.range(0.08, 0.2), rng.range(0.06, 0.14), rng.range(0.08, 0.2));
                let out = (c + off - pos).normalize_or(Vec3::Y);
                let vel = out * rng.range(2.0, 6.0) + Vec3::Y * rng.range(1.0, 4.0);
                let life = (rng.range(2.5, 4.0) * self.config.tick_rate.hz() as f32) as u32;
                let rot = Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.0), rng.range(0.0, 3.0), 0.0);
                let id = self.spawn(
                    Spawn::new("debris", c + off)
                        .visual(Visual::new(Shape::Box { half: size }, color))
                        .body(BodyKind::Dynamic)
                        .rot(rot),
                );
                if let Some(e) = self.state.entities.get_mut(id) {
                    e.lifetime = Some(life);
                    if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                        b.set_linvel(vel, true);
                    }
                }
            }
        }
        // Push props and characters.
        let push = self.config.bombs.push;
        let reach = radius * 2.5;
        for e in self.state.entities.map.values_mut() {
            let d = e.pos - pos;
            let dist = d.length();
            if dist > reach || e.bomb.is_some() {
                continue;
            }
            let fall = 1.0 - dist / reach;
            let dir = (d + Vec3::Y * 0.5).normalize_or(Vec3::Y);
            if let Some(ch) = &mut e.character {
                ch.impulse += dir * push * fall;
            } else if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                if b.is_dynamic() {
                    let m = b.mass();
                    b.apply_impulse(dir * push * fall * m, true);
                }
            }
        }
        self.state.physics.wake_near(pos, reach + 4.0);
        events.push(SimEvent::Explosion { pos, radius });
    }

    /// Runs `n` ticks with the same input.
    pub fn run(&mut self, n: u64, input: &InputFrame) {
        for _ in 0..n {
            self.step(input);
        }
    }

    /// Takes the events produced since the last call.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    // ------------------------------------------------------------------ time travel

    /// Shows the world as it was at `target` (restore + re-simulate). The recorded future is
    /// kept until `commit_rewind`, so scrubbing forward again works.
    pub fn rewind_to(&mut self, target: u64) -> bool {
        let Some(snap) = self.history.snapshots.iter().rev().find(|s| s.tick <= target).cloned() else { return false };
        self.restore(snap);
        self.replaying = true;
        while self.state.tick < target {
            let Some(input) = self.history.input_at(self.state.tick) else { break };
            self.step_inner(&input);
        }
        self.replaying = false;
        true
    }

    /// Starts a new timeline from the current tick (drops the recorded future).
    pub fn commit_rewind(&mut self) {
        let t = self.state.tick;
        self.history.truncate_after(t);
        // The replay recording restarts from scratch semantics: keep inputs up to `t`.
        let mut r = Replay { scene: self.recording.scene.clone(), seed: self.recording.seed, ..Default::default() };
        for (i, f) in self.recording.iter().enumerate() {
            if i as u64 >= t {
                break;
            }
            r.push(f);
        }
        self.recording = r;
    }

    /// Saves the full state to a file (binary).
    pub fn save_state(&self, path: &Path) -> Result<()> {
        let mut bytes = Vec::new();
        ciborium::into_writer(&self.state, &mut bytes).context("serializing state")?;
        if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }

    pub fn load_state(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let state: SimState = ciborium::from_reader(&bytes[..]).context("snapshot file is invalid or from another version")?;
        self.restore(state);
        self.history.clear();
        Ok(())
    }

    // ------------------------------------------------------------------ internals

    fn sync_from_physics(&mut self) {
        let bodies = &self.state.physics.bodies;
        for e in self.state.entities.map.values_mut() {
            if let Some(b) = e.body.and_then(|h| bodies.get(h)) {
                e.pos = b.translation();
                e.rot = *b.rotation();
            }
        }
    }

    fn run_behaviors(&mut self, dt: f32) {
        let ids: Vec<EntityId> =
            self.state.entities.iter().filter(|e| !matches!(e.behavior, Behavior::None)).map(|e| e.id).collect();
        for id in ids {
            let Some(e) = self.state.entities.get(id) else { continue };
            match e.behavior.clone() {
                Behavior::None => {}
                Behavior::Spin { speed } => {
                    if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
                        b.set_angvel(Vector::new(0.0, speed as Real, 0.0), true);
                    } else if let Some(e) = self.state.entities.get_mut(id) {
                        e.rot = Quat::from_rotation_y(speed * dt) * e.rot;
                    }
                }
                Behavior::Rain { interval, max, area, height, mut timer, mut spawned } => {
                    let origin = e.pos;
                    timer += 1;
                    if timer >= interval {
                        timer = 0;
                        let rng = &mut self.state.rng;
                        let pos = origin + Vec3::new(rng.range(-area, area), height, rng.range(-area, area));
                        let palette = ["#e8704a", "#f2c14e", "#5b8def", "#9b5de5", "#3bb273", "#f15bb5", "#00bbf9"];
                        let color = Color::hex(palette[rng.below(palette.len() as u32) as usize]);
                        let shape = match rng.below(4) {
                            0 => Shape::Box { half: Vec3::splat(rng.range(0.2, 0.45)) },
                            1 => Shape::Sphere { radius: rng.range(0.2, 0.45) },
                            2 => Shape::Capsule { half_height: rng.range(0.15, 0.35), radius: rng.range(0.15, 0.3) },
                            _ => Shape::RoundedBox { half: Vec3::new(0.45, 0.25, 0.3), radius: 0.1 },
                        };
                        let rot = Quat::from_euler(glam::EulerRot::XYZ, rng.range(0.0, 3.0), rng.range(0.0, 3.0), 0.0);
                        let bounce = rng.range(0.0, 0.5);
                        let new_id = self.spawn(
                            Spawn::new("rain", pos)
                                .visual(Visual::new(shape, color))
                                .body(BodyKind::Dynamic)
                                .rot(rot)
                                .restitution(bounce),
                        );
                        spawned.push_back(new_id);
                        while spawned.len() > max as usize {
                            if let Some(old) = spawned.pop_front() {
                                self.despawn(old);
                            }
                        }
                    }
                    if let Some(e) = self.state.entities.get_mut(id) {
                        e.behavior = Behavior::Rain { interval, max, area, height, timer, spawned };
                    }
                }
            }
        }
    }

    pub fn frame(&mut self) -> RenderFrame {
        if *self.config_arc != self.config {
            self.config_arc = std::sync::Arc::new(self.config.clone());
        }
        let room = self
            .state
            .world
            .current_room
            .and_then(|i| self.state.world.rooms.get(i as usize))
            .map(|r| crate::frame::RoomInfo { id: r.id, key: r.key.clone(), def: r.def.clone() });
        RenderFrame {
            tick: self.state.tick,
            time: self.time(),
            dt: self.dt(),
            objects: self
                .state
                .entities
                .iter()
                .filter_map(|e| {
                    let puppet = e.character.as_ref().map(|c| PuppetFrame { state: c.anim, feet_offset: c.height() * 0.5 });
                    if e.visual.is_none() && puppet.is_none() {
                        return None;
                    }
                    let visual = e.visual.clone().unwrap_or(Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE));
                    let pulse = e.bomb.as_ref().map(|b| b.fuse).unwrap_or(-1.0);
                    Some(RenderObject { id: e.id, pos: e.pos, rot: e.rot, visual, puppet, pulse })
                })
                .collect(),
            statics: self.state.statics.clone(),
            focus: self.state.focus,
            focus_is_player: self.state.player.is_some(),
            player: self.state.player,
            puppet_def: self.config.puppet.clone(),
            room,
            config: self.config_arc.clone(),
            events: self.events.clone(),
        }
    }

    /// Hash of the dynamic state, for repeatability checks.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |x: u64| {
            h ^= x;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        mix(self.state.tick);
        for e in self.state.entities.iter() {
            mix(e.id.0 as u64);
            for f in e.pos.to_array().into_iter().chain(e.rot.to_array()) {
                mix(f.to_bits() as u64);
            }
        }
        mix(self.state.statics.block_count() as u64);
        h
    }
}
