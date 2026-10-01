//! Procedural wilderness: terraced tile terrain generated from the seed, one mesh + one
//! triangle-mesh collider per 32 m chunk, plus trees and rocks.

use glam::{Quat, Vec2, Vec3};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::params::{ParamVisitor, Tunable};
use crate::rng::{hash_f32, hash3};
use crate::shape::{Look, Shape};
use crate::statics::{CHUNK_SIZE, Decor};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainParams {
    /// Height range of hills (m).
    pub amplitude: f32,
    /// Size of features (m).
    pub scale: f32,
    /// Terrace step height (m). Steps up to ~0.32 m can be walked up.
    pub step: f32,
    /// Water level (m).
    pub water: f32,
    pub tree_density: f32,
    pub rock_density: f32,
    pub prop_density: f32,
    /// Flattening margin around the pavilion (m).
    pub flatten_margin: f32,
    /// Streaming radius around interest points (chunks); regions beyond `unload` go dormant.
    pub load_radius: i32,
    pub unload_radius: i32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            amplitude: 7.0,
            scale: 60.0,
            step: 0.3,
            water: -1.5,
            tree_density: 0.014,
            rock_density: 0.006,
            prop_density: 0.0015,
            flatten_margin: 14.0,
            load_radius: 2,
            unload_radius: 3,
        }
    }
}

impl Tunable for TerrainParams {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("amplitude", &mut self.amplitude, 0.0, 30.0, "Hill height (m) — affects chunks generated afterwards");
        v.float("scale", &mut self.scale, 10.0, 300.0, "Feature size (m)");
        v.float("step", &mut self.step, 0.05, 2.0, "Terrace step (m)");
        v.float("water", &mut self.water, -10.0, 5.0, "Water level (m)");
        v.float("tree_density", &mut self.tree_density, 0.0, 0.2, "Trees per cell");
        v.float("rock_density", &mut self.rock_density, 0.0, 0.1, "Rocks per cell");
        v.float("prop_density", &mut self.prop_density, 0.0, 0.05, "Loose crates per cell");
        v.float("flatten_margin", &mut self.flatten_margin, 0.0, 60.0, "Flat ground around the pavilion (m)");
        v.int("load_radius", &mut self.load_radius, 1, 8, "Chunks loaded around the player");
        v.int("unload_radius", &mut self.unload_radius, 2, 10, "Chunks beyond this go dormant");
    }
}

/// Rectangles (xz min, max) where terrain is flat and absent (the pavilion).
#[derive(Clone, Debug, Default)]
pub struct Footprint {
    pub rects: Vec<(Vec2, Vec2)>,
}

impl Footprint {
    /// Distance from `p` to the nearest rectangle (0 inside).
    pub fn distance(&self, p: Vec2) -> f32 {
        self.rects
            .iter()
            .map(|(lo, hi)| {
                let d = (*lo - p).max(p - *hi).max(Vec2::ZERO);
                d.length()
            })
            .fold(f32::INFINITY, f32::min)
    }
    pub fn contains(&self, p: Vec2) -> bool {
        self.rects.iter().any(|(lo, hi)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y)
    }
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise in [0, 1].
fn noise(seed: u64, x: f32, z: f32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (smooth(x - x0), smooth(z - z0));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let h = |dx: i32, dz: i32| hash_f32(seed, ix + dx, iz + dz, 7);
    let a = h(0, 0) + (h(1, 0) - h(0, 0)) * fx;
    let b = h(0, 1) + (h(1, 1) - h(0, 1)) * fx;
    a + (b - a) * fz
}

fn fbm(seed: u64, x: f32, z: f32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..5 {
        sum += noise(seed.wrapping_add(o * 101), x * freq, z * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

pub struct TerrainGen<'a> {
    pub seed: u64,
    pub p: &'a TerrainParams,
    pub footprint: &'a Footprint,
}

#[derive(Clone, Copy, PartialEq)]
enum Cell {
    Hole,
    Water(f32),
    Land(f32),
}

impl TerrainGen<'_> {
    /// Smooth height before terracing.
    pub fn raw_height(&self, x: f32, z: f32) -> f32 {
        let s = self.p.scale.max(1.0);
        let base = (fbm(self.seed, x / s, z / s) - 0.45) * 2.0 * self.p.amplitude;
        let ridge = (1.0 - (fbm(self.seed ^ 0xabc, x / (s * 2.3), z / (s * 2.3)) * 2.0 - 1.0).abs()) * self.p.amplitude * 0.6;
        let h = base + ridge * 0.5;
        let d = self.footprint.distance(Vec2::new(x, z));
        let m = self.p.flatten_margin.max(0.01);
        let w = smooth(((d - m * 0.25) / (m * 0.75)).clamp(0.0, 1.0));
        h * w
    }

    fn cell(&self, x: i32, z: i32) -> Cell {
        let (cx, cz) = (x as f32 + 0.5, z as f32 + 0.5);
        if self.footprint.contains(Vec2::new(cx, cz)) {
            return Cell::Hole;
        }
        let h = self.raw_height(cx, cz);
        let step = self.p.step.max(0.01);
        let q = (h / step).round() * step;
        if q < self.p.water { Cell::Water(self.p.water) } else { Cell::Land(q) }
    }

    /// Top of the terrain at a cell (holes count as 0, the pavilion floor).
    pub fn cell_height(&self, x: i32, z: i32) -> f32 {
        match self.cell(x, z) {
            Cell::Hole => 0.0,
            Cell::Water(h) | Cell::Land(h) => h,
        }
    }

    pub fn generate(&self, cx: i32, cz: i32) -> TerrainPatch {
        let n = CHUNK_SIZE as i32;
        let (x0, z0) = (cx * n, cz * n);
        let mut m = TerrainPatch { cx, cz, ..Default::default() };
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        let grass = Color::hex("#88b96f");
        let grass2 = Color::hex("#7aa865");
        let dirt = Color::hex("#a88b65");
        let rock = Color::hex("#9b9a96");
        let snow = Color::hex("#eef0f2");
        let sand = Color::hex("#d8c99a");
        let water = Color::hex("#4f86c6");
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (x0 + i, z0 + j);
                let c = self.cell(x, z);
                let (h, top) = match c {
                    Cell::Hole => continue,
                    Cell::Water(h) => (h, water),
                    Cell::Land(h) => {
                        let v = hash_f32(self.seed, x, z, 3);
                        let col = if h > self.p.amplitude * 1.05 {
                            snow
                        } else if h > self.p.amplitude * 0.7 {
                            rock
                        } else if h < self.p.water + 0.35 {
                            sand
                        } else if v < 0.5 {
                            grass
                        } else {
                            grass2
                        };
                        (h, col.scale(0.94 + 0.12 * hash_f32(self.seed, x, z, 4)))
                    }
                };
                let (fx0, fz0, fx1, fz1) = (x as f32, z as f32, x as f32 + 1.0, z as f32 + 1.0);
                m.quad([[fx0, h, fz1], [fx1, h, fz1], [fx1, h, fz0], [fx0, h, fz0]], [0.0, 1.0, 0.0], top);
                let side = if matches!(c, Cell::Water(_)) { water } else { dirt.lerp(top, 0.25) };
                let mut lowest = h;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nh = self.cell_height(x + dx, z + dz);
                    if nh >= h - 1e-4 {
                        continue;
                    }
                    lowest = lowest.min(nh);
                    let s = side.scale(0.82);
                    match (dx, dz) {
                        (1, 0) => m.quad([[fx1, nh, fz1], [fx1, nh, fz0], [fx1, h, fz0], [fx1, h, fz1]], [1.0, 0.0, 0.0], s),
                        (-1, 0) => m.quad([[fx0, nh, fz0], [fx0, nh, fz1], [fx0, h, fz1], [fx0, h, fz0]], [-1.0, 0.0, 0.0], s),
                        (0, 1) => m.quad([[fx0, nh, fz1], [fx1, nh, fz1], [fx1, h, fz1], [fx0, h, fz1]], [0.0, 0.0, 1.0], s),
                        _ => m.quad([[fx1, nh, fz0], [fx0, nh, fz0], [fx0, h, fz0], [fx1, h, fz0]], [0.0, 0.0, -1.0], s),
                    }
                }
                lo = lo.min(Vec3::new(fx0, lowest, fz0));
                hi = hi.max(Vec3::new(fx1, h, fz1));
            }
        }
        if lo.x == f32::MAX {
            lo = Vec3::new(x0 as f32, 0.0, z0 as f32);
            hi = lo;
        }
        m.min = lo;
        m.max = hi;
        m.build_shape();
        m
    }

    /// Trees and rocks for a chunk.
    pub fn decor(&self, cx: i32, cz: i32) -> Vec<Decor> {
        let n = CHUNK_SIZE as i32;
        let mut out = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (cx * n + i, cz * n + j);
                let Cell::Land(h) = self.cell(x, z) else { continue };
                if self.footprint.distance(Vec2::new(x as f32, z as f32)) < self.p.flatten_margin * 0.5 {
                    continue;
                }
                let r = hash_f32(self.seed, x, z, 11);
                let jitter = |k: i32| hash_f32(self.seed, x, z, 20 + k) * 0.6 - 0.3;
                let base = Vec3::new(x as f32 + 0.5 + jitter(0), h, z as f32 + 0.5 + jitter(1));
                if r < self.p.tree_density && h < self.p.amplitude * 0.75 && h > self.p.water + 0.4 {
                    let hh = 0.55 + hash_f32(self.seed, x, z, 12) * 0.5;
                    out.push(Decor {
                        shape: Shape::Cylinder { half_height: hh, radius: 0.17 },
                        pos: base + Vec3::Y * hh,
                        rot: Quat::IDENTITY,
                        color: Color::hex("#7a5232"),
                        look: Look::Cel,
                        emissive: 0.0,
                        solid: true,
                        collider: None,
                    });
                    let leaves = ["#4f9a4a", "#5fae54", "#3f8a46", "#6dbb5c"];
                    let lc = Color::hex(leaves[(hash3(self.seed, x, z, 13) % 4) as usize]);
                    let blobs = 1 + (hash3(self.seed, x, z, 14) % 3) as i32;
                    for b in 0..blobs {
                        let a = b as f32 * 2.1 + r * 40.0;
                        let off = if blobs == 1 { Vec3::ZERO } else { Vec3::new(a.cos(), 0.0, a.sin()) * 0.45 };
                        let rad = 0.7 + hash_f32(self.seed, x, z, 15 + b) * 0.4;
                        out.push(Decor {
                            shape: Shape::Sphere { radius: rad },
                            pos: base + Vec3::Y * (hh * 2.0 + rad * 0.6 + b as f32 * 0.25) + off,
                            rot: Quat::IDENTITY,
                            color: lc.scale(0.9 + 0.2 * hash_f32(self.seed, x, z, 30 + b)),
                            look: Look::Cel,
                            emissive: 0.0,
                            solid: false,
                            collider: None,
                        });
                    }
                } else if r > 1.0 - self.p.rock_density {
                    let s = 0.25 + hash_f32(self.seed, x, z, 16) * 0.45;
                    let half = Vec3::new(s * 1.2, s * 0.8, s);
                    out.push(Decor {
                        shape: Shape::RoundedBox { half, radius: (s * 0.35).min(0.2) },
                        pos: base + Vec3::Y * half.y * 0.8,
                        rot: Quat::from_rotation_y(r * 300.0),
                        color: Color::hex("#8d8d8a").scale(0.9 + 0.2 * hash_f32(self.seed, x, z, 17)),
                        look: Look::Cel,
                        emissive: 0.0,
                        solid: true,
                        collider: None,
                    });
                }
            }
        }
        out
    }

    /// Cells that get a loose prop (for persistence tests): world positions on the ground.
    pub fn props(&self, cx: i32, cz: i32) -> Vec<Vec3> {
        let n = CHUNK_SIZE as i32;
        let mut out = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (cx * n + i, cz * n + j);
                if hash_f32(self.seed, x, z, 40) < self.p.prop_density {
                    if let Cell::Land(h) = self.cell(x, z) {
                        if self.footprint.distance(Vec2::new(x as f32, z as f32)) > 3.0 {
                            out.push(Vec3::new(x as f32 + 0.5, h, z as f32 + 0.5));
                        }
                    }
                }
            }
        }
        out
    }
}

/// One chunk of terrain surface: render mesh data and the collision shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TerrainPatch {
    pub cx: i32,
    pub cz: i32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
    #[serde(skip)]
    shape: Option<SharedShape>,
}

impl TerrainPatch {
    fn quad(&mut self, p: [[f32; 3]; 4], n: [f32; 3], c: Color) {
        let i0 = self.positions.len() as u32;
        for v in p {
            self.positions.push(v);
            self.normals.push(n);
            self.colors.push(c.0);
        }
        self.indices.extend_from_slice(&[i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
    }

    fn build_shape(&mut self) {
        self.shape = self.make_shape();
    }

    fn make_shape(&self) -> Option<SharedShape> {
        if self.indices.is_empty() {
            return None;
        }
        let verts: Vec<Vector> = self.positions.iter().map(|p| Vector::new(p[0] as Real, p[1] as Real, p[2] as Real)).collect();
        let idx: Vec<[u32; 3]> = self.indices.chunks(3).map(|t| [t[0], t[1], t[2]]).collect();
        SharedShape::trimesh_with_flags(verts, idx, TriMeshFlags::FIX_INTERNAL_EDGES | TriMeshFlags::MERGE_DUPLICATE_VERTICES)
            .ok()
    }

    /// Collider for this patch (the shape is shared, so re-activating a chunk is cheap).
    pub fn collider(&self) -> ColliderBuilder {
        let shape = self.shape.clone().or_else(|| self.make_shape()).unwrap_or_else(|| SharedShape::ball(0.01));
        ColliderBuilder::new(shape).friction(0.8)
    }
}
