//! Pavilion Lite: a compact, headless-first 3D game engine for AI agents. Read AGENTS.md.
//!
//! A game is a `Game` (rules + state) acting on a `World` (entities, physics, camera) that
//! `Sim` steps at 60 ticks per second. Agents drive it through `tools` (CLI, REPL, MCP);
//! people play it in a window (`pav play <game>`).

pub mod character;
pub mod entity;
pub mod font;
pub mod input;
pub mod level;
pub mod nav;
pub mod params;
pub mod puppet;
pub mod render;
pub mod sim;
pub mod tools;
pub mod util;
pub mod view;
#[cfg(feature = "window")]
pub mod window;
pub mod world;

pub use glam;

/// Everything a game file needs: `use pavlite::prelude::*;`
pub mod prelude {
    pub use glam::{Quat, Vec2, Vec3};
    pub use serde_json::{Value, json};

    pub use crate::character::{LockAxis, MoveModel};
    pub use crate::entity::{Body, Entity, Id, Look, Shape, Spawn};
    pub use crate::input::{Input, buttons};
    pub use crate::nav::NavGrid;
    pub use crate::params::{ParamVisitor, Tunable};
    pub use crate::puppet::{Act, Held, Plan, Puppet};
    pub use crate::sim::{Game, GameDef};
    pub use crate::util::{Color, Rng, approach, dir_of, wrap_angle, yaw_of};
    pub use crate::view::Draw;
    pub use crate::world::{Camera, Event, RayHit, Shot, World};
}
