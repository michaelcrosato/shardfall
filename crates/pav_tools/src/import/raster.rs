//! A small CPU rasteriser for the importer: orthographic views of coloured triangles with a
//! depth buffer, for the fitter's silhouettes, the fidelity metrics and the compare image. Plus
//! the triangles both sides are drawn from: a source `Scene` (face colours) and a compiled prop
//! (the shapes the engine draws, from pav_core's own geometry).
//!
//! f64 throughout and no threads: the same triangles give the same pixels.

use std::f64::consts::TAU;

use glam::{DMat3, DQuat, DVec3, Quat, Vec3};
use pav_core::props::CompiledProp;
use pav_core::props::geom::{Generated, Primitive};

use super::normalize;
use super::scene::Scene;

/// A triangle to draw: its corners (metres) and its colour (linear RGB).
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub p: [DVec3; 3],
    pub c: [f64; 3],
}

/// An orthographic view: screen x along `right`, y along `up`; `toward` points at the viewer,
/// so a larger `p · toward` is nearer.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub right: DVec3,
    pub up: DVec3,
    pub toward: DVec3,
}

impl View {
    /// Looking back along `toward` (from where it points) with `up` as near up as it can be.
    pub fn new(toward: DVec3, up: DVec3) -> View {
        let t = toward.normalize();
        let r = up.cross(t).normalize();
        View { right: r, up: t.cross(r), toward: t }
    }

    /// From yaw degrees round +y (0: from the front, +z) and pitch degrees above.
    pub fn orbit(yaw: f64, pitch: f64) -> View {
        let (y, p) = (yaw.to_radians(), pitch.to_radians());
        View::new(DVec3::new(y.sin() * p.cos(), p.sin(), y.cos() * p.cos()), DVec3::Y)
    }

    pub fn screen(&self, p: DVec3) -> [f64; 2] {
        [p.dot(self.right), p.dot(self.up)]
    }
}

/// The six views of the fidelity metrics (plan §8.6): front, back, left, right, top, and iso
/// (yaw 45°, pitch 30°).
pub fn six_views() -> [(&'static str, View); 6] {
    [
        ("front", View::new(DVec3::Z, DVec3::Y)),
        ("back", View::new(DVec3::NEG_Z, DVec3::Y)),
        ("left", View::new(DVec3::NEG_X, DVec3::Y)),
        ("right", View::new(DVec3::X, DVec3::Y)),
        ("top", View::new(DVec3::Y, DVec3::NEG_Z)),
        ("iso", View::orbit(45.0, 30.0)),
    ]
}

/// The views down the three axes (x, y, z), for silhouettes in a part's own frame.
pub fn axis_views() -> [View; 3] {
    [View::new(DVec3::X, DVec3::Y), View::new(DVec3::Y, DVec3::NEG_Z), View::new(DVec3::Z, DVec3::Y)]
}

/// The screen rectangle an image covers: `lo` to `hi`, `w` × `h` pixels (they need not be
/// square: an intersection over union is the same at any aspect).
#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub lo: [f64; 2],
    pub hi: [f64; 2],
    pub w: usize,
    pub h: usize,
}

impl Window {
    /// Around every corner of `tris` seen in `view`, padded by `pad` of the larger side.
    pub fn around<'a>(tris: impl IntoIterator<Item = &'a Tri>, view: &View, pad: f64, w: usize, h: usize) -> Option<Window> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for t in tris {
            for p in t.p {
                let s = view.screen(p);
                for k in 0..2 {
                    lo[k] = lo[k].min(s[k]);
                    hi[k] = hi[k].max(s[k]);
                }
            }
        }
        if !(lo[0].is_finite() && lo[1].is_finite()) {
            return None;
        }
        let side = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
        let grow = side * pad;
        // A side with no extent still gets a sliver, so a flat shape covers a row.
        let at_least = |l: f64, h: f64| {
            if h - l < side * 1e-3 { ((l + h) * 0.5 - side * 5e-4, (l + h) * 0.5 + side * 5e-4) } else { (l, h) }
        };
        let (x0, x1) = at_least(lo[0], hi[0]);
        let (y0, y1) = at_least(lo[1], hi[1]);
        Some(Window { lo: [x0 - grow, y0 - grow], hi: [x1 + grow, y1 + grow], w, h })
    }
}

/// What one view saw: per pixel (row 0 at the top) whether anything covers it, the nearest
/// surface's colour, and how much it faces the light (for pictures).
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub cover: Vec<bool>,
    pub depth: Vec<f64>,
    pub colour: Vec<[f64; 3]>,
    pub shade: Vec<f64>,
}

impl Image {
    pub fn covered(&self) -> usize {
        self.cover.iter().filter(|c| **c).count()
    }
}

/// Draws `tris` in `view` over `win`: a pixel is covered when its centre is inside a triangle
/// (edges included); the nearest triangle there gives its colour.
pub fn draw(tris: &[Tri], view: &View, win: &Window) -> Image {
    let (w, h) = (win.w, win.h);
    let mut img = Image {
        w,
        h,
        cover: vec![false; w * h],
        depth: vec![f64::NEG_INFINITY; w * h],
        colour: vec![[0.0; 3]; w * h],
        shade: vec![0.0; w * h],
    };
    let sx = w as f64 / (win.hi[0] - win.lo[0]);
    let sy = h as f64 / (win.hi[1] - win.lo[1]);
    // A light over the viewer's right shoulder.
    let light = (view.toward * 1.0 + view.up * 0.6 + view.right * 0.35).normalize();
    for t in tris {
        // Pixel coordinates: x right, y down.
        let q = t.p.map(|p| {
            let s = view.screen(p);
            [(s[0] - win.lo[0]) * sx, (win.hi[1] - s[1]) * sy, p.dot(view.toward)]
        });
        let area = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
        if area.abs() < 1e-12 {
            continue;
        }
        let x0 = q.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let x1 = (q.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as usize).min(w);
        let y0 = q.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let y1 = (q.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as usize).min(h);
        let n = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).normalize_or_zero();
        let n = if n.dot(view.toward) < 0.0 { -n } else { n };
        let shade = n.dot(light).max(0.0);
        let edge = |a: [f64; 3], b: [f64; 3], x: f64, y: f64| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        for y in y0..y1 {
            let py = y as f64 + 0.5;
            for x in x0..x1 {
                let px = x as f64 + 0.5;
                let (e0, e1, e2) = (edge(q[1], q[2], px, py), edge(q[2], q[0], px, py), edge(q[0], q[1], px, py));
                let inside = if area > 0.0 { e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0 } else { e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0 };
                if !inside {
                    continue;
                }
                let z = (e0 * q[0][2] + e1 * q[1][2] + e2 * q[2][2]) / area;
                let i = y * w + x;
                img.cover[i] = true;
                if z > img.depth[i] {
                    img.depth[i] = z;
                    img.colour[i] = t.c;
                    img.shade[i] = shade;
                }
            }
        }
    }
    img
}

impl Image {
    /// Covers every pixel the border cannot reach through uncovered ones: the silhouette of
    /// a shell open along the view (a tube without caps seen down its axis) as its solid.
    pub fn fill_holes(&mut self) {
        let (w, h) = (self.w, self.h);
        let mut outside = vec![false; w * h];
        let mut stack: Vec<usize> = Vec::new();
        for x in 0..w {
            stack.extend([x, (h - 1) * w + x]);
        }
        for y in 0..h {
            stack.extend([y * w, y * w + w - 1]);
        }
        while let Some(i) = stack.pop() {
            if outside[i] || self.cover[i] {
                continue;
            }
            outside[i] = true;
            let (x, y) = (i % w, i / w);
            if x > 0 {
                stack.push(i - 1);
            }
            if x + 1 < w {
                stack.push(i + 1);
            }
            if y > 0 {
                stack.push(i - w);
            }
            if y + 1 < h {
                stack.push(i + w);
            }
        }
        for (c, o) in self.cover.iter_mut().zip(outside) {
            *c = !o;
        }
    }
}

/// Covered in both / covered in either; 1 when neither covers anything.
pub fn iou(a: &Image, b: &Image) -> f64 {
    let (mut both, mut either) = (0usize, 0usize);
    for (x, y) in a.cover.iter().zip(&b.cover) {
        both += (*x && *y) as usize;
        either += (*x || *y) as usize;
    }
    if either == 0 { 1.0 } else { both as f64 / either as f64 }
}

/// Linear RGB as Oklab.
pub fn oklab(c: [f64; 3]) -> [f64; 3] {
    super::scene::oklab(c.map(|x| x.max(0.0)))
}

/// The distance between two colours in Oklab (ΔE_OK).
pub fn delta_e(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (a, b) = (oklab(a), oklab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

// --- Triangles ---------------------------------------------------------------------------------

/// Every triangle of a scene in its face colour (7-sample texture mean × factors).
pub fn scene_tris(s: &Scene) -> Vec<Tri> {
    let mut out = Vec::with_capacity(s.tris());
    for (mi, m) in s.meshes.iter().enumerate() {
        for (ti, t) in m.tris.iter().enumerate() {
            let c = normalize::face_colour(s, mi, ti);
            out.push(Tri { p: t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from))), c: [c[0], c[1], c[2]] });
        }
    }
    out
}

fn v3(v: Vec3) -> DVec3 {
    v.as_dvec3()
}

/// `#rrggbb` (or a theme role, through the default palette) as linear RGB.
pub fn hex_linear(hex: &str) -> [f64; 3] {
    let hex = match hex.strip_prefix('$') {
        Some(_) => pav_core::props::DEFAULT_ROLES.iter().find(|(role, _)| *role == hex).map_or("#808080", |(_, h)| *h),
        None => hex,
    };
    pav_core::color::Color::hex(hex).0.map(f64::from)
}

/// The triangles a compiled prop draws (hidden parts left out), in metres in its frame: each
/// part's shape from pav_core's geometry (generated meshes as built; the renderer's own box,
/// sphere, cylinder and capsule tessellated here), in its colour.
pub fn prop_tris(compiled: &CompiledProp) -> Vec<Tri> {
    let mut out = Vec::new();
    for part in compiled.parts.iter().filter(|p| !p.hidden) {
        let colour = hex_linear(part.surface_color()).map(|c| c * part.tint as f64);
        let (pos, rot, scale) = (v3(part.pos), part.rot.as_dquat(), part.scale as f64);
        let place = |p: DVec3| pos + rot * (p * scale);
        if let Some(g) = Generated::of(&part.prim, part.mirrored) {
            // Built already reflected for a mirror copy, as the view draws it.
            let mesh = g.build();
            let size = v3(g.scale);
            for t in mesh.indices.chunks(3) {
                let corners = [t[0], t[1], t[2]].map(|i| place(v3(mesh.positions[i as usize]) * size));
                let c = match mesh.colors.get(t[0] as usize) {
                    Some(vc) => [vc.x, vc.y, vc.z].map(|x| x as f64 * part.tint as f64),
                    None => colour,
                };
                out.push(Tri { p: corners, c });
            }
            continue;
        }
        // The renderer's own shapes are symmetric in x: a mirror copy draws the same.
        for corners in shape_tris(&part.prim) {
            out.push(Tri { p: corners.map(place), c: colour });
        }
    }
    out
}

/// Triangles for the shapes the renderer draws with its own meshes or distance fields.
pub fn shape_tris(prim: &Primitive) -> Vec<[DVec3; 3]> {
    match prim {
        Primitive::Box { half } => box_tris(v3(*half)),
        Primitive::RoundedBox { half, radius } => rounded_box_tris(v3(*half), *radius as f64),
        Primitive::Sphere { radius } => ellipsoid_tris(DVec3::ZERO, DVec3::splat(*radius as f64)),
        Primitive::Ellipsoid { half } => ellipsoid_tris(DVec3::ZERO, v3(*half)),
        Primitive::Capsule { a, b, radius, radius_end } => {
            let (a, b, r0, r1) = (*a as f64, *b as f64, *radius as f64, *radius_end as f64);
            let mut out = ellipsoid_tris(DVec3::Y * a, DVec3::splat(r0));
            out.extend(ellipsoid_tris(DVec3::Y * b, DVec3::splat(r1)));
            out.extend(frustum_tris(a, b, r0, r1, 32));
            out
        }
        Primitive::Cylinder { half_height, radius, radius_end, segments, .. } => {
            let h = *half_height as f64;
            frustum_tris(-h, h, *radius as f64, *radius_end as f64, *segments)
        }
        // Every other shape is generated (`prop_tris` builds those).
        _ => Vec::new(),
    }
}

fn box_tris(h: DVec3) -> Vec<[DVec3; 3]> {
    let c = |i: usize| {
        DVec3::new(if i & 1 == 0 { -h.x } else { h.x }, if i & 2 == 0 { -h.y } else { h.y }, if i & 4 == 0 { -h.z } else { h.z })
    };
    let quads = [[0, 2, 6, 4], [1, 5, 7, 3], [0, 4, 5, 1], [2, 3, 7, 6], [0, 1, 3, 2], [4, 6, 7, 5]];
    quads.iter().flat_map(|q| [[c(q[0]), c(q[1]), c(q[2])], [c(q[0]), c(q[2]), c(q[3])]]).collect()
}

/// A latitude-longitude ball scaled to `half`.
fn ellipsoid_tris(centre: DVec3, half: DVec3) -> Vec<[DVec3; 3]> {
    let (rings, segs) = (12, 24);
    let at = |i: usize, j: usize| {
        let lat = std::f64::consts::PI * (i as f64 / rings as f64 - 0.5);
        let lon = TAU * j as f64 / segs as f64;
        centre + DVec3::new(lat.cos() * lon.sin(), lat.sin(), lat.cos() * lon.cos()) * half
    };
    let mut out = Vec::new();
    for i in 0..rings {
        for j in 0..segs {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            out.push([a, b, c]);
            out.push([a, c, d]);
        }
    }
    out
}

/// A rounded box: a ball's octants pushed out to the corners of the inner box, so the bands
/// between them are the flat faces and the round edges.
fn rounded_box_tris(half: DVec3, radius: f64) -> Vec<[DVec3; 3]> {
    let r = radius.min(half.min_element()).max(0.0);
    let inner = half - DVec3::splat(r);
    // Rings and segments that never sit on an axis plane, so every point has a side.
    let (rings, segs) = (12, 24);
    let at = |i: usize, j: usize| {
        let lat = std::f64::consts::PI * ((i as f64 + 0.5) / rings as f64 - 0.5);
        let lon = TAU * (j as f64 + 0.5) / segs as f64;
        let d = DVec3::new(lat.cos() * lon.sin(), lat.sin(), lat.cos() * lon.cos());
        d * r + inner * d.signum()
    };
    let mut out = Vec::new();
    for i in 0..rings - 1 {
        for j in 0..segs {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            out.push([a, b, c]);
            out.push([a, c, d]);
        }
    }
    // Close the poles with the top and bottom faces.
    for (i, y) in [(0, -1.0), (rings - 1, 1.0)] {
        let pole = DVec3::new(0.0, y * half.y, 0.0);
        for j in 0..segs {
            out.push([pole, at(i, j), at(i, j + 1)]);
        }
    }
    out
}

/// A cylinder or cone from y `a` (radius `r0`) to `b` (radius `r1`), capped.
fn frustum_tris(a: f64, b: f64, r0: f64, r1: f64, segments: u32) -> Vec<[DVec3; 3]> {
    let n = segments.max(3) as usize;
    let ring = |y: f64, r: f64, j: usize| {
        let phi = TAU * j as f64 / n as f64;
        DVec3::new(phi.sin() * r, y, phi.cos() * r)
    };
    let mut out = Vec::new();
    for j in 0..n {
        let (p0, p1, q0, q1) = (ring(a, r0, j), ring(a, r0, j + 1), ring(b, r1, j), ring(b, r1, j + 1));
        out.push([p0, p1, q1]);
        out.push([p0, q1, q0]);
        out.push([DVec3::Y * a, p1, p0]);
        out.push([DVec3::Y * b, q0, q1]);
    }
    out
}

/// Moves triangles by a rotation and translation (into a part's frame or out of it).
pub fn transform(tris: &[Tri], rot: DMat3, offset: DVec3) -> Vec<Tri> {
    tris.iter().map(|t| Tri { p: t.p.map(|p| rot * p + offset), c: t.c }).collect()
}

/// A rotation as pav_core's f32 quaternion.
pub fn quat(m: DMat3) -> Quat {
    DQuat::from_mat3(&m).normalize().as_quat()
}
