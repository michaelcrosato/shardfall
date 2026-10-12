//! Fidelity of a translation against its source (plan §8.6), on the CPU and deterministic:
//! - six orthographic views (front, back, left, right, top, iso) at 128 px: the intersection
//!   over union of the silhouettes, its minimum (`iou`) and mean (`iou_mean`); a view in which
//!   the source is a sliver (under 2% as deep as wide) says nothing and is skipped;
//! - `de`: the Oklab distance ×100 between the nearest surfaces' colours where both cover a
//!   pixel, averaged per view, then over the views;
//! - `mean_mm` / `worst_mm`: the symmetric Chamfer distance (4,096 surface samples each way,
//!   to the other surface), its mean and 95th percentile;
//! - the tier from those and the asset's size in characters.
//!
//! The GPU cross-check (`iou_gpu`) is deferred (plan §10.3).

use glam::DVec3;

use super::raster::{self, Image, Tri, View, Window};

/// Pixels a side in the metric views.
pub const VIEW_PX: usize = 128;
/// Pixels a side in the compare image's tiles.
pub const COMPARE_PX: usize = 160;
/// Surface samples each way for the Chamfer distance.
pub const SAMPLES: usize = 4_096;

/// How closely a translation fits its source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fidelity {
    /// The worst view's IoU.
    pub iou: f64,
    pub iou_mean: f64,
    /// Per view (skipped views left out).
    pub views: Vec<(&'static str, f64)>,
    /// Oklab ΔE × 100 over shared pixels.
    pub de: f64,
    pub mean_mm: f64,
    pub worst_mm: f64,
}

/// The six views' silhouettes and colours compared.
pub fn compare(source: &[Tri], fit: &[Tri]) -> Fidelity {
    let mut out = Fidelity::default();
    let mut des = Vec::new();
    for (name, view) in raster::six_views() {
        let Some(own) = Window::around(source, &view, 0.0, 1, 1) else { continue };
        let (w, h) = (own.hi[0] - own.lo[0], own.hi[1] - own.lo[1]);
        if w.min(h) < 0.02 * w.max(h) {
            continue;
        }
        let Some(win) = Window::around(source.iter().chain(fit), &view, 0.05, VIEW_PX, VIEW_PX) else { continue };
        let (a, b) = (raster::draw(source, &view, &win), raster::draw(fit, &view, &win));
        out.views.push((name, raster::iou(&a, &b)));
        if let Some(de) = shared_de(&a, &b) {
            des.push(de);
        }
    }
    if out.views.is_empty() {
        // A model flat in every direction (a point or a line): nothing to compare.
        out.iou = 1.0;
        out.iou_mean = 1.0;
    } else {
        out.iou = out.views.iter().map(|v| v.1).fold(1.0, f64::min);
        out.iou_mean = out.views.iter().map(|v| v.1).sum::<f64>() / out.views.len() as f64;
    }
    out.de = if des.is_empty() { 0.0 } else { des.iter().sum::<f64>() / des.len() as f64 };
    let (mean, worst) = chamfer(source, fit);
    out.mean_mm = mean;
    out.worst_mm = worst;
    out
}

/// Mean ΔE_OK × 100 over the pixels both images cover.
fn shared_de(a: &Image, b: &Image) -> Option<f64> {
    let (mut sum, mut n) = (0.0, 0usize);
    for i in 0..a.cover.len() {
        if a.cover[i] && b.cover[i] {
            sum += raster::delta_e(a.colour[i], b.colour[i]);
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f64 * 100.0)
}

// --- Chamfer -----------------------------------------------------------------------------------

/// Points spread over a surface by area: stratified over the running area, placed inside each
/// triangle by a low-discrepancy sequence (no randomness).
pub fn surface_samples(tris: &[Tri], n: usize) -> Vec<DVec3> {
    let mut acc = Vec::with_capacity(tris.len());
    let mut total = 0.0;
    for t in tris {
        total += (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).length() * 0.5;
        acc.push(total);
    }
    if total <= 0.0 {
        return Vec::new();
    }
    // The R2 sequence (plastic number).
    let (a1, a2) = (0.754_877_666_246_692_7, 0.569_840_290_998_053_3);
    (0..n)
        .map(|k| {
            let at = (k as f64 + 0.5) / n as f64 * total;
            let i = acc.partition_point(|&a| a < at).min(tris.len() - 1);
            let (mut s, mut t) = ((0.5 + a1 * k as f64).fract(), (0.5 + a2 * k as f64).fract());
            if s + t > 1.0 {
                (s, t) = (1.0 - s, 1.0 - t);
            }
            let p = tris[i].p;
            p[0] + (p[1] - p[0]) * s + (p[2] - p[0]) * t
        })
        .collect()
}

/// The closest point of triangle `abc` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Triangles bucketed on a uniform grid, for nearest-surface queries.
struct Grid {
    lo: DVec3,
    cell: f64,
    dims: [usize; 3],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(tris: &[Tri]) -> Grid {
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for t in tris {
            for p in t.p {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let size = (hi - lo).max(DVec3::splat(1e-6));
        let cell = (size.x * size.y * size.z / 4096.0).cbrt().max(size.max_element() / 64.0);
        let dims = [0, 1, 2].map(|k| ((size[k] / cell).ceil() as usize).clamp(1, 64));
        let mut g = Grid { lo, cell, dims, cells: vec![Vec::new(); dims[0] * dims[1] * dims[2]] };
        for (i, t) in tris.iter().enumerate() {
            let tl = t.p[0].min(t.p[1]).min(t.p[2]);
            let th = t.p[0].max(t.p[1]).max(t.p[2]);
            let (a, b) = (g.index(tl), g.index(th));
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        g.cells[(x * dims[1] + y) * dims[2] + z].push(i as u32);
                    }
                }
            }
        }
        g
    }

    fn index(&self, p: DVec3) -> [usize; 3] {
        [0, 1, 2].map(|k| (((p[k] - self.lo[k]) / self.cell).floor().max(0.0) as usize).min(self.dims[k] - 1))
    }

    /// The distance from `p` to the nearest triangle: cells in growing shells until the
    /// nearest found is closer than any unsearched cell can be.
    fn distance(&self, tris: &[Tri], p: DVec3) -> f64 {
        let c = self.index(p);
        let mut best = f64::INFINITY;
        let outside = (self.lo - p)
            .max(p - (self.lo + DVec3::new(self.dims[0] as f64, self.dims[1] as f64, self.dims[2] as f64) * self.cell))
            .max(DVec3::ZERO)
            .length();
        let max_ring = *self.dims.iter().max().unwrap();
        for ring in 0..=max_ring {
            // Every cell of this shell is at least (ring - 1) cells away (plus how far outside).
            if best <= (ring as f64 - 1.0).max(0.0) * self.cell + outside {
                break;
            }
            let lo = c.map(|v| v as i64 - ring as i64);
            let hi = c.map(|v| v as i64 + ring as i64);
            for x in lo[0].max(0)..=hi[0].min(self.dims[0] as i64 - 1) {
                for y in lo[1].max(0)..=hi[1].min(self.dims[1] as i64 - 1) {
                    for z in lo[2].max(0)..=hi[2].min(self.dims[2] as i64 - 1) {
                        let on_shell = x == lo[0] || x == hi[0] || y == lo[1] || y == hi[1] || z == lo[2] || z == hi[2];
                        if !on_shell {
                            continue;
                        }
                        for &i in &self.cells[((x as usize) * self.dims[1] + y as usize) * self.dims[2] + z as usize] {
                            let t = &tris[i as usize];
                            best = best.min((closest(p, t.p[0], t.p[1], t.p[2]) - p).length());
                        }
                    }
                }
            }
        }
        best
    }
}

/// The symmetric Chamfer distance in mm: the mean and the 95th percentile of the distances from
/// each surface's samples to the other surface.
pub fn chamfer(a: &[Tri], b: &[Tri]) -> (f64, f64) {
    if a.is_empty() || b.is_empty() {
        return (0.0, 0.0);
    }
    let (ga, gb) = (Grid::new(a), Grid::new(b));
    let mut d: Vec<f64> = surface_samples(a, SAMPLES).into_iter().map(|p| gb.distance(b, p)).collect();
    d.extend(surface_samples(b, SAMPLES).into_iter().map(|p| ga.distance(a, p)));
    if d.is_empty() {
        return (0.0, 0.0);
    }
    let mean = d.iter().sum::<f64>() / d.len() as f64 * 1000.0;
    d.sort_by(f64::total_cmp);
    let worst = d[((d.len() as f64 * 0.95) as usize).min(d.len() - 1)] * 1000.0;
    (mean, worst)
}

// --- Tiers -------------------------------------------------------------------------------------

/// The tier a translation earns (plan §8.6): `exact` (`fit=mesh`, the whole model one mesh part, that matches),
/// `fit` (good enough for the library; editable when at most 30% of its area is mesh parts),
/// `draft` (disk only), `fail` (not written).
pub fn tier(f: &Fidelity, chars: usize, whole_mesh: bool) -> &'static str {
    if whole_mesh && f.iou >= 0.98 && f.de <= 1.5 {
        "exact"
    } else if f.iou >= 0.85 && f.iou_mean >= 0.90 && f.de <= 6.0 && chars <= 6_000 {
        "fit"
    } else if f.iou >= 0.75 && f.de <= 10.0 {
        "draft"
    } else {
        "fail"
    }
}

/// Whether a `fit` translation is mostly primitives an agent can edit.
pub fn editable(mesh_pct: f64) -> bool {
    mesh_pct <= 30.0
}

/// Words of prop files that are one token each (field names and common values).
const ONE_TOKEN: &[&str] = &[
    "format", "name", "unit", "color", "type", "size", "pos", "yaw", "pitch", "roll", "parts", "points", "depth", "radius",
    "height", "profile", "segments", "mirror", "array", "count", "step", "mesh", "source", "library", "license", "title", "fit",
    "iou", "de", "box", "cylinder", "cone", "sphere", "capsule", "cm", "axis", "mean", "worst", "pct", "url", "end", "blue",
    "red", "grey", "green", "brown", "dark", "light", "white", "black", "pink", "teal", "orange", "yellow", "purple",
];

/// An estimate of the o200k tokens in a text, from its pre-tokens: runs of marks (a line break
/// joins the marks after it) and of digits a token per three characters, words a token per
/// three letters (common prop words one). Within a few percent of tiktoken on prop files; for
/// budgets, not billing.
pub fn tokens(text: &str) -> usize {
    let b = text.as_bytes();
    let (mut i, mut n) = (0, 0);
    let letter = |c: u8| c.is_ascii_alphabetic() || c >= 0x80;
    let mark = |c: u8| !c.is_ascii_alphanumeric() && !c.is_ascii_whitespace() && c < 0x80;
    while i < b.len() {
        let c = b[i];
        let s = i;
        if c.is_ascii_whitespace() {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && mark(b[i]) && b[i] != b'_' {
                // Joins the marks that follow.
                while i < b.len() && mark(b[i]) {
                    i += 1;
                }
                n += (i - s - 1).div_ceil(3).max(1);
            }
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            n += (i - s).div_ceil(3);
        } else if letter(c) || (c == b'_' && b.get(i + 1).is_some_and(|&d| letter(d))) {
            i += 1;
            while i < b.len() && letter(b[i]) {
                i += 1;
            }
            let word = &text[s..i];
            n += if ONE_TOKEN.contains(&word.trim_start_matches('_')) {
                1
            } else {
                ((word.len() as f64 / 3.0).round() as usize).max(1)
            };
        } else {
            while i < b.len() && mark(b[i]) && !(b[i] == b'_' && b.get(i + 1).is_some_and(|&d| letter(d))) {
                i += 1;
            }
            i = i.max(s + 1);
            n += (i - s).div_ceil(3);
        }
    }
    n
}

// --- The compare image -------------------------------------------------------------------------

/// The source over its translation in the six views (columns front, back, left, right, top,
/// iso), each tile framed alike for both rows, lit from over the viewer's shoulder: a PNG.
pub fn compare_png(source: &[Tri], fit: &[Tri]) -> Vec<u8> {
    let (px, gap) = (COMPARE_PX, 4);
    let views = raster::six_views();
    let (w, h) = (views.len() * px + (views.len() + 1) * gap, 2 * px + 3 * gap);
    let mut rgb = vec![236u8; w * h * 3];
    for (col, (_, view)) in views.iter().enumerate() {
        let Some(win) = square(source.iter().chain(fit), view) else { continue };
        for (row, tris) in [source, fit].into_iter().enumerate() {
            let img = raster::draw(tris, view, &win);
            let (ox, oy) = (gap + col * (px + gap), gap + row * (px + gap));
            for y in 0..px {
                for x in 0..px {
                    let i = y * px + x;
                    let o = ((oy + y) * w + ox + x) * 3;
                    let c =
                        if img.cover[i] { img.colour[i].map(|c| c * (0.45 + 0.55 * img.shade[i])) } else { [0.62, 0.62, 0.62] };
                    for k in 0..3 {
                        rgb[o + k] = super::scene::linear_to_srgb(c[k]);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w as u32, h as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().expect("png header");
        wr.write_image_data(&rgb).expect("png data");
    }
    out
}

/// A square window round the triangles (pictures keep their proportions).
fn square<'a>(tris: impl IntoIterator<Item = &'a Tri>, view: &View) -> Option<Window> {
    let win = Window::around(tris, view, 0.06, COMPARE_PX, COMPARE_PX)?;
    let side = (win.hi[0] - win.lo[0]).max(win.hi[1] - win.lo[1]);
    let mid = [(win.lo[0] + win.hi[0]) * 0.5, (win.lo[1] + win.hi[1]) * 0.5];
    Some(Window { lo: [mid[0] - side * 0.5, mid[1] - side * 0.5], hi: [mid[0] + side * 0.5, mid[1] + side * 0.5], ..win })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(c: DVec3, h: f64, colour: [f64; 3]) -> Vec<Tri> {
        let prim = pav_core::props::geom::Primitive::Box { half: glam::Vec3::splat(h as f32) };
        raster::shape_tris(&prim).into_iter().map(|p| Tri { p: p.map(|v| v + c), c: colour }).collect()
    }

    #[test]
    fn identical_shapes_match_and_shifted_ones_do_not() {
        let a = cube(DVec3::ZERO, 0.5, [0.5, 0.2, 0.1]);
        let f = compare(&a, &a);
        assert_eq!((f.iou, f.iou_mean, f.de), (1.0, 1.0, 0.0));
        assert!(f.mean_mm < 0.01, "{f:?}");
        assert_eq!(f.views.len(), 6);
        // Half a cube to the side: a third overlaps in the front view.
        let b = cube(DVec3::new(0.5, 0.0, 0.0), 0.5, [0.5, 0.2, 0.1]);
        let g = compare(&a, &b);
        assert!((g.views[0].1 - 1.0 / 3.0).abs() < 0.02, "{g:?}");
        assert!(g.mean_mm > 100.0);
        let red = cube(DVec3::ZERO, 0.5, [0.8, 0.0, 0.0]);
        assert!(compare(&a, &red).de > 10.0);
        assert!(compare_png(&a, &b).starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn token_estimate_counts_json_pieces() {
        assert_eq!(tokens(r#"{"type":"box"}"#), 5);
        assert_eq!(tokens("123456 7"), 3);
    }
}
