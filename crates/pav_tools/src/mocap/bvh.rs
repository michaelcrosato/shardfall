//! BioVision hierarchy files (.bvh), the plain-text format most motion-capture datasets ship
//! in: a skeleton (joints, offsets, channels), then every frame's channel values. Forward
//! kinematics gives each joint's world rotation and position; the 36 body points are placed on
//! the joints by a bone map, so the takes are cut like the CMU database's (`super::takes`).
//!
//! Maps: 100STYLE's skeleton (Hips, Chest .. Chest4, Neck, Head, RightCollar, RightShoulder,
//! RightElbow, RightWrist, RightHip, RightKnee, RightAnkle, RightToe), and the MotionBuilder
//! names Mixamo, LaFAN1 and many converters use (Hips, Spine, Spine1, Spine2, Neck, Head,
//! LeftShoulder, LeftArm, LeftForeArm, LeftHand, LeftUpLeg, LeftLeg, LeftFoot, LeftToeBase).
//! A map is a table of joint names per body point, with fallbacks: another skeleton is one more
//! table in `MAPS`.

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

    pub fn frames(&self) -> usize {
        self.values.len() / self.width
    }

    pub fn joints(&self) -> &[String] {
        &self.names
    }

    /// Each joint's world rotation and position (file units) at a frame (`None`: the rest pose).
    fn pose(&self, frame: Option<usize>) -> Vec<(DMat3, DVec3)> {
        let mut out: Vec<(DMat3, DVec3)> = Vec::with_capacity(self.names.len());
        for j in 0..self.names.len() {
            let (mut r, mut t) = (DMat3::IDENTITY, self.offset[j]);
            if let Some(f) = frame {
                let v = &self.values[f * self.width..];
                for (k, c) in self.chans[j].iter().enumerate() {
                    let x = v[self.first[j] + k];
                    match *c {
                        Chan::Pos(a) => t[a] += x,
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
/// `{S}` is the side: Left or Right.
struct Map {
    name: &'static str,
    /// Joints only this skeleton has.
    test: &'static [&'static str],
    points: [&'static [(&'static str, Place)]; 23],
}

use Place::*;

const MAPS: [Map; 2] = [
    Map {
        name: "100style",
        test: &["Chest4", "RightCollar"],
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
];

/// A BVH skeleton with its body points placed: the takes it reads and its rest body.
pub struct Rigged {
    pub bvh: Bvh,
    pub map: &'static str,
    /// Metres per file unit.
    pub scale: f64,
    at: Vec<(usize, Place)>,
    fwd_local: DVec3,
    pub body: Body,
}

impl Rigged {
    /// Places the body points on a parsed file (`units`: metres per file unit; found from the
    /// legs' length when absent).
    pub fn new(bvh: Bvh, units: Option<f64>) -> Result<Rigged> {
        let map = MAPS.iter().find(|m| m.test.iter().all(|t| bvh.find(t).is_some())).ok_or_else(|| {
            anyhow!(
                "the skeleton is not one the BVH reader knows (100STYLE's, or MotionBuilder names: Hips, Spine, LeftUpLeg ...); its joints: {}",
                bvh.names.join(", ")
            )
        })?;
        let mut at = Vec::with_capacity(POINTS.len());
        let mut missing = Vec::new();
        for (i, p) in POINTS.iter().enumerate() {
            let (alts, s) = match i {
                0..10 => (map.points[i], ""),
                _ => (map.points[10 + (i - 10) % 13], if p.ends_with('L') { "Left" } else { "Right" }),
            };
            match alts.iter().find_map(|(n, pl)| bvh.find(&n.replace("{S}", s)).map(|j| (j, *pl))) {
                Some(x) => at.push(x),
                None => missing.push(alts[0].0.replace("{S}", s)),
            }
        }
        if !missing.is_empty() {
            bail!("the {} map needs joints the file lacks: {}", map.name, missing.join(", "));
        }
        let rest = bvh.pose(None);
        // Units: the legs' length at rest, read as a person's (about 85 cm).
        let leg = |s: usize| {
            let (h, a) =
                (at[super::readable::side(s, super::readable::HIP)].0, at[super::readable::side(s, super::readable::ANKLE)].0);
            (rest[h].1 - rest[a].1).length()
        };
        let scale = units.unwrap_or_else(|| {
            let l = (leg(0) + leg(1)) / 2.0;
            [1.0, 0.01, 0.001, 0.0254, 0.0254 / 0.45]
                .into_iter()
                .min_by(|a, b| (a * l / 0.85).ln().abs().total_cmp(&(b * l / 0.85).ln().abs()))
                .unwrap_or(0.01)
        });
        let mut r = Rigged {
            bvh,
            map: map.name,
            scale,
            at,
            fwd_local: DVec3::Z,
            body: Body { rest: Vec::new(), fwd: [0.0; 3], right: [0.0; 3] },
        };
        // Forward from the heel toward the toes, right toward the right hip.
        let pts = r.place(&rest);
        let (ankle, toe) =
            (pts[super::readable::side(0, super::readable::ANKLE)], pts[super::readable::side(0, super::readable::TOE)]);
        let f = DVec3::new(toe[0] - ankle[0], 0.0, toe[2] - ankle[2]).normalize_or(DVec3::Z);
        let mut right = DVec3::new(f.z, 0.0, -f.x);
        let (hl, hr) = (pts[super::readable::side(0, super::readable::HIP)], pts[super::readable::side(1, super::readable::HIP)]);
        if (hr[0] - hl[0]) * right.x + (hr[2] - hl[2]) * right.z < 0.0 {
            right = -right;
        }
        r.fwd_local = f;
        r.body = Body { rest: r.place(&rest), fwd: f.to_array(), right: right.to_array() };
        Ok(r)
    }

    /// The body points (metres) from a pose of the joints.
    fn place(&self, pose: &[(DMat3, DVec3)]) -> Vec<V> {
        let ahead = self.fwd_local * (0.12 / self.scale);
        let across = self.fwd_local * (0.035 / self.scale);
        self.at
            .iter()
            .map(|&(j, pl)| {
                let (m, p) = pose[j];
                let tip = self.bvh.tip(j);
                let local = match pl {
                    Joint => DVec3::ZERO,
                    Tip => tip,
                    Along(t) => tip * t,
                    Ahead => ahead,
                    Across(t, s) => tip * t + across * s,
                };
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
