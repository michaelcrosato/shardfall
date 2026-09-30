//! The simulation: fixed-tick, headless, snapshot-able.

use anyhow::Result;
use glam::{Quat, Vec3};
use rapier::prelude::*;

use crate::choice_enum;
use crate::color::Color;
use crate::entity::{Behavior, BodyKind, Entities, Entity, EntityId, Spawn};
use crate::frame::{RenderFrame, RenderObject, SimEvent};
use crate::input::InputFrame;
use crate::params::{ChoiceParam, ParamVisitor, Tunable};
use crate::physics::{EventCollector, PhysicsState, entity_tag};
use crate::rng::Rng;
use crate::shape::{Shape, Visual};
use crate::statics::StaticWorld;

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

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub tick_rate: TickRate,
    /// World gravity for props (m/s^2, positive = down).
    pub gravity: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self { tick_rate: TickRate::Hz60, gravity: 9.81 }
    }
}

impl Tunable for SimConfig {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.tick_rate.visit_choice(v, "tick_rate", "Simulation ticks per second");
        v.float("gravity", &mut self.gravity, 0.0, 40.0, "Gravity for physics props (m/s²)");
    }
}

/// Everything that changes during play. Cloning it is a snapshot.
#[derive(Clone)]
pub struct SimState {
    pub tick: u64,
    pub seed: u64,
    pub rng: Rng,
    pub physics: PhysicsState,
    pub entities: Entities,
    pub statics: StaticWorld,
    pub scene: String,
    pub focus: Vec3,
}

pub struct Sim {
    pub config: SimConfig,
    pub state: SimState,
    pipeline: PhysicsPipeline,
    events: Vec<SimEvent>,
}

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
            },
            config,
            pipeline: PhysicsPipeline::new(),
            events: Vec::new(),
        }
    }

    /// A world populated by the named built-in scene.
    pub fn new(scene: &str, seed: u64) -> Result<Self> {
        let mut sim = Self::empty(seed);
        crate::scenes::build(&mut sim, scene)?;
        sim.state.scene = scene.to_string();
        Ok(sim)
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
            },
        );
        self.events.push(SimEvent::Spawned { id, pos: s.pos });
        id
    }

    pub fn despawn(&mut self, id: EntityId) -> bool {
        match self.state.entities.map.remove(&id) {
            Some(e) => {
                if let Some(b) = e.body {
                    self.state.physics.remove_body(b);
                }
                true
            }
            None => false,
        }
    }

    /// Moves an entity (teleport), resetting its velocity.
    pub fn set_position(&mut self, id: EntityId, pos: Vec3) -> bool {
        let Some(e) = self.state.entities.map.get_mut(&id) else { return false };
        e.pos = pos;
        if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
            b.set_translation(pos, true);
            b.set_linvel(Vector::ZERO, true);
            b.set_angvel(Vector::ZERO, true);
        }
        true
    }

    /// Advances one fixed tick.
    pub fn step(&mut self, _input: &InputFrame) {
        let dt = self.dt();
        self.state.physics.params.dt = dt as Real;
        self.state.physics.gravity = Vector::new(0.0, -self.config.gravity as Real, 0.0);
        self.run_behaviors(dt);
        let collector = EventCollector::default();
        self.state.physics.step(&mut self.pipeline, &collector);
        self.sync_from_physics();
        self.state.tick += 1;
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

    pub fn frame(&self) -> RenderFrame {
        RenderFrame {
            tick: self.state.tick,
            time: self.time(),
            dt: self.dt(),
            objects: self
                .state
                .entities
                .iter()
                .filter_map(|e| e.visual.as_ref().map(|v| RenderObject { id: e.id, pos: e.pos, rot: e.rot, visual: v.clone() }))
                .collect(),
            statics: self.state.statics.clone(),
            focus: self.state.focus,
            focus_is_player: false,
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
        h
    }
}
