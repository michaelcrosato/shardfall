//! The source model of a prop: the document as written, in its own unit, with each optional
//! field kept as written (None: not written). Both formats read into it; a format-1 document
//! is in metres and its nested `shape` maps onto the flat part. `needed_format` names the
//! lowest format that holds a document, and the writer emits that format.

use std::collections::BTreeMap;

use glam::Vec3;
use serde_json::{Map, Value};

use super::{PropAsset, PropPart, read};
use crate::issue::Issue;
use crate::shape::{Look, Shape, Sway};

/// A closed set of names, read and written as the strings listed.
macro_rules! names {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $var:ident => $label:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($(#[$vm])* $var),+ }

        impl $name {
            pub const NAMES: &'static [&'static str] = &[$($label),+];
            const ALL: &'static [$name] = &[$($name::$var),+];

            pub fn name(self) -> &'static str {
                Self::NAMES[self as usize]
            }

            pub fn parse(text: &str) -> Option<Self> {
                Self::NAMES.iter().position(|name| *name == text).map(|index| Self::ALL[index])
            }
        }
    };
}

names! {
    /// Length unit of a document (D1: cm is the house unit). Rooms, placement and physics stay
    /// in metres.
    Unit { Cm => "cm", M => "m" }
}

impl Unit {
    /// Document units in one metre.
    pub fn per_metre(self) -> f32 {
        match self {
            Unit::Cm => 100.0,
            Unit::M => 1.0,
        }
    }
}

names! {
    /// A direction along a frame axis: a round part's axis, an arc's facing, a radial axis.
    Dir { X => "x", Y => "y", Z => "z", NegX => "-x", NegY => "-y", NegZ => "-z" }
}

impl Dir {
    pub fn vector(self) -> Vec3 {
        match self {
            Dir::X => Vec3::X,
            Dir::Y => Vec3::Y,
            Dir::Z => Vec3::Z,
            Dir::NegX => Vec3::NEG_X,
            Dir::NegY => Vec3::NEG_Y,
            Dir::NegZ => Vec3::NEG_Z,
        }
    }
}

names! {
    /// A frame axis without a sign: mirror planes and grid planes.
    Axis { X => "x", Y => "y", Z => "z" }
}

names! {
    /// Primitive types. The first five are format 1's shapes.
    PrimType {
        Box => "box",
        RoundedBox => "rounded_box",
        Sphere => "sphere",
        Capsule => "capsule",
        Cylinder => "cylinder",
        Ellipsoid => "ellipsoid",
        Cone => "cone",
        Tube => "tube",
        Torus => "torus",
        Wedge => "wedge",
        Lathe => "lathe",
        Extrude => "extrude",
    }
}

impl PrimType {
    pub fn in_format1(self) -> bool {
        (self as usize) < 5
    }

    /// Types that take `from`/`to`: their ends are the visible ends.
    pub fn takes_ends(self) -> bool {
        matches!(self, Self::Box | Self::RoundedBox | Self::Cylinder | Self::Cone | Self::Tube | Self::Capsule)
    }

    /// Types with their own axis (`axis`); extrude's is its depth.
    pub fn takes_axis(self) -> bool {
        matches!(self, Self::Capsule | Self::Cylinder | Self::Cone | Self::Tube | Self::Torus | Self::Lathe | Self::Extrude)
    }

    pub fn default_axis(self) -> Dir {
        if self == Self::Extrude { Dir::Z } else { Dir::Y }
    }

    /// Types drawn as a sector with `arc` and `facing`.
    pub fn takes_arc(self) -> bool {
        matches!(self, Self::Cylinder | Self::Cone | Self::Tube | Self::Torus)
    }
}

names! {
    /// What an asset is, for retrieval.
    Category {
        Furniture => "furniture",
        Container => "container",
        Architecture => "architecture",
        Lighting => "lighting",
        Vegetation => "vegetation",
        Rock => "rock",
        Weapon => "weapon",
        Armour => "armour",
        Tool => "tool",
        Food => "food",
        Town => "town",
        Dungeon => "dungeon",
        Signage => "signage",
    }
}

names! {
    /// Socket names: where other things attach. +y is the socket's working direction.
    SocketName {
        Grip => "grip",
        Grip2 => "grip2",
        /// +y along the blade.
        Tip => "tip",
        /// +y up.
        Top => "top",
        /// +y into the wall.
        WallMount => "wall_mount",
        Hang => "hang",
        Sit => "sit",
    }
}

names! {
    /// Material layer patterns (closed v1 vocabulary).
    Pattern {
        Noise => "noise",
        Voronoi => "voronoi",
        Wave => "wave",
        Brick => "brick",
        Checker => "checker",
        Gradient => "gradient",
        Magic => "magic",
        Planks => "planks",
        Tiles => "tiles",
        Cobbles => "cobbles",
        Stripes => "stripes",
        Dots => "dots",
        Shingles => "shingles",
        Wood => "wood",
    }
}

names! {
    /// How a material's pattern is laid on a part.
    MapMode { Object => "object", World => "world", Axis => "axis" }
}

/// Theme colour roles; a colour may name one instead of a hex value.
pub const ROLES: &[&str] = &["$floor", "$floor2", "$wall", "$pillar", "$accent", "$light"];

/// Particle presets for `fx`.
pub const PRESETS: &[&str] = &[
    "fire",
    "smoke",
    "sparks",
    "embers",
    "fountain",
    "snow",
    "rain",
    "fireflies",
    "magic",
    "steam",
    "dust",
    "bubbles",
    "confetti",
];

/// A prop document.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub name: String,
    pub unit: Unit,
    /// Root surface defaults, which every part inherits.
    pub surface: Surface,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<Category>,
    pub source: Option<Source>,
    pub fit: Option<Fit>,
    pub materials: BTreeMap<String, MaterialEntry>,
    pub parts: BTreeMap<String, Node>,
    pub sockets: BTreeMap<String, Socket>,
}

/// Surface fields, which cascade from the root through groups to parts: at each level an
/// explicit field wins, then that level's `mat`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub mat: Option<String>,
    /// `#rrggbb` or a role such as `$wall`.
    pub color: Option<String>,
    pub look: Option<Look>,
    pub emissive: Option<f32>,
    pub solid: Option<bool>,
}

impl Surface {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One node of the part tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub from: Option<Vec3>,
    pub to: Option<Vec3>,
    pub pos: Option<Vec3>,
    pub attach: Option<Attach>,
    pub offset: Option<Vec3>,
    pub axis: Option<Dir>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub roll: Option<f32>,
    pub spin: Option<f32>,
    pub surface: Surface,
    pub hidden: Option<bool>,
    pub light: Option<Light>,
    pub fx: Option<Fx>,
    pub sway: Option<Sway>,
    /// Mirror planes in x, y, z order; empty: not mirrored.
    pub mirror: Vec<Axis>,
    pub replicate: Option<Replicate>,
    pub chance: Option<f32>,
    pub jitter: Option<Jitter>,
    pub pick: Option<u32>,
    pub scale: Option<f32>,
    pub remap: Option<BTreeMap<String, String>>,
    pub recolor: Option<BTreeMap<String, String>>,
}

impl Node {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            from: None,
            to: None,
            pos: None,
            attach: None,
            offset: None,
            axis: None,
            yaw: None,
            pitch: None,
            roll: None,
            spin: None,
            surface: Surface::default(),
            hidden: None,
            light: None,
            fx: None,
            sway: None,
            mirror: Vec::new(),
            replicate: None,
            chance: None,
            jitter: None,
            pick: None,
            scale: None,
            remap: None,
            recolor: None,
        }
    }

    pub fn prim(&self) -> Option<&Prim> {
        match &self.kind {
            Kind::Prim(prim) => Some(prim),
            _ => None,
        }
    }

    pub fn children(&self) -> Option<&BTreeMap<String, Node>> {
        match &self.kind {
            Kind::Group(parts) => Some(parts),
            _ => None,
        }
    }

    /// Copies this node makes before mirroring: 1, or the replication's count.
    pub fn copies(&self) -> usize {
        match &self.replicate {
            None => 1,
            Some(Replicate::Array(array)) => array.count as usize,
            Some(Replicate::Grid(grid)) => grid.count[0] as usize * grid.count[1] as usize,
            Some(Replicate::Radial(radial)) => radial.count as usize,
        }
    }
}

/// What a node is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Prim(Prim),
    /// Another asset's resolved parts, by library name.
    Ref(String),
    /// A tool-written mesh file, by `<pack>/<name>`.
    Mesh(String),
    /// Children placed in this node's frame.
    Group(BTreeMap<String, Node>),
}

/// A primitive's type and shape fields, as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Prim {
    pub kind: PrimType,
    /// Full size: 3 values, or `[across, thick]` for a box placed with from/to.
    pub size: Option<Vec<f32>>,
    pub half: Option<Vec3>,
    pub radius: Option<f32>,
    pub radius_end: Option<f32>,
    pub minor_radius: Option<f32>,
    pub thickness: Option<f32>,
    pub height: Option<f32>,
    pub half_height: Option<f32>,
    pub arc: Option<f32>,
    pub facing: Option<Dir>,
    pub peak: Option<f32>,
    pub profile: Option<Vec<[f32; 2]>>,
    pub points: Option<Vec<[f32; 2]>>,
    pub depth: Option<f32>,
    pub segments: Option<u32>,
}

impl Prim {
    pub fn new(kind: PrimType) -> Self {
        Self {
            kind,
            size: None,
            half: None,
            radius: None,
            radius_end: None,
            minor_radius: None,
            thickness: None,
            height: None,
            half_height: None,
            arc: None,
            facing: None,
            peak: None,
            profile: None,
            points: None,
            depth: None,
            segments: None,
        }
    }
}

/// `attach`: the node's position comes from a point on another part.
#[derive(Clone, Debug, PartialEq)]
pub struct Attach {
    /// A resolved id: `seat`, `leg[1]`, `arm~x.hand`, or a group.
    pub to: String,
    /// Where on the target (default top).
    pub at: Option<Anchor>,
    /// Which point of this node goes there (default: the side facing the target).
    pub own: Option<Anchor>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// `top`, `bottom_front`, … in the parent frame, or `from`/`to` (the target's ends).
    Word(String),
    /// `[u,v,w]` in -1..1 over the target's own box.
    Point([f32; 3]),
}

/// Copies of a node, centred on its position.
#[derive(Clone, Debug, PartialEq)]
pub enum Replicate {
    Array(Array),
    Grid(Grid),
    Radial(Radial),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    pub count: u32,
    pub step: Option<Vec3>,
    pub span: Option<Vec3>,
    pub cycle: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub count: [u32; 2],
    pub step: [f32; 2],
    pub plane: Option<[Axis; 2]>,
    pub cycle: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Radial {
    pub count: u32,
    pub radius: f32,
    pub start: Option<f32>,
    pub arc: Option<f32>,
    pub axis: Option<Dir>,
    pub cycle: Option<Vec<String>>,
}

impl Replicate {
    pub fn cycle(&self) -> Option<&Vec<String>> {
        match self {
            Replicate::Array(array) => array.cycle.as_ref(),
            Replicate::Grid(grid) => grid.cycle.as_ref(),
            Replicate::Radial(radial) => radial.cycle.as_ref(),
        }
    }
}

/// Seeded variation per copy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Jitter {
    pub pos: Option<Vec3>,
    /// Degrees of yaw, pitch and roll.
    pub rot: Option<Vec3>,
    pub scale: Option<[f32; 2]>,
    pub color: Option<f32>,
}

/// A point light at the part (`LightDef` without its offset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Light {
    pub color: Option<String>,
    pub radius: Option<f32>,
    pub intensity: Option<f32>,
    pub flicker: Option<f32>,
    pub pulse: Option<f32>,
    pub shadows: Option<bool>,
}

/// A particle emitter at the part (`EmitterDef` without offset and ground).
#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    pub preset: String,
    pub rate: Option<f32>,
    pub color: Option<String>,
    pub size: Option<f32>,
    pub speed: Option<f32>,
    pub life: Option<f32>,
    pub area: Option<Vec3>,
}

/// A material: a library name (`MAT/…`) or written inline.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialEntry {
    Library(String),
    Inline(Material),
}

/// A material written inline, read by `material::read_inline` in the asset's unit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    /// The fields as read: lengths in metres, the base chain not yet applied.
    pub fields: crate::material::Fields,
    /// The checked JSON as written (lengths in the asset's unit), which the writer lays out in
    /// canonical order.
    pub written: Map<String, Value>,
}

/// A named point where other things attach.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Socket {
    pub pos: Option<Vec3>,
    pub attach: Option<Attach>,
    pub offset: Option<Vec3>,
    pub axis: Option<Dir>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub roll: Option<f32>,
}

/// Where an asset came from (written by tools; kept on copy).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub library: Option<String>,
    pub url: Option<String>,
    /// An SPDX identifier.
    pub license: Option<String>,
    pub author: Option<String>,
    pub sha256: Option<String>,
    pub title: Option<String>,
    /// How an importer brought the model into the engine's frame when that was not the
    /// default (`asset_import`'s words: `up=z yaw=90 scale=0.01 place=floor`), so it can read
    /// the source the same way again.
    pub norm: Option<String>,
}

/// How closely a translated asset fits its source (written by tools).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    pub mean_mm: Option<f32>,
    pub worst_mm: Option<f32>,
    pub iou: Option<f32>,
    pub de: Option<f32>,
    pub mesh_pct: Option<f32>,
    pub parts_hash: Option<String>,
}

impl Doc {
    /// Read either format from text: strict JSON, the whole-document check, the typed model.
    pub fn read(text: &str) -> Result<Self, Vec<Issue>> {
        Self::from_value(&read::strict_value(text).map_err(|issue| vec![issue])?)
    }

    /// Read a document of either format. Errors list every problem with its path.
    pub fn from_value(value: &Value) -> Result<Self, Vec<Issue>> {
        Self::from_value_in(value, &super::get)
    }

    /// `from_value` with references found in `assets` instead of the library.
    pub(crate) fn from_value_in(
        value: &Value,
        assets: &dyn Fn(&str) -> Option<std::sync::Arc<super::PropDef>>,
    ) -> Result<Self, Vec<Issue>> {
        match read::migrate(value).map_err(|issue| vec![issue])? {
            2 => read::v2::doc(value, assets),
            _ => PropAsset::from_v1_value(value.clone()).map(|asset| Self::from_v1(&asset)),
        }
    }

    /// A format-1 asset in the source model. `to_v1` gives it back unchanged.
    pub fn from_v1(asset: &PropAsset) -> Self {
        // Format 1 leaves out only bit-identical defaults, so negative zero stays written.
        let written = |v: Vec3| v.to_array().iter().any(|x| x.to_bits() != 0).then_some(v);
        let number = |x: f32| (x.to_bits() != 0).then_some(x);
        let parts = asset
            .parts
            .iter()
            .map(|(name, part)| {
                let prim = match part.shape {
                    Shape::Box { half } => Prim { half: Some(half), ..Prim::new(PrimType::Box) },
                    Shape::RoundedBox { half, radius } => {
                        Prim { half: Some(half), radius: Some(radius), ..Prim::new(PrimType::RoundedBox) }
                    }
                    Shape::Sphere { radius } => Prim { radius: Some(radius), ..Prim::new(PrimType::Sphere) },
                    Shape::Capsule { half_height, radius } => Prim::new(PrimType::Capsule).round(radius, half_height),
                    Shape::Cylinder { half_height, radius } => Prim::new(PrimType::Cylinder).round(radius, half_height),
                };
                let mut node = Node::new(Kind::Prim(prim));
                node.pos = written(part.pos);
                node.yaw = number(part.yaw);
                node.pitch = number(part.pitch);
                node.roll = number(part.roll);
                node.surface = Surface {
                    mat: None,
                    color: Some(part.color.clone()),
                    look: (part.look != Look::Cel).then_some(part.look),
                    emissive: number(part.emissive),
                    solid: (!part.solid).then_some(false),
                };
                (name.clone(), node)
            })
            .collect();
        Self {
            name: asset.name.clone(),
            unit: Unit::M,
            surface: Surface::default(),
            description: asset.description.clone(),
            tags: Vec::new(),
            category: None,
            source: None,
            fit: None,
            materials: BTreeMap::new(),
            parts,
            sockets: BTreeMap::new(),
        }
    }

    /// The first thing that needs format 2: its path and what it is. None: format 1 holds it
    /// (metres, and only format 1's shapes, fields and keys).
    pub fn needs_format2(&self) -> Option<(String, String)> {
        let found = |path: &str, what: &str| Some((path.to_owned(), what.to_owned()));
        if self.unit != Unit::M {
            return found("unit", &format!("unit \"{}\"", self.unit.name()));
        }
        for (key, set) in [
            ("mat", self.surface.mat.is_some()),
            ("color", self.surface.color.is_some()),
            ("look", self.surface.look.is_some()),
            ("emissive", self.surface.emissive.is_some()),
            ("solid", self.surface.solid.is_some()),
            ("tags", !self.tags.is_empty()),
            ("category", self.category.is_some()),
            ("source", self.source.is_some()),
            ("fit", self.fit.is_some()),
            ("materials", !self.materials.is_empty()),
            ("sockets", !self.sockets.is_empty()),
        ] {
            if set {
                return found(key, key);
            }
        }
        for (name, node) in &self.parts {
            let path = format!("parts.{name}");
            let Kind::Prim(prim) = &node.kind else {
                let what = match node.kind {
                    Kind::Group(_) => "parts (a group)",
                    Kind::Ref(_) => "ref",
                    _ => "mesh",
                };
                return found(&path, what);
            };
            if !prim.kind.in_format1() {
                return found(&format!("{path}.type"), &format!("type \"{}\"", prim.kind.name()));
            }
            let fields = [
                ("size", prim.size.is_some()),
                ("radius_end", prim.radius_end.is_some()),
                ("height", prim.height.is_some()),
                ("arc", prim.arc.is_some()),
                ("facing", prim.facing.is_some()),
                ("segments", prim.segments.is_some()),
                ("from", node.from.is_some()),
                ("to", node.to.is_some()),
                ("attach", node.attach.is_some()),
                ("offset", node.offset.is_some()),
                ("axis", node.axis.is_some()),
                ("spin", node.spin.is_some()),
                ("mat", node.surface.mat.is_some()),
                ("hidden", node.hidden.is_some()),
                ("light", node.light.is_some()),
                ("fx", node.fx.is_some()),
                ("sway", node.sway.is_some()),
                ("mirror", !node.mirror.is_empty()),
                ("array", matches!(node.replicate, Some(Replicate::Array(_)))),
                ("grid", matches!(node.replicate, Some(Replicate::Grid(_)))),
                ("radial", matches!(node.replicate, Some(Replicate::Radial(_)))),
                ("chance", node.chance.is_some()),
                ("jitter", node.jitter.is_some()),
                ("scale", node.scale.is_some()),
            ];
            if let Some((key, _)) = fields.iter().find(|(_, set)| *set) {
                return found(&format!("{path}.{key}"), key);
            }
            match &node.surface.color {
                Some(color) if color.starts_with('$') => return found(&format!("{path}.color"), &format!("role {color}")),
                None => return found(&path, "a part without color"),
                _ => {}
            }
        }
        None
    }

    pub fn needed_format(&self) -> u32 {
        if self.needs_format2().is_some() { 2 } else { 1 }
    }

    /// The format-1 asset, when format 1 holds this document.
    pub fn to_v1(&self) -> Option<PropAsset> {
        if self.needs_format2().is_some() {
            return None;
        }
        let mut parts = BTreeMap::new();
        for (name, node) in &self.parts {
            let prim = node.prim()?;
            let shape = match prim.kind {
                PrimType::Box => Shape::Box { half: prim.half? },
                PrimType::RoundedBox => Shape::RoundedBox { half: prim.half?, radius: prim.radius? },
                PrimType::Sphere => Shape::Sphere { radius: prim.radius? },
                PrimType::Capsule => Shape::Capsule { half_height: prim.half_height?, radius: prim.radius? },
                _ => Shape::Cylinder { half_height: prim.half_height?, radius: prim.radius? },
            };
            let part = PropPart {
                shape,
                pos: node.pos.unwrap_or(Vec3::ZERO),
                yaw: node.yaw.unwrap_or(0.0),
                pitch: node.pitch.unwrap_or(0.0),
                roll: node.roll.unwrap_or(0.0),
                color: node.surface.color.clone()?,
                look: node.surface.look.unwrap_or_default(),
                emissive: node.surface.emissive.unwrap_or(0.0),
                solid: node.surface.solid.unwrap_or(true),
            };
            parts.insert(name.clone(), part);
        }
        Some(PropAsset { format: 1, name: self.name.clone(), description: self.description.clone(), parts })
    }

    /// The canonical file text, in the lowest format that holds the document.
    pub fn text(&self) -> String {
        match self.to_v1() {
            Some(asset) => asset.text(),
            None => self.write(false),
        }
    }

    /// `prop1-` revisions for documents format 1 holds, so their files, `if_revision` values
    /// and history heads stay valid; otherwise `prop2-` over the canonical compact text.
    pub fn revision(&self) -> String {
        match self.to_v1() {
            Some(asset) => asset.revision(),
            None => format!("prop2-{:016x}", super::fnv(self.write(true).as_bytes())),
        }
    }

    /// Authored nodes, groups and references included.
    pub fn node_count(&self) -> usize {
        fn count(parts: &BTreeMap<String, Node>) -> usize {
            parts.values().map(|node| 1 + node.children().map_or(0, count)).sum()
        }
        count(&self.parts)
    }
}

impl Prim {
    fn round(self, radius: f32, half_height: f32) -> Self {
        Self { radius: Some(radius), half_height: Some(half_height), ..self }
    }
}
