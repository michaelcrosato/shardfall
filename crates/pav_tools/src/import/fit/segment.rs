//! Segmentation (plan §8.5, step 1): the connected parts of each (node, material) group, every
//! piece with its triangles in face colour, its area-weighted colour and how far its faces'
//! colours spread.

use std::collections::BTreeMap;

use glam::DVec3;

use super::super::normalize;
use super::super::raster::{Tri, oklab};
use super::super::scene::Scene;

/// Faces within this Oklab distance of a cluster's first colour join it.
pub const CLUSTER: f64 = 0.04;
/// A second colour makes a part multicoloured when it covers this share of its area...
pub const SECOND_SHARE: f64 = 0.15;
/// ...and differs from the first by this much.
pub const SECOND_DE: f64 = 0.06;

/// One piece to fit.
#[derive(Clone, Debug)]
pub struct Piece {
    /// The scene node it came from (names, when a model has several).
    pub node: usize,
    pub tris: Vec<Tri>,
    /// Square metres.
    pub area: f64,
    /// Area-weighted mean, linear RGB.
    pub colour: [f64; 3],
    /// The area-weighted mean Oklab distance of its faces' colours from `colour`.
    pub spread: f64,
    pub multicolour: bool,
    /// An open surface (an edge with one face) in a double-sided material: seen from behind too.
    pub two_sided: bool,
}

impl Piece {
    /// A piece of these triangles: its area, colour, colour spread and whether it has two
    /// clear colours.
    pub fn from_tris(node: usize, tris: Vec<Tri>) -> Piece {
        let (mut area, mut sum) = (0.0, [0.0; 3]);
        for t in &tris {
            let a = tri_area(t);
            area += a;
            for (s, c) in sum.iter_mut().zip(t.c) {
                *s += c * a;
            }
        }
        let colour = if area > 0.0 { sum.map(|s| s / area) } else { tris.first().map_or([0.5; 3], |t| t.c) };
        let (_, cl) = clusters(&tris);
        let total: f64 = cl.iter().map(|c| c.1).sum();
        let multicolour = cl.len() > 1 && cl[1..].iter().any(|c| c.1 >= SECOND_SHARE * total && dist(c.0, cl[0].0) >= SECOND_DE);
        let mean = oklab(colour);
        let spread = if area > 0.0 { tris.iter().map(|t| tri_area(t) * dist(oklab(t.c), mean)).sum::<f64>() / area } else { 0.0 };
        Piece { node, tris, area, colour, spread, multicolour, two_sided: false }
    }

    /// The box round its corners.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        bounds(&self.tris)
    }
}

pub fn tri_area(t: &Tri) -> f64 {
    (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).length() * 0.5
}

pub fn bounds(tris: &[Tri]) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for t in tris {
        for p in t.p {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

/// Greedy colour clusters over faces, largest area first: each face's cluster index, and each
/// cluster's (Oklab of its first colour, area).
fn clusters(tris: &[Tri]) -> (Vec<usize>, Vec<([f64; 3], f64)>) {
    let mut order: Vec<usize> = (0..tris.len()).collect();
    let areas: Vec<f64> = tris.iter().map(tri_area).collect();
    // Faces of one colour together, the colour covering most area first.
    let mut by_colour: BTreeMap<[u64; 3], f64> = BTreeMap::new();
    let key = |c: [f64; 3]| c.map(f64::to_bits);
    for (t, a) in tris.iter().zip(&areas) {
        *by_colour.entry(key(t.c)).or_default() += a;
    }
    order.sort_by(|&a, &b| by_colour[&key(tris[b].c)].total_cmp(&by_colour[&key(tris[a].c)]).then(a.cmp(&b)));
    let mut label = vec![usize::MAX; tris.len()];
    let mut out: Vec<([f64; 3], f64)> = Vec::new();
    for i in order {
        let lab = oklab(tris[i].c);
        let near = out.iter().position(|(c, _)| dist(*c, lab) < CLUSTER);
        let k = near.unwrap_or_else(|| {
            out.push((lab, 0.0));
            out.len() - 1
        });
        out[k].1 += areas[i];
        label[i] = k;
    }
    (label, out)
}

pub fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The pieces of a scene: its connected parts, each with its colour spread (the area-weighted
/// mean Oklab distance of its faces from its colour) and whether it is multicoloured (a second
/// colour cluster with 15% of its area, 0.06 away from the first). A multicoloured piece is
/// not split (its colour patches are open surfaces no primitive fits): the asset loop moves it
/// into the mesh part when the asset's colours are off.
pub fn pieces(s: &Scene) -> Vec<Piece> {
    let mut out = Vec::new();
    for part in normalize::parts(s) {
        let tris: Vec<Tri> = part
            .tris
            .iter()
            .map(|&(mi, ti)| {
                let m = &s.meshes[mi as usize];
                let t = m.tris[ti as usize];
                let c = normalize::face_colour(s, mi as usize, ti as usize);
                Tri { p: t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from))), c: [c[0], c[1], c[2]] }
            })
            .collect();
        let double_sided = part.material.and_then(|i| s.materials.get(i)).is_some_and(|m| m.double_sided);
        let open = double_sided && is_open(&tris);
        let piece = Piece { two_sided: open, ..Piece::from_tris(part.node, tris) };
        out.push(piece);
    }
    out
}

/// Whether a surface has an edge only one of its faces uses (corners welded at 0.1 mm): an open
/// sheet or shell, whose back can be seen.
fn is_open(tris: &[Tri]) -> bool {
    let key = |p: DVec3| [p.x, p.y, p.z].map(|c| (c / normalize::WELD).round() as i64);
    let mut uses: BTreeMap<([i64; 3], [i64; 3]), u32> = BTreeMap::new();
    for t in tris {
        let k = t.p.map(key);
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            if a != b {
                *uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
    }
    uses.values().any(|&n| n == 1)
}
