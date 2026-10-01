//! Game objects. An entity is one "fat" struct with optional parts, which keeps it cloneable
//! (for snapshots), serializable (for agents) and easy to extend: add a field.

use std::collections::{BTreeMap, VecDeque};

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::character::Character;
use crate::shape::Visual;
use crate::statics::RegionKey;

/// Physical surface properties (kept so sleeping entities can be recreated).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self { density: 1.0, friction: 0.5, restitution: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

/// Simple scripted behaviours usable from scenes and room files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Behavior {
    #[default]
    None,
    /// Drops random props over an area, keeping at most `max` alive.
    Rain {
        interval: u32,
        max: u32,
        area: f32,
        height: f32,
        #[serde(default)]
        timer: u32,
        #[serde(default)]
        spawned: VecDeque<EntityId>,
    },
    /// Kinematic rotation about Y (radians per second).
    Spin { speed: f32 },
    /// Moves back and forth along `offset` (kinematic bodies: platforms, doors, crushers).
    Move(MoverDef),
    /// Turns about an axis, continuously or swinging (sweepers, pendulums, windmills).
    Rotate(RotatorDef),
    /// Fires projectiles in patterns (dodge gauntlet, bullet hell).
    Emitter(EmitterDef),
}

/// Back-and-forth motion. Vectors are in the object's own frame (they turn with the room).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoverDef {
    /// Displacement at the far end (m).
    pub offset: Vec3,
    /// Seconds for a full there-and-back cycle.
    pub period: f32,
    /// 0..1 offset into the cycle (desynchronise neighbours).
    pub phase: f32,
    /// Share of the cycle spent waiting at each end (0..0.9): doors and crushers.
    pub hold: f32,
    /// Ease in/out (false = constant speed).
    pub smooth: bool,
    /// Start pose (filled in on the first tick).
    pub origin: Option<(Vec3, Quat)>,
}

impl Default for MoverDef {
    fn default() -> Self {
        Self { offset: Vec3::new(0.0, 0.0, 3.0), period: 4.0, phase: 0.0, hold: 0.0, smooth: true, origin: None }
    }
}

impl MoverDef {
    /// 0..1 position along the path at time `t`.
    pub fn amount(&self, t: f32) -> f32 {
        let u = (t / self.period.max(0.05) + self.phase).rem_euclid(1.0);
        let tri = 1.0 - (2.0 * u - 1.0).abs();
        let h = self.hold.clamp(0.0, 0.9);
        let s = ((tri - h * 0.5) / (1.0 - h)).clamp(0.0, 1.0);
        if self.smooth { s * s * (3.0 - 2.0 * s) } else { s }
    }
}

/// Rotation about `axis` through `pivot` (both in the object's own frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotatorDef {
    pub axis: Vec3,
    /// Degrees per second (continuous rotation).
    pub speed: f32,
    /// Swing amplitude in degrees (0 = continuous rotation).
    pub swing: f32,
    /// Seconds per swing cycle.
    pub period: f32,
    pub phase: f32,
    /// Rotation centre relative to the object's centre.
    pub pivot: Vec3,
    pub origin: Option<(Vec3, Quat)>,
}

impl Default for RotatorDef {
    fn default() -> Self {
        Self { axis: Vec3::Y, speed: 90.0, swing: 0.0, period: 2.0, phase: 0.0, pivot: Vec3::ZERO, origin: None }
    }
}

impl RotatorDef {
    pub fn angle(&self, t: f32) -> f32 {
        if self.swing > 0.0 {
            self.swing.to_radians() * ((t / self.period.max(0.05) + self.phase) * std::f32::consts::TAU).sin()
        } else {
            (self.speed * t + self.phase * 360.0).to_radians()
        }
    }
}

crate::choice_enum! {
    /// Projectile emitter patterns.
    #[derive(Default)]
    pub enum Pattern {
        /// `count` bullets in a fan of `spread` degrees toward the player.
        #[default]
        Aimed => "aimed",
        /// A fan along the emitter's forward direction (+Z of the object).
        Forward => "forward",
        /// `count` bullets evenly around a circle.
        Ring => "ring",
        /// A ring that turns by `spin` degrees per shot.
        Spiral => "spiral",
        /// Random directions within `spread` of forward.
        Random => "random",
    }
}

/// Projectile emitter settings plus its running state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterDef {
    pub pattern: Pattern,
    /// Seconds between shots.
    pub interval: f32,
    /// Bullets per shot.
    pub count: u32,
    /// Fan width (degrees).
    pub spread: f32,
    /// Spiral rotation per shot (degrees).
    pub spin: f32,
    pub speed: f32,
    pub radius: f32,
    pub color: String,
    /// Seconds a bullet lives.
    pub life: f32,
    pub knockback: f32,
    /// Only fires while the player is this close (m).
    pub range: f32,
    /// Shots per burst, then `pause` seconds of rest (0 = no bursts).
    pub burst: u32,
    pub pause: f32,
    /// Height of the muzzle relative to the object's centre.
    pub height: f32,
    /// Seconds before the first shot (desynchronise emitters).
    pub delay: f32,
    /// Downward acceleration of the bullets (0 = straight lines).
    pub gravity: f32,
    pub timer: f32,
    pub angle: f32,
    pub shots: u32,
}

impl Default for EmitterDef {
    fn default() -> Self {
        Self {
            pattern: Pattern::Aimed,
            interval: 1.0,
            count: 1,
            spread: 0.0,
            spin: 12.0,
            speed: 6.0,
            radius: 0.15,
            color: "#ff5a3c".into(),
            life: 6.0,
            knockback: 6.0,
            range: 30.0,
            burst: 0,
            pause: 1.0,
            height: 0.0,
            delay: 0.0,
            gravity: 0.0,
            timer: 0.0,
            angle: 0.0,
            shots: 0,
        }
    }
}

/// Touching this entity hurts: knockback, and optionally back to the last checkpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hazard {
    pub knockback: f32,
    pub respawn: bool,
}

impl Default for Hazard {
    fn default() -> Self {
        Self { knockback: 7.0, respawn: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// Visual only, no physics.
    #[default]
    None,
    /// Immovable collider.
    Fixed,
    /// Moved by code (platforms, doors).
    Kinematic,
    /// Fully simulated.
    Dynamic,
}

/// A lit fuse.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bomb {
    pub fuse: f32,
    pub radius: f32,
    pub owner: Option<EntityId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub pos: Vec3,
    pub rot: Quat,
    pub body_kind: BodyKind,
    pub body: Option<RigidBodyHandle>,
    pub visual: Option<Visual>,
    pub behavior: Behavior,
    #[serde(default)]
    pub character: Option<Box<Character>>,
    #[serde(default)]
    pub bomb: Option<Bomb>,
    /// Ticks until the entity despawns by itself (debris).
    #[serde(default)]
    pub lifetime: Option<u32>,
    /// The region (room / terrain chunk) the entity belongs to, for streaming and resets.
    #[serde(default)]
    pub region: Option<RegionKey>,
    #[serde(default)]
    pub material: Material,
    #[serde(default)]
    pub hazard: Option<Hazard>,
}

/// Everything needed to create an entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spawn {
    #[serde(default)]
    pub name: String,
    pub pos: Vec3,
    #[serde(default = "quat_identity")]
    pub rot: Quat,
    #[serde(default)]
    pub visual: Option<Visual>,
    #[serde(default)]
    pub body: BodyKind,
    #[serde(default = "one")]
    pub density: f32,
    #[serde(default = "half")]
    pub friction: f32,
    #[serde(default)]
    pub restitution: f32,
    #[serde(default)]
    pub behavior: Behavior,
    #[serde(default)]
    pub region: Option<RegionKey>,
    #[serde(default)]
    pub hazard: Option<Hazard>,
}

fn quat_identity() -> Quat {
    Quat::IDENTITY
}
fn one() -> f32 {
    1.0
}
fn half() -> f32 {
    0.5
}

impl Spawn {
    pub fn new(name: &str, pos: Vec3) -> Self {
        Self {
            name: name.into(),
            pos,
            rot: Quat::IDENTITY,
            visual: None,
            body: BodyKind::None,
            density: 1.0,
            friction: 0.5,
            restitution: 0.0,
            behavior: Behavior::None,
            region: None,
            hazard: None,
        }
    }
    pub fn visual(mut self, v: Visual) -> Self {
        self.visual = Some(v);
        self
    }
    pub fn body(mut self, b: BodyKind) -> Self {
        self.body = b;
        self
    }
    pub fn rot(mut self, r: Quat) -> Self {
        self.rot = r;
        self
    }
    pub fn behavior(mut self, b: Behavior) -> Self {
        self.behavior = b;
        self
    }
    pub fn restitution(mut self, r: f32) -> Self {
        self.restitution = r;
        self
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entities {
    pub map: BTreeMap<EntityId, Entity>,
    pub next: u32,
}

impl Entities {
    pub fn alloc_id(&mut self) -> EntityId {
        self.next += 1;
        EntityId(self.next)
    }
    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.map.get(&id)
    }
    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.map.get_mut(&id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.map.values()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn find(&self, name: &str) -> Option<&Entity> {
        self.map.values().find(|e| e.name == name)
    }
}
