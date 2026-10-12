//! `asset_edit action=check`: geometry findings on a prop's resolved parts, one line each
//! (`code: ids fact`). Parts are oriented boxes (round shapes by their bounding box, contained
//! exactly); an axis-aligned sweep finds candidate pairs and a 15-axis separating-axis test
//! measures their gap. Thresholds are the ones tuned on the templates and the dogfood assets
//! (docs/ASSET_SYSTEM_PLAN.md §7.3).

use std::collections::BTreeMap;

use glam::Vec3;

use super::{PropAsset, PropPart};
use crate::shape::{Look, Shape};

/// Parts closer than this count as touching (floating, floor contact).
const JOIN: f32 = 0.005;
/// Faces closer than this count as coplanar (z-fighting).
const COPLANAR: f32 = 0.001;
/// Findings shown before `+N more`.
pub const MAX_LINES: usize = 10;

/// One finding: its code, the parts it names, and the line agents read.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub ids: Vec<String>,
    pub line: String,
}

impl Finding {
    /// What identifies a finding across revisions: a moved part keeps its finding's key.
    pub fn key(&self) -> String {
        format!("{}:{}", self.code, self.ids.join(","))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Target size in cm per axis (None skips an axis).
    pub target_cm: Option<[Option<f32>; 3]>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Boxy,
    Sphere,
    Capsule,
    Cylinder,
}

struct Solid<'a> {
    name: &'a str,
    part: &'a PropPart,
    kind: Kind,
    center: Vec3,
    axes: [Vec3; 3],
    half: Vec3,
    min: Vec3,
    max: Vec3,
}

impl<'a> Solid<'a> {
    fn new(name: &'a str, part: &'a PropPart) -> Self {
        let (kind, half) = match part.shape {
            Shape::Box { half } | Shape::RoundedBox { half, .. } => (Kind::Boxy, half),
            Shape::Sphere { radius } => (Kind::Sphere, Vec3::splat(radius)),
            Shape::Capsule { half_height, radius } => (Kind::Capsule, Vec3::new(radius, half_height + radius, radius)),
            Shape::Cylinder { half_height, radius } => (Kind::Cylinder, Vec3::new(radius, half_height, radius)),
        };
        let rotation = part.rotation();
        let axes = [rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z];
        let extent = Vec3::new(radius_on(&axes, half, Vec3::X), radius_on(&axes, half, Vec3::Y), radius_on(&axes, half, Vec3::Z));
        let extent = if kind == Kind::Sphere { half } else { extent };
        Self { name, part, kind, center: part.pos, axes, half, min: part.pos - extent, max: part.pos + extent }
    }

    fn interval(&self, n: Vec3) -> (f32, f32) {
        let c = self.center.dot(n);
        let r = radius_on(&self.axes, self.half, n);
        (c - r, c + r)
    }

    fn corners(&self) -> impl Iterator<Item = Vec3> + '_ {
        (0..8).map(move |i| {
            let s = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
            self.center
                + self.axes[0] * s(1) * self.half.x
                + self.axes[1] * s(2) * self.half.y
                + self.axes[2] * s(4) * self.half.z
        })
    }

    /// Exact containment for round shapes; the oriented box otherwise.
    fn contains(&self, q: Vec3) -> bool {
        let d = q - self.center;
        let l = Vec3::new(d.dot(self.axes[0]), d.dot(self.axes[1]), d.dot(self.axes[2]));
        match self.kind {
            Kind::Sphere => d.length() <= self.half.x,
            Kind::Capsule => {
                let r = self.half.x;
                let y = l.y.clamp(-(self.half.y - r), self.half.y - r);
                Vec3::new(l.x, l.y - y, l.z).length() <= r
            }
            Kind::Cylinder => l.x.hypot(l.z) <= self.half.x && l.y.abs() <= self.half.y,
            Kind::Boxy => l.abs().cmple(self.half).all(),
        }
    }

    fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

fn radius_on(axes: &[Vec3; 3], half: Vec3, n: Vec3) -> f32 {
    axes[0].dot(n).abs() * half.x + axes[1].dot(n).abs() * half.y + axes[2].dot(n).abs() * half.z
}

/// Signed separation along the best separating axis: positive is a gap, negative an overlap.
fn gap(a: &Solid, b: &Solid) -> f32 {
    if a.kind == Kind::Sphere && b.kind == Kind::Sphere {
        return a.center.distance(b.center) - a.half.x - b.half.x;
    }
    let mut best = f32::NEG_INFINITY;
    let mut test = |n: Vec3| {
        let length = n.length();
        if length < 1e-6 {
            return;
        }
        let n = n / length;
        let (a0, a1) = a.interval(n);
        let (b0, b1) = b.interval(n);
        best = best.max((b0 - a1).max(a0 - b1));
    };
    for axis in a.axes.iter().chain(&b.axes) {
        test(*axis);
    }
    for u in &a.axes {
        for v in &b.axes {
            test(u.cross(*v));
        }
    }
    best
}

fn names(ids: &[&str], max: usize) -> String {
    let shown = ids.iter().take(max).copied().collect::<Vec<_>>().join(", ");
    if ids.len() > max { format!("{shown} +{}", ids.len() - max) } else { shown }
}

fn cm(metres: f32) -> String {
    let cm = metres * 100.0;
    if cm.abs() >= 9.95 { format!("{}", cm.round()) } else { format!("{}", (cm * 10.0).round() / 10.0) }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Every finding for an asset, in a stable order.
pub fn lint(asset: &PropAsset, options: &Options) -> Vec<Finding> {
    let solids: Vec<Solid> = asset.parts.iter().map(|(name, part)| Solid::new(name, part)).collect();
    let n = solids.len();
    let mut out = Vec::new();
    let mut push = |code: &'static str, ids: Vec<&str>, fact: String| {
        out.push(Finding { code, ids: ids.iter().map(|s| s.to_string()).collect(), line: format!("{code}: {fact}") });
    };
    if n == 0 {
        return out;
    }
    let lo = solids.iter().map(|s| s.min).reduce(Vec3::min).unwrap();
    let hi = solids.iter().map(|s| s.max).reduce(Vec3::max).unwrap();

    // Candidate pairs: an axis-aligned sweep along x, then the separating-axis gap.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| solids[a].min.x.total_cmp(&solids[b].min.x));
    let mut gaps: BTreeMap<(usize, usize), f32> = BTreeMap::new();
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if solids[j].min.x > solids[i].max.x + JOIN {
                break;
            }
            let (a, b) = (&solids[i], &solids[j]);
            let apart = (a.min - b.max).max(b.min - a.max).max_element();
            if apart <= JOIN {
                gaps.insert((i.min(j), i.max(j)), gap(a, b));
            }
        }
    }
    let gap_of = |i: usize, j: usize| gaps.get(&(i.min(j), i.max(j))).copied();

    // Floor contact. An asset whose top is at its origin hangs from it (format 1 has no
    // sockets to say so), so the floor rules do not apply to it.
    let hangs = hi.y.abs() <= 0.02 && lo.y < -JOIN;
    let below: Vec<&str> = solids.iter().filter(|s| s.min.y < -JOIN).map(|s| s.name).collect();
    if hangs {
    } else if !below.is_empty() {
        let fact = format!("{} reach{} y={} cm", names(&below, 4), if below.len() == 1 { "es" } else { "" }, cm(lo.y));
        push("below_floor", below.clone(), fact);
    } else if lo.y > JOIN {
        push("not_grounded", vec![], format!("the lowest part is at y={} cm", cm(lo.y)));
    }

    // Floating groups: parts that do not connect to an anchor (the floor; for an asset that
    // hangs from its origin, the hook plane; for one on a wall, the wall).
    let mut parent: Vec<usize> = (0..n).collect();
    for (&(i, j), &g) in &gaps {
        if g <= JOIN {
            let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
            parent[ri] = rj;
        }
    }
    let anchored: Vec<bool> = if hangs || (lo.y > JOIN && hi.y.abs() <= 0.02) {
        solids.iter().map(|s| s.max.y >= -JOIN).collect()
    } else if lo.y <= JOIN {
        solids.iter().map(|s| s.min.y <= JOIN).collect()
    } else if (-0.01..=0.02).contains(&lo.z) {
        solids.iter().map(|s| s.min.z <= lo.z + JOIN).collect()
    } else {
        vec![false; n]
    };
    let roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, root) in roots.iter().enumerate() {
        groups.entry(*root).or_default().push(i);
    }
    if groups.len() > 1 {
        // The groups that hold an anchor; without one, the largest group stands for the object.
        let mut reference: Vec<usize> = (0..n).filter(|&i| anchored[i]).map(|i| roots[i]).collect();
        if reference.is_empty() {
            reference.extend(groups.iter().max_by_key(|(_, g)| g.len()).map(|(root, _)| *root));
        }
        let rest: Vec<usize> = (0..n).filter(|&i| reference.contains(&roots[i])).collect();
        for (root, group) in &groups {
            if reference.contains(root) {
                continue;
            }
            let nearest = group
                .iter()
                .flat_map(|&i| rest.iter().map(move |&j| (i, j)))
                .map(|(i, j)| gap_of(i, j).unwrap_or_else(|| gap(&solids[i], &solids[j])))
                .fold(f32::INFINITY, f32::min);
            let ids: Vec<&str> = group.iter().map(|&i| solids[i].name).collect();
            push("floating", ids.clone(), format!("{} ({} cm from the rest)", names(&ids, 3), cm(nearest)));
        }
    }

    // Hidden and poking parts: penetrations into box-like parts.
    for (&(i, j), &g) in &gaps {
        if g > -JOIN {
            continue;
        }
        for (a, b) in [(&solids[i], &solids[j]), (&solids[j], &solids[i])] {
            if b.kind != Kind::Boxy {
                continue;
            }
            let inside = a.corners().all(|q| {
                let d = q - b.center;
                (0..3).all(|k| d.dot(b.axes[k]).abs() <= b.half[k] - 0.001)
            });
            if inside {
                push("hidden", vec![a.name, b.name], format!("{} is inside {}", a.name, b.name));
                continue;
            }
            let k = (0..3).min_by(|&p, &q| b.half[p].total_cmp(&b.half[q])).unwrap();
            let normal = b.axes[k];
            let (b0, b1) = b.interval(normal);
            let (a0, a1) = a.interval(normal);
            if a0 < b0 - JOIN && a1 > b1 + JOIN {
                let d = a.center - b.center;
                let centred = (0..3).filter(|&m| m != k).all(|m| d.dot(b.axes[m]).abs() <= b.half[m]);
                let (small, big) = ((b0 - a0).min(a1 - b1), (b0 - a0).max(a1 - b1));
                if centred && small < 0.03 && big >= 0.03 {
                    push("poke", vec![a.name, b.name], format!("{} pokes {} cm out of {}", a.name, cm(small), b.name));
                }
            }
        }
    }

    // Z-fighting: coplanar faces facing the same way, in different surfaces, left visible.
    let faces = |s: &Solid| -> Vec<(Vec3, f32, usize)> {
        let axes: &[usize] = match s.kind {
            Kind::Boxy => &[0, 1, 2],
            Kind::Cylinder => &[1],
            _ => &[],
        };
        axes.iter()
            .flat_map(|&k| {
                [-1.0, 1.0].map(|sign| (s.axes[k] * sign, (s.center + s.axes[k] * sign * s.half[k]).dot(s.axes[k] * sign), k))
            })
            .collect()
    };
    let span = |s: &Solid, others: &[usize], axis: Vec3| {
        let c = s.center.dot(axis);
        let r: f32 = others.iter().map(|&m| s.axes[m].dot(axis).abs() * s.half[m]).sum();
        (c - r, c + r)
    };
    for (&(i, j), &g) in &gaps {
        let (a, b) = (&solids[i], &solids[j]);
        if g > JOIN || (a.part.color == b.part.color && a.part.look == b.part.look) {
            continue;
        }
        let mut hit = None;
        for (na, da, ka) in faces(a) {
            for (nb, db, kb) in faces(b) {
                // Bottoms resting on the floor are never seen.
                if na.dot(nb) < 0.999 || (da - db).abs() > COPLANAR || (na.y < -0.999 && (da + lo.y).abs() < COPLANAR) {
                    continue;
                }
                let others_a: Vec<usize> = (0..3).filter(|&m| m != ka).collect();
                let others_b: Vec<usize> = (0..3).filter(|&m| m != kb).collect();
                let in_plane = others_a.iter().map(|&m| a.axes[m]).chain(others_b.iter().map(|&m| b.axes[m]));
                let overlapping = in_plane.clone().all(|axis| {
                    let (ia, ib) = (span(a, &others_a, axis), span(b, &others_b, axis));
                    ia.1.min(ib.1) - ia.0.max(ib.0) >= 0.01
                });
                if !overlapping {
                    continue;
                }
                let (u, v) = (a.axes[others_a[0]], a.axes[others_a[1]]);
                let (iu, iv) = {
                    let (au, bu) = (span(a, &others_a, u), span(b, &others_b, u));
                    let (av, bv) = (span(a, &others_a, v), span(b, &others_b, v));
                    ((au.0.max(bu.0), au.1.min(bu.1)), (av.0.max(bv.0), av.1.min(bv.1)))
                };
                // A sample counts where both faces really overlap (a rotated face's projection is
                // larger than the face) and no third part covers it.
                let on_b_face = |q: Vec3| {
                    let d = q - b.center;
                    others_b.iter().all(|&m| d.dot(b.axes[m]).abs() <= b.half[m] + COPLANAR)
                };
                let mut open = 0;
                for fu in [0.17, 0.5, 0.83] {
                    for fv in [0.17, 0.5, 0.83] {
                        // u, v and na are a's own orthonormal axes: 2 mm in front of the face.
                        let q = u * (iu.0 + (iu.1 - iu.0) * fu) + v * (iv.0 + (iv.1 - iv.0) * fv) + na * (da + 0.002);
                        let covered = solids.iter().enumerate().any(|(m, s)| m != i && m != j && s.contains(q));
                        open += usize::from(on_b_face(q - na * 0.002) && !covered);
                    }
                }
                if open >= 3 {
                    hit = Some((na, da));
                }
            }
        }
        if let Some((normal, distance)) = hit {
            let axis = ["x", "y", "z"][(0..3).max_by(|&p, &q| normal[p].abs().total_cmp(&normal[q].abs())).unwrap()];
            push(
                "z_fight",
                vec![a.name, b.name],
                format!("{} and {} share a face ({axis}={} cm)", a.name, b.name, cm(distance.abs())),
            );
        }
    }

    // Mirror pairs by name: left/right or l/r.
    let by_name: BTreeMap<&str, &Solid> = solids.iter().map(|s| (s.name, s)).collect();
    for s in &solids {
        let words: Vec<&str> = s.name.split(['_', '-']).collect();
        let swapped: Vec<&str> = words
            .iter()
            .map(|w| match *w {
                "left" => "right",
                "l" => "r",
                "right" => "left",
                "r" => "l",
                w => w,
            })
            .collect();
        if swapped == words || !words.iter().any(|w| matches!(*w, "left" | "l")) {
            continue;
        }
        let Some(other) = by_name.get(swapped.join("_").as_str()).or_else(|| by_name.get(swapped.join("-").as_str())) else {
            continue;
        };
        let (p, q) = (s.part, other.part);
        let mut bad = Vec::new();
        if (p.pos.x + q.pos.x).abs() > 0.01 {
            bad.push(format!("x {}/{} cm", cm(p.pos.x), cm(q.pos.x)));
        }
        if (p.pos.y - q.pos.y).abs() > 0.01 || (p.pos.z - q.pos.z).abs() > 0.01 {
            bad.push("y/z differ".to_string());
        }
        if (s.half - other.half).abs().max_element() > 0.001 || s.kind != other.kind {
            bad.push("size differs".to_string());
        }
        // Compare the geometry, not the angles: each axis that matters for the shape must be the
        // mirror image of the other's (in either direction, since the shapes are symmetric).
        let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        let axes: &[usize] = match s.kind {
            Kind::Boxy => &[0, 1, 2],
            Kind::Capsule | Kind::Cylinder => &[1],
            Kind::Sphere => &[],
        };
        if axes.iter().any(|&k| other.axes[k].dot(flip(s.axes[k])).abs() < 0.9998) {
            bad.push("rotation not mirrored".to_string());
        }
        if !bad.is_empty() {
            push("mirror", vec![s.name, other.name], format!("{}/{} {}", s.name, other.name, bad.join(", ")));
        }
    }

    // Size against a target, tiny parts, the unit, the default colour.
    let size = hi - lo;
    if let Some(target) = options.target_cm {
        let mut off = Vec::new();
        for (k, goal) in target.iter().enumerate() {
            if let Some(goal) = goal.filter(|g| *g > 0.0) {
                let ratio = size[k] * 100.0 / goal - 1.0;
                // "More than 10%": exactly 10% (in f32) passes.
                if ratio.abs() > 0.1005 {
                    off.push(format!("{} {:+.0}%", ["x", "y", "z"][k], ratio * 100.0));
                }
            }
        }
        if !off.is_empty() {
            let goals = target.map(|g| g.map(|g| format!("{g}")).unwrap_or_else(|| "-".into())).join("x");
            push(
                "size",
                vec![],
                format!("{}x{}x{} cm vs target {goals}: {}", cm(size.x), cm(size.y), cm(size.z), off.join(", ")),
            );
        }
    }
    let tiny: Vec<&str> = solids.iter().filter(|s| s.size().max_element() < 0.02).map(|s| s.name).collect();
    if !tiny.is_empty() {
        push("tiny", tiny.clone(), format!("{} under 2 cm (about a pixel in game)", names(&tiny, 4)));
    }
    if size.max_element() < 0.05 || size.max_element() > 30.0 {
        push("unit_suspect", vec![], format!("{}x{}x{} cm; positions and sizes are metres", cm(size.x), cm(size.y), cm(size.z)));
    }
    let plain: Vec<&str> =
        solids.iter().filter(|s| s.part.color == "#e8704a" && s.part.look == Look::Cel).map(|s| s.name).collect();
    if !plain.is_empty() {
        push("default_color", plain.clone(), format!("{} use the default orange #e8704a", names(&plain, 4)));
    }
    out
}

/// The reply form: at most `MAX_LINES` lines, then `+N more`.
pub fn lines(findings: &[Finding]) -> Vec<String> {
    let mut lines: Vec<String> = findings.iter().take(MAX_LINES).map(|f| f.line.clone()).collect();
    if findings.len() > MAX_LINES {
        lines.push(format!("+{} more", findings.len() - MAX_LINES));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::get;

    fn codes(asset: &PropAsset, options: &Options) -> Vec<String> {
        lint(asset, options).into_iter().map(|f| f.line).collect()
    }

    fn reference() -> Vec<PropAsset> {
        let mut assets: Vec<PropAsset> =
            ["BUILTIN/box", "BUILTIN/crate", "BUILTIN/bench", "BUILTIN/lantern"].map(|n| (*get(n).unwrap()).clone()).into();
        for text in [
            include_str!("../../tests/fixtures/props/dogfood_stall_v1.json"),
            include_str!("../../tests/fixtures/props/dogfood_well_v1.json"),
        ] {
            assets.push(PropAsset::parse(text).unwrap());
        }
        assets
    }

    #[test]
    fn templates_and_the_dogfood_well_are_clean_and_the_stall_shows_its_real_faults() {
        for asset in reference() {
            let found = codes(&asset, &Options::default());
            if asset.name == "market_stall" {
                assert_eq!(
                    found,
                    [
                        "poke: crate_l pokes 2 cm out of crate_l_band",
                        "poke: crate_r pokes 2 cm out of crate_r_band",
                        "z_fight: crate_l and crate_l_rim share a face (y=138 cm)",
                        "z_fight: crate_r and crate_r_rim share a face (y=138 cm)",
                    ]
                );
            } else {
                assert!(found.is_empty(), "{}: {found:?}", asset.name);
            }
        }
    }

    #[test]
    fn seven_planted_defects_give_exactly_their_codes() {
        let bench = get("BUILTIN/bench").unwrap();
        let planted = bench
            .patch(&serde_json::json!([
                {"op":"add","part":"lamp","value":{"shape":{"type":"sphere","radius":0.08},"pos":[0,2,0],"color":"#ffd58c"}},
                {"op":"add","part":"rug","value":{"shape":{"type":"box","half":[0.6,0.01,0.3]},"pos":[0,-0.015,0.7],"color":"#7a3030"}},
                {"op":"add","part":"peg","value":{"shape":{"type":"box","half":[0.02,0.13,0.02]},"pos":[0.5,0.82,0],"color":"#202020"}},
                {"op":"add","part":"core","value":{"shape":{"type":"box","half":[0.3,0.03,0.1]},"pos":[-0.3,0.8,0],"color":"#ff0000"}},
                {"op":"add","part":"inlay","value":{"shape":{"type":"box","half":[0.3,0.005,0.1]},"pos":[0.3,0.885,0],"color":"#2a6a9a"}},
                {"op":"set","part":"leg_back_right","fields":{"pos":[0.8,0.56,-0.25]}}
            ]))
            .unwrap();
        let options = Options { target_cm: Some([Some(180.0), Some(90.0), Some(60.0)]) };
        let found = lint(&planted, &options);
        let mut seen: Vec<&str> = found.iter().map(|f| f.code).collect();
        seen.sort();
        assert_eq!(seen, ["below_floor", "floating", "hidden", "mirror", "poke", "size", "z_fight"], "{found:#?}");
        let text: Vec<&str> = found.iter().map(|f| f.line.as_str()).collect();
        assert!(text.contains(&"floating: lamp (49 cm from the rest)"), "{text:?}");
        assert!(text.contains(&"hidden: core is inside seat"), "{text:?}");
        assert!(text.contains(&"poke: peg pokes 2 cm out of seat"), "{text:?}");
        assert!(text.contains(&"z_fight: inlay and seat share a face (y=89 cm)"), "{text:?}");
    }

    #[test]
    fn hanging_and_wall_assets_anchor_at_their_origin() {
        let lantern = PropAsset::parse(
            r##"{"format":1,"name":"hanging","parts":{
            "chain":{"shape":{"type":"cylinder","radius":0.01,"half_height":0.2},"pos":[0,-0.2,0],"color":"#333333"},
            "body":{"shape":{"type":"box","half":[0.1,0.15,0.1]},"pos":[0,-0.55,0],"color":"#806040"},
            "drop":{"shape":{"type":"sphere","radius":0.03},"pos":[0,-0.9,0],"color":"#806040"}}}"##,
        )
        .unwrap();
        let found = codes(&lantern, &Options::default());
        assert_eq!(found, ["floating: drop (17 cm from the rest)"]);
    }

    #[test]
    fn a_256_part_asset_lints_quickly() {
        let mut parts = String::new();
        for i in 0..256 {
            let (x, y) = ((i % 16) as f32 * 0.2, (i / 16) as f32 * 0.1 + 0.05);
            parts += &format!(
                r##"{}"s{i}":{{"shape":{{"type":"box","half":[0.1,0.05,0.1]}},"pos":[{x},{y},0],"color":"#{:06x}"}}"##,
                if i == 0 { "" } else { "," },
                0x404040 + i * 3
            );
        }
        let asset = PropAsset::parse(&format!(r#"{{"format":1,"name":"wall","parts":{{{parts}}}}}"#)).unwrap();
        let started = std::time::Instant::now();
        let found = lint(&asset, &Options::default());
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert!(found.is_empty(), "{found:?}");
        let budget = if cfg!(debug_assertions) { 400.0 } else { 10.0 };
        assert!(elapsed < budget, "256 parts took {elapsed:.1} ms");
    }
}
