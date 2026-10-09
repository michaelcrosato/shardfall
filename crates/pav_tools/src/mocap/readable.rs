//! The readable format's encoder: a captured body's rest measurements (`measure`), a key pose
//! from captured body points (`encode_pose`), every body point rebuilt from a key pose on the
//! captured body (`decode_pose`), and a clip fitted with as few key poses as keep it within a
//! tolerance of its capture (`fit`).
//!
//! A port of my-3D2dge's src/mocap/readable.js (its encoding half), step for step: f64
//! arithmetic in the same order, captured points stored as f32 as the reference stores them,
//! fdlibm's trigonometry (the `libm` crate, as V8 uses it), V8's `Math.hypot` and JavaScript's
//! rounding. The same capture becomes the same set in either engine, number for number.
//!
//! Body points are millimetres in the rig's frame: forward, right, up.

use pav_core::clips::{Clip, Key};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use libm::{acos, asin, atan2, cos, sin};

/// The body points of a captured frame, in order.
pub const POINTS: [&str; 36] = [
    "pelvis", "spine1", "spine2", "chest", "neck", "head", "headTop", "faceF", "chestF", "pelvisF", //
    "clavL", "shL", "elbowL", "wristL", "indexL", "knuckL", "pinkyL", "fistL", "hipL", "kneeL", "ankleL", "ballL", "toeL",
    "clavR", "shR", "elbowR", "wristR", "indexR", "knuckR", "pinkyR", "fistR", "hipR", "kneeR", "ankleR", "ballR", "toeR",
];
/// Points per frame.
pub const P: usize = POINTS.len();

pub const PELVIS: usize = 0;
pub const SPINE1: usize = 1;
pub const SPINE2: usize = 2;
pub const CHEST: usize = 3;
pub const NECK: usize = 4;
pub const HEAD: usize = 5;
pub const HEAD_TOP: usize = 6;
pub const FACE_F: usize = 7;
pub const CHEST_F: usize = 8;
pub const PELVIS_F: usize = 9;
// Parts of a side: `side(s, KNEE)`.
pub const CLAV: usize = 0;
pub const SH: usize = 1;
pub const ELBOW: usize = 2;
pub const WRIST: usize = 3;
pub const INDEX: usize = 4;
pub const KNUCK: usize = 5;
pub const PINKY: usize = 6;
pub const FIST: usize = 7;
pub const HIP: usize = 8;
pub const KNEE: usize = 9;
pub const ANKLE: usize = 10;
pub const BALL: usize = 11;
pub const TOE: usize = 12;

/// A point of one side (0 = L, 1 = R).
pub const fn side(s: usize, part: usize) -> usize {
    10 + 13 * s + part
}

/// The index of a point by name.
pub fn index(name: &str) -> Option<usize> {
    POINTS.iter().position(|p| *p == name)
}

/// L is -1, R is +1.
const SG: [f64; 2] = [-1.0, 1.0];
const DG: f64 = std::f64::consts::PI / 180.0;

pub type V = [f64; 3];
/// A frame: its forward, right and up axes, written in the parent's coordinates.
pub type Frame = [V; 3];
type Q = [f64; 4];

/// JavaScript's `Math.round`: halves round up.
pub fn js_round(x: f64) -> f64 {
    let r = x.ceil();
    if r - 0.5 > x { r - 1.0 } else { r }
}

/// `Math.hypot` as V8 computes it: scaled by the largest, the squares summed with Kahan's
/// compensation.
pub fn hypot(v: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut nan = false;
    for x in v {
        if x.is_nan() {
            nan = true;
        } else if x.abs() > max {
            max = x.abs();
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let (mut sum, mut comp) = (0.0f64, 0.0f64);
    for x in v {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Math.max` and `Math.min` (a NaN wins).
fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}
fn clamp1(x: f64) -> f64 {
    jmax(-1.0, jmin(1.0, x))
}

pub fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn mul(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn len(a: V) -> f64 {
    hypot(&a)
}
pub fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// Unit length (a zero vector stays zero).
pub fn norm(a: V) -> V {
    let l = hypot(&a);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    [a[0] / l, a[1] / l, a[2] / l]
}

fn frame_fr(right: V, fwd: V) -> Frame {
    let r = norm(right);
    let f = norm(sub(fwd, mul(r, dot(fwd, r))));
    [f, r, cross(f, r)]
}
fn frame_uf(up: V, fwd: V) -> Frame {
    let u = norm(up);
    let f = norm(sub(fwd, mul(u, dot(fwd, u))));
    [f, cross(u, f), u]
}
/// A world direction in frame `m`'s coordinates.
pub fn loc(m: &Frame, v: V) -> V {
    [dot(v, m[0]), dot(v, m[1]), dot(v, m[2])]
}
/// A direction in frame `m`'s coordinates, in the world.
pub fn wld(m: &Frame, l: V) -> V {
    [
        m[0][0] * l[0] + m[1][0] * l[1] + m[2][0] * l[2],
        m[0][1] * l[0] + m[1][1] * l[1] + m[2][1] * l[2],
        m[0][2] * l[0] + m[1][2] * l[1] + m[2][2] * l[2],
    ]
}
/// A child frame in its parent's coordinates.
fn rel(pm: &Frame, cm: &Frame) -> Frame {
    [loc(pm, cm[0]), loc(pm, cm[1]), loc(pm, cm[2])]
}
/// A parent frame, then one relative to it: the child in the grandparent's coordinates.
fn compose(pm: &Frame, rm: &Frame) -> Frame {
    [wld(pm, rm[0]), wld(pm, rm[1]), wld(pm, rm[2])]
}

/// A frame's rotation as a quaternion [x, y, z, w].
fn quat_of(m: &Frame) -> Q {
    let [m00, m10, m20] = m[0];
    let [m01, m11, m21] = m[1];
    let [m02, m12, m22] = m[2];
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, s / 4.0]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [s / 4.0, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, s / 4.0, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, s / 4.0, (m10 - m01) / s]
    };
    let l = hypot(&q);
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}
fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qconj(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
fn qrot(q: Q, v: V) -> V {
    let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), qconj(q));
    [p[0], p[1], p[2]]
}
fn qslerp(a: Q, b: Q, t: f64) -> Q {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let mut bb = b;
    if d < 0.0 {
        d = -d;
        bb = [-b[0], -b[1], -b[2], -b[3]];
    }
    if d > 0.9995 {
        let o: Q = std::array::from_fn(|i| a[i] + (bb[i] - a[i]) * t);
        let l = hypot(&o);
        return [o[0] / l, o[1] / l, o[2] / l, o[3] / l];
    }
    let th = acos(d);
    let k0 = sin((1.0 - t) * th) / sin(th);
    let k1 = sin(t * th) / sin(th);
    std::array::from_fn(|i| a[i] * k0 + bb[i] * k1)
}
/// The rotation that takes rest frame `a` to frame `b`.
fn q_from_to(a: &Frame, b: &Frame) -> Q {
    qmul(quat_of(b), qconj(quat_of(a)))
}

/// Degrees [turn, lean, tilt] of a frame: yaw about up, then pitch about right (+ = forward),
/// then roll (+ = toward the right).
fn euler(m: &Frame) -> V {
    let (f, u) = (m[0], m[2]);
    let yaw = atan2(f[1], f[0]);
    let pitch = asin(clamp1(-f[2]));
    let (cy, sy) = (cos(yaw), sin(yaw));
    let u1 = [u[0] * cy + u[1] * sy, -u[0] * sy + u[1] * cy, u[2]];
    let (cp, sp) = (cos(pitch), sin(pitch));
    let u2 = [u1[0] * cp - u1[2] * sp, u1[1], u1[0] * sp + u1[2] * cp];
    [yaw / DG, pitch / DG, -atan2(-u2[1], u2[2]) / DG]
}
fn from_euler(a: V) -> Frame {
    let (y, p, r) = (a[0] * DG, a[1] * DG, -a[2] * DG);
    let (cy, sy, cp, sp, cr, sr) = (cos(y), sin(y), cos(p), sin(p), cos(r), sin(r));
    let rot = |v: V| -> V {
        let [x, yy, z] = v;
        let (yy, z) = (yy * cr - z * sr, yy * sr + z * cr);
        let (x, z) = (x * cp + z * sp, -x * sp + z * cp);
        [x * cy - yy * sy, x * sy + yy * cy, z]
    };
    [rot([1.0, 0.0, 0.0]), rot([0.0, 1.0, 0.0]), rot([0.0, 0.0, 1.0])]
}

/// Two-bone IK: the middle joint and the end, the joint bending toward `hint`.
fn ik3(a: V, b: V, l1: f64, l2: f64, hint: V) -> (V, V) {
    let mut d = sub(b, a);
    let mut b = b;
    let mut dist = len(d);
    let max = l1 + l2 - 0.01;
    if dist > max {
        d = mul(d, max / dist);
        b = add(a, d);
        dist = max;
    }
    dist = jmax(dist, 0.01);
    let n = mul(d, 1.0 / dist);
    let h = sub(hint, mul(n, dot(hint, n)));
    let hl = len(h);
    let h = if hl < 1e-4 { [n[1], -n[0], 0.0] } else { mul(h, 1.0 / hl) };
    let x = (l1 * l1 - l2 * l2 + dist * dist) / (2.0 * dist);
    let y = jmax(0.0, l1 * l1 - x * x).sqrt();
    (add(add(a, mul(n, x)), mul(h, y)), b)
}
/// The bend at a limb's middle joint (degrees, 0 = straight) for a root-to-end distance.
fn bend_of(dist: f64, l1: f64, l2: f64) -> f64 {
    180.0 - acos(clamp1((l1 * l1 + l2 * l2 - dist * dist) / (2.0 * l1 * l2))) / DG
}
/// The root-to-end distance of a limb bent `bend` degrees.
fn reach_of(bend: f64, l1: f64, l2: f64) -> f64 {
    jmax(0.0, l1 * l1 + l2 * l2 + 2.0 * l1 * l2 * cos(bend * DG)).sqrt()
}
/// An angle carried round by whole turns to continue from `prev`.
fn unwrap(v: f64, prev: Option<f64>) -> f64 {
    match prev {
        None => v,
        Some(p) => v + 360.0 * js_round((p - v) / 360.0),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Limb {
    Arm,
    Leg,
}
// A twist is measured round the limb from the way its middle joint naturally bends: known for
// one limb direction (REST) and carried to the actual direction by the shortest turn.
fn rest_dir(limb: Limb, s: f64) -> V {
    match limb {
        Limb::Arm => [0.5, s * 0.5, -0.7],
        Limb::Leg => [0.3, 0.0, -1.0],
    }
}
fn bend_dir(limb: Limb, s: f64) -> V {
    match limb {
        Limb::Arm => [-1.0, s * 0.5, -0.3],
        Limb::Leg => [1.0, s * 0.1, 0.0],
    }
}
fn across(h: V, d: V) -> V {
    norm(sub(h, mul(d, dot(h, d))))
}
fn hint_for(limb: Limb, s: f64, m: &Frame, d_local: V) -> V {
    let d = norm(d_local);
    let r = norm(rest_dir(limb, s));
    let n0 = across(norm(bend_dir(limb, s)), r);
    let ax = cross(r, d);
    let sn = len(ax);
    let cs = dot(r, d);
    let mut n = n0;
    if sn > 1e-9 {
        let k = mul(ax, 1.0 / sn);
        let a = atan2(sn, cs);
        let (c, si) = (cos(a), sin(a));
        n = add(add(mul(n0, c), mul(cross(k, n0), si)), mul(k, dot(k, n0) * (1.0 - c)));
    }
    wld(m, across(n, d))
}
/// The signed angle (degrees) round axis `d` from hint `h` to the bend direction `b`.
fn pole_of(d: V, h: V, b: V) -> f64 {
    let hn = norm(sub(h, mul(d, dot(h, d))));
    let bn = norm(sub(b, mul(d, dot(b, d))));
    atan2(dot(cross(hn, bn), d), dot(hn, bn)) / DG
}
fn pole_hint(d: V, h: V, deg: f64) -> V {
    let hn = norm(sub(h, mul(d, dot(h, d))));
    let a = deg * DG;
    add(mul(hn, cos(a)), mul(cross(d, hn), sin(a)))
}

/// A captured clip: the 36 body points at a fixed rate.
#[derive(Clone, Debug, Default)]
pub struct Cap {
    pub name: String,
    pub n: usize,
    pub dur: f64,
    pub looping: bool,
    pub fps: f64,
    /// n frames × 36 points × (forward, right, up), millimetres.
    pub data: Vec<f32>,
    /// How far the root has carried the body at each frame, n × (forward, right) mm; `None`
    /// when it stays put.
    pub travel: Option<Vec<f32>>,
    /// The recording and the stretch of it, for clips cut from a capture database.
    pub take: Option<String>,
    /// How far a loop's cycle carried the hips (mm) before it was played in place.
    pub stride: Option<f64>,
}

fn pt(x: &[f32], i: usize) -> V {
    [x[i * 3] as f64, x[i * 3 + 1] as f64, x[i * 3 + 2] as f64]
}

/// The body points at time `t` (frames in-betweened; loops wrap).
pub fn cap_sample(c: &Cap, t: f64, out: &mut [f32]) {
    let n = c.n;
    let u = jmax(0.0, t) * c.fps;
    let f = if c.looping { u % (n.saturating_sub(1).max(1) as f64) } else { jmin(u, (n - 1) as f64) };
    let i0 = f.floor();
    let k = f - i0;
    let i0 = i0 as usize;
    let (a, b) = (i0 * P * 3, (n - 1).min(i0 + 1) * P * 3);
    let d = &c.data;
    for i in 0..P * 3 {
        out[i] = (d[a + i] as f64 + (d[b + i] as f64 - d[a + i] as f64) * k) as f32;
    }
}

/// Where the points errors are measured on start in a frame: the finger helpers (index, pinky)
/// only aim a blade.
const SHOWN: [usize; P - 4] = {
    let mut out = [0; P - 4];
    let (mut i, mut j) = (0, 0);
    while i < P {
        if i != side(0, INDEX) && i != side(0, PINKY) && i != side(1, INDEX) && i != side(1, PINKY) {
            out[j] = i * 3;
            j += 1;
        }
        i += 1;
    }
    out
};

/// The largest distance between matching body points of two poses (mm).
pub fn error(a: &[f32], b: &[f32]) -> f64 {
    let mut w = 0.0f64;
    for i in SHOWN {
        let d = |j: usize| a[i + j] as f64 - b[i + j] as f64;
        w = jmax(w, d(0) * d(0) + d(1) * d(1) + d(2) * d(2));
    }
    w.sqrt()
}
/// The average distance between matching body points (mm).
pub fn mean_error(a: &[f32], b: &[f32]) -> f64 {
    let mut s = 0.0;
    for i in SHOWN {
        let d = |j: usize| a[i + j] as f64 - b[i + j] as f64;
        s += hypot(&[d(0), d(1), d(2)]);
    }
    s / SHOWN.len() as f64
}

/// Key frames (Ramer-Douglas-Peucker over whole poses): in-betweening the rest stays within
/// `tol` mm of the capture.
pub fn reduce(c: &Cap, tol: f64) -> Vec<usize> {
    let (d, n) = (&c.data, c.n);
    let mut keep = vec![0, n - 1];
    let mut tmp = vec![0f32; P * 3];
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if n < 2 || b - a < 2 {
            continue;
        }
        let (mut wi, mut we) = (None, 0.0);
        for i in a + 1..b {
            let t = (i - a) as f64 / (b - a) as f64;
            for j in 0..P * 3 {
                let (x, y) = (d[a * P * 3 + j] as f64, d[b * P * 3 + j] as f64);
                tmp[j] = (x + (y - x) * t) as f32;
            }
            let e = error(&tmp, &d[i * P * 3..(i + 1) * P * 3]);
            if e > we {
                we = e;
                wi = Some(i);
            }
        }
        if let Some(wi) = wi.filter(|_| we > tol) {
            keep.push(wi);
            stack.push((a, wi));
            stack.push((wi, b));
        }
    }
    keep.sort_unstable();
    keep.dedup();
    keep
}

/// Left and right values.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sides {
    #[serde(rename = "L")]
    pub l: V,
    #[serde(rename = "R")]
    pub r: V,
}
impl Sides {
    fn get(&self, s: usize) -> V {
        if s == 0 { self.l } else { self.r }
    }
}

/// A body's rest measurements (mm), from its own captured clips: proportions, rest offsets, and
/// how much of the hips-to-chest turn each spine bone takes (and the neck between chest and
/// head). Saved in the set file (`sources.<id>.rest`) under the reference's names.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rest {
    #[serde(rename = "H0")]
    pub h0: f64,
    #[serde(rename = "hipZ")]
    pub hip_z: f64,
    #[serde(rename = "ankleZ")]
    pub ankle_z: f64,
    #[serde(rename = "Pr")]
    pub pr: Frame,
    #[serde(rename = "Cr")]
    pub cr: Frame,
    #[serde(rename = "Hr")]
    pub hr: Frame,
    pub hip: Sides,
    pub spine: [V; 4],
    #[serde(rename = "neckSeg")]
    pub neck_seg: V,
    pub clav: Sides,
    /// The collarbone: pivot to shoulder.
    pub cv: Sides,
    #[serde(rename = "topH")]
    pub top_h: V,
    #[serde(rename = "faceH")]
    pub face_h: V,
    pub up: f64,
    pub fore: f64,
    pub knuck: f64,
    pub fist: f64,
    pub thigh: f64,
    pub shin: f64,
    pub ab: f64,
    pub bt: f64,
    #[serde(rename = "abPitch")]
    pub ab_pitch: f64,
    #[serde(rename = "btPitch")]
    pub bt_pitch: f64,
    #[serde(rename = "spineW")]
    pub spine_w: [f64; 4],
    #[serde(rename = "neckW")]
    pub neck_w: f64,
}

fn rnd(v: f64, k: f64) -> f64 {
    js_round(v * k) / k
}
fn rnd3(v: V, k: f64) -> V {
    [rnd(v[0], k), rnd(v[1], k), rnd(v[2], k)]
}

impl Rest {
    /// Proportions and rest offsets from the rest clip (`rest`, else the first clip's first
    /// frame), and the spine's and neck's shares of the turn, fitted from frames of every clip.
    pub fn measure(clips: &[Cap], rest: Option<&str>) -> Rest {
        let first = rest.and_then(|n| clips.iter().find(|c| c.name == n)).unwrap_or(&clips[0]);
        let mut tp = vec![0f32; P * 3];
        cap_sample(first, 0.0, &mut tp);
        let q = |i: usize| pt(&tp, i);
        let d = |a: usize, b: usize| len(sub(q(a), q(b)));
        let (l, r) = (0, 1);
        let pr = frame_fr(sub(q(side(r, HIP)), q(side(l, HIP))), sub(q(PELVIS_F), q(PELVIS)));
        let cr = frame_uf(sub(q(NECK), q(CHEST)), sub(q(CHEST_F), q(CHEST)));
        let hr = frame_uf(sub(q(HEAD_TOP), q(HEAD)), sub(q(FACE_F), q(HEAD)));
        let sides = |f: &dyn Fn(usize) -> V| Sides { l: f(l), r: f(r) };
        let (ankle, ball, toe) = (side(l, ANKLE), side(l, BALL), side(l, TOE));
        let mut m = Rest {
            h0: q(PELVIS)[2],
            hip_z: (q(side(l, HIP))[2] + q(side(r, HIP))[2]) / 2.0,
            ankle_z: q(side(l, ANKLE))[2],
            pr,
            cr,
            hr,
            hip: sides(&|s| loc(&pr, sub(q(side(s, HIP)), q(PELVIS)))),
            spine: [sub(q(SPINE1), q(PELVIS)), sub(q(SPINE2), q(SPINE1)), sub(q(CHEST), q(SPINE2)), sub(q(NECK), q(CHEST))],
            neck_seg: sub(q(HEAD), q(NECK)),
            clav: sides(&|s| loc(&cr, sub(q(side(s, CLAV)), q(NECK)))),
            cv: sides(&|s| loc(&cr, sub(q(side(s, SH)), q(side(s, CLAV))))),
            top_h: loc(&hr, sub(q(HEAD_TOP), q(HEAD))),
            face_h: loc(&hr, sub(q(FACE_F), q(HEAD))),
            up: d(side(l, SH), side(l, ELBOW)),
            fore: d(side(l, ELBOW), side(l, WRIST)),
            knuck: d(side(l, KNUCK), side(l, WRIST)),
            fist: d(side(l, FIST), side(l, WRIST)),
            thigh: d(side(l, HIP), side(l, KNEE)),
            shin: d(side(l, KNEE), side(l, ANKLE)),
            ab: d(ankle, ball),
            bt: d(ball, toe),
            ab_pitch: asin((q(ankle)[2] - q(ball)[2]) / d(ankle, ball)) / DG,
            bt_pitch: asin((q(ball)[2] - q(toe)[2]) / d(ball, toe)) / DG,
            spine_w: [0.0; 4],
            neck_w: 0.0,
        };
        // In motion each spine bone turns part of the way from the hips' rotation to the chest's;
        // that share differs per library, so it is fitted from the clips themselves.
        struct Sample {
            x: Vec<f32>,
            q: [Q; 3],
        }
        let mut frames = Vec::new();
        for c in clips {
            let step = (c.n / 6).max(1);
            let mut f = 0;
            while f < c.n {
                let mut x = vec![0f32; P * 3];
                cap_sample(c, f as f64 / c.fps, &mut x);
                let p = |i: usize| pt(&x, i);
                let fp = frame_fr(sub(p(side(r, HIP)), p(side(l, HIP))), sub(p(PELVIS_F), p(PELVIS)));
                let fc = frame_uf(sub(p(NECK), p(CHEST)), sub(p(CHEST_F), p(CHEST)));
                let fh = frame_uf(sub(p(HEAD_TOP), p(HEAD)), sub(p(FACE_F), p(HEAD)));
                let q = [q_from_to(&pr, &fp), q_from_to(&cr, &fc), q_from_to(&hr, &fh)];
                frames.push(Sample { x, q });
                f += step;
            }
        }
        let fit_w = |a: usize, b: usize, from: usize, to: usize, rest: V| -> f64 {
            let (mut best, mut be) = (0.0, f64::INFINITY);
            let mut w = 0.0;
            while w <= 1.0001 {
                let mut e = 0.0;
                for s in &frames {
                    e += len(sub(qrot(qslerp(s.q[from], s.q[to], w), rest), sub(pt(&s.x, b), pt(&s.x, a))));
                }
                if e < be {
                    be = e;
                    best = w;
                }
                w += 0.05;
            }
            js_round(best * 100.0) / 100.0
        };
        m.spine_w = [
            fit_w(PELVIS, SPINE1, 0, 1, m.spine[0]),
            fit_w(SPINE1, SPINE2, 0, 1, m.spine[1]),
            fit_w(SPINE2, CHEST, 0, 1, m.spine[2]),
            1.0,
        ];
        m.neck_w = fit_w(NECK, HEAD, 1, 2, m.neck_seg);
        // Plain, rounded data: it is saved in the set file.
        for v in [
            &mut m.h0,
            &mut m.hip_z,
            &mut m.ankle_z,
            &mut m.up,
            &mut m.fore,
            &mut m.knuck,
            &mut m.fist,
            &mut m.thigh,
            &mut m.shin,
            &mut m.ab,
            &mut m.bt,
            &mut m.ab_pitch,
            &mut m.bt_pitch,
        ] {
            *v = rnd(*v, 100.0);
        }
        for f in [&mut m.pr, &mut m.cr, &mut m.hr] {
            *f = [rnd3(f[0], 1e5), rnd3(f[1], 1e5), rnd3(f[2], 1e5)];
        }
        for v in m.spine.iter_mut().chain([&mut m.neck_seg, &mut m.top_h, &mut m.face_h]) {
            *v = rnd3(*v, 10.0);
        }
        for s in [&mut m.hip, &mut m.clav, &mut m.cv] {
            *s = Sides { l: rnd3(s.l, 10.0), r: rnd3(s.r, 10.0) };
        }
        m
    }

    /// The rest as set-file data (whole numbers written without a decimal point).
    pub fn to_value(&self) -> Value {
        tidy(serde_json::to_value(self).unwrap_or(Value::Null))
    }

    fn lower(&self) -> f64 {
        self.fore + self.fist
    }
}

/// Whole-valued numbers as integers, the way JavaScript writes them.
pub fn tidy(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 && !n.is_i64() && !n.is_u64() => Value::from(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(tidy).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, tidy(v))).collect()),
        v => v,
    }
}

/// A key pose in f64 (the encoder's working form of `pav_core::clips::Key`).
#[derive(Clone, Debug, Default)]
pub struct K {
    pub t: f64,
    pub hips: V,
    pub body: V,
    pub chest: V,
    pub head: V,
    pub sh: [Option<[f64; 2]>; 2],
    pub arm: [[f64; 5]; 2],
    pub leg: [[f64; 5]; 2],
    pub foot: [Option<V>; 2],
    pub blade: Option<V>,
    pub root: Option<[f64; 2]>,
}

fn f32s<const N: usize>(a: [f64; N]) -> [f32; N] {
    a.map(|v| v as f32)
}
fn f64s<const N: usize>(a: [f32; N]) -> [f64; N] {
    a.map(|v| v as f64)
}

impl K {
    pub fn to_key(&self) -> Key {
        Key {
            t: self.t as f32,
            hips: f32s(self.hips),
            body: f32s(self.body),
            chest: f32s(self.chest),
            head: f32s(self.head),
            sh_l: self.sh[0].map(f32s),
            sh_r: self.sh[1].map(f32s),
            arm_l: f32s(self.arm[0]),
            arm_r: f32s(self.arm[1]),
            leg_l: f32s(self.leg[0]),
            leg_r: f32s(self.leg[1]),
            foot_l: self.foot[0].map(f32s),
            foot_r: self.foot[1].map(f32s),
            blade: self.blade.map(f32s),
            root: self.root.map(f32s),
        }
    }
    pub fn from_key(k: &Key) -> K {
        K {
            t: k.t as f64,
            hips: f64s(k.hips),
            body: f64s(k.body),
            chest: f64s(k.chest),
            head: f64s(k.head),
            sh: [k.sh_l.map(f64s), k.sh_r.map(f64s)],
            arm: [f64s(k.arm_l), f64s(k.arm_r)],
            leg: [f64s(k.leg_l), f64s(k.leg_r)],
            foot: [k.foot_l.map(f64s), k.foot_r.map(f64s)],
            blade: k.blade.map(f64s),
            root: k.root.map(f64s),
        }
    }
}

/// The torso and head frames and the joints placed from them (shared by encoding and decoding,
/// so rounding never drifts).
struct Torso {
    p: V,
    rp: Frame,
    rc: Frame,
    rh: Frame,
    spine1: V,
    spine2: V,
    chest: V,
    neck: V,
    head: V,
}

impl Torso {
    fn new(r: &Rest, k: &K) -> Torso {
        let p = mul(k.hips, r.h0 / 100.0);
        let rp = from_euler(k.body);
        let rc = compose(&rp, &from_euler(k.chest));
        let rh = compose(&rc, &from_euler(k.head));
        let (qp, qc, qh) = (q_from_to(&r.pr, &rp), q_from_to(&r.cr, &rc), q_from_to(&r.hr, &rh));
        let mut sp = [p; 5];
        for i in 0..4 {
            sp[i + 1] = add(sp[i], qrot(qslerp(qp, qc, r.spine_w[i]), r.spine[i]));
        }
        let neck = sp[4];
        let head = add(neck, qrot(qslerp(qc, qh, r.neck_w), r.neck_seg));
        Torso { p, rp, rc, rh, spine1: sp[1], spine2: sp[2], chest: sp[3], neck, head }
    }
    fn sh(&self, r: &Rest, k: &K, s: usize) -> V {
        let sv = k.sh[s].unwrap_or([0.0, 0.0]);
        let v0 = r.cv.get(s);
        let l = len(v0);
        let az = atan2(v0[0], v0[1] * SG[s]) + sv[0] * DG;
        let el = asin(v0[2] / l) + sv[1] * DG;
        add(
            add(self.neck, wld(&self.rc, r.clav.get(s))),
            wld(&self.rc, [l * cos(el) * sin(az), SG[s] * l * cos(el) * cos(az), l * sin(el)]),
        )
    }
    fn hip(&self, r: &Rest, s: usize) -> V {
        add(self.p, wld(&self.rp, r.hip.get(s)))
    }
}

fn limb_end(root: V, m: &Frame, v: V, s: usize, l: f64) -> V {
    add(root, wld(m, [v[0] * l, v[1] * SG[s] * l, v[2] * l]))
}

fn uw(a: V, b: Option<V>) -> V {
    std::array::from_fn(|i| js_round(unwrap(a[i], b.map(|b| b[i]))))
}
/// A turn, lean and tilt has a second reading (turn + 180, 180 - lean, tilt + 180): take
/// whichever continues from the key before, so a body leaning past horizontal keeps leaning.
fn ue(e: V, b: Option<V>) -> V {
    let a1 = uw(e, b);
    let Some(b) = b else { return a1 };
    let a2 = uw([e[0] + 180.0, 180.0 - e[1], e[2] + 180.0], Some(b));
    let d = |a: V| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs();
    if d(a2) < d(a1) { a2 } else { a1 }
}

/// One key pose from captured body points (`prev`: the key before, so angles continue without
/// jumps).
pub fn encode_pose(r: &Rest, x: &[f32], prev: Option<&K>) -> K {
    let p = |i: usize| pt(x, i);
    let mut k = K { hips: p(PELVIS).map(|v| js_round(v / r.h0 * 100.0)), ..Default::default() };
    let rpx = frame_fr(sub(p(side(1, HIP)), p(side(0, HIP))), sub(p(PELVIS_F), p(PELVIS)));
    let rcx = frame_uf(sub(p(NECK), p(CHEST)), sub(p(CHEST_F), p(CHEST)));
    let rhx = frame_uf(sub(p(HEAD_TOP), p(HEAD)), sub(p(FACE_F), p(HEAD)));
    k.body = ue(euler(&rpx), prev.map(|q| q.body));
    k.chest = ue(euler(&rel(&from_euler(k.body), &rcx)), prev.map(|q| q.chest));
    let rc = compose(&from_euler(k.body), &from_euler(k.chest));
    k.head = ue(euler(&rel(&rc, &rhx)), prev.map(|q| q.head));
    for (s, &sg) in SG.iter().enumerate() {
        let v = loc(&rc, sub(p(side(s, SH)), p(side(s, CLAV))));
        let v0 = r.cv.get(s);
        let az = |a: V| atan2(a[0], a[1] * sg) / DG;
        let el = |a: V| asin(clamp1(a[2] / len(a))) / DG;
        k.sh[s] = Some([js_round(az(v) - az(v0)), js_round(el(v) - el(v0))]);
    }
    let tq = Torso::new(r, &k);
    for (s, &sg) in SG.iter().enumerate() {
        for limb in [Limb::Arm, Limb::Leg] {
            let (root, m, end, mid, l1, l2) = match limb {
                Limb::Arm => (tq.sh(r, &k, s), tq.rc, FIST, ELBOW, r.up, r.lower()),
                Limb::Leg => (tq.hip(r, s), tq.rp, ANKLE, KNEE, r.thigh, r.shin),
            };
            let mut v = loc(&m, sub(p(side(s, end)), root));
            // A fist curled in past the shoulder is closer than a straight hand can fold: point
            // the fully bent limb away from the elbow instead, so the elbow stays where it was.
            if len(v) < (l1 - l2).abs() + 10.0 {
                v = mul(norm(loc(&m, sub(p(side(s, mid)), root))), -(l1 - l2).abs());
            }
            let dir = norm([v[0], v[1] * sg, v[2]]).map(|x| js_round(x * 100.0));
            let bend = js_round(bend_of(len(v), l1, l2));
            let target = limb_end(root, &m, norm(dir), s, reach_of(bend, l1, l2));
            let dd = norm(sub(target, root));
            let tw = pole_of(dd, hint_for(limb, sg, &m, [dir[0], dir[1] * sg, dir[2]]), sub(p(side(s, mid)), root));
            let prev_t = prev.map(|q| if limb == Limb::Arm { q.arm[s][4] } else { q.leg[s][4] });
            let out = [dir[0], dir[1], dir[2], bend, js_round(unwrap(tw, prev_t))];
            match limb {
                Limb::Arm => k.arm[s] = out,
                Limb::Leg => k.leg[s] = out,
            }
        }
        let fd = loc(&tq.rp, norm(sub(p(side(s, BALL)), p(side(s, ANKLE)))));
        let td = loc(&tq.rp, norm(sub(p(side(s, TOE)), p(side(s, BALL)))));
        let pf = prev.and_then(|q| q.foot[s]);
        let down = asin(clamp1(-fd[2])) / DG;
        k.foot[s] = Some([
            js_round(down - r.ab_pitch),
            js_round(unwrap(atan2(fd[1] * sg, fd[0]) / DG, pf.map(|f| f[1]))),
            js_round(down - asin(clamp1(-td[2])) / DG - (r.ab_pitch - r.bt_pitch)),
        ]);
    }
    k
}

/// Every body point rebuilt from one key pose, on the rest body's proportions.
pub fn decode_pose(r: &Rest, k: &K, out: &mut [f32]) {
    let tq = Torso::new(r, k);
    let mut set = |i: usize, v: V| {
        out[i * 3] = v[0] as f32;
        out[i * 3 + 1] = v[1] as f32;
        out[i * 3 + 2] = v[2] as f32;
    };
    let (p, rp, rc, rh, neck, head) = (tq.p, tq.rp, tq.rc, tq.rh, tq.neck, tq.head);
    set(PELVIS, p);
    set(PELVIS_F, add(p, mul(rp[0], 120.0)));
    set(SPINE1, tq.spine1);
    set(SPINE2, tq.spine2);
    set(CHEST, tq.chest);
    set(CHEST_F, add(tq.chest, mul(rc[0], 120.0)));
    set(NECK, neck);
    set(HEAD, head);
    set(HEAD_TOP, add(head, wld(&rh, r.top_h)));
    set(FACE_F, add(head, wld(&rh, r.face_h)));
    let blade = k.blade.map(|b| norm(wld(&rc, b)));
    for (s, &sg) in SG.iter().enumerate() {
        let sh = tq.sh(r, k, s);
        let a = k.arm[s];
        let an = norm([a[0], a[1], a[2]]);
        let ft = limb_end(sh, &rc, an, s, reach_of(a[3], r.up, r.lower()));
        let ad = norm(sub(ft, sh));
        let (el, fist) =
            ik3(sh, ft, r.up, r.lower(), pole_hint(ad, hint_for(Limb::Arm, sg, &rc, [an[0], an[1] * sg, an[2]]), a[4]));
        let fd = norm(sub(fist, el));
        let wr = add(el, mul(fd, r.fore));
        set(side(s, CLAV), add(neck, wld(&rc, r.clav.get(s))));
        set(side(s, SH), sh);
        set(side(s, ELBOW), el);
        set(side(s, WRIST), wr);
        set(side(s, KNUCK), add(wr, mul(fd, r.knuck)));
        set(side(s, FIST), fist);
        let acr = match blade {
            Some(b) if s == 1 => b,
            _ => rc[1],
        };
        set(side(s, INDEX), add(fist, mul(acr, 20.0)));
        set(side(s, PINKY), add(fist, mul(acr, -20.0)));
        let hip = tq.hip(r, s);
        let l = k.leg[s];
        let ln = norm([l[0], l[1], l[2]]);
        let at = limb_end(hip, &rp, ln, s, reach_of(l[3], r.thigh, r.shin));
        let ld = norm(sub(at, hip));
        let (kn, an) =
            ik3(hip, at, r.thigh, r.shin, pole_hint(ld, hint_for(Limb::Leg, sg, &rp, [ln[0], ln[1] * sg, ln[2]]), l[4]));
        set(side(s, HIP), hip);
        set(side(s, KNEE), kn);
        set(side(s, ANKLE), an);
        let ft = k.foot[s].unwrap_or([0.0; 3]);
        let pa = (ft[0] + r.ab_pitch) * DG;
        let pb = (ft[0] + r.bt_pitch - ft[2]) * DG;
        let yw = ft[1] * DG;
        let dir = |q: f64| wld(&rp, [cos(q) * cos(yw), sg * cos(q) * sin(yw), -sin(q)]);
        let ball = add(an, mul(dir(pa), r.ab));
        set(side(s, BALL), ball);
        set(side(s, TOE), add(ball, mul(dir(pb), r.bt)));
    }
}

fn lerp_v<const N: usize>(a: [f64; N], b: [f64; N], u: f64) -> [f64; N] {
    std::array::from_fn(|j| a[j] + (b[j] - a[j]) * u)
}
fn lerp_o<const N: usize>(a: Option<[f64; N]>, b: Option<[f64; N]>, u: f64) -> Option<[f64; N]> {
    a.map(|a| lerp_v(a, b.unwrap_or(a), u))
}

/// The key pose at time `t`: every number in-betweened from the keys either side (loops wrap).
pub fn key_at(keys: &[K], dur: f64, looping: bool, t: f64) -> K {
    let dur = if dur != 0.0 { dur } else { keys[keys.len() - 1].t };
    let tt = if looping && dur > 0.0 { ((t % dur) + dur) % dur } else { jmin(jmax(0.0, t), dur) };
    let mut i = 0;
    while i + 2 < keys.len() && keys[i + 1].t <= tt {
        i += 1;
    }
    let (a, b) = (&keys[i], &keys[(i + 1).min(keys.len() - 1)]);
    let u = if b.t > a.t { jmin(1.0, jmax(0.0, (tt - a.t) / (b.t - a.t))) } else { 0.0 };
    K {
        t: a.t,
        hips: lerp_v(a.hips, b.hips, u),
        body: lerp_v(a.body, b.body, u),
        chest: lerp_v(a.chest, b.chest, u),
        head: lerp_v(a.head, b.head, u),
        sh: [lerp_o(a.sh[0], b.sh[0], u), lerp_o(a.sh[1], b.sh[1], u)],
        arm: [lerp_v(a.arm[0], b.arm[0], u), lerp_v(a.arm[1], b.arm[1], u)],
        leg: [lerp_v(a.leg[0], b.leg[0], u), lerp_v(a.leg[1], b.leg[1], u)],
        foot: [lerp_o(a.foot[0], b.foot[0], u), lerp_o(a.foot[1], b.foot[1], u)],
        blade: lerp_o(a.blade, b.blade, u),
        root: lerp_o(a.root, b.root, u),
    }
}

/// A clip's key poses from frames `kf` of a capture. `blade`: a sword clip (the right fist's
/// blade direction is kept).
pub fn encode(r: &Rest, cap: &Cap, kf: &[usize], blade: bool) -> Vec<K> {
    let mut keys: Vec<K> = Vec::with_capacity(kf.len());
    let mut x = vec![0f32; P * 3];
    for &f in kf {
        cap_sample(cap, f as f64 / cap.fps, &mut x);
        let mut k = encode_pose(r, &x, keys.last());
        // To the millisecond: a key lands on its frame.
        k.t = js_round(f as f64 / cap.fps * 1000.0) / 1000.0;
        if blade {
            let m = compose(&from_euler(k.body), &from_euler(k.chest));
            k.blade = Some(loc(&m, norm(sub(pt(&x, side(1, INDEX)), pt(&x, side(1, PINKY))))).map(|v| js_round(v * 100.0)));
        }
        if let Some(t) = &cap.travel {
            k.root = Some([js_round(t[f * 2] as f64 / r.h0 * 100.0), js_round(t[f * 2 + 1] as f64 / r.h0 * 100.0)]);
        }
        keys.push(k);
    }
    keys
}

/// A clip's length as the set records it (to the millisecond).
pub fn dur_of(cap: &Cap) -> f64 {
    js_round(cap.dur * 1000.0) / 1000.0
}

/// A fitted clip: its key poses and how close they stay to the capture.
pub struct Fitted {
    pub keys: Vec<K>,
    /// The worst body-point error over the clip (mm).
    pub max: f64,
    /// The average body-point error (mm).
    pub mean: f64,
}

/// Key poses within `tol` mm of the capture: it starts from the position key frames and adds the
/// frame the in-betweening misses most, until the rest is within `tol` of what the format can
/// capture at all (a frame encoded as its own key): keys past the format's own limits are not
/// worth their tokens.
pub fn fit(r: &Rest, cap: &Cap, tol: f64, blade: bool) -> Fitted {
    let mut kf = reduce(cap, tol);
    let (mut x, mut y) = (vec![0f32; P * 3], vec![0f32; P * 3]);
    let t = |f: usize| f as f64 / cap.fps;
    let dur = dur_of(cap);
    let floor: Vec<f64> = (0..cap.n)
        .map(|f| {
            cap_sample(cap, t(f), &mut x);
            let one = encode(r, cap, &[f], blade);
            decode_pose(r, &key_at(&one, dur, cap.looping, t(f)), &mut y);
            error(&x, &y)
        })
        .collect();
    let mut guard = 0;
    loop {
        let keys = encode(r, cap, &kf, blade);
        let (mut worst, mut wf, mut max, mut sum) = (0.0, None, 0.0f64, 0.0);
        for (f, fl) in floor.iter().enumerate() {
            cap_sample(cap, t(f), &mut x);
            decode_pose(r, &key_at(&keys, dur, cap.looping, t(f)), &mut y);
            let e = error(&x, &y);
            let over = e - fl;
            max = jmax(max, e);
            sum += mean_error(&x, &y);
            if over > worst {
                worst = over;
                wf = Some(f);
            }
        }
        let done = match wf {
            None => true,
            Some(w) => worst <= tol || kf.contains(&w) || guard > cap.n,
        };
        if done {
            return Fitted { keys, max, mean: sum / cap.n as f64 };
        }
        kf.push(wf.unwrap());
        kf.sort_unstable();
        guard += 1;
    }
}

/// A fitted capture as a clip of the set.
pub fn clip_of(cap: &Cap, keys: &[K], src: &str) -> Clip {
    Clip {
        clip: cap.name.clone(),
        src: src.to_string(),
        orig: None,
        take: cap.take.clone(),
        dur: dur_of(cap) as f32,
        looping: cap.looping,
        speed: None,
        tags: Vec::new(),
        desc: String::new(),
        keys: keys.iter().map(K::to_key).collect(),
    }
}

/// Every body point of a clip at time `t`, on the body it was measured on (for checking a
/// translation against its capture).
pub fn pose_of(r: &Rest, clip: &Clip, t: f64, out: &mut [f32]) {
    let keys: Vec<K> = clip.keys.iter().map(K::from_key).collect();
    decode_pose(r, &key_at(&keys, clip.dur as f64, clip.looping, t), out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_and_lengths_follow_javascript() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_round(-0.4).to_bits(), (-0.0f64).to_bits());
        assert_eq!(hypot(&[3.0, 4.0]), 5.0);
        assert_eq!(hypot(&[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(norm([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        let shown: Vec<&str> = SHOWN.iter().map(|i| POINTS[i / 3]).collect();
        assert!(shown.len() == 32 && shown.iter().all(|p| !p.starts_with("index") && !p.starts_with("pinky")));
    }

    #[test]
    fn euler_angles_round_trip() {
        for a in [[30.0, -20.0, 10.0], [-150.0, 60.0, -45.0], [0.0, 0.0, 0.0], [90.0, 5.0, 170.0]] {
            let e = euler(&from_euler(a));
            for i in 0..3 {
                assert!((e[i] - a[i]).abs() < 1e-9, "{a:?} -> {e:?}");
            }
        }
    }
}
