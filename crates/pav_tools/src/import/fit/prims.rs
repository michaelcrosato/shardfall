//! Primitive candidates for one piece, in its own frame (plan §8.5, step 3): a box, an
//! ellipsoid or sphere, and along each local axis a cylinder, a cone or frustum, a capsule, a
//! lathe (the side silhouettes' radius per height, simplified to at most 12 points) and an
//! extrusion (the silhouette's outline down that axis, at most 24 points). Each is scored by the
//! worst of its three local-axis silhouette IoUs at 64 px, less 0.0002 per token it costs.

use glam::{DMat3, DVec3, Vec3};
use pav_core::props::geom::{Generated, Primitive};

use super::super::raster::{self, Tri, View, Window};
use super::frame::{Frame, MIN_HALF};

/// Pixels a side for the fitting silhouettes.
pub const FIT_PX: usize = 64;
/// What one token costs in score.
pub const TOKEN_COST: f64 = 0.0002;
/// Lathe profiles and extrusion outlines: most points.
pub const LATHE_POINTS: usize = 12;
pub const OUTLINE_POINTS: usize = 24;

/// A fitted primitive: the shape in its own frame, the frame's axes and origin in the piece's
/// local frame, and how well it fits.
#[derive(Clone, Debug)]
pub struct Cand {
    pub prim: Primitive,
    /// The primitive's axes in the piece's frame (columns).
    pub axes: DMat3,
    /// Its origin in the piece's frame.
    pub at: DVec3,
    pub iou: f64,
    pub tokens: usize,
}

impl Cand {
    pub fn score(&self) -> f64 {
        self.iou - TOKEN_COST * self.tokens as f64
    }
}

fn f(v: DVec3) -> Vec3 {
    v.as_vec3()
}

/// A primitive's triangles in its own frame (unscaled).
pub fn prim_tris(prim: &Primitive) -> Vec<[DVec3; 3]> {
    if let Some(g) = Generated::of(prim, false) {
        let mesh = g.build();
        let s = g.scale.as_dvec3();
        return mesh.indices.chunks(3).map(|t| [t[0], t[1], t[2]].map(|i| mesh.positions[i as usize].as_dvec3() * s)).collect();
    }
    raster::shape_tris(prim)
}

/// The view down local axis `k`: screen x along axis (k+1)%3, y along (k+2)%3.
pub fn axis_view(k: usize) -> View {
    let e = |i: usize| DVec3::AXES[i % 3];
    View::new(e(k), e(k + 2))
}

/// The view from the side of axis `k`, looking down axis `side`: screen y along axis k.
fn side_view(k: usize, side: usize) -> View {
    View::new(DVec3::AXES[side], DVec3::AXES[k])
}

/// The worst of the three axis silhouettes' IoUs of `cand` against `local` (both in the piece's
/// frame).
pub fn silhouette_iou(local: &[Tri], cand: &[Tri]) -> f64 {
    let mut worst: f64 = 1.0;
    for k in 0..3 {
        let view = axis_view(k);
        let Some(win) = Window::around(local.iter().chain(cand), &view, 0.02, FIT_PX, FIT_PX) else { return 0.0 };
        let (mut a, mut b) = (raster::draw(local, &view, &win), raster::draw(cand, &view, &win));
        // A flat piece seen edge-on covers nothing: that view says nothing.
        if a.covered() == 0 {
            continue;
        }
        // Pieces are often open shells (their caps belong to a neighbour): compare solids.
        a.fill_holes();
        b.fill_holes();
        worst = worst.min(raster::iou(&a, &b));
    }
    worst
}

/// A primitive placed in the piece's frame, as triangles there.
pub fn placed(prim: &Primitive, axes: DMat3, at: DVec3) -> Vec<Tri> {
    prim_tris(prim).into_iter().map(|t| Tri { p: t.map(|p| axes * p + at), c: [0.5; 3] }).collect()
}

/// The axes of a round shape along local axis `k` (its own y), right-handed.
pub fn round_axes(k: usize) -> DMat3 {
    let e = |i: usize| DVec3::AXES[i % 3];
    DMat3::from_cols(e(k + 2), e(k), e(k + 1))
}

/// The axes of an extrusion down local axis `k` (its own z): outline x along (k+1)%3, y along
/// (k+2)%3.
pub fn extrude_axes(k: usize) -> DMat3 {
    let e = |i: usize| DVec3::AXES[i % 3];
    DMat3::from_cols(e(k + 1), e(k + 2), e(k))
}

/// Douglas–Peucker on an open polyline: the kept indices.
fn rdp(points: &[[f64; 2]], tol: f64) -> Vec<usize> {
    fn go(points: &[[f64; 2]], a: usize, b: usize, tol: f64, keep: &mut Vec<usize>) {
        let (pa, pb) = (points[a], points[b]);
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let len = (dx * dx + dy * dy).sqrt();
        let mut best = (0.0, a);
        for (i, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = if len < 1e-12 {
                ((p[0] - pa[0]).powi(2) + (p[1] - pa[1]).powi(2)).sqrt()
            } else {
                ((p[0] - pa[0]) * dy - (p[1] - pa[1]) * dx).abs() / len
            };
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol {
            go(points, a, best.1, tol, keep);
            keep.push(best.1);
            go(points, best.1, b, tol, keep);
        }
    }
    if points.len() < 3 {
        return (0..points.len()).collect();
    }
    let mut keep = vec![0];
    go(points, 0, points.len() - 1, tol, &mut keep);
    keep.push(points.len() - 1);
    keep
}

/// Simplifies an open polyline to at most `max` points, loosening the tolerance from `tol`.
fn simplify(points: &[[f64; 2]], tol: f64, max: usize) -> Vec<[f64; 2]> {
    let mut t = tol;
    loop {
        let keep = rdp(points, t);
        if keep.len() <= max || t > 1e3 {
            return keep.into_iter().map(|i| points[i]).collect();
        }
        t *= 1.5;
    }
}

/// The radius per height of a piece round local axis `k`, from its two side silhouettes
/// (`rows` heights from the bottom up): (height, radius) at each row's middle, None where
/// nothing covers the row.
fn radii(local: &[Tri], k: usize, half: DVec3, rows: usize) -> Vec<(f64, Option<f64>)> {
    let (a, b) = ((k + 1) % 3, (k + 2) % 3);
    let mut out: Vec<(f64, Option<f64>)> =
        (0..rows).map(|r| (-half[k] + (r as f64 + 0.5) * 2.0 * half[k] / rows as f64, None)).collect();
    for side in [a, b] {
        let view = side_view(k, side);
        let other = if side == a { b } else { a };
        let reach = half[other].max(MIN_HALF) * 1.02;
        let win = Window { lo: [-reach, -half[k]], hi: [reach, half[k]], w: FIT_PX * 2, h: rows };
        let img = raster::draw(local, &view, &win);
        let px = 2.0 * reach / img.w as f64;
        for (r, slot) in out.iter_mut().enumerate() {
            // Image row 0 is the top.
            let row = rows - 1 - r;
            let cols: Vec<usize> = (0..img.w).filter(|&x| img.cover[row * img.w + x]).collect();
            if let (Some(&lo), Some(&hi)) = (cols.first(), cols.last()) {
                let x0 = -reach + lo as f64 * px;
                let x1 = -reach + (hi + 1) as f64 * px;
                let r = x0.abs().max(x1.abs());
                slot.1 = Some(slot.1.map_or(r * 0.5, |s| s + r * 0.5));
            }
        }
    }
    out
}

/// The outer outline of the silhouette down local axis `k`, in that view's screen coordinates
/// (the piece's (k+1)%3 and (k+2)%3 axes), counter-clockwise.
fn outline(local: &[Tri], k: usize) -> Option<Vec<[f64; 2]>> {
    let view = axis_view(k);
    let own = Window::around(local, &view, 0.0, 1, 1)?;
    let side = (own.hi[0] - own.lo[0]).max(own.hi[1] - own.lo[1]);
    let n = 96;
    let px = side / (n - 2) as f64;
    let (w, h) = (((own.hi[0] - own.lo[0]) / px).ceil() as usize + 2, ((own.hi[1] - own.lo[1]) / px).ceil() as usize + 2);
    let lo = [own.lo[0] - px, own.lo[1] - px];
    let win = Window { lo, hi: [lo[0] + w as f64 * px, lo[1] + h as f64 * px], w, h };
    let img = raster::draw(local, &view, &win);
    // The largest 4-connected blob.
    let mut blob = vec![0u32; w * h];
    let mut best = (0, 0usize);
    let mut next = 0;
    for start in 0..w * h {
        if !img.cover[start] || blob[start] != 0 {
            continue;
        }
        next += 1;
        let (mut stack, mut size) = (vec![start], 0);
        blob[start] = next;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if img.cover[j] && blob[j] == 0 {
                    blob[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        if size > best.1 {
            best = (next, size);
        }
    }
    if best.1 < 3 {
        return None;
    }
    let inside =
        |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && blob[y as usize * w + x as usize] == best.0;
    // Walk the outer boundary along pixel corners (image coordinates, y down), the blob on the
    // right: from the topmost-leftmost pixel's top-left corner, heading right along its top.
    // Headings: 0 right, 1 down, 2 left, 3 up; turning right is +1.
    let first = (0..w * h).find(|&i| blob[i] == best.0)?;
    let (sx, sy) = ((first % w) as i64, (first / w) as i64);
    let step = [(1i64, 0i64), (0, 1), (-1, 0), (0, -1)];
    // The pixels right and left of the edge leaving corner (cx, cy) in heading d.
    let right_of = |cx: i64, cy: i64, d: usize| match d {
        0 => (cx, cy),
        1 => (cx - 1, cy),
        2 => (cx - 1, cy - 1),
        _ => (cx, cy - 1),
    };
    let left_of = |cx: i64, cy: i64, d: usize| match d {
        0 => (cx, cy - 1),
        1 => (cx, cy),
        2 => (cx - 1, cy),
        _ => (cx - 1, cy - 1),
    };
    let (mut cx, mut cy, mut dir) = (sx, sy, 0usize);
    let mut corners: Vec<(i64, i64)> = Vec::new();
    for _ in 0..4 * (w + 1) * (h + 1) {
        corners.push((cx, cy));
        cx += step[dir].0;
        cy += step[dir].1;
        // Right first, so the walk hugs the blob and touches itself rather than crossing.
        let next = [(dir + 1) % 4, dir, (dir + 3) % 4, (dir + 2) % 4].into_iter().find(|&d| {
            let (rx, ry) = right_of(cx, cy, d);
            let (lx, ly) = left_of(cx, cy, d);
            inside(rx, ry) && !inside(lx, ly)
        })?;
        if (cx, cy, next) == (sx, sy, 0) {
            break;
        }
        dir = next;
    }
    // Corners on screen (y up), keeping only where the walk turns.
    let n = corners.len();
    let turns: Vec<(i64, i64)> = (0..n)
        .filter(|&i| {
            let (p, c, q) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
            (c.0 - p.0, c.1 - p.1) != (q.0 - c.0, q.1 - c.1)
        })
        .map(|i| corners[i])
        .collect();
    Some(turns.iter().map(|&(x, y)| [lo[0] + x as f64 * px, lo[1] + (h as i64 - y) as f64 * px]).collect())
}

/// A closed outline simplified to at most `max` points: split at the two farthest-apart
/// corners and simplify each half.
fn simplify_loop(points: &[[f64; 2]], tol: f64, max: usize) -> Vec<[f64; 2]> {
    let n = points.len();
    if n <= 3 {
        return points.to_vec();
    }
    let far = (1..n)
        .max_by(|&i, &j| {
            let d = |k: usize| (points[k][0] - points[0][0]).powi(2) + (points[k][1] - points[0][1]).powi(2);
            d(i).total_cmp(&d(j))
        })
        .unwrap_or(n / 2);
    let mut t = tol;
    loop {
        let first: Vec<[f64; 2]> = points[..=far].to_vec();
        let mut second: Vec<[f64; 2]> = points[far..].to_vec();
        second.push(points[0]);
        let a = rdp(&first, t);
        let b = rdp(&second, t);
        let mut out: Vec<[f64; 2]> = a.iter().map(|&i| first[i]).collect();
        out.extend(b.iter().skip(1).take(b.len().saturating_sub(2)).map(|&i| second[i]));
        if out.len() <= max || t > 1e3 {
            return out;
        }
        t *= 1.4;
    }
}

/// Whether a closed outline has crossing edges.
pub fn crosses(points: &[[f64; 2]]) -> bool {
    let n = points.len();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b, c, d) = (points[i], points[(i + 1) % n], points[j], points[(j + 1) % n]);
            if cross(c, d, a) * cross(c, d, b) < 0.0 && cross(a, b, c) * cross(a, b, d) < 0.0 {
                return true;
            }
        }
    }
    false
}

/// A regular polygon: its corners, centre, circumradius and the angle of its first corner.
#[derive(Clone, Copy, Debug)]
struct Facets {
    n: u32,
    centre: [f64; 2],
    radius: f64,
    phase: f64,
}

/// Whether an outline is a regular polygon of 5 to 16 corners (radii within 7%, corners within
/// 30% of an even spacing): a faceted cylinder or lathe seen down its axis.
fn facets(pts: &[[f64; 2]]) -> Option<Facets> {
    let n = pts.len();
    if !(5..=16).contains(&n) {
        return None;
    }
    let c = pts.iter().fold([0.0; 2], |a, p| [a[0] + p[0] / n as f64, a[1] + p[1] / n as f64]);
    let r: Vec<f64> = pts.iter().map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt()).collect();
    let mean = r.iter().sum::<f64>() / n as f64;
    if mean <= 0.0 || r.iter().any(|x| (x - mean).abs() > 0.07 * mean) {
        return None;
    }
    let mut ang: Vec<f64> = pts.iter().map(|p| (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(std::f64::consts::TAU)).collect();
    ang.sort_by(f64::total_cmp);
    let even = std::f64::consts::TAU / n as f64;
    for i in 0..n {
        let gap = (ang[(i + 1) % n] - ang[i]).rem_euclid(std::f64::consts::TAU);
        if (gap - even).abs() > 0.3 * even {
            return None;
        }
    }
    // The corners' common phase: the circular mean of n times their angles.
    let (cs, sn) = ang.iter().fold((0.0, 0.0), |(c0, s0), a| (c0 + (a * n as f64).cos(), s0 + (a * n as f64).sin()));
    // The radius of the regular polygon with the outline's area (traced corners wander a pixel).
    let area = (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum::<f64>().abs() * 0.5;
    let radius = (2.0 * area / (n as f64 * (std::f64::consts::TAU / n as f64).sin())).sqrt();
    Some(Facets { n: n as u32, centre: c, radius, phase: sn.atan2(cs) / n as f64 })
}

/// A rough token count for a part of this shape (its fields as the writer prints them).
fn tokens(prim: &Primitive) -> usize {
    let base = 18; // name, pos, colour and punctuation
    base + match prim {
        Primitive::Box { .. } | Primitive::Ellipsoid { .. } => 8,
        Primitive::Sphere { .. } => 4,
        Primitive::Cylinder { radius, radius_end, segments, .. } if radius == radius_end => 8 + 3 * (*segments != 32) as usize,
        Primitive::Cylinder { .. } | Primitive::Capsule { .. } => 12,
        Primitive::Lathe { profile, .. } => 4 + 6 * profile.len(),
        Primitive::Extrude { points, .. } => 6 + 6 * points.len(),
        _ => 12,
    }
}

/// Every candidate for a piece whose triangles are `local` (in its frame `fr`), best first.
pub fn candidates(local: &[Tri], fr: &Frame) -> Vec<Cand> {
    let h = fr.half;
    let mut out = Vec::new();
    let mut add = |prim: Primitive, axes: DMat3, at: DVec3| {
        let tris = placed(&prim, axes, at);
        if tris.is_empty() {
            return;
        }
        let iou = silhouette_iou(local, &tris);
        let tokens = tokens(&prim);
        out.push(Cand { prim, axes, at, iou, tokens });
    };
    add(Primitive::Box { half: f(h) }, DMat3::IDENTITY, DVec3::ZERO);
    let (lo, hi) = (h.min_element(), h.max_element());
    if hi - lo <= 0.05 * hi {
        add(Primitive::Sphere { radius: ((h.x + h.y + h.z) / 3.0) as f32 }, DMat3::IDENTITY, DVec3::ZERO);
    } else {
        add(Primitive::Ellipsoid { half: f(h) }, DMat3::IDENTITY, DVec3::ZERO);
    }
    for k in 0..3 {
        let (a, b) = ((k + 1) % 3, (k + 2) % 3);
        let axes = round_axes(k);
        let hh = h[k] as f32;
        let r_box = ((h[a] + h[b]) * 0.5) as f32;
        add(
            Primitive::Cylinder { half_height: hh, radius: r_box, radius_end: r_box, arc: 360.0, segments: 32 },
            axes,
            DVec3::ZERO,
        );
        // Radius per height from the side silhouettes.
        let rows = radii(local, k, h, FIT_PX);
        // The silhouette down this axis: an extrusion's outline, and corners if it is faceted.
        let side = (2.0 * h[a]).max(2.0 * h[b]);
        let outline_pts = outline(local, k).map(|l| simplify_loop(&l, side / 94.0, OUTLINE_POINTS));
        let faceted = outline_pts.as_deref().and_then(facets);
        if let Some(fc) = faceted {
            let spun = axes * DMat3::from_rotation_y(fc.phase);
            let at = DVec3::AXES[a] * fc.centre[0] + DVec3::AXES[b] * fc.centre[1];
            let r = fc.radius as f32;
            add(Primitive::Cylinder { half_height: hh, radius: r, radius_end: r, arc: 360.0, segments: fc.n }, spun, at);
        }
        let known: Vec<(f64, f64)> = rows.iter().filter_map(|(y, r)| r.map(|r| (*y, r))).collect();
        if known.len() >= 2 {
            // A frustum by least squares over the rows.
            let n = known.len() as f64;
            let (sy, sr) = known.iter().fold((0.0, 0.0), |(a, b), (y, r)| (a + y, b + r));
            let (my, mr) = (sy / n, sr / n);
            let (sxy, sxx) = known.iter().fold((0.0, 0.0), |(a, b), (y, r)| (a + (y - my) * (r - mr), b + (y - my).powi(2)));
            let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
            let r0 = (mr + slope * (-h[k] - my)).max(0.0) as f32;
            let r1 = (mr + slope * (h[k] - my)).max(0.0) as f32;
            if (r0 - r1).abs() > 0.05 * r0.max(r1) {
                let (r0, r1) = if r1 < 0.08 * r0 {
                    (r0, 0.0)
                } else if r0 < 0.08 * r1 {
                    (0.0, r1)
                } else {
                    (r0, r1)
                };
                if r0 > 0.0 {
                    add(
                        Primitive::Cylinder { half_height: hh, radius: r0, radius_end: r1, arc: 360.0, segments: 32 },
                        axes,
                        DVec3::ZERO,
                    );
                    if let Some(fc) = faceted {
                        // Faceted and tapered: the wider end's corners on the outline's circle.
                        let scale = fc.radius as f32 / r0.max(r1).max(1e-6);
                        let at = DVec3::AXES[a] * fc.centre[0] + DVec3::AXES[b] * fc.centre[1];
                        let prim = Primitive::Cylinder {
                            half_height: hh,
                            radius: r0 * scale,
                            radius_end: r1 * scale,
                            arc: 360.0,
                            segments: fc.n,
                        };
                        add(prim, axes * DMat3::from_rotation_y(fc.phase), at);
                    }
                } else {
                    // A cone pointing down: turned over.
                    let down = DMat3::from_cols(axes.col(0), -axes.col(1), -axes.col(2));
                    add(
                        Primitive::Cylinder { half_height: hh, radius: r1, radius_end: 0.0, arc: 360.0, segments: 32 },
                        down,
                        DVec3::ZERO,
                    );
                }
            } else {
                let r = mr as f32;
                add(
                    Primitive::Cylinder { half_height: hh, radius: r, radius_end: r, arc: 360.0, segments: 32 },
                    axes,
                    DVec3::ZERO,
                );
            }
            // A capsule when the ends are round enough to need it.
            let rc = r_box.min(hh);
            if hh > rc * 1.05 {
                add(Primitive::Capsule { a: -(hh - rc), b: hh - rc, radius: rc, radius_end: rc }, axes, DVec3::ZERO);
            }
            // A lathe: the rows' radii from the bottom to the top, simplified.
            let size = 2.0 * h.max_element();
            let mut profile: Vec<[f64; 2]> = Vec::new();
            for (i, (y, r)) in known.iter().enumerate() {
                if i == 0 {
                    profile.push([*r, -h[k]]);
                }
                profile.push([*r, *y]);
                if i + 1 == known.len() {
                    profile.push([*r, h[k]]);
                }
            }
            let profile = simplify(&profile.iter().map(|p| [p[1], p[0]]).collect::<Vec<_>>(), 0.02 * size, LATHE_POINTS);
            let profile: Vec<[f32; 2]> = profile.iter().map(|p| [p[1].max(0.0) as f32, p[0] as f32]).collect();
            if profile.len() >= 2 && profile.iter().any(|p| p[0] > 0.0) {
                if let Some(fc) = faceted.filter(|fc| fc.n >= 6) {
                    // Faceted: corners on the outline's circle at the widest row.
                    let widest = profile.iter().map(|p| p[0]).fold(0.0f32, f32::max);
                    let scale = fc.radius as f32 / widest.max(1e-6);
                    let corners: Vec<[f32; 2]> = profile.iter().map(|p| [p[0] * scale, p[1]]).collect();
                    let at = DVec3::AXES[a] * fc.centre[0] + DVec3::AXES[b] * fc.centre[1];
                    add(Primitive::Lathe { profile: corners, segments: fc.n }, axes * DMat3::from_rotation_y(fc.phase), at);
                }
                add(Primitive::Lathe { profile, segments: 24 }, axes, DVec3::ZERO);
            }
        }
        // An extrusion down this axis.
        if let Some(mut pts) = outline_pts {
            if pts.len() >= 3 && !crosses(&pts) {
                let area: f64 = (0..pts.len())
                    .map(|i| pts[i][0] * pts[(i + 1) % pts.len()][1] - pts[(i + 1) % pts.len()][0] * pts[i][1])
                    .sum();
                if area < 0.0 {
                    pts.reverse();
                }
                let points: Vec<[f32; 2]> = pts.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
                add(Primitive::Extrude { points, half_depth: hh }, extrude_axes(k), DVec3::ZERO);
            }
        }
    }
    out.sort_by(|a, b| b.score().total_cmp(&a.score()));
    out
}
