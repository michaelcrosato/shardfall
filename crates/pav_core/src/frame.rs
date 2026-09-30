//! What the simulation publishes for rendering each tick. Cheap to clone (static geometry is
//! shared through `Arc`s).

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::shape::Visual;
use crate::statics::StaticWorld;

#[derive(Clone, Debug)]
pub struct RenderObject {
    pub id: EntityId,
    pub pos: Vec3,
    pub rot: Quat,
    pub visual: Visual,
}

/// Things that happened during a tick, for sound and effects.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimEvent {
    Spawned { id: EntityId, pos: Vec3 },
    Impact { pos: Vec3, strength: f32 },
    Jump { pos: Vec3 },
    Land { pos: Vec3, speed: f32 },
    Explosion { pos: Vec3, radius: f32 },
    Step { pos: Vec3 },
}

#[derive(Clone, Debug, Default)]
pub struct RenderFrame {
    pub tick: u64,
    pub time: f64,
    pub dt: f32,
    pub objects: Vec<RenderObject>,
    pub statics: StaticWorld,
    /// Suggested camera focus (the player, or the scene centre).
    pub focus: Vec3,
    /// True when `focus` is a player character (enables cutaway/fade helpers).
    pub focus_is_player: bool,
    pub events: Vec<SimEvent>,
}
