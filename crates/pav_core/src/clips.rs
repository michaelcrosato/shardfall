//! Motion clips: animation captured or keyed elsewhere (motion-capture databases, animation
//! libraries) as readable key poses, played on the biped puppet.
//!
//! The format is the one my-3D2dge introduced for AI models, kept as it is so clips move freely
//! between the two engines. A clip is a short list of key poses; each names what an animator
//! would: where the hips are, how the body, chest and head turn and lean, how the shoulders reach
//! and shrug, which way each arm and leg points and how much the elbow or knee bends, how the
//! feet point. Nothing in it depends on body size (limbs are a direction plus a bend, heights are
//! shares of hip height), so the puppet plays any clip at its own proportions: each limb takes the
//! clip's direction at its own length and the elbows and knees are solved with the same IK the
//! walk uses. Every set file carries the legend (`legend`) and its credits.
//!
//! Sets are the `*.json` files in /anim (embedded at build time; `crate::anim::reload` re-reads
//! them). A clip is named `SET/Clip` (or just `Clip` when only one set has it); animation state
//! stores its `ClipId`, a hash of that name.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use glam::{Quat, Vec3};
use serde::{Deserialize, Deserializer, Serialize};

use crate::puppet::{PuppetDef, PuppetState, Skel};

/// Play flags (`PuppetState::clip_flags`).
/// Only the chest, head and arms follow the clip; the legs keep walking.
pub const UPPER: u8 = 1;
/// Left and right swapped.
pub const MIRROR: u8 = 2;
/// The clip's own travel moves the body (otherwise it plays in place).
pub const TRAVEL: u8 = 4;
/// Fading out (set by `PuppetState::stop_clip`).
pub const STOP: u8 = 8;
/// Fades out by itself when the clip ends (one-shot gestures over the procedural animation).
pub const ONCE: u8 = 16;
/// Play flag: fade in and out three times as fast (an attack's wind-up is short).
pub const QUICK: u8 = 32;
/// Hold the first and last poses instead of wrapping. The editor uses this to inspect a
/// loop's final key without changing how that clip plays in the game.
pub const CLAMP: u8 = 128;

/// Seconds a clip takes to fade in or out.
pub const FADE: f32 = 0.25;

/// Performance settings (the `anim` group): how fast performing characters play their clips
/// and moves, and whether their clips are mirrored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimParams {
    pub tempo: f32,
    pub mirror: bool,
}

impl Default for AnimParams {
    fn default() -> Self {
        Self { tempo: 1.0, mirror: false }
    }
}

impl crate::params::Tunable for AnimParams {
    fn visit(&mut self, v: &mut dyn crate::params::ParamVisitor) {
        v.float("tempo", &mut self.tempo, 0.1, 3.0, "Speed of performers' clips and moves (1 = as captured)");
        v.bool("mirror", &mut self.mirror, "Performers play their clips mirrored, left for right");
    }
}

fn limb<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 5], D::Error> {
    let v = Vec::<f32>::deserialize(d)?;
    if v.len() < 3 || v.len() > 5 {
        return Err(serde::de::Error::custom(format!("a limb is [forward, out, up, bend, twist], got {} numbers", v.len())));
    }
    let mut a = [0.0; 5];
    a[..v.len()].copy_from_slice(&v);
    Ok(a)
}

/// One key pose (see `LEGEND`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: f32,
    pub hips: [f32; 3],
    pub body: [f32; 3],
    pub chest: [f32; 3],
    pub head: [f32; 3],
    #[serde(rename = "shL", default, skip_serializing_if = "Option::is_none")]
    pub sh_l: Option<[f32; 2]>,
    #[serde(rename = "shR", default, skip_serializing_if = "Option::is_none")]
    pub sh_r: Option<[f32; 2]>,
    #[serde(rename = "armL", deserialize_with = "limb")]
    pub arm_l: [f32; 5],
    #[serde(rename = "armR", deserialize_with = "limb")]
    pub arm_r: [f32; 5],
    #[serde(rename = "legL", deserialize_with = "limb")]
    pub leg_l: [f32; 5],
    #[serde(rename = "legR", deserialize_with = "limb")]
    pub leg_r: [f32; 5],
    #[serde(rename = "footL", default, skip_serializing_if = "Option::is_none")]
    pub foot_l: Option<[f32; 3]>,
    #[serde(rename = "footR", default, skip_serializing_if = "Option::is_none")]
    pub foot_r: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blade: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<[f32; 2]>,
}

/// The legend every set file carries: what each number in a key pose means.
pub const LEGEND: &[&str] = &[
    "Readable key poses (the my-3D2dge format). Times in seconds; every other number is a whole number. Rig frame: forward, right, up.",
    "L and R are the figure's own sides. Directions are percentages and need not add up: [100, 0, 0] and [50, 0, 0] agree.",
    "hips   [forward, right, up]   the pelvis, in percent of standing hip height (100 = standing, 55 = sitting on a chair)",
    "body   [turn, lean, tilt]     the pelvis, degrees: turn + = toward the right, lean + = forward, tilt + = toward the right",
    "chest  [turn, lean, tilt]     the chest, relative to the pelvis; head [turn, lean, tilt] relative to the chest",
    "shL shR    [reach, shrug]     degrees the shoulder swings forward (into a punch or an aim) and up, from rest",
    "armL armR  [forward, out, up, bend, twist]  which way the arm points from the shoulder to the fist, in chest space (out + = away from the body on that side; [0, 0, -100] hangs down, [100, 0, 0] punches ahead); bend = elbow bend in degrees (0 = straight); twist = degrees the elbow swings round the arm from its natural direction (back and down)",
    "legL legR  [forward, out, up, bend, twist]  hip to ankle in pelvis space; bend: the knee; twist: from pointing forward",
    "footL footR [down, out, toes]   degrees the foot points down from standing, turns outward, and the toes bend up",
    "blade  [forward, right, up]   (sword clips) the direction of a blade held in the right fist, in chest space",
    "root   [forward, right]       (clips that travel) how far the whole body has moved, in percent of standing hip height",
    "speed  (a clip's header: loops that walk or run in place) how fast the capture travelled, percent of standing hip height a second",
];

/// One clip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Its name within the set (filled in from the set's key when absent).
    #[serde(default)]
    pub clip: String,
    /// Which of the set's sources it came from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub src: String,
    /// What it was cut from: the source's own clip name, or a capture database's take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orig: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<String>,
    pub dur: f32,
    #[serde(rename = "loop", default)]
    pub looping: bool,
    /// A loop that walks or runs in place: how fast its capture travelled, in percent of
    /// standing hip height a second (`walk_rate` matches it to a character's ground speed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    pub keys: Vec<Key>,
}

/// A set: clips from one source family, with their credits.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClipSet {
    pub set: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub format: u32,
    #[serde(default)]
    pub credit: String,
    #[serde(default)]
    pub legend: Vec<String>,
    #[serde(default)]
    pub fps: f32,
    /// The libraries the clips came from: label, origin, license, url, and the body they were
    /// measured on (`rest`, which this engine doesn't need: clips fit any body).
    #[serde(default)]
    pub sources: BTreeMap<String, serde_json::Value>,
    /// Each clip's average and worst body-point error against its capture (mm).
    #[serde(default)]
    pub fit: BTreeMap<String, [f32; 2]>,
    pub clips: BTreeMap<String, Clip>,
}

impl ClipSet {
    /// Parses a set file (`//` comment lines are allowed and skipped).
    pub fn parse(text: &str) -> Result<Self, String> {
        let clean: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let mut s: ClipSet = serde_json::from_str(&clean).map_err(|e| e.to_string())?;
        for (name, c) in s.clips.iter_mut() {
            if c.clip.is_empty() {
                c.clip = name.clone();
            }
            c.check().map_err(|e| format!("{}/{name}: {e}", s.set))?;
        }
        Ok(s)
    }
}

/// A number as the format writes it: whole numbers bare, others to the millisecond.
fn num(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.3}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn nums(v: &[f32]) -> String {
    format!("[{}]", v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(","))
}

fn json(v: &impl Serialize) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

impl Key {
    /// One key pose as three short lines (torso, arms, legs), the way a model reads it.
    pub fn text(&self) -> String {
        let mut torso = vec![format!("\"hips\":{}", nums(&self.hips))];
        torso.push(format!("\"body\":{}", nums(&self.body)));
        torso.push(format!("\"chest\":{}", nums(&self.chest)));
        torso.push(format!("\"head\":{}", nums(&self.head)));
        if let Some(r) = self.root {
            torso.push(format!("\"root\":{}", nums(&r)));
        }
        let mut arms = Vec::new();
        for (n, v) in [("shL", self.sh_l), ("shR", self.sh_r)] {
            if let Some(v) = v {
                arms.push(format!("\"{n}\":{}", nums(&v)));
            }
        }
        arms.push(format!("\"armL\":{}", nums(&self.arm_l)));
        arms.push(format!("\"armR\":{}", nums(&self.arm_r)));
        if let Some(b) = self.blade {
            arms.push(format!("\"blade\":{}", nums(&b)));
        }
        let mut legs = vec![format!("\"legL\":{}", nums(&self.leg_l)), format!("\"legR\":{}", nums(&self.leg_r))];
        for (n, v) in [("footL", self.foot_l), ("footR", self.foot_r)] {
            if let Some(v) = v {
                legs.push(format!("\"{n}\":{}", nums(&v)));
            }
        }
        format!("  {{\"t\":{},\n   {},\n   {},\n   {}}}", num(self.t), torso.join(", "), arms.join(", "), legs.join(", "))
    }
}

impl Clip {
    /// The clip as readable text: its header on one line, then its key poses.
    pub fn text(&self) -> String {
        let mut head = vec![format!("\"clip\": {}", json(&self.clip))];
        if !self.src.is_empty() {
            head.push(format!("\"src\": {}", json(&self.src)));
        }
        if let Some(o) = &self.orig {
            head.push(format!("\"orig\": {}", json(o)));
        }
        if let Some(t) = &self.take {
            head.push(format!("\"take\": {}", json(t)));
        }
        head.push(format!("\"dur\": {}", num(self.dur)));
        head.push(format!("\"loop\": {}", self.looping));
        if let Some(v) = self.speed {
            head.push(format!("\"speed\": {}", num(v)));
        }
        if !self.tags.is_empty() {
            head.push(format!("\"tags\": {}", json(&self.tags)));
        }
        if !self.desc.is_empty() {
            head.push(format!("\"desc\": {}", json(&self.desc)));
        }
        let keys: Vec<String> = self.keys.iter().map(|k| k.text()).collect();
        format!("{{{}, \"keys\": [\n{}\n]}}", head.join(", "), keys.join(",\n"))
    }
}

impl ClipSet {
    /// The whole set as a file: header fields, the legend, sources and fit on their own lines,
    /// then every clip.
    pub fn to_text(&self) -> String {
        let mut out = String::from("{\n");
        out +=
            &format!("\"set\": {},\n\"title\": {},\n\"format\": {},\n", json(&self.set), json(&self.title), self.format.max(1));
        out += &format!("\"credit\": {},\n", json(&self.credit));
        let legend = if self.legend.is_empty() { LEGEND.iter().map(|s| s.to_string()).collect() } else { self.legend.clone() };
        out +=
            &format!("\"legend\": [\n{}\n],\n", legend.iter().map(|l| format!("  {}", json(l))).collect::<Vec<_>>().join(",\n"));
        out += &format!("\"fps\": {},\n", num(self.fps));
        let src: Vec<String> = self.sources.iter().map(|(k, v)| format!("  {}: {}", json(k), json(v))).collect();
        out += &format!("\"sources\": {{\n{}\n}},\n", src.join(",\n"));
        let fit: Vec<String> = self.fit.iter().map(|(k, v)| format!("{}: {}", json(k), nums(v))).collect();
        out += &format!("\"fit\": {{{}}},\n", fit.join(", "));
        let clips: Vec<String> = self.clips.iter().map(|(k, c)| format!("{}: {}", json(k), c.text())).collect();
        out += &format!("\"clips\": {{\n{}\n}}\n}}\n", clips.join(",\n"));
        out
    }
}

impl Clip {
    fn check(&mut self) -> Result<(), String> {
        if self.keys.is_empty() {
            return Err("a clip needs at least one key pose".into());
        }
        if self.keys.iter().any(|k| !k.t.is_finite()) {
            return Err("every key needs a time".into());
        }
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        if self.dur <= 0.0 {
            self.dur = self.keys.last().map_or(0.0, |k| k.t);
        }
        Ok(())
    }

    /// The pose at time `t` (seconds): key poses in-betweened, loops wrap, one-shots hold their
    /// last pose.
    pub fn key_at(&self, t: f32) -> Key {
        let dur = self.duration();
        let tt = if self.looping && dur > 0.0 { t.rem_euclid(dur) } else { t.clamp(0.0, dur) };
        self.sample_key(tt)
    }

    /// The pose with time held between the first and last keys, including a loop's end key.
    pub fn key_at_clamped(&self, t: f32) -> Key {
        self.sample_key(t.clamp(0.0, self.duration()))
    }

    fn duration(&self) -> f32 {
        if self.dur > 0.0 { self.dur } else { self.keys.last().map_or(0.0, |k| k.t) }
    }

    fn sample_key(&self, tt: f32) -> Key {
        let k = &self.keys;
        let mut i = 0;
        while i + 2 < k.len() && k[i + 1].t <= tt {
            i += 1;
        }
        let (a, b) = (&k[i], &k[(i + 1).min(k.len() - 1)]);
        let u = if b.t > a.t { ((tt - a.t) / (b.t - a.t)).clamp(0.0, 1.0) } else { 0.0 };
        lerp_key(a, b, u)
    }

    /// Does the clip travel (carry the body along)?
    pub fn travels(&self) -> bool {
        self.keys.iter().any(|k| k.root.is_some_and(|r| r[0].abs() + r[1].abs() > 5.0))
    }
}

fn lerp_n<const N: usize>(a: [f32; N], b: [f32; N], u: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * u)
}

fn lerp_opt<const N: usize>(a: Option<[f32; N]>, b: Option<[f32; N]>, u: f32) -> Option<[f32; N]> {
    a.map(|a| lerp_n(a, b.unwrap_or(a), u))
}

/// Every number in-betweened (a field missing on one side holds the other's value).
pub fn lerp_key(a: &Key, b: &Key, u: f32) -> Key {
    Key {
        t: a.t + (b.t - a.t) * u,
        hips: lerp_n(a.hips, b.hips, u),
        body: lerp_n(a.body, b.body, u),
        chest: lerp_n(a.chest, b.chest, u),
        head: lerp_n(a.head, b.head, u),
        sh_l: lerp_opt(a.sh_l, b.sh_l, u),
        sh_r: lerp_opt(a.sh_r, b.sh_r, u),
        arm_l: lerp_n(a.arm_l, b.arm_l, u),
        arm_r: lerp_n(a.arm_r, b.arm_r, u),
        leg_l: lerp_n(a.leg_l, b.leg_l, u),
        leg_r: lerp_n(a.leg_r, b.leg_r, u),
        foot_l: lerp_opt(a.foot_l, b.foot_l, u),
        foot_r: lerp_opt(a.foot_r, b.foot_r, u),
        blade: lerp_opt(a.blade, b.blade, u),
        root: lerp_opt(a.root, b.root, u),
    }
}

/// The same pose on the other side: left and right swap, turns and tilts change sign.
pub fn mirror(k: &Key) -> Key {
    let neg = |v: [f32; 3]| [-v[0], v[1], -v[2]];
    let limb = |v: [f32; 5]| [v[0], v[1], v[2], v[3], -v[4]];
    Key {
        t: k.t,
        hips: [k.hips[0], -k.hips[1], k.hips[2]],
        body: neg(k.body),
        chest: neg(k.chest),
        head: neg(k.head),
        sh_l: k.sh_r,
        sh_r: k.sh_l,
        arm_l: limb(k.arm_r),
        arm_r: limb(k.arm_l),
        leg_l: limb(k.leg_r),
        leg_r: limb(k.leg_l),
        foot_l: k.foot_r,
        foot_r: k.foot_l,
        blade: k.blade.map(|b| [b[0], -b[1], b[2]]),
        root: k.root.map(|r| [r[0], -r[1]]),
    }
}

// --- decoding a key pose onto the puppet ---------------------------------------------------------

/// Turn, lean and tilt (degrees) as a rotation in character space (x right, y up, z forward):
/// tilt first (about forward, + toward the right), then lean (about right, + forward), then turn
/// (about up, + toward the right).
pub fn euler(a: [f32; 3]) -> Quat {
    Quat::from_rotation_y(a[0].to_radians())
        * Quat::from_rotation_x(a[1].to_radians())
        * Quat::from_rotation_z(-a[2].to_radians())
}

/// A limb's direction in the format's [forward, out, up] as character-space axes for side `s`.
fn dir(v: [f32; 5], s: f32) -> Vec3 {
    Vec3::new(v[1] * s, v[2], v[0]).normalize_or(Vec3::NEG_Y)
}

/// Root-to-end distance of a two-bone limb bent `bend` degrees at the middle joint.
fn reach_of(bend: f32, l1: f32, l2: f32) -> f32 {
    (l1 * l1 + l2 * l2 + 2.0 * l1 * l2 * bend.to_radians().cos()).max(0.0).sqrt()
}

#[derive(Clone, Copy)]
enum Limb {
    Arm,
    Leg,
}

fn across(h: Vec3, d: Vec3) -> Vec3 {
    (h - d * h.dot(d)).normalize_or(Vec3::Z)
}

/// The way a limb's middle joint naturally bends for direction `d_local` (in the parent's
/// frame `m`): known for one rest direction (an arm hanging forward, out and down with its elbow
/// back; a leg down with its knee forward) and carried to `d_local` by the shortest turn, so it
/// moves smoothly with the limb (the format's reference for twist).
fn hint_for(limb: Limb, s: f32, m: Quat, d_local: Vec3) -> Vec3 {
    let (rest, bend) = match limb {
        Limb::Arm => (Vec3::new(s * 0.5, -0.7, 0.5), Vec3::new(s * 0.5, -0.3, -1.0)),
        Limb::Leg => (Vec3::new(0.0, -1.0, 0.3), Vec3::new(s * 0.1, 0.0, 1.0)),
    };
    let d = d_local.normalize_or(Vec3::NEG_Y);
    let r = rest.normalize();
    let n0 = across(bend.normalize(), r);
    let ax = r.cross(d);
    let sn = ax.length();
    let n = if sn > 1e-9 {
        let k = ax / sn;
        let a = sn.atan2(r.dot(d));
        let (sa, ca) = a.sin_cos();
        n0 * ca + k.cross(n0) * sa + k * k.dot(n0) * (1.0 - ca)
    } else {
        n0
    };
    m * across(n, d)
}

/// `hint` turned `deg` degrees round the limb direction `d`.
fn pole(d: Vec3, hint: Vec3, deg: f32) -> Vec3 {
    let hn = across(hint, d);
    let (s, c) = deg.to_radians().sin_cos();
    hn * c + d.cross(hn) * s
}

/// The puppet's skeleton for key pose `k` (character space, feet at the origin), at the
/// puppet's own proportions. `travel` moves it by the clip's own travel.
/// The puppet's standing hip height: what a clip's percentages are shares of.
pub fn hip_height(def: &PuppetDef) -> f32 {
    def.leg_length * def.scale * 0.97
}

/// How fast to play a clip that walks or runs in place so its feet keep pace with the ground:
/// the character's ground speed over the clip's own (1 when the clip doesn't say), kept
/// within 0.4..2.5.
pub fn walk_rate(def: &PuppetDef, id: u32, ground_speed: f32) -> f32 {
    pace(def, id).map_or(1.0, |p| (ground_speed / p).clamp(0.4, 2.5))
}

/// The ground speed (m/s) a clip that walks or runs in place moves at as captured, on this
/// puppet: its `speed` in hip heights at the puppet's own hip height.
pub fn pace(def: &PuppetDef, id: u32) -> Option<f32> {
    with(id, |c| c.speed).flatten().filter(|s| *s > 1.0).map(|s| s / 100.0 * hip_height(def))
}

/// The pace of a puppet's walk clip (m/s), when it has one that says.
pub fn walk_pace(def: &PuppetDef) -> Option<f32> {
    if def.walk_clip.is_empty() {
        return None;
    }
    pace(def, find_cached(&def.walk_clip))
}

/// Which of a walk and a run loop keeps pace with `ground_speed` (m/s): the run above the pace
/// between theirs (their geometric mean, so each covers the same share of speeds), a little
/// past it once one is playing (`running`), so a speed near the line doesn't flicker. A clip
/// that doesn't say how fast it went counts as an ordinary walk (1.4 hip heights a second) or
/// run (4).
pub fn run_over_walk(def: &PuppetDef, walk: u32, run: u32, ground_speed: f32, running: bool) -> bool {
    let hh = hip_height(def);
    let (w, r) = (pace(def, walk).unwrap_or(1.4 * hh), pace(def, run).unwrap_or(4.0 * hh));
    let mid = (w * r).sqrt();
    ground_speed > mid * if running { 0.87 } else { 1.15 }
}

static STRIKES: std::sync::Mutex<Option<HashMap<u32, f32>>> = std::sync::Mutex::new(None);

/// When a clip's strike lands (seconds): the moment a hand reaches furthest ahead of the
/// hips (the weapon's tip, for a clip that holds a blade; a foot, for a clip tagged `kick`).
/// Measured once a clip.
pub fn strike_time(id: u32) -> Option<f32> {
    if let Some(&t) = STRIKES.lock().unwrap().as_ref().and_then(|m| m.get(&id)) {
        return Some(t);
    }
    let t = measure_strike(id)?;
    STRIKES.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, t);
    Some(t)
}

fn measure_strike(id: u32) -> Option<f32> {
    with(id, |c| {
        let def = PuppetDef::default();
        let kick = c.tags.iter().any(|t| t == "kick");
        let blade = c.keys.iter().any(|k| k.blade.is_some());
        let (mut best, mut at) = (f32::NEG_INFINITY, 0.0);
        let n = (c.dur * 60.0).ceil().max(1.0) as usize;
        for i in 0..=n {
            let t = c.dur * i as f32 / n as f32;
            let s = skel(&def, &c.key_at(t), false);
            let ends = if kick { s.ankle } else { s.hand };
            let mut reach = ends.iter().map(|p| p.z - s.pelvis.z).fold(f32::NEG_INFINITY, f32::max);
            if blade {
                reach = reach.max((s.hand[1] + s.weapon * 0.9).z - s.pelvis.z);
            }
            if reach > best {
                (best, at) = (reach, t);
            }
        }
        at
    })
}

/// A clip name and the strike time written after it (`SET/Clip@0.4`).
fn split_at_sign(name: &str) -> (&str, Option<f32>) {
    match name.rsplit_once('@') {
        Some((n, t)) => (n.trim(), t.trim().parse::<f32>().ok()),
        None => (name.trim(), None),
    }
}

/// A puppet's captured attack for swing `combo` of skill `skill` (a list goes round): the clip
/// and when its strike lands (seconds; `SET/Clip@0.4` says so outright).
pub fn attack_clip(def: &PuppetDef, skill: &str, combo: u32) -> Option<(u32, f32)> {
    let (clip, at) = split_at_sign(def.attack_clips.get(skill)?.swing(combo));
    let id = find_cached(clip);
    if id == 0 {
        return None;
    }
    Some((id, at.or_else(|| strike_time(id))?))
}

/// Whether clip `id` is one of a puppet's captured attacks.
pub fn is_attack(def: &PuppetDef, id: u32) -> bool {
    id != 0 && def.attack_clips.values().flat_map(|s| s.names()).any(|n| find_cached(split_at_sign(n).0) == id)
}

/// A puppet's captured death: the clip, how fast to play it, and when it has the body down
/// (seconds). A monster's body clears soon after it falls, so a long fall plays up to half as
/// fast again, to be down within about 1.2 s.
pub fn death_clip(def: &PuppetDef) -> Option<(u32, f32, f32)> {
    if def.death_clip.is_empty() {
        return None;
    }
    let id = find_cached(&def.death_clip);
    let dur = with(id, |c| c.dur)?;
    let rate = (dur / 1.2).clamp(1.0, 1.5);
    Some((id, rate, dur / rate))
}

/// Every clip a puppet names that the library doesn't have (data checks).
pub fn missing(def: &PuppetDef) -> Vec<String> {
    let mut names: Vec<&str> = vec![&def.idle_clip, &def.walk_clip, &def.run_clip, &def.death_clip, &def.dodge_clip];
    names.extend(def.attack_clips.values().flat_map(|s| s.names()).map(|n| split_at_sign(n).0));
    names.into_iter().map(str::trim).filter(|n| !n.is_empty() && find(n).is_none()).map(str::to_string).collect()
}

pub fn skel(def: &PuppetDef, k: &Key, travel: bool) -> Skel {
    let sc = def.scale;
    let leg = def.leg_length * sc;
    let arm = def.arm_length * sc;
    let hr = def.head_radius * sc;
    let tr = def.torso_radius * sc;
    let lr = def.limb_radius * sc;
    let hip_h = hip_height(def);
    let qp = euler(k.body);
    let qc = qp * euler(k.chest);
    let qh = qc * euler(k.head);
    let mut pelvis = Vec3::new(k.hips[1], k.hips[2], k.hips[0]) * (hip_h / 100.0);
    if travel {
        if let Some(r) = k.root {
            pelvis += Vec3::new(r[1], 0.0, r[0]) * (hip_h / 100.0);
        }
    }
    // The spine bends between the pelvis and the chest; one segment stands in for it.
    let torso_dir = (qp.slerp(qc, 0.6) * Vec3::Y).normalize_or(Vec3::Y);
    let chest = pelvis + torso_dir * def.torso_length * sc;
    let neck = chest + (qc * Vec3::Y) * hr * 0.55;
    let head = neck + (qh * Vec3::Y) * hr * 0.95;
    let mut sk = Skel {
        pelvis,
        chest,
        neck,
        head,
        pelvis_rot: qp,
        chest_rot: qc,
        head_rot: qh,
        crown: qh * Vec3::Y,
        shoulder: [Vec3::ZERO; 2],
        elbow: [Vec3::ZERO; 2],
        hand: [Vec3::ZERO; 2],
        hip: [Vec3::ZERO; 2],
        knee: [Vec3::ZERO; 2],
        ankle: [Vec3::ZERO; 2],
        toe: [Vec3::Z; 2],
        weapon: Vec3::Z,
        blink: 0.0,
    };
    let (a1, a2) = (arm * 0.5, arm * 0.5);
    let (l1, l2) = (leg * 0.5, leg * 0.5);
    for (i, s) in [(0usize, -1.0f32), (1, 1.0)] {
        // Shoulder: the rest offset swung forward by `reach` and up by `shrug`.
        let v0 = Vec3::new(s * def.shoulder_width * sc, -0.05 * sc, 0.0);
        let l = v0.length();
        let [reach, shrug] = if i == 0 { k.sh_l } else { k.sh_r }.unwrap_or([0.0, 0.0]);
        let az = reach.to_radians();
        let el = (v0.y / l).asin() + shrug.to_radians();
        let sh = chest + qc * Vec3::new(s * l * el.cos() * az.cos(), l * el.sin(), l * el.cos() * az.sin());
        let a = if i == 0 { k.arm_l } else { k.arm_r };
        let d = dir(a, s);
        let target = sh + qc * d * reach_of(a[3], a1, a2);
        let ad = (target - sh).normalize_or(Vec3::NEG_Y);
        let (elbow, hand) = crate::puppet::ik(sh, target, a1, a2, pole(ad, hint_for(Limb::Arm, s, qc, d), a[4]));
        sk.shoulder[i] = sh;
        sk.elbow[i] = elbow;
        sk.hand[i] = hand;
        // Leg: from the hip, the clip's direction at the leg's reach for that knee bend.
        let hip = pelvis + qp * Vec3::new(s * def.hip_width * sc, 0.0, 0.0);
        let g = if i == 0 { k.leg_l } else { k.leg_r };
        let d = dir(g, s);
        let target = hip + qp * d * reach_of(g[3], l1, l2);
        let gd = (target - hip).normalize_or(Vec3::NEG_Y);
        let (knee, ankle) = crate::puppet::ik(hip, target, l1, l2, pole(gd, hint_for(Limb::Leg, s, qp, d), g[4]));
        sk.hip[i] = hip;
        sk.knee[i] = knee;
        sk.ankle[i] = ankle;
        let ft = if i == 0 { k.foot_l } else { k.foot_r }.unwrap_or([0.0; 3]);
        let (pa, yw) = (ft[0].to_radians(), ft[1].to_radians());
        sk.toe[i] = (qp * Vec3::new(s * pa.cos() * yw.sin(), -pa.sin(), pa.cos() * yw.cos())).normalize_or(Vec3::Z);
    }
    sk.weapon = match k.blade {
        Some(b) => (qc * Vec3::new(b[1], b[2], b[0])).normalize_or(Vec3::Z),
        None => (sk.hand[1] - sk.elbow[1]).normalize_or(Vec3::Z),
    };
    // Nothing goes through the floor: a pose that dips below it (pointed toes in a jump, a body
    // lying on the ground) is lifted until its lowest point stands on it.
    let mut low = sk.ankle[0].y.min(sk.ankle[1].y);
    for i in 0..2 {
        low = low.min(sk.knee[i].y - lr).min(sk.hand[i].y - lr).min((sk.ankle[i] + sk.toe[i] * 0.12 * sc).y);
    }
    low = low.min(sk.head.y - hr).min(sk.pelvis.y - tr * 0.9).min(sk.chest.y - tr * 0.9);
    if low < 0.0 {
        sk.transform(Quat::IDENTITY, Vec3::Y * -low);
    }
    sk
}

fn smooth(w: f32) -> f32 {
    let w = w.clamp(0.0, 1.0);
    w * w * (3.0 - 2.0 * w)
}

/// `b` laid over `a` by `w` (0..1). `upper` keeps `a`'s hips and legs and moves `b`'s upper body
/// onto `a`'s pelvis. Elbows and knees are solved again so the limbs keep their lengths.
pub fn blend(def: &PuppetDef, a: &Skel, b: &Skel, w: f32, upper: bool) -> Skel {
    let w = smooth(w);
    if w <= 0.0 {
        return *a;
    }
    let mut b = *b;
    if upper {
        let shift = a.pelvis - b.pelvis;
        for v in [&mut b.pelvis, &mut b.chest, &mut b.neck, &mut b.head] {
            *v += shift;
        }
        for i in 0..2 {
            for v in [&mut b.shoulder[i], &mut b.elbow[i], &mut b.hand[i]] {
                *v += shift;
            }
            b.hip[i] = a.hip[i];
            b.knee[i] = a.knee[i];
            b.ankle[i] = a.ankle[i];
            b.toe[i] = a.toe[i];
        }
        b.pelvis_rot = a.pelvis_rot;
    }
    let l = |x: Vec3, y: Vec3| x.lerp(y, w);
    let n = |x: Vec3, y: Vec3| x.lerp(y, w).normalize_or(y);
    let mut s = Skel {
        pelvis: l(a.pelvis, b.pelvis),
        chest: l(a.chest, b.chest),
        neck: l(a.neck, b.neck),
        head: l(a.head, b.head),
        pelvis_rot: a.pelvis_rot.slerp(b.pelvis_rot, w),
        chest_rot: a.chest_rot.slerp(b.chest_rot, w),
        head_rot: a.head_rot.slerp(b.head_rot, w),
        crown: n(a.crown, b.crown),
        weapon: n(a.weapon, b.weapon),
        blink: a.blink,
        ..*a
    };
    let k = def.scale;
    let (arm, leg) = (def.arm_length * k, def.leg_length * k);
    for i in 0..2 {
        s.shoulder[i] = l(a.shoulder[i], b.shoulder[i]);
        s.hand[i] = l(a.hand[i], b.hand[i]);
        let hint = l(a.elbow[i] - (a.shoulder[i] + a.hand[i]) * 0.5, b.elbow[i] - (b.shoulder[i] + b.hand[i]) * 0.5);
        let (e, h) = crate::puppet::ik(s.shoulder[i], s.hand[i], arm * 0.5, arm * 0.5, hint);
        s.elbow[i] = e;
        s.hand[i] = h;
        s.hip[i] = l(a.hip[i], b.hip[i]);
        s.ankle[i] = l(a.ankle[i], b.ankle[i]);
        let hint = l(a.knee[i] - (a.hip[i] + a.ankle[i]) * 0.5, b.knee[i] - (b.hip[i] + b.ankle[i]) * 0.5);
        let (kn, an) = crate::puppet::ik(s.hip[i], s.ankle[i], leg * 0.5, leg * 0.5, hint);
        s.knee[i] = kn;
        s.ankle[i] = an;
        s.toe[i] = n(a.toe[i], b.toe[i]);
    }
    s
}

/// The skeleton of clip `id` at time `t` with `flags`, or None if there is no such clip.
pub fn pose_of(def: &PuppetDef, id: u32, t: f32, flags: u8) -> Option<Skel> {
    let mut k = with(id, |c| if flags & CLAMP != 0 { c.key_at_clamped(t) } else { c.key_at(t) })?;
    if flags & MIRROR != 0 {
        k = mirror(&k);
    }
    Some(skel(def, &k, flags & TRAVEL != 0))
}

/// The procedural skeleton `base` with the clips `st` is playing laid over it.
pub fn over(def: &PuppetDef, st: &PuppetState, base: Skel) -> Skel {
    let mut sk = base;
    for (id, t, w, flags) in [(st.clip2, st.clip2_t, st.clip2_w, st.clip2_flags), (st.clip, st.clip_t, st.clip_w, st.clip_flags)]
    {
        if id == 0 || w <= 0.0 {
            continue;
        }
        if let Some(c) = pose_of(def, id, t, flags) {
            sk = blend(def, &sk, &c, w, flags & UPPER != 0);
        }
    }
    sk
}

// --- the library ---------------------------------------------------------------------------------

/// A clip's id: a hash of `SET/Clip` (never 0, which means none).
pub fn clip_id(set: &str, clip: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in set.bytes().chain(std::iter::once(b'/')).chain(clip.bytes()) {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h.max(1)
}

/// Every set, and every clip by id.
#[derive(Default)]
pub struct ClipLib {
    pub sets: Vec<Arc<ClipSet>>,
    by_id: HashMap<u32, (usize, String)>,
}

impl ClipLib {
    /// Build a replacement without changing this library. Other sets, including libraries
    /// loaded from subfolders, remain available. Hash collisions fail before installation.
    pub fn replacing_set(&self, set: ClipSet) -> Result<Self, String> {
        // A full motion library can contain millions of key values. Keep its immutable
        // sets shared; an edit only needs a fresh index and the set being replaced.
        let mut next = Self { sets: self.sets.clone(), by_id: self.by_id.clone() };
        if let Some(index) = self.sets.iter().position(|existing| existing.set == set.set) {
            next.by_id.retain(|_, (i, _)| *i != index);
            for name in set.clips.keys() {
                let id = clip_id(&set.set, name);
                if let Some((j, other)) = next.by_id.get(&id) {
                    return Err(format!("{}/{name} and {}/{other} hash alike: rename one", set.set, next.sets[*j].set));
                }
                next.by_id.insert(id, (index, name.clone()));
            }
            next.sets[index] = Arc::new(set);
        } else {
            next.add(set)?;
        }
        Ok(next)
    }

    pub fn add(&mut self, set: ClipSet) -> Result<(), String> {
        if self.sets.iter().any(|s| s.set == set.set) {
            return Err(format!("two sets are called {}", set.set));
        }
        let i = self.sets.len();
        for name in set.clips.keys() {
            let id = clip_id(&set.set, name);
            if let Some((j, other)) = self.by_id.get(&id) {
                return Err(format!("{}/{name} and {}/{other} hash alike: rename one", set.set, self.sets[*j].set));
            }
            self.by_id.insert(id, (i, name.clone()));
        }
        self.sets.push(Arc::new(set));
        Ok(())
    }

    pub fn get(&self, id: u32) -> Option<&Clip> {
        let (i, name) = self.by_id.get(&id)?;
        self.sets[*i].clips.get(name)
    }

    /// The set and clip names of `id`.
    pub fn name_of(&self, id: u32) -> Option<String> {
        let (i, name) = self.by_id.get(&id)?;
        Some(format!("{}/{name}", self.sets[*i].set))
    }

    /// `SET/Clip`, or a bare `Clip` name (the first set that has it), case-insensitive.
    pub fn find(&self, name: &str) -> Option<u32> {
        let (set, clip) = match name.split_once('/') {
            Some((s, c)) => (Some(s), c),
            None => (None, name),
        };
        for s in &self.sets {
            if set.is_some_and(|x| !x.eq_ignore_ascii_case(&s.set)) {
                continue;
            }
            if let Some(c) = s.clips.keys().find(|c| c.eq_ignore_ascii_case(clip)) {
                return Some(clip_id(&s.set, c));
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

fn load() -> Result<ClipLib, String> {
    let mut lib = ClipLib::default();
    for name in crate::anim::files("json") {
        let text = crate::anim::source(&name).unwrap_or_default();
        let set = ClipSet::parse(&text).map_err(|e| format!("anim/{name}: {e}"))?;
        lib.add(set).map_err(|e| format!("anim/{name}: {e}"))?;
    }
    Ok(lib)
}

static LIB: RwLock<Option<Arc<ClipLib>>> = RwLock::new(None);

/// The clip library (the embedded sets the first time).
pub fn library() -> Arc<ClipLib> {
    if let Some(l) = LIB.read().unwrap().as_ref() {
        return l.clone();
    }
    let l = Arc::new(load().unwrap_or_else(|e| panic!("clip sets: {e}")));
    // Another thread may have initialized or edited the library while the files loaded.
    LIB.write().unwrap().get_or_insert(l).clone()
}

/// Re-reads the sets (see `crate::anim::reload`). Returns how many sets and clips there are.
pub fn reload() -> Result<(usize, usize), String> {
    let l = load()?;
    let n = (l.sets.len(), l.len());
    *LIB.write().unwrap() = Some(Arc::new(l));
    *NAMES.lock().unwrap() = None;
    *STRIKES.lock().unwrap() = None;
    Ok(n)
}

/// Install one checked set without reloading or dropping other sets. Stable clip ids make
/// changes visible on the next frame. Readers keep their old Arc until that frame is complete.
pub fn replace_set(set: ClipSet) -> Result<(), String> {
    // Parse the written form before it can enter the renderer. Authoring tools apply their
    // stricter value checks before this shared library operation.
    let set = ClipSet::parse(&set.to_text())?;
    let _ = library();
    {
        // The file watcher and the live tools can update different sets on different
        // threads. Build from the latest library while holding its write lock, so neither
        // accepted update can replace the other with an older snapshot.
        let mut library = LIB.write().unwrap();
        let next = library.as_ref().expect("library initialized above").replacing_set(set)?;
        *library = Some(Arc::new(next));
    }
    // A name lookup can hold NAMES while reading LIB. Release LIB before clearing caches.
    *NAMES.lock().unwrap() = None;
    *STRIKES.lock().unwrap() = None;
    Ok(())
}

/// Adds every set in the folder `anim/<sub>` (on disk: the big libraries that aren't embedded,
/// such as `cmu`, every take of the CMU database) to the library. Sets already loaded are
/// skipped. Returns how many sets and clips were added.
pub fn load_folder(sub: &str) -> Result<(usize, usize), String> {
    let dir = crate::anim::disk_dir().ok_or("no anim folder on disk (run from the repository, or set PAV_ANIM)")?.join(sub);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let old = library();
    let mut lib = ClipLib::default();
    for s in &old.sets {
        lib.add((**s).clone())?;
    }
    let (mut sets, before) = (0, lib.len());
    for p in files {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let set = ClipSet::parse(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        if lib.sets.iter().any(|s| s.set == set.set) {
            continue;
        }
        lib.add(set).map_err(|e| format!("{}: {e}", p.display()))?;
        sets += 1;
    }
    let added = lib.len() - before;
    *LIB.write().unwrap() = Some(Arc::new(lib));
    *NAMES.lock().unwrap() = None;
    *STRIKES.lock().unwrap() = None;
    Ok((sets, added))
}

/// Runs `f` on clip `id`, if there is one.
pub fn with<R>(id: u32, f: impl FnOnce(&Clip) -> R) -> Option<R> {
    if id == 0 {
        return None;
    }
    library().get(id).map(f)
}

/// The id of a clip by name (`SET/Clip` or `Clip`).
pub fn find(name: &str) -> Option<u32> {
    library().find(name)
}

static NAMES: std::sync::Mutex<Option<HashMap<String, u32>>> = std::sync::Mutex::new(None);

/// `find` remembered (0 = no such clip), for names looked up every tick.
pub fn find_cached(name: &str) -> u32 {
    let mut cache = NAMES.lock().unwrap();
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some(&id) = map.get(name) {
        return id;
    }
    let id = find(name).unwrap_or(0);
    map.insert(name.to_string(), id);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing() -> Key {
        Key {
            hips: [0.0, 0.0, 100.0],
            arm_l: [0.0, 0.0, -100.0, 10.0, 0.0],
            arm_r: [0.0, 0.0, -100.0, 10.0, 0.0],
            leg_l: [0.0, 0.0, -100.0, 5.0, 0.0],
            leg_r: [0.0, 0.0, -100.0, 5.0, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn replacing_a_set_shares_other_keys_and_rejects_collisions_without_changes() {
        let set = |name: &str, clip: &str| ClipSet {
            set: name.into(),
            clips: BTreeMap::from([(
                clip.into(),
                Clip { clip: clip.into(), dur: 1.0, keys: vec![standing()], ..Default::default() },
            )]),
            ..Default::default()
        };
        let mut original = ClipLib::default();
        original.add(set("A", "Safe")).unwrap();
        original.add(set("A/B", "C")).unwrap();
        let id = clip_id("A", "Safe");
        let other = clip_id("A/B", "C");
        let mut edited = set("A", "Safe");
        edited.clips.get_mut("Safe").unwrap().keys[0].hips[2] = 60.0;
        let next = original.replacing_set(edited).unwrap();
        assert!(Arc::ptr_eq(&original.sets[1], &next.sets[1]), "unrelated key arrays stay shared");
        assert_eq!(original.by_id[&other], next.by_id[&other], "other set indices remain stable");
        assert_eq!(original.get(id).unwrap().keys[0].hips[2], 100.0, "existing readers keep the old pose");
        assert_eq!(next.get(id).unwrap().keys[0].hips[2], 60.0);
        // Both names hash the bytes A/B/C. This constructs a collision without a random search.
        assert!(next.replacing_set(set("A", "B/C")).is_err());
        assert_eq!(next.get(id).unwrap().keys[0].hips[2], 60.0, "a failed replacement leaves its input unchanged");
        assert!(next.get(other).is_some());
    }

    #[test]
    fn a_standing_key_stands_on_the_floor() {
        let def = PuppetDef::default();
        let sk = skel(&def, &standing(), false);
        for i in 0..2 {
            assert!(sk.ankle[i].y.abs() < 0.03, "feet on the floor: {}", sk.ankle[i]);
            assert!(sk.hand[i].y < sk.shoulder[i].y - 0.3, "arms hang: {}", sk.hand[i]);
            // Limbs keep the puppet's own lengths.
            let arm = def.arm_length * def.scale;
            assert!(((sk.elbow[i] - sk.shoulder[i]).length() - arm * 0.5).abs() < 1e-3);
            assert!(((sk.hand[i] - sk.elbow[i]).length() - arm * 0.5).abs() < 1e-3);
        }
        assert!(sk.head.y > 1.4, "head up: {}", sk.head);
    }

    #[test]
    fn directions_and_mirror() {
        let def = PuppetDef::default();
        let mut k = standing();
        k.arm_r = [100.0, 0.0, 0.0, 0.0, 0.0]; // punch straight ahead
        let sk = skel(&def, &k, false);
        assert!(sk.hand[1].z > 0.4 && (sk.hand[1].y - sk.shoulder[1].y).abs() < 0.05, "punch ahead: {}", sk.hand[1]);
        let m = skel(&def, &mirror(&k), false);
        assert!(m.hand[0].z > 0.4 && m.hand[1].z < 0.2, "mirrored: left punches {} {}", m.hand[0], m.hand[1]);
        // A turn of the body to the right turns the chest's front toward +x.
        k.body = [90.0, 0.0, 0.0];
        let t = skel(&def, &k, false);
        assert!((t.chest_rot * Vec3::Z).x > 0.9, "turned right");
        // Lying flat (leaning back 90 degrees with the hips low) stays above the floor.
        k.body = [0.0, -90.0, 0.0];
        k.hips = [0.0, 0.0, 15.0];
        let lying = skel(&def, &k, false);
        assert!(lying.head.y - def.head_radius > -1e-3 && lying.pelvis.y > 0.0, "{lying:?}");
    }

    #[test]
    fn keys_inbetween_and_loop() {
        let mut a = standing();
        let mut b = standing();
        b.t = 1.0;
        b.hips = [0.0, 0.0, 60.0];
        a.t = 0.0;
        let c = Clip { clip: "x".into(), dur: 1.0, looping: true, keys: vec![a, b], ..Default::default() };
        assert!((c.key_at(0.5).hips[2] - 80.0).abs() < 1e-3);
        assert!((c.key_at(1.25).hips[2] - 90.0).abs() < 1e-3, "loops wrap");
        let one = Clip { looping: false, ..c };
        assert!((one.key_at(5.0).hips[2] - 60.0).abs() < 1e-3, "one-shots hold");
    }
}
