//! Game objects. An entity is one "fat" struct with optional parts, which keeps it cloneable
//! (for snapshots), serializable (for agents) and easy to extend: add a field.

use std::collections::{BTreeMap, VecDeque};

use glam::{Quat, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::shape::Visual;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

/// Simple scripted behaviours usable from scenes and room files.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub pos: Vec3,
    pub rot: Quat,
    pub body_kind: BodyKind,
    #[serde(skip)]
    pub body: Option<RigidBodyHandle>,
    pub visual: Option<Visual>,
    pub behavior: Behavior,
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

#[derive(Clone, Debug, Default)]
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
