//! Normalisation (`f64` throughout): a model's up axis, facing (90° steps), units and origin made
//! the engine's (+y up, front +z, metres), and triangles turned outward where a primitive's
//! normals say they are inside out. Plus what the census and the fitter build on: vertices
//! welded at 0.1 mm for adjacency, the connected parts of each (node, material) group, and each
//! face's colour (base colour × texture × vertex colour, averaged in linear light).

use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use glam::{DMat4, DVec3};

use super::scene::{Scene, hex, oklab, srgb_to_linear};

/// Where the origin goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Where the file has it.
    Keep,
    /// On the floor under the middle: the lowest point at y = 0, centred in x and z.
    Floor,
    /// The middle of the bounds.
    Center,
}

/// How a pack or model is brought into the engine's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Norm {
    /// The file's up axis is +z (rotated −90° about x to +y).
    pub z_up: bool,
    /// Turned this many degrees about +y (a multiple of 90) so its front faces +z.
    pub yaw: i32,
    /// Metres per file unit.
    pub scale: f64,
    pub place: Place,
}

impl Default for Norm {
    fn default() -> Self {
        Norm { z_up: false, yaw: 0, scale: 1.0, place: Place::Keep }
    }
}

impl Norm {
    /// From the words a catalog or tool call uses (`up` y|z, `yaw` degrees, `scale` m per unit,
    /// `place` keep|floor|center); a missing word keeps the default.
    pub fn parse(up: Option<&str>, yaw: Option<f64>, scale: Option<f64>, place: Option<&str>) -> Result<Norm> {
        let mut n = Norm::default();
        match up.map(str::to_lowercase).as_deref() {
            None | Some("y") => {}
            Some("z") => n.z_up = true,
            Some(u) => bail!("up={u}: y or z"),
        }
        if let Some(y) = yaw {
            if y.fract() != 0.0 || (y as i64).rem_euclid(90) != 0 {
                bail!("yaw={y}: a multiple of 90 degrees");
            }
            n.yaw = (y as i64).rem_euclid(360) as i32;
        }
        if let Some(s) = scale {
            if !(s.is_finite() && s > 0.0) {
                bail!("scale={s}: metres per unit, above 0");
            }
            n.scale = s;
        }
        n.place = match place.map(str::to_lowercase).as_deref() {
            None | Some("keep") => Place::Keep,
            Some("floor") => Place::Floor,
            Some("center" | "centre") => Place::Center,
            Some(p) => bail!("place={p}: keep, floor or center"),
        };
        Ok(n)
    }

    /// The axis, facing and unit change as one matrix (placement comes after, from the bounds).
    pub fn matrix(&self) -> DMat4 {
        // Exact quarter turns (no rounding noise from sin and cos): z up is −90° about x,
        // (x, y, z) → (x, z, −y).
        let up = if self.z_up {
            DMat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0])
        } else {
            DMat4::IDENTITY
        };
        let (s, c) = match self.yaw {
            90 => (1.0, 0.0),
            180 => (0.0, -1.0),
            270 => (-1.0, 0.0),
            _ => (0.0, 1.0),
        };
        let yaw = DMat4::from_cols_array(&[c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]);
        DMat4::from_scale(DVec3::splat(self.scale)) * yaw * up
    }
}

/// What normalising did.
#[derive(Clone, Debug, Default)]
pub struct Normalized {
    /// Meshes whose triangles were reversed (their normals said they were inside out).
    pub flipped: Vec<usize>,
    /// How far the placement moved the model (metres).
    pub moved: [f64; 3],
}

/// Brings a scene into the engine's frame.
pub fn normalize(s: &mut Scene, n: &Norm) -> Normalized {
    let m = n.matrix();
    if m != DMat4::IDENTITY {
        s.transform(m);
    }
    let mut out = Normalized { flipped: inside_out(s), ..Normalized::default() };
    for &i in &out.flipped {
        for t in &mut s.meshes[i].tris {
            t.swap(1, 2);
        }
    }
    if let (Some((lo, hi)), true) = (s.bounds(), n.place != Place::Keep) {
        let mid = (lo + hi) * 0.5;
        let to = match n.place {
            Place::Floor => DVec3::new(-mid.x, -lo.y, -mid.z),
            _ => -mid,
        };
        if to != DVec3::ZERO {
            s.transform(DMat4::from_translation(to));
            out.moved = to.to_array();
        }
    }
    out
}

/// The meshes whose vertex normals disagree with their triangles' winding over more than half
/// their area.
pub fn inside_out(s: &Scene) -> Vec<usize> {
    let mut out = Vec::new();
    for (mi, m) in s.meshes.iter().enumerate() {
        if m.nrm.is_empty() {
            continue;
        }
        let (mut total, mut wrong) = (0.0, 0.0);
        for t in &m.tris {
            let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
            let g = (p[1] - p[0]).cross(p[2] - p[0]);
            let area = g.length() * 0.5;
            let nv: DVec3 = t.iter().map(|&i| DVec3::from(m.nrm[i as usize].map(f64::from))).sum();
            total += area;
            if g.dot(nv) < 0.0 {
                wrong += area;
            }
        }
        if total > 0.0 && wrong > 0.5 * total {
            out.push(mi);
        }
    }
    out
}

/// The share of triangle area (0..1) whose winding agrees with its vertex normals, over meshes
/// that have normals.
pub fn normal_agreement(s: &Scene) -> f64 {
    let (mut total, mut good) = (0.0, 0.0);
    for m in s.meshes.iter().filter(|m| !m.nrm.is_empty()) {
        for t in &m.tris {
            let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
            let g = (p[1] - p[0]).cross(p[2] - p[0]);
            let nv: DVec3 = t.iter().map(|&i| DVec3::from(m.nrm[i as usize].map(f64::from))).sum();
            total += g.length();
            if g.dot(nv) >= 0.0 {
                good += g.length();
            }
        }
    }
    if total > 0.0 { good / total } else { 1.0 }
}

/// The weld grid: 0.1 mm.
pub const WELD: f64 = 1e-4;

/// One connected part: triangles (mesh, triangle) of one (node, material) group that share
/// welded vertices.
#[derive(Clone, Debug)]
pub struct Part {
    pub node: usize,
    pub material: Option<usize>,
    pub tris: Vec<(u32, u32)>,
    /// Square metres.
    pub area: f64,
}

fn find(p: &mut [u32], mut i: u32) -> u32 {
    while p[i as usize] != i {
        p[i as usize] = p[p[i as usize] as usize];
        i = p[i as usize];
    }
    i
}

/// The connected parts of every (node, material) group, in the order their first triangle
/// appears.
pub fn parts(s: &Scene) -> Vec<Part> {
    let mut groups: BTreeMap<(usize, Option<usize>), Vec<usize>> = BTreeMap::new();
    for (i, m) in s.meshes.iter().enumerate() {
        groups.entry((m.node, m.material)).or_default().push(i);
    }
    let mut out = Vec::new();
    for ((node, material), meshes) in groups {
        // Welded vertex ids across the group's meshes.
        let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
        let mut welded: Vec<Vec<u32>> = Vec::new();
        for &mi in &meshes {
            let w = s.meshes[mi]
                .pos
                .iter()
                .map(|p| {
                    let k = p.map(|c| (c as f64 / WELD).round() as i64);
                    let n = ids.len() as u32;
                    *ids.entry(k).or_insert(n)
                })
                .collect();
            welded.push(w);
        }
        let mut parent: Vec<u32> = (0..ids.len() as u32).collect();
        for (k, &mi) in meshes.iter().enumerate() {
            for t in &s.meshes[mi].tris {
                let [a, b, c] = t.map(|i| welded[k][i as usize]);
                for (x, y) in [(a, b), (b, c)] {
                    let (rx, ry) = (find(&mut parent, x), find(&mut parent, y));
                    if rx != ry {
                        parent[rx.max(ry) as usize] = rx.min(ry);
                    }
                }
            }
        }
        let mut by_root: BTreeMap<u32, usize> = BTreeMap::new();
        let first = out.len();
        for (k, &mi) in meshes.iter().enumerate() {
            let m = &s.meshes[mi];
            for (ti, t) in m.tris.iter().enumerate() {
                let r = find(&mut parent, welded[k][t[0] as usize]);
                let at = *by_root.entry(r).or_insert_with(|| {
                    out.push(Part { node, material, tris: Vec::new(), area: 0.0 });
                    out.len() - 1
                });
                let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
                out[at].tris.push((mi as u32, ti as u32));
                out[at].area += (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
            }
        }
        debug_assert!(out[first..].iter().all(|p| !p.tris.is_empty()));
    }
    out
}

/// Where a triangle's texture is sampled: a 7-point rule exact for quintics (Radon), as
/// barycentric weights of the corners and the sample's share of the area mean.
const SAMPLES: [([f64; 3], f64); 7] = {
    const A1: f64 = 0.059_715_871_789_769_82;
    const B1: f64 = 0.470_142_064_105_115_1;
    const A2: f64 = 0.797_426_985_353_087_3;
    const B2: f64 = 0.101_286_507_323_456_3;
    const W1: f64 = 0.132_394_152_788_506_2;
    const W2: f64 = 0.125_939_180_544_827_1;
    [
        ([1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0], 0.225),
        ([A1, B1, B1], W1),
        ([B1, A1, B1], W1),
        ([B1, B1, A1], W1),
        ([A2, B2, B2], W2),
        ([B2, A2, B2], W2),
        ([B2, B2, A2], W2),
    ]
};

/// A triangle's colour, linear RGBA: its material's base colour × the area mean of its texture
/// (7 samples, in linear light) × its vertex colours' mean.
pub fn face_colour(s: &Scene, mesh: usize, tri: usize) -> [f64; 4] {
    let m = &s.meshes[mesh];
    let t = m.tris[tri];
    let mat = m.material.and_then(|i| s.materials.get(i));
    let mut c = mat.map_or([1.0; 4], |mt| mt.base.map(f64::from));
    if let Some(tex) = mat.and_then(|mt| mt.base_tex) {
        let uvs = if tex.uv_set == 1 { &m.uv1 } else { &m.uv };
        if let (Some(img), false) = (s.images.get(tex.image), uvs.is_empty()) {
            if !img.rgba.is_empty() && img.w > 0 && img.h > 0 {
                let x = tex.xform.map(f64::from);
                let corners = t.map(|i| uvs[i as usize].map(f64::from));
                let texel = |b: [f64; 3]| -> [f64; 4] {
                    let uv = [0, 1].map(|k| (0..3).map(|j| b[j] * corners[j][k]).sum::<f64>());
                    let (u, v) = (x[0] * uv[0] + x[1] * uv[1] + x[2], x[3] * uv[0] + x[4] * uv[1] + x[5]);
                    let px = ((tex.wrap[0].apply(u) * img.w as f64) as u32).min(img.w - 1);
                    let py = ((tex.wrap[1].apply(v) * img.h as f64) as u32).min(img.h - 1);
                    let o = ((py * img.w + px) * 4) as usize;
                    let p = &img.rgba[o..o + 4];
                    [srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2]), p[3] as f64 / 255.0]
                };
                // The first sample plus the weighted differences: exact when every sample lands
                // on one texel.
                let first = texel(SAMPLES[0].0);
                let mut mean = first;
                for (b, w) in &SAMPLES[1..] {
                    let other = texel(*b);
                    for k in 0..4 {
                        mean[k] += w * (other[k] - first[k]);
                    }
                }
                for k in 0..4 {
                    c[k] *= mean[k];
                }
            }
        }
    }
    if !m.col.is_empty() {
        for (k, ck) in c.iter_mut().enumerate() {
            *ck *= t.iter().map(|&i| m.col[i as usize][k] as f64).sum::<f64>() / 3.0;
        }
    }
    c
}

/// Colours closer than this (ΔE in Oklab) count as one in `palette`.
pub const SAME_COLOUR: f64 = 0.05;

/// The model's main colours: faces bucketed by colour (sRGB, 32 levels a channel), buckets that
/// look alike (ΔE_OK < `SAME_COLOUR`, largest first) merged, each colour's area-weighted mean as
/// hex with its share of the area (%), largest first, at most `n`.
pub fn palette(s: &Scene, n: usize) -> Vec<(String, f64)> {
    let mut buckets: BTreeMap<[u8; 3], ([f64; 3], f64)> = BTreeMap::new();
    let mut total = 0.0;
    for (mi, m) in s.meshes.iter().enumerate() {
        for (ti, t) in m.tris.iter().enumerate() {
            let p = t.map(|i| DVec3::from(m.pos[i as usize].map(f64::from)));
            let area = (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
            if area <= 0.0 {
                continue;
            }
            let c = face_colour(s, mi, ti);
            let key = [0, 1, 2].map(|k| super::scene::linear_to_srgb(c[k]) >> 3);
            let b = buckets.entry(key).or_insert(([0.0; 3], 0.0));
            for (acc, ck) in b.0.iter_mut().zip(c) {
                *acc += ck * area;
            }
            b.1 += area;
            total += area;
        }
    }
    let mut order: Vec<([f64; 3], f64)> = buckets.into_values().collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    // Greedy merge, largest first: (linear sum, area, Oklab of the first member).
    let mut merged: Vec<([f64; 3], f64, [f64; 3])> = Vec::new();
    for (sum, a) in order {
        let lab = oklab(sum.map(|v| v / a));
        let near = merged.iter_mut().find(|m| (0..3).map(|k| (m.2[k] - lab[k]).powi(2)).sum::<f64>().sqrt() < SAME_COLOUR);
        match near {
            Some(m) => {
                for (acc, v) in m.0.iter_mut().zip(sum) {
                    *acc += v;
                }
                m.1 += a;
            }
            None => merged.push((sum, a, lab)),
        }
    }
    let mut out: Vec<(String, f64)> =
        merged.iter().map(|(sum, a, _)| (hex(sum.map(|v| v / a)), (a / total * 1000.0).round() / 10.0)).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns_and_z_up_are_exact() {
        let n = Norm::parse(Some("z"), Some(90.0), Some(0.01), None).unwrap();
        let m = n.matrix();
        // z-up +z becomes +y; a quarter turn takes +x to -z... at 0.01 m per unit.
        assert_eq!(m.transform_point3(DVec3::new(0.0, 0.0, 100.0)), DVec3::new(0.0, 1.0, 0.0));
        assert_eq!(m.transform_point3(DVec3::new(100.0, 0.0, 0.0)), DVec3::new(0.0, 0.0, -1.0));
        assert!(Norm::parse(None, Some(45.0), None, None).is_err());
        assert!(Norm::parse(None, None, None, Some("top")).is_err());
        assert_eq!(Norm::parse(None, Some(-90.0), None, None).unwrap().yaw, 270);
    }
}
