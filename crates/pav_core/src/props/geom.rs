//! Geometry of resolved primitives: their local box, hull points, and the meshes generated
//! for the shapes the renderer has no mesh for. Frustums (tapered or cut cylinders and cones),
//! tubes, tori and wedges are unit-normalised: they fit [-0.5, 0.5]³ like the renderer's own
//! meshes, so an instance's scale is its size and one mesh serves every size. Lathes and
//! extrusions are absolute. Meshes are closed and wound counter-clockwise seen from outside.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Quat, Vec3};
use serde::Serialize;

/// Segments round a cylinder, cone, tube or torus unless the part says otherwise.
pub const SEGMENTS: u32 = 32;
/// Segments round a lathe unless the part says otherwise.
pub const LATHE_SEGMENTS: u32 = 24;
const TORUS_RINGS: u32 = 16;
/// Triangles one generated mesh may have.
pub const MAX_TRIANGLES: usize = 8_192;

/// A primitive with its sizes resolved to metres, in its own frame: local +y is its axis, and
/// an arc is centred on local +z.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Primitive {
    Box {
        half: Vec3,
    },
    RoundedBox {
        half: Vec3,
        radius: f32,
    },
    Sphere {
        radius: f32,
    },
    Ellipsoid {
        half: Vec3,
    },
    /// Cap centres at local y `a` and `b` (a ≤ b) with their radii: a capsule, or a round cone.
    Capsule {
        a: f32,
        b: f32,
        radius: f32,
        radius_end: f32,
    },
    /// A cylinder or cone: `radius` at the bottom, `radius_end` at the top.
    Cylinder {
        half_height: f32,
        radius: f32,
        radius_end: f32,
        arc: f32,
        segments: u32,
    },
    Tube {
        half_height: f32,
        radius: f32,
        thickness: f32,
        arc: f32,
    },
    Torus {
        radius: f32,
        minor_radius: f32,
        arc: f32,
    },
    /// A box whose top narrows to a ridge along x at z = peak · half.z.
    Wedge {
        half: Vec3,
        peak: f32,
    },
    /// `[r, y]` points turned round y.
    Lathe {
        profile: Vec<[f32; 2]>,
        segments: u32,
    },
    /// `[x, y]` outline (the front view), `half_depth` either side along z.
    Extrude {
        points: Vec<[f32; 2]>,
        half_depth: f32,
    },
}

impl Primitive {
    /// The box that holds the drawn shape, as centre and half size in the local frame. An arc's
    /// box is its visible sector.
    pub fn local_box(&self) -> (Vec3, Vec3) {
        let centred = |half: Vec3| (Vec3::ZERO, half);
        match self {
            Primitive::Box { half } | Primitive::RoundedBox { half, .. } | Primitive::Ellipsoid { half } => centred(*half),
            Primitive::Wedge { half, .. } => centred(*half),
            Primitive::Sphere { radius } => centred(Vec3::splat(*radius)),
            Primitive::Capsule { a, b, radius, radius_end } => {
                let (low, high) = (a - radius, b + radius_end);
                let r = radius.max(*radius_end);
                (Vec3::new(0.0, (low + high) * 0.5, 0.0), Vec3::new(r, (high - low) * 0.5, r))
            }
            Primitive::Cylinder { half_height, radius, radius_end, arc, .. } => {
                sector_box(radius.max(*radius_end), 0.0, *arc, -half_height, *half_height, 0.0)
            }
            Primitive::Tube { half_height, radius, thickness, arc } => {
                sector_box(*radius, radius - thickness, *arc, -half_height, *half_height, 0.0)
            }
            Primitive::Torus { radius, minor_radius, arc } => {
                sector_box(*radius, *radius, *arc, -minor_radius, *minor_radius, *minor_radius)
            }
            Primitive::Lathe { profile, .. } => {
                let r = profile.iter().map(|p| p[0]).fold(0.0, f32::max);
                let (low, high) =
                    profile.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
                (Vec3::new(0.0, (low + high) * 0.5, 0.0), Vec3::new(r, (high - low) * 0.5, r))
            }
            Primitive::Extrude { points, half_depth } => {
                let (min, max) =
                    points.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
                        let p = Vec3::new(p[0], p[1], 0.0);
                        (lo.min(p), hi.max(p))
                    });
                let (centre, half) = ((min + max) * 0.5, (max - min) * 0.5);
                (Vec3::new(centre.x, centre.y, 0.0), Vec3::new(half.x, half.y, *half_depth))
            }
        }
    }

    /// Points whose hull is the shape (round shapes within a few percent), in the local
    /// frame: collider hulls and camera framing.
    pub fn hull(&self) -> Vec<Vec3> {
        let corners = |centre: Vec3, half: Vec3| -> Vec<Vec3> {
            (0..8).map(|i| centre + half * Vec3::new(sign(i & 1), sign(i & 2), sign(i & 4))).collect()
        };
        match self {
            Primitive::Box { half } | Primitive::RoundedBox { half, .. } => corners(Vec3::ZERO, *half),
            Primitive::Sphere { radius } => ball(Vec3::ZERO, Vec3::splat(*radius)),
            Primitive::Ellipsoid { half } => ball(Vec3::ZERO, *half),
            Primitive::Capsule { a, b, radius, radius_end } => {
                let mut points = ball(Vec3::Y * *a, Vec3::splat(*radius));
                points.extend(ball(Vec3::Y * *b, Vec3::splat(*radius_end)));
                points
            }
            Primitive::Cylinder { half_height, radius, radius_end, arc, .. } => {
                let mut points = ring(*radius, -half_height, *arc, 16);
                points.extend(ring(*radius_end, *half_height, *arc, 16));
                if *arc < 360.0 {
                    points.extend([Vec3::Y * -half_height, Vec3::Y * *half_height]);
                }
                points
            }
            Primitive::Tube { half_height, radius, thickness, arc } => {
                let mut points = ring(*radius, -half_height, *arc, 16);
                points.extend(ring(*radius, *half_height, *arc, 16));
                if *arc < 360.0 {
                    points.extend(ring(radius - thickness, -half_height, *arc, 16));
                    points.extend(ring(radius - thickness, *half_height, *arc, 16));
                }
                points
            }
            Primitive::Torus { radius, minor_radius, arc } => {
                let mut points = Vec::new();
                for step in [-1.0, 0.0, 1.0] {
                    let y = step * minor_radius;
                    let reach = (1.0 - step * step) * minor_radius;
                    points.extend(ring(radius + reach, y, *arc, 16));
                    points.extend(ring(radius - reach, y, *arc, 16));
                }
                points
            }
            Primitive::Wedge { half, peak } => {
                let mut points: Vec<Vec3> = corners(Vec3::ZERO, *half).into_iter().filter(|p| p.y < 0.0).collect();
                points.extend([Vec3::new(-half.x, half.y, peak * half.z), Vec3::new(half.x, half.y, peak * half.z)]);
                points
            }
            Primitive::Lathe { profile, .. } => profile.iter().flat_map(|p| ring(p[0], p[1], 360.0, 16)).collect(),
            Primitive::Extrude { points, half_depth } => {
                points.iter().flat_map(|p| [Vec3::new(p[0], p[1], -half_depth), Vec3::new(p[0], p[1], *half_depth)]).collect()
            }
        }
    }
}

fn sign(bit: usize) -> f32 {
    if bit == 0 { -1.0 } else { 1.0 }
}

/// Points on an ellipsoid: the poles and rings every 22.5 degrees of latitude.
fn ball(centre: Vec3, half: Vec3) -> Vec<Vec3> {
    let mut points = vec![centre + Vec3::Y * half.y, centre - Vec3::Y * half.y];
    for band in -3..=3 {
        let latitude = band as f32 * std::f32::consts::FRAC_PI_8;
        points.extend(ring(latitude.cos(), latitude.sin(), 360.0, 16).into_iter().map(|p| centre + p * half));
    }
    points
}

/// Points round y at radius `r` and height `y`, over an arc centred on +z.
fn ring(r: f32, y: f32, arc: f32, count: u32) -> Vec<Vec3> {
    let (start, step, count) = angles(arc, count);
    (0..count).map(|i| direction(start + step * i as f32) * r + Vec3::Y * y).collect()
}

/// The first angle, the step and the number of angles to cover an arc centred on +z.
fn angles(arc: f32, segments: u32) -> (f32, f32, u32) {
    if arc >= 360.0 {
        (0.0, TAU / segments as f32, segments)
    } else {
        let sweep = arc.to_radians();
        let steps = ((segments as f32 * arc / 360.0).ceil() as u32).max(2);
        (-sweep * 0.5, sweep / steps as f32, steps + 1)
    }
}

/// The angles round an arc and the spans between them: a full circle's last span ends at its
/// first angle, so the seam closes exactly.
fn spans(arc: f32, segments: u32) -> (Vec<f32>, Vec<(f32, f32)>) {
    let (start, step, count) = angles(arc, segments);
    let phis: Vec<f32> = (0..count).map(|i| start + step * i as f32).collect();
    let spans = if arc >= 360.0 {
        (0..phis.len()).map(|i| (phis[i], phis[(i + 1) % phis.len()])).collect()
    } else {
        phis.windows(2).map(|pair| (pair[0], pair[1])).collect()
    };
    (phis, spans)
}

/// The horizontal direction at angle φ from +z toward +x.
fn direction(phi: f32) -> Vec3 {
    Vec3::new(phi.sin(), 0.0, phi.cos())
}

/// The box of an annular sector between radii `inner` and `outer` (centred on +z, `arc`
/// degrees), from `low` to `high` in y, grown by `grow` in x and z.
fn sector_box(outer: f32, inner: f32, arc: f32, low: f32, high: f32, grow: f32) -> (Vec3, Vec3) {
    let (x, z_min, z_max) = if arc >= 360.0 {
        (outer, -outer, outer)
    } else {
        let half = (arc * 0.5).to_radians();
        let x = outer * half.min(FRAC_PI_2).sin();
        let z_min = if half >= PI {
            -outer
        } else if half.cos() >= 0.0 {
            inner * half.cos()
        } else {
            outer * half.cos()
        };
        (x, z_min.min(outer), outer)
    };
    let (z_min, z_max) = (z_min - grow, z_max + grow);
    (Vec3::new(0.0, (low + high) * 0.5, (z_min + z_max) * 0.5), Vec3::new(x + grow, (high - low) * 0.5, (z_max - z_min) * 0.5))
}

// --- Generated meshes ------------------------------------------------------------------------

/// A mesh in pav_core's terms; the view turns it into renderer data. `colors` is empty for a
/// mesh drawn in its part's colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeomMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
}

impl GeomMesh {
    fn vertex(&mut self, position: Vec3, normal: Vec3) -> u32 {
        self.positions.push(position);
        self.normals.push(normal.normalize_or_zero());
        (self.positions.len() - 1) as u32
    }

    /// A triangle wound counter-clockwise seen from where its vertex normals point.
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        let [pa, pb, pc] = [a, b, c].map(|i| self.positions[i as usize]);
        let face = (pb - pa).cross(pc - pa);
        let outward = self.normals[a as usize] + self.normals[b as usize] + self.normals[c as usize];
        if face.dot(outward) < 0.0 {
            self.indices.extend_from_slice(&[a, c, b]);
        } else {
            self.indices.extend_from_slice(&[a, b, c]);
        }
    }

    /// A flat convex polygon facing `normal`.
    fn polygon(&mut self, points: &[Vec3], normal: Vec3) {
        let first = self.positions.len() as u32;
        for point in points {
            self.vertex(*point, normal);
        }
        for i in 1..points.len() as u32 - 1 {
            self.tri(first, first + i, first + i + 1);
        }
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// The enclosed volume, positive when the winding faces out.
    pub fn signed_volume(&self) -> f32 {
        let (triangles, _) = self.indices.as_chunks::<3>();
        triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// Reflected in x, wound back to face out: a mirrored part's mesh.
    pub fn mirrored(&self) -> Self {
        let flip = |v: &Vec3| Vec3::new(-v.x, v.y, v.z);
        let mut indices = self.indices.clone();
        for t in indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        Self { positions: self.positions.iter().map(flip).collect(), normals: self.normals.iter().map(flip).collect(), indices }
    }
}

/// A generated mesh for a part: its key (equal keys build equal meshes) and the scale that
/// sizes it. None: the renderer draws the shape with its own mesh or as a signed distance field.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub key: u64,
    pub scale: Vec3,
    spec: Spec,
}

/// What a generated mesh is made from, quantized so that equal specs build equal meshes:
/// ratios and peaks in thousandths, arcs in tenths of a degree, lengths in tenths of a mm.
#[derive(Clone, Debug, PartialEq)]
enum Spec {
    Frustum { bottom: u32, top: u32, arc: u32, segments: u32 },
    Tube { inner: u32, arc: u32, segments: u32 },
    Torus { minor: u32, arc: u32 },
    Wedge { peak: i32 },
    Lathe { profile: Vec<[i32; 2]>, segments: u32 },
    Extrude { points: Vec<[i32; 2]>, half_depth: i32 },
}

fn thousandths(x: f32) -> u32 {
    (x.clamp(0.0, 1.0) * 1000.0).round() as u32
}

fn tenths(degrees: f32) -> u32 {
    (degrees.clamp(0.0, 360.0) * 10.0).round() as u32
}

fn decimillimetres(metres: f32) -> i32 {
    (metres * 10_000.0).round() as i32
}

impl Generated {
    /// The generated mesh for `prim`, if the renderer has none. `mirrored`: the part is a
    /// mirror copy; only shapes that are not symmetric in x (extrusions) change.
    pub fn of(prim: &Primitive, mirrored: bool) -> Option<Self> {
        let (spec, scale) = match prim {
            Primitive::Cylinder { half_height, radius, radius_end, arc, segments } => {
                let r = radius.max(*radius_end);
                let plain = *arc >= 360.0 && *segments == SEGMENTS && (radius_end == radius || *radius_end == 0.0);
                if plain {
                    return None;
                }
                let spec = Spec::Frustum {
                    bottom: thousandths(radius / r),
                    top: thousandths(radius_end / r),
                    arc: tenths(*arc),
                    segments: *segments,
                };
                (spec, Vec3::new(2.0 * r, 2.0 * half_height, 2.0 * r))
            }
            Primitive::Tube { half_height, radius, thickness, arc } => {
                let spec =
                    Spec::Tube { inner: thousandths((radius - thickness) / radius), arc: tenths(*arc), segments: SEGMENTS };
                (spec, Vec3::new(2.0 * radius, 2.0 * half_height, 2.0 * radius))
            }
            Primitive::Torus { radius, minor_radius, arc } => (
                Spec::Torus { minor: thousandths(minor_radius / radius), arc: tenths(*arc) },
                Vec3::splat(2.0 * (radius + minor_radius)),
            ),
            Primitive::Wedge { half, peak } => {
                (Spec::Wedge { peak: (peak.clamp(-1.0, 1.0) * 1000.0).round() as i32 }, *half * 2.0)
            }
            Primitive::Lathe { profile, segments } => {
                let profile = profile.iter().map(|p| [decimillimetres(p[0]), decimillimetres(p[1])]).collect();
                (Spec::Lathe { profile, segments: *segments }, Vec3::ONE)
            }
            Primitive::Extrude { points, half_depth } => {
                let x = if mirrored { -1.0 } else { 1.0 };
                let points = points.iter().map(|p| [decimillimetres(p[0] * x), decimillimetres(p[1])]).collect();
                (Spec::Extrude { points, half_depth: decimillimetres(*half_depth) }, Vec3::ONE)
            }
            _ => return None,
        };
        Some(Self { key: spec.key(), scale, spec })
    }

    pub fn build(&self) -> GeomMesh {
        self.spec.build()
    }
}

/// FNV-1a over numbers, without a buffer: mesh keys are hashed for every part.
struct Hasher(u64);

impl Hasher {
    fn new(tag: &str) -> Self {
        let mut hasher = Hasher(0xcbf29ce484222325);
        hasher.bytes(tag.as_bytes());
        hasher
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }

    fn number(&mut self, n: i64) -> &mut Self {
        self.bytes(&n.to_le_bytes());
        self
    }
}

impl Spec {
    fn key(&self) -> u64 {
        let mut h;
        match self {
            Spec::Frustum { bottom, top, arc, segments } => {
                h = Hasher::new("frustum");
                h.number(*bottom as i64).number(*top as i64).number(*arc as i64).number(*segments as i64);
            }
            Spec::Tube { inner, arc, segments } => {
                h = Hasher::new("tube");
                h.number(*inner as i64).number(*arc as i64).number(*segments as i64);
            }
            Spec::Torus { minor, arc } => {
                h = Hasher::new("torus");
                h.number(*minor as i64).number(*arc as i64);
            }
            Spec::Wedge { peak } => {
                h = Hasher::new("wedge");
                h.number(*peak as i64);
            }
            Spec::Lathe { profile, segments } => {
                h = Hasher::new("lathe");
                h.number(*segments as i64);
                for p in profile {
                    h.number(p[0] as i64).number(p[1] as i64);
                }
            }
            Spec::Extrude { points, half_depth } => {
                h = Hasher::new("extrude");
                h.number(*half_depth as i64);
                for p in points {
                    h.number(p[0] as i64).number(p[1] as i64);
                }
            }
        }
        h.0
    }

    fn build(&self) -> GeomMesh {
        let ratio = |q: u32| q as f32 / 1000.0;
        let degrees = |q: u32| q as f32 / 10.0;
        let metres = |q: i32| q as f32 / 10_000.0;
        match self {
            Spec::Frustum { bottom, top, arc, segments } => {
                frustum(0.5 * ratio(*bottom), 0.5 * ratio(*top), degrees(*arc), *segments)
            }
            Spec::Tube { inner, arc, segments } => tube(0.5 * ratio(*inner), degrees(*arc), *segments),
            Spec::Torus { minor, arc } => torus(ratio(*minor), degrees(*arc)),
            Spec::Wedge { peak } => wedge(*peak as f32 / 1000.0),
            Spec::Lathe { profile, segments } => {
                lathe(&profile.iter().map(|p| [metres(p[0]), metres(p[1])]).collect::<Vec<_>>(), *segments)
            }
            Spec::Extrude { points, half_depth } => {
                extrude(&points.iter().map(|p| [metres(p[0]), metres(p[1])]).collect::<Vec<_>>(), metres(*half_depth))
            }
        }
    }
}

/// A cylinder or cone from y -0.5 (radius `bottom`) to 0.5 (radius `top`), over an arc centred
/// on +z, capped, with the arc's cuts closed.
fn frustum(bottom: f32, top: f32, arc: f32, segments: u32) -> GeomMesh {
    let mut m = GeomMesh::default();
    let (phis, spans) = spans(arc, segments);
    let full = arc >= 360.0;
    let slope = bottom - top;
    for (p0, p1) in spans {
        let normal = |phi: f32| direction(phi) + Vec3::Y * slope;
        let b0 = m.vertex(direction(p0) * bottom - Vec3::Y * 0.5, normal(p0));
        let b1 = m.vertex(direction(p1) * bottom - Vec3::Y * 0.5, normal(p1));
        if top > 0.0 {
            let t0 = m.vertex(direction(p0) * top + Vec3::Y * 0.5, normal(p0));
            let t1 = m.vertex(direction(p1) * top + Vec3::Y * 0.5, normal(p1));
            m.tri(b0, t0, t1);
            m.tri(b0, t1, b1);
        } else {
            let apex = m.vertex(Vec3::Y * 0.5, normal((p0 + p1) * 0.5));
            m.tri(b0, apex, b1);
        }
    }
    for (radius, y, normal) in [(bottom, -0.5, Vec3::NEG_Y), (top, 0.5, Vec3::Y)] {
        if radius > 0.0 {
            // An arc's cap fans out from its centre, which every point of the sector can see.
            let centre = (!full).then_some(Vec3::Y * y);
            let points: Vec<Vec3> =
                centre.into_iter().chain(phis.iter().map(|phi| direction(*phi) * radius + Vec3::Y * y)).collect();
            m.polygon(&points, normal);
        }
    }
    if !full {
        for (phi, side) in [(phis[0], -1.0), (phis[phis.len() - 1], 1.0)] {
            let outward = Vec3::new(phi.cos(), 0.0, -phi.sin()) * side;
            let mut points = vec![Vec3::Y * -0.5, direction(phi) * bottom - Vec3::Y * 0.5];
            if top > 0.0 {
                points.push(direction(phi) * top + Vec3::Y * 0.5);
            }
            points.push(Vec3::Y * 0.5);
            m.polygon(&points, outward);
        }
    }
    m
}

/// A pipe from y -0.5 to 0.5, outer radius 0.5 and inner radius `inner`, over an arc.
fn tube(inner: f32, arc: f32, segments: u32) -> GeomMesh {
    let mut m = GeomMesh::default();
    let (phis, spans) = spans(arc, segments);
    for (p0, p1) in spans {
        for (radius, out) in [(0.5, 1.0), (inner, -1.0)] {
            let a = m.vertex(direction(p0) * radius - Vec3::Y * 0.5, direction(p0) * out);
            let b = m.vertex(direction(p1) * radius - Vec3::Y * 0.5, direction(p1) * out);
            let c = m.vertex(direction(p1) * radius + Vec3::Y * 0.5, direction(p1) * out);
            let d = m.vertex(direction(p0) * radius + Vec3::Y * 0.5, direction(p0) * out);
            m.tri(a, b, c);
            m.tri(a, c, d);
        }
        for (y, normal) in [(-0.5, Vec3::NEG_Y), (0.5, Vec3::Y)] {
            let at = |phi: f32, r: f32| direction(phi) * r + Vec3::Y * y;
            m.polygon(&[at(p0, inner), at(p0, 0.5), at(p1, 0.5), at(p1, inner)], normal);
        }
    }
    if arc < 360.0 {
        for (phi, side) in [(phis[0], -1.0), (phis[phis.len() - 1], 1.0)] {
            let outward = Vec3::new(phi.cos(), 0.0, -phi.sin()) * side;
            let at = |r: f32, y: f32| direction(phi) * r + Vec3::Y * y;
            m.polygon(&[at(inner, -0.5), at(0.5, -0.5), at(0.5, 0.5), at(inner, 0.5)], outward);
        }
    }
    m
}

/// A torus whose outer edge reaches 0.5: major radius R, minor radius `minor`·R, over an arc,
/// with round ends closed by discs.
fn torus(minor: f32, arc: f32) -> GeomMesh {
    let mut m = GeomMesh::default();
    let major = 0.5 / (1.0 + minor);
    let tube = minor * major;
    let (phis, spans) = spans(arc, SEGMENTS);
    let (_, rings) = self::spans(360.0, TORUS_RINGS);
    let surface = |phi: f32, theta: f32| {
        let normal = direction(phi) * theta.cos() + Vec3::Y * theta.sin();
        (direction(phi) * major + normal * tube, normal)
    };
    for (p0, p1) in spans {
        for &(t0, t1) in &rings {
            let corners = [(p0, t0), (p1, t0), (p1, t1), (p0, t1)].map(|(phi, theta)| {
                let (position, normal) = surface(phi, theta);
                m.vertex(position, normal)
            });
            m.tri(corners[0], corners[1], corners[2]);
            m.tri(corners[0], corners[2], corners[3]);
        }
    }
    if arc < 360.0 {
        for (phi, side) in [(phis[0], -1.0), (phis[phis.len() - 1], 1.0)] {
            let outward = Vec3::new(phi.cos(), 0.0, -phi.sin()) * side;
            let points: Vec<Vec3> = rings.iter().map(|(theta, _)| surface(phi, *theta).0).collect();
            m.polygon(&points, outward);
        }
    }
    m
}

/// The unit box with its top narrowed to a ridge along x at z = peak/2.
fn wedge(peak: f32) -> GeomMesh {
    let mut m = GeomMesh::default();
    let ridge_z = peak * 0.5;
    let bottom = |x: f32, z: f32| Vec3::new(x, -0.5, z);
    let ridge = |x: f32| Vec3::new(x, 0.5, ridge_z);
    let centre = Vec3::new(0.0, -1.0 / 6.0, ridge_z / 3.0);
    let face = |m: &mut GeomMesh, points: &[Vec3]| {
        let normal = (points[1] - points[0]).cross(points[2] - points[0]).normalize_or_zero();
        let middle = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let normal = if normal.dot(middle - centre) < 0.0 { -normal } else { normal };
        m.polygon(points, normal);
    };
    face(&mut m, &[bottom(-0.5, -0.5), bottom(0.5, -0.5), bottom(0.5, 0.5), bottom(-0.5, 0.5)]);
    face(&mut m, &[bottom(-0.5, 0.5), bottom(0.5, 0.5), ridge(0.5), ridge(-0.5)]);
    face(&mut m, &[bottom(0.5, -0.5), bottom(-0.5, -0.5), ridge(-0.5), ridge(0.5)]);
    face(&mut m, &[bottom(-0.5, -0.5), bottom(-0.5, 0.5), ridge(-0.5)]);
    face(&mut m, &[bottom(0.5, 0.5), bottom(0.5, -0.5), ridge(0.5)]);
    m
}

/// A profile of `[r, y]` points turned round y; ends off the axis are closed by discs.
fn lathe(profile: &[[f32; 2]], segments: u32) -> GeomMesh {
    let mut m = GeomMesh::default();
    // The profile and the axis enclose the solid; its orientation says which side is out.
    let mut closed: Vec<[f32; 2]> = profile.to_vec();
    closed.push([0.0, profile[profile.len() - 1][1]]);
    closed.push([0.0, profile[0][1]]);
    let area: f32 = (0..closed.len())
        .map(|i| {
            let (a, b) = (closed[i], closed[(i + 1) % closed.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let out = if area >= 0.0 { 1.0 } else { -1.0 };
    let outward = |a: [f32; 2], b: [f32; 2]| [(b[1] - a[1]) * out, -(b[0] - a[0]) * out];
    let at = |p: [f32; 2], phi: f32| direction(phi) * p[0] + Vec3::Y * p[1];
    let normal = |n: [f32; 2], phi: f32| direction(phi) * n[0] + Vec3::Y * n[1];
    let (phis, spans) = spans(360.0, segments);
    for pair in profile.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let n = outward(a, b);
        for &(p0, p1) in &spans {
            let mid = p0 + (p1 - p0).rem_euclid(TAU) * 0.5;
            match (a[0] > 0.0, b[0] > 0.0) {
                (true, true) => {
                    let corners = [(a, p0), (a, p1), (b, p1), (b, p0)].map(|(p, phi)| m.vertex(at(p, phi), normal(n, phi)));
                    m.tri(corners[0], corners[1], corners[2]);
                    m.tri(corners[0], corners[2], corners[3]);
                }
                (true, false) => {
                    let corners = [(a, p0), (a, p1)].map(|(p, phi)| m.vertex(at(p, phi), normal(n, phi)));
                    let tip = m.vertex(at(b, 0.0), normal(n, mid));
                    m.tri(corners[0], corners[1], tip);
                }
                (false, true) => {
                    let corners = [(b, p0), (b, p1)].map(|(p, phi)| m.vertex(at(p, phi), normal(n, phi)));
                    let tip = m.vertex(at(a, 0.0), normal(n, mid));
                    m.tri(corners[0], corners[1], tip);
                }
                (false, false) => {}
            }
        }
    }
    let last = profile.len() - 1;
    for (end, toward, first) in [(profile[0], profile[1], true), (profile[last], profile[last - 1], false)] {
        if end[0] > 0.0 {
            // The disc faces away from the solid: opposite to where the profile goes, or for a
            // flat end, along the outward normal of the segment that closes the profile there
            // (the axis to the first point, the last point to the axis).
            let rise = end[1] - toward[1];
            let axis = [0.0, end[1]];
            let closing = if first { outward(axis, end) } else { outward(end, axis) };
            let up = if rise.abs() > 1e-6 { rise.signum() } else { closing[1].signum() };
            let points: Vec<Vec3> = phis.iter().map(|phi| at(end, *phi)).collect();
            m.polygon(&points, Vec3::Y * up);
        }
    }
    if m.signed_volume() < 0.0 {
        for t in m.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        for n in &mut m.normals {
            *n = -*n;
        }
    }
    m
}

/// An `[x, y]` outline extruded `half_depth` either side along z.
fn extrude(points: &[[f32; 2]], half_depth: f32) -> GeomMesh {
    let mut m = GeomMesh::default();
    let mut outline: Vec<[f32; 2]> = points.to_vec();
    let area: f32 = (0..outline.len())
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if area < 0.0 {
        outline.reverse();
    }
    let at = |p: [f32; 2], z: f32| Vec3::new(p[0], p[1], z);
    for (z, normal) in [(half_depth, Vec3::Z), (-half_depth, Vec3::NEG_Z)] {
        let first = m.positions.len() as u32;
        for p in &outline {
            m.vertex(at(*p, z), normal);
        }
        for [a, b, c] in ear_clip(&outline) {
            m.tri(first + a, first + b, first + c);
        }
    }
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        let normal = Vec3::new(b[1] - a[1], -(b[0] - a[0]), 0.0);
        m.polygon(&[at(a, -half_depth), at(b, -half_depth), at(b, half_depth), at(a, half_depth)], normal);
    }
    m
}

/// Triangles of a simple counter-clockwise polygon, by ear clipping.
fn ear_clip(points: &[[f32; 2]]) -> Vec<[u32; 3]> {
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let inside = |p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
    };
    let mut left: Vec<usize> = (0..points.len()).collect();
    let mut out = Vec::new();
    while left.len() > 3 {
        let n = left.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (points[left[(i + n - 1) % n]], points[left[i]], points[left[(i + 1) % n]]);
            cross(a, b, c) > 0.0
                && !left.iter().any(|&k| {
                    let p = points[k];
                    p != a && p != b && p != c && inside(p, a, b, c)
                })
        });
        // A degenerate outline has no ear; cut the sharpest corner so the loop ends.
        let i = ear.unwrap_or(0);
        out.push([left[(i + n - 1) % n] as u32, left[i] as u32, left[(i + 1) % n] as u32]);
        left.remove(i);
    }
    out.push([left[0] as u32, left[1] as u32, left[2] as u32]);
    out
}

/// The rotation that turns local +y toward `axis` the way format 2's axis field does.
pub fn base_rotation(axis: Vec3, extrude: bool) -> Quat {
    let half_turn = |v: Vec3| Quat::from_axis_angle(v, PI);
    let quarter = |v: Vec3, sign: f32| Quat::from_axis_angle(v, sign * FRAC_PI_2);
    if extrude {
        // Local +z is the extrusion's axis.
        match axis.to_array().map(|x| x.round() as i32) {
            [1, 0, 0] => quarter(Vec3::Y, 1.0),
            [-1, 0, 0] => quarter(Vec3::Y, -1.0),
            [0, 1, 0] => quarter(Vec3::X, -1.0),
            [0, -1, 0] => quarter(Vec3::X, 1.0),
            [0, 0, -1] => half_turn(Vec3::Y),
            _ => Quat::IDENTITY,
        }
    } else {
        match axis.to_array().map(|x| x.round() as i32) {
            [1, 0, 0] => quarter(Vec3::Z, -1.0),
            [-1, 0, 0] => quarter(Vec3::Z, 1.0),
            [0, 0, 1] => quarter(Vec3::X, 1.0),
            [0, 0, -1] => quarter(Vec3::X, -1.0),
            [0, -1, 0] => half_turn(Vec3::X),
            _ => Quat::IDENTITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Edges by welded position: a closed, consistently wound mesh uses each directed edge
    /// once and its reverse once.
    fn closed(mesh: &GeomMesh) -> Result<(), String> {
        let weld = |p: Vec3| (p * 100_000.0).round().as_ivec3().to_array();
        let mut edges: BTreeMap<([i32; 3], [i32; 3]), i32> = BTreeMap::new();
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = t.map(|i| weld(mesh.positions[i as usize]));
            for k in 0..3 {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                if a == b {
                    return Err("a degenerate triangle".into());
                }
                *edges.entry((a, b)).or_default() += 1;
            }
        }
        for (&(a, b), count) in &edges {
            if *count != 1 || edges.get(&(b, a)) != Some(&1) {
                return Err(format!("edge {a:?}->{b:?} used {count} times, reverse {:?}", edges.get(&(b, a))));
            }
        }
        Ok(())
    }

    fn check(name: &str, mesh: &GeomMesh) {
        closed(mesh).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(mesh.signed_volume() > 0.0, "{name} faces out");
        assert!(mesh.triangles() <= MAX_TRIANGLES, "{name}");
        for (i, n) in mesh.normals.iter().enumerate() {
            assert!((n.length() - 1.0).abs() < 1e-4, "{name} normal {i}");
        }
    }

    #[test]
    fn generated_meshes_are_closed_and_face_out() {
        let parts = [
            ("frustum", Primitive::Cylinder { half_height: 0.5, radius: 0.3, radius_end: 0.1, arc: 360.0, segments: 32 }),
            ("cone arc", Primitive::Cylinder { half_height: 0.5, radius: 0.3, radius_end: 0.0, arc: 120.0, segments: 32 }),
            ("half disc", Primitive::Cylinder { half_height: 0.01, radius: 0.2, radius_end: 0.2, arc: 180.0, segments: 32 }),
            ("hexagonal", Primitive::Cylinder { half_height: 0.2, radius: 0.2, radius_end: 0.2, arc: 360.0, segments: 6 }),
            ("wide arc", Primitive::Cylinder { half_height: 0.2, radius: 0.2, radius_end: 0.3, arc: 270.0, segments: 32 }),
            ("tube", Primitive::Tube { half_height: 0.4, radius: 0.7, thickness: 0.1, arc: 360.0 }),
            ("tube arc", Primitive::Tube { half_height: 0.4, radius: 0.7, thickness: 0.1, arc: 90.0 }),
            ("torus", Primitive::Torus { radius: 0.13, minor_radius: 0.008, arc: 360.0 }),
            ("bail", Primitive::Torus { radius: 0.13, minor_radius: 0.008, arc: 180.0 }),
            ("gable", Primitive::Wedge { half: Vec3::new(1.0, 0.5, 0.6), peak: 0.0 }),
            ("ramp", Primitive::Wedge { half: Vec3::new(1.0, 0.5, 0.6), peak: 1.0 }),
            (
                "vase",
                Primitive::Lathe { profile: vec![[0.0, 0.0], [0.1, 0.02], [0.12, 0.08], [0.06, 0.2], [0.08, 0.3]], segments: 24 },
            ),
            (
                "down",
                Primitive::Lathe { profile: vec![[0.08, 0.3], [0.06, 0.2], [0.12, 0.08], [0.1, 0.02], [0.0, 0.0]], segments: 24 },
            ),
            ("ring", Primitive::Lathe { profile: vec![[0.2, 0.0], [0.3, 0.0], [0.3, 0.1], [0.2, 0.1]], segments: 16 }),
            (
                "blade",
                Primitive::Extrude {
                    points: vec![[-0.03, 0.0], [0.03, 0.0], [0.024, 1.0], [0.0, 1.13], [-0.024, 1.0]],
                    half_depth: 0.004,
                },
            ),
            (
                "notch",
                Primitive::Extrude { points: vec![[0.0, 0.0], [0.4, 0.0], [0.4, 0.3], [0.2, 0.1], [0.0, 0.3]], half_depth: 0.05 },
            ),
        ];
        for (name, prim) in &parts {
            let generated = Generated::of(prim, false).unwrap_or_else(|| panic!("{name} is generated"));
            let mesh = generated.build();
            check(name, &mesh);
            // The scaled mesh stays inside the part's box.
            let (centre, half) = prim.local_box();
            for p in &mesh.positions {
                let p = *p * generated.scale;
                assert!(((p - centre).abs() - half).max_element() < 1e-4, "{name}: {p} outside {centre} ± {half}");
            }
        }
        // Shapes the renderer draws itself are not generated.
        for prim in [
            Primitive::Cylinder { half_height: 0.5, radius: 0.3, radius_end: 0.3, arc: 360.0, segments: SEGMENTS },
            Primitive::Cylinder { half_height: 0.5, radius: 0.3, radius_end: 0.0, arc: 360.0, segments: SEGMENTS },
            Primitive::Box { half: Vec3::ONE },
            Primitive::Ellipsoid { half: Vec3::ONE },
        ] {
            assert!(Generated::of(&prim, false).is_none(), "{prim:?}");
        }
    }

    #[test]
    fn unit_meshes_share_keys_and_absolute_ones_do_not() {
        let key = |prim: Primitive| Generated::of(&prim, false).unwrap().key;
        let small = key(Primitive::Cylinder { half_height: 0.1, radius: 0.3, radius_end: 0.15, arc: 360.0, segments: 32 });
        let large = key(Primitive::Cylinder { half_height: 2.0, radius: 6.0, radius_end: 3.0, arc: 360.0, segments: 32 });
        assert_eq!(small, large, "one frustum mesh serves every size with the same taper");
        assert_ne!(small, key(Primitive::Cylinder { half_height: 0.1, radius: 0.3, radius_end: 0.2, arc: 360.0, segments: 32 }));
        let tube = |r: f32| key(Primitive::Tube { half_height: r, radius: r, thickness: r / 10.0, arc: 180.0 });
        assert_eq!(tube(0.1), tube(5.0));
        let ring = |r: f32| key(Primitive::Torus { radius: r, minor_radius: r / 8.0, arc: 360.0 });
        assert_eq!(ring(0.1), ring(3.0));
        let wedge = |x: f32| key(Primitive::Wedge { half: Vec3::splat(x), peak: 0.5 });
        assert_eq!(wedge(0.1), wedge(3.0));
        let lathe = |r: f32| key(Primitive::Lathe { profile: vec![[0.0, 0.0], [r, 0.1], [r, 0.2]], segments: 24 });
        assert_ne!(lathe(0.1), lathe(0.2), "lathes are absolute");
        // Keys differ between kinds that share numbers.
        assert_ne!(tube(0.1), ring(0.1));
    }

    #[test]
    fn a_mirrored_extrusion_is_reflected_and_still_faces_out() {
        let flag = Primitive::Extrude { points: vec![[0.0, 0.0], [0.2, 0.06], [0.0, 0.12]], half_depth: 0.005 };
        let plain = Generated::of(&flag, false).unwrap();
        let mirrored = Generated::of(&flag, true).unwrap();
        assert_ne!(plain.key, mirrored.key);
        let (a, b) = (plain.build(), mirrored.build());
        check("mirrored flag", &b);
        assert!(b.positions.iter().all(|p| p.x <= 1e-6), "the flag points the other way");
        // Reflecting a mesh reverses its winding; GeomMesh::mirrored winds it back.
        let reflected = a.mirrored();
        check("reflected flag", &reflected);
        assert!((reflected.signed_volume() - a.signed_volume()).abs() < 1e-9);
        // Symmetric shapes ignore the mirror.
        let arc = Primitive::Tube { half_height: 0.1, radius: 0.2, thickness: 0.02, arc: 120.0 };
        assert_eq!(Generated::of(&arc, true), Generated::of(&arc, false));
    }

    #[test]
    fn boxes_and_hulls_cover_the_visible_shape() {
        let half_disc = Primitive::Cylinder { half_height: 0.008, radius: 0.216, radius_end: 0.216, arc: 180.0, segments: 32 };
        let (centre, half) = half_disc.local_box();
        assert!((centre - Vec3::new(0.0, 0.0, 0.108)).length() < 1e-6 && (half - Vec3::new(0.216, 0.008, 0.108)).length() < 1e-6);
        let quarter_tube = Primitive::Tube { half_height: 0.1, radius: 1.0, thickness: 0.5, arc: 90.0 };
        let (centre, half) = quarter_tube.local_box();
        let z_min = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((centre.z - (1.0 + z_min) / 2.0).abs() < 1e-5 && (half.x - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        for prim in [
            half_disc,
            quarter_tube,
            Primitive::Torus { radius: 0.13, minor_radius: 0.008, arc: 180.0 },
            Primitive::Capsule { a: -0.2, b: 0.3, radius: 0.05, radius_end: 0.02 },
            Primitive::Wedge { half: Vec3::new(1.0, 0.5, 0.6), peak: -0.5 },
        ] {
            let (centre, half) = prim.local_box();
            for p in prim.hull() {
                assert!(((p - centre).abs() - half).max_element() < 1e-5, "{prim:?}: {p}");
            }
        }
    }

    #[test]
    fn axis_rotations_follow_the_plan() {
        for (axis, expected) in [
            (Vec3::X, Vec3::X),
            (Vec3::NEG_X, Vec3::NEG_X),
            (Vec3::Z, Vec3::Z),
            (Vec3::NEG_Z, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::NEG_Y),
        ] {
            assert!((base_rotation(axis, false) * Vec3::Y - expected).length() < 1e-6, "{axis}");
            assert!((base_rotation(axis, true) * Vec3::Z - expected).length() < 1e-6, "{axis}");
        }
        assert!((base_rotation(Vec3::Y, true) * Vec3::Z - Vec3::Y).length() < 1e-6);
    }
}
