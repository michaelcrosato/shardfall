//! What the simulation publishes for rendering each tick. Cheap to clone (static geometry is
//! shared through `Arc`s).

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::puppet::{PuppetDef, PuppetState};
use crate::shape::Visual;
use crate::statics::StaticWorld;

/// Animation state of a character, drawn as a puppet.
#[derive(Clone, Copy, Debug)]
pub struct PuppetFrame {
    pub state: PuppetState,
    /// Distance from the entity position (capsule centre) down to the feet.
    pub feet_offset: f32,
}

#[derive(Clone, Debug)]
pub struct RenderObject {
    pub id: EntityId,
    pub pos: Vec3,
    pub rot: Quat,
    pub visual: Visual,
    pub puppet: Option<PuppetFrame>,
    /// Blink/highlight driver: bomb fuse seconds left, or < 0 for none.
    pub pulse: f32,
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
    Throw { pos: Vec3 },
    EnterRoom { room: u16 },
    ExitRoom { room: u16 },
}

/// The room the player is in, for the HUD (info card, control guide, camera defaults).
#[derive(Clone, Debug)]
pub struct RoomInfo {
    pub id: u16,
    pub key: String,
    pub def: std::sync::Arc<crate::room::RoomDef>,
}

#[derive(Clone, Debug)]
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
    pub player: Option<EntityId>,
    pub puppet_def: PuppetDef,
    pub room: Option<RoomInfo>,
    /// The simulation's configuration (shared; changes when rooms override parameters).
    pub config: std::sync::Arc<crate::sim::SimConfig>,
    pub events: Vec<SimEvent>,
}
