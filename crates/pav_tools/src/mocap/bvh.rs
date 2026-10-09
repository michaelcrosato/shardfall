//! BioVision hierarchy files (.bvh), the plain-text format most motion-capture datasets ship
//! in: a skeleton (joints, offsets, channels), then every frame's channel values. Forward
//! kinematics gives each joint's world rotation and position; the 36 body points are placed on
//! the joints by a bone map, so the takes are cut like the CMU database's (`super::takes`).
//!
//! Maps: 100STYLE's skeleton (Hips, Chest .. Chest4, Neck, Head, RightCollar, RightShoulder,
//! RightElbow, RightWrist, RightHip, RightKnee, RightAnkle, RightToe); the MotionBuilder names
//! Mixamo, LaFAN1 and many converters use (Hips, Spine, Spine1, Spine2, Neck, Head, LeftShoulder,
//! LeftArm, LeftForeArm, LeftHand, LeftUpLeg, LeftLeg, LeftFoot, LeftToeBase or LeftToe); and the
//! Bandai Namco Research motion datasets' (Hips, Spine, Chest, Neck, Head, Shoulder_L,
//! UpperArm_L, LowerArm_L, Hand_L, UpperLeg_L, LowerLeg_L, Foot_L, Toes_L); and the Unreal and
//! Rigify names FBX files from Unreal and Blender carry (pelvis, spine_01, upperarm_l, thigh_l;
//! DEF-hips, DEF-upper_arm.L). A map is a table of joint names per body point, with fallbacks:
//! another skeleton is one more table in `MAPS`.
//!
//! Files differ in two ways the reader allows for. Position channels give a joint's place
//! relative to its parent outright (as three.js reads them): exporters that key every joint's
//! translation repeat its offset there. And a skeleton's zero pose (every rotation zero) is not
//! always a body standing: MotionBuilder exports lay each bone along its own x axis, so the
//! spine and legs point sideways until the first frame turns them. The rest the body is measured
//! on is then the frame where it stands straightest.

use anyhow::{Result, anyhow, bail};
use glam::{DMat3, DVec3};

use super::readable::{POINTS, V};
use super::takes::{Body, Take};

#[derive(Clone, Copy, Debug)]
enum Chan {
    Pos(usize),
    Rot(usize),
}

/// A parsed .bvh file.
pub struct Bvh {
    names: Vec<String>,
    parent: Vec<Option<usize>>,
    offset: Vec<DVec3>,
    chans: Vec<Vec<Chan>>,
    /// Where each joint's channels start in a frame's values.
    first: Vec<usize>,
    /// The end site's offset, for a joint that ends a chain.
    end: Vec<Option<DVec3>>,
    /// Seconds per frame.
    pub frame_time: f64,
    width: usize,
    values: Vec<f64>,
    /// The joints' rest transforms (rotation, and place in the parent's frame), when a file
    /// gives them outright (an FBX skeleton's bind pose); else the zero pose.
    bind: Vec<(DMat3, DVec3)>,
    /// Every frame's joint transforms given outright (an FBX stack), frame after frame.
    local: Vec<(DMat3, DVec3)>,
}

impl Bvh {
    pub fn parse(text: &str) -> Result<Bvh> {
        let mut tok = text.split_whitespace().peekable();
        let mut b = Bvh {
            names: Vec::new(),
            parent: Vec::new(),
            offset: Vec::new(),
            chans: Vec::new(),
            first: Vec::new(),
            end: Vec::new(),
            frame_time: 0.0,
            width: 0,
            values: Vec::new(),
            bind: Vec::new(),
            local: Vec::new(),
        };
        let num = |t: Option<&str>| -> Result<f64> {
            t.ok_or_else(|| anyhow!("the file ends early"))?.parse::<f64>().map_err(|e| anyhow!("a number: {e}"))
        };
        let mut stack: Vec<usize> = Vec::new();
        let mut frames = 0usize;
        while let Some(t) = tok.next() {
            match t {
                "HIERARCHY" | "{" => {}
                "ROOT" | "JOINT" => {
                    let name = tok.next().ok_or_else(|| anyhow!("a joint without a name"))?;
                    b.names.push(name.to_string());
                    b.parent.push(stack.last().copied());
                    b.offset.push(DVec3::ZERO);
                    b.chans.push(Vec::new());
                    b.first.push(0);
                    b.end.push(None);
                    stack.push(b.names.len() - 1);
                }
                "End" => {
                    // End Site { OFFSET x y z }
                    let _site = tok.next();
                    let mut off = DVec3::ZERO;
                    while let Some(t) = tok.next() {
                        match t {
                            "}" => break,
                            "OFFSET" => {
                                let (x, y, z) = (num(tok.next())?, num(tok.next())?, num(tok.next())?);
                                off = DVec3::new(x, y, z);
                            }
                            _ => {}
                        }
                    }
                    if let Some(&j) = stack.last() {
                        b.end[j].get_or_insert(off);
                    }
                }
                "OFFSET" => {
                    let j = *stack.last().ok_or_else(|| anyhow!("OFFSET outside a joint"))?;
                    b.offset[j] = DVec3::new(num(tok.next())?, num(tok.next())?, num(tok.next())?);
                }
                "CHANNELS" => {
                    let j = *stack.last().ok_or_else(|| anyhow!("CHANNELS outside a joint"))?;
                    let n = num(tok.next())? as usize;
                    b.first[j] = b.width;
                    for _ in 0..n {
                        let c = tok.next().ok_or_else(|| anyhow!("the file ends early"))?.to_ascii_lowercase();
                        let axis = match c.as_bytes().first() {
                            Some(b'x') => 0,
                            Some(b'y') => 1,
                            Some(b'z') => 2,
                            _ => bail!("channel {c} is not read"),
                        };
                        b.chans[j].push(if c.ends_with("position") { Chan::Pos(axis) } else { Chan::Rot(axis) });
                    }
                    b.width += n;
                }
                "}" => {
                    stack.pop();
                }
                "MOTION" => {}
                "Frames:" => frames = num(tok.next())? as usize,
                "Frame" => {
                    let _time = tok.next();
                    b.frame_time = num(tok.next())?;
                    break;
                }
                other => bail!("unexpected '{other}' in the hierarchy"),
            }
        }
        if b.names.is_empty() {
            bail!("no joints: not a BVH file");
        }
        b.values.reserve(frames * b.width);
        for t in tok {
            b.values.push(t.parse::<f64>().map_err(|e| anyhow!("frame values: {e}"))?);
        }
        if b.width == 0 || b.values.len() < b.width {
            bail!("no frames");
        }
        b.values.truncate(b.values.len() / b.width * b.width);
        if b.frame_time <= 0.0 {
            b.frame_time = 1.0 / 30.0;
        }
        Ok(b)
    }

    /// A skeleton and its frames given as transforms (`bind`: each joint's at rest; `local`:
    /// each frame's, frame after frame), as an FBX file's stack is read.
    pub fn from_poses(
        names: Vec<String>,
        parent: Vec<Option<usize>>,
        bind: Vec<(DMat3, DVec3)>,
        local: Vec<(DMat3, DVec3)>,
        frame_time: f64,
    ) -> Bvh {
        let n = names.len();
        Bvh {
            offset: bind.iter().map(|b| b.1).collect(),
            chans: vec![Vec::new(); n],
            first: vec![0; n],
            end: vec![None; n],
            names,
            parent,
            frame_time,
            width: 0,
            values: Vec::new(),
            bind,
            local,
        }
    }

    pub fn frames(&self) -> usize {
        self.values.len().checked_div(self.width).unwrap_or_else(|| self.local.len() / self.names.len().max(1))
    }

    pub fn joints(&self) -> &[String] {
        &self.names
    }

    /// Each joint's world rotation and position (file units) at a frame (`None`: the rest pose).
    fn pose(&self, frame: Option<usize>) -> Vec<(DMat3, DVec3)> {
        let mut out: Vec<(DMat3, DVec3)> = Vec::with_capacity(self.names.len());
        let n = self.names.len();
        for j in 0..n {
            let (mut r, mut t) = match frame {
                None if !self.bind.is_empty() => self.bind[j],
                Some(f) if !self.local.is_empty() => self.local[f * n + j],
                _ => (DMat3::IDENTITY, self.offset[j]),
            };
            if let Some(f) = frame.filter(|_| self.local.is_empty()) {
                let v = &self.values[f * self.width..];
                for (k, c) in self.chans[j].iter().enumerate() {
                    let x = v[self.first[j] + k];
                    match *c {
                        Chan::Pos(a) => t[a] = x,
                        Chan::Rot(0) => r *= DMat3::from_rotation_x(x.to_radians()),
                        Chan::Rot(1) => r *= DMat3::from_rotation_y(x.to_radians()),
                        Chan::Rot(_) => r *= DMat3::from_rotation_z(x.to_radians()),
                    }
                }
            }
            out.push(match self.parent[j] {
                Some(p) => (out[p].0 * r, out[p].1 + out[p].0 * t),
                None => (r, t),
            });
        }
        out
    }

    /// A joint's far end at rest, in its own frame: its end site, else its first child.
    fn tip(&self, j: usize) -> DVec3 {
        self.end[j]
            .or_else(|| (0..self.names.len()).find(|&c| self.parent[c] == Some(j)).map(|c| self.offset[c]))
            .unwrap_or(DVec3::ZERO)
    }

    fn find(&self, name: &str) -> Option<usize> {
        let bare = |n: &str| n.rsplit(':').next().unwrap_or(n).to_ascii_lowercase();
        let want = name.to_ascii_lowercase();
        self.names.iter().position(|n| bare(n) == want || n.to_ascii_lowercase() == want)
    }
}

/// Where a body point sits on a joint.
#[derive(Clone, Copy, Debug)]
enum Place {
    Joint,
    /// The joint's far end.
    Tip,
    /// A share of the way from the joint to its far end.
    Along(f64),
    /// 12 cm in front of the joint, the way the body faces at rest.
    Ahead,
    /// A share along, then 3.5 cm across the hand (+ toward the thumb, the way the body faces
    /// in a palms-down T-pose).
    Across(f64, f64),
}

/// A bone map: for each body point, the joints that may give it, the first a file has winning.
/// `{S}` is the side (`sides`).
struct Map {
    name: &'static str,
    /// Joints only this skeleton has.
    test: &'static [&'static str],
    /// What `{S}` is on the left and on the right.
    sides: [&'static str; 2],
    points: [&'static [(&'static str, Place)]; 23],
}

use Place::*;

const MAPS: [Map; 5] = [
    Map {
        name: "100style",
        test: &["Chest4", "RightCollar"],
        sides: ["Left", "Right"],
        points: [
            &[("Hips", Joint)],
            &[("Chest", Joint)],
            &[("Chest2", Joint)],
            &[("Chest4", Joint)],
            &[("Neck", Joint)],
            &[("Head", Joint)],
            &[("Head", Tip)],
            &[("Head", Ahead)],
            &[("Chest4", Ahead)],
            &[("Hips", Ahead)],
            // A side: clav, sh, elbow, wrist, index, knuck, pinky, fist, hip, knee, ankle, ball, toe.
            &[("{S}Collar", Joint)],
            &[("{S}Shoulder", Joint)],
            &[("{S}Elbow", Joint)],
            &[("{S}Wrist", Joint)],
            &[("{S}Wrist", Across(0.75, 1.0))],
            &[("{S}Wrist", Along(0.5))],
            &[("{S}Wrist", Across(0.75, -1.0))],
            &[("{S}Wrist", Along(0.75))],
            &[("{S}Hip", Joint)],
            &[("{S}Knee", Joint)],
            &[("{S}Ankle", Joint)],
            &[("{S}Toe", Joint)],
            &[("{S}Toe", Tip)],
        ],
    },
    Map {
        name: "motionbuilder",
        test: &["LeftUpLeg", "LeftForeArm"],
        sides: ["Left", "Right"],
        points: [
            &[("Hips", Joint)],
            &[("Spine", Joint)],
            &[("Spine1", Joint)],
            &[("Spine2", Joint), ("Spine1", Along(0.5))],
            &[("Neck", Joint)],
            &[("Head", Joint)],
            &[("HeadTop_End", Joint), ("Head", Tip)],
            &[("Head", Ahead)],
            &[("Spine2", Ahead), ("Spine1", Ahead)],
            &[("Hips", Ahead)],
            &[("{S}Shoulder", Joint)],
            &[("{S}Arm", Joint)],
            &[("{S}ForeArm", Joint)],
            &[("{S}Hand", Joint)],
            &[("{S}HandIndex1", Joint), ("{S}Hand", Across(0.75, 1.0))],
            &[("{S}HandMiddle1", Joint), ("{S}Hand", Along(0.5))],
            &[("{S}HandPinky1", Joint), ("{S}Hand", Across(0.75, -1.0))],
            &[("{S}HandMiddle2", Joint), ("{S}Hand", Along(0.75))],
            &[("{S}UpLeg", Joint)],
            &[("{S}Leg", Joint)],
            &[("{S}Foot", Joint)],
            &[("{S}ToeBase", Joint), ("{S}Toe", Joint)],
            &[("{S}Toe_End", Joint), ("{S}ToeBase", Tip), ("{S}Toe", Tip)],
        ],
    },
    Map {
        name: "bandai-namco",
        test: &["UpperLeg_L", "LowerArm_L"],
        sides: ["_L", "_R"],
        points: [
            &[("Hips", Joint)],
            &[("Spine", Joint)],
            &[("Spine", Along(0.5))],
            &[("Chest", Joint)],
            &[("Neck", Joint)],
            &[("Head", Joint)],
            &[("Head", Tip)],
            &[("Head", Ahead)],
            &[("Chest", Ahead)],
            &[("Hips", Ahead)],
            &[("Shoulder{S}", Joint)],
            &[("UpperArm{S}", Joint)],
            &[("LowerArm{S}", Joint)],
            &[("Hand{S}", Joint)],
            &[("Hand{S}", Across(0.75, 1.0))],
            &[("Hand{S}", Along(0.5))],
            &[("Hand{S}", Across(0.75, -1.0))],
            &[("Hand{S}", Along(0.75))],
            &[("UpperLeg{S}", Joint)],
            &[("LowerLeg{S}", Joint)],
            &[("Foot{S}", Joint)],
            &[("Toes{S}", Joint)],
            &[("Toes{S}", Tip)],
        ],
    },
    Map {
        name: "unreal",
        test: &["spine_01", "upperarm_l"],
        sides: ["_l", "_r"],
        points: [
            &[("pelvis", Joint)],
            &[("spine_01", Joint)],
            &[("spine_02", Joint)],
            &[("spine_03", Joint)],
            &[("neck_01", Joint)],
            &[("head", Joint)],
            &[("head", Tip)],
            &[("head", Ahead)],
            &[("spine_03", Ahead)],
            &[("pelvis", Ahead)],
            &[("clavicle{S}", Joint)],
            &[("upperarm{S}", Joint)],
            &[("lowerarm{S}", Joint)],
            &[("hand{S}", Joint)],
            &[("index_01{S}", Joint), ("hand{S}", Across(0.75, 1.0))],
            &[("middle_01{S}", Joint), ("hand{S}", Along(0.5))],
            &[("pinky_01{S}", Joint), ("hand{S}", Across(0.75, -1.0))],
            &[("middle_02{S}", Joint), ("hand{S}", Along(0.75))],
            &[("thigh{S}", Joint)],
            &[("calf{S}", Joint)],
            &[("foot{S}", Joint)],
            &[("ball{S}", Joint)],
            &[("ball_leaf{S}", Joint), ("ball{S}", Tip)],
        ],
    },
    Map {
        name: "rigify",
        test: &["DEF-hips", "DEF-upper_arm.L"],
        sides: [".L", ".R"],
        points: [
            &[("DEF-hips", Joint)],
            &[("DEF-spine.001", Joint)],
            &[("DEF-spine.002", Joint)],
            &[("DEF-spine.003", Joint)],
            &[("DEF-neck", Joint)],
            &[("DEF-head", Joint)],
            &[("DEF-head", Tip)],
            &[("DEF-head", Ahead)],
            &[("DEF-spine.003", Ahead)],
            &[("DEF-hips", Ahead)],
            &[("DEF-shoulder{S}", Joint)],
            &[("DEF-upper_arm{S}", Joint)],
            &[("DEF-forearm{S}", Joint)],
            &[("DEF-hand{S}", Joint)],
            &[("DEF-f_index.01{S}", Joint), ("DEF-hand{S}", Across(0.75, 1.0))],
            &[("DEF-f_middle.01{S}", Joint), ("DEF-hand{S}", Along(0.5))],
            &[("DEF-f_pinky.01{S}", Joint), ("DEF-hand{S}", Across(0.75, -1.0))],
            &[("DEF-f_middle.02{S}", Joint), ("DEF-hand{S}", Along(0.75))],
            &[("DEF-thigh{S}", Joint)],
            &[("DEF-shin{S}", Joint)],
            &[("DEF-foot{S}", Joint)],
            &[("DEF-toe{S}", Joint)],
            &[("DEF-toe{S}", Tip)],
        ],
    },
];

/// A BVH skeleton with its body points placed: the takes it reads and its rest body.
pub struct Rigged {
    pub bvh: Bvh,
    pub map: &'static str,
    /// Metres per file unit.
    pub scale: f64,
    /// Each body point: its joint, and where it sits in the joint's own frame (file units).
    at: Vec<(usize, DVec3)>,
    /// The frame the body was measured standing in (`None`: the zero pose).
    pub rest_frame: Option<usize>,
    /// How straight it stands there (1: legs straight down, feet level).
    pub stands: f64,
    pub body: Body,
}

/// How far past a joint whose bone has no length in the file (an end site of zero) its far end
/// is taken to be, along the bone leading to it: the head's top, a hand's fingertips, the toes'
/// tips (metres).
fn reach(point: usize) -> f64 {
    use super::readable::{BALL, FIST, HEAD_TOP, INDEX, KNUCK, PINKY, TOE};
    match point {
        HEAD_TOP => 0.18,
        _ if point >= 10 && [INDEX, KNUCK, PINKY, FIST].contains(&((point - 10) % 13)) => 0.18,
        _ if point >= 10 && [BALL, TOE].contains(&((point - 10) % 13)) => 0.06,
        _ => 0.1,
    }
}

/// How straight a body stands in a pose: both legs straight down (1 when they are), the feet
/// level, the head above the hips.
fn standing(pose: &[(DMat3, DVec3)], hips: [usize; 2], ankles: [usize; 2], legs: [f64; 2], pelvis: usize, head: usize) -> f64 {
    let down = |s: usize| (pose[hips[s]].1.y - pose[ankles[s]].1.y) / legs[s].max(1e-9);
    let level = (pose[ankles[0]].1.y - pose[ankles[1]].1.y).abs() / legs[0].max(1e-9);
    let upright = if pose[head].1.y > pose[pelvis].1.y { 0.0 } else { 1.0 };
    down(0).min(down(1)) - level - upright
}

impl Rigged {
    /// Places the body points on a parsed file (`units`: metres per file unit; found from the
    /// legs' length when absent).
    pub fn new(bvh: Bvh, units: Option<f64>) -> Result<Rigged> {
        let map = MAPS.iter().find(|m| m.test.iter().all(|t| bvh.find(t).is_some())).ok_or_else(|| {
            anyhow!(
                "the skeleton is not one the BVH reader knows (100STYLE's, MotionBuilder names: Hips, Spine, LeftUpLeg ..., or Bandai Namco's: UpperLeg_L ...); its joints: {}",
                bvh.names.join(", ")
            )
        })?;
        let mut at = Vec::with_capacity(POINTS.len());
        let mut missing = Vec::new();
        for (i, p) in POINTS.iter().enumerate() {
            let (alts, s) = match i {
                0..10 => (map.points[i], ""),
                _ => (map.points[10 + (i - 10) % 13], map.sides[if p.ends_with('L') { 0 } else { 1 }]),
            };
            match alts.iter().find_map(|(n, pl)| bvh.find(&n.replace("{S}", s)).map(|j| (j, *pl))) {
                Some(x) => at.push(x),
                None => missing.push(alts[0].0.replace("{S}", s)),
            }
        }
        if !missing.is_empty() {
            bail!("the {} map needs joints the file lacks: {}", map.name, missing.join(", "));
        }
        // The legs' length, joint to joint: the units, and how straight a pose stands.
        let zero = bvh.pose(None);
        let (hip, knee, ankle) = (super::readable::HIP, super::readable::KNEE, super::readable::ANKLE);
        let j = |s: usize, part: usize| at[super::readable::side(s, part)].0;
        let leg =
            |s: usize| (zero[j(s, hip)].1 - zero[j(s, knee)].1).length() + (zero[j(s, knee)].1 - zero[j(s, ankle)].1).length();
        let legs = [leg(0), leg(1)];
        let stands = |pose: &[(DMat3, DVec3)]| {
            standing(pose, [j(0, hip), j(1, hip)], [j(0, ankle), j(1, ankle)], legs, at[0].0, at[super::readable::HEAD].0)
        };
        // The rest: the zero pose when the body stands in it, else the frame it stands
        // straightest in (every tenth looked at, then the frames around the best).
        let zero_stands = stands(&zero);
        let (stood, rest_frame) = if zero_stands > 0.9 {
            (zero_stands, None)
        } else {
            let n = bvh.frames();
            let best = |frames: &mut dyn Iterator<Item = usize>| {
                frames.map(|f| (stands(&bvh.pose(Some(f))), f)).fold((f64::NEG_INFINITY, 0), |a, b| if b.0 > a.0 { b } else { a })
            };
            let (_, f) = best(&mut (0..n).step_by(10));
            let (score, f) = best(&mut (f.saturating_sub(9)..(f + 10).min(n)));
            (score, Some(f))
        };
        let rest = bvh.pose(rest_frame);
        // Units: the legs' length, read as a person's (about 85 cm).
        let scale = units.unwrap_or_else(|| {
            let l = (legs[0] + legs[1]) / 2.0;
            [1.0, 0.01, 0.001, 0.0254, 0.0254 / 0.45]
                .into_iter()
                .min_by(|a, b| (a * l / 0.85).ln().abs().total_cmp(&(b * l / 0.85).ln().abs()))
                .unwrap_or(0.01)
        });
        // A joint's far end in its own frame: the file's, else `reach` metres along the bone
        // leading to it, as it lies at rest.
        let tip_of = |jt: usize, point: usize| -> DVec3 {
            let t = bvh.tip(jt);
            if t.length() > 1e-9 {
                return t;
            }
            let mut p = bvh.parent[jt];
            while let Some(q) = p.filter(|&q| (rest[jt].1 - rest[q].1).length() < 1e-9) {
                p = bvh.parent[q];
            }
            let dir = p.map_or(DVec3::Y, |q| (rest[jt].1 - rest[q].1).normalize_or(DVec3::Y));
            rest[jt].0.transpose() * dir * (reach(point) / scale)
        };
        // Forward from the heel toward the toes, right toward the right hip.
        let world = |i: usize| -> DVec3 {
            let (jt, pl) = at[i];
            let (m, p) = rest[jt];
            let local = match pl {
                Tip => tip_of(jt, i),
                _ => DVec3::ZERO,
            };
            p + m * local
        };
        let (ankle_p, toe_p) =
            (world(super::readable::side(0, super::readable::ANKLE)), world(super::readable::side(0, super::readable::TOE)));
        let mut f = DVec3::new(toe_p.x - ankle_p.x, 0.0, toe_p.z - ankle_p.z).normalize_or(DVec3::Z);
        let (hl, hr) = (world(super::readable::side(0, hip)), world(super::readable::side(1, hip)));
        if rest_frame.is_some() {
            // A captured frame: a foot can point anywhere mid-stride, the hips cannot. Forward
            // is square to the hips, the foot only saying which way.
            let across = DVec3::new(hr.x - hl.x, 0.0, hr.z - hl.z);
            let square = DVec3::new(-across.z, 0.0, across.x).normalize_or(f);
            f = if square.dot(f) < 0.0 { -square } else { square };
        }
        let mut right = DVec3::new(f.z, 0.0, -f.x);
        if (hr.x - hl.x) * right.x + (hr.z - hl.z) * right.z < 0.0 {
            right = -right;
        }
        // Each point's place in its joint's frame: forward, written in that frame at rest.
        let at: Vec<(usize, DVec3)> = at
            .iter()
            .enumerate()
            .map(|(i, &(jt, pl))| {
                let ahead = rest[jt].0.transpose() * f / scale;
                let local = match pl {
                    Joint => DVec3::ZERO,
                    Tip => tip_of(jt, i),
                    Along(t) => tip_of(jt, i) * t,
                    Ahead => ahead * 0.12,
                    Across(t, sd) => tip_of(jt, i) * t + ahead * (0.035 * sd),
                };
                (jt, local)
            })
            .collect();
        let mut r = Rigged {
            bvh,
            map: map.name,
            scale,
            at,
            rest_frame,
            stands: stood,
            body: Body { rest: Vec::new(), fwd: [0.0; 3], right: [0.0; 3] },
        };
        r.body = Body { rest: r.place(&rest), fwd: f.to_array(), right: right.to_array() };
        Ok(r)
    }

    /// Places and measures the body as `like` does, when it is the same skeleton (the take that
    /// stands straightest gives every take of a library its rest). Returns whether it did.
    pub fn adopt(&mut self, like: &Rigged) -> bool {
        if self.bvh.names != like.bvh.names || self.bvh.offset != like.bvh.offset {
            return false;
        }
        self.map = like.map;
        self.scale = like.scale;
        self.at = like.at.clone();
        self.rest_frame = like.rest_frame;
        self.stands = like.stands;
        self.body = Body { rest: like.body.rest.clone(), fwd: like.body.fwd, right: like.body.right };
        true
    }

    /// The body points (metres) from a pose of the joints.
    fn place(&self, pose: &[(DMat3, DVec3)]) -> Vec<V> {
        self.at
            .iter()
            .map(|&(j, local)| {
                let (m, p) = pose[j];
                ((p + m * local) * self.scale).to_array()
            })
            .collect()
    }
}

/// One take of a BVH skeleton.
pub struct BvhTake<'a>(pub &'a Rigged);

impl Take for BvhTake<'_> {
    fn frames(&self) -> usize {
        self.0.bvh.frames()
    }
    fn points(&self, frame: usize) -> Vec<V> {
        self.0.place(&self.0.bvh.pose(Some(frame)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: &str = "HIERARCHY
ROOT Hips
{
 OFFSET 0 0 0
 CHANNELS 6 Xposition Yposition Zposition Zrotation Xrotation Yrotation
 JOINT Spine { OFFSET 0 10 0 CHANNELS 3 Zrotation Xrotation Yrotation
  JOINT Spine1 { OFFSET 0 10 0 CHANNELS 3 Zrotation Xrotation Yrotation
   JOINT Spine2 { OFFSET 0 10 0 CHANNELS 3 Zrotation Xrotation Yrotation
    JOINT Neck { OFFSET 0 15 0 CHANNELS 3 Zrotation Xrotation Yrotation
     JOINT Head { OFFSET 0 10 0 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 0 18 0 } } }
    JOINT LeftShoulder { OFFSET 3 12 0 CHANNELS 3 Zrotation Xrotation Yrotation
     JOINT LeftArm { OFFSET 15 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
      JOINT LeftForeArm { OFFSET 28 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
       JOINT LeftHand { OFFSET 25 0 0 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 18 0 0 } } } } }
    JOINT RightShoulder { OFFSET -3 12 0 CHANNELS 3 Zrotation Xrotation Yrotation
     JOINT RightArm { OFFSET -15 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
      JOINT RightForeArm { OFFSET -28 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
       JOINT RightHand { OFFSET -25 0 0 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET -18 0 0 } } } } } } } }
 JOINT LeftUpLeg { OFFSET 10 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
  JOINT LeftLeg { OFFSET 0 -44 0 CHANNELS 3 Zrotation Xrotation Yrotation
   JOINT LeftFoot { OFFSET 0 -43 0 CHANNELS 3 Zrotation Xrotation Yrotation
    JOINT LeftToeBase { OFFSET 0 -8 14 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 0 0 6 } } } } }
 JOINT RightUpLeg { OFFSET -10 0 0 CHANNELS 3 Zrotation Xrotation Yrotation
  JOINT RightLeg { OFFSET 0 -44 0 CHANNELS 3 Zrotation Xrotation Yrotation
   JOINT RightFoot { OFFSET 0 -43 0 CHANNELS 3 Zrotation Xrotation Yrotation
    JOINT RightToeBase { OFFSET 0 -8 14 CHANNELS 3 Zrotation Xrotation Yrotation End Site { OFFSET 0 0 6 } } } } }
}
MOTION
Frames: 2
Frame Time: 0.5
";

    fn tiny() -> Rigged {
        let joints = 22;
        let mut text = TINY.to_string();
        for f in 0..2 {
            let mut v = vec![0.0f64; 6 + 3 * (joints - 1)];
            v[1] = 95.0;
            v[3] = 0.0;
            if f == 1 {
                // The left elbow (LeftForeArm, joint 8) bends 90 degrees about y.
                v[6 + 3 * 7 + 2] = 90.0;
            }
            text += &v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
            text += "\n";
        }
        Rigged::new(Bvh::parse(&text).unwrap(), None).unwrap()
    }

    #[test]
    fn reads_a_motionbuilder_skeleton() {
        let r = tiny();
        assert_eq!(r.map, "motionbuilder");
        assert_eq!(r.bvh.frames(), 2);
        assert!((r.scale - 0.01).abs() < 1e-12, "centimetres: {}", r.scale);
        // Faces +z (heel to toe), the right hip toward -x.
        assert!((DVec3::from(r.body.fwd) - DVec3::Z).length() < 1e-9);
        assert!((DVec3::from(r.body.right) + DVec3::X).length() < 1e-9);
        let rest = BvhTake(&r).points(0);
        let p = |n: &str| DVec3::from(rest[super::super::readable::index(n).unwrap()]);
        assert!((p("pelvis").y - 0.95).abs() < 1e-9);
        assert!(((p("shL") - p("elbowL")).length() - 0.28).abs() < 1e-9);
        assert!((p("toeL").z - 0.20).abs() < 1e-9, "the toe is the end site: {}", p("toeL"));
        // The bent elbow swings the forearm round the vertical.
        let bent = BvhTake(&r).points(1);
        let q = |n: &str| DVec3::from(bent[super::super::readable::index(n).unwrap()]);
        let fore = q("wristL") - q("elbowL");
        assert!(fore.x.abs() < 1e-9 && (fore.z.abs() - 0.25).abs() < 1e-9, "forearm turned 90 degrees: {fore}");
    }
}
