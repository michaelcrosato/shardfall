//! The bone-name tables that recognise a rig: for each rig family, the bone name (or names, the
//! first present wins) that plays each VRM 1.0 humanoid bone. Names compare after
//! `normalize_name`: namespace prefixes (`mixamorig:`, `Armature|`) dropped, case folded,
//! separators removed.
//!
//! Ported from my-3danimations (MIT licence, Michael Crosato and contributors),
//! `packages/core/src/skeleton/tables.ts` at commit 70a45c6e9b27e7a894574d672537d7fe190c8a5d:
//! the same tables in the same order, `normalize_name` as `normalizeName`. Added here: the
//! `kaykit` table (KayKit's character packs name their own bones: `upperarm.l`, `wrist.l`),
//! and `recognize`, which picks the table that covers a rig's bones.

use std::collections::BTreeMap;

/// A rig family's names for the humanoid bones.
#[derive(Clone, Debug)]
pub struct Table {
    pub name: &'static str,
    pub about: &'static str,
    /// VRM bone → the rig's candidate names, in order.
    pub bones: BTreeMap<String, Vec<String>>,
}

/// The 15 bones VRM 1.0 requires of a humanoid.
pub const REQUIRED: [&str; 15] = [
    "hips",
    "spine",
    "head",
    "leftUpperArm",
    "leftLowerArm",
    "leftHand",
    "rightUpperArm",
    "rightLowerArm",
    "rightHand",
    "leftUpperLeg",
    "leftLowerLeg",
    "leftFoot",
    "rightUpperLeg",
    "rightLowerLeg",
    "rightFoot",
];

/// A name as tables compare it: no namespace (nor the `mixamorig` three.js leaves), lower case,
/// letters and digits.
pub fn normalize_name(name: &str) -> String {
    let bare = match name.rfind([':', '|']) {
        Some(at) => &name[at + 1..],
        None => name,
    };
    let low: String = bare.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect();
    // three.js's GLTFLoader sanitizes "mixamorig:Hips" to "mixamorigHips": drop the prefix it leaves.
    if let Some(rest) = low.strip_prefix("mixamorig") {
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        if rest.starts_with(|c: char| c.is_ascii_lowercase()) {
            return rest.to_string();
        }
    }
    low
}

const FINGERS: [&str; 4] = ["Index", "Middle", "Ring", "Little"];
const JOINTS: [&str; 3] = ["Proximal", "Intermediate", "Distal"];
/// Each side: (VRM side prefix, `S`, `s`, `x`) as tables.ts's `side(S, s, x)` receives them.
const SIDES: [(&str, &str, &str, &str); 2] = [("left", "Left", "L", "l"), ("right", "Right", "R", "r")];

type Side = fn(&str, &str, &str) -> Vec<(String, Vec<String>)>;

/// A table from its midline names and a function of the side (S: Left/Right, s: L/R, x: l/r).
fn table(name: &'static str, about: &'static str, center: &[(&str, &[&str])], side: Side) -> Table {
    let mut bones = BTreeMap::new();
    for (bone, names) in center {
        bones.insert(bone.to_string(), names.iter().map(|n| n.to_string()).collect());
    }
    for (vs, cap, short, low) in SIDES {
        for (part, names) in side(cap, short, low) {
            bones.insert(format!("{vs}{part}"), names);
        }
    }
    Table { name, about, bones }
}

fn one(part: &str, name: String) -> (String, Vec<String>) {
    (part.to_string(), vec![name])
}

/// VRM 1.0's own names (and Godot's SkeletonProfileHumanoid, which normalizes the same).
fn vrm_side(cap: &str, _: &str, _: &str) -> Vec<(String, Vec<String>)> {
    let mut o: Vec<(String, Vec<String>)> = [
        "Shoulder",
        "UpperArm",
        "LowerArm",
        "Hand",
        "UpperLeg",
        "LowerLeg",
        "Foot",
        "Toes",
        "Eye",
        "ThumbMetacarpal",
        "ThumbProximal",
        "ThumbDistal",
    ]
    .iter()
    .map(|p| one(p, format!("{cap}{p}")))
    .collect();
    for f in FINGERS {
        for j in JOINTS {
            o.push(one(&format!("{f}{j}"), format!("{cap}{f}{j}")));
        }
    }
    o
}

fn unity_side(cap: &str, _: &str, _: &str) -> Vec<(String, Vec<String>)> {
    let mut o: Vec<(String, Vec<String>)> = [
        ("Shoulder", "Shoulder"),
        ("UpperArm", "UpperArm"),
        ("LowerArm", "LowerArm"),
        ("Hand", "Hand"),
        ("UpperLeg", "UpperLeg"),
        ("LowerLeg", "LowerLeg"),
        ("Foot", "Foot"),
        ("Toes", "Toes"),
        ("Eye", "Eye"),
        ("ThumbMetacarpal", "ThumbProximal"),
        ("ThumbProximal", "ThumbIntermediate"),
        ("ThumbDistal", "ThumbDistal"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{cap}{n}")))
    .collect();
    for f in FINGERS {
        for j in JOINTS {
            o.push(one(&format!("{f}{j}"), format!("{cap}{f}{j}")));
        }
    }
    o
}

fn mixamo_side(cap: &str, _: &str, _: &str) -> Vec<(String, Vec<String>)> {
    let mut o: Vec<(String, Vec<String>)> = [
        ("Shoulder", "Shoulder"),
        ("UpperArm", "Arm"),
        ("LowerArm", "ForeArm"),
        ("Hand", "Hand"),
        ("UpperLeg", "UpLeg"),
        ("LowerLeg", "Leg"),
        ("Foot", "Foot"),
        ("Toes", "ToeBase"),
        ("Eye", "Eye"),
        ("ThumbMetacarpal", "HandThumb1"),
        ("ThumbProximal", "HandThumb2"),
        ("ThumbDistal", "HandThumb3"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{cap}{n}")))
    .collect();
    for f in FINGERS {
        let finger = if f == "Little" { "Pinky" } else { f };
        for (i, j) in JOINTS.iter().enumerate() {
            o.push(one(&format!("{f}{j}"), format!("{cap}Hand{finger}{}", i + 1)));
        }
    }
    o
}

/// The Unreal mannequins (4 and 5 name their limbs alike).
fn unreal_side(_: &str, _: &str, x: &str) -> Vec<(String, Vec<String>)> {
    let mut o: Vec<(String, Vec<String>)> = [
        ("Shoulder", "clavicle"),
        ("UpperArm", "upperarm"),
        ("LowerArm", "lowerarm"),
        ("Hand", "hand"),
        ("UpperLeg", "thigh"),
        ("LowerLeg", "calf"),
        ("Foot", "foot"),
        ("Toes", "ball"),
        ("ThumbMetacarpal", "thumb_01"),
        ("ThumbProximal", "thumb_02"),
        ("ThumbDistal", "thumb_03"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{n}_{x}")))
    .collect();
    for f in FINGERS {
        let finger = if f == "Little" { "pinky".to_string() } else { f.to_lowercase() };
        for (i, j) in JOINTS.iter().enumerate() {
            o.push(one(&format!("{f}{j}"), format!("{finger}_0{}_{x}", i + 1)));
        }
    }
    o
}

fn rigify_side(_: &str, s: &str, _: &str) -> Vec<(String, Vec<String>)> {
    let mut o: Vec<(String, Vec<String>)> = [
        ("Shoulder", "DEF-shoulder"),
        ("UpperArm", "DEF-upper_arm"),
        ("LowerArm", "DEF-forearm"),
        ("Hand", "DEF-hand"),
        ("UpperLeg", "DEF-thigh"),
        ("LowerLeg", "DEF-shin"),
        ("Foot", "DEF-foot"),
        ("Toes", "DEF-toe"),
        ("ThumbMetacarpal", "DEF-thumb.01"),
        ("ThumbProximal", "DEF-thumb.02"),
        ("ThumbDistal", "DEF-thumb.03"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{n}.{s}")))
    .collect();
    for f in FINGERS {
        let finger = if f == "Little" { "pinky".to_string() } else { f.to_lowercase() };
        for (i, j) in JOINTS.iter().enumerate() {
            o.push(one(&format!("{f}{j}"), format!("DEF-f_{finger}.0{}.{s}", i + 1)));
        }
    }
    o
}

fn cmu_side(_: &str, _: &str, x: &str) -> Vec<(String, Vec<String>)> {
    [
        ("Shoulder", "clavicle"),
        ("UpperArm", "humerus"),
        ("LowerArm", "radius"),
        ("Hand", "wrist"),
        ("UpperLeg", "femur"),
        ("LowerLeg", "tibia"),
        ("Foot", "foot"),
        ("Toes", "toes"),
        ("ThumbProximal", "thumb"),
        ("MiddleProximal", "fingers"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{x}{n}")))
    .collect()
}

fn style100_side(cap: &str, _: &str, _: &str) -> Vec<(String, Vec<String>)> {
    [
        ("Shoulder", "Collar"),
        ("UpperArm", "Shoulder"),
        ("LowerArm", "Elbow"),
        ("Hand", "Wrist"),
        ("UpperLeg", "Hip"),
        ("LowerLeg", "Knee"),
        ("Foot", "Ankle"),
        ("Toes", "Toe"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{cap}{n}")))
    .collect()
}

/// KayKit's character packs (Kay Lousberg): `hips`, `spine`, `chest`, `head`, and per side
/// `upperarm.l`, `lowerarm.l`, `wrist.l` (the wrist joint; its child `hand.l` is the palm),
/// `upperleg.l`, `lowerleg.l`, `foot.l`, `toes.l`. Added in Shardfall (not in tables.ts).
fn kaykit_side(_: &str, _: &str, x: &str) -> Vec<(String, Vec<String>)> {
    [
        ("UpperArm", "upperarm"),
        ("LowerArm", "lowerarm"),
        ("Hand", "wrist"),
        ("UpperLeg", "upperleg"),
        ("LowerLeg", "lowerleg"),
        ("Foot", "foot"),
        ("Toes", "toes"),
    ]
    .iter()
    .map(|(p, n)| one(p, format!("{n}.{x}")))
    .collect()
}

/// Every table `recognize` tries, most specific first (tables.ts's order, then `kaykit`).
pub fn tables() -> &'static [Table] {
    static TABLES: std::sync::OnceLock<Vec<Table>> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        vec![
            table(
                "vrm",
                "VRM 1.0 humanoid bone names (also Godot 4's SkeletonProfileHumanoid)",
                &[
                    ("hips", &["hips"]),
                    ("spine", &["spine"]),
                    ("chest", &["chest"]),
                    ("upperChest", &["upperChest"]),
                    ("neck", &["neck"]),
                    ("head", &["head"]),
                    ("jaw", &["jaw"]),
                ],
                vrm_side,
            ),
            table(
                "unity",
                "Unity HumanBodyBones as bone names (and VRM 0.x)",
                &[
                    ("hips", &["Hips"]),
                    ("spine", &["Spine"]),
                    ("chest", &["Chest"]),
                    ("upperChest", &["UpperChest"]),
                    ("neck", &["Neck"]),
                    ("head", &["Head"]),
                    ("jaw", &["Jaw"]),
                ],
                unity_side,
            ),
            table(
                "mixamo",
                "Mixamo (mixamorig:*)",
                &[
                    ("hips", &["Hips"]),
                    ("spine", &["Spine"]),
                    ("chest", &["Spine1"]),
                    ("upperChest", &["Spine2"]),
                    ("neck", &["Neck"]),
                    ("head", &["Head"]),
                ],
                mixamo_side,
            ),
            table(
                "unreal5",
                "Unreal Engine 5 mannequin (spine_01..05, neck_01..02, metacarpals)",
                &[
                    ("hips", &["pelvis"]),
                    ("spine", &["spine_01"]),
                    ("chest", &["spine_03"]),
                    ("upperChest", &["spine_05"]),
                    ("neck", &["neck_01"]),
                    ("head", &["head"]),
                ],
                unreal_side,
            ),
            table(
                "unreal4",
                "Unreal Engine 4 mannequin and Unreal-style exports (Quaternius UAL2, Mesh2Motion)",
                &[
                    ("hips", &["pelvis"]),
                    ("spine", &["spine_01"]),
                    ("chest", &["spine_02"]),
                    ("upperChest", &["spine_03"]),
                    ("neck", &["neck_01"]),
                    ("head", &["head"]),
                ],
                unreal_side,
            ),
            table(
                "rigify",
                "Blender Rigify deform bones (DEF-*), as Quaternius' UAL1 Godot export and standard Rigify name them",
                &[
                    ("hips", &["DEF-hips", "DEF-spine"]),
                    ("spine", &["DEF-spine.001"]),
                    ("chest", &["DEF-spine.002"]),
                    ("upperChest", &["DEF-spine.003"]),
                    ("neck", &["DEF-neck", "DEF-spine.004"]),
                    ("head", &["DEF-head", "DEF-spine.006"]),
                ],
                rigify_side,
            ),
            table(
                "cmu",
                "CMU motion capture skeletons (ASF)",
                &[
                    ("hips", &["root"]),
                    ("spine", &["lowerback"]),
                    ("chest", &["upperback"]),
                    ("upperChest", &["thorax"]),
                    ("neck", &["lowerneck"]),
                    ("head", &["head"]),
                ],
                cmu_side,
            ),
            table(
                "bvh100style",
                "100STYLE BVH (Chest..Chest4, Collar, Shoulder for the upper arm)",
                &[
                    ("hips", &["Hips"]),
                    ("spine", &["Chest"]),
                    ("chest", &["Chest2"]),
                    ("upperChest", &["Chest4"]),
                    ("neck", &["Neck"]),
                    ("head", &["Head"]),
                ],
                style100_side,
            ),
            table(
                "kaykit",
                "KayKit character packs (Kay Lousberg): hips, spine, chest, head; upperarm.l, lowerarm.l, wrist.l; upperleg.l, lowerleg.l, foot.l, toes.l",
                &[("hips", &["hips"]), ("spine", &["spine"]), ("chest", &["chest"]), ("head", &["head"])],
                kaykit_side,
            ),
        ]
    })
}

/// A rig recognised: the table that named it and, for each VRM bone it has, the rig's bone
/// (its name as the file writes it).
#[derive(Clone, Debug, PartialEq)]
pub struct Recognized {
    pub table: &'static str,
    pub bones: BTreeMap<String, String>,
    /// Required VRM bones the rig does not have.
    pub missing: Vec<&'static str>,
}

/// The table that covers the most of the required bones of a rig whose bones are `names` (the
/// first such table on a tie), with every VRM bone it maps. None when no table covers any.
pub fn recognize<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Recognized> {
    let mut by_norm: BTreeMap<String, &str> = BTreeMap::new();
    for name in names {
        // The first bone of a normalized name wins (a rig rarely repeats one).
        by_norm.entry(normalize_name(name)).or_insert(name);
    }
    let mut best: Option<(usize, usize, Recognized)> = None;
    for t in tables() {
        let mut bones = BTreeMap::new();
        for (vrm, candidates) in &t.bones {
            if let Some(found) = candidates.iter().find_map(|c| by_norm.get(&normalize_name(c))) {
                bones.insert(vrm.clone(), found.to_string());
            }
        }
        let missing: Vec<&'static str> = REQUIRED.iter().copied().filter(|b| !bones.contains_key(*b)).collect();
        let (covered, all) = (REQUIRED.len() - missing.len(), bones.len());
        if covered == 0 {
            continue;
        }
        if best.as_ref().is_none_or(|(c, a, _)| covered > *c || (covered == *c && all > *a)) {
            best = Some((covered, all, Recognized { table: t.name, bones, missing }));
        }
    }
    best.map(|(_, _, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_normalize_as_tables_ts_does() {
        assert_eq!(normalize_name("mixamorig:LeftUpLeg"), "leftupleg");
        assert_eq!(normalize_name("mixamorigLeftUpLeg"), "leftupleg");
        assert_eq!(normalize_name("mixamorig1:Hips"), "hips");
        assert_eq!(normalize_name("Armature|DEF-upper_arm.L"), "defupperarml");
        assert_eq!(normalize_name("upperarm.l"), "upperarml");
        assert_eq!(normalize_name("spine_01"), "spine01");
    }

    #[test]
    fn every_table_names_the_required_bones() {
        let tables = tables();
        assert_eq!(tables.len(), 9);
        for t in tables.iter().filter(|t| t.name != "cmu" && t.name != "bvh100style") {
            for bone in REQUIRED {
                assert!(t.bones.contains_key(bone), "{}: {bone}", t.name);
            }
        }
        let mixamo = tables.iter().find(|t| t.name == "mixamo").unwrap();
        assert_eq!(mixamo.bones["leftLittleDistal"], ["LeftHandPinky3"]);
        assert_eq!(mixamo.bones["rightUpperArm"], ["RightArm"]);
        let rigify = tables.iter().find(|t| t.name == "rigify").unwrap();
        assert_eq!(rigify.bones["hips"], ["DEF-hips", "DEF-spine"]);
        assert_eq!(rigify.bones["leftIndexProximal"], ["DEF-f_index.01.L"]);
    }

    #[test]
    fn rigs_are_recognised_by_their_names() {
        let kaykit = [
            "root",
            "hips",
            "spine",
            "chest",
            "head",
            "upperarm.l",
            "lowerarm.l",
            "wrist.l",
            "hand.l",
            "handslot.l",
            "upperarm.r",
            "lowerarm.r",
            "wrist.r",
            "hand.r",
            "upperleg.l",
            "lowerleg.l",
            "foot.l",
            "toes.l",
            "upperleg.r",
            "lowerleg.r",
            "foot.r",
            "toes.r",
            "kneeIK.l",
        ];
        let r = recognize(kaykit).unwrap();
        assert_eq!(r.table, "kaykit");
        assert!(r.missing.is_empty(), "{:?}", r.missing);
        assert_eq!(r.bones["leftHand"], "wrist.l");
        assert_eq!(r.bones["rightToes"], "toes.r");
        let mixamo = ["mixamorig:Hips", "mixamorig:Spine", "mixamorig:Head", "mixamorig:LeftArm", "mixamorig:LeftForeArm"];
        let r = recognize(mixamo).unwrap();
        assert_eq!(r.table, "mixamo");
        assert_eq!(r.bones["leftLowerArm"], "mixamorig:LeftForeArm");
        assert!(r.missing.contains(&"rightFoot"));
        assert!(recognize(["Bone", "Bone.001"]).is_none());
    }
}
