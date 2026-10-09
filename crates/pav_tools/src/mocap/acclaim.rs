//! The Acclaim skeleton and motion formats the CMU motion capture database uses: `.asf`, a
//! subject's bones, and `.amc`, a take (the bones' angles frame by frame). Forward kinematics as
//! the format defines it: each bone has a rest direction and length in world space and an axis
//! frame C (Euler angles, X then Y then Z about fixed axes); a frame's angles R turn it as
//! M = M_parent · C · R · C⁻¹, and its end sits at the parent's end + length · M · direction.
//! Lengths are in 1/0.45 inches (`:units length 0.45`).
//!
//! The 36 body points are fixed on the bones. CMU's hands have one finger bone and a thumb, so
//! the knuckle line (index, pinky) is set across the hand, toward the thumb, and the fist point
//! halfway down the fingers. A port of my-3D2dge's tools/asf-amc.mjs.

use std::collections::HashMap;

use anyhow::{Result, bail};

use super::readable::{POINTS, V};
use super::takes::{Body, Take};
use libm::{cos, sin};

const DG: f64 = std::f64::consts::PI / 180.0;

/// A 3x3 matrix, row major.
type M3 = [f64; 9];
const I3: M3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn mm(a: &M3, b: &M3) -> M3 {
    let mut o = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            o[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
        }
    }
    o
}
fn mv(m: &M3, v: V) -> V {
    [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[3] * v[0] + m[4] * v[1] + m[5] * v[2], m[6] * v[0] + m[7] * v[1] + m[8] * v[2]]
}
fn tr(m: &M3) -> M3 {
    [m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]
}
/// Rotation by x, then y, then z degrees about the fixed axes: Rz · Ry · Rx.
fn euler(x: f64, y: f64, z: f64) -> M3 {
    let (cx, sx, cy, sy, cz, sz) = (cos(x * DG), sin(x * DG), cos(y * DG), sin(y * DG), cos(z * DG), sin(z * DG));
    mm(
        &mm(&[cz, -sz, 0.0, sz, cz, 0.0, 0.0, 0.0, 1.0], &[cy, 0.0, sy, 0.0, 1.0, 0.0, -sy, 0.0, cy]),
        &[1.0, 0.0, 0.0, 0.0, cx, -sx, 0.0, sx, cx],
    )
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V) -> V {
    let l = super::readable::hypot(&a);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    mul(a, 1.0 / l)
}

struct Bone {
    dir: V,
    len: f64,
    c: M3,
    ci: M3,
    /// Which rotation each of the bone's channels drives (0 = x, 1 = y, 2 = z; others are skipped).
    dof: Vec<Option<usize>>,
    /// The parent bone (`None`: the root).
    parent: Option<usize>,
}

/// A subject's skeleton.
pub struct Skeleton {
    /// Metres per unit.
    scale: f64,
    names: Vec<String>,
    bones: Vec<Bone>,
    /// Bones with their parents before them.
    order: Vec<usize>,
    root_c: M3,
    root_ci: M3,
}

impl Skeleton {
    pub fn parse(text: &str) -> Result<Skeleton> {
        let mut scale = 0.0254 / 0.45;
        let mut section = String::new();
        let mut root_axis = [0.0; 3];
        struct Cur {
            name: String,
            dir: V,
            len: f64,
            axis: V,
            dof: Vec<String>,
        }
        let mut cur: Option<Cur> = None;
        let mut list: Vec<Cur> = Vec::new();
        let mut parent_of: Vec<(String, String)> = Vec::new();
        let nums = |w: &[&str]| -> V { std::array::from_fn(|i| w.get(i).and_then(|x| x.parse().ok()).unwrap_or(0.0)) };
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let w: Vec<&str> = line.split_whitespace().collect();
            if line.starts_with(':') {
                section = w[0].to_string();
                continue;
            }
            match section.as_str() {
                ":units" if w[0] == "length" => {
                    if let Some(v) = w.get(1).and_then(|x| x.parse::<f64>().ok()) {
                        scale = 0.0254 / v;
                    }
                }
                ":root" if w[0] == "orientation" => root_axis = nums(&w[1..]),
                ":bonedata" => match w[0] {
                    "begin" => cur = Some(Cur { name: String::new(), dir: [0.0; 3], len: 0.0, axis: [0.0; 3], dof: Vec::new() }),
                    "end" => list.extend(cur.take()),
                    "name" => {
                        if let Some(c) = cur.as_mut() {
                            c.name = w.get(1).unwrap_or(&"").to_string();
                        }
                    }
                    "direction" => {
                        if let Some(c) = cur.as_mut() {
                            c.dir = nums(&w[1..]);
                        }
                    }
                    "length" => {
                        if let Some(c) = cur.as_mut() {
                            c.len = w.get(1).and_then(|x| x.parse().ok()).unwrap_or(0.0);
                        }
                    }
                    "axis" => {
                        if let Some(c) = cur.as_mut() {
                            c.axis = nums(&w[1..]);
                        }
                    }
                    "dof" => {
                        if let Some(c) = cur.as_mut() {
                            c.dof = w[1..].iter().map(|d| d.to_lowercase()).collect();
                        }
                    }
                    _ => {}
                },
                ":hierarchy" if w[0] != "begin" && w[0] != "end" => {
                    for c in &w[1..] {
                        parent_of.push((c.to_string(), w[0].to_string()));
                    }
                }
                _ => {}
            }
        }
        if list.is_empty() {
            bail!("no bones: not an Acclaim skeleton (.asf)");
        }
        let names: Vec<String> = list.iter().map(|c| c.name.clone()).collect();
        let index = |n: &str| names.iter().position(|x| x == n);
        let parent_name = |n: &str| parent_of.iter().rev().find(|(c, _)| c == n).map(|(_, p)| p.clone());
        let bones: Vec<Bone> = list
            .iter()
            .map(|c| {
                let m = euler(c.axis[0], c.axis[1], c.axis[2]);
                Bone {
                    dir: norm(c.dir),
                    len: c.len,
                    c: m,
                    ci: tr(&m),
                    dof: c.dof.iter().map(|d| ["rx", "ry", "rz"].iter().position(|x| x == d)).collect(),
                    parent: parent_name(&c.name).and_then(|p| if p == "root" { None } else { index(&p) }),
                }
            })
            .collect();
        let mut order = Vec::new();
        for i in 0..bones.len() {
            let mut chain = vec![i];
            while let Some(p) = bones[*chain.last().unwrap()].parent {
                if order.contains(&p) || chain.contains(&p) {
                    break;
                }
                chain.push(p);
            }
            for &c in chain.iter().rev() {
                if !order.contains(&c) {
                    order.push(c);
                }
            }
        }
        let rc = euler(root_axis[0], root_axis[1], root_axis[2]);
        Ok(Skeleton { scale, names, bones, order, root_c: rc, root_ci: tr(&rc) })
    }

    fn bone(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

/// A take: each frame's values per bone (index 0 is the root, then the skeleton's bones).
pub struct Motion {
    frames: Vec<Vec<Vec<f64>>>,
}

impl Motion {
    pub fn parse(text: &str, sk: &Skeleton) -> Motion {
        let mut frames: Vec<Vec<Vec<f64>>> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(':') {
                continue;
            }
            if line.bytes().all(|b| b.is_ascii_digit()) {
                frames.push(vec![Vec::new(); sk.bones.len() + 1]);
                continue;
            }
            let Some(f) = frames.last_mut() else { continue };
            let mut w = line.split_whitespace();
            let name = w.next().unwrap_or("");
            let slot = if name == "root" { Some(0) } else { sk.bone(name).map(|b| b + 1) };
            if let Some(s) = slot {
                f[s] = w.map(|x| x.parse().unwrap_or(f64::NAN)).collect();
            }
        }
        Motion { frames }
    }
}

/// Every bone's world rotation and end point (metres) for one frame (`None`: the rest pose,
/// root at the origin).
struct Posed {
    m: Vec<M3>,
    end: Vec<V>,
    root_m: M3,
    root_end: V,
}

impl Posed {
    fn start(&self, sk: &Skeleton, b: usize) -> V {
        match sk.bones[b].parent {
            Some(p) => self.end[p],
            None => self.root_end,
        }
    }
}

fn pose(sk: &Skeleton, frame: Option<&[Vec<f64>]>) -> Posed {
    let r: Vec<f64> = frame.map(|f| f[0].clone()).filter(|r| r.len() >= 6).unwrap_or_else(|| vec![0.0; 6]);
    let root_m = mm(&mm(&sk.root_c, &euler(r[3], r[4], r[5])), &sk.root_ci);
    let root_end = mul([r[0], r[1], r[2]], sk.scale);
    let mut m = vec![I3; sk.bones.len()];
    let mut end = vec![[0.0; 3]; sk.bones.len()];
    for &n in &sk.order {
        let b = &sk.bones[n];
        let mut a = [0.0; 3];
        if let Some(v) = frame.map(|f| &f[n + 1]) {
            for (i, d) in b.dof.iter().enumerate() {
                if let (Some(d), Some(x)) = (d, v.get(i)) {
                    a[*d] = *x;
                }
            }
        }
        let (pm, pe) = match b.parent {
            Some(p) => (m[p], end[p]),
            None => (root_m, root_end),
        };
        m[n] = mm(&mm(&mm(&pm, &b.c), &euler(a[0], a[1], a[2])), &b.ci);
        end[n] = add(pe, mul(mv(&m[n], b.dir), b.len * sk.scale));
    }
    Posed { m, end, root_m, root_end }
}

/// A point fixed on a bone: that far along it (0 = its start, 1 = its end), plus an offset at
/// rest (metres, world). `None` is the root.
#[derive(Clone, Copy)]
struct On {
    bone: Option<usize>,
    t: f64,
    off: V,
}

/// A CMU subject: its skeleton and where each body point sits on it.
pub struct Subject {
    sk: Skeleton,
    on: Vec<On>,
    pub body: Body,
}

/// The bones the body points need.
const NEED: [&str; 27] = [
    "lowerback",
    "upperback",
    "thorax",
    "upperneck",
    "head", //
    "lhipjoint",
    "lfemur",
    "ltibia",
    "lfoot",
    "ltoes",
    "lclavicle",
    "lhumerus",
    "lradius",
    "lhand",
    "lfingers",
    "lthumb", //
    "rhipjoint",
    "rfemur",
    "rtibia",
    "rfoot",
    "rtoes",
    "rclavicle",
    "rhumerus",
    "rradius",
    "rhand",
    "rfingers",
    "rthumb",
];

impl Subject {
    pub fn parse(asf: &str) -> Result<Subject> {
        let sk = Skeleton::parse(asf)?;
        let missing: Vec<&str> = NEED.iter().copied().filter(|n| sk.bone(n).is_none()).collect();
        if !missing.is_empty() {
            bail!("the skeleton lacks bones: {}", missing.join(", "));
        }
        let b = |n: &str| sk.bone(n).unwrap();
        let rest = pose(&sk, None);
        // The rig's frame from the rest pose: up is +y; forward from the heel toward the toes;
        // right toward the right hip.
        let (ltoes, ltibia) = (rest.end[b("ltoes")], rest.end[b("ltibia")]);
        let fwd = norm([ltoes[0] - ltibia[0], 0.0, ltoes[2] - ltibia[2]]);
        let mut right = [fwd[2], 0.0, -fwd[0]];
        if dot(sub(rest.end[b("rhipjoint")], rest.end[b("lhipjoint")]), right) < 0.0 {
            right = mul(right, -1.0);
        }
        let on = |bone: &str, t: f64, off: V| On { bone: if bone == "root" { None } else { Some(b(bone)) }, t, off };
        let f12 = mul(fwd, 0.12);
        // Toward the thumb, across the hand.
        let across = |s: &str| {
            let (h, th) = (sk.bones[b(&format!("{s}hand"))].dir, sk.bones[b(&format!("{s}thumb"))].dir);
            norm(sub(th, mul(h, dot(th, h))))
        };
        let z = [0.0; 3];
        let mut at: HashMap<String, On> = HashMap::new();
        for (p, o) in [
            ("pelvis", on("root", 1.0, z)),
            ("spine1", on("lowerback", 1.0, z)),
            ("spine2", on("upperback", 1.0, z)),
            ("chest", on("thorax", 0.5, z)),
            ("neck", on("thorax", 1.0, z)),
            ("head", on("upperneck", 1.0, z)),
            ("headTop", on("head", 1.0, z)),
            ("faceF", on("head", 0.0, f12)),
            ("chestF", on("thorax", 0.5, f12)),
            ("pelvisF", on("root", 1.0, f12)),
        ] {
            at.insert(p.into(), o);
        }
        for (big, s) in [("L", "l"), ("R", "r")] {
            let bn = |n: &str| format!("{s}{n}");
            for (p, o) in [
                ("clav", on(&bn("clavicle"), 0.15, z)),
                ("sh", on(&bn("clavicle"), 1.0, z)),
                ("elbow", on(&bn("humerus"), 1.0, z)),
                ("wrist", on(&bn("radius"), 1.0, z)),
                ("knuck", on(&bn("hand"), 1.0, z)),
                ("index", on(&bn("hand"), 1.0, mul(across(s), 0.035))),
                ("pinky", on(&bn("hand"), 1.0, mul(across(s), -0.035))),
                ("fist", on(&bn("fingers"), 0.5, z)),
                ("hip", on(&bn("hipjoint"), 1.0, z)),
                ("knee", on(&bn("femur"), 1.0, z)),
                ("ankle", on(&bn("tibia"), 1.0, z)),
                ("ball", on(&bn("foot"), 1.0, z)),
                ("toe", on(&bn("toes"), 1.0, z)),
            ] {
                at.insert(format!("{p}{big}"), o);
            }
        }
        let on: Vec<On> = POINTS.iter().map(|p| at[*p]).collect();
        let mut subject = Subject { sk, on, body: Body { rest: Vec::new(), fwd, right } };
        subject.body.rest = subject.points(&rest);
        Ok(subject)
    }

    fn points(&self, ps: &Posed) -> Vec<V> {
        self.on
            .iter()
            .map(|o| match o.bone {
                None => add(ps.root_end, mv(&ps.root_m, o.off)),
                Some(b) => {
                    let bone = &self.sk.bones[b];
                    add(
                        add(ps.start(&self.sk, b), mul(mv(&ps.m[b], bone.dir), bone.len * self.sk.scale * o.t)),
                        mv(&ps.m[b], o.off),
                    )
                }
            })
            .collect()
    }

    /// One of the subject's takes, from its .amc text.
    pub fn take(&self, amc: &str) -> Result<SubjectTake<'_>> {
        let motion = Motion::parse(amc, &self.sk);
        if motion.frames.is_empty() {
            bail!("no frames: not an Acclaim motion (.amc)");
        }
        Ok(SubjectTake { subject: self, motion })
    }
}

pub struct SubjectTake<'a> {
    subject: &'a Subject,
    motion: Motion,
}

impl Take for SubjectTake<'_> {
    fn frames(&self) -> usize {
        self.motion.frames.len()
    }
    fn points(&self, frame: usize) -> Vec<V> {
        self.subject.points(&pose(&self.subject.sk, Some(&self.motion.frames[frame])))
    }
}
