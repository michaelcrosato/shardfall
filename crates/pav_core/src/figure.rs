//! Imported figures: rigged characters from model libraries (KayKit's adventurers), drawn as
//! rigid per-bone segments that follow the biped puppet's skeleton, so every clip, move and
//! procedural walk the puppets play plays on them (docs/AGENT_ASSET_STORE.md §4, S3b).
//!
//! A figure is two readable files in a library pack, both written by `asset_import`:
//! - the body, `<pack>/<name>.json`: a format-2 prop of category `character` whose parts are
//!   the segments, one mesh part each (`meshes/<name>_<segment>.json`), named after the
//!   segment and placed in a standing display pose (what the Catalogue and previews show);
//! - the rig, `<pack>/rigs/<name>.json` (`RigFile`): which of the model's bones each segment
//!   takes, each segment's rest joint and tail (cm, the engine's frame: feet on the floor,
//!   facing +z), and the look's proportions read from the bones (a `PuppetDef` fragment, m).
//!
//! A segment's mesh is written in its own frame: the origin at its joint, the torso's and the
//! head's axes the character's, a limb's +y along the bone and +z the way its middle joint
//! bends out (elbows back, knees forward), a foot's +y forward and +z up. Posing sets each
//! frame from the puppet's `Skel`: the torso and head turn with the pelvis, chest and head; a
//! limb's root follows its parent, its end the puppet's hand or ankle, and its middle joint is
//! solved again at the figure's own (unequal) bone lengths; the body then stands with its
//! lowest point where the puppet's is. Nothing is skinned: each segment moves whole.
//!
//! Sides: the puppet carries a clip's right side at +x (facing +z), a mirror image of the
//! capture. A figure keeps its own mesh, so its limbs are matched to the puppet's by where they
//! sit, not by name: the chain whose rest joint is at +x follows the puppet's index 1. A glTF
//! character faces +z with its left side at +x, so it performs clips as the puppets do (both
//! mirror images of the captures) and its motion agrees with the procedural walk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use glam::{Mat3, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::issue::{Code, Issue};
use crate::props::geom::Primitive;
use crate::props::{self, PropDef};
use crate::puppet::{PuppetDef, Skel};

/// The segments of the engine's figure body plan, in drawing order. `L` and `R` are the rig's
/// own sides (VRM's left and right).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Seg {
    Hips,
    Spine,
    Chest,
    Head,
    UpperArmL,
    LowerArmL,
    HandL,
    UpperArmR,
    LowerArmR,
    HandR,
    UpperLegL,
    LowerLegL,
    FootL,
    UpperLegR,
    LowerLegR,
    FootR,
}

impl Seg {
    pub const ALL: [Seg; 16] = [
        Seg::Hips,
        Seg::Spine,
        Seg::Chest,
        Seg::Head,
        Seg::UpperArmL,
        Seg::LowerArmL,
        Seg::HandL,
        Seg::UpperArmR,
        Seg::LowerArmR,
        Seg::HandR,
        Seg::UpperLegL,
        Seg::LowerLegL,
        Seg::FootL,
        Seg::UpperLegR,
        Seg::LowerLegR,
        Seg::FootR,
    ];
    pub const NAMES: [&'static str; 16] = [
        "hips",
        "spine",
        "chest",
        "head",
        "upper_arm_l",
        "lower_arm_l",
        "hand_l",
        "upper_arm_r",
        "lower_arm_r",
        "hand_r",
        "upper_leg_l",
        "lower_leg_l",
        "foot_l",
        "upper_leg_r",
        "lower_leg_r",
        "foot_r",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn parse(name: &str) -> Option<Seg> {
        Self::NAMES.iter().position(|n| *n == name).map(|i| Self::ALL[i])
    }

    /// Every segment but the chest, which a rig without one folds into its spine (VRM 1.0's
    /// required bones, one segment each).
    pub fn required(self) -> bool {
        self != Seg::Chest
    }

    /// The segment a VRM 1.0 humanoid bone's triangles go to: shoulders, the upper chest and
    /// the neck ride with the chest, the jaw and eyes with the head, fingers with the hand,
    /// toes with the foot. None for a bone the plan has no place for.
    pub fn of_vrm(bone: &str) -> Option<Seg> {
        let (side, part) = if let Some(p) = bone.strip_prefix("left") {
            (Some(0), p)
        } else if let Some(p) = bone.strip_prefix("right") {
            (Some(1), p)
        } else {
            (None, bone)
        };
        let pick = |l: Seg, r: Seg| if side == Some(0) { l } else { r };
        Some(match (side, part) {
            (None, "hips") => Seg::Hips,
            (None, "spine") => Seg::Spine,
            (None, "chest" | "upperChest" | "neck") => Seg::Chest,
            (None, "head" | "jaw") => Seg::Head,
            (Some(_), "Shoulder") => Seg::Chest,
            (Some(_), "Eye") => Seg::Head,
            (Some(_), "UpperArm") => pick(Seg::UpperArmL, Seg::UpperArmR),
            (Some(_), "LowerArm") => pick(Seg::LowerArmL, Seg::LowerArmR),
            (Some(_), "UpperLeg") => pick(Seg::UpperLegL, Seg::UpperLegR),
            (Some(_), "LowerLeg") => pick(Seg::LowerLegL, Seg::LowerLegR),
            (Some(_), "Foot" | "Toes") => pick(Seg::FootL, Seg::FootR),
            (Some(_), p)
                if p == "Hand"
                    || p.starts_with("Thumb")
                    || ["Index", "Middle", "Ring", "Little"].iter().any(|f| p.starts_with(f)) =>
            {
                pick(Seg::HandL, Seg::HandR)
            }
            _ => return None,
        })
    }

    /// The VRM bone whose joint is the segment's.
    pub fn vrm(self) -> &'static str {
        match self {
            Seg::Hips => "hips",
            Seg::Spine => "spine",
            Seg::Chest => "chest",
            Seg::Head => "head",
            Seg::UpperArmL => "leftUpperArm",
            Seg::LowerArmL => "leftLowerArm",
            Seg::HandL => "leftHand",
            Seg::UpperArmR => "rightUpperArm",
            Seg::LowerArmR => "rightLowerArm",
            Seg::HandR => "rightHand",
            Seg::UpperLegL => "leftUpperLeg",
            Seg::LowerLegL => "leftLowerLeg",
            Seg::FootL => "leftFoot",
            Seg::UpperLegR => "rightUpperLeg",
            Seg::LowerLegR => "rightLowerLeg",
            Seg::FootR => "rightFoot",
        }
    }

    /// The segment it hangs from (None: the hips).
    pub fn parent(self) -> Option<Seg> {
        use Seg::*;
        Some(match self {
            Hips => return None,
            Spine | UpperLegL | UpperLegR => Hips,
            Chest => Spine,
            Head | UpperArmL | UpperArmR => Chest,
            LowerArmL => UpperArmL,
            HandL => LowerArmL,
            LowerArmR => UpperArmR,
            HandR => LowerArmR,
            LowerLegL => UpperLegL,
            FootL => LowerLegL,
            LowerLegR => UpperLegR,
            FootR => LowerLegR,
        })
    }

    /// The limb chain a segment belongs to: (root, middle, end) and whether it is an arm.
    fn chain(self) -> Option<([Seg; 3], bool)> {
        use Seg::*;
        match self {
            UpperArmL | LowerArmL | HandL => Some(([UpperArmL, LowerArmL, HandL], true)),
            UpperArmR | LowerArmR | HandR => Some(([UpperArmR, LowerArmR, HandR], true)),
            UpperLegL | LowerLegL | FootL => Some(([UpperLegL, LowerLegL, FootL], false)),
            UpperLegR | LowerLegR | FootR => Some(([UpperLegR, LowerLegR, FootR], false)),
            _ => None,
        }
    }
}

/// The four limb chains: (root, middle, end), arm.
const CHAINS: [([Seg; 3], bool); 4] = [
    ([Seg::UpperArmL, Seg::LowerArmL, Seg::HandL], true),
    ([Seg::UpperArmR, Seg::LowerArmR, Seg::HandR], true),
    ([Seg::UpperLegL, Seg::LowerLegL, Seg::FootL], false),
    ([Seg::UpperLegR, Seg::LowerLegR, Seg::FootR], false),
];

/// The look's proportions, read from the bones: the `PuppetDef` fields a figure sets (m).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proportions {
    pub scale: f32,
    pub leg_length: f32,
    pub arm_length: f32,
    pub torso_length: f32,
    pub torso_radius: f32,
    pub hip_width: f32,
    pub shoulder_width: f32,
    pub head_radius: f32,
    pub limb_radius: f32,
}

/// One segment of a rig file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegFile {
    /// The model's bones whose triangles it took, its own first.
    pub bones: Vec<String>,
    /// Its rest joint and the end of its bone (the next joint, or for the head, a hand or a
    /// foot the far end of its mesh along the bone), cm.
    pub joint: [f32; 3],
    pub tail: [f32; 3],
    /// Triangles in its mesh.
    pub tris: u32,
}

/// A figure's rig file (`<pack>/rigs/<name>.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigFile {
    /// 1.
    pub format: u32,
    /// The figure's name (the body's leaf).
    pub name: String,
    /// The body prop, `LIB/<pack>/<name>`.
    pub body: String,
    /// `cm`: the unit of joints and tails.
    pub unit: String,
    /// The bone-name table that recognised the rig (`kaykit`, `mixamo`, `vrm`...).
    pub table: String,
    pub look: Proportions,
    /// By segment name.
    pub segments: BTreeMap<String, SegFile>,
    /// Bones with no segment of their own (helpers, slots) and the rigid meshes kept (helmets,
    /// capes), by the segment their triangles went to: `hand.l > hand_l`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folded: Vec<String>,
    /// Meshes left out: things held in a hand slot (the engine draws a look's weapon).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<String>,
}

impl RigFile {
    pub fn parse(text: &str) -> Result<RigFile, String> {
        let rig: RigFile = serde_json::from_str(text).map_err(|e| format!("not a rig file: {e}"))?;
        if rig.format != 1 {
            return Err(format!("format {} (this reader reads 1)", rig.format));
        }
        if rig.unit != "cm" {
            return Err(format!("unit \"{}\": joints are in cm", rig.unit));
        }
        if let Some(bad) = rig.segments.keys().find(|k| Seg::parse(k).is_none()) {
            return Err(format!("segment \"{bad}\" is not one of {}", Seg::NAMES.join(", ")));
        }
        Ok(rig)
    }

    /// The canonical text: one segment a line.
    pub fn text(&self) -> String {
        let mut out = String::from("{");
        let head = serde_json::json!({
            "format": self.format, "name": self.name, "body": self.body, "unit": self.unit, "table": self.table,
        });
        for (k, v) in head.as_object().unwrap() {
            out.push_str(&format!("{}:{},", serde_json::json!(k), v));
        }
        out.push_str(&format!("\n\"look\":{},\n\"segments\":{{", serde_json::to_string(&self.look).unwrap()));
        let segs: Vec<String> = Seg::ALL
            .iter()
            .filter_map(|s| self.segments.get(s.name()).map(|f| (s, f)))
            .map(|(s, f)| format!("\n{}:{}", serde_json::json!(s.name()), serde_json::to_string(f).unwrap()))
            .collect();
        out.push_str(&segs.join(","));
        out.push('}');
        if !self.folded.is_empty() {
            out.push_str(&format!(",\n\"folded\":{}", serde_json::json!(self.folded)));
        }
        if !self.left_out.is_empty() {
            out.push_str(&format!(",\n\"left_out\":{}", serde_json::json!(self.left_out)));
        }
        out.push_str("}\n");
        out
    }

    fn seg(&self, s: Seg) -> Option<&SegFile> {
        self.segments.get(s.name())
    }

    /// A segment's rest joint and tail, metres.
    pub fn joint(&self, s: Seg) -> Option<(Vec3, Vec3)> {
        self.seg(s).map(|f| (Vec3::from(f.joint) * 0.01, Vec3::from(f.tail) * 0.01))
    }
}

/// A segment's frame in character space (x right, y up, z forward, the feet at the origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegPose {
    pub seg: Seg,
    pub rot: Quat,
    pub pos: Vec3,
}

/// `v` without its part along `d`, as a unit vector (None when nothing is left).
fn across(v: Vec3, d: Vec3) -> Option<Vec3> {
    let p = v - d * v.dot(d);
    (p.length_squared() > 1e-10).then(|| p.normalize())
}

/// The frame whose +y is `dir` and +z is `pole` made perpendicular to it (`fallback` when the
/// pole lies along the bone).
fn frame(dir: Vec3, pole: Vec3, fallback: Vec3) -> Quat {
    let y = dir.normalize_or(Vec3::Y);
    let z = across(pole, y).or_else(|| across(fallback, y)).or_else(|| across(Vec3::Z, y)).unwrap_or(Vec3::X);
    Quat::from_mat3(&Mat3::from_cols(y.cross(z), y, z)).normalize()
}

/// The rest frame of each segment, as its mesh is written: the torso and head in the
/// character's axes, a limb along its bone with +z where its middle joint bends out (an elbow
/// back, a knee forward), a foot forward with +z up, a hand in its forearm's frame.
pub fn rest_frame(rig: &RigFile, s: Seg) -> Option<(Quat, Vec3)> {
    let (joint, tail) = rig.joint(s)?;
    let rot = match s.chain() {
        None => Quat::IDENTITY,
        Some(([_, mid, end], arm)) => {
            let pole = if arm { Vec3::NEG_Z } else { Vec3::Z };
            if s == end && !arm {
                frame(Vec3::Z, Vec3::Y, Vec3::Y)
            } else if s == end {
                rest_frame(rig, mid)?.0
            } else {
                frame(tail - joint, pole, Vec3::Y)
            }
        }
    };
    Some((rot, joint))
}

/// Which of the puppet's sides a chain follows: the one it sits on (+x is index 1).
fn puppet_side(rig: &RigFile, root: Seg) -> usize {
    rig.joint(root).map_or(0, |(j, _)| usize::from(j.x >= 0.0))
}

/// The puppet look `def` with the figure's proportions (a biped at the figure's own size).
pub fn fit(rig: &RigFile, def: &PuppetDef) -> PuppetDef {
    let l = &rig.look;
    PuppetDef {
        scale: l.scale,
        leg_length: l.leg_length,
        arm_length: l.arm_length,
        torso_length: l.torso_length,
        torso_radius: l.torso_radius,
        hip_width: l.hip_width,
        shoulder_width: l.shoulder_width,
        head_radius: l.head_radius,
        limb_radius: l.limb_radius,
        body: crate::puppet::BodyPlan::Biped,
        ..def.clone()
    }
}

/// Each segment's frame for skeleton `sk` (made with `fit`'s look), in character space. `low`
/// gives a segment's points nearest the floor in its own frame (its mesh's extremes): the body
/// stands with its lowest point where the puppet's lowest is.
pub fn pose(rig: &RigFile, def: &PuppetDef, sk: &Skel, low: &dyn Fn(Seg) -> Vec<Vec3>) -> Vec<SegPose> {
    let j = |s: Seg| rig.joint(s).map(|(j, _)| j);
    let mut out: Vec<SegPose> = Vec::with_capacity(16);
    let (qp, qc, qh) = (sk.pelvis_rot, sk.chest_rot, sk.head_rot);
    let has = |s: Seg| rig.seg(s).is_some();
    let q_spine = if has(Seg::Chest) { qp.slerp(qc, 0.5) } else { qc };
    // The middle of the hip joints at rest is the puppet's pelvis.
    let pm = match (j(Seg::UpperLegL), j(Seg::UpperLegR)) {
        (Some(a), Some(b)) => (a + b) * 0.5,
        _ => j(Seg::Hips).unwrap_or(Vec3::ZERO),
    };
    let hips_j = j(Seg::Hips).unwrap_or(pm);
    let hips = SegPose { seg: Seg::Hips, rot: qp, pos: sk.pelvis + qp * (hips_j - pm) };
    let spine_j = j(Seg::Spine).unwrap_or(hips_j);
    let spine = SegPose { seg: Seg::Spine, rot: q_spine, pos: hips.pos + qp * (spine_j - hips_j) };
    let (chest, chest_j) = match j(Seg::Chest) {
        Some(cj) => (SegPose { seg: Seg::Chest, rot: qc, pos: spine.pos + q_spine * (cj - spine_j) }, cj),
        None => (spine, spine_j),
    };
    for p in [hips, spine, chest] {
        if has(p.seg) {
            out.push(p);
        }
    }
    if let Some(hj) = j(Seg::Head) {
        out.push(SegPose { seg: Seg::Head, rot: qh, pos: chest.pos + chest.rot * (hj - chest_j) });
    }
    let k = def.scale;
    for ([root, mid, end], arm) in CHAINS {
        let (Some(rj), Some(mj), Some(ej)) = (j(root), j(mid), j(end)) else { continue };
        let i = puppet_side(rig, root);
        let s = if i == 1 { 1.0 } else { -1.0 };
        let (l1, l2) = ((mj - rj).length().max(1e-3), (ej - mj).length().max(1e-3));
        // The root rides with its parent; the puppet's shrug and reach move a shoulder on.
        let (start, reach, hint, toe) = if arm {
            let fk = chest.pos + chest.rot * (rj - chest_j);
            let rest = sk.chest + qc * Vec3::new(s * def.shoulder_width * k, -0.05 * k, 0.0);
            let start = fk + (sk.shoulder[i] - rest);
            (start, sk.hand[i] - sk.shoulder[i], sk.elbow[i] - (sk.shoulder[i] + sk.hand[i]) * 0.5, None)
        } else {
            let start = hips.pos + qp * (rj - hips_j);
            (start, sk.ankle[i] - sk.hip[i], sk.knee[i] - (sk.hip[i] + sk.ankle[i]) * 0.5, Some(sk.toe[i]))
        };
        // The parent's rest pole, carried along, for a limb held dead straight.
        let parent = if arm { chest.rot } else { qp };
        let fallback = parent * if arm { Vec3::NEG_Z } else { Vec3::Z };
        // Unequal bones cannot fold flat: the end stays at least their difference away, so
        // both keep their lengths (the puppet's equal halves can).
        let least = (l1 - l2).abs() * 1.001 + 1e-4;
        let reach = if reach.length() < least { reach.normalize_or(parent * Vec3::NEG_Y) * least } else { reach };
        let pole = if hint.length_squared() > 1e-10 { hint } else { fallback };
        let (m, e) = crate::puppet::ik(start, start + reach, l1, l2, pole);
        let upper = frame(m - start, hint, fallback);
        let lower = frame(e - m, hint, fallback);
        if has(root) {
            out.push(SegPose { seg: root, rot: upper, pos: start });
        }
        if has(mid) {
            out.push(SegPose { seg: mid, rot: lower, pos: m });
        }
        if has(end) {
            let rot = match toe {
                Some(toe) => frame(toe, qp * Vec3::Y, lower * Vec3::Y),
                None => lower,
            };
            out.push(SegPose { seg: end, rot, pos: e });
        }
    }
    // Stand the body's lowest point where the puppet's lowest point is.
    let ours = out.iter().flat_map(|p| low(p.seg).into_iter().map(move |v| (p.pos + p.rot * v).y)).fold(f32::INFINITY, f32::min);
    if ours.is_finite() {
        let lift = crate::clips::lowest(def, sk) - ours;
        for p in &mut out {
            p.pos.y += lift;
        }
    }
    out.sort_by_key(|p| p.seg);
    out
}

/// The key pose a figure's body is written in: standing, arms down a little out from the body.
pub fn display_key() -> crate::clips::Key {
    crate::clips::Key {
        hips: [0.0, 0.0, 100.0],
        arm_l: [6.0, 22.0, -100.0, 12.0, 0.0],
        arm_r: [6.0, 22.0, -100.0, 12.0, 0.0],
        leg_l: [0.0, 4.0, -100.0, 4.0, 0.0],
        leg_r: [0.0, 4.0, -100.0, 4.0, 0.0],
        ..Default::default()
    }
}

/// The points of a segment's mesh nearest each of 14 directions (the 6 axes and 8 diagonals),
/// in its own frame: enough to find where a posed body meets the floor.
pub fn extremes(points: &[Vec3]) -> Vec<Vec3> {
    let mut dirs = vec![Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
    for i in 0..8 {
        let sign = |b: usize| if i & b == 0 { 1.0 } else { -1.0 };
        dirs.push(Vec3::new(sign(1), sign(2), sign(4)));
    }
    let mut out: Vec<Vec3> = Vec::new();
    for d in dirs {
        if let Some(p) = points.iter().copied().max_by(|a, b| a.dot(d).total_cmp(&b.dot(d))) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// A figure ready to draw: its rig, its body prop and, per segment, the body part that draws
/// it (its display transform) and its mesh's extremes.
pub struct Figure {
    /// The body's name, `LIB/<pack>/<name>`.
    pub name: String,
    pub rig: RigFile,
    pub body: Arc<PropDef>,
    /// Per segment: the index of its part in `body.compiled.parts` and its points' extremes.
    parts: BTreeMap<Seg, (usize, Vec<Vec3>)>,
}

impl Figure {
    /// A figure from its rig and body (the body's parts named after the segments).
    pub fn new(name: &str, rig: RigFile, body: Arc<PropDef>) -> Result<Figure, String> {
        let mut parts = BTreeMap::new();
        for (i, part) in body.compiled.parts.iter().enumerate() {
            let Some(seg) = Seg::parse(&part.id) else { continue };
            if rig.seg(seg).is_none() {
                return Err(format!("{name}: the body's part {} has no segment in the rig", part.id));
            }
            let points = match &part.prim {
                Primitive::Mesh { mesh, .. } => extremes(&mesh.mesh.positions),
                prim => extremes(&prim.hull()),
            };
            parts.insert(seg, (i, points));
        }
        Ok(Figure { name: name.to_string(), rig, body, parts })
    }

    /// `def` at the figure's proportions.
    pub fn fit(&self, def: &PuppetDef) -> PuppetDef {
        fit(&self.rig, def)
    }

    /// Each segment's frame for skeleton `sk` (made with `fit`'s look).
    pub fn pose(&self, def: &PuppetDef, sk: &Skel) -> Vec<SegPose> {
        pose(&self.rig, def, sk, &|s| self.parts.get(&s).map(|(_, p)| p.clone()).unwrap_or_default())
    }

    /// The body parts to draw for `poses`: each part's index and the transform (rotation,
    /// translation) that carries the part, as the body places it, onto its posed segment:
    /// `pose · display⁻¹`. Applied before the part's own transform.
    pub fn part_transforms(&self, poses: &[SegPose]) -> Vec<(usize, Quat, Vec3)> {
        poses
            .iter()
            .filter_map(|p| {
                let (i, _) = self.parts.get(&p.seg)?;
                let part = &self.body.compiled.parts[*i];
                let inv = part.rot.inverse();
                let rot = p.rot * inv;
                Some((*i, rot, p.pos - rot * part.pos))
            })
            .collect()
    }

    /// The segments the body draws.
    pub fn segments(&self) -> Vec<Seg> {
        self.parts.keys().copied().collect()
    }

    /// A segment's points nearest the floor, in its own frame.
    pub fn extremes(&self, s: Seg) -> &[Vec3] {
        self.parts.get(&s).map_or(&[], |(_, p)| p)
    }
}

/// Whether a prop lint finding does not apply to `def` because it is a figure's body: its
/// segments come in left and right pairs whose meshes keep their source's asymmetry (a bracer
/// on one arm), which the `mirror` lint, written for authored props, would flag.
pub fn exempt(def: &PropDef, finding: &props::lint::Finding) -> bool {
    def.doc.category == Some(props::doc::Category::Character) && finding.code == "mirror"
}

/// Where a figure's rig file sits under the props root: `<tier>/<pack>/rigs/<name>.json`.
pub fn rig_file(root: &Path, name: &str) -> Option<PathBuf> {
    let pack = props::store::pack(name)?;
    let file = format!("{}.json", props::leaf(name));
    ["lib", "local"].iter().map(|tier| root.join(tier).join(pack).join("rigs").join(&file)).find(|p| p.is_file())
}

fn registry() -> &'static RwLock<BTreeMap<String, Arc<Figure>>> {
    static FIGURES: OnceLock<RwLock<BTreeMap<String, Arc<Figure>>>> = OnceLock::new();
    FIGURES.get_or_init(Default::default)
}

/// Makes `rig` and its body (`rig.body`, which must load) the figure `rig.body`.
pub fn install(rig: RigFile) -> Result<Arc<Figure>, String> {
    let name = props::store::canonical(&rig.body)?;
    let body = props::resolve_ref(&name)?;
    let figure = Arc::new(Figure::new(&name, rig, body)?);
    registry().write().unwrap().insert(name, figure.clone());
    Ok(figure)
}

/// Forgets a figure (the next `get` reads it again).
pub fn forget(name: &str) {
    if let Ok(name) = props::store::canonical(name) {
        registry().write().unwrap().remove(&name);
    }
}

/// The figure `name` (`LIB/kaykit_adventurers/knight`): installed, else read once from its
/// rig file on disk (native builds; the browser has none).
pub fn get(name: &str) -> Result<Arc<Figure>, String> {
    let name = props::store::canonical(name)?;
    if let Some(f) = registry().read().unwrap().get(&name) {
        return Ok(f.clone());
    }
    if cfg!(target_arch = "wasm32") {
        return Err(format!("{name}: figures are read from disk (desktop builds)"));
    }
    let path = rig_file(&props::disk_root(), &name)
        .ok_or_else(|| format!("{name}: no rig file (rigs/{}.json in its pack)", props::leaf(&name)))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rig = RigFile::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if rig.body != name {
        return Err(format!("{}: its body is {}, not {name}", path.display(), rig.body));
    }
    install(rig)
}

/// What `check` finds in a figure: required segments missing (unmapped bones), too few
/// segments drawn, a segment's mesh far from its bone, a joint far from the mesh it hangs
/// from (the body would come apart), a body below the floor (or floating)
/// when it stands in its display pose, proportions out of a biped's range, a pose that is not
/// finite. Errors, in the Issue format.
pub fn check(figure: &Figure) -> Vec<Issue> {
    let rig = &figure.rig;
    let mut out = Vec::new();
    for s in Seg::ALL.iter().filter(|s| s.required()) {
        if rig.seg(*s).is_none() {
            out.push(
                Issue::error(format!("segments.{}", s.name()), Code::Missing, format!("no bone of the rig plays {}", s.vrm()))
                    .expected("a bone mapped by the rig's table (VRM 1.0's 15 required bones)")
                    .fix("map the rig's bone in its table (crates/pav_tools/src/import/skeleton.rs) and import it again"),
            );
        }
    }
    let drawn = figure.segments();
    if drawn.len() < 12 {
        out.push(
            Issue::error("parts", Code::Limit, format!("{} segments drawn", drawn.len()))
                .expected("at least 12 of the 16 (a body with arms, legs, hands and feet)"),
        );
    }
    // Each segment's mesh in its own frame, and at rest in the character's.
    let local_points = |s: Seg| -> Vec<Vec3> {
        let Some((i, _)) = figure.parts.get(&s) else { return Vec::new() };
        match &figure.body.compiled.parts[*i].prim {
            Primitive::Mesh { mesh, .. } => mesh.mesh.positions.clone(),
            prim => prim.hull(),
        }
    };
    let rest_points = |s: Seg| -> Vec<Vec3> {
        let Some((rest, joint)) = rest_frame(rig, s) else { return Vec::new() };
        local_points(s).into_iter().map(|p| joint + rest * p).collect()
    };
    for s in &drawn {
        let Some((joint, tail)) = rig.joint(*s) else { continue };
        let points = rest_points(*s);
        if points.is_empty() {
            continue;
        }
        // Its joint sits in the box of the mesh it hangs from (a wrist at the end of the
        // forearm), or near it.
        let parent = std::iter::successors(s.parent(), |p| p.parent()).find(|p| drawn.contains(p));
        if let (Some(parent), Some((rest, pj))) = (parent, parent.and_then(|p| rest_frame(rig, p))) {
            let local = local_points(parent);
            let (lo, hi) = local.iter().fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let at = rest.inverse() * (joint - pj);
            let near = (lo - at).max(at - hi).max(Vec3::ZERO).length();
            let bone = rig.joint(parent).map_or(0.0, |(j, t)| (t - j).length());
            let limit = 0.03 + 0.25 * bone;
            if near > limit {
                out.push(
                    Issue::error(
                        format!("segments.{}.joint", s.name()),
                        Code::OutOfRange,
                        format!("{:.0} cm from {}'s mesh: the body would come apart there", near * 100.0, parent.name()),
                    )
                    .expected(format!("within {:.0} cm", limit * 100.0))
                    .fix("the joint is not the model's: import it again"),
                );
            }
        }
        // The mesh's middle, in the rest pose, against the bone from joint to tail.
        let centre = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let bone = tail - joint;
        let t =
            if bone.length_squared() > 1e-8 { ((centre - joint).dot(bone) / bone.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let off = (centre - (joint + bone * t)).length();
        let limit = (0.6 * bone.length()).max(0.15);
        if off > limit {
            out.push(
                Issue::error(
                    format!("parts.{}", s.name()),
                    Code::OutOfRange,
                    format!("its mesh's middle is {:.0} cm from its bone", off * 100.0),
                )
                .expected(format!("within {:.0} cm", limit * 100.0))
                .fix("the mesh is in the wrong segment's frame: import it again"),
            );
        }
    }
    let l = &rig.look;
    for (field, v, lo, hi) in [
        ("leg_length", l.leg_length, 0.15, 1.6),
        ("arm_length", l.arm_length, 0.15, 1.6),
        ("torso_length", l.torso_length, 0.1, 1.2),
        ("hip_width", l.hip_width, 0.02, 0.5),
        ("shoulder_width", l.shoulder_width, 0.04, 0.6),
        ("scale", l.scale, 0.3, 3.0),
    ] {
        if !(lo..=hi).contains(&v) {
            out.push(Issue::error(format!("look.{field}"), Code::OutOfRange, format!("{v} m")).expected(format!("{lo}-{hi} m")));
        }
    }
    // Standing in the display pose, the body's soles are on the floor.
    let def = figure.fit(&PuppetDef::default());
    let sk = crate::clips::skel(&def, &display_key(), false);
    let poses = figure.pose(&def, &sk);
    if poses.iter().any(|p| !p.pos.is_finite() || !p.rot.is_finite()) {
        out.push(Issue::error("", Code::OutOfRange, "the display pose is not finite"));
    } else {
        let mut low = f32::INFINITY;
        for p in &poses {
            if let Some((i, _)) = figure.parts.get(&p.seg) {
                let part = &figure.body.compiled.parts[*i];
                let points = match &part.prim {
                    Primitive::Mesh { mesh, .. } => mesh.mesh.positions.clone(),
                    prim => prim.hull(),
                };
                low = points.iter().map(|v| (p.pos + p.rot * *v).y).fold(low, f32::min);
            }
        }
        if !(-0.02..=0.02).contains(&low) {
            out.push(
                Issue::error("", Code::OutOfRange, format!("standing, its lowest point is {:.0} cm from the floor", low * 100.0))
                    .expected("within 2 cm: soles on the floor")
                    .fix("a segment's mesh is out of place: import it again"),
            );
        }
        // The body as written stands on the floor too.
        let b = figure.body.compiled.bounds;
        if b.min.y < -0.02 {
            out.push(
                Issue::error("parts", Code::OutOfRange, format!("the body dips {:.0} cm below the floor", -b.min.y * 100.0))
                    .expected("its lowest point on the floor"),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vrm_bones_fold_into_the_body_plan() {
        assert_eq!(Seg::of_vrm("hips"), Some(Seg::Hips));
        assert_eq!(Seg::of_vrm("neck"), Some(Seg::Chest));
        assert_eq!(Seg::of_vrm("leftShoulder"), Some(Seg::Chest));
        assert_eq!(Seg::of_vrm("rightIndexDistal"), Some(Seg::HandR));
        assert_eq!(Seg::of_vrm("leftThumbMetacarpal"), Some(Seg::HandL));
        assert_eq!(Seg::of_vrm("leftToes"), Some(Seg::FootL));
        assert_eq!(Seg::of_vrm("rightEye"), Some(Seg::Head));
        assert_eq!(Seg::of_vrm("tail"), None);
        for s in Seg::ALL {
            assert_eq!(Seg::parse(s.name()), Some(s));
            assert_eq!(Seg::of_vrm(s.vrm()), Some(s), "{}", s.vrm());
        }
    }

    /// A T-posed rig 1.8 m tall facing +z, its left side at +x (a glTF character's).
    fn t_pose() -> RigFile {
        let mut segments = BTreeMap::new();
        let mut put = |s: Seg, joint: [f32; 3], tail: [f32; 3]| {
            segments.insert(s.name().to_string(), SegFile { bones: vec![s.vrm().into()], joint, tail, tris: 12 });
        };
        put(Seg::Hips, [0.0, 95.0, 0.0], [0.0, 110.0, 0.0]);
        put(Seg::Spine, [0.0, 110.0, 0.0], [0.0, 130.0, 0.0]);
        put(Seg::Chest, [0.0, 130.0, 0.0], [0.0, 150.0, 0.0]);
        put(Seg::Head, [0.0, 155.0, 0.0], [0.0, 180.0, 0.0]);
        for (x, [u, l, h]) in
            [(1.0, [Seg::UpperArmL, Seg::LowerArmL, Seg::HandL]), (-1.0, [Seg::UpperArmR, Seg::LowerArmR, Seg::HandR])]
        {
            put(u, [18.0 * x, 145.0, 0.0], [46.0 * x, 145.0, 0.0]);
            put(l, [46.0 * x, 145.0, 0.0], [72.0 * x, 145.0, 0.0]);
            put(h, [72.0 * x, 145.0, 0.0], [90.0 * x, 145.0, 0.0]);
        }
        for (x, [u, l, f]) in
            [(1.0, [Seg::UpperLegL, Seg::LowerLegL, Seg::FootL]), (-1.0, [Seg::UpperLegR, Seg::LowerLegR, Seg::FootR])]
        {
            put(u, [10.0 * x, 92.0, 0.0], [10.0 * x, 50.0, 0.0]);
            put(l, [10.0 * x, 50.0, 0.0], [10.0 * x, 8.0, 0.0]);
            put(f, [10.0 * x, 8.0, 0.0], [10.0 * x, 0.0, 20.0]);
        }
        RigFile {
            format: 1,
            name: "t".into(),
            body: "LIB/test_figures/t".into(),
            unit: "cm".into(),
            table: "vrm".into(),
            look: Proportions {
                scale: 1.0,
                leg_length: 0.84,
                arm_length: 0.54,
                torso_length: 0.55,
                torso_radius: 0.16,
                hip_width: 0.1,
                shoulder_width: 0.18,
                head_radius: 0.1,
                limb_radius: 0.05,
            },
            segments,
            folded: Vec::new(),
            left_out: Vec::new(),
        }
    }

    #[test]
    fn rig_files_round_trip_and_refuse_strangers() {
        let rig = t_pose();
        let text = rig.text();
        assert_eq!(RigFile::parse(&text).unwrap(), rig);
        assert!(text.lines().count() >= 18, "{text}");
        assert!(RigFile::parse(&text.replace("\"format\":1", "\"format\":2")).unwrap_err().contains("format 2"));
        assert!(RigFile::parse(&text.replace("\"hips\":{", "\"pelvis\":{")).unwrap_err().contains("pelvis"));
        assert!(RigFile::parse(&text.replace("\"tris\":12", "\"tris\":12,\"x\":1")).is_err());
    }

    #[test]
    fn limbs_follow_the_puppet_and_keep_their_lengths() {
        let rig = t_pose();
        let def = fit(&rig, &PuppetDef::default());
        let sk = crate::clips::skel(&def, &display_key(), false);
        let feet_low = |s: Seg| if matches!(s, Seg::FootL | Seg::FootR) { vec![Vec3::new(0.0, -0.08, 0.0)] } else { vec![] };
        let poses = pose(&rig, &def, &sk, &feet_low);
        assert_eq!(poses.len(), 16);
        let at = |s: Seg| poses.iter().find(|p| p.seg == s).unwrap();
        // The left arm (at +x at rest) follows the puppet's +x arm, down by the body.
        let (hand, shoulder) = (at(Seg::HandL).pos, at(Seg::UpperArmL).pos);
        assert!(hand.x > 0.0 && hand.y < shoulder.y - 0.3, "{hand} {shoulder}");
        // Bones keep their rest lengths.
        let len = |a: Seg, b: Seg| (at(b).pos - at(a).pos).length();
        assert!((len(Seg::UpperArmL, Seg::LowerArmL) - 0.28).abs() < 1e-3);
        assert!((len(Seg::LowerLegR, Seg::FootR) - 0.42).abs() < 1e-3);
        // The soles stand on the floor: the foot's lowest point at 0.
        let sole = (at(Seg::FootL).pos + at(Seg::FootL).rot * Vec3::new(0.0, -0.08, 0.0)).y;
        assert!(sole.abs() < 0.01, "{sole}");
        // A limb's +y runs along its bone; a knee bends forward.
        let thigh = at(Seg::UpperLegL);
        let down = (at(Seg::LowerLegL).pos - thigh.pos).normalize();
        assert!((thigh.rot * Vec3::Y).dot(down) > 0.999);
        assert!((thigh.rot * Vec3::Z).z > 0.5, "the knee bends forward: {}", thigh.rot * Vec3::Z);
        // Rest frames: a T-posed left arm along +x with its elbow back.
        let (q, _) = rest_frame(&rig, Seg::UpperArmL).unwrap();
        assert!((q * Vec3::Y - Vec3::X).length() < 1e-5 && (q * Vec3::Z - Vec3::NEG_Z).length() < 1e-5);
        let (q, _) = rest_frame(&rig, Seg::FootR).unwrap();
        assert!((q * Vec3::Y - Vec3::Z).length() < 1e-5 && (q * Vec3::Z - Vec3::Y).length() < 1e-5);
    }

    #[test]
    fn every_embedded_clip_poses_a_figure_finitely() {
        let rig = t_pose();
        let def = fit(&rig, &PuppetDef::default());
        let lib = crate::clips::library();
        let mut n = 0;
        for (set, name) in lib.sets.iter().flat_map(|set| set.clips.keys().map(move |name| (set.set.clone(), name.clone()))) {
            let id = crate::clips::clip_id(&set, &name);
            let dur = crate::clips::with(id, |c| c.dur).unwrap();
            for k in 0..4 {
                let sk = crate::clips::pose_of(&def, id, dur * k as f32 / 3.0, 0).unwrap();
                for p in pose(&rig, &def, &sk, &|_| vec![Vec3::ZERO]) {
                    assert!(p.pos.is_finite() && p.rot.is_finite(), "{name} {:?}", p.seg);
                }
                n += 1;
            }
        }
        assert!(n > 400, "{n} poses");
    }
}
