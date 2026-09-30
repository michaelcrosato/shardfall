//! Procedural mesh generation. Every built-in mesh fits the unit cube [-0.5, 0.5]^3
//! so an instance's scale equals its size. Winding is counter-clockwise when seen from outside.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// Identifies a mesh. Built-ins and rounded boxes are generated on demand by the renderer;
/// `Custom` meshes must be uploaded with `Renderer::upsert_mesh` first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MeshKey {
    Cube,
    Sphere,
    Cylinder,
    Cone,
    Plane,
    /// Rounded box with absolute half extents and corner radius, in millimetres.
    RoundedBox {
        half_mm: [u32; 3],
        radius_mm: u32,
    },
    Custom(u64),
}

impl MeshKey {
    pub fn rounded_box(half: Vec3, radius: f32) -> Self {
        let mm = |v: f32| (v.max(0.0) * 1000.0).round() as u32;
        MeshKey::RoundedBox { half_mm: [mm(half.x), mm(half.y), mm(half.z)], radius_mm: mm(radius) }
    }

    pub fn generate(&self) -> Option<MeshData> {
        Some(match *self {
            MeshKey::Cube => MeshData::cube(),
            MeshKey::Sphere => MeshData::sphere(32, 16),
            MeshKey::Cylinder => MeshData::cylinder(32),
            MeshKey::Cone => MeshData::cone(32),
            MeshKey::Plane => MeshData::plane(),
            MeshKey::RoundedBox { half_mm, radius_mm } => {
                let h = Vec3::new(half_mm[0] as f32, half_mm[1] as f32, half_mm[2] as f32) / 1000.0;
                MeshData::rounded_box(h, radius_mm as f32 / 1000.0, 4)
            }
            MeshKey::Custom(_) => return None,
        })
    }
}

/// Tangent frame (u, v) for a face with normal n, such that cross(u, v) == n.
fn face_frame(n: Vec3) -> (Vec3, Vec3) {
    let v = if n.y.abs() > 0.5 { Vec3::Z * n.y.signum() } else { Vec3::Y };
    let u = v.cross(n);
    (u, v)
}

const FACES: [Vec3; 6] = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];

impl MeshData {
    fn push(&mut self, pos: Vec3, normal: Vec3, uv: [f32; 2]) -> u32 {
        self.vertices.push(Vertex { pos: pos.to_array(), normal: normal.to_array(), uv });
        (self.vertices.len() - 1) as u32
    }

    /// Adds a quad; corners must be counter-clockwise seen from the side `normal` points to.
    pub fn push_quad(&mut self, p: [Vec3; 4], normal: Vec3) {
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let i0 = self.push(p[0], normal, uvs[0]);
        for k in 1..4 {
            self.push(p[k], normal, uvs[k]);
        }
        self.indices.extend_from_slice(&[i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
    }

    pub fn cube() -> Self {
        let mut m = MeshData::default();
        for n in FACES {
            let (u, v) = face_frame(n);
            let c = n * 0.5;
            m.push_quad([c - u * 0.5 - v * 0.5, c + u * 0.5 - v * 0.5, c + u * 0.5 + v * 0.5, c - u * 0.5 + v * 0.5], n);
        }
        m
    }

    /// Unit plane in XZ facing +Y.
    pub fn plane() -> Self {
        let mut m = MeshData::default();
        let (u, v) = face_frame(Vec3::Y);
        m.push_quad([-u * 0.5 - v * 0.5, u * 0.5 - v * 0.5, u * 0.5 + v * 0.5, -u * 0.5 + v * 0.5], Vec3::Y);
        m
    }

    pub fn sphere(segments: u32, rings: u32) -> Self {
        let mut m = MeshData::default();
        for i in 0..=rings {
            let theta = std::f32::consts::PI * i as f32 / rings as f32;
            for j in 0..=segments {
                let phi = std::f32::consts::TAU * j as f32 / segments as f32;
                let n = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
                m.push(n * 0.5, n, [j as f32 / segments as f32, i as f32 / rings as f32]);
            }
        }
        let row = segments + 1;
        for i in 0..rings {
            for j in 0..segments {
                let a = i * row + j;
                let b = a + row;
                m.indices.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
            }
        }
        m
    }

    pub fn cylinder(segments: u32) -> Self {
        Self::lathe(segments, 0.5, 0.5)
    }

    pub fn cone(segments: u32) -> Self {
        Self::lathe(segments, 0.5, 0.0)
    }

    /// Solid of revolution around Y from y=-0.5 (radius r0) to y=0.5 (radius r1), with caps.
    fn lathe(segments: u32, r0: f32, r1: f32) -> Self {
        let mut m = MeshData::default();
        let slope = (r0 - r1) / 1.0;
        for j in 0..=segments {
            let phi = std::f32::consts::TAU * j as f32 / segments as f32;
            let (s, c) = phi.sin_cos();
            let n = Vec3::new(c, slope, s).normalize();
            let u = j as f32 / segments as f32;
            m.push(Vec3::new(c * r0, -0.5, s * r0), n, [u, 1.0]);
            m.push(Vec3::new(c * r1, 0.5, s * r1), n, [u, 0.0]);
        }
        for j in 0..segments {
            let a = j * 2;
            // a = bottom j, a+1 = top j, a+2 = bottom j+1, a+3 = top j+1
            m.indices.extend_from_slice(&[a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        for (y, r, n) in [(-0.5f32, r0, Vec3::NEG_Y), (0.5, r1, Vec3::Y)] {
            if r <= 0.0 {
                continue;
            }
            let center = m.push(Vec3::new(0.0, y, 0.0), n, [0.5, 0.5]);
            let first = m.vertices.len() as u32;
            for j in 0..=segments {
                let phi = std::f32::consts::TAU * j as f32 / segments as f32;
                let (s, c) = phi.sin_cos();
                m.push(Vec3::new(c * r, y, s * r), n, [0.5 + c * 0.5, 0.5 + s * 0.5]);
            }
            for j in 0..segments {
                let (a, b) = (first + j, first + j + 1);
                if n.y > 0.0 {
                    m.indices.extend_from_slice(&[center, b, a]);
                } else {
                    m.indices.extend_from_slice(&[center, a, b]);
                }
            }
        }
        m
    }

    /// Box with rounded edges, absolute half extents. `seg` = segments per rounded edge.
    pub fn rounded_box(half: Vec3, radius: f32, seg: u32) -> Self {
        let r = radius.min(half.min_element()).max(0.0);
        let inner = half - Vec3::splat(r);
        let axis_coords = |h: f32, inn: f32| -> Vec<f32> {
            let mut v = Vec::new();
            for i in 0..=seg {
                let a = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / seg as f32);
                v.push(-inn - r * a.sin());
            }
            for i in 0..=seg {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / seg as f32;
                v.push(inn + r * a.sin());
            }
            let _ = h;
            v
        };
        let mut m = MeshData::default();
        for n in FACES {
            let (u, v) = face_frame(n);
            let (ua, va, na) = (u.abs(), v.abs(), n.abs());
            let us = axis_coords(half.dot(ua), inner.dot(ua));
            let vs = axis_coords(half.dot(va), inner.dot(va));
            let base = m.vertices.len() as u32;
            for (j, &t) in vs.iter().enumerate() {
                for (i, &s) in us.iter().enumerate() {
                    let p = n * half.dot(na) + u * s + v * t;
                    let c = p.clamp(-inner, inner);
                    let d = p - c;
                    let normal = if d.length_squared() > 1e-12 { d.normalize() } else { n };
                    let pos = c + normal * r;
                    m.push(pos, normal, [i as f32 / (us.len() - 1) as f32, j as f32 / (vs.len() - 1) as f32]);
                }
            }
            let row = us.len() as u32;
            for j in 0..(vs.len() as u32 - 1) {
                for i in 0..(row - 1) {
                    let a = base + j * row + i;
                    let b = a + row;
                    m.indices.extend_from_slice(&[a, a + 1, b + 1, a, b + 1, b]);
                }
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// For convex meshes centred on the origin every triangle must face outward.
    fn assert_outward(m: &MeshData, name: &str) {
        for t in m.indices.chunks(3) {
            let p = |i: u32| Vec3::from(m.vertices[i as usize].pos);
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            let n = (b - a).cross(c - a);
            if n.length_squared() < 1e-12 {
                continue; // degenerate (poles, collapsed strips)
            }
            let centroid = (a + b + c) / 3.0;
            assert!(n.dot(centroid) > 0.0, "{name}: inward triangle {t:?}");
        }
    }

    #[test]
    fn builtin_meshes_face_outward() {
        assert_outward(&MeshData::cube(), "cube");
        assert_outward(&MeshData::sphere(16, 8), "sphere");
        assert_outward(&MeshData::cylinder(16), "cylinder");
        assert_outward(&MeshData::cone(16), "cone");
        assert_outward(&MeshData::rounded_box(Vec3::new(1.0, 0.5, 0.7), 0.2, 3), "rounded_box");
    }
}
