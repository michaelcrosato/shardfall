//! Soft bodies (rapier): jelly cubes and balls, cloth, ropes. An entity with a `soft` part owns
//! one rapier soft body; the view draws its particles as a deforming surface (or a rope).

use std::sync::Arc;

use glam::{Quat, Vec3};
use rapier::dynamics::{SoftBodyBuilder, SoftBodyHandle, SpringCoefficients};
use serde::{Deserialize, Serialize};

use crate::physics::PhysicsState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SoftShape {
    /// A jelly block made of tetrahedra.
    Cuboid {
        half: Vec3,
        #[serde(default = "res3")]
        res: [u32; 3],
    },
    /// A hollow ball that keeps its volume (a balloon / bouncy ball).
    Sphere {
        radius: f32,
        #[serde(default = "two")]
        subdivisions: u32,
    },
    /// A sheet: `size` = [width along the object's X, height]; hangs down (-Y) when `vertical`,
    /// else lies along +Z.
    Cloth {
        size: [f32; 2],
        #[serde(default = "res2")]
        res: [u32; 2],
        #[serde(default)]
        vertical: bool,
    },
    /// A rope from the object's position to `pos + to` (object frame).
    Rope {
        to: Vec3,
        #[serde(default = "twenty")]
        segments: u32,
        #[serde(default = "rope_r")]
        radius: f32,
    },
}

fn res3() -> [u32; 3] {
    [4, 4, 4]
}
fn res2() -> [u32; 2] {
    [12, 12]
}
fn two() -> u32 {
    2
}
fn twenty() -> u32 {
    20
}
fn rope_r() -> f32 {
    0.05
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoftDef {
    pub shape: SoftShape,
    /// Natural frequency of the springs (Hz): higher = stiffer.
    pub stiffness: f32,
    /// Damping ratio (0 = wobbles forever, 1 = no wobble).
    pub damping: f32,
    /// Total mass (kg).
    pub mass: f32,
    /// Which particles are pinned in place: "" | "top" | "start" | "end" | "ends" | "corners" |
    /// "top_corners" | "edge".
    pub pin: String,
    /// Tear when stretched past this fraction (e.g. 0.6); 0 = unbreakable.
    pub tear: f32,
    /// Name of an object the rope's end (or the cloth's bottom corners) is tied to.
    pub attach: String,
}

impl Default for SoftDef {
    fn default() -> Self {
        Self {
            shape: SoftShape::Cuboid { half: Vec3::splat(0.5), res: res3() },
            stiffness: 8.0,
            damping: 0.3,
            mass: 4.0,
            pin: String::new(),
            tear: 0.0,
            attach: String::new(),
        }
    }
}

/// The soft-body part of an entity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoftPart {
    pub def: SoftDef,
    pub handle: Option<SoftBodyHandle>,
    /// Surface triangles (particle indices) for drawing; empty for ropes.
    pub surface: Arc<Vec<[u32; 3]>>,
    /// Rope segments (particle indices).
    pub segments: Arc<Vec<[u32; 2]>>,
    /// Particle positions and velocities kept while the region is dormant.
    #[serde(default)]
    pub saved: Vec<(Vec3, Vec3)>,
    /// Particles to attach to the `attach` object (indices).
    #[serde(default)]
    pub attach_particles: Vec<u32>,
    /// The entity the particles are tied to (restored when the region wakes up).
    #[serde(default)]
    pub tied_to: Option<crate::entity::EntityId>,
}

/// Particle indices selected by a pin mode for a generated shape.
fn pinned(def: &SoftDef, positions: &[Vec3], grid: Option<(u32, u32)>) -> Vec<u32> {
    let n = positions.len() as u32;
    if n == 0 {
        return Vec::new();
    }
    let top_y = positions.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let tops = || (0..n).filter(|&i| positions[i as usize].y > top_y - 1e-3).collect::<Vec<u32>>();
    match (def.pin.as_str(), &def.shape) {
        ("", _) => Vec::new(),
        ("start", _) => vec![0],
        ("end", _) => vec![n - 1],
        ("ends", _) => vec![0, n - 1],
        ("top", SoftShape::Cloth { vertical: false, .. }) => grid.map(|(_, nv)| (0..nv).collect()).unwrap_or_default(),
        ("top", _) => tops(),
        (mode, _) => match grid {
            // Cloth grid: particle (i, j) = i * nv + j; i along the width, j along the height.
            Some((nu, nv)) => {
                let id = |i: u32, j: u32| i * nv + j;
                match mode {
                    "top_corners" => vec![id(0, 0), id(nu - 1, 0)],
                    "corners" => vec![id(0, 0), id(nu - 1, 0), id(0, nv - 1), id(nu - 1, nv - 1)],
                    "edge" => (0..n).filter(|&k| k / nv == 0 || k / nv == nu - 1 || k % nv == 0 || k % nv == nv - 1).collect(),
                    _ => Vec::new(),
                }
            }
            None => tops(),
        },
    }
}

/// Builds the rapier soft body for `def` placed at `pos` / `rot`.
pub fn builder(def: &SoftDef, pos: Vec3, rot: Quat) -> (SoftBodyBuilder, Vec<u32>) {
    let mut grid = None;
    let mut attach = Vec::new();
    let b = match &def.shape {
        SoftShape::Cuboid { half, res } => {
            SoftBodyBuilder::cuboid(Vec3::ZERO, *half, res[0] as usize, res[1] as usize, res[2] as usize)
        }
        SoftShape::Sphere { radius, subdivisions } => SoftBodyBuilder::sphere(Vec3::ZERO, *radius, *subdivisions as usize),
        SoftShape::Cloth { size, res, vertical } => {
            let (nu, nv) = (res[0].max(2), res[1].max(2));
            let du = Vec3::X * size[0] / (nu - 1) as f32;
            let dv = if *vertical { Vec3::NEG_Y } else { Vec3::Z } * size[1] / (nv - 1) as f32;
            grid = Some((nu, nv));
            attach = vec![nv - 1, (nu - 1) * nv + nv - 1];
            let origin = -du * (nu - 1) as f32 * 0.5;
            SoftBodyBuilder::cloth(origin, du, dv, nu as usize, nv as usize)
        }
        SoftShape::Rope { to, segments, radius } => {
            let n = (*segments).max(2) as usize + 1;
            attach = vec![n as u32 - 1];
            SoftBodyBuilder::rope(Vec3::ZERO, *to, n).particle_radius(*radius)
        }
    };
    // Into the world: rotate and move the particles.
    let positions: Vec<Vec3> = b.positions.iter().map(|p| pos + rot * *p).collect();
    let pins = pinned(def, &positions, grid);
    let n = positions.len().max(1);
    let mut b = b
        .positions(positions)
        .particle_mass(def.mass / n as f32)
        .softness(SpringCoefficients::new(def.stiffness.max(0.1), def.damping.max(0.0)))
        .pinned_particles(pins);
    if def.tear > 0.0 {
        b = b.tear_strain(def.tear);
    }
    (b, attach)
}

impl PhysicsState {
    /// Creates a soft body; returns its handle and drawing topology.
    pub fn insert_soft(&mut self, def: &SoftDef, pos: Vec3, rot: Quat) -> SoftPart {
        let (b, attach_particles) = builder(def, pos, rot);
        let h = self.soft_bodies.insert(b, &mut self.bodies, &mut self.colliders);
        let (surface, segments) = match self.soft_bodies.get(h) {
            Some(sb) => {
                let surface: Vec<[u32; 3]> = sb.boundary().to_vec();
                let segments: Vec<[u32; 2]> = if matches!(def.shape, SoftShape::Rope { .. }) {
                    sb.edges().iter().map(|e| e.vertices).filter(|p| p[1] == p[0] + 1).collect()
                } else {
                    Vec::new()
                };
                (surface, segments)
            }
            None => (Vec::new(), Vec::new()),
        };
        SoftPart {
            def: def.clone(),
            handle: Some(h),
            surface: Arc::new(surface),
            segments: Arc::new(segments),
            saved: Vec::new(),
            attach_particles,
            tied_to: None,
        }
    }

    pub fn remove_soft(&mut self, h: SoftBodyHandle) {
        self.soft_bodies.remove(
            h,
            &mut self.islands,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
        );
    }

    /// Current particle positions (empty if the body is gone).
    pub fn soft_positions(&self, h: SoftBodyHandle) -> Vec<Vec3> {
        self.soft_bodies.get(h).map(|b| b.particle_positions().collect()).unwrap_or_default()
    }
}
