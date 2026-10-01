//! Pavilion renderer: a stylized 3D renderer on wgpu. Knows nothing about the simulation.

pub mod capture;
pub(crate) mod fx;
pub mod gpu;
pub mod mesh;
pub mod renderer;
pub mod scene;
pub mod shadows;
pub mod text;

pub use mesh::{MeshData, MeshKey};
pub use renderer::{RenderStats, Renderer};
pub use scene::*;
pub use text::{Anchor, Text3d};
pub use wgpu;
