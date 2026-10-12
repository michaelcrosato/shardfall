//! Readable procedural props. Named parts use the engine's shapes, colors and surface looks.
//! A published definition is immutable; placed instances can keep it in their snapshots.

pub mod compile;
pub mod doc;
pub mod geom;
pub mod index;
pub mod lint;
pub mod mesh;
mod patch;
mod read;
pub mod store;
pub mod table;
mod write;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use glam::{Quat, Vec3};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::color::Color;
use crate::issue::{self, Issue};
use crate::shape::{Look, Shape, Visual};

pub use compile::{CompiledProp, DEFAULT_ROLES, ResolvedPart};
pub use doc::Doc;
pub use patch::MAX_OPS;
pub use patch::v2::{OPS as OPS2, Patched, patch as patch_document};
/// Format 2's field tables, legend and field guide.
pub use read::v2 as format2;
pub use read::{DOCUMENT_FIELDS, LOOKS, OPS, PART_FIELDS, SHAPES, check, migrate, schema, strict_value};
/// Reader helpers the material reader shares, so both word their errors alike.
pub(crate) use read::{amount, color_fix, is_color, look_hint, number, wrong_type};
pub use store::{Tier, canonical, disk_root, editable, leaf};
pub use table::PropDefTable;
/// The material writer shares the canonical layout of inline materials.
pub(crate) use write::{MaterialText, material_entries};

pub const MAX_PARTS: usize = 256;
const MAX_SIZE: f32 = 1_000.0;
const MAX_POSITION: f32 = 10_000.0;
const MIN_SIZE: f32 = 0.001;

pub const LEGEND: &str = "Parts have stable lowercase names. Metres: x right, y up, z forward \
(+z is the object's front). The origin is the placement point, normally on the floor. pos is a \
part's centre. yaw, pitch and roll are degrees applied Y, then X, then Z, as room objects do: \
+yaw turns +z toward +x, +pitch tips +z down. Box half values are half extents. Cylinder and \
capsule axes are local y; cylinder height is 2*half_height, capsule total height \
2*(half_height+radius). Colors are sRGB #rrggbb. look is flat, cel, lit, unlit, or cutout; \
cutout draws ordinary props flat. emissive adds glow. solid selects parts used by the \
instance's collider. A set operation merges part fields and replaces shape as a complete \
object. A batch is accepted or rejected as a whole.";

/// Local axis-aligned bounds after the requested uniform scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
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

    /// Width x height x depth in centimetres, as replies report it: whole centimetres from
    /// 10 cm up, tenths below.
    pub fn size_cm(&self) -> String {
        let cm = |metres: f32| {
            let cm = metres * 100.0;
            if cm >= 9.95 { format!("{}", cm.round()) } else { format!("{}", (cm * 10.0).round() / 10.0) }
        };
        let size = self.size();
        format!("{}x{}x{}", cm(size.x), cm(size.y), cm(size.z))
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

    /// Points on the part's surface whose hull is the part (round shapes within 4%), at its local
    /// position and scale. Cameras frame these to fit what is drawn, not its bounding box.
    pub fn hull(&self, scale: f32) -> Vec<Vec3> {
        let ring = |y: f32, radius: f32| {
            (0..16).map(move |i| {
                let angle = i as f32 * std::f32::consts::TAU / 16.0;
                Vec3::new(angle.cos() * radius, y, angle.sin() * radius)
            })
        };
        // Rings every 22.5 degrees of latitude and the poles; a capsule's halves sit `lift` apart.
        let ball = |radius: f32, lift: f32| {
            let mut points = vec![Vec3::Y * (radius + lift), -Vec3::Y * (radius + lift)];
            for band in -3..=3 {
                let latitude = band as f32 * std::f32::consts::FRAC_PI_8;
                let (y, r) = (latitude.sin() * radius, latitude.cos() * radius);
                if band >= 0 {
                    points.extend(ring(y + lift, r));
                }
                if band <= 0 {
                    points.extend(ring(y - lift, r));
                }
            }
            points
        };
        let side = |bit: usize| if bit == 0 { -1.0 } else { 1.0 };
        let local = match self.shape {
            Shape::Box { half } | Shape::RoundedBox { half, .. } => {
                (0..8).map(|i| half * Vec3::new(side(i & 1), side(i & 2), side(i & 4))).collect()
            }
            Shape::Sphere { radius } => ball(radius, 0.0),
            Shape::Capsule { half_height, radius } => ball(radius, half_height),
            Shape::Cylinder { half_height, radius } => ring(half_height, radius).chain(ring(-half_height, radius)).collect(),
        };
        let rotation = self.rotation();
        local.into_iter().map(|p| (self.pos + rotation * p) * scale).collect()
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
    /// Read a definition from text. Errors list every problem with its path, one per line.
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::read(text).map_err(|issues| issue::render(&issues))
    }

    /// Strict JSON, the whole-document check, then the typed model.
    pub fn read(text: &str) -> Result<Self, Vec<Issue>> {
        Self::from_value(read::strict_value(text).map_err(|issue| vec![issue])?)
    }

    /// The format-1 model of a checked document. A format-2 document is read and checked whole;
    /// it converts when format 1 holds it. Everything else reads documents as a `Doc`.
    pub fn from_value(value: Value) -> Result<Self, Vec<Issue>> {
        if read::migrate(&value).map_err(|issue| vec![issue])? == 2 {
            let doc = read::v2::doc(&value, &get)?;
            return doc.to_v1().ok_or_else(|| {
                let (path, what) = doc.needs_format2().unwrap_or_else(|| (String::new(), "this document".into()));
                vec![Issue::error(path, issue::Code::Format, format!("{what} needs format 2, which this model cannot hold"))]
            });
        }
        Self::from_v1_value(value)
    }

    /// The format-1 check, then the typed model. The typed validation stays as a backstop.
    pub(crate) fn from_v1_value(value: Value) -> Result<Self, Vec<Issue>> {
        let issues = read::check(&value);
        if !issues.is_empty() {
            return Err(issues);
        }
        let backstop = |error: String| vec![Issue::error("", issue::Code::Format, error)];
        let asset: Self = serde_json::from_value(value).map_err(|e| backstop(e.to_string()))?;
        asset.validate().map_err(backstop)?;
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
        format!("prop1-{:016x}", fnv(&bytes))
    }

    pub fn bounds(&self, scale: f32) -> Bounds {
        self.parts
            .values()
            .map(|part| part.bounds(scale))
            .reduce(Bounds::union)
            .unwrap_or(Bounds { min: Vec3::ZERO, max: Vec3::ZERO })
    }
}

/// FNV-1a, 64 bits: revisions are content identifiers for edit guards, not security hashes.
pub(crate) fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
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

/// A definition in the library: the document, its compiled parts and what it depends on.
#[derive(Debug)]
pub struct PropDef {
    pub doc: Doc,
    /// The document's revision (`prop1-…`, `prop2-…`).
    pub revision: String,
    /// What the compiled parts depend on: the revision, or with references `propk-…` over the
    /// revision and the referenced assets' keys.
    pub key: String,
    /// Assets the document's references name, sorted.
    pub deps: Vec<String>,
    /// How deep references nest below this asset: 0 without any.
    pub ref_depth: u8,
    /// Whether a seed changes anything (`chance`, `jitter` or `pick`, here or in a reference).
    /// Without them every seed draws `compiled`.
    pub varies: bool,
    /// Seed 0, which a placed copy draws unless it has a seed of its own.
    pub compiled: Arc<CompiledProp>,
    variants: Mutex<BTreeMap<u32, Arc<CompiledProp>>>,
    /// The definitions its references compiled against: every seed variant uses the same.
    pinned: BTreeMap<String, Arc<PropDef>>,
    /// The format-1 asset when format 1 holds the document: format-1 patches and the
    /// native editor's part drafts work on it.
    pub v1: Option<Arc<PropAsset>>,
}

/// Seed variants each definition keeps.
const VARIANTS: usize = 64;

impl PropDef {
    /// Compile a document against the library.
    pub fn new(doc: Doc) -> Result<Self, Vec<Issue>> {
        Self::with_deps(doc, &get, None)
    }

    /// A format-1 asset, keeping its shared `Arc` as the format-1 view.
    pub fn from_v1(asset: Arc<PropAsset>) -> Result<Self, Vec<Issue>> {
        asset.validate().map_err(|error| vec![Issue::error("", issue::Code::Format, error)])?;
        Self::with_deps(Doc::from_v1(&asset), &|_| None, Some(asset))
    }

    fn with_deps(doc: Doc, deps: compile::Deps, v1: Option<Arc<PropAsset>>) -> Result<Self, Vec<Issue>> {
        let compiled = compile::compile(&doc, deps, 0)?;
        let (mut names, mut meshes) = (BTreeSet::new(), BTreeSet::new());
        fn refs(parts: &BTreeMap<String, doc::Node>, names: &mut BTreeSet<String>, meshes: &mut BTreeSet<String>) {
            for node in parts.values() {
                match &node.kind {
                    doc::Kind::Ref(name) => {
                        names.insert(canonical(name).unwrap_or_else(|_| name.clone()));
                    }
                    doc::Kind::Mesh(name) => {
                        meshes.insert(name.clone());
                    }
                    doc::Kind::Group(children) => refs(children, names, meshes),
                    _ => {}
                }
            }
        }
        refs(&doc.parts, &mut names, &mut meshes);
        let pinned: BTreeMap<String, Arc<PropDef>> = names.iter().filter_map(|name| Some((name.clone(), deps(name)?))).collect();
        let ref_depth = pinned.values().map(|dep| dep.ref_depth + 1).max().unwrap_or(0);
        fn varies(parts: &BTreeMap<String, doc::Node>) -> bool {
            parts.values().any(|node| {
                node.chance.is_some() || node.jitter.is_some() || node.pick.is_some() || node.children().is_some_and(varies)
            })
        }
        let varies = varies(&doc.parts) || pinned.values().any(|dep| dep.varies);
        let revision = doc.revision();
        // Mesh files the parts drew, by content: a re-imported mesh changes the key.
        let mesh_keys: Vec<String> =
            meshes.iter().filter_map(|name| mesh::get(name).map(|m| format!("{name}:{:016x}", m.key))).collect();
        let key = if pinned.is_empty() && mesh_keys.is_empty() {
            revision.clone()
        } else {
            let closure: Vec<&str> = std::iter::once(revision.as_str())
                .chain(pinned.values().map(|dep| dep.key.as_str()))
                .chain(mesh_keys.iter().map(String::as_str))
                .collect();
            format!("propk-{:016x}", fnv(closure.join("|").as_bytes()))
        };
        let v1 = v1.or_else(|| doc.to_v1().map(Arc::new));
        Ok(Self {
            doc,
            revision,
            key,
            deps: names.into_iter().collect(),
            ref_depth,
            varies,
            compiled: Arc::new(compiled),
            variants: Mutex::default(),
            pinned,
            v1,
        })
    }

    /// A referenced definition as this one compiled against it.
    pub fn dep(&self, name: &str) -> Option<Arc<PropDef>> {
        self.pinned.get(&canonical(name).ok()?).cloned()
    }

    /// The parts for one seed. Seed 0 is `compiled`, and so is every seed of an asset that does
    /// not vary; others are compiled once and kept, up to 64.
    pub fn variant(&self, seed: u32) -> Arc<CompiledProp> {
        if seed == 0 || !self.varies {
            return self.compiled.clone();
        }
        if let Some(variant) = self.variants.lock().unwrap().get(&seed) {
            return variant.clone();
        }
        // The document compiled at install; a seed only changes chance, jitter and pick. The
        // references are the ones seed 0 used, whatever the library holds now.
        let variant =
            compile::compile(&self.doc, &|name| self.dep(name), seed).map(Arc::new).unwrap_or_else(|_| self.compiled.clone());
        let mut variants = self.variants.lock().unwrap();
        if variants.len() >= VARIANTS {
            let oldest = *variants.keys().next().expect("full");
            variants.remove(&oldest);
        }
        variants.insert(seed, variant.clone());
        variant
    }

    /// The parts' box for an instance at `scale` (seed 0).
    pub fn bounds(&self, scale: f32) -> Bounds {
        self.compiled.bounds_at(scale)
    }
}

/// Snapshots embed each instance's definition as its document, so a restored world keeps the
/// asset it was saved with. A format-1 definition writes exactly what format 1 always wrote.
/// A definition with references also carries the definitions they compiled against, as
/// `{"document": …, "refs": {"WORKSHOP/stool": …}}`, so a restore never reads the library.
pub mod embedded {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde::de::Error as _;
    use serde::ser::{Error as _, SerializeMap};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    use super::{Doc, PropAsset, PropDef, canonical};
    use crate::issue;

    pub fn serialize<S: Serializer>(def: &Arc<PropDef>, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(asset) = &def.v1 {
            return asset.serialize(serializer);
        }
        let document = serde_json::from_str::<Value>(&def.doc.text()).map_err(S::Error::custom)?;
        if def.pinned.is_empty() {
            return document.serialize(serializer);
        }
        struct Embedded<'a>(&'a Arc<PropDef>);
        impl Serialize for Embedded<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serialize(self.0, serializer)
            }
        }
        let refs: BTreeMap<&str, Embedded> = def.pinned.iter().map(|(name, dep)| (name.as_str(), Embedded(dep))).collect();
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("document", &document)?;
        map.serialize_entry("refs", &refs)?;
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<PropDef>, D::Error> {
        read(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }

    pub(super) fn read(value: Value) -> Result<Arc<PropDef>, String> {
        let def = match value {
            Value::Object(mut map) if map.contains_key("document") => {
                let refs = match map.remove("refs") {
                    Some(Value::Object(refs)) => refs,
                    _ => Default::default(),
                };
                let refs =
                    refs.into_iter().map(|(name, dep)| Ok((name, read(dep)?))).collect::<Result<BTreeMap<_, _>, String>>()?;
                let lookup = |name: &str| refs.get(&canonical(name).ok()?).cloned();
                let doc = Doc::from_value_in(&map["document"], &lookup).map_err(|issues| issue::render(&issues))?;
                PropDef::with_deps(doc, &lookup, None)
            }
            value if value.get("format").and_then(Value::as_u64) == Some(2) => {
                let doc = Doc::from_value_in(&value, &|_| None).map_err(|issues| issue::render(&issues))?;
                PropDef::with_deps(doc, &|_| None, None)
            }
            value => {
                let asset: PropAsset = serde_json::from_value(value).map_err(|e| e.to_string())?;
                PropDef::from_v1(Arc::new(asset))
            }
        };
        def.map(Arc::new).map_err(|issues| issue::render(&issues))
    }
}

#[derive(Clone, Debug, Default)]
pub struct PropLibrary {
    pub defs: BTreeMap<String, Arc<PropDef>>,
}

impl PropLibrary {
    fn insert(&mut self, name: String, def: Arc<PropDef>) {
        self.defs.insert(name, def);
    }

    /// Recompile everything that references `name`, directly or through other references.
    /// Returns the names recompiled; a dependent that no longer compiles keeps its last parts.
    fn recompile_dependents(&mut self, name: &str) -> Vec<String> {
        let mut changed = Vec::new();
        let mut queue = vec![name.to_owned()];
        while let Some(dep) = queue.pop() {
            let dependents: Vec<(String, Arc<PropDef>)> =
                self.defs.iter().filter(|(_, def)| def.deps.contains(&dep)).map(|(k, v)| (k.clone(), v.clone())).collect();
            for (dependent, old) in dependents {
                if changed.contains(&dependent) || dependent == name {
                    continue;
                }
                let lookup = |n: &str| self.defs.get(&canonical(n).ok()?).cloned();
                if let Ok(def) = PropDef::with_deps(old.doc.clone(), &lookup, old.v1.clone()) {
                    self.insert(dependent.clone(), Arc::new(def));
                    changed.push(dependent.clone());
                    queue.push(dependent);
                }
            }
        }
        changed
    }
}

static LIBRARY: OnceLock<RwLock<Arc<PropLibrary>>> = OnceLock::new();

fn shared() -> &'static RwLock<Arc<PropLibrary>> {
    LIBRARY.get_or_init(|| {
        // The templates are format 1 and compiled up front; `LIB/core` compiles on first use.
        let mut library = PropLibrary::default();
        for (name, text) in store::embedded_assets().filter(|(name, _)| name.starts_with("BUILTIN/")) {
            let asset = PropAsset::parse(text).unwrap_or_else(|e| panic!("built-in prop {name}: {e}"));
            assert_eq!(asset.name, leaf(&name), "template name matches its file");
            let def = PropDef::from_v1(Arc::new(asset)).unwrap_or_else(|e| panic!("built-in prop {name}: {}", issue::render(&e)));
            library.insert(name, Arc::new(def));
        }
        RwLock::new(Arc::new(library))
    })
}

pub fn library() -> Arc<PropLibrary> {
    shared().read().unwrap().clone()
}

/// A definition with its compiled parts. A library pack's asset is read and compiled on first
/// use: `LIB/core` from the embedded data, other packs from `disk_root()` (native builds).
pub fn get(name: &str) -> Option<Arc<PropDef>> {
    lookup(name, None).ok().flatten()
}

/// `get`, with why it failed: an unknown name (and the nearest known one), or a library file
/// that does not read or compile. Game code and rooms resolve their references with this.
pub fn resolve_ref(name: &str) -> Result<Arc<PropDef>, String> {
    lookup(name, None)?.ok_or_else(|| unknown_asset(name))
}

/// `get` from `root` instead of `disk_root()` (tests, tools). A pack asset on disk is read
/// and compiled afresh each call, and the shared library (which caches `disk_root()`'s) is
/// left as it was.
pub fn resolve_in(root: &std::path::Path, name: &str) -> Result<Option<Arc<PropDef>>, String> {
    lookup(name, Some(root))
}

thread_local! {
    /// Library assets being compiled on this thread: a reference back to one is unknown.
    static LOADING: std::cell::RefCell<BTreeSet<String>> = const { std::cell::RefCell::new(BTreeSet::new()) };
}

fn lookup(name: &str, root: Option<&std::path::Path>) -> Result<Option<Arc<PropDef>>, String> {
    let name = canonical(name)?;
    // An explicit root's pack assets are its own: the shared library caches `disk_root()`'s.
    let own = root.is_some() && store::pack(&name).is_some() && !store::embedded(&name);
    if let Some(def) = library().defs.get(&name).filter(|_| !own) {
        return Ok(Some(def.clone()));
    }
    if store::pack(&name).is_none() || LOADING.with(|loading| loading.borrow().contains(&name)) {
        return Ok(None);
    }
    let dir = root.map(std::path::Path::to_path_buf).unwrap_or_else(disk_root);
    let Some(text) = store::library_text(&dir, &name) else { return Ok(None) };
    LOADING.with(|loading| loading.borrow_mut().insert(name.clone()));
    let compiled = (|| {
        let error = |issues: Vec<Issue>| format!("{name}: {}", issue::render(&issues));
        let deps = |dep: &str| lookup(dep, root).ok().flatten();
        let doc = Doc::from_value_in(&strict_value(&text?).map_err(|e| error(vec![e]))?, &deps).map_err(error)?;
        if doc.name != leaf(&name) {
            return Err(format!("{name}: its file names \"{}\"; a library asset's name is its file's", doc.name));
        }
        PropDef::with_deps(doc, &deps, None).map_err(error)
    })();
    LOADING.with(|loading| loading.borrow_mut().remove(&name));
    if own {
        return Ok(Some(Arc::new(compiled?)));
    }
    install(&name, compiled?)?;
    Ok(library().defs.get(&name).cloned())
}

/// Every name `get` finds without reading a disk: the library's and the embedded assets'.
pub fn known_names() -> Vec<String> {
    let mut names: BTreeSet<String> = library().defs.keys().cloned().collect();
    names.extend(store::embedded_assets().map(|(name, _)| name));
    names.into_iter().collect()
}

/// Why `name` names no asset, with the closest known name when one is near.
pub fn unknown_asset(name: &str) -> String {
    let names = known_names();
    let wanted = canonical(name).unwrap_or_else(|_| name.to_string());
    match issue::suggestion(&wanted, names.iter().map(String::as_str)) {
        Some(fix) => format!("no prop '{name}' — {fix}"),
        None => format!("no prop '{name}' (assets find=WORDS searches the library)"),
    }
}

/// Install a compiled definition, keeping every other definition shared. Returns the
/// definitions that reference it and were recompiled, so their placed copies can be refreshed.
pub fn install(name: &str, def: PropDef) -> Result<Vec<String>, String> {
    let mut current = shared().write().unwrap();
    let name = installable(&current, name, &def)?;
    let mut next = (**current).clone();
    next.insert(name.clone(), Arc::new(def));
    let dependents = next.recompile_dependents(&name);
    *current = Arc::new(next);
    Ok(dependents)
}

/// Why `install` would refuse `def` as `name` now. Callers that save files check first.
pub fn check_install(name: &str, def: &PropDef) -> Result<(), String> {
    installable(&library(), name, def).map(|_| ())
}

fn installable(library: &PropLibrary, name: &str, def: &PropDef) -> Result<String, String> {
    let name = canonical(name)?;
    if leaf(&name) != def.doc.name {
        return Err(format!("{name} does not match the prop's name '{}'", def.doc.name));
    }
    if def.deps.contains(&name) || closure(library, &def.deps).contains(&name) {
        return Err(format!("{name} would reference itself"));
    }
    Ok(name)
}

/// Every asset `deps` reach through references.
fn closure(library: &PropLibrary, deps: &[String]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = deps.to_vec();
    while let Some(name) = stack.pop() {
        if seen.insert(name.clone()) {
            if let Some(def) = library.defs.get(&name) {
                stack.extend(def.deps.iter().cloned());
            }
        }
    }
    seen
}

/// The definitions whose references name `name` directly.
pub fn dependents(name: &str) -> Vec<String> {
    let Ok(name) = canonical(name) else { return Vec::new() };
    library().defs.iter().filter(|(_, def)| def.deps.contains(&name)).map(|(dependent, _)| dependent.clone()).collect()
}

/// Remove a workshop definition. One that others reference stays: they would lose their parts.
pub fn remove(name: &str) -> Result<bool, String> {
    let name = canonical(name)?;
    let mut current = shared().write().unwrap();
    removable(&current, &name)?;
    if !current.defs.contains_key(&name) {
        return Ok(false);
    }
    let mut next = (**current).clone();
    next.defs.remove(&name);
    *current = Arc::new(next);
    Ok(true)
}

/// Why `remove` would refuse `name` now. Callers that delete files check first.
pub fn check_remove(name: &str) -> Result<(), String> {
    removable(&library(), &canonical(name)?)
}

fn removable(library: &PropLibrary, name: &str) -> Result<(), String> {
    if name.starts_with("BUILTIN/") {
        return Err("built-in templates cannot be removed".into());
    }
    if !editable(name) {
        return Err(format!("{name} is a library asset; library assets cannot be removed"));
    }
    let users: Vec<&str> =
        library.defs.iter().filter(|(_, def)| def.deps.iter().any(|dep| dep == name)).map(|(n, _)| n.as_str()).collect();
    if !users.is_empty() {
        return Err(format!("{name} is referenced by {}; change or remove those first", users.join(", ")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn named_batches_keep_other_parts_and_reject_incomplete_transactions() {
        let original = (**get("BUILTIN/bench").unwrap().v1.as_ref().unwrap()).clone();
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
    fn canonical_text_is_exact_compact_and_keeps_revisions() {
        // Revisions captured before the canonical writer; F2 must keep them too. The bench's is
        // the one row 2.5 gave it, with its rear legs fixed.
        let golden = [
            ("BUILTIN/box", "prop1-501ba5dc44b112fd"),
            ("BUILTIN/crate", "prop1-464f1ac1ee324075"),
            ("BUILTIN/bench", "prop1-1dec2d266aa7c89d"),
            ("BUILTIN/lantern", "prop1-fc44b95ee89f262e"),
        ];
        let mut assets: Vec<(PropAsset, &str)> =
            golden.iter().map(|(name, revision)| ((**get(name).unwrap().v1.as_ref().unwrap()).clone(), *revision)).collect();
        for (text, revision) in [
            (include_str!("../../tests/fixtures/props/dogfood_stall_v1.json"), "prop1-bd43f9ba44a259b0"),
            (include_str!("../../tests/fixtures/props/dogfood_well_v1.json"), "prop1-368eeb091c94f947"),
        ] {
            assets.push((PropAsset::parse(text).unwrap(), revision));
        }
        for (asset, revision) in &assets {
            assert_eq!(asset.revision(), *revision, "{}", asset.name);
            let text = asset.text();
            let read = PropAsset::parse(&text).unwrap();
            assert_eq!(&read, asset, "{}", asset.name);
            assert_eq!(read.revision(), *revision);
            assert_eq!(read.text(), text, "the writer is idempotent");
            assert!(!text.contains("e-") && !text.contains("e+") && !text.contains(".0,") && !text.contains(".0]"));
            assert_eq!(text.lines().count(), asset.parts.len() + 2 + usize::from(!asset.description.is_empty()));
        }
        let bench = get("BUILTIN/bench").unwrap().doc.text();
        assert!(bench.len() <= 1_000, "a copied bench file is {} bytes", bench.len());
        assert!(bench.contains(r##""backrest":{"shape":{"type":"rounded_box","half":[1.1,0.25,0.04],"radius":0.025},"pos":[0,1.165,-0.43],"pitch":-11.5346,"color":"#bc8150"}"##));

        // Only bit-identical defaults are left out: negative zero survives a save.
        let mut odd = assets[0].0.clone();
        let body = odd.parts.get_mut("body").unwrap();
        body.pos = Vec3::new(-0.0, 0.0, 0.0);
        body.roll = -0.0;
        body.look = Look::Lit;
        body.emissive = 0.5;
        body.solid = false;
        body.shape = Shape::Capsule { half_height: 0.0, radius: 1.0e-3 };
        let text = odd.text();
        assert!(
            text.contains(r#""shape":{"type":"capsule","radius":0.001,"half_height":0},"pos":[-0.0,0,0],"roll":-0.0"#),
            "{text}"
        );
        assert!(text.contains(r#""look":"lit","emissive":0.5,"solid":false}"#), "{text}");
        assert_eq!(PropAsset::parse(&text).unwrap().revision(), odd.revision());
    }

    fn errors(text: &str) -> Vec<String> {
        PropAsset::parse(text).unwrap_err().lines().map(String::from).collect()
    }

    #[test]
    fn every_problem_names_its_path_with_the_valid_values_and_a_fix() {
        let doc = |part: &str| format!(r#"{{"format":1,"name":"probe","parts":{{"p":{part}}}}}"#);
        let cases = [
            (
                r#"{"shape":{"type":"cone","radius":0.3,"half_height":0.5}}"#,
                r#"parts.p.shape.type: unknown shape type "cone" (expected box, rounded_box, sphere, capsule, cylinder) — cone needs format 2; in format 1 use a cylinder"#,
            ),
            (
                r#"{"shape":{"type":"box","half":[0.1,0.1,0.1]},"rotation":[0,45,0]}"#,
                "parts.p.rotation: unknown key (expected shape, pos, yaw, pitch, roll, color, look, emissive, solid) — use yaw, pitch and roll in degrees",
            ),
            (
                r##"{"shape":{"type":"box","half":[0.1,0.1,0.1]},"color":"#f00"}"##,
                r##"parts.p.color: "#f00" is not a #rrggbb colour — write all six digits: "#ff0000""##,
            ),
            (
                r#"{"shape":{"type":"cylinder","height":1,"radius":0.2}}"#,
                r#"parts.p.shape.height: unknown key (expected type, radius, half_height) — use "half_height":0.5 (height = 2*half_height)"#,
            ),
            (r#"{"shape":{"type":"sphere","radius":-0.2}}"#, "parts.p.shape.radius: -0.2 is outside 0.001 to 1000"),
            (r#"{"shape":{"type":"box","half":[0.1,0.1,0.1]},"pos":[0,1]}"#, "parts.p.pos: expected 3 numbers [x,y,z], got 2"),
            (
                r#"{"shape":{"type":"box","half":[0.1,0.1,0.1]},"look":"toon"}"#,
                r#"parts.p.look: unknown look "toon" (expected flat, cel, lit, unlit, cutout) — cel is the toon look"#,
            ),
            (
                r#"{"shape":{"type":"box","half":[0.1,0.1,0.1]},"yaw":"45"}"#,
                r#"parts.p.yaw: expected a number, got the string "45" — write 45 without quotes"#,
            ),
            (r#"{"shape":{"type":"box","half":[0,0.1,0.1]}}"#, "parts.p.shape.half[0]: 0 is outside 0.001 to 1000"),
            (
                r#"{"shape":{"type":"rounded_box","half":[0.1,0.1,0.1],"radius":0.2}}"#,
                "parts.p.shape.radius: 0.2 is outside 0 to 0.1",
            ),
        ];
        for (part, expected) in cases {
            assert_eq!(errors(&doc(part)), [expected], "{part}");
        }
        let size = errors(&doc(r#"{"shape":{"type":"box","size":[1,1,1]}}"#));
        assert_eq!(
            size,
            [r#"parts.p.shape.size: unknown key (expected type, half) — box sizes are half extents: "half":[0.5,0.5,0.5]"#]
        );
        assert_eq!(
            errors(&doc(r#"{"shape":{"type":"rounded_box","half":[1,1,1]}}"#)),
            ["parts.p.shape.radius: missing (expected rounded_box needs half and radius)"]
        );
        // Values exactly at an f32 bound pass, as the typed model reads them.
        assert!(PropAsset::parse(&doc(r#"{"shape":{"type":"sphere","radius":0.001}}"#)).is_ok());
        let document = errors(r#"{"format":1,"name":"Bad Name","colour":"x","parts":{}}"#);
        assert_eq!(
            document,
            [
                r#"colour: unknown key (expected format, name, description, parts) — spelled "color""#,
                r#"name: "Bad Name" is not a usable name: use 1 to 64 lowercase letters, digits, underscores or hyphens, starting with a letter"#,
                "parts: 0 parts (expected 1 to 256)",
            ]
        );
        // Format 2 reads through its own check; a newer format is refused before any other check.
        assert_eq!(
            errors(r#"{"format":2,"name":"Bad Name","colour":"x","parts":{}}"#),
            [
                r#"colour: unknown key — spelled "color""#,
                r#"unit: missing (expected cm or m) — add "unit":"cm" (the house unit) or "unit":"m""#,
                r#"name: "Bad Name" is not a usable name: use 1 to 64 lowercase letters, digits, underscores or hyphens, starting with a letter"#,
                "parts: 0 nodes (expected 1 to 256, groups and refs included)",
            ]
        );
        assert_eq!(errors(r#"{"format":3,"name":"x","parts":{}}"#), ["format: format 3 needs a newer engine (expected 1 or 2)"]);
        assert_eq!(errors("{\"format\":1,"), ["(root): not valid JSON: EOF while parsing a value at line 1 column 12"]);
    }

    #[test]
    fn patch_errors_name_the_operation_and_change_nothing() {
        let bench = get("BUILTIN/bench").unwrap().v1.clone().unwrap();
        let patch = |ops: serde_json::Value| bench.patch(&ops).unwrap_err();
        assert_eq!(
            patch(
                json!([{"op":"set","part":"seat","fields":{"color":"#ffffff"}},{"op":"set","part":"crossbar","fields":{"color":"grey"}}])
            ),
            r##"ops[1] crossbar.color: "grey" is not a #rrggbb colour — use a hex colour such as "#808080""##
        );
        assert_eq!(
            patch(json!([{"op":"set","part":"seta","fields":{}}])),
            r#"ops[0] seta: no part "seta" — did you mean "seat"?"#
        );
        assert_eq!(
            patch(json!([{"op":"add","part":"seat","value":{}}])),
            r#"ops[0] seat: part "seat" already exists — use op:set to change it"#
        );
        assert_eq!(
            patch(json!([{"op":"set","part":"seat","fields":{"shape":{"radius":0.8}}}])),
            "ops[0] seat.shape.type: missing (expected box, rounded_box, sphere, capsule, cylinder) — set replaces the whole shape: give the type and every field, e.g. {\"type\":\"box\",\"half\":[0.5,0.5,0.5]}"
        );
        assert_eq!(
            patch(json!([{"op":"mirror","path":"leg_front_left","side":"both"}])),
            "ops[0].op: unknown op \"mirror\" (expected set, add, remove) — format 1 batches use set, add and remove: add the new part with op:add"
        );
        assert_eq!(
            patch(json!([{"op":"set","part":"seat","fields":{"colour":"#ffffff"}}])),
            r#"ops[0] seat.colour: unknown key (expected shape, pos, yaw, pitch, roll, color, look, emissive, solid) — spelled "color""#
        );
        assert_eq!(
            patch(json!({"op":"set"})),
            "ops: expected an array of operations — ops:[{\"op\":\"set\",\"part\":\"seat\",\"fields\":{\"color\":\"#4060a0\"}}]"
        );
        let changed = bench.patch(&json!([{"op":"set","part":"seat","fields":{"pitch":-3}}])).unwrap();
        assert_eq!(changed.parts["seat"].pitch, -3.0);
        assert_eq!(changed.parts["backrest"], bench.parts["backrest"]);
    }

    #[test]
    fn schema_covers_every_field() {
        // The checker's tables are the typed model's fields, and the field guide shows them all.
        let typed = |value: serde_json::Value| value.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>();
        let table = |rows: &[(&str, &str)]| rows.iter().map(|(key, _)| key.to_string()).collect::<BTreeSet<_>>();
        let bench = get("BUILTIN/bench").unwrap().v1.clone().unwrap();
        assert_eq!(typed(serde_json::to_value(&*bench).unwrap()), table(DOCUMENT_FIELDS));
        assert_eq!(typed(serde_json::to_value(PropPart::default()).unwrap()), table(PART_FIELDS));
        let shapes = [
            Shape::Box { half: Vec3::ONE },
            Shape::RoundedBox { half: Vec3::ONE, radius: 0.1 },
            Shape::Sphere { radius: 1.0 },
            Shape::Capsule { half_height: 1.0, radius: 1.0 },
            Shape::Cylinder { half_height: 1.0, radius: 1.0 },
        ];
        for shape in shapes {
            let value = serde_json::to_value(shape).unwrap();
            let kind = value["type"].as_str().unwrap().to_owned();
            let (_, fields, _) = SHAPES.iter().find(|(name, _, _)| *name == kind.as_str()).unwrap();
            let mut expected: BTreeSet<String> = fields.iter().map(|f| f.to_string()).collect();
            expected.insert("type".into());
            assert_eq!(typed(value), expected, "{kind}");
        }
        assert_eq!(
            LOOKS,
            ["flat", "cel", "lit", "unlit", "cutout"].map(|look| {
                serde_json::to_value(serde_json::from_value::<Look>(json!(look)).unwrap()).unwrap().as_str().unwrap().to_owned()
            })
        );
        let guide = schema(None).unwrap();
        for (key, _) in DOCUMENT_FIELDS {
            assert!(guide["document"].get(*key).is_some(), "{key}");
        }
        for (key, _) in PART_FIELDS {
            assert!(guide["part"].get(*key).is_some(), "{key}");
        }
        for (kind, fields, line) in SHAPES {
            assert_eq!(guide["shapes"][*kind], *line);
            for field in *fields {
                assert!(line.contains(field), "{kind} guide names {field}");
            }
        }
        for (op, fields, line) in OPS {
            assert_eq!(guide["ops"][*op], *line);
            for field in *fields {
                assert!(line.contains(field), "{op} guide names {field}");
            }
        }
        assert_eq!(schema(Some("shapes")).unwrap().as_object().unwrap().len(), 3);
        assert_eq!(
            schema(Some("shape")).unwrap_err(),
            r#"topic: unknown topic "shape" (expected legend, document, part, shapes, ops, limits) — did you mean "shapes"?"#
        );
    }

    #[test]
    fn installing_a_referenced_asset_recompiles_what_uses_it() {
        let post = |height: u32| {
            let text = format!(
                r##"{{"format":2,"name":"registry_post","unit":"cm","parts":{{"post":{{"type":"box","size":[10,{height},10],"pos":[0,{},0],"color":"#806040"}}}}}}"##,
                height / 2
            );
            PropDef::new(Doc::read(&text).unwrap()).unwrap()
        };
        install("WORKSHOP/registry_post", post(100)).unwrap();
        let pair = Doc::read(
            r#"{"format":2,"name":"registry_pair","unit":"cm","parts":{
"a":{"ref":"WORKSHOP/registry_post","pos":[-50,0,0]},
"b":{"ref":"WORKSHOP/registry_post","pos":[50,0,0]}}}"#,
        )
        .unwrap();
        let pair = PropDef::new(pair).unwrap();
        assert_eq!((pair.deps.as_slice(), pair.ref_depth), (["WORKSHOP/registry_post".to_owned()].as_slice(), 1));
        assert!(pair.key.starts_with("propk-") && pair.revision.starts_with("prop2-"));
        assert_eq!(install("WORKSHOP/registry_pair", pair).unwrap(), Vec::<String>::new());
        let before = get("WORKSHOP/registry_pair").unwrap();
        assert_eq!(before.compiled.parts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a.post", "b.post"]);
        assert!(before.v1.is_none(), "format 2 has no format-1 view");

        // A taller post: the pair recompiles with a new key and the same revision.
        assert_eq!(install("WORKSHOP/registry_post", post(200)).unwrap(), ["WORKSHOP/registry_pair"]);
        let after = get("WORKSHOP/registry_pair").unwrap();
        assert_eq!(after.revision, before.revision);
        assert_ne!(after.key, before.key);
        assert!((after.compiled.bounds.max.y - 2.0).abs() < 1e-6);

        // A post that references the pair would reference itself.
        let looped =
            Doc::read(r#"{"format":2,"name":"registry_post","unit":"cm","parts":{"pair":{"ref":"WORKSHOP/registry_pair"}}}"#)
                .unwrap();
        let error = install("WORKSHOP/registry_post", PropDef::new(looped).unwrap()).unwrap_err();
        assert_eq!(error, "WORKSHOP/registry_post would reference itself");

        // A seed's variant is compiled once.
        assert!(Arc::ptr_eq(&after.variant(7), &after.variant(7)) && Arc::ptr_eq(&after.variant(0), &after.compiled));
        assert!(remove("WORKSHOP/registry_pair").unwrap() && remove("WORKSHOP/registry_post").unwrap());
        assert!(get("WORKSHOP/registry_pair").is_none());
    }

    #[test]
    fn references_vary_by_copy_and_seed_and_keep_what_they_compiled_against() {
        let lamp = |height: u32| {
            let text = format!(
                r##"{{"format":2,"name":"vary_lamp","unit":"cm","materials":{{"wood":{{"color":"#806040"}}}},"parts":{{
"post":{{"type":"box","size":[10,{height},10],"pos":[0,{},0],"mat":"wood","jitter":{{"pos":[5,0,5]}}}},
"cap":{{"type":"sphere","radius":8,"pos":[0,{},0],"mat":"wood","color":"#ffcc00","light":{{"radius":300}}}}}}}}"##,
                height / 2,
                height + 8
            );
            PropDef::new(Doc::read(&text).unwrap()).unwrap()
        };
        install("WORKSHOP/vary_lamp", lamp(100)).unwrap();
        let pair = |parts: &str| {
            let text = format!(
                r##"{{"format":2,"name":"vary_pair","unit":"cm","materials":{{"oak":{{"color":"#c08040"}}}},"parts":{{{parts}}}}}"##
            );
            Doc::read(&text)
        };
        let doc = pair(
            r#""a":{"ref":"WORKSHOP/vary_lamp","pos":[-100,0,0],"remap":{"wood":"oak"}},"b":{"ref":"WORKSHOP/vary_lamp","pos":[100,0,0]}"#,
        )
        .unwrap();
        let def = PropDef::new(doc).unwrap();
        let part = |compiled: &CompiledProp, id: &str| compiled.parts.iter().find(|p| p.id == id).unwrap().clone();
        // Each copy of a reference draws its own variation, and the seed reaches inside.
        let (a, b) = (part(&def.compiled, "a.post"), part(&def.compiled, "b.post"));
        assert_ne!((a.pos.x + 1.0, a.pos.z), (b.pos.x - 1.0, b.pos.z), "two references vary apart");
        assert_ne!(part(&def.variant(7), "a.post").pos, a.pos, "the seed varies a referenced part");
        // remap swaps the material inside the reference's cascade: an explicit colour still wins.
        // Inline materials belong to their asset: the swapped-in one to this, the other to the lamp.
        let mat = |p: &ResolvedPart| p.mat.as_ref().map(|m| m.name.clone());
        assert_eq!((a.color.as_deref(), a.surface_color(), mat(&a).as_deref()), (None, "#c08040", Some("#oak")));
        assert_eq!((b.surface_color(), mat(&b).as_deref()), ("#806040", Some("WORKSHOP/vary_lamp#wood")));
        assert_eq!(part(&def.compiled, "a.cap").color.as_deref(), Some("#ffcc00"));
        assert_eq!(def.compiled.lights.iter().map(|l| l.part.as_str()).collect::<Vec<_>>(), ["a.cap", "b.cap"]);
        // A remap names the referenced asset's materials; its lights count toward this one's.
        assert_eq!(
            issue::render(&pair(r#""a":{"ref":"WORKSHOP/vary_lamp","remap":{"wod":"oak"}}"#).unwrap_err()),
            r#"parts.a.remap.wod: WORKSHOP/vary_lamp has no material "wod" (expected wood) — did you mean "wood"?"#
        );
        assert_eq!(
            issue::render(&pair(r#""row":{"ref":"WORKSHOP/vary_lamp","array":{"count":5,"step":[50,0,0]}}"#).unwrap_err()),
            "parts: 5 lights after copies (expected at most 4)"
        );
        // A referenced asset stays while something uses it.
        install("WORKSHOP/vary_pair", def).unwrap();
        let held = get("WORKSHOP/vary_pair").unwrap();
        assert_eq!(dependents("WORKSHOP/vary_lamp"), ["WORKSHOP/vary_pair"]);
        assert_eq!(
            remove("WORKSHOP/vary_lamp").unwrap_err(),
            "WORKSHOP/vary_lamp is referenced by WORKSHOP/vary_pair; change or remove those first"
        );
        // A taller lamp recompiles the pair; a copy that still holds the old pair keeps the old
        // lamp in every seed.
        assert_eq!(install("WORKSHOP/vary_lamp", lamp(200)).unwrap(), ["WORKSHOP/vary_pair"]);
        let height = |compiled: &CompiledProp| part(compiled, "a.post").bounds(1.0).size().y;
        assert!((height(&get("WORKSHOP/vary_pair").unwrap().compiled) - 2.0).abs() < 1e-5);
        assert!((height(&held.variant(11)) - 1.0).abs() < 1e-5, "variants use the lamp the pair compiled against");
        assert!(remove("WORKSHOP/vary_pair").unwrap() && remove("WORKSHOP/vary_lamp").unwrap());
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
        for (name, def) in before.defs.iter().filter(|(name, _)| name.starts_with("BUILTIN/")) {
            let asset = def.v1.as_ref().expect("templates are format 1");
            assert_eq!(name, &format!("BUILTIN/{}", asset.name));
            let parsed = PropAsset::parse(&asset.text()).unwrap();
            assert_eq!(asset.revision(), parsed.revision());
            assert!(asset.bounds(1.0).size().min_element() > 0.0);
            let mut bytes = Vec::new();
            ciborium::into_writer(asset.as_ref(), &mut bytes).unwrap();
            let snapshot: PropAsset = ciborium::from_reader(bytes.as_slice()).unwrap();
            assert_eq!(snapshot, **asset, "part shape serde also works in snapshots");
        }
        let mut invalid = (**before.defs["BUILTIN/box"].v1.as_ref().unwrap()).clone();
        invalid.name = "invalid_test_prop".into();
        invalid.parts.get_mut("body").unwrap().pos.x = f32::INFINITY;
        assert!(PropDef::from_v1(Arc::new(invalid)).is_err());
        assert!(get("WORKSHOP/invalid_test_prop").is_none());
        assert!(Arc::ptr_eq(&before.defs["BUILTIN/bench"], &library().defs["BUILTIN/bench"]));
        assert!(canonical("../../bad").is_err());
        assert!(canonical("WORKSHOP/con").is_err());
        assert_eq!(canonical("Workshop/My_Bench").unwrap(), "WORKSHOP/my_bench");
        let duplicate = r##"{"format":1,"name":"duplicate","parts":{"body":{"shape":{"type":"sphere","radius":1}},"body":{"shape":{"type":"sphere","radius":2}}}}"##;
        assert_eq!(
            PropAsset::parse(duplicate).unwrap_err(),
            r#"parts.body: "body" appears twice — keep one; names must be unique"#
        );
    }
}
