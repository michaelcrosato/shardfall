//! Turns simulation frames into render scenes: camera rig, interpolation, style settings.

pub mod arpg;
pub mod build;
pub mod camera;
pub mod fx;
pub mod look;
pub mod vehicles;
pub mod water;

pub use build::{ViewBuilder, ViewSettings};
pub use camera::{CameraParams, CameraRig};
