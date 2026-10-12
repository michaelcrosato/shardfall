//! Mesh parts (plan §8.5, step 8): the pieces no primitive fits, as one small mesh file per
//! asset: corners welded on whole millimetres, at most 64 colours (Oklab median cut when a model
//! has more), at most 4,096 triangles (vertex clustering until it fits).

use std::collections::BTreeMap;

use glam::DVec3;
use pav_core::props::mesh::{MAX_PALETTE, MAX_TRIS, MeshFile};

use super::super::raster::{Tri, oklab};
use super::super::scene::hex;

/// Faces welded onto a grid of `cell` millimetres: each cell's corners become one vertex at
/// their mean (whole millimetres in the file), vertices in first-use order, faces with their
/// colour, degenerate faces dropped.
fn weld(tris: &[Tri], cell: f64) -> (Vec<[i32; 3]>, Vec<([u32; 3], [f64; 3])>) {
    let mut ids: BTreeMap<[i64; 3], u32> = BTreeMap::new();
    let mut sums: Vec<(DVec3, f64)> = Vec::new();
    let mut faces = Vec::new();
    for t in tris {
        let idx = t.p.map(|p| {
            let k = [p.x, p.y, p.z].map(|c| (c * 1000.0 / cell).round() as i64);
            let i = *ids.entry(k).or_insert_with(|| {
                sums.push((DVec3::ZERO, 0.0));
                (sums.len() - 1) as u32
            });
            sums[i as usize].0 += p;
            sums[i as usize].1 += 1.0;
            i
        });
        if idx[0] != idx[1] && idx[1] != idx[2] && idx[0] != idx[2] {
            faces.push((idx, t.c));
        }
    }
    let v = sums.iter().map(|(sum, n)| (*sum / *n * 1000.0).to_array().map(|c| c.round() as i32)).collect();
    (v, faces)
}

/// Colours cut to at most `max` by median cut in Oklab (area-blind: faces count alike), each
/// box's mean as hex; returns every input colour's palette index.
fn median_cut(colours: &[[f64; 3]], max: usize) -> (Vec<String>, Vec<usize>) {
    let labs: Vec<[f64; 3]> = colours.iter().map(|c| oklab(*c)).collect();
    let mut boxes: Vec<Vec<usize>> = vec![(0..colours.len()).collect()];
    while boxes.len() < max {
        // Split the box with the widest channel at its median.
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| {
                let span = |k: usize| {
                    let (lo, hi) =
                        b.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &j| (l.min(labs[j][k]), h.max(labs[j][k])));
                    hi - lo
                };
                let (k, s) = (0..3).map(|k| (k, span(k))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                (i, k, s)
            })
            .max_by(|a, b| a.2.total_cmp(&b.2));
        let Some((i, k, s)) = widest else { break };
        if s <= 0.0 {
            break;
        }
        let mut b = boxes.swap_remove(i);
        b.sort_by(|&x, &y| labs[x][k].total_cmp(&labs[y][k]).then(x.cmp(&y)));
        let upper = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(upper);
    }
    let mut palette = Vec::new();
    let mut index = vec![0; colours.len()];
    for b in &boxes {
        let mean = b.iter().fold([0.0; 3], |a, &j| [a[0] + colours[j][0], a[1] + colours[j][1], a[2] + colours[j][2]]);
        let mean = mean.map(|c| c / b.len() as f64);
        palette.push(hex(mean));
        for &j in b {
            index[j] = palette.len() - 1;
        }
    }
    (palette, index)
}

/// A mesh file of `tris` (metres, linear colours) named `name`.
pub fn mesh_file(name: &str, tris: &[Tri]) -> Option<MeshFile> {
    // Whole millimetres; over budget, coarser cells (a fifth larger each time) until it fits.
    let mut cell = 1.0;
    let (v, faces) = loop {
        let (v, faces) = weld(tris, cell);
        if faces.len() <= MAX_TRIS || cell > 1e6 {
            break (v, faces);
        }
        cell *= 1.2;
    };
    if faces.is_empty() {
        return None;
    }
    // Colours as hex; the palette is the sorted distinct hexes when they fit.
    let hexes: Vec<String> = faces.iter().map(|(_, c)| hex(*c)).collect();
    let distinct: Vec<String> = {
        let mut d = hexes.clone();
        d.sort();
        d.dedup();
        d
    };
    let (palette, index): (Vec<String>, Vec<usize>) = if distinct.len() <= MAX_PALETTE {
        let at: BTreeMap<&String, usize> = distinct.iter().enumerate().map(|(i, h)| (h, i)).collect();
        (distinct.clone(), hexes.iter().map(|h| at[h]).collect())
    } else {
        let colours: Vec<[f64; 3]> = faces.iter().map(|(_, c)| *c).collect();
        median_cut(&colours, MAX_PALETTE)
    };
    // Only the vertices faces use, in first-use order.
    let mut remap: BTreeMap<u32, u32> = BTreeMap::new();
    let mut used = Vec::new();
    let mut f = Vec::with_capacity(faces.len());
    for ((idx, _), p) in faces.iter().zip(&index) {
        let idx = idx.map(|i| {
            let n = used.len() as u32;
            *remap.entry(i).or_insert_with(|| {
                used.push(v[i as usize]);
                n
            })
        });
        f.push([idx[0], idx[1], idx[2], *p as u32]);
    }
    Some(MeshFile { name: name.to_string(), budget: MAX_TRIS as u32, two_sided: false, palette, v: used, f, source: None })
}
