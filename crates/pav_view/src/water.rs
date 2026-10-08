//! Rippling water: every water zone's surface is a height grid run by the 2D wave equation on
//! the CPU. Whatever moves through the water (characters wading, balls, crates) pushes the
//! surface down where it is, splashes and explosions drop in bigger dents, and raindrops make
//! little rings; the waves spread, bounce off the edges and fade. Each frame the grid becomes
//! a mesh whose vertex colours carry the shading (so the ripples show in flat and cel styles
//! too) and whose normals catch the sun in the lit style. Drawing only: swimming and wading
//! stay the simulation's business.

use std::collections::HashMap;

use glam::Vec3;
use pav_core::RenderObject;
use pav_core::zones::Zone;
use pav_render::mesh::{MeshData, Vertex};

/// Simulation steps per second (the wave equation is stepped at a fixed rate).
const RATE: f32 = 60.0;
/// The biggest dent or bump (m): keeps a pile of splashes from blowing up the surface.
const MAX_HEIGHT: f32 = 0.3;

/// Wave settings (from `ViewSettings`).
#[derive(Clone, Copy, Debug)]
pub struct WaterSettings {
    /// Wave speed (m/s).
    pub speed: f32,
    /// Seconds for a ripple to lose half its height.
    pub fade: f32,
    /// Raindrops per square metre per second.
    pub rain: f32,
}

/// One water surface: heights now and one step ago on a grid over the zone.
pub struct WaterSurface {
    min: Vec3,
    max: Vec3,
    nx: usize,
    nz: usize,
    cell: f32,
    h: Vec<f32>,
    prev: Vec<f32>,
    acc: f32,
    rain_acc: f32,
    rng: u32,
    /// Positions of the things in the water at the last step (for their speed).
    seen: HashMap<u32, Vec3>,
    pub used: bool,
}

impl WaterSurface {
    /// A still surface over a zone (its top is the waterline).
    pub fn new(z: &Zone) -> Self {
        let size = z.max - z.min;
        // Fine cells for ponds, coarser for lakes (at most 160 x 160).
        let cell = (size.x.max(size.z) / 160.0).max(0.14);
        let nx = ((size.x / cell).ceil() as usize).max(2);
        let nz = ((size.z / cell).ceil() as usize).max(2);
        let n = (nx + 1) * (nz + 1);
        Self {
            min: z.min,
            max: z.max,
            nx,
            nz,
            cell,
            h: vec![0.0; n],
            prev: vec![0.0; n],
            acc: 0.0,
            rain_acc: 0.0,
            rng: 0x9E37_79B9,
            seen: HashMap::new(),
            used: true,
        }
    }

    /// The water's surface height.
    pub fn level(&self) -> f32 {
        self.max.y - 0.04
    }

    fn idx(&self, i: usize, j: usize) -> usize {
        j * (self.nx + 1) + i
    }

    /// Pushes the surface down (or up, for negative `depth`) in a soft circle around `p`.
    pub fn dent(&mut self, p: Vec3, radius: f32, depth: f32) {
        let r = radius.max(self.cell * 1.5);
        let (i0, i1) = (((p.x - r - self.min.x) / self.cell).floor() as i64, ((p.x + r - self.min.x) / self.cell).ceil() as i64);
        let (j0, j1) = (((p.z - r - self.min.z) / self.cell).floor() as i64, ((p.z + r - self.min.z) / self.cell).ceil() as i64);
        for j in j0.max(0)..=j1.min(self.nz as i64) {
            for i in i0.max(0)..=i1.min(self.nx as i64) {
                let x = self.min.x + i as f32 * self.cell;
                let z = self.min.z + j as f32 * self.cell;
                let d2 = ((x - p.x).powi(2) + (z - p.z).powi(2)) / (r * r);
                if d2 < 1.0 {
                    let k = self.idx(i as usize, j as usize);
                    let w = (1.0 - d2) * (1.0 - d2);
                    // Moved, not set moving: the previous height moves too, so the dent
                    // springs back instead of sinking the whole surface.
                    let d = (self.h[k] - depth * w).clamp(-MAX_HEIGHT, MAX_HEIGHT) - self.h[k];
                    self.h[k] += d;
                    self.prev[k] += d;
                }
            }
        }
    }

    /// Whether `p` is over this surface (horizontally).
    pub fn contains(&self, p: Vec3) -> bool {
        self.inside(p, 0.0)
    }

    fn inside(&self, p: Vec3, margin: f32) -> bool {
        p.x > self.min.x - margin && p.x < self.max.x + margin && p.z > self.min.z - margin && p.z < self.max.z + margin
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Advances the waves by `dt` seconds. Things in `objects` that cross the waterline stir
    /// it in proportion to their speed.
    pub fn step(&mut self, dt: f32, s: &WaterSettings, objects: &[RenderObject]) {
        // Catch up at most half a second (a long frame, or a tool that renders now and then).
        self.acc = (self.acc + dt).min(0.5);
        let h = 1.0 / RATE;
        while self.acc >= h {
            self.acc -= h;
            self.stir(objects, h);
            self.rain(s.rain, h);
            self.wave_step(s, h);
        }
    }

    fn stir(&mut self, objects: &[RenderObject], h: f32) {
        let level = self.level();
        let mut seen = HashMap::new();
        for o in objects {
            let (bottom, top, radius) = match &o.puppet {
                Some(p) => (o.pos.y - p.feet_offset, o.pos.y + 0.6, 0.35),
                None => {
                    let e = o.visual.shape.half_extents();
                    (o.pos.y - e.y, o.pos.y + e.y, e.x.max(e.z).clamp(0.12, 1.0))
                }
            };
            if bottom > level + 0.05 || top < level - 0.4 || !self.inside(o.pos, 0.0) {
                continue;
            }
            seen.insert(o.id.0, o.pos);
            if let Some(last) = self.seen.get(&o.id.0) {
                let v = (o.pos - *last) / h;
                let push = (Vec3::new(v.x, 0.0, v.z).length() * 0.006 + v.y.abs() * 0.02).min(0.06);
                if push > 0.0005 {
                    self.dent(o.pos, radius, push);
                }
            } else {
                // Just fell in (or walked in): a splash.
                self.dent(o.pos, radius * 1.4, 0.08);
            }
        }
        self.seen = seen;
    }

    fn rain(&mut self, rate: f32, h: f32) {
        if rate <= 0.0 {
            return;
        }
        let area = (self.max.x - self.min.x) * (self.max.z - self.min.z);
        self.rain_acc += rate * area * h;
        while self.rain_acc >= 1.0 {
            self.rain_acc -= 1.0;
            let (a, b) = (self.rand(), self.rand());
            let p = Vec3::new(self.min.x + a * (self.max.x - self.min.x), 0.0, self.min.z + b * (self.max.z - self.min.z));
            let depth = 0.012 + 0.012 * self.rand();
            self.dent(p, (self.cell * 2.6).max(0.3), depth);
        }
    }

    /// One step of the damped wave equation (each point accelerates toward the average of
    /// its neighbours); the edges reflect.
    fn wave_step(&mut self, s: &WaterSettings, h: f32) {
        let c = s.speed * h / self.cell;
        let c2 = (c * c).min(0.45);
        // Damping the velocity by d per step shrinks the amplitude by about sqrt(d): square the
        // per-step factor so `fade` is the amplitude's half-life.
        let damp = 0.25f32.powf(h / s.fade.max(0.05));
        let (nx, nz) = (self.nx, self.nz);
        let mut next = std::mem::take(&mut self.prev);
        for j in 0..=nz {
            for i in 0..=nx {
                let k = j * (nx + 1) + i;
                let at = |ii: usize, jj: usize| self.h[jj * (nx + 1) + ii];
                let l = at(i.saturating_sub(1), j);
                let r = at((i + 1).min(nx), j);
                let u = at(i, j.saturating_sub(1));
                let d = at(i, (j + 1).min(nz));
                let lap = l + r + u + d - 4.0 * self.h[k];
                // A weak spring back to the waterline settles what the waves leave behind.
                let rest = -0.002 * self.h[k];
                next[k] = (self.h[k] + (self.h[k] - next[k]) * damp + c2 * lap + rest).clamp(-MAX_HEIGHT, MAX_HEIGHT);
            }
        }
        self.prev = std::mem::replace(&mut self.h, next);
    }

    /// The surface as a mesh in world space: `color` is the water's colour (linear), `to_sun`
    /// points at the sun. Slopes facing the sun are lighter, crests get a little foam.
    pub fn mesh(&self, color: Vec3, to_sun: Vec3) -> MeshData {
        let (nx, nz) = (self.nx, self.nz);
        let level = self.level();
        let mut m = MeshData::default();
        m.vertices.reserve((nx + 1) * (nz + 1));
        let flat_light = to_sun.y.max(0.2);
        for j in 0..=nz {
            for i in 0..=nx {
                let k = self.idx(i, j);
                let hx = self.h[self.idx((i + 1).min(nx), j)] - self.h[self.idx(i.saturating_sub(1), j)];
                let hz = self.h[self.idx(i, (j + 1).min(nz))] - self.h[self.idx(i, j.saturating_sub(1))];
                // Slopes are exaggerated so centimetre ripples read from a raised camera.
                let n = Vec3::new(-hx * 4.0 / self.cell, 1.0, -hz * 4.0 / self.cell).normalize();
                let light = (n.dot(to_sun).max(0.0) - flat_light) * 1.6;
                let foam = ((self.h[k] - 0.02) * 9.0).clamp(0.0, 0.35);
                let c = (color * (1.0 + light) + Vec3::splat(foam)).max(Vec3::ZERO);
                let x = (self.min.x + i as f32 * self.cell).min(self.max.x);
                let z = (self.min.z + j as f32 * self.cell).min(self.max.z);
                m.vertices.push(Vertex {
                    pos: [x, level + self.h[k], z],
                    normal: n.to_array(),
                    uv: [i as f32 / nx as f32, j as f32 / nz as f32],
                    color: [c.x, c.y, c.z, 1.0],
                });
            }
        }
        m.indices.reserve(nx * nz * 6);
        for j in 0..nz {
            for i in 0..nx {
                let a = (j * (nx + 1) + i) as u32;
                let b = a + (nx + 1) as u32;
                m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        m
    }

    /// Height of the surface above the still waterline at grid point nearest `p` (tests).
    pub fn height_at(&self, p: Vec3) -> f32 {
        let i = (((p.x - self.min.x) / self.cell).round().max(0.0) as usize).min(self.nx);
        let j = (((p.z - self.min.z) / self.cell).round().max(0.0) as usize).min(self.nz);
        self.h[self.idx(i, j)]
    }

    /// The biggest ripple anywhere (tests).
    pub fn roughness(&self) -> f32 {
        self.h.iter().fold(0.0f32, |a, h| a.max(h.abs()))
    }
}

/// A zone's key for keeping its surface between frames (its box in centimetres).
pub fn zone_key(z: &Zone) -> [i32; 6] {
    let q = |v: f32| (v * 100.0).round() as i32;
    [q(z.min.x), q(z.min.y), q(z.min.z), q(z.max.x), q(z.max.y), q(z.max.z)]
}

/// Water zones with the same waterline that together tile a rectangle become one zone, so a
/// pool built from several strips (steps, different depths) ripples as one surface.
pub fn merge_zones(zones: &[&Zone]) -> Vec<Zone> {
    let mut out: Vec<Zone> = zones.iter().map(|z| (*z).clone()).collect();
    let area = |z: &Zone| (z.max.x - z.min.x) * (z.max.z - z.min.z);
    loop {
        let mut merged = false;
        'pairs: for a in 0..out.len() {
            for b in a + 1..out.len() {
                let (za, zb) = (&out[a], &out[b]);
                if (za.max.y - zb.max.y).abs() > 0.01 {
                    continue;
                }
                let min = za.min.min(zb.min);
                let max = za.max.max(zb.max);
                let bbox = (max.x - min.x) * (max.z - min.z);
                let touching = za.min.x <= zb.max.x + 0.01
                    && zb.min.x <= za.max.x + 0.01
                    && za.min.z <= zb.max.z + 0.01
                    && zb.min.z <= za.max.z + 0.01;
                if touching && (area(za) + area(zb) - bbox).abs() < 0.01 * bbox.max(1.0) {
                    let mut m = za.clone();
                    m.min = min;
                    m.max = max;
                    out[a] = m;
                    out.swap_remove(b);
                    merged = true;
                    break 'pairs;
                }
            }
        }
        if !merged {
            return out;
        }
    }
}
