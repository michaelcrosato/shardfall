//! What the simulation publishes for rendering each tick. Cheap to clone (static geometry is
//! shared through `Arc`s).

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::puppet::{PuppetDef, PuppetState};
use crate::shape::Visual;
use crate::statics::StaticWorld;

/// Animation state of a character, drawn as a puppet.
#[derive(Clone, Debug)]
pub struct PuppetFrame {
    pub state: PuppetState,
    /// Distance from the entity position (capsule centre) down to the feet.
    pub feet_offset: f32,
    /// Own look (NPCs); None = `RenderFrame::puppet_def`.
    pub def: Option<std::sync::Arc<PuppetDef>>,
    /// Creature feet and swinging chains.
    pub rig: Option<crate::rig::RigView>,
    /// Colour wash (hit flash, frozen, burning...) and its strength.
    pub tint: Option<([f32; 3], f32)>,
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
    /// Deformable body: particle positions and how to draw them.
    pub soft: Option<SoftView>,
    /// Car / helicopter parts (wheels, rotors).
    pub vehicle: Option<crate::vehicle::VehicleView>,
    /// A guard's vision cone.
    pub cone: Option<crate::stealth::ConeView>,
    /// Fixed scenery (no character, never moves, not a game actor): view filters treat it
    /// as part of the environment rather than the characters and objects.
    pub scenery: bool,
}

#[derive(Clone, Debug)]
pub struct SoftView {
    pub points: Vec<Vec3>,
    /// Surface triangles (cubes, balls, cloth); empty for ropes.
    pub surface: std::sync::Arc<Vec<[u32; 3]>>,
    /// Rope segments.
    pub segments: std::sync::Arc<Vec<[u32; 2]>>,
    /// Rope thickness.
    pub radius: f32,
    /// Sheets are seen from both sides.
    pub two_sided: bool,
}

/// Things that happened during a tick, for sound and effects.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SimEvent {
    Spawned {
        id: EntityId,
        pos: Vec3,
    },
    Impact {
        pos: Vec3,
        strength: f32,
    },
    Jump {
        pos: Vec3,
    },
    Land {
        pos: Vec3,
        speed: f32,
    },
    Explosion {
        pos: Vec3,
        radius: f32,
    },
    Step {
        pos: Vec3,
    },
    Throw {
        pos: Vec3,
    },
    EnterRoom {
        room: u16,
    },
    ExitRoom {
        room: u16,
    },
    Hit {
        pos: Vec3,
        strength: f32,
    },
    Respawn {
        pos: Vec3,
    },
    CourseStart {
        pos: Vec3,
    },
    CourseFinish {
        pos: Vec3,
        time: f32,
        new_best: bool,
    },
    Gate {
        pos: Vec3,
        ok: bool,
    },
    Checkpoint {
        pos: Vec3,
    },
    Splash {
        pos: Vec3,
    },
    Pad {
        pos: Vec3,
    },
    /// A blaster shot, an enemy hit, an enemy destroyed.
    Shot {
        pos: Vec3,
    },
    Damage {
        pos: Vec3,
    },
    Destroyed {
        pos: Vec3,
        size: f32,
    },
    /// A guard raised the alarm.
    Spotted {
        pos: Vec3,
    },
    Grab {
        pos: Vec3,
    },
    Roll {
        pos: Vec3,
    },
    /// A tile starts crumbling / breaks.
    Crack {
        pos: Vec3,
    },
    Break {
        pos: Vec3,
    },
    Bounce {
        pos: Vec3,
    },
    /// Shardfall: a weapon swing, a hit landing (power = damage, element index), a kill, a
    /// slam, a spell cast, an explosion, gold, a potion, a block, a level-up.
    Swing {
        pos: Vec3,
        heavy: bool,
    },
    Strike {
        pos: Vec3,
        power: f32,
        element: u8,
        crit: bool,
    },
    Slain {
        pos: Vec3,
        size: f32,
    },
    Slam {
        pos: Vec3,
        radius: f32,
    },
    Spell {
        pos: Vec3,
        element: u8,
    },
    Blast {
        pos: Vec3,
        element: u8,
    },
    Coin {
        pos: Vec3,
    },
    Potion {
        pos: Vec3,
    },
    Block {
        pos: Vec3,
    },
    LevelUp {
        pos: Vec3,
    },
    /// An item dropped (rarity 0 normal .. 3 unique) or was picked up.
    Loot {
        pos: Vec3,
        rarity: u8,
    },
    Pickup {
        pos: Vec3,
        rarity: u8,
    },
    /// Arrived somewhere else (Shardfall place code).
    Travel {
        place: u32,
    },
}

/// The room the player is in, for the HUD (info card, control guide, camera defaults).
#[derive(Clone, Debug)]
pub struct RoomInfo {
    pub id: u16,
    pub key: String,
    pub def: std::sync::Arc<crate::room::RoomDef>,
    /// Quarter turns (clockwise from above) the room was rotated by when placed.
    pub quarters: u8,
}

/// A projectile as drawn (velocity lets the view interpolate).
#[derive(Clone, Copy, Debug)]
pub struct ProjectileView {
    pub pos: Vec3,
    pub vel: Vec3,
    pub radius: f32,
    pub color: crate::color::Color,
}

/// Game state the HUD shows.
#[derive(Clone, Debug, Default)]
pub struct HudFrame {
    pub course: Option<crate::course::CourseHud>,
    pub last_result: Option<crate::course::CourseResult>,
    /// Short message and the tick it was posted.
    pub message: Option<(String, u64)>,
    pub feel: crate::feel::FeelReport,
    /// Camera cue from pads / camera zones, and a counter that changes with it.
    pub cue: Option<std::sync::Arc<crate::zones::CameraCue>>,
    pub cue_serial: u64,
    /// Player hit flash (seconds of invulnerability left).
    pub invuln: f32,
    pub model: String,
    pub physics: PhysicsStats,
    /// View settings from pads (applied on top of the room's), and a counter that changes.
    pub view: std::collections::BTreeMap<String, crate::params::ParamValue>,
    pub view_serial: u64,
    /// Boss health bar: name and remaining fraction.
    pub boss: Option<(String, f32)>,
}

/// Physics counters for the stats overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    pub dynamic: usize,
    pub sleeping: usize,
    pub colliders: usize,
    pub joints: usize,
    pub soft_bodies: usize,
    pub particles: usize,
    pub contacts: usize,
    pub projectiles: usize,
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
    pub projectiles: Vec<ProjectileView>,
    pub hud: HudFrame,
    /// Shardfall state for the view and HUD (None outside the game).
    pub game: Option<std::sync::Arc<crate::arpg::GameFrame>>,
}
