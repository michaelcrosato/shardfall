//! Pavilion simulation core: headless (no window, GPU or audio dependencies).
// `x as Real` casts are no-ops with f32 physics but required by the f64 switch.
#![allow(clippy::unnecessary_cast)]

pub mod ai;
pub mod arpg;
pub mod behaviors;
pub mod character;
pub mod color;
pub mod course;
pub mod destruct;
pub mod entity;
pub mod feel;
pub mod frame;
pub mod fxdef;
pub mod guide;
pub mod health;
pub mod history;
pub mod input;
pub mod joints;
pub mod level;
pub mod nav;
pub mod params;
pub mod parts;
pub mod physics;
pub mod projectile;
pub mod puppet;
pub mod rig;
pub mod rng;
pub mod room;
pub mod scenes;
pub mod shape;
pub mod sim;
pub mod softbody;
pub mod statics;
pub mod stealth;
pub mod terrain;
pub mod vehicle;
pub mod world;
pub mod zones;

pub use color::Color;
pub use entity::{Behavior, BodyKind, Entity, EntityId, Spawn};
pub use frame::{RenderFrame, RenderObject, SimEvent};
pub use glam;
pub use input::InputFrame;
pub use rapier;
pub use shape::{Look, Shape, Visual};
pub use sim::{Sim, SimConfig, SimState};
