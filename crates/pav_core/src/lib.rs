//! Pavilion simulation core: headless (no window, GPU or audio dependencies).

pub mod color;
pub mod entity;
pub mod frame;
pub mod input;
pub mod params;
pub mod physics;
pub mod rng;
pub mod scenes;
pub mod shape;
pub mod sim;
pub mod statics;

pub use color::Color;
pub use entity::{Behavior, BodyKind, Entity, EntityId, Spawn};
pub use frame::{RenderFrame, RenderObject, SimEvent};
pub use glam;
pub use input::InputFrame;
pub use rapier;
pub use shape::{Look, Shape, Visual};
pub use sim::{Sim, SimConfig, SimState};
