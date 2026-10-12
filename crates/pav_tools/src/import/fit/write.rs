//! Writing fitted pieces as a format-2 document (plan §8.5, steps 4-7): each primitive in the
//! cheapest of its equivalent forms (the turn with the fewest angles; axis fields for round
//! shapes on a frame axis), angles in whole degrees, centimetres to 0.1; mirror pairs in x then
//! z folded into `mirror`, three or more equal parts at an equal step into `array`; names from
//! the node or the colour and shape; the colour most of the area has at the root; thin small
//! parts (and a thin small mesh part) `solid:false` (step 9).

use std::collections::BTreeMap;

use glam::{DMat3, DVec3};
use pav_core::props::geom::Primitive;
use serde_json::{Map, Value, json};

use super::frame::{self, snap};
use super::segment::{bounds, dist};
use super::{Choice, Fitted, Options};
use crate::import::metrics::Fidelity;
use crate::import::raster::oklab;
use crate::import::scene::hex;

/// Colours this close (Oklab) are written as one.
const SAME_COLOUR: f64 = 0.02;
/// Positions this close (metres) count as mirror images or equal steps.
const NEAR: f64 = 0.002;
/// Plan §8.5 step 9: a part thinner than this (metres, its box's thinnest side in its own
/// frame)...
pub const THIN: f64 = 0.03;
/// ...whose box is under this share of the asset's box is decorative: `solid:false`, out of
/// the collider (banners, blades, leaves, trim). Thin but large parts (a table top, a floor
/// tile) stay solid.
pub const SMALL: f64 = 0.005;

/// A box's volume, each side at least a millimetre (flat models have a volume too).
fn volume(size: DVec3) -> f64 {
    size.max(DVec3::splat(0.001)).element_product()
}

/// Whether a part of box `size` stays out of the collider of an asset whose box holds `asset`
/// cubic metres.
fn decorative(size: DVec3, asset: f64) -> bool {
    size.min_element() < THIN && volume(size) < SMALL * asset
}

/// Centimetres as a JSON number: whole when that is within 3% of `scale` (metres: the extent
/// the number sizes or places) of the exact value, else to 0.1 (D1: prop-scale numbers become
/// one-token integers; small parts keep their tenths).
pub fn num(metres: f64, scale: f64) -> Value {
    let exact = metres * 100.0;
    let whole = exact.round();
    let t = if (whole - exact).abs() <= 3.0 * scale { whole } else { (exact * 10.0).round() / 10.0 };
    let t = if t == 0.0 { 0.0 } else { t };
    if t.fract() == 0.0 { json!(t as i64) } else { json!(t) }
}

/// A point or step, each axis rounded against the part's extent along it.
fn num3(v: DVec3, extent: DVec3) -> Value {
    json!([num(v.x, extent.x), num(v.y, extent.y), num(v.z, extent.z)])
}

/// A primitive written: its type and shape fields, the turn (`axis`, then yaw, pitch, roll),
/// and its centre, all in the asset's frame.
#[derive(Clone, Debug, PartialEq)]
struct Written {
    fields: Map<String, Value>,
    /// Snapped yaw, pitch, roll.
    euler: [f64; 3],
    pos: DVec3,
    /// Its extent along the asset's axes (metres), which its position rounds against.
    extent: DVec3,
}

impl Written {
    /// Fields that say what the part is apart from where it is.
    fn shape_key(&self) -> String {
        let mut f = self.fields.clone();
        f.insert("e".into(), json!(self.euler));
        Value::Object(f).to_string()
    }
}

/// How many angles a turn needs once snapped, then how far they turn: the cheapest first.
fn cost(e: &[f64; 3]) -> (usize, i64) {
    (e.iter().filter(|a| **a != 0.0).count(), e.iter().map(|a| a.abs() as i64).sum())
}

/// The 24 turns of a cube (signed permutation matrices with determinant 1).
fn cube_turns() -> Vec<DMat3> {
    let mut out = Vec::new();
    for perm in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        for signs in 0..8 {
            let cols = [0, 1, 2].map(|i| DVec3::AXES[perm[i]] * if signs >> i & 1 == 1 { -1.0 } else { 1.0 });
            let m = DMat3::from_cols(cols[0], cols[1], cols[2]);
            if m.determinant() > 0.0 {
                out.push(m);
            }
        }
    }
    out
}

/// A box-like shape (box, ellipsoid) with half size `half` in axes `w`: the equivalent axes
/// with the fewest angles.
fn boxlike(kind: &str, half: DVec3, w: DMat3, pos: DVec3) -> Written {
    let mut best: Option<(([f64; 3], DVec3), (usize, i64))> = None;
    for p in cube_turns() {
        let w2 = w * p;
        let (e, _) = frame::snapped(w2);
        let h2 = (p.transpose() * half).abs();
        let c = cost(&e);
        if best.as_ref().is_none_or(|(_, bc)| c < *bc) {
            best = Some(((e, h2), c));
        }
    }
    let ((euler, h), _) = best.expect("24 turns");
    let mut fields = Map::new();
    fields.insert("type".into(), json!(kind));
    fields.insert("size".into(), num3(h * 2.0, h * 2.0));
    Written { fields, euler, pos, extent: DVec3::ZERO }
}

/// A turn that takes +y to `d` with the fewest angles: an axis field when `d` is a frame axis,
/// else roll alone, pitch alone, or yaw and pitch.
fn toward(d: DVec3) -> (Option<&'static str>, [f64; 3]) {
    let near = |a: DVec3| d.dot(a) > (1.0f64).to_radians().cos();
    for (a, name) in
        [(DVec3::Y, ""), (DVec3::NEG_Y, "-y"), (DVec3::X, "x"), (DVec3::NEG_X, "-x"), (DVec3::Z, "z"), (DVec3::NEG_Z, "-z")]
    {
        if near(a) {
            return (if name.is_empty() { None } else { Some(name) }, [0.0; 3]);
        }
    }
    // Ry(yaw) Rx(pitch) y = (sin p sin yaw, cos p, sin p cos yaw); Rz(roll) y = (-sin r, cos r, 0).
    let small = (1.0f64).to_radians().sin();
    if d.z.abs() < small {
        return (None, [0.0, 0.0, snap((-d.x).atan2(d.y).to_degrees())]);
    }
    if d.x.abs() < small {
        return (None, [0.0, snap(d.z.atan2(d.y).to_degrees()), 0.0]);
    }
    let pitch = d.y.clamp(-1.0, 1.0).acos().to_degrees();
    let yaw = d.x.atan2(d.z).to_degrees();
    (None, [snap(yaw), snap(pitch), 0.0])
}

/// A round shape along `d` (its own +y in the asset): either way along the axis, whichever is
/// cheaper (`flip` turns the shape's own fields over).
fn round(fields: impl Fn(bool) -> Map<String, Value>, d: DVec3, pos: DVec3) -> Written {
    let mut best: Option<(Written, (usize, i64, usize))> = None;
    for flip in [false, true] {
        let (axis, euler) = toward(if flip { -d } else { d });
        let mut f = fields(flip);
        if let Some(a) = axis {
            f.insert("axis".into(), json!(a));
        }
        let c = cost(&euler);
        let c = (c.0 + axis.is_some() as usize, c.1, flip as usize);
        let w = Written { fields: f, euler, pos, extent: DVec3::ZERO };
        if best.as_ref().is_none_or(|(_, bc)| c < *bc) {
            best = Some((w, c));
        }
    }
    best.expect("two ways").0
}

/// A faceted round shape (`n` corners) in axes `w`: its turns by a corner's angle about its own
/// axis (and over, when `flip` gives the turned-over fields), whichever needs the fewest angles.
fn faceted(fields: impl Fn(bool) -> Map<String, Value>, n: u32, w: DMat3, pos: DVec3) -> Written {
    let mut best: Option<(Written, (usize, i64))> = None;
    for over in [false, true] {
        for j in 0..n {
            let spin = DMat3::from_rotation_y(std::f64::consts::TAU * j as f64 / n as f64);
            let s = if over { DMat3::from_rotation_x(std::f64::consts::PI) * spin } else { spin };
            let (e, _) = frame::snapped(w * s);
            let c = cost(&e);
            if best.as_ref().is_none_or(|(_, bc)| c < *bc) {
                best = Some((Written { fields: fields(over), euler: e, pos, extent: DVec3::ZERO }, c));
            }
        }
    }
    best.expect("n turns").0
}

/// An outline counter-clockwise from its lowest (then leftmost) corner, so equal outlines (a
/// mirror image's among them) write alike.
fn canonical_outline(p: &mut [DVec3]) {
    let n = p.len();
    let area: f64 = (0..n).map(|i| p[i].x * p[(i + 1) % n].y - p[(i + 1) % n].x * p[i].y).sum();
    if area < 0.0 {
        p.reverse();
    }
    let first = (0..n).min_by(|&a, &b| p[a].y.total_cmp(&p[b].y).then(p[a].x.total_cmp(&p[b].x))).unwrap_or(0);
    p.rotate_left(first);
}

/// An extrusion with outline `points` in axes `w`: the in-plane quarter turns and the turn
/// over, whichever needs the fewest angles.
fn extrusion(points: &[[f32; 2]], half_depth: f32, w: DMat3, pos: DVec3) -> Written {
    let mut best: Option<(Written, (usize, i64))> = None;
    for over in [false, true] {
        for quarter in 0..4 {
            let theta = quarter as f64 * std::f64::consts::FRAC_PI_2;
            let s =
                DMat3::from_rotation_z(theta) * if over { DMat3::from_rotation_y(std::f64::consts::PI) } else { DMat3::IDENTITY };
            let (e, _) = frame::snapped(w * s);
            let c = cost(&e);
            if best.as_ref().is_some_and(|(_, bc)| c >= *bc) {
                continue;
            }
            // A point q in the old axes is s⁻¹ q in the new ones.
            let inv = s.transpose();
            let mut turned: Vec<DVec3> = points.iter().map(|p| inv * DVec3::new(p[0] as f64, p[1] as f64, 0.0)).collect();
            canonical_outline(&mut turned);
            let (lo, hi) = turned.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(l, h), p| (l.min(*p), h.max(*p)));
            let size = hi - lo;
            let pts: Vec<Value> = turned.iter().map(|p| json!([num(p.x, size.x), num(p.y, size.y)])).collect();
            let mut f = Map::new();
            f.insert("type".into(), json!("extrude"));
            f.insert("points".into(), json!(pts));
            f.insert("depth".into(), num(2.0 * half_depth as f64, 2.0 * half_depth as f64));
            best = Some((Written { fields: f, euler: e, pos, extent: DVec3::ZERO }, c));
        }
    }
    best.expect("eight turns").0
}

/// A fitted primitive in the asset's frame, written its cheapest way.
fn written(prim: &Primitive, w: DMat3, pos: DVec3) -> Written {
    let mut out = written_shape(prim, w, pos);
    out.extent = prim.placed_box(glam::Vec3::ZERO, crate::import::raster::quat(w), 1.0, false).1.as_dvec3() * 2.0;
    out
}

fn written_shape(prim: &Primitive, w: DMat3, pos: DVec3) -> Written {
    let y = w.col(1);
    match prim {
        Primitive::Box { half } => boxlike("box", half.as_dvec3(), w, pos),
        Primitive::Ellipsoid { half } => boxlike("ellipsoid", half.as_dvec3(), w, pos),
        Primitive::Sphere { radius } => {
            let mut f = Map::new();
            f.insert("type".into(), json!("sphere"));
            f.insert("radius".into(), num(*radius as f64, 2.0 * *radius as f64));
            Written { fields: f, euler: [0.0; 3], pos, extent: DVec3::ZERO }
        }
        Primitive::Cylinder { half_height, radius, radius_end, segments, .. } => {
            let (h, r0, r1) = (*half_height as f64, *radius as f64, *radius_end as f64);
            let n = *segments;
            let fields = |flip: bool| {
                let (a, b) = if flip { (r1, r0) } else { (r0, r1) };
                let mut f = Map::new();
                if b == 0.0 {
                    f.insert("type".into(), json!("cone"));
                    f.insert("radius".into(), num(a, 2.0 * a));
                } else {
                    let d = 2.0 * a.max(b);
                    f.insert("type".into(), json!("cylinder"));
                    f.insert("radius".into(), num(a, d));
                    if num(a, d) != num(b, d) {
                        f.insert("radius_end".into(), num(b, d));
                    }
                }
                f.insert("height".into(), num(2.0 * h, 2.0 * h));
                if n != pav_core::props::geom::SEGMENTS {
                    f.insert("segments".into(), json!(n));
                }
                f
            };
            if n != pav_core::props::geom::SEGMENTS { faceted(fields, n, w, pos) } else { round(fields, y, pos) }
        }
        Primitive::Capsule { a, b, radius, radius_end } => {
            let (a, b, r0, r1) = (*a as f64, *b as f64, *radius as f64, *radius_end as f64);
            // Centred: the tips at ±(b + r) when a = -b.
            let fields = |flip: bool| {
                let (ra, rb) = if flip { (r1, r0) } else { (r0, r1) };
                let mut f = Map::new();
                f.insert("type".into(), json!("capsule"));
                let d = 2.0 * ra.max(rb);
                let len = b - a + r0 + r1;
                f.insert("radius".into(), num(ra, d));
                if num(ra, d) != num(rb, d) {
                    f.insert("radius_end".into(), num(rb, d));
                }
                f.insert("height".into(), num(len, len));
                f
            };
            let centre = pos + y * ((a - r0 + b + r1) * 0.5);
            round(fields, y, centre)
        }
        Primitive::Lathe { profile, segments } => {
            let fields = |flip: bool| {
                let mut pts: Vec<[f64; 2]> =
                    profile.iter().map(|p| [p[0] as f64, if flip { -p[1] as f64 } else { p[1] as f64 }]).collect();
                if flip {
                    pts.reverse();
                }
                let mut f = Map::new();
                f.insert("type".into(), json!("lathe"));
                let d = 2.0 * pts.iter().map(|p| p[0]).fold(0.0, f64::max);
                let len = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max)
                    - pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                f.insert("profile".into(), json!(pts.iter().map(|p| json!([num(p[0], d), num(p[1], len)])).collect::<Vec<_>>()));
                if *segments != pav_core::props::geom::LATHE_SEGMENTS {
                    f.insert("segments".into(), json!(segments));
                }
                f
            };
            if *segments != pav_core::props::geom::LATHE_SEGMENTS {
                faceted(fields, *segments, w, pos)
            } else {
                round(fields, y, pos)
            }
        }
        Primitive::Extrude { points, half_depth } => extrusion(points, *half_depth, w, pos),
        // The fitter makes no other shapes.
        other => boxlike("box", other.local_box().1.as_dvec3(), w, pos),
    }
}

/// The reflection of a part in the plane through the origin across `axis`, written its
/// cheapest way: M W D is a proper turn (D reflects the shape's own x, which only changes an
/// outline).
fn reflected(prim: &Primitive, w: DMat3, pos: DVec3, axis: usize) -> Written {
    let mut m = DVec3::ONE;
    m[axis] = -1.0;
    let m = DMat3::from_diagonal(m);
    let d = DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0));
    let prim2 = match prim {
        Primitive::Extrude { points, half_depth } => {
            Primitive::Extrude { points: points.iter().map(|p| [-p[0], p[1]]).collect(), half_depth: *half_depth }
        }
        p => p.clone(),
    };
    written(&prim2, m * w * d, m * pos)
}

/// A colour's name: a grey by lightness, else a hue word (brown for dark oranges and reds).
pub fn colour_name(c: [f64; 3]) -> &'static str {
    let [l, a, b] = oklab(c);
    let chroma = (a * a + b * b).sqrt();
    if chroma < 0.035 {
        return match l {
            l if l < 0.3 => "black",
            l if l < 0.5 => "dark",
            l if l < 0.75 => "grey",
            l if l < 0.92 => "light",
            _ => "white",
        };
    }
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    match hue {
        h if !(15.0..345.0).contains(&h) => "pink",
        h if h < 45.0 => {
            if l < 0.62 {
                "brown"
            } else {
                "red"
            }
        }
        h if h < 80.0 => {
            if l < 0.68 {
                "brown"
            } else {
                "orange"
            }
        }
        h if h < 115.0 => {
            if l < 0.6 {
                "olive"
            } else {
                "yellow"
            }
        }
        h if h < 165.0 => "green",
        h if h < 220.0 => "teal",
        h if h < 285.0 => "blue",
        _ => "purple",
    }
}

/// A node or material name as a part name: lowercase words joined by `_` (`woodBark` →
/// `wood_bark`), starting with a letter, at most 40 characters.
pub fn part_name(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in raw.chars() {
        if ch.is_ascii_uppercase() && prev_lower {
            out.push('_');
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.starts_with(|c: char| c.is_ascii_lowercase()) { out } else { format!("p_{out}") };
    out.chars().take(40).collect::<String>().trim_end_matches('_').to_string()
}

/// One node to write: a part, maybe folded with its mirror images and equal copies.
struct Node {
    written: Written,
    colour: String,
    area: f64,
    base: String,
    mirror: Vec<&'static str>,
    array: Option<(usize, DVec3)>,
    /// In the collider (not a thin small part).
    solid: bool,
}

/// The `fit` field: the numbers as the ledger rounds them.
pub fn fit_field(f: &Fidelity, mesh_pct: f64) -> Value {
    let r = |x: f64, q: f64| (x / q).round() * q;
    let num = |x: f64| {
        let v = (x * 1000.0).round() / 1000.0;
        if v.fract() == 0.0 { json!(v as i64) } else { json!(v) }
    };
    json!({
        "iou": num(r(f.iou, 0.01)),
        "de": num(r(f.de, 0.1)),
        "mesh_pct": num(r(mesh_pct, 1.0)),
        "mean_mm": num(r(f.mean_mm, 0.1)),
        "worst_mm": num(r(f.worst_mm, 1.0)),
    })
}

/// The document (without `fit`) for fitted pieces; `mesh` names the asset's mesh part.
pub fn document(fitted: &[Fitted], o: &Options, mesh: Option<&str>, node_names: &[String]) -> Value {
    // The asset's box, which decorative parts are small against.
    let all: Vec<_> = fitted.iter().flat_map(|f| f.piece.tris.iter().copied()).collect();
    let (lo, hi) = bounds(&all);
    let asset = volume(hi - lo);
    // Every primitive written its cheapest way, with its colour and area.
    let mut nodes: Vec<Node> = Vec::new();
    let mut shapes: Vec<(Primitive, DMat3, DVec3)> = Vec::new();
    for f in fitted {
        let Choice::Prim { frame, cand } = &f.choice else { continue };
        let w = frame.rot * cand.axes;
        let pos = frame.centre + frame.rot * cand.at;
        let written = written(&cand.prim, w, pos);
        let base = match node_names.get(f.piece.node) {
            Some(n) if !n.is_empty() => part_name(n),
            _ => format!("{}_{}", colour_name(f.piece.colour), written.fields["type"].as_str().unwrap_or("part")),
        };
        let solid = !decorative(cand.prim.local_box().1.as_dvec3() * 2.0, asset);
        nodes.push(Node {
            written,
            colour: hex(f.piece.colour),
            area: f.piece.area,
            base,
            mirror: Vec::new(),
            array: None,
            solid,
        });
        shapes.push((cand.prim.clone(), w, pos));
    }
    // Colours that look alike become one, the largest area's.
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by(|&a, &b| nodes[b].area.total_cmp(&nodes[a].area).then(a.cmp(&b)));
    let mut palette: Vec<(String, [f64; 3], f64)> = Vec::new();
    for &i in &order {
        let lab = oklab(crate::import::raster::hex_linear(&nodes[i].colour));
        match palette.iter_mut().find(|(_, l, _)| dist(*l, lab) < SAME_COLOUR) {
            Some((hex, _, area)) => {
                nodes[i].colour = hex.clone();
                *area += nodes[i].area;
            }
            None => palette.push((nodes[i].colour.clone(), lab, nodes[i].area)),
        }
    }

    // Mirror pairs in x, then in z: the part at +x (+z) stays and gains `mirror`.
    let mut alive = vec![true; nodes.len()];
    for (axis, name) in [(0usize, "x"), (2, "z")] {
        for i in 0..nodes.len() {
            if !alive[i] || nodes[i].written.pos[axis] <= NEAR {
                continue;
            }
            let (prim, w, pos) = &shapes[i];
            let image = reflected(prim, *w, *pos, axis);
            let found = (0..nodes.len()).find(|&j| {
                j != i
                    && alive[j]
                    && nodes[j].colour == nodes[i].colour
                    && nodes[j].mirror == nodes[i].mirror
                    && nodes[j].written.shape_key() == image.shape_key()
                    && (nodes[j].written.pos - image.pos).abs().max_element() <= NEAR
            });
            if let Some(j) = found {
                alive[j] = false;
                nodes[i].mirror.push(name);
                nodes[i].area += nodes[j].area;
            }
        }
    }
    // Equal parts at an equal step: an array centred on them.
    let mut groups: BTreeMap<(String, String, Vec<&'static str>), Vec<usize>> = BTreeMap::new();
    for i in (0..nodes.len()).filter(|&i| alive[i]) {
        groups.entry((nodes[i].written.shape_key(), nodes[i].colour.clone(), nodes[i].mirror.clone())).or_default().push(i);
    }
    for members in groups.values() {
        if members.len() < 3 {
            continue;
        }
        let mut sorted = members.clone();
        sorted.sort_by(|&a, &b| {
            let (p, q) = (nodes[a].written.pos, nodes[b].written.pos);
            p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)).then(p.z.total_cmp(&q.z))
        });
        let step = nodes[sorted[1]].written.pos - nodes[sorted[0]].written.pos;
        let even =
            sorted.windows(2).all(|w| (nodes[w[1]].written.pos - nodes[w[0]].written.pos - step).abs().max_element() <= NEAR);
        if !even || step.length() <= NEAR {
            continue;
        }
        let first = sorted[0];
        let centre = sorted.iter().map(|&i| nodes[i].written.pos).sum::<DVec3>() / sorted.len() as f64;
        for &j in &sorted[1..] {
            alive[j] = false;
            nodes[first].area += nodes[j].area;
        }
        nodes[first].written.pos = centre;
        nodes[first].array = Some((sorted.len(), step));
    }

    // The root colour: the one most written parts have (then the most area), which they omit.
    let mut uses: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
    for i in (0..nodes.len()).filter(|&i| alive[i]) {
        let u = uses.entry(nodes[i].colour.as_str()).or_default();
        u.0 += 1;
        u.1 += nodes[i].area;
    }
    let root =
        uses.iter().max_by(|a, b| a.1.0.cmp(&b.1.0).then(a.1.1.total_cmp(&b.1.1)).then(b.0.cmp(a.0))).map(|(c, _)| c.to_string());
    // Names by area, numbered when they repeat.
    let mut kept: Vec<usize> = (0..nodes.len()).filter(|&i| alive[i]).collect();
    kept.sort_by(|&a, &b| nodes[b].area.total_cmp(&nodes[a].area).then(a.cmp(&b)));
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut parts = Map::new();
    for &i in &kept {
        let n = &nodes[i];
        let count = seen.entry(n.base.clone()).or_default();
        *count += 1;
        let name = if *count == 1 { n.base.clone() } else { format!("{}{}", n.base, count) };
        let mut part = n.written.fields.clone();
        part.insert("pos".into(), num3(n.written.pos, n.written.extent));
        for (k, a) in ["yaw", "pitch", "roll"].iter().zip(n.written.euler) {
            if a != 0.0 {
                part.insert((*k).into(), json!(a as i64));
            }
        }
        if root.as_deref() != Some(n.colour.as_str()) {
            part.insert("color".into(), json!(n.colour));
        }
        if !n.solid {
            part.insert("solid".into(), json!(false));
        }
        match n.mirror.as_slice() {
            [] => {}
            [one] => {
                part.insert("mirror".into(), json!(one));
            }
            many => {
                part.insert("mirror".into(), json!(many));
            }
        }
        if let Some((count, step)) = n.array {
            part.insert("array".into(), json!({"count": count, "step": num3(step, n.written.extent)}));
        }
        parts.insert(name, Value::Object(part));
    }
    if let Some(mesh) = mesh {
        let name = if parts.contains_key("mesh") { "mesh_part".to_string() } else { "mesh".to_string() };
        let tris: Vec<_> =
            fitted.iter().filter(|f| matches!(f.choice, Choice::Mesh)).flat_map(|f| f.piece.tris.iter().copied()).collect();
        let (lo, hi) = bounds(&tris);
        let mut part = json!({"mesh": mesh});
        if decorative(hi - lo, asset) {
            part["solid"] = json!(false);
        }
        parts.insert(name, part);
    }
    let mut doc = Map::new();
    doc.insert("format".into(), json!(2));
    doc.insert("name".into(), json!(o.name));
    doc.insert("unit".into(), json!("cm"));
    if let Some(root) = root {
        doc.insert("color".into(), json!(root));
    }
    if !o.description.is_empty() {
        doc.insert("description".into(), json!(o.description));
    }
    if let Some(category) = &o.category {
        doc.insert("category".into(), json!(category));
    }
    if !o.tags.is_empty() {
        doc.insert("tags".into(), json!(o.tags));
    }
    let mut source = Map::new();
    for (k, v) in [
        ("library", &o.source.library),
        ("title", &o.source.title),
        ("license", &o.source.license),
        ("url", &o.source.url),
        ("norm", &o.source.norm),
    ] {
        if let Some(v) = v {
            source.insert(k.into(), json!(v));
        }
    }
    if !source.is_empty() {
        doc.insert("source".into(), Value::Object(source));
    }
    doc.insert("parts".into(), Value::Object(parts));
    Value::Object(doc)
}
