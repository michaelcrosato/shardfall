//! Pavilion simulation core: headless (no window, GPU or audio dependencies).
// `x as Real` casts are no-ops with f32 physics but required by the f64 switch.
#![allow(clippy::unnecessary_cast)]

pub mod character;
pub mod color;
pub mod entity;
pub mod frame;
pub mod history;
pub mod input;
pub mod level;
pub mod params;
pub mod physics;
pub mod puppet;
pub mod rng;
pub mod room;
pub mod scenes;
pub mod shape;
pub mod sim;
pub mod statics;
pub mod terrain;
pub mod world;

pub use color::Color;
pub use entity::{Behavior, BodyKind, Entity, EntityId, Spawn};
pub use frame::{RenderFrame, RenderObject, SimEvent};
pub use glam;
pub use input::InputFrame;
pub use rapier;
pub use shape::{Look, Shape, Visual};
pub use sim::{Sim, SimConfig, SimState};
