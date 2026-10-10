//! Readable procedural props. Named parts use the engine's shapes, colors and surface looks.
//! A published definition is immutable; placed instances can keep it in their snapshots.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

use glam::{Quat, Vec3};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::color::Color;
use crate::shape::{Look, Shape, Visual};

pub const MAX_PARTS: usize = 256;
const MAX_SIZE: f32 = 1_000.0;
const MAX_POSITION: f32 = 10_000.0;
const MIN_SIZE: f32 = 0.001;

pub const LEGEND: &str = "Parts have stable lowercase names. Positions and dimensions use metres: \
x right, y up, z forward. The asset origin is the placement point, normally on the floor. \
pos is a part's local centre. yaw rotates about Y, pitch about X, roll about Z; all use degrees \
in Y-X-Z order, as room objects do. Box half values are half extents. Cylinder height is \
2*half_height; capsule total height is 2*(half_height+radius). Colors are sRGB #rrggbb. \
look is flat, cel, lit, unlit, or cutout; cutout draws ordinary props flat. emissive adds glow. \
solid selects parts used by the instance's collider. A set operation merges part fields \
and replaces shape as a complete object. A batch is accepted or rejected as a whole.";

/// Local axis-aligned bounds after the requested uniform scale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn union(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }
}

/// One primitive in the prop's own coordinate frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropPart {
    #[serde(deserialize_with = "read_shape")]
    pub shape: Shape,
    #[serde(default)]
    pub pos: Vec3,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub roll: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub emissive: f32,
    #[serde(default = "yes")]
    pub solid: bool,
}

fn default_color() -> String {
    "#e8704a".into()
}

fn yes() -> bool {
    true
}

impl Default for PropPart {
    fn default() -> Self {
        Self {
            shape: Shape::Box { half: Vec3::splat(0.5) },
            pos: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            color: default_color(),
            look: Look::Cel,
            emissive: 0.0,
            solid: true,
        }
    }
}

// Shape is shared with older room files. Check its field names here without changing their
// parser's compatibility. This also applies when a prop is read through snapshot serde.
fn read_shape<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Shape, D::Error> {
    let value = Value::deserialize(deserializer)?;
    let object = value.as_object().ok_or_else(|| serde::de::Error::custom("shape must be an object"))?;
    let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| serde::de::Error::custom("shape needs type"))?;
    let allowed: &[&str] = match kind {
        "box" => &["type", "half"],
        "rounded_box" => &["type", "half", "radius"],
        "sphere" => &["type", "radius"],
        "capsule" | "cylinder" => &["type", "half_height", "radius"],
        _ => return Err(serde::de::Error::custom(format!("unknown shape type '{kind}'"))),
    };
    for field in object.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(serde::de::Error::custom(format!("unknown {kind} shape field '{field}'")));
        }
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

impl PropPart {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(glam::EulerRot::YXZ, self.yaw.to_radians(), self.pitch.to_radians(), self.roll.to_radians())
    }

    /// Shape dimensions are scaled. The caller applies pos * scale and rotation separately.
    pub fn visual(&self, scale: f32) -> Visual {
        let shape = match self.shape {
            Shape::Box { half } => Shape::Box { half: half * scale },
            Shape::RoundedBox { half, radius } => Shape::RoundedBox { half: half * scale, radius: radius * scale },
            Shape::Sphere { radius } => Shape::Sphere { radius: radius * scale },
            Shape::Capsule { half_height, radius } => Shape::Capsule { half_height: half_height * scale, radius: radius * scale },
            Shape::Cylinder { half_height, radius } => {
                Shape::Cylinder { half_height: half_height * scale, radius: radius * scale }
            }
        };
        let mut visual = Visual::new(shape, Color::hex(&self.color));
        visual.look = self.look;
        visual.emissive = self.emissive;
        visual
    }

    /// Bounds include the part's local position. Supply a finite positive uniform scale.
    pub fn bounds(&self, scale: f32) -> Bounds {
        let rotation = self.rotation();
        let axis = (rotation * Vec3::Y).normalize().abs();
        let half = match self.shape {
            Shape::Box { half } | Shape::RoundedBox { half, .. } => {
                (rotation * Vec3::X).abs() * half.x + axis * half.y + (rotation * Vec3::Z).abs() * half.z
            }
            Shape::Sphere { radius } => Vec3::splat(radius),
            Shape::Capsule { half_height, radius } => axis * half_height + Vec3::splat(radius),
            Shape::Cylinder { half_height, radius } => {
                let radial = Vec3::new(
                    (1.0 - axis.x * axis.x).max(0.0).sqrt(),
                    (1.0 - axis.y * axis.y).max(0.0).sqrt(),
                    (1.0 - axis.z * axis.z).max(0.0).sqrt(),
                );
                axis * half_height + radial * radius
            }
        } * scale;
        let center = self.pos * scale;
        Bounds { min: center - half, max: center + half }
    }

    pub fn validate(&self) -> Result<(), String> {
        let positive = |name: &str, value: f32| {
            if value.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be a finite number from {MIN_SIZE} to {MAX_SIZE}"))
            }
        };
        match self.shape {
            Shape::Box { half } | Shape::RoundedBox { half, .. } => {
                for (axis, value) in [("half.x", half.x), ("half.y", half.y), ("half.z", half.z)] {
                    positive(axis, value)?;
                }
                if let Shape::RoundedBox { radius, .. } = self.shape {
                    if !radius.is_finite() || radius < 0.0 || radius > half.min_element() {
                        return Err("radius must be finite and between zero and the smallest box half extent".into());
                    }
                }
            }
            Shape::Sphere { radius } => positive("radius", radius)?,
            Shape::Capsule { half_height, radius } => {
                positive("radius", radius)?;
                if !half_height.is_finite() || !(0.0..=MAX_SIZE).contains(&half_height) {
                    return Err(format!("half_height must be finite and between zero and {MAX_SIZE}"));
                }
            }
            Shape::Cylinder { half_height, radius } => {
                positive("half_height", half_height)?;
                positive("radius", radius)?;
            }
        }
        if !self.pos.is_finite() || self.pos.abs().max_element() > MAX_POSITION {
            return Err(format!("pos must contain finite numbers from -{MAX_POSITION} to {MAX_POSITION}"));
        }
        for (field, angle) in [("yaw", self.yaw), ("pitch", self.pitch), ("roll", self.roll)] {
            if !angle.is_finite() || angle.abs() > 36_000.0 {
                return Err(format!("{field} must be a finite angle from -36000 to 36000 degrees"));
            }
        }
        if self.color.len() != 7 || !self.color.starts_with('#') || Color::try_hex(&self.color).is_none() {
            return Err("color must be #rrggbb".into());
        }
        if !self.emissive.is_finite() || !(0.0..=32.0).contains(&self.emissive) {
            return Err("emissive must be a finite number from 0 to 32".into());
        }
        Ok(())
    }
}

/// A reusable prop. The map keys are stable part identifiers, never array positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropAsset {
    pub format: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(deserialize_with = "read_parts")]
    pub parts: BTreeMap<String, PropPart>,
}

fn read_parts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, PropPart>, D::Error> {
    struct Parts;
    impl<'de> serde::de::Visitor<'de> for Parts {
        type Value = BTreeMap<String, PropPart>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object of uniquely named parts")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut parts = BTreeMap::new();
            while let Some((name, part)) = map.next_entry::<String, PropPart>()? {
                if parts.insert(name.clone(), part).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate part name '{name}'")));
                }
                if parts.len() > MAX_PARTS {
                    return Err(serde::de::Error::custom(format!("a prop supports at most {MAX_PARTS} parts")));
                }
            }
            Ok(parts)
        }
    }
    deserializer.deserialize_map(Parts)
}

impl PropAsset {
    pub fn parse(text: &str) -> Result<Self, String> {
        let asset: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        asset.validate()?;
        Ok(asset)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err(format!("unsupported prop format {}; use format 1", self.format));
        }
        validate_name(&self.name)?;
        if self.description.len() > 4096 {
            return Err("description must contain at most 4096 bytes".into());
        }
        if self.parts.is_empty() || self.parts.len() > MAX_PARTS {
            return Err(format!("a prop needs 1 to {MAX_PARTS} named parts"));
        }
        for (name, part) in &self.parts {
            validate_part_name(name).map_err(|e| format!("part '{name}': {e}"))?;
            part.validate().map_err(|e| format!("part '{name}': {e}"))?;
        }
        Ok(())
    }

    pub fn revision(&self) -> String {
        // BTreeMap order and explicit defaults make reformatting a file keep its revision.
        // This is a content identifier for edit guards, not a security hash.
        let bytes = serde_json::to_vec(self).expect("prop definition serializes");
        let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
        format!("prop1-{hash:016x}")
    }

    pub fn bounds(&self, scale: f32) -> Bounds {
        self.parts
            .values()
            .map(|part| part.bounds(scale))
            .reduce(Bounds::union)
            .unwrap_or(Bounds { min: Vec3::ZERO, max: Vec3::ZERO })
    }

    pub fn text(&self) -> String {
        serde_json::to_string_pretty(self).expect("prop definition serializes") + "\n"
    }

    /// Apply a named-part transaction. The original stays unchanged if any operation fails.
    pub fn patch(&self, value: &Value) -> Result<Self, String> {
        let operations: Vec<PatchOp> = serde_json::from_value(value.clone()).map_err(|e| format!("invalid ops: {e}"))?;
        if operations.len() > 1024 {
            return Err("a patch supports at most 1024 operations".into());
        }
        let mut next = self.clone();
        for (index, operation) in operations.into_iter().enumerate() {
            let result = (|| -> Result<(), String> {
                match operation {
                    PatchOp::Set { part, fields } => {
                        validate_part_name(&part)?;
                        let current = next.parts.get(&part).ok_or_else(|| format!("no part '{part}' to set"))?;
                        let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
                        let object = value.as_object_mut().expect("part is an object");
                        for (field, value) in fields {
                            object.insert(field, value);
                        }
                        let changed = serde_json::from_value(value).map_err(|e| format!("part '{part}': {e}"))?;
                        next.parts.insert(part, changed);
                    }
                    PatchOp::Add { part, value } => {
                        validate_part_name(&part)?;
                        if next.parts.contains_key(&part) {
                            return Err(format!("part '{part}' already exists; use op=set"));
                        }
                        next.parts.insert(part, value);
                    }
                    PatchOp::Remove { part } => {
                        if next.parts.remove(&part).is_none() {
                            return Err(format!("no part '{part}' to remove"));
                        }
                    }
                }
                Ok(())
            })();
            result.map_err(|e| format!("operation {}: {e}", index + 1))?;
        }
        next.validate()?;
        Ok(next)
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum PatchOp {
    Set { part: String, fields: Map<String, Value> },
    Add { part: String, value: PropPart },
    Remove { part: String },
}

pub fn validate_part_name(name: &str) -> Result<(), String> {
    let mut chars = name.bytes();
    if name.len() > 64
        || !chars.next().is_some_and(|c| c.is_ascii_lowercase())
        || chars.any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != b'_' && c != b'-')
    {
        return Err("use 1 to 64 lowercase letters, digits, underscores or hyphens, starting with a letter".into());
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<(), String> {
    validate_part_name(name)?;
    if ["con", "prn", "aux", "nul"].contains(&name)
        || (name.len() == 4 && (name.starts_with("com") || name.starts_with("lpt")) && matches!(name.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(format!("'{name}' is a reserved Windows file name; choose another name"));
    }
    Ok(())
}

/// Normalize tool input. A bare name selects the workshop; explicit prefixes select a set.
pub fn canonical(name: &str) -> Result<String, String> {
    let (set, leaf) = name.split_once('/').unwrap_or(("WORKSHOP", name));
    let set = match set.to_ascii_uppercase().as_str() {
        "BUILTIN" => "BUILTIN",
        "WORKSHOP" => "WORKSHOP",
        _ => return Err("prop names use BUILTIN/name or WORKSHOP/name".into()),
    };
    let leaf = leaf.to_ascii_lowercase();
    validate_name(&leaf)?;
    Ok(format!("{set}/{leaf}"))
}

#[derive(Clone, Debug, Default)]
pub struct PropLibrary {
    pub assets: BTreeMap<String, Arc<PropAsset>>,
}

const TEMPLATES: &[(&str, &str)] = &[
    ("box", include_str!("../../../assets/props/templates/box.json")),
    ("crate", include_str!("../../../assets/props/templates/crate.json")),
    ("bench", include_str!("../../../assets/props/templates/bench.json")),
    ("lantern", include_str!("../../../assets/props/templates/lantern.json")),
];

static LIBRARY: OnceLock<RwLock<Arc<PropLibrary>>> = OnceLock::new();

fn shared() -> &'static RwLock<Arc<PropLibrary>> {
    LIBRARY.get_or_init(|| {
        let mut library = PropLibrary::default();
        for (name, text) in TEMPLATES {
            let asset = PropAsset::parse(text).unwrap_or_else(|e| panic!("built-in prop {name}: {e}"));
            assert_eq!(asset.name, *name, "template name matches its file");
            library.assets.insert(format!("BUILTIN/{name}"), Arc::new(asset));
        }
        RwLock::new(Arc::new(library))
    })
}

pub fn library() -> Arc<PropLibrary> {
    shared().read().unwrap().clone()
}

pub fn get(name: &str) -> Option<Arc<PropAsset>> {
    let name = canonical(name).ok()?;
    library().assets.get(&name).cloned()
}

/// Replace one library entry while keeping every other definition shared.
pub fn install(name: &str, asset: Arc<PropAsset>) -> Result<(), String> {
    let name = canonical(name)?;
    asset.validate()?;
    if name.split_once('/').map(|(_, leaf)| leaf) != Some(asset.name.as_str()) {
        return Err(format!("{name} does not match the prop's name '{}'", asset.name));
    }
    let mut current = shared().write().unwrap();
    let mut next = (**current).clone();
    next.assets.insert(name, asset);
    *current = Arc::new(next);
    Ok(())
}

pub fn remove(name: &str) -> Result<bool, String> {
    let name = canonical(name)?;
    if name.starts_with("BUILTIN/") {
        return Err("built-in templates cannot be removed".into());
    }
    let mut current = shared().write().unwrap();
    if !current.assets.contains_key(&name) {
        return Ok(false);
    }
    let mut next = (**current).clone();
    next.assets.remove(&name);
    *current = Arc::new(next);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn named_batches_keep_other_parts_and_reject_incomplete_transactions() {
        let original = (*get("BUILTIN/bench").unwrap()).clone();
        let changed = original
            .patch(&json!([
                {"op":"set","part":"seat","fields":{"color":"#4060a0"}},
                {"op":"add","part":"brace","value":{"shape":{"type":"box","half":[0.8,0.04,0.04]},"pos":[0,0.3,0]}},
                {"op":"remove","part":"backrest"}
            ]))
            .unwrap();
        assert_eq!(changed.parts["seat"].color, "#4060a0");
        assert_eq!(changed.parts["leg_front_left"], original.parts["leg_front_left"]);
        assert!(changed.parts.contains_key("brace") && !changed.parts.contains_key("backrest"));
        assert!(original.parts.contains_key("backrest"));
        let before = original.revision();
        assert!(
            original
                .patch(&json!([
                    {"op":"set","part":"seat","fields":{"color":"#ffffff"}},
                    {"op":"remove","part":"missing"}
                ]))
                .is_err()
        );
        assert_eq!(original.revision(), before);
        assert!(original.patch(&json!([{"op":"set","part":"seat","fields":{"colour":"#ffffff"}}])).is_err());
        assert!(
            original
                .patch(&json!([{"op":"set","part":"seat","fields":{"shape":{"type":"sphere","radius":0.4,"half":[1,1,1]}}}]))
                .is_err()
        );
        assert!(original.patch(&json!([{"op":"set","part":"seat","fields":{"shape":{"radius":0.4}}}])).is_err());
    }

    #[test]
    fn rotated_and_scaled_bounds_fit_the_actual_primitives() {
        let mut part = PropPart {
            shape: Shape::Box { half: Vec3::new(1.0, 0.5, 0.25) },
            pos: Vec3::new(3.0, 2.0, 1.0),
            yaw: 90.0,
            ..Default::default()
        };
        let bounds = part.bounds(2.0);
        assert!((bounds.min - Vec3::new(5.5, 3.0, 0.0)).length() < 1e-5);
        assert!((bounds.max - Vec3::new(6.5, 5.0, 4.0)).length() < 1e-5);
        part.shape = Shape::Capsule { half_height: 1.0, radius: 0.2 };
        part.yaw = 0.0;
        part.roll = 90.0;
        assert!((part.bounds(1.0).size() - Vec3::new(2.4, 0.4, 0.4)).length() < 1e-5);
        part.shape = Shape::Sphere { radius: 0.3 };
        assert!((part.bounds(1.0).size() - Vec3::splat(0.6)).length() < 1e-5);
        part.shape = Shape::Cylinder { half_height: 1.0, radius: 0.2 };
        assert!((part.bounds(1.0).size() - Vec3::new(2.0, 0.4, 0.4)).length() < 1e-5);
    }

    #[test]
    fn templates_round_trip_and_bad_files_do_not_enter_the_library() {
        let before = library();
        for (name, asset) in before.assets.iter().filter(|(name, _)| name.starts_with("BUILTIN/")) {
            assert_eq!(name, &format!("BUILTIN/{}", asset.name));
            let parsed = PropAsset::parse(&asset.text()).unwrap();
            assert_eq!(asset.revision(), parsed.revision());
            assert!(asset.bounds(1.0).size().min_element() > 0.0);
            let mut bytes = Vec::new();
            ciborium::into_writer(asset.as_ref(), &mut bytes).unwrap();
            let snapshot: PropAsset = ciborium::from_reader(bytes.as_slice()).unwrap();
            assert_eq!(snapshot, **asset, "part shape serde also works in snapshots");
        }
        let mut invalid = (**before.assets.get("BUILTIN/box").unwrap()).clone();
        invalid.name = "invalid_test_prop".into();
        invalid.parts.get_mut("body").unwrap().pos.x = f32::INFINITY;
        assert!(install("WORKSHOP/invalid_test_prop", Arc::new(invalid)).is_err());
        assert!(get("WORKSHOP/invalid_test_prop").is_none());
        assert!(Arc::ptr_eq(&before.assets["BUILTIN/bench"], &library().assets["BUILTIN/bench"]));
        assert!(canonical("../../bad").is_err());
        assert!(canonical("WORKSHOP/con").is_err());
        assert_eq!(canonical("Workshop/My_Bench").unwrap(), "WORKSHOP/my_bench");
        let duplicate = r##"{"format":1,"name":"duplicate","parts":{"body":{"shape":{"type":"sphere","radius":1}},"body":{"shape":{"type":"sphere","radius":2}}}}"##;
        assert!(PropAsset::parse(duplicate).unwrap_err().contains("duplicate part name"));
    }
}
