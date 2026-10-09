//! The simulation: fixed-tick, headless, snapshot-able, rewindable.

use std::path::Path;

use anyhow::{Context, Result};
use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::character::{self, Action, BombParams, Character, MovementParams, RADIUS};
use crate::choice_enum;
use crate::color::Color;
use crate::course::Courses;
use crate::entity::{Behavior, BodyKind, Bomb, Entities, Entity, EntityId, Spawn};
use crate::feel::FeelMeter;
use crate::frame::{PuppetFrame, RenderFrame, RenderObject, SimEvent};
use crate::history::{History, Replay};
use crate::input::InputFrame;
use crate::params::{ChoiceParam, ParamVisitor, Tunable, nested};
use crate::physics::{EventCollector, PhysicsState, entity_tag};
use crate::projectile::{Projectiles, Target};
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
    /// Shardfall difficulty multipliers.
    pub difficulty: crate::arpg::Difficulty,
    /// Performers: tempo and mirroring of their clips and moves.
    pub anim: crate::clips::AnimParams,
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
            difficulty: crate::arpg::Difficulty::default(),
            anim: crate::clips::AnimParams::default(),
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
    /// Visits every simulation tunable as top-level groups: sim, movement, bombs, puppet, world,
    /// difficulty, anim.
    pub fn visit_groups(&mut self, v: &mut dyn ParamVisitor) {
        nested(v, "sim", self);
        nested(v, "movement", &mut self.movement);
        nested(v, "bombs", &mut self.bombs);
        nested(v, "puppet", &mut self.puppet);
        nested(v, "world", &mut self.terrain);
        nested(v, "difficulty", &mut self.difficulty);
        nested(v, "anim", &mut self.anim);
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
    #[serde(default)]
    pub courses: Courses,
    #[serde(default)]
    pub projectiles: Projectiles,
    #[serde(default)]
    pub feel: FeelMeter,
    /// Signals sent by pads this tick (read by spawners next tick).
    #[serde(default)]
    pub signals: Vec<String>,
    /// Tiles that are crumbling or waiting to regrow.
    #[serde(default)]
    pub crumbles: Vec<crate::destruct::Crumble>,
    /// Shardfall, when this scene is part of the game.
    #[serde(default)]
    pub game: Option<Box<crate::arpg::Game>>,
}

impl SimState {
    /// Rough memory footprint (for the history budget and stats).
    pub fn approx_bytes(&self) -> usize {
        self.physics.colliders.len() * 700
            + self.physics.bodies.len() * 600
            + self.entities.len() * 400
            + self.projectiles.list.len() * 64
            + 4096
    }
}

pub struct Sim {
    pub config: SimConfig,
    pub state: SimState,
    pub history: History,
    /// Inputs since the scene was built (for replays and crash reports).
    pub recording: Replay,
    pipeline: PhysicsPipeline,
    pub(crate) events: Vec<SimEvent>,
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
                courses: Courses::default(),
                projectiles: Projectiles::default(),
                feel: FeelMeter::default(),
                signals: Vec::new(),
                crumbles: Vec::new(),
                game: None,
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
        let soft = s.soft.as_ref().map(|d| {
            let part = self.state.physics.insert_soft(d, s.pos, s.rot);
            if let Some(sb) = part.handle.and_then(|h| self.state.physics.soft_bodies.get_mut(h)) {
                let tag = entity_tag(id.0);
                let root = sb.root_body();
                if let Some(b) = self.state.physics.bodies.get(root) {
                    for c in b.colliders().to_vec() {
                        if let Some(col) = self.state.physics.colliders.get_mut(c) {
                            col.user_data = tag;
                        }
                    }
                }
            }
            part
        });
        let body = match s.body {
            _ if soft.is_some() => None,
            BodyKind::None => None,
            kind => {
                let builder = match kind {
                    BodyKind::Fixed => RigidBodyBuilder::fixed(),
                    BodyKind::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
                    _ => RigidBodyBuilder::dynamic(),
                }
                .pose(Pose::from_parts(s.pos, s.rot))
                .linear_damping(s.damping as Real)
                .angular_damping(s.damping as Real);
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
                material: crate::entity::Material {
                    density: s.density,
                    friction: s.friction,
                    restitution: s.restitution,
                    damping: s.damping,
                },
                hazard: s.hazard,
                soft,
                joints: Vec::new(),
                ai: None,
                vehicle: None,
                health: None,
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
        let center = feet + Vec3::Y * (ch.height() * 0.5 + character::PLACE_LIFT);
        let b = self.character_body(id, center, ch.posture);
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
                hazard: None,
                soft: None,
                joints: Vec::new(),
                ai: None,
                vehicle: None,
                health: None,
            },
        );
        id
    }

    /// Creates a non-player character with its own look and (optionally) a brain.
    pub fn spawn_npc(
        &mut self,
        name: &str,
        feet: Vec3,
        facing: f32,
        def: PuppetDef,
        ai: Option<crate::ai::Ai>,
        region: Option<crate::statics::RegionKey>,
    ) -> EntityId {
        let id = self.spawn_character(name, feet);
        if let Some(e) = self.state.entities.get_mut(id) {
            e.region = region;
            e.ai = ai.map(Box::new);
            if let Some(ch) = &mut e.character {
                ch.facing = facing;
                ch.anim.facing = facing;
                ch.puppet = Some(std::sync::Arc::new(def));
            }
        }
        id
    }

    /// A character's kinematic capsule (when it is created or wakes up).
    pub(crate) fn character_body(&mut self, id: EntityId, center: Vec3, posture: character::Posture) -> RigidBodyHandle {
        let body = RigidBodyBuilder::kinematic_position_based().translation(center);
        let collider =
            ColliderBuilder::capsule_y(posture.half_height() as Real, RADIUS as Real).friction(0.0).user_data(entity_tag(id.0));
        self.state.physics.insert(body, collider).0
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
                if let Some(h) = e.soft.and_then(|s| s.handle) {
                    self.state.physics.remove_soft(h);
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
                ch.impulse = Vec3::ZERO;
                ch.stun = 0.0;
                ch.roll = 0.0;
                ch.hang = None;
                ch.grid_target = None;
                ch.climbing = None;
                pos + Vec3::Y * (ch.height() * 0.5 + character::PLACE_LIFT)
            }
            None => pos,
        };
        e.pos = center;
        if self.state.player == Some(id) {
            self.state.focus = center;
        }
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
        // Hit-stop slows the whole world for a moment.
        let raw_dt = self.dt();
        let dt = raw_dt * self.game_time_scale();
        self.state.physics.params.dt = dt as Real;
        self.state.physics.gravity = Vector::new(0.0, -self.config.gravity as Real, 0.0);

        // Behaviours set this tick's velocities of moving objects; characters then move
        // (rapier's controller carries them with kinematic platforms) and the physics step
        // advances everything together.
        self.run_behaviors(dt);

        // Getting in and out of vehicles.
        self.vehicle_interact(input);

        // The game decides what the hero and its monsters do this tick.
        let mut events = Vec::new();
        let game_inputs = self.game_pre(input, dt, &mut events);

        // Characters.
        let ids: Vec<EntityId> = self.state.entities.iter().filter(|e| e.character.is_some()).map(|e| e.id).collect();
        let mut actions = Vec::new();
        let idle = InputFrame::default();
        let player_feet = self.player().and_then(|p| Some(p.pos - Vec3::Y * p.character.as_ref()?.height() * 0.5));
        let all_feet: Vec<Vec3> =
            self.state.entities.iter().filter_map(|e| Some(e.pos - Vec3::Y * e.character.as_ref()?.height() * 0.5)).collect();
        let mut fallen = Vec::new();
        for id in ids {
            let npc_input: InputFrame;
            let inp = if let Some(gi) = game_inputs.get(&id) {
                gi
            } else if Some(id) == self.state.player {
                input
            } else {
                // Non-player characters: their brain drives them like a player would.
                let st = &mut self.state;
                match st.entities.map.get_mut(&id) {
                    Some(e) if e.ai.is_some() => {
                        let feet = e.pos - Vec3::Y * e.character.as_ref().map(|c| c.height() * 0.5).unwrap_or(0.0);
                        let ai = e.ai.as_mut().unwrap();
                        if feet.y < crate::course::FALL_LIMIT {
                            fallen.push((id, ai.home));
                        }
                        npc_input = ai.think(feet, player_feet, &all_feet, &mut st.rng, dt);
                        if let (Some(f), Some(ch)) = (ai.rest_facing(), e.character.as_mut()) {
                            if ch.stun <= 0.0 {
                                ch.facing = f;
                            }
                        }
                        if let (Some(p), Some(ch)) = (ai.perform.as_mut(), e.character.as_mut()) {
                            p.tick(&mut ch.anim, dt, &self.config.anim);
                        }
                        &npc_input
                    }
                    _ => &idle,
                }
            };
            let own = self.state.entities.get(id).and_then(|e| e.character.as_ref()).and_then(|c| c.puppet.clone());
            character::tick(
                &mut self.state,
                id,
                inp,
                &self.config.movement,
                &self.config.bombs,
                own.as_deref().unwrap_or(&self.config.puppet),
                self.config.gravity,
                dt,
                &mut events,
                &mut actions,
            );
        }
        for (id, home) in fallen {
            self.set_position(id, home);
        }
        self.update_guards(dt, &mut events);
        for a in actions {
            match a {
                Action::ThrowBomb { from, vel, owner } => {
                    self.throw_bomb(from, vel, owner);
                    events.push(SimEvent::Throw { pos: from });
                }
                Action::Hit { id, at, dir, knockback, respawn } => {
                    self.hit_character(id, at, dir, knockback, respawn, &mut events)
                }
                Action::Shoot { from, vel, owner } => {
                    self.state.projectiles.spawn(crate::projectile::Projectile {
                        pos: from,
                        vel,
                        radius: 0.09,
                        life: self.config.bombs.shot_range / vel.length().max(0.1),
                        color: Color::hex("#9ef0ff"),
                        knockback: 0.0,
                        gravity: 0.0,
                        owner: Some(owner),
                        team: crate::projectile::Team::Player,
                        damage: 1.0,
                    });
                    events.push(SimEvent::Shot { pos: from });
                }
            }
        }
        self.drive_vehicles(input, dt);
        self.zone_props(dt);
        let collector = EventCollector::default();
        self.state.physics.step(&mut self.pipeline, &collector);
        self.sync_from_physics();
        self.seat_riders();
        let contacts = collector.contacts.into_inner().unwrap_or_default();
        if !contacts.is_empty() {
            self.break_blocks(contacts, &mut events);
        }
        self.update_crumbles(dt, &mut events);

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

        // Shootable things: sway, hit flash.
        self.update_health(dt);

        // Projectiles.
        if !self.state.projectiles.list.is_empty() {
            let hitbox = self.config.movement.hitbox;
            let player = self.state.player;
            let targets: Vec<Target> = self
                .state
                .entities
                .iter()
                .filter_map(|e| {
                    let c = e.character.as_ref()?;
                    let radius = if Some(e.id) == player { hitbox } else { RADIUS };
                    Some(Target { id: e.id, feet: e.pos - Vec3::Y * c.height() * 0.5, height: c.height(), radius })
                })
                .collect();
            let enemies: Vec<crate::projectile::Enemy> = self
                .state
                .entities
                .iter()
                .filter(|e| e.health.is_some())
                .map(|e| crate::projectile::Enemy {
                    id: e.id,
                    center: e.pos,
                    radius: e.visual.as_ref().map(|v| v.shape.half_extents().max_element()).unwrap_or(0.5),
                })
                .collect();
            let physics = &self.state.physics;
            let entities = &self.state.entities;
            let out = self.state.projectiles.step(dt, &targets, &enemies, |from, dir, len, owner| {
                let ignore = owner.and_then(|id| entities.get(id)).and_then(|e| e.body);
                crate::projectile::static_blocked(physics, from, dir, len, ignore)
            });
            for (id, at, dir, kb) in out.hits {
                self.hit_character(id, at, dir, kb, false, &mut events);
            }
            for (id, at, dmg) in out.damage {
                self.damage(id, at, dmg, &mut events);
            }
        }

        self.game_post(dt, raw_dt, &mut events);
        self.game_travel(&mut events);

        if let Some(p) = self.player() {
            self.state.focus = p.pos;
        }
        self.update_zones(&mut events);
        self.measure_feel(input);
        self.update_room_tracking(&mut events);
        if self.state.tick.is_multiple_of(15) {
            self.update_streaming(2);
        }
        self.state.tick += 1;
        if !self.replaying {
            self.events.extend(events);
        }
    }

    fn measure_feel(&mut self, input: &InputFrame) {
        let Some(e) = self.player() else { return };
        let Some(ch) = e.character.as_ref() else { return };
        let (vel, grounded, feet) =
            (ch.vel, ch.grounded || ch.climbing.is_some() || ch.hang.is_some(), e.pos - Vec3::Y * ch.height() * 0.5);
        let dt = self.dt();
        let tick = self.state.tick;
        self.state.feel.update(tick, dt, input.move_dir, vel, feet, grounded);
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
                    Spawn::new("~debris", c + off)
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
            if let Some(h) = e.soft.as_ref().and_then(|s| s.handle) {
                // Soft bodies: every particle in reach gets kicked away from the blast.
                if let Some(sb) = self.state.physics.soft_bodies.get_mut(h) {
                    let pv: Vec<(Vec3, Vec3)> = sb.particle_positions().zip(sb.particle_velocities()).collect();
                    for (i, (p, v)) in pv.into_iter().enumerate() {
                        let d = p - pos;
                        let f = 1.0 - d.length() / reach;
                        if f > 0.0 {
                            sb.set_particle_velocity(i, v + (d + Vec3::Y * 0.5).normalize_or(Vec3::Y) * push * f);
                        }
                    }
                }
                continue;
            }
            if let Some(ch) = &mut e.character {
                ch.impulse += dir * push * fall;
                ch.stun = ch.stun.max(0.25 * fall);
                ch.hang = None;
                ch.anim.hit(dir, 0.5 + 1.5 * fall);
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
        let ph = &self.state.physics;
        for e in self.state.entities.map.values_mut() {
            if let Some(b) = e.body.and_then(|h| ph.bodies.get(h)) {
                e.pos = b.translation();
                e.rot = *b.rotation();
            } else if let Some(sb) = e.soft.as_ref().and_then(|s| s.handle).and_then(|h| ph.soft_bodies.get(h)) {
                e.pos = sb.center_of_mass();
            }
        }
    }

    /// Adds a joint owned by `owner` (to `link.other`, or the world) and creates it now if both
    /// bodies exist.
    pub fn add_joint(&mut self, owner: EntityId, link: crate::joints::JointLink) -> bool {
        let Some(a) = self.state.entities.get(owner).and_then(|e| e.body) else { return false };
        let b = match link.other {
            Some(o) => match self.state.entities.get(o).and_then(|e| e.body) {
                Some(b) => Some(b),
                None => return false,
            },
            None => None,
        };
        self.state.physics.insert_joint(a, b, &link);
        if let Some(e) = self.state.entities.get_mut(owner) {
            e.joints.push(link);
        }
        true
    }

    /// Recreates the joints of the given entities (after their region wakes up).
    pub(crate) fn restore_joints(&mut self, ids: &[EntityId]) {
        for id in ids {
            let Some(e) = self.state.entities.get(*id) else { continue };
            let Some(a) = e.body else { continue };
            for link in e.joints.clone() {
                let b = match link.other {
                    Some(o) => match self.state.entities.get(o).and_then(|x| x.body) {
                        Some(b) => Some(b),
                        None => continue,
                    },
                    None => None,
                };
                self.state.physics.insert_joint(a, b, &link);
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
            .map(|r| crate::frame::RoomInfo { id: r.id, key: r.key.clone(), def: r.def.clone(), quarters: r.place.quarters });
        let c = &self.state.courses;
        let invuln = self.player().and_then(|p| p.character.as_ref()).map(|c| c.invuln).unwrap_or(0.0);
        let hud = crate::frame::HudFrame {
            course: c.hud(self.state.tick, self.dt()),
            last_result: c.last.clone(),
            message: c.message.clone(),
            feel: self.state.feel.report,
            cue: c.cue(),
            cue_serial: c.cue_serial,
            invuln,
            model: crate::params::ChoiceParam::name(self.config.movement.model).to_string(),
            physics: self.physics_stats(),
            view: c.view.clone(),
            view_serial: c.view_serial,
            boss: self.boss_bar(),
            pad_note: c.pad_note.clone(),
        };
        RenderFrame {
            tick: self.state.tick,
            time: self.time(),
            dt: self.dt(),
            objects: self
                .state
                .entities
                .iter()
                .filter_map(|e| {
                    let puppet = e.character.as_ref().filter(|c| c.riding.is_none()).map(|c| PuppetFrame {
                        state: c.anim,
                        feet_offset: c.height() * 0.5,
                        def: c.puppet.clone(),
                        rig: c.rig.as_ref().map(|r| r.view()),
                        tint: self.state.game.as_ref().and_then(|g| g.tint(e.id)),
                    });
                    if e.visual.is_none() && puppet.is_none() {
                        return None;
                    }
                    let mut visual = e.visual.clone().unwrap_or(Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE));
                    if e.health.as_ref().is_some_and(|h| h.flash > 0.0) {
                        // Hit flash.
                        visual.color = Color::WHITE;
                        visual.emissive = visual.emissive.max(0.45);
                    }
                    let pulse = e.bomb.as_ref().map(|b| b.fuse).unwrap_or(-1.0);
                    let soft = e.soft.as_ref().and_then(|s| {
                        let h = s.handle?;
                        let radius = match &s.def.shape {
                            crate::softbody::SoftShape::Rope { radius, .. } => *radius,
                            _ => 0.0,
                        };
                        Some(crate::frame::SoftView {
                            points: self.state.physics.soft_positions(h),
                            surface: s.surface.clone(),
                            segments: s.segments.clone(),
                            radius,
                            two_sided: matches!(s.def.shape, crate::softbody::SoftShape::Cloth { .. }),
                        })
                    });
                    let vehicle = e.vehicle.as_ref().map(|v| v.view());
                    let cone = self.guard_cone(e);
                    let scenery = e.character.is_none()
                        && matches!(e.body_kind, BodyKind::None | BodyKind::Fixed)
                        && matches!(e.behavior, Behavior::None)
                        && e.bomb.is_none()
                        && e.health.is_none()
                        && e.vehicle.is_none()
                        && e.soft.is_none()
                        && e.lifetime.is_none()
                        && !self.state.game.as_ref().is_some_and(|g| g.actors.contains_key(&e.id));
                    Some(RenderObject { id: e.id, pos: e.pos, rot: e.rot, visual, puppet, pulse, soft, vehicle, cone, scenery })
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
            projectiles: self
                .state
                .projectiles
                .list
                .iter()
                .map(|p| crate::frame::ProjectileView { pos: p.pos, vel: p.vel, radius: p.radius, color: p.color })
                .collect(),
            hud,
            game: self.state.game.as_ref().map(|g| std::sync::Arc::new(g.frame(self))),
        }
    }

    /// Counts for the physics overlay.
    pub fn physics_stats(&self) -> crate::frame::PhysicsStats {
        let ph = &self.state.physics;
        let (mut dynamic, mut sleeping) = (0, 0);
        for (_, b) in ph.bodies.iter() {
            if b.is_dynamic() {
                dynamic += 1;
                if b.is_sleeping() {
                    sleeping += 1;
                }
            }
        }
        crate::frame::PhysicsStats {
            dynamic,
            sleeping,
            colliders: ph.colliders.len(),
            joints: ph.impulse_joints.len(),
            soft_bodies: ph.soft_bodies.len(),
            particles: ph.soft_bodies.iter().map(|(_, s)| s.num_particles()).sum(),
            contacts: ph.narrow_phase.contact_pairs().filter(|p| p.has_any_active_contact()).count(),
            projectiles: self.state.projectiles.list.len(),
        }
    }

    /// Hash of the gameplay state, for repeatability checks: the tick, the random numbers, every
    /// entity, rigid and soft bodies (pose and velocity), the level's blocks, projectiles,
    /// courses, signals, crumbling tiles and the game (hero, items, gold, monsters). Two runs
    /// that hash the same played out the same.
    pub fn state_hash(&self) -> u64 {
        let s = &self.state;
        let mut h = Fnv::default();
        h.u64(s.tick);
        h.data(&s.rng);
        h.data(&s.entities);
        for (_, b) in s.physics.bodies.iter() {
            h.floats(&b.translation().to_array());
            h.floats(&b.rotation().to_array());
            h.floats(&b.linvel().to_array());
            h.floats(&b.angvel().to_array());
        }
        for (_, sb) in s.physics.soft_bodies.iter() {
            for (p, v) in sb.particle_positions().zip(sb.particle_velocities()) {
                h.floats(&p.to_array());
                h.floats(&v.to_array());
            }
        }
        h.u64(s.statics.block_count() as u64);
        h.data(&s.projectiles);
        h.data(&s.courses);
        h.data(&s.signals);
        h.data(&s.crumbles);
        h.data(&s.game);
        h.0
    }
}

/// 64-bit FNV-1a over everything written to it.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn u64(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.byte(b);
        }
    }

    fn floats(&mut self, fs: &[f32]) {
        for f in fs {
            for b in f.to_bits().to_le_bytes() {
                self.byte(b);
            }
        }
    }

    /// Mixes in a value's serialized form (the same CBOR that snapshot files use; the state
    /// keeps its maps ordered, so the bytes are the same every run).
    fn data<T: Serialize + ?Sized>(&mut self, value: &T) {
        // Writing into the hash can't fail, and every state type serializes.
        let _ = ciborium::into_writer(value, &mut *self);
    }

    fn byte(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }
}

impl std::io::Write for Fnv {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &b in buf {
            self.byte(b);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
