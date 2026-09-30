//! Turns simulation frames into render scenes: camera rig, interpolation, style settings.

pub mod build;
pub mod camera;

pub use build::{ViewBuilder, ViewSettings};
pub use camera::{CameraParams, CameraRig};
