//! The format-2 check, which builds the typed model as it reads. Field tables per node kind;
//! types and ranges in the document's unit; the rules between fields (placement, sizes,
//! copies) and between nodes (materials, attach targets, surfaces, limits). Every problem is
//! reported with its path, the valid values and, where there is one, a fix.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{EulerRot, Quat, Vec3};
use serde_json::{Map, Value, json};

use super::{amount, color_fix, is_color, look_hint, number, wrong_type};
use crate::issue::{self, Code, Issue, join};
use crate::params::ChoiceParam;
use crate::props::doc::{
    Anchor, Array, Attach, Axis, Category, Dir, Doc, Fit, Fx, Grid, Jitter, Kind, Layer, Light, MapMode, Material, MaterialEntry,
    Node, PRESETS, Pattern, Prim, PrimType, ROLES, Radial, Replicate, Socket, SocketName, Source, Surface, Unit,
};
use crate::props::{MAX_PARTS, PropDef, validate_name, validate_part_name};
use crate::shape::{Look, Sway};

/// Groups inside groups.
pub const MAX_GROUP_DEPTH: usize = 4;
/// Parts after copies, mirrors and references.
pub const MAX_RESOLVED: usize = 1_024;
pub const MAX_COPIES: u32 = 64;
pub const MAX_GRID_SIDE: u32 = 32;
pub const MAX_GRID_COPIES: u32 = 256;
pub const MAX_LIGHTS: usize = 4;
pub const MAX_FX: usize = 4;
pub const MAX_TAGS: usize = 16;
pub const MAX_LAYERS: usize = 3;

pub const LEGEND: &str = "Parts are named nodes: a primitive (type), a group (parts) or another asset \
(ref). Lengths are in the document's unit (cm is the house unit): x right, y up, z forward (+z is \
the object's front). The origin is the placement point, normally on the floor. Place a node with \
pos (its centre), from/to (the two visible ends of a box, rounded_box, cylinder, cone, tube or \
capsule) or attach (onto another part, its top by default). size is the full size and height the \
full length along a round part's axis (y unless axis says otherwise). yaw, pitch and roll are \
degrees applied Y, then X, then Z: +yaw turns +z toward +x, +pitch tips +z down. A from/to part's \
local y runs from to to; its local z is up made perpendicular, so x is horizontal: a board \
leaning sideways turns its width front-to-back (spin:90 turns it back). Groups pass their frame \
and their surface (mat, color, look, emissive, solid) to their children; the nearest level wins. \
mirror adds reflected copies; array, grid and radial add copies centred on the node, and a cycle \
gives copy i entry i mod n. Colours are #rrggbb or a theme role ($floor $floor2 $wall $pillar \
$accent $light). A batch is accepted or rejected as a whole.";

/// Document fields and their field-guide lines.
pub const DOCUMENT_FIELDS: &[(&str, &str)] = &[
    ("format", "2 (required); format 1 files still read"),
    ("name", "lowercase id, the file name (required)"),
    ("unit", "cm (the house unit) or m (required)"),
    ("mat", "default material for every part: a key in materials"),
    ("color", "default colour: #rrggbb or a role ($floor $floor2 $wall $pillar $accent $light)"),
    ("look", "default look: flat | cel | lit | unlit | cutout"),
    ("emissive", "default glow 0-32"),
    ("solid", "default: part of the collider (true)"),
    ("description", "what it is and how it stands; at most 4096 bytes"),
    ("tags", "up to 16 words of a-z, 0-9 and _, each at most 24 characters"),
    (
        "category",
        "furniture | container | architecture | lighting | vegetation | rock | weapon | armour | tool | food | town | dungeon | signage",
    ),
    ("source", "{library,url,license,author,sha256,title}: where it came from (tools write it)"),
    ("fit", "{mean_mm,worst_mm,iou,de,mesh_pct,parts_hash}: how well it fits its source (tools write it)"),
    ("materials", "{key: {color,look,emissive,roughness,metallic,map,scale,turn,layers}}; parts say mat:key"),
    ("parts", "{id: node}: 1-256 nodes, groups and refs included (required)"),
    ("sockets", "{grip|grip2|tip|top|wall_mount|hang|sit: {pos|attach, offset, axis|yaw,pitch,roll}}"),
];

/// Node fields (besides the shape fields) and their field-guide lines.
pub const NODE_FIELDS: &[(&str, &str)] = &[
    ("type", "a primitive: see shapes"),
    ("ref", "another asset's parts, by name (BUILTIN/crate)"),
    ("mesh", "a tool-written mesh, pack/name"),
    ("from", "[x,y,z] one visible end of a box, rounded_box, cylinder, cone, tube or capsule"),
    ("to", "[x,y,z] the other end; local y runs from to to"),
    ("pos", "[x,y,z] the centre; default [0,0,0]"),
    (
        "attach",
        "\"id\" or {to, at, self}: onto another part (at: top, bottom, front, back, left, right, _ combos, from, to or [u,v,w])",
    ),
    ("offset", "[x,y,z] added to the attach point"),
    ("axis", "x | y | z | -x | -y | -z: a round part's own axis (default y; extrude z)"),
    ("yaw", "degrees about y; default 0"),
    ("pitch", "degrees about x; default 0"),
    ("roll", "degrees about z; default 0"),
    ("spin", "degrees about a from/to part's own axis"),
    ("mat", "a key in materials"),
    ("color", "#rrggbb or a role such as $wall"),
    ("look", "flat | cel | lit | unlit | cutout"),
    ("emissive", "glow 0-32"),
    ("solid", "part of the collider; default true"),
    ("hidden", "true: collision only, not drawn"),
    ("light", "{color,radius,intensity,flicker,pulse,shadows}: a point light at the part"),
    ("fx", "{preset,rate,color,size,speed,life,area}: particles at the part"),
    ("sway", "none | leaves | grass"),
    ("mirror", "\"x\" or [\"x\",\"z\"]: adds mirrored copies"),
    ("array", "{count, step|span, cycle}: copies in a row, centred on the node"),
    ("grid", "{count:[a,b], step:[sa,sb], plane, cycle}: copies in rows, centred"),
    ("radial", "{count, radius, start, arc, axis, cycle}: copies round an axis"),
    ("chance", "0-1: how likely the node appears"),
    ("jitter", "{pos,rot,scale,color}: seeded variation per copy"),
    ("pick", "a group shows n of its children"),
    ("scale", "a ref's uniform scale"),
    ("remap", "{its material: a material here}: a ref's materials"),
    ("recolor", "{palette index: material}: a mesh's colours"),
    ("parts", "{id: node}: a group; children use its frame and surface"),
];

/// Primitive types: their shape fields and field-guide lines, in `PrimType` order.
pub const SHAPES: &[(&str, &[&str], &str)] = &[
    ("box", &["size", "half"], "size:[x,y,z], or [across,thick] with from/to; half (half extents) also reads"),
    ("rounded_box", &["size", "half", "radius"], "as box (size or half) plus radius, the round edges"),
    ("sphere", &["radius"], "radius"),
    (
        "capsule",
        &["radius", "radius_end", "height", "half_height"],
        "radius, radius_end; height tip to tip, or from/to the tips; half_height (format 1) also reads",
    ),
    (
        "cylinder",
        &["radius", "radius_end", "height", "half_height", "arc", "facing", "segments"],
        "radius, radius_end, height or from/to, arc, facing, segments (3-64); half_height also reads",
    ),
    ("ellipsoid", &["size"], "size:[x,y,z]"),
    (
        "cone",
        &["radius", "radius_end", "height", "arc", "facing", "segments"],
        "radius, radius_end (default 0), height or from/to, arc, facing, segments (3-64)",
    ),
    (
        "tube",
        &["radius", "thickness", "height", "arc", "facing"],
        "radius (outer), thickness (inward), height or from/to, arc, facing",
    ),
    ("torus", &["radius", "minor_radius", "arc", "facing"], "radius (to the tube's centre), minor_radius, arc, facing"),
    ("wedge", &["size", "peak"], "size:[x,y,z], peak -1 (back) to 1 (front), 0 a gable"),
    ("lathe", &["profile", "segments"], "profile [[r,y],...] 2-32 points turned round y, segments 6-64 (24)"),
    ("extrude", &["points", "depth"], "points [[x,y],...] 3-64 (the front view), depth along z"),
];

/// Inline material fields and their field-guide lines.
pub const MATERIAL_FIELDS: &[(&str, &str)] = &[
    ("base", "a library material to start from"),
    ("color", "#rrggbb or a role (required unless base)"),
    ("look", "flat | cel | lit | unlit; default cel"),
    ("emissive", "glow 0-32"),
    ("roughness", "0-1"),
    ("metallic", "0-1; adds a highlight"),
    ("map", "object | world | axis: how patterns are laid"),
    ("scale", "0.1-10: multiplies every layer size"),
    ("turn", "true: patterns turn 90 degrees"),
    ("layers", "up to 3 pattern layers, bottom first: [{type,...}]"),
];

/// Layer parameters in canonical order.
const LAYER_KEYS: &[&str] = &[
    "type",
    "size",
    "radius",
    "offset",
    "jitter",
    "feature",
    "gap",
    "bevel",
    "vary",
    "cells",
    "cavity",
    "shape",
    "detail",
    "depth",
    "distort",
    "rings",
    "from",
    "to",
    "axis",
    "angle",
    "cover",
    "soft",
    "color",
    "shade",
    "strength",
    "glow",
    "facing",
    "masked_by",
    "seed",
];

/// The fields a node kind takes besides the shape fields, in canonical order.
const NODE_ORDER: &[&str] = &[
    "from", "to", "pos", "attach", "offset", "axis", "yaw", "pitch", "roll", "spin", "mat", "color", "look", "emissive", "solid",
    "hidden", "light", "fx", "sway", "mirror", "array", "grid", "radial", "chance", "jitter", "pick", "scale", "remap",
    "recolor", "parts",
];

const KINDS: &[&str] = &["type", "ref", "mesh", "parts"];
const WORDS: &[&str] = &["top", "bottom", "front", "back", "left", "right"];

pub const LIMITS: &str = "1-256 nodes (groups and refs included), 1024 parts after copies; groups 4 deep; \
array and radial 64 copies; grid 32x32 and 256 copies; lights 4 and fx 4 after copies; sizes 0.001-1000 m \
after the unit; pos within 10000 m; angles within 36000 degrees";

/// The format-2 field guide. Topics: legend, document, node, shapes, materials, ops, limits;
/// none gives all of them.
pub fn schema(topic: Option<&str>) -> Result<Value, String> {
    let sections = vec![
        ("legend", json!(LEGEND)),
        ("document", super::pairs(DOCUMENT_FIELDS)),
        ("node", super::pairs(NODE_FIELDS)),
        ("shapes", super::triples(SHAPES)),
        ("materials", super::pairs(MATERIAL_FIELDS)),
        ("ops", super::triples(crate::props::OPS2)),
        ("limits", json!(LIMITS)),
    ];
    super::guide(2, sections, topic, "asset_edit action=create name=x asset={…}, then patch batches")
}

/// An unknown key. The valid keys are listed only when there is no fix: a node takes about
/// thirty, and a fix already says what to write.
fn unknown_key(path: &str, key: &str, allowed: &[&str], fix: Option<String>) -> Issue {
    let issue = Issue::error(join(path, key), Code::UnknownKey, "unknown key");
    match fix.or_else(|| issue::suggestion(key, allowed.iter().copied())) {
        Some(fix) => issue.fix(fix),
        None => issue.expected(allowed.join(", ")),
    }
}

fn keys(rows: &[(&'static str, &str)]) -> Vec<&'static str> {
    rows.iter().map(|(key, _)| *key).collect()
}

fn shape_fields(kind: PrimType) -> &'static [&'static str] {
    SHAPES[kind as usize].1
}

/// Fixes for keys models tend to guess, at each level of a format-2 document.
fn hint(level: &str, key: &str, value: &Value) -> Option<String> {
    let diameter = || match value.as_f64() {
        Some(x) => format!("use \"radius\":{}", amount(x / 2.0)),
        None => "use radius (half the diameter)".into(),
    };
    Some(match (level, key) {
        (_, "colour") => "spelled \"color\"".into(),
        ("doc", "units") => "spelled \"unit\"".into(),
        ("doc", "version") => "use \"format\":2".into(),
        ("doc", "objects" | "children" | "shapes" | "components" | "nodes") => "parts go in \"parts\":{id:{...}}".into(),
        ("doc", "title" | "label") => "use description".into(),
        ("doc", "params" | "presets") => "parametric families are not available yet".into(),
        ("node", "rotation" | "rot" | "rotate" | "angle" | "angles" | "orientation" | "direction") => {
            "use axis (a round part's direction), from/to (a part between two points) or yaw, pitch and roll in degrees".into()
        }
        ("node", "position" | "location" | "translation" | "center" | "centre" | "origin") => {
            "use pos:[x,y,z] (the centre)".into()
        }
        ("node", "dimensions" | "dims" | "extents" | "width") => "use size:[x,y,z] (the full size)".into(),
        ("node", "half_extents") => match value
            .as_array()
            .map(|items| items.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
        {
            Some(Some(halves)) => {
                format!("use \"size\":[{}] (the full size)", halves.iter().map(|x| amount(x * 2.0)).collect::<Vec<_>>().join(","))
            }
            _ => "use size (the full size, twice the half extents)".into(),
        },
        ("node", "diameter") => diameter(),
        ("node", "length" | "len") => "use height, or from/to for the two ends".into(),
        ("node", "material" | "materials" | "texture") => "use mat (a key in materials)".into(),
        ("node", "children" | "items" | "nodes") => "use parts (a group's children)".into(),
        ("node", "count" | "copies" | "repeat" | "times") => "use array:{count,step} (or grid, radial)".into(),
        ("node", "collide" | "collision" | "collider" | "collidable" | "physics") => {
            "use solid (true: part of the collider)".into()
        }
        ("node", "glow" | "emission" | "emit") => "use emissive (0-32)".into(),
        ("node", "visible" | "invisible") => "use hidden:true for collision without drawing".into(),
        ("node", "name" | "id") => "a part's name is its key in parts".into(),
        ("node", "shape") => "format 2 parts are flat: put type and the shape fields on the part".into(),
        ("node", "symmetric" | "symmetry" | "mirrored") => "use mirror:\"x\"".into(),
        ("node", "particles" | "particle" | "effect" | "emitter") => "use fx:{\"preset\":\"fire\"}".into(),
        ("material", "texture" | "pattern" | "patterns" | "layer") => "use layers:[{type,...}]".into(),
        ("material", "mat" | "material" | "inherit" | "extends") => "use base (a library material)".into(),
        ("material", "metalness" | "metal") => "use metallic (0-1)".into(),
        ("material", "rough" | "gloss" | "glossiness" | "shininess") => "use roughness (0-1)".into(),
        ("material", "format" | "name" | "unit" | "description" | "tags" | "source" | "fit") => {
            "only material files take this; an inline material uses the asset's unit".into()
        }
        ("light", "range" | "distance" | "reach") => "use radius".into(),
        ("light", "brightness" | "power" | "strength") => "use intensity".into(),
        ("fx", "type" | "kind" | "effect" | "name" | "particles") => "use preset".into(),
        ("fx", "amount" | "count" | "density") => "use rate (particles per second)".into(),
        ("array", "steps" | "spacing" | "offset" | "gap" | "interval" | "delta") => {
            "use step:[x,y,z] (between copies) or span (first to last)".into()
        }
        ("array" | "radial", "copies" | "n" | "number" | "num") => "use count".into(),
        ("array", "length" | "total" | "extent") => "use span:[x,y,z] (first copy to last)".into(),
        ("grid", "rows" | "cols" | "columns" | "size") => "use count:[a,b]".into(),
        ("grid", "spacing" | "gap") => "use step:[sa,sb]".into(),
        ("radial", "angle" | "start_angle" | "offset") => "use start (degrees)".into(),
        ("radial", "sweep" | "degrees") => "use arc (degrees)".into(),
        ("attach", "target" | "parent" | "on" | "onto" | "part" | "id") => "use to".into(),
        ("attach", "anchor" | "side" | "face" | "point" | "where") => "use at".into(),
        ("attach", "own") => "use self".into(),
        ("socket", "position") => "use pos:[x,y,z]".into(),
        ("socket", "rotation" | "direction") => "use axis or yaw, pitch and roll".into(),
        _ => return None,
    })
}

fn type_hint(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "cube" | "cuboid" | "rect" | "block" | "slab" | "plane" | "quad" => "use box",
        "ball" | "orb" => "use sphere",
        "pill" => "use capsule",
        "frustum" | "truncated_cone" => "use cone with radius_end",
        "pyramid" => "use cone with segments:4",
        "ring" | "donut" | "doughnut" => "use torus (a ring) or tube (a pipe)",
        "pipe" | "hollow_cylinder" => "use tube",
        "disc" | "disk" | "rod" | "pole" | "column" => "use cylinder",
        "ramp" | "gable" | "prism" | "slope" | "roof" => "use wedge with peak",
        "ellipse" | "oval" => "use ellipsoid",
        "revolve" | "vase" => "use lathe with a profile",
        "polygon" | "extrusion" => "use extrude with points",
        "mesh" | "model" | "obj" | "gltf" | "glb" => "imported meshes come from asset_import, which is not available yet",
        "group" => "a group has parts:{...} and no type",
        _ => return None,
    })
}

/// A fix for a key this node cannot take although others can.
fn misplaced(kind: &str, prim: Option<PrimType>, key: &str) -> Option<String> {
    let all_shape_fields = SHAPES.iter().any(|(_, fields, _)| fields.contains(&key));
    Some(match (kind, prim, key) {
        ("type", Some(p), _) if all_shape_fields => {
            let fields = shape_fields(p).join(", ");
            match (p, key) {
                (PrimType::Box, "radius") => {
                    format!("a box has no radius (use rounded_box for round edges); its fields: {fields}")
                }
                (PrimType::Box | PrimType::RoundedBox | PrimType::Ellipsoid | PrimType::Wedge, "height") => {
                    "the height is size's second value: size:[x,y,z]".into()
                }
                (PrimType::Sphere, "size") => "use radius (an ellipsoid takes size)".into(),
                (PrimType::Capsule | PrimType::Cylinder | PrimType::Cone | PrimType::Tube, "size") => {
                    "use radius and height (or from/to)".into()
                }
                (_, "half") => "half is format 1's box size: use size (the full size)".into(),
                (_, "half_height") => "use height (the full length)".into(),
                _ => format!("a {} takes {fields}", p.name()),
            }
        }
        (_, _, _) if all_shape_fields => format!("{key} belongs on a part with a type"),
        ("type", Some(p), "from" | "to" | "spin") if !p.takes_ends() => {
            "from/to place a box, rounded_box, cylinder, cone, tube or capsule: use pos".into()
        }
        (_, _, "from" | "to") => "from/to place a single part: use pos".into(),
        (_, _, "spin") => "spin turns a from/to part about its axis".into(),
        ("type", Some(p), "axis") => format!("a {} has no axis: turn it with yaw, pitch and roll", p.name()),
        (_, _, "axis") => "turn it with yaw, pitch and roll".into(),
        ("ref", _, "mat" | "color" | "look" | "emissive" | "solid") => {
            "a ref keeps its own surfaces: remap:{its material: a material here}".into()
        }
        ("mesh", _, "mat" | "color" | "look" | "emissive") => "use recolor:{palette index: material}".into(),
        (_, _, "hidden" | "light" | "fx" | "sway") => format!("put {key} on a part with a type"),
        (_, _, "pick") => "pick chooses among a group's children".into(),
        (_, _, "scale") => "scale resizes a ref: change this part's size instead".into(),
        (_, _, "remap") => "remap renames a ref's materials".into(),
        (_, _, "recolor") => "recolor sets a mesh's palette".into(),
        _ => return None,
    })
}

/// Fields a node of this kind takes, in canonical order.
fn allowed(kind: &str, prim: Option<PrimType>) -> Vec<&'static str> {
    let ends = prim.is_some_and(PrimType::takes_ends);
    let shaped = matches!(kind, "type" | "parts");
    let mut keys: Vec<&'static str> = match (kind, prim) {
        ("type", Some(p)) => std::iter::once("type").chain(shape_fields(p).iter().copied()).collect(),
        ("type", None) => vec!["type"],
        ("ref", _) => vec!["ref"],
        ("mesh", _) => vec!["mesh"],
        _ => Vec::new(),
    };
    keys.extend(NODE_ORDER.iter().copied().filter(|key| match *key {
        "from" | "to" | "spin" => ends,
        "axis" => prim.is_some_and(PrimType::takes_axis),
        "mat" | "color" | "look" | "emissive" | "solid" => shaped,
        "hidden" | "light" | "fx" | "sway" => kind == "type",
        "pick" | "parts" => kind == "parts",
        "scale" => matches!(kind, "ref" | "mesh"),
        "remap" => kind == "ref",
        "recolor" => kind == "mesh",
        _ => true,
    }));
    keys
}

/// What a node inherits from its ancestors.
#[derive(Clone, Copy)]
struct Inherit {
    /// A colour or material reaches it from an enclosing level.
    surface: bool,
    solid: bool,
    /// Enclosing groups.
    groups: usize,
    /// Copies of it that its ancestors make.
    copies: usize,
}

#[derive(Default)]
struct Totals {
    parts: usize,
    lights: usize,
    fx: usize,
}

struct Reader<'a> {
    issues: Vec<Issue>,
    unit: Unit,
    /// Material keys as written, so a broken material is reported once.
    materials: BTreeSet<String>,
    assets: &'a dyn Fn(&str) -> Option<Arc<PropDef>>,
}

/// Read a format-2 document. `assets` looks up the targets of `ref` nodes.
pub(crate) fn doc(value: &Value, assets: &dyn Fn(&str) -> Option<Arc<PropDef>>) -> Result<Doc, Vec<Issue>> {
    let mut reader = Reader { issues: Vec::new(), unit: Unit::Cm, materials: BTreeSet::new(), assets };
    let doc = reader.document(value);
    match doc {
        Some(doc) if reader.issues.is_empty() => Ok(doc),
        _ if reader.issues.is_empty() => Err(vec![Issue::error("", Code::Format, "not a readable format-2 document")]),
        _ => Err(reader.issues),
    }
}

impl Reader<'_> {
    fn push(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    /// Reports a missing field unless a fix for an unknown key beside it already names it
    /// ("diameter" → use "radius":5).
    fn missing(&mut self, path: &str, key: &str, expected: impl Into<String>) {
        let named = |fix: &str| {
            fix.contains(&format!("\"{key}\"")) || fix.contains(&format!("{key}:")) || fix.starts_with(&format!("use {key}"))
        };
        let hinted = self.issues.iter().any(|issue| {
            issue.code == Code::UnknownKey
                && issue.path.rsplit_once('.').is_some_and(|(parent, _)| parent == path)
                && issue.fix.as_deref().is_some_and(named)
        });
        let expected: String = expected.into();
        if !hinted {
            let issue = Issue::error(join(path, key), Code::Missing, "missing");
            self.push(if expected.is_empty() { issue } else { issue.expected(expected) });
        }
    }

    /// Lengths in the document's unit: 0.001-1000 m.
    fn length_range(&self) -> (f32, f32) {
        match self.unit {
            Unit::Cm => (0.1, 100_000.0),
            Unit::M => (0.001, 1_000.0),
        }
    }

    /// Positions in the document's unit: within 10000 m.
    fn position_range(&self) -> f32 {
        match self.unit {
            Unit::Cm => 1_000_000.0,
            Unit::M => 10_000.0,
        }
    }

    fn unknown_keys(&mut self, path: &str, object: &Map<String, Value>, allowed: &[&str], level: &str) {
        for (key, value) in object {
            if !allowed.contains(&key.as_str()) {
                self.push(unknown_key(path, key, allowed, hint(level, key, value)));
            }
        }
    }

    fn object<'v>(&mut self, path: &str, value: &'v Value, allowed: &[&str], level: &str) -> Option<&'v Map<String, Value>> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, &format!("an object {{{}}}", allowed.join(",")), value));
            return None;
        };
        self.unknown_keys(path, object, allowed, level);
        Some(object)
    }

    fn ranged(&mut self, path: &str, value: &Value, min: f32, max: f32) -> Option<f32> {
        number(path, value, min, max, &mut self.issues)
    }

    fn length(&mut self, path: &str, value: &Value) -> Option<f32> {
        let (min, max) = self.length_range();
        self.measure(path, value, min, max)
    }

    /// A length that may be zero.
    fn extent(&mut self, path: &str, value: &Value) -> Option<f32> {
        let (_, max) = self.length_range();
        self.measure(path, value, 0.0, max)
    }

    /// A length in the document's unit: a range error names the unit, and a length written
    /// in metres in a cm document gets the conversion.
    fn measure(&mut self, path: &str, value: &Value, min: f32, max: f32) -> Option<f32> {
        let before = self.issues.len();
        let length = self.ranged(path, value, min, max);
        let unit = self.unit;
        if let Some(issue) = self.issues.get_mut(before).filter(|issue| issue.code == Code::OutOfRange) {
            let range = format!("{} to {} {}", amount(min as f64), amount(max as f64), unit.name());
            issue.message = format!("{value} is outside {range}");
            issue.expected = Some(range);
            let cm = value.as_f64().unwrap_or_default() * 100.0;
            if unit == Unit::Cm && cm > 0.0 && (cm as f32) < max && value.as_f64().is_some_and(|x| (x as f32) < min) {
                issue.fix = Some(format!("lengths are in cm: {value} m is {}", amount(cm)));
            }
        }
        length
    }

    fn coordinate(&mut self, path: &str, value: &Value) -> Option<f32> {
        let max = self.position_range();
        self.ranged(path, value, -max, max)
    }

    fn angle(&mut self, path: &str, value: &Value) -> Option<f32> {
        self.ranged(path, value, -36_000.0, 36_000.0)
    }

    /// `n` values read by `item`, or one of `counts` values when several are allowed.
    fn list<T>(
        &mut self,
        path: &str,
        value: &Value,
        counts: std::ops::RangeInclusive<usize>,
        what: &str,
        mut item: impl FnMut(&mut Self, &str, &Value) -> Option<T>,
    ) -> Option<Vec<T>> {
        let expected = if counts.start() == counts.end() {
            format!("{} {what}", counts.start())
        } else {
            format!("{}-{} {what}", counts.start(), counts.end())
        };
        let Some(items) = value.as_array() else {
            self.push(wrong_type(path, &expected, value));
            return None;
        };
        if !counts.contains(&items.len()) {
            self.push(Issue::error(path, Code::InvalidType, format!("expected {expected}, got {}", items.len())));
            return None;
        }
        let mut out = Vec::with_capacity(items.len());
        let mut ok = true;
        for (index, value) in items.iter().enumerate() {
            match item(self, &format!("{path}[{index}]"), value) {
                Some(x) => out.push(x),
                None => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn point(&mut self, path: &str, value: &Value) -> Option<Vec3> {
        self.list(path, value, 3..=3, "numbers [x,y,z]", Self::coordinate).map(|v| Vec3::from_slice(&v))
    }

    fn integer(&mut self, path: &str, value: &Value, min: u32, max: u32) -> Option<u32> {
        match value.as_i64().or_else(|| value.as_u64().map(|n| n.min(i64::MAX as u64) as i64)) {
            Some(n) if (min as i64..=max as i64).contains(&n) => Some(n as u32),
            Some(n) => {
                self.push(
                    Issue::error(path, Code::OutOfRange, format!("{n} is outside {min} to {max}"))
                        .expected(format!("{min} to {max}")),
                );
                None
            }
            None if value.is_number() => {
                self.push(Issue::error(path, Code::InvalidType, format!("expected a whole number, got {value}")));
                None
            }
            None => {
                self.push(wrong_type(path, "a whole number", value));
                None
            }
        }
    }

    fn boolean(&mut self, path: &str, value: &Value) -> Option<bool> {
        let flag = value.as_bool();
        if flag.is_none() {
            self.push(wrong_type(path, "true or false", value));
        }
        flag
    }

    fn text(&mut self, path: &str, value: &Value) -> Option<String> {
        let text = value.as_str().map(String::from);
        if text.is_none() {
            self.push(wrong_type(path, "text", value));
        }
        text
    }

    /// One of `names`, with a hint or a did-you-mean.
    fn choice<T>(
        &mut self,
        path: &str,
        value: &Value,
        what: &str,
        names: &[&str],
        parse: impl Fn(&str) -> Option<T>,
        hint: impl Fn(&str) -> Option<String>,
    ) -> Option<T> {
        let Some(text) = value.as_str() else {
            self.push(wrong_type(path, &format!("one of {}", names.join(", ")), value));
            return None;
        };
        let parsed = parse(text);
        if parsed.is_none() {
            self.push(
                Issue::error(path, Code::UnknownValue, format!("unknown {what} \"{text}\""))
                    .expected(names.join(", "))
                    .maybe_fix(hint(text).or_else(|| issue::suggestion(text, names.iter().copied()))),
            );
        }
        parsed
    }

    /// One of a short list of words.
    fn word(&mut self, path: &str, value: &Value, what: &str, names: &[&'static str]) -> Option<&'static str> {
        self.choice(path, value, what, names, |text| names.iter().copied().find(|name| *name == text), |_| None)
    }

    fn dir(&mut self, path: &str, value: &Value) -> Option<Dir> {
        let plus =
            |text: &str| text.strip_prefix('+').filter(|axis| Dir::parse(axis).is_some()).map(|axis| format!("write \"{axis}\""));
        self.choice(path, value, "direction", Dir::NAMES, Dir::parse, plus)
    }

    /// `#rrggbb` or a theme role.
    fn color(&mut self, path: &str, value: &Value) -> Option<String> {
        let Some(text) = value.as_str() else {
            self.push(wrong_type(path, "\"#rrggbb\" or a role", value));
            return None;
        };
        if is_color(text) || ROLES.contains(&text) {
            return Some(text.into());
        }
        if text.starts_with('$') {
            self.push(
                Issue::error(path, Code::UnknownValue, format!("unknown role \"{text}\""))
                    .expected(ROLES.join(", "))
                    .maybe_fix(issue::suggestion(text, ROLES.iter().copied())),
            );
        } else {
            self.push(
                Issue::error(path, Code::UnknownValue, format!("\"{text}\" is not a #rrggbb colour or a role"))
                    .expected("#rrggbb or $floor, $floor2, $wall, $pillar, $accent, $light")
                    .maybe_fix(color_fix(text)),
            );
        }
        None
    }

    fn look(&mut self, path: &str, value: &Value, cutout: bool) -> Option<Look> {
        let names: &[&str] = if cutout { <Look as ChoiceParam>::NAMES } else { &["flat", "cel", "lit", "unlit"] };
        let parse = |text: &str| names.iter().position(|name| *name == text).map(<Look as ChoiceParam>::from_index);
        self.choice(path, value, "look", names, parse, |text| look_hint(text).map(String::from))
    }

    /// A material key here. Library materials (`MAT/…`) are not available yet.
    fn material_ref(&mut self, path: &str, value: &Value) -> Option<String> {
        let name = self.text(path, value)?;
        if self.materials.contains(&name) {
            return Some(name);
        }
        let issue = if name.starts_with("MAT/") {
            Issue::error(path, Code::UnknownMaterial, format!("no library material \"{name}\""))
                .fix("library materials are not available yet: write one under materials")
        } else if self.materials.is_empty() {
            Issue::error(path, Code::UnknownMaterial, format!("no material \"{name}\""))
                .fix(format!("add \"materials\":{{\"{name}\":{{\"color\":\"#rrggbb\"}}}} or use color"))
        } else {
            Issue::error(path, Code::UnknownMaterial, format!("no material \"{name}\""))
                .expected(self.materials.iter().cloned().collect::<Vec<_>>().join(", "))
                .maybe_fix(issue::suggestion(&name, self.materials.iter().map(String::as_str)))
        };
        self.push(issue);
        None
    }

    fn surface(&mut self, path: &str, object: &Map<String, Value>) -> Surface {
        let at = |key: &str| join(path, key);
        Surface {
            mat: object.get("mat").and_then(|v| self.material_ref(&at("mat"), v)),
            color: object.get("color").and_then(|v| self.color(&at("color"), v)),
            look: object.get("look").and_then(|v| self.look(&at("look"), v, true)),
            emissive: object.get("emissive").and_then(|v| self.ranged(&at("emissive"), v, 0.0, 32.0)),
            solid: object.get("solid").and_then(|v| self.boolean(&at("solid"), v)),
        }
    }

    fn document(&mut self, value: &Value) -> Option<Doc> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type("", "an object {format,name,unit,parts}", value));
            return None;
        };
        self.unknown_keys("", object, &keys(DOCUMENT_FIELDS), "doc");
        let unit = match object.get("unit") {
            None => {
                self.push(
                    Issue::error("unit", Code::Missing, "missing")
                        .expected("cm or m")
                        .fix("add \"unit\":\"cm\" (the house unit) or \"unit\":\"m\""),
                );
                None
            }
            Some(value) => {
                let hint = |text: &str| match text {
                    "mm" | "millimetres" | "millimeters" => {
                        Some("use cm (lengths in tenths of a centimetre are fine)".to_owned())
                    }
                    "metres" | "meters" | "metre" | "meter" => Some("write \"m\"".to_owned()),
                    "centimetres" | "centimeters" => Some("write \"cm\"".to_owned()),
                    _ => None,
                };
                self.choice("unit", value, "unit", Unit::NAMES, Unit::parse, hint)
            }
        };
        self.unit = unit.unwrap_or(Unit::Cm);
        let name = match object.get("name") {
            None => {
                self.push(Issue::error("name", Code::Missing, "missing"));
                None
            }
            Some(Value::String(name)) => match validate_name(name) {
                Ok(()) => Some(name.clone()),
                Err(error) => {
                    self.push(Issue::error("name", Code::UnknownValue, format!("\"{name}\" is not a usable name: {error}")));
                    None
                }
            },
            Some(value) => {
                self.push(wrong_type("name", "a lowercase name", value));
                None
            }
        };
        if let Some(Value::Object(materials)) = object.get("materials") {
            self.materials = materials.keys().cloned().collect();
        }
        let surface = self.surface("", object);
        let description = match object.get("description") {
            None => String::new(),
            Some(Value::String(text)) if text.len() > 4096 => {
                self.push(
                    Issue::error("description", Code::Limit, format!("{} bytes is too long", text.len()))
                        .expected("at most 4096"),
                );
                String::new()
            }
            Some(value) => self.text("description", value).unwrap_or_default(),
        };
        let tags = object.get("tags").map(|value| self.tags(value)).unwrap_or_default();
        let category = object
            .get("category")
            .and_then(|value| self.choice("category", value, "category", Category::NAMES, Category::parse, |_| None));
        let source = object.get("source").and_then(|value| self.source(value));
        let fit = object.get("fit").and_then(|value| self.fit(value));
        let materials = match object.get("materials") {
            None => BTreeMap::new(),
            Some(Value::Object(map)) => {
                let mut materials = BTreeMap::new();
                for (key, value) in map {
                    let path = format!("materials.{key}");
                    if let Err(error) = validate_part_name(key) {
                        self.push(Issue::error(&path, Code::UnknownValue, format!("\"{key}\" is not a material key: {error}")));
                    }
                    if let Some(material) = self.material(&path, value) {
                        materials.insert(key.clone(), material);
                    }
                }
                materials
            }
            Some(value) => {
                self.push(wrong_type("materials", "an object {key: material}", value));
                BTreeMap::new()
            }
        };
        let mut totals = Totals::default();
        let parts = match object.get("parts") {
            None => {
                self.push(Issue::error("parts", Code::Missing, "missing").fix("add \"parts\":{id:{type,...}}"));
                None
            }
            Some(Value::Object(parts)) => {
                let count = count_nodes(parts);
                if parts.is_empty() || count > MAX_PARTS {
                    self.push(
                        Issue::error("parts", Code::Limit, format!("{count} nodes"))
                            .expected(format!("1 to {MAX_PARTS}, groups and refs included")),
                    );
                }
                let inherit = Inherit {
                    surface: surface.mat.is_some() || surface.color.is_some(),
                    solid: surface.solid.unwrap_or(true),
                    groups: 0,
                    copies: 1,
                };
                Some(self.nodes("parts", parts, inherit, &mut totals))
            }
            Some(value) => {
                self.push(wrong_type("parts", "an object {id: node}", value));
                None
            }
        };
        for (what, count, max) in
            [("parts", totals.parts, MAX_RESOLVED), ("lights", totals.lights, MAX_LIGHTS), ("fx", totals.fx, MAX_FX)]
        {
            if count > max {
                self.push(
                    Issue::error("parts", Code::Limit, format!("{count} {what} after copies")).expected(format!("at most {max}")),
                );
            }
        }
        let sockets = match object.get("sockets") {
            None => BTreeMap::new(),
            Some(value) => self.sockets(value),
        };
        let parts = parts?;
        if let Some(Value::Object(raw)) = object.get("parts") {
            self.attachments(&parts, raw, &sockets);
        }
        Some(Doc { name: name?, unit: unit?, surface, description, tags, category, source, fit, materials, parts, sockets })
    }

    fn tags(&mut self, value: &Value) -> Vec<String> {
        let valid = |tag: &str| {
            (1..=24).contains(&tag.len()) && tag.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        };
        let tags = self.list("tags", value, 0..=MAX_TAGS, "tags", |reader, path, value| {
            let tag = reader.text(path, value)?;
            if !valid(&tag) {
                reader.push(
                    Issue::error(path, Code::UnknownValue, format!("\"{tag}\" is not a tag"))
                        .expected("1-24 characters of a-z, 0-9 and _"),
                );
                return None;
            }
            Some(tag)
        });
        let tags = tags.unwrap_or_default();
        let mut seen = BTreeSet::new();
        for (index, tag) in tags.iter().enumerate() {
            if !seen.insert(tag) {
                self.push(
                    Issue::error(format!("tags[{index}]"), Code::Duplicate, format!("\"{tag}\" appears twice")).fix("keep one"),
                );
            }
        }
        tags
    }

    fn source(&mut self, value: &Value) -> Option<Source> {
        let object = self.object("source", value, &["library", "url", "license", "author", "sha256", "title"], "source")?;
        let mut field = |key: &str| object.get(key).and_then(|v| self.text(&format!("source.{key}"), v));
        let source = Source {
            library: field("library"),
            url: field("url"),
            license: field("license"),
            author: field("author"),
            sha256: field("sha256"),
            title: field("title"),
        };
        if let Some(sha) = source
            .sha256
            .as_ref()
            .filter(|sha| sha.len() != 64 || !sha.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        {
            self.push(
                Issue::error("source.sha256", Code::UnknownValue, format!("\"{sha}\" is not a sha256"))
                    .expected("64 lowercase hex digits"),
            );
        }
        Some(source)
    }

    fn fit(&mut self, value: &Value) -> Option<Fit> {
        let object = self.object("fit", value, &["mean_mm", "worst_mm", "iou", "de", "mesh_pct", "parts_hash"], "fit")?;
        let mut field =
            |key: &str, max: f32| object.get(key).and_then(|v| number(&format!("fit.{key}"), v, 0.0, max, &mut self.issues));
        let fit = Fit {
            mean_mm: field("mean_mm", 1e6),
            worst_mm: field("worst_mm", 1e6),
            iou: field("iou", 1.0),
            de: field("de", 100.0),
            mesh_pct: field("mesh_pct", 100.0),
            parts_hash: None,
        };
        Some(Fit { parts_hash: object.get("parts_hash").and_then(|v| self.text("fit.parts_hash", v)), ..fit })
    }

    fn material(&mut self, path: &str, value: &Value) -> Option<MaterialEntry> {
        if let Value::String(name) = value {
            let fix = if name.starts_with("MAT/") {
                "library materials are not available yet: write the material inline, {\"color\":\"#rrggbb\"}"
            } else {
                "write the material inline, {\"color\":\"#rrggbb\"}"
            };
            self.push(Issue::error(path, Code::UnknownMaterial, format!("no library material \"{name}\"")).fix(fix));
            return None;
        }
        let object = self.object(path, value, &keys(MATERIAL_FIELDS), "material")?;
        let at = |key: &str| join(path, key);
        let base = object.get("base").and_then(|value| {
            let name = self.text(&at("base"), value)?;
            self.push(
                Issue::error(at("base"), Code::UnknownMaterial, format!("no library material \"{name}\""))
                    .fix("library materials are not available yet: give color and the other fields here"),
            );
            None::<String>
        });
        let material = Material {
            base,
            color: object.get("color").and_then(|v| self.color(&at("color"), v)),
            look: object.get("look").and_then(|v| self.look(&at("look"), v, false)),
            emissive: object.get("emissive").and_then(|v| self.ranged(&at("emissive"), v, 0.0, 32.0)),
            roughness: object.get("roughness").and_then(|v| self.ranged(&at("roughness"), v, 0.0, 1.0)),
            metallic: object.get("metallic").and_then(|v| self.ranged(&at("metallic"), v, 0.0, 1.0)),
            map: object.get("map").and_then(|v| self.choice(&at("map"), v, "map", MapMode::NAMES, MapMode::parse, |_| None)),
            scale: object.get("scale").and_then(|v| self.ranged(&at("scale"), v, 0.1, 10.0)),
            turn: object.get("turn").and_then(|v| self.boolean(&at("turn"), v)),
            layers: object.get("layers").and_then(|v| {
                self.list(&at("layers"), v, 0..=MAX_LAYERS, "layers", |reader, path, value| reader.layer(path, value))
            }),
        };
        if !object.contains_key("color") && !object.contains_key("base") {
            self.push(Issue::error(at("color"), Code::Missing, "missing").fix("a material needs color"));
        }
        Some(MaterialEntry::Inline(material))
    }

    /// A pattern layer: the type's own parameters, the shared ones, and for cellular and 2D
    /// patterns theirs.
    fn layer(&mut self, path: &str, value: &Value) -> Option<Layer> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, "an object {type,...}", value));
            return None;
        };
        let at = |key: &str| join(path, key);
        let kind = match object.get("type") {
            None => {
                self.push(Issue::error(at("type"), Code::Missing, "missing").expected(Pattern::NAMES.join(", ")));
                return None;
            }
            Some(value) => self.choice(&at("type"), value, "pattern", Pattern::NAMES, Pattern::parse, |_| None)?,
        };
        use Pattern::*;
        let own: &[&str] = match kind {
            Noise => &["size", "detail", "distort", "cover", "soft"],
            Voronoi => &["size", "jitter", "feature", "cover", "gap"],
            Wave => &["size", "axis", "rings", "distort", "cover", "soft"],
            Brick => &["size", "offset", "gap"],
            Checker => &["size"],
            Gradient => &["axis", "from", "to"],
            Magic => &["size", "depth", "distort", "cover", "soft"],
            Planks | Tiles => &["size", "gap"],
            Cobbles => &["size", "jitter", "gap"],
            Stripes => &["size", "cover", "axis", "soft"],
            Dots => &["size", "radius", "offset", "jitter"],
            Shingles => &["size", "shape", "gap"],
            Wood => &["size", "axis", "distort", "cover", "soft"],
        };
        let cellular = matches!(kind, Voronoi | Brick | Planks | Tiles | Cobbles | Dots | Shingles);
        let flat = cellular || kind == Checker;
        let allowed: Vec<&'static str> = LAYER_KEYS
            .iter()
            .copied()
            .filter(|key| {
                *key == "type"
                    || own.contains(key)
                    || ["color", "shade", "strength", "glow", "facing", "masked_by", "seed"].contains(key)
                    || (cellular && ["vary", "cells", "cavity", "bevel"].contains(key))
                    || (flat && ["axis", "angle"].contains(key))
            })
            .collect();
        self.unknown_keys(path, object, &allowed, "layer");
        let mut params = Vec::new();
        for key in allowed.iter().copied().filter(|key| *key != "type") {
            let Some(value) = object.get(key) else { continue };
            let path = at(key);
            let ok = match key {
                "size" => match value {
                    Value::Array(_) if matches!(kind, Brick | Planks | Tiles | Shingles) => {
                        self.list(&path, value, 2..=2, "sizes [w,h]", Self::length).is_some()
                    }
                    _ => self.length(&path, value).is_some(),
                },
                "radius" | "gap" | "bevel" => self.extent(&path, value).is_some(),
                "from" | "to" => self.coordinate(&path, value).is_some(),
                "detail" | "depth" => self.integer(&path, value, 1, 4).is_some(),
                "seed" => self.integer(&path, value, 0, u32::MAX).is_some(),
                "distort" => self.ranged(&path, value, 0.0, 10.0).is_some(),
                "angle" => self.ranged(&path, value, -360.0, 360.0).is_some(),
                "shade" => self.ranged(&path, value, 0.0, 2.0).is_some(),
                "glow" => self.ranged(&path, value, 0.0, 8.0).is_some(),
                "rings" => self.boolean(&path, value).is_some(),
                "color" => self.color(&path, value).is_some(),
                "cells" => self.list(&path, value, 2..=4, "colours", Self::color).is_some(),
                "feature" => self.word(&path, value, "feature", &["spots", "edges", "cells"]).is_some(),
                "shape" => self.word(&path, value, "shape", &["round", "square", "point"]).is_some(),
                "facing" => self.word(&path, value, "facing", &["up", "down", "side"]).is_some(),
                "masked_by" => self.word(&path, value, "mask", &["prev", "not_prev"]).is_some(),
                "axis" => match value {
                    Value::Array(_) => {
                        self.list(&path, value, 3..=3, "numbers [x,y,z]", |r, p, v| r.ranged(p, v, -1.0, 1.0)).is_some()
                    }
                    _ => self.choice(&path, value, "axis", Axis::NAMES, Axis::parse, |_| None).is_some(),
                },
                _ => self.ranged(&path, value, 0.0, 1.0).is_some(),
            };
            if ok {
                params.push((key, value.clone()));
            }
        }
        if object.contains_key("color") && object.contains_key("shade") {
            self.push(
                Issue::error(at("shade"), Code::Conflict, "a layer paints color or darkens by shade, not both").fix("keep one"),
            );
        }
        Some(Layer { kind, params })
    }

    fn nodes(&mut self, path: &str, parts: &Map<String, Value>, inherit: Inherit, totals: &mut Totals) -> BTreeMap<String, Node> {
        let mut nodes = BTreeMap::new();
        for (name, value) in parts {
            let path = join(path, name);
            if let Err(error) = validate_part_name(name) {
                self.push(Issue::error(&path, Code::UnknownValue, format!("\"{name}\" is not a part name: {error}")));
            }
            if let Some(node) = self.node(&path, value, inherit, totals) {
                nodes.insert(name.clone(), node);
            }
        }
        nodes
    }

    fn node(&mut self, path: &str, value: &Value, inherit: Inherit, totals: &mut Totals) -> Option<Node> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, "an object: a part {type,...} or a group {parts:{...}}", value));
            return None;
        };
        let at = |key: &str| join(path, key);
        let kinds: Vec<&str> = KINDS.iter().copied().filter(|key| object.contains_key(*key)).collect();
        let kind = match kinds.as_slice() {
            [kind] => *kind,
            [] => {
                let has = |keys: &[&str]| keys.iter().any(|key| object.contains_key(*key));
                let fix = if has(&["shape"]) {
                    "format 2 parts are flat: put type and the shape fields on the part, e.g. \"type\":\"box\",\"size\":[...]"
                } else if has(&["children", "items", "nodes"]) {
                    "a group's children go in parts:{id:{...}}"
                } else if has(&["kind", "primitive", "shape_type"]) {
                    "the primitive's name goes in type, e.g. \"type\":\"box\""
                } else {
                    "give a type, e.g. \"type\":\"box\",\"size\":[...], or parts:{...} for a group"
                };
                self.push(Issue::error(at("type"), Code::Missing, "missing").expected("type, ref or parts").fix(fix));
                return None;
            }
            several => {
                self.push(
                    Issue::error(
                        path,
                        Code::Conflict,
                        format!("a node has one of type, ref, mesh or parts; this one has {}", several.join(" and ")),
                    )
                    .fix("split it: a group's parts can hold the primitive"),
                );
                return None;
            }
        };
        let prim_type = match (kind, object.get("type")) {
            ("type", Some(value)) => {
                let names = PrimType::NAMES;
                self.choice(&at("type"), value, "shape type", names, PrimType::parse, |text| type_hint(text).map(String::from))
            }
            _ => None,
        };
        let allowed = allowed(kind, prim_type);
        for (key, value) in object {
            let unknown_type = kind == "type" && prim_type.is_none();
            let shape_field = SHAPES.iter().any(|(_, fields, _)| fields.contains(&key.as_str()));
            if !allowed.contains(&key.as_str())
                && !(unknown_type && (shape_field || matches!(key.as_str(), "from" | "to" | "spin" | "axis")))
            {
                let fix = misplaced(kind, prim_type, key).or_else(|| hint("node", key, value));
                self.push(unknown_key(path, key, &allowed, fix));
            }
        }
        if kind == "mesh" {
            self.push(
                Issue::error(at("mesh"), Code::UnknownAsset, "mesh parts need asset_import, which is not available yet")
                    .fix("build the shape from primitives"),
            );
            return None;
        }
        if kind == "type" && prim_type.is_none() {
            return None;
        }
        let kind = match kind {
            "type" => Kind::Prim(self.prim(path, object, prim_type?)),
            "ref" => Kind::Ref(self.reference(&at("ref"), &object["ref"])?),
            _ => Kind::Group(BTreeMap::new()),
        };
        let mut node = Node::new(kind);
        // Keys this node cannot take were reported above; only the others are read.
        let field = |key: &str| object.get(key).filter(|_| allowed.contains(&key)).map(|value| (join(path, key), value));
        if let Some((p, v)) = field("from") {
            node.from = self.point(&p, v);
        }
        if let Some((p, v)) = field("to") {
            node.to = self.point(&p, v);
        }
        if let Some((p, v)) = field("pos") {
            node.pos = self.point(&p, v);
        }
        if let Some((p, v)) = field("attach") {
            node.attach = self.attach(&p, v);
        }
        if let Some((p, v)) = field("offset") {
            node.offset = self.point(&p, v);
        }
        if let Some((p, v)) = field("axis") {
            node.axis = self.dir(&p, v);
        }
        for (key, slot) in
            [("yaw", &mut node.yaw), ("pitch", &mut node.pitch), ("roll", &mut node.roll), ("spin", &mut node.spin)]
        {
            if let Some((p, v)) = field(key) {
                *slot = self.angle(&p, v);
            }
        }
        if allowed.contains(&"mat") {
            node.surface = self.surface(path, object);
        }
        if let Some((p, v)) = field("hidden") {
            node.hidden = self.boolean(&p, v);
        }
        if let Some((p, v)) = field("light") {
            node.light = self.light(&p, v);
        }
        if let Some((p, v)) = field("fx") {
            node.fx = self.fx(&p, v);
        }
        if let Some((p, v)) = field("sway") {
            let names = <Sway as ChoiceParam>::NAMES;
            let parse = |text: &str| names.iter().position(|name| *name == text).map(<Sway as ChoiceParam>::from_index);
            node.sway = self.choice(&p, v, "sway", names, parse, |_| None);
        }
        if let Some((p, v)) = field("mirror") {
            node.mirror = self.mirror(&p, v).unwrap_or_default();
        }
        let replications: Vec<&str> = ["array", "grid", "radial"].into_iter().filter(|key| object.contains_key(*key)).collect();
        if replications.len() > 1 {
            self.push(
                Issue::error(
                    at(replications[1]),
                    Code::Conflict,
                    format!("one of array, grid or radial per node; this one has {}", replications.join(" and ")),
                )
                .fix("nest a group to combine them"),
            );
        }
        node.replicate = match replications.first().copied() {
            Some("array") => self.array(&at("array"), &object["array"]).map(Replicate::Array),
            Some("grid") => self.grid(&at("grid"), &object["grid"]).map(Replicate::Grid),
            Some("radial") => self.radial(&at("radial"), &object["radial"]).map(Replicate::Radial),
            _ => None,
        };
        if let Some((p, v)) = field("chance") {
            node.chance = self.ranged(&p, v, 0.0, 1.0);
        }
        if let Some((p, v)) = field("jitter") {
            node.jitter = self.jitter(&p, v);
        }
        if let Some((p, v)) = field("scale") {
            node.scale = self.ranged(&p, v, 0.01, 100.0);
        }
        if let Some((p, v)) = field("remap") {
            node.remap = self.remap(&p, v, &node.kind);
        }
        self.placement(path, object, &node, &allowed);
        if let Kind::Prim(prim) = &node.kind {
            self.prim_rules(path, object, prim, &node);
        }

        // What reaches the children, and this node's share of the asset's totals. A colour or
        // material written wrongly has its own error, so it still counts here.
        let own_surface = ["mat", "color", "colour", "material", "texture"].iter().any(|key| object.contains_key(*key));
        let cycle = ["array", "grid", "radial"].iter().any(|key| object.get(*key).and_then(|r| r.get("cycle")).is_some());
        let solid = node.surface.solid.unwrap_or(inherit.solid);
        let copies = (inherit.copies * node.copies()) << node.mirror.len();
        match &node.kind {
            Kind::Prim(_) => {
                if !(own_surface || cycle || inherit.surface) {
                    self.push(
                        Issue::error(path, Code::Missing, "no colour: nothing gives this part color or mat")
                            .fix("add color or mat here, on an enclosing group or at the top of the document"),
                    );
                }
                if node.hidden == Some(true) && !solid {
                    self.push(
                        Issue::error(at("hidden"), Code::Conflict, "a hidden part is collision only, so it must be solid")
                            .fix("remove solid:false (here or on an enclosing group), or drop hidden"),
                    );
                }
                totals.parts += copies;
                totals.lights += copies * usize::from(node.light.is_some());
                totals.fx += copies * usize::from(node.fx.is_some());
            }
            Kind::Ref(name) => {
                // Its lights and emitters come along and count toward this document's.
                let (parts, lights, fx) = (self.assets)(name)
                    .map_or((1, 0, 0), |def| (def.compiled.parts.len(), def.compiled.lights.len(), def.compiled.fx.len()));
                totals.parts += copies * parts;
                totals.lights += copies * lights;
                totals.fx += copies * fx;
            }
            _ => {}
        }
        if matches!(node.kind, Kind::Group(_)) {
            let groups = inherit.groups + 1;
            if groups > MAX_GROUP_DEPTH {
                self.push(
                    Issue::error(path, Code::Limit, format!("groups nest {groups} deep"))
                        .expected(format!("at most {MAX_GROUP_DEPTH}")),
                );
            }
            let inherit = Inherit { surface: inherit.surface || own_surface || cycle, solid, groups, copies };
            let children = match &object["parts"] {
                Value::Object(parts) if !parts.is_empty() => self.nodes(&at("parts"), parts, inherit, totals),
                Value::Object(_) => {
                    self.push(Issue::error(at("parts"), Code::Limit, "an empty group").expected("at least one part"));
                    BTreeMap::new()
                }
                value => {
                    self.push(wrong_type(&at("parts"), "an object {id: node}", value));
                    BTreeMap::new()
                }
            };
            if let Some((p, v)) = field("pick") {
                node.pick = self.integer(&p, v, 1, (object["parts"].as_object().map_or(1, Map::len)).max(1) as u32);
            }
            node.kind = Kind::Group(children);
        }
        Some(node)
    }

    fn reference(&mut self, path: &str, value: &Value) -> Option<String> {
        let name = self.text(path, value)?;
        let canonical = match crate::props::canonical(&name) {
            Ok(canonical) => canonical,
            Err(error) => {
                self.push(Issue::error(path, Code::UnknownAsset, format!("\"{name}\" is not an asset name: {error}")));
                return None;
            }
        };
        if (self.assets)(&canonical).is_none() {
            let library = crate::props::library();
            let names: Vec<&str> = library.defs.keys().map(String::as_str).collect();
            self.push(
                Issue::error(path, Code::UnknownAsset, format!("no asset \"{name}\""))
                    .maybe_fix(issue::suggestion(&canonical, names.iter().copied())),
            );
        }
        Some(name)
    }

    /// `{its material: a material here}`. Format-1 assets have no materials to rename.
    fn remap(&mut self, path: &str, value: &Value, kind: &Kind) -> Option<BTreeMap<String, String>> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, "an object {its material: a material here}", value));
            return None;
        };
        // The keys are the referenced asset's materials (an unknown asset is reported once, on
        // the ref).
        let referenced = match kind {
            Kind::Ref(name) => (self.assets)(name).map(|def| (name, def)),
            _ => None,
        };
        let mut remap = BTreeMap::new();
        for (key, value) in object {
            let at = join(path, key);
            if let Some((name, def)) = &referenced {
                if !def.doc.materials.contains_key(key) {
                    let keys: Vec<&str> = def.doc.materials.keys().map(String::as_str).collect();
                    let issue = Issue::error(&at, Code::UnknownMaterial, format!("{name} has no material \"{key}\""));
                    self.push(if keys.is_empty() {
                        issue
                    } else {
                        issue.expected(keys.join(", ")).maybe_fix(issue::suggestion(key, keys.iter().copied()))
                    });
                }
            }
            if let Some(material) = self.material_ref(&at, value) {
                remap.insert(key.clone(), material);
            }
        }
        Some(remap)
    }

    fn prim(&mut self, path: &str, object: &Map<String, Value>, kind: PrimType) -> Prim {
        let at = |key: &str| join(path, key);
        let mut prim = Prim::new(kind);
        let fields = shape_fields(kind);
        let get = |key: &str| if fields.contains(&key) { object.get(key) } else { None };
        if let Some(value) = get("size") {
            prim.size = self.list(&at("size"), value, 2..=3, "sizes [x,y,z]", Self::length);
        }
        if let Some(value) = get("half") {
            prim.half = self.list(&at("half"), value, 3..=3, "half extents [x,y,z]", Self::length).map(|v| Vec3::from_slice(&v));
        }
        if let Some(value) = get("radius") {
            prim.radius =
                if kind == PrimType::RoundedBox { self.extent(&at("radius"), value) } else { self.length(&at("radius"), value) };
        }
        if let Some(value) = get("radius_end") {
            prim.radius_end = self.extent(&at("radius_end"), value);
        }
        for (key, slot) in [
            ("minor_radius", &mut prim.minor_radius),
            ("thickness", &mut prim.thickness),
            ("height", &mut prim.height),
            ("depth", &mut prim.depth),
        ] {
            if let Some(value) = get(key) {
                *slot = self.length(&at(key), value);
            }
        }
        if let Some(value) = get("half_height") {
            prim.half_height = if kind == PrimType::Capsule {
                self.extent(&at("half_height"), value)
            } else {
                self.length(&at("half_height"), value)
            };
        }
        if let Some(value) = get("arc") {
            prim.arc = self.ranged(&at("arc"), value, 1.0, 360.0);
        }
        if let Some(value) = get("facing") {
            prim.facing = self.dir(&at("facing"), value);
        }
        if let Some(value) = get("peak") {
            prim.peak = self.ranged(&at("peak"), value, -1.0, 1.0);
        }
        if let Some(value) = get("segments") {
            let min = if kind == PrimType::Lathe { 6 } else { 3 };
            prim.segments = self.integer(&at("segments"), value, min, 64);
        }
        let pair = |reader: &mut Self, path: &str, value: &Value, first_min: f32| {
            let max = reader.position_range();
            reader
                .list(path, value, 2..=2, "numbers", |reader, path, value| {
                    let min = if path.ends_with("[0]") { first_min } else { -max };
                    reader.ranged(path, value, min, max)
                })
                .map(|v| [v[0], v[1]])
        };
        if let Some(value) = get("profile") {
            prim.profile =
                self.list(&at("profile"), value, 2..=32, "points [r,y]", |reader, path, value| pair(reader, path, value, 0.0));
        }
        if let Some(value) = get("points") {
            let max = self.position_range();
            prim.points =
                self.list(&at("points"), value, 3..=64, "points [x,y]", |reader, path, value| pair(reader, path, value, -max));
            if let Some(points) = &prim.points {
                if let Some(problem) = polygon_problem(points) {
                    self.push(
                        Issue::error(at("points"), Code::UnknownValue, problem)
                            .fix("list the outline once, without crossing lines"),
                    );
                }
            }
        }
        prim
    }

    /// Placement rules: one of pos, from/to or attach; from/to set the direction. Keys this
    /// node cannot take were reported already.
    fn placement(&mut self, path: &str, object: &Map<String, Value>, node: &Node, allowed: &[&str]) {
        let at = |key: &str| join(path, key);
        let has = |key: &str| object.contains_key(key) && allowed.contains(&key);
        match (has("from"), has("to")) {
            (true, false) => self.push(Issue::error(at("to"), Code::Missing, "missing").expected("from and to together")),
            (false, true) => self.push(Issue::error(at("from"), Code::Missing, "missing").expected("from and to together")),
            _ => {}
        }
        let ends = has("from") || has("to");
        if has("pos") && ends {
            self.push(Issue::error(at("pos"), Code::Conflict, "from/to already place the part").fix("remove pos"));
        }
        if has("attach") && (has("pos") || ends) {
            let other = if ends { "from/to" } else { "pos" };
            self.push(
                Issue::error(at("attach"), Code::Conflict, format!("attach and {other} both place the part"))
                    .fix("keep one; offset shifts an attach point"),
            );
        }
        if has("offset") && !has("attach") {
            self.push(Issue::error(at("offset"), Code::Conflict, "offset shifts an attach point").fix("use pos"));
        }
        if ends {
            for key in ["axis", "yaw", "pitch", "roll"] {
                if has(key) {
                    self.push(
                        Issue::error(at(key), Code::Conflict, "from/to already set the part's direction")
                            .fix("remove it; spin turns the part about its axis"),
                    );
                }
            }
        } else if has("spin") {
            self.push(
                Issue::error(at("spin"), Code::Conflict, "spin turns a from/to part about its axis")
                    .fix("use yaw, pitch or roll"),
            );
        }
        if let (Some(from), Some(to)) = (node.from, node.to) {
            let (min, _) = self.length_range();
            if from.distance(to) < min {
                self.push(
                    Issue::error(at("to"), Code::Conflict, "from and to are the same point")
                        .fix("move them apart: they are the part's two ends"),
                );
            }
        }
    }

    /// Shape rules: which fields give the size, and the limits between them.
    fn prim_rules(&mut self, path: &str, object: &Map<String, Value>, prim: &Prim, node: &Node) {
        use PrimType::*;
        let at = |key: &str| join(path, key);
        let fields = shape_fields(prim.kind);
        let takes_ends = prim.kind.takes_ends();
        let has = |key: &str| object.contains_key(key) && (fields.contains(&key) || (takes_ends && matches!(key, "from" | "to")));
        let ends = has("from") || has("to");
        let length = match (node.from, node.to) {
            (Some(from), Some(to)) => Some(from.distance(to)),
            _ => None,
        };
        let need = |reader: &mut Self, key: &str, expected: &str| {
            if !has(key) {
                reader.missing(path, key, expected);
            }
        };
        match prim.kind {
            Box | RoundedBox => match (has("size"), has("half")) {
                (false, false) => need(self, "size", "size:[x,y,z] (with from/to, [across,thick])"),
                (true, true) => self.push(
                    Issue::error(at("half"), Code::Conflict, "size and half both give the size").fix("keep size (the full size)"),
                ),
                (_, true) if ends => self.push(
                    Issue::error(at("half"), Code::Conflict, "a from/to box takes size:[across,thick]")
                        .fix("replace half with size"),
                ),
                _ => {}
            },
            Ellipsoid | Wedge => need(self, "size", "size:[x,y,z]"),
            Sphere => need(self, "radius", ""),
            Torus => {
                need(self, "radius", "");
                need(self, "minor_radius", "");
            }
            Lathe => need(self, "profile", "[[r,y],...]: 2-32 points out from the axis"),
            Extrude => {
                need(self, "points", "[[x,y],...]: the front outline, 3-64 points");
                need(self, "depth", "");
            }
            Capsule | Cylinder | Cone | Tube => {
                need(self, "radius", "");
                if prim.kind == Tube {
                    need(self, "thickness", "the wall, inward from radius");
                }
                let given: Vec<&str> = ["height", "half_height", "from"]
                    .into_iter()
                    .filter(|key| has(key) || (*key == "from" && has("to")))
                    .collect();
                match given.as_slice() {
                    [] => self.missing(path, "height", "height, or from and to"),
                    [_] => {}
                    [first, second, ..] => {
                        let name = |key: &str| if key == "from" { "from/to".to_owned() } else { key.to_owned() };
                        self.push(
                            Issue::error(
                                at(if *second == "from" { "from" } else { second }),
                                Code::Conflict,
                                format!("{} and {} both give the length", name(first), name(second)),
                            )
                            .fix("keep one"),
                        )
                    }
                }
                if prim.kind == Capsule && has("half_height") && has("radius_end") {
                    self.push(
                        Issue::error(at("radius_end"), Code::Conflict, "half_height is format 1's capsule length")
                            .fix("use height (tip to tip) with radius_end"),
                    );
                }
            }
        }
        if let Some(size) = &prim.size {
            let expected = if ends { 2 } else { 3 };
            if size.len() != expected {
                let message = if ends {
                    format!("a from/to {} takes 2 sizes [across,thick], got {}", prim.kind.name(), size.len())
                } else {
                    format!("expected 3 sizes [x,y,z], got {}", size.len())
                };
                let fix = if ends { "the length comes from from/to" } else { "[across,thick] is for a part placed with from/to" };
                self.push(Issue::error(at("size"), Code::InvalidType, message).fix(fix));
            }
        }
        // Half sizes, for the rounded box's radius.
        let halves: Option<Vec<f32>> = match (&prim.size, prim.half, length) {
            (Some(size), _, Some(length)) if size.len() == 2 => Some(vec![size[0] / 2.0, length / 2.0, size[1] / 2.0]),
            (Some(size), _, None) if size.len() == 3 => Some(size.iter().map(|x| x / 2.0).collect()),
            (None, Some(half), _) => Some(half.to_array().to_vec()),
            _ => None,
        };
        if let (RoundedBox, Some(radius), Some(halves)) = (prim.kind, prim.radius, halves) {
            let smallest = halves.into_iter().fold(f32::INFINITY, f32::min);
            if radius > smallest {
                self.push(
                    Issue::error(
                        at("radius"),
                        Code::OutOfRange,
                        format!("{} is more than the smallest half size {}", amount(radius as f64), amount(smallest as f64)),
                    )
                    .expected(format!("0 to {}", amount(smallest as f64))),
                );
            }
        }
        if let (Tube, Some(radius), Some(thickness)) = (prim.kind, prim.radius, prim.thickness) {
            if thickness > radius {
                self.push(
                    Issue::error(
                        at("thickness"),
                        Code::OutOfRange,
                        format!("{} is more than the radius {}", amount(thickness as f64), amount(radius as f64)),
                    )
                    .expected(format!("at most {}", amount(radius as f64))),
                );
            }
        }
        if let (Torus, Some(radius), Some(minor)) = (prim.kind, prim.radius, prim.minor_radius) {
            if minor > radius {
                self.push(
                    Issue::error(
                        at("minor_radius"),
                        Code::OutOfRange,
                        format!("{} is more than the radius {}", amount(minor as f64), amount(radius as f64)),
                    )
                    .expected(format!("at most {}", amount(radius as f64))),
                );
            }
        }
        if let (Capsule, Some(radius)) = (prim.kind, prim.radius) {
            let ends_needed = radius + prim.radius_end.unwrap_or(radius);
            let (key, length) = match (length, prim.height) {
                (Some(length), _) => ("to", length),
                (None, Some(height)) => ("height", height),
                _ => ("", f32::INFINITY),
            };
            if length < ends_needed {
                let what = if key == "to" { "the ends are" } else { "height is" };
                self.push(
                    Issue::error(
                        at(key),
                        Code::OutOfRange,
                        format!(
                            "{what} {} long, less than radius + radius_end ({})",
                            amount(length as f64),
                            amount(ends_needed as f64)
                        ),
                    )
                    .fix("lengthen it or use smaller radii"),
                );
            }
        }
        if let Some(facing) = prim.facing {
            let rotation = Quat::from_euler(
                EulerRot::YXZ,
                node.yaw.unwrap_or(0.0).to_radians(),
                node.pitch.unwrap_or(0.0).to_radians(),
                node.roll.unwrap_or(0.0).to_radians(),
            );
            let axis = rotation * node.axis.unwrap_or(prim.kind.default_axis()).vector();
            if facing.vector().dot(axis).abs() > 1e-4 {
                self.push(
                    Issue::error(at("facing"), Code::Conflict, format!("facing {} is not across the part's axis", facing.name()))
                        .fix("pick a direction at right angles to the axis"),
                );
            }
        }
    }

    fn attach(&mut self, path: &str, value: &Value) -> Option<Attach> {
        match value {
            Value::String(to) => self.target(path, to).map(|to| Attach { to, at: None, own: None }),
            Value::Object(object) => {
                self.unknown_keys(path, object, &["to", "at", "self"], "attach");
                let to = match object.get("to") {
                    None => {
                        self.push(
                            Issue::error(join(path, "to"), Code::Missing, "missing").expected("the id of the part to attach to"),
                        );
                        None
                    }
                    Some(Value::String(to)) => self.target(&join(path, "to"), to),
                    Some(value) => {
                        self.push(wrong_type(&join(path, "to"), "a part id", value));
                        None
                    }
                };
                let at = object.get("at").and_then(|value| self.anchor(&join(path, "at"), value, true));
                let own = object.get("self").and_then(|value| self.anchor(&join(path, "self"), value, false));
                Some(Attach { to: to?, at, own })
            }
            value => {
                self.push(wrong_type(path, "a part id or {to, at, self}", value));
                None
            }
        }
    }

    /// The written form of a resolved id: `name`, `name[i]`, `name~x` and dotted children.
    fn target(&mut self, path: &str, id: &str) -> Option<String> {
        let valid = id.split('.').all(|segment| parse_segment(segment).is_some());
        if !valid {
            self.push(
                Issue::error(path, Code::UnknownValue, format!("\"{id}\" is not a part id"))
                    .expected("name, name[i] for a copy, name~x for a mirror copy, group.child"),
            );
            return None;
        }
        Some(id.into())
    }

    fn anchor(&mut self, path: &str, value: &Value, ends: bool) -> Option<Anchor> {
        match value {
            Value::Array(_) => self
                .list(path, value, 3..=3, "numbers [u,v,w] in -1..1", |reader, path, value| reader.ranged(path, value, -1.0, 1.0))
                .map(|v| Anchor::Point([v[0], v[1], v[2]])),
            Value::String(word) if ends && (word == "from" || word == "to") => Some(Anchor::Word(word.clone())),
            Value::String(word) => {
                let parts: Vec<&str> = word.split('_').collect();
                let known = parts.iter().all(|part| WORDS.contains(part));
                let opposite = |a: &str, b: &str| parts.contains(&a) && parts.contains(&b);
                let unique = parts.iter().collect::<BTreeSet<_>>().len() == parts.len();
                if known && unique && !opposite("top", "bottom") && !opposite("front", "back") && !opposite("left", "right") {
                    return Some(Anchor::Word(word.clone()));
                }
                let expected = if ends {
                    "top, bottom, front, back, left, right, _ combos (top_front), from, to or [u,v,w]"
                } else {
                    "top, bottom, front, back, left, right, _ combos (top_front) or [u,v,w]"
                };
                let fix = match word.as_str() {
                    "center" | "centre" | "middle" => Some("use [0,0,0] for the centre".to_owned()),
                    "from" | "to" => Some("from and to name the target's ends; use at".to_owned()),
                    _ => issue::suggestion(word, WORDS.iter().copied()),
                };
                self.push(
                    Issue::error(path, Code::UnknownValue, format!("unknown anchor \"{word}\""))
                        .expected(expected)
                        .maybe_fix(fix),
                );
                None
            }
            value => {
                self.push(wrong_type(path, "a side such as \"top\" or [u,v,w]", value));
                None
            }
        }
    }

    fn mirror(&mut self, path: &str, value: &Value) -> Option<Vec<Axis>> {
        let axis = |reader: &mut Self, path: &str, value: &Value| {
            reader.choice(path, value, "mirror axis", Axis::NAMES, Axis::parse, |_| None)
        };
        let mut axes = match value {
            Value::Array(_) => self.list(path, value, 1..=3, "axes", axis)?,
            _ => vec![axis(self, path, value)?],
        };
        let count = axes.len();
        axes.sort();
        axes.dedup();
        if axes.len() != count {
            self.push(Issue::error(path, Code::Duplicate, "an axis appears twice").fix("list each axis once"));
            return None;
        }
        Some(axes)
    }

    fn cycle(&mut self, path: &str, value: &Value) -> Option<Vec<String>> {
        self.list(path, value, 1..=64, "entries (material keys, #rrggbb or roles)", |reader, path, value| {
            let entry = reader.text(path, value)?;
            if is_color(&entry) || ROLES.contains(&entry.as_str()) || reader.materials.contains(&entry) {
                return Some(entry);
            }
            if entry.starts_with('#') || entry.starts_with('$') {
                return reader.color(path, value);
            }
            reader.material_ref(path, value)
        })
    }

    fn array(&mut self, path: &str, value: &Value) -> Option<Array> {
        let object = self.object(path, value, &["count", "step", "span", "cycle"], "array")?;
        let at = |key: &str| join(path, key);
        let count = match object.get("count") {
            None => {
                self.missing(path, "count", format!("1 to {MAX_COPIES}"));
                None
            }
            Some(value) => self.integer(&at("count"), value, 1, MAX_COPIES),
        };
        let step = object.get("step").and_then(|value| self.point(&at("step"), value));
        let span = object.get("span").and_then(|value| self.point(&at("span"), value));
        match (object.contains_key("step"), object.contains_key("span")) {
            (false, false) => self.missing(path, "step", "step (between copies) or span (first to last)"),
            (true, true) => {
                self.push(Issue::error(at("span"), Code::Conflict, "step and span both space the copies").fix("keep one"))
            }
            _ => {}
        }
        if span.is_some() && count == Some(1) {
            self.push(
                Issue::error(at("span"), Code::Conflict, "span needs at least 2 copies").fix("use count 2 or more, or step"),
            );
        }
        let cycle = object.get("cycle").and_then(|value| self.cycle(&at("cycle"), value));
        Some(Array { count: count?, step, span, cycle })
    }

    fn grid(&mut self, path: &str, value: &Value) -> Option<Grid> {
        let object = self.object(path, value, &["count", "step", "plane", "cycle"], "grid")?;
        let at = |key: &str| join(path, key);
        let count = match object.get("count") {
            None => {
                self.missing(path, "count", "[a,b], each 1 to 32");
                None
            }
            Some(value) => self.list(&at("count"), value, 2..=2, "counts [a,b]", |reader, path, value| {
                reader.integer(path, value, 1, MAX_GRID_SIDE)
            }),
        };
        if let Some(count) = &count {
            if count[0] * count[1] > MAX_GRID_COPIES {
                self.push(
                    Issue::error(at("count"), Code::Limit, format!("{} copies", count[0] * count[1]))
                        .expected(format!("at most {MAX_GRID_COPIES}")),
                );
            }
        }
        let step = match object.get("step") {
            None => {
                self.missing(path, "step", "[sa,sb]");
                None
            }
            Some(value) => self.list(&at("step"), value, 2..=2, "steps [sa,sb]", Self::coordinate),
        };
        let plane = object.get("plane").and_then(|value| {
            let text = self.text(&at("plane"), value)?;
            let axes: Vec<Option<Axis>> = text.chars().map(|c| Axis::parse(&c.to_string())).collect();
            match axes.as_slice() {
                [Some(a), Some(b)] if a != b => Some([*a, *b]),
                _ => {
                    self.push(
                        Issue::error(at("plane"), Code::UnknownValue, format!("unknown plane \"{text}\""))
                            .expected("two different axes: xz, xy, yz, zx, yx or zy"),
                    );
                    None
                }
            }
        });
        let cycle = object.get("cycle").and_then(|value| self.cycle(&at("cycle"), value));
        let (count, step) = (count?, step?);
        Some(Grid { count: [count[0], count[1]], step: [step[0], step[1]], plane, cycle })
    }

    fn radial(&mut self, path: &str, value: &Value) -> Option<Radial> {
        let object = self.object(path, value, &["count", "radius", "start", "arc", "axis", "cycle"], "radial")?;
        let at = |key: &str| join(path, key);
        let count = match object.get("count") {
            None => {
                self.missing(path, "count", format!("1 to {MAX_COPIES}"));
                None
            }
            Some(value) => self.integer(&at("count"), value, 1, MAX_COPIES),
        };
        let radius = match object.get("radius") {
            None => {
                self.missing(path, "radius", "the distance from the axis");
                None
            }
            Some(value) => self.extent(&at("radius"), value),
        };
        let start = object.get("start").and_then(|value| self.angle(&at("start"), value));
        let arc = object.get("arc").and_then(|value| self.ranged(&at("arc"), value, 1.0, 360.0));
        let axis = object.get("axis").and_then(|value| self.dir(&at("axis"), value));
        let cycle = object.get("cycle").and_then(|value| self.cycle(&at("cycle"), value));
        Some(Radial { count: count?, radius: radius?, start, arc, axis, cycle })
    }

    fn jitter(&mut self, path: &str, value: &Value) -> Option<Jitter> {
        let object = self.object(path, value, &["pos", "rot", "scale", "color"], "jitter")?;
        let at = |key: &str| join(path, key);
        let pos = object
            .get("pos")
            .and_then(|value| self.list(&at("pos"), value, 3..=3, "numbers [x,y,z]", Self::extent))
            .map(|v| Vec3::from_slice(&v));
        let rot = object
            .get("rot")
            .and_then(|value| {
                self.list(&at("rot"), value, 3..=3, "degrees [yaw,pitch,roll]", |reader, path, value| {
                    reader.ranged(path, value, 0.0, 180.0)
                })
            })
            .map(|v| Vec3::from_slice(&v));
        let scale = object.get("scale").and_then(|value| {
            let scale = self.list(&at("scale"), value, 2..=2, "numbers [lo,hi]", |reader, path, value| {
                reader.ranged(path, value, 0.01, 10.0)
            })?;
            if scale[0] > scale[1] {
                self.push(Issue::error(at("scale"), Code::Conflict, "lo is above hi").fix(format!(
                    "write [{},{}]",
                    amount(scale[1] as f64),
                    amount(scale[0] as f64)
                )));
                return None;
            }
            Some([scale[0], scale[1]])
        });
        let color = object.get("color").and_then(|value| self.ranged(&at("color"), value, 0.0, 1.0));
        Some(Jitter { pos, rot, scale, color })
    }

    fn light(&mut self, path: &str, value: &Value) -> Option<Light> {
        let object = self.object(path, value, &["color", "radius", "intensity", "flicker", "pulse", "shadows"], "light")?;
        let at = |key: &str| join(path, key);
        Some(Light {
            color: object.get("color").and_then(|v| self.color(&at("color"), v)),
            radius: object.get("radius").and_then(|v| self.length(&at("radius"), v)),
            intensity: object.get("intensity").and_then(|v| self.ranged(&at("intensity"), v, 0.0, 32.0)),
            flicker: object.get("flicker").and_then(|v| self.ranged(&at("flicker"), v, 0.0, 1.0)),
            pulse: object.get("pulse").and_then(|v| self.ranged(&at("pulse"), v, 0.0, 10.0)),
            shadows: object.get("shadows").and_then(|v| self.boolean(&at("shadows"), v)),
        })
    }

    fn fx(&mut self, path: &str, value: &Value) -> Option<Fx> {
        let object = self.object(path, value, &["preset", "rate", "color", "size", "speed", "life", "area"], "fx")?;
        let at = |key: &str| join(path, key);
        let preset = match object.get("preset") {
            None => {
                self.missing(path, "preset", PRESETS.join(", "));
                None
            }
            Some(value) => self.word(&at("preset"), value, "preset", PRESETS).map(String::from),
        };
        let mut multiplier = |key: &str| object.get(key).and_then(|v| number(&at(key), v, 0.01, 10.0, &mut self.issues));
        let (size, speed, life) = (multiplier("size"), multiplier("speed"), multiplier("life"));
        Some(Fx {
            preset: preset?,
            rate: object.get("rate").and_then(|v| self.ranged(&at("rate"), v, 0.0, 1_000.0)),
            color: object.get("color").and_then(|v| self.color(&at("color"), v)),
            size,
            speed,
            life,
            area: object
                .get("area")
                .and_then(|v| self.list(&at("area"), v, 3..=3, "half extents [x,y,z]", Self::extent))
                .map(|v| Vec3::from_slice(&v)),
        })
    }

    fn sockets(&mut self, value: &Value) -> BTreeMap<String, Socket> {
        let mut sockets = BTreeMap::new();
        let Some(object) = value.as_object() else {
            self.push(wrong_type("sockets", "an object {name: socket}", value));
            return sockets;
        };
        for (name, value) in object {
            let path = format!("sockets.{name}");
            if SocketName::parse(name).is_none() {
                self.push(
                    Issue::error(&path, Code::UnknownValue, format!("unknown socket \"{name}\""))
                        .expected(SocketName::NAMES.join(", "))
                        .maybe_fix(issue::suggestion(name, SocketName::NAMES.iter().copied())),
                );
                continue;
            }
            let Some(fields) = self.object(&path, value, &["pos", "attach", "offset", "axis", "yaw", "pitch", "roll"], "socket")
            else {
                continue;
            };
            let at = |key: &str| join(&path, key);
            let mut socket = Socket {
                pos: fields.get("pos").and_then(|v| self.point(&at("pos"), v)),
                attach: fields.get("attach").and_then(|v| self.attach(&at("attach"), v)),
                offset: fields.get("offset").and_then(|v| self.point(&at("offset"), v)),
                axis: fields.get("axis").and_then(|v| self.dir(&at("axis"), v)),
                ..Socket::default()
            };
            for (key, slot) in [("yaw", &mut socket.yaw), ("pitch", &mut socket.pitch), ("roll", &mut socket.roll)] {
                if let Some(value) = fields.get(key) {
                    *slot = self.angle(&at(key), value);
                }
            }
            if fields.contains_key("pos") && fields.contains_key("attach") {
                self.push(
                    Issue::error(at("attach"), Code::Conflict, "attach and pos both place the socket")
                        .fix("keep one; offset shifts an attach point"),
                );
            }
            if fields.contains_key("offset") && !fields.contains_key("attach") {
                self.push(Issue::error(at("offset"), Code::Conflict, "offset shifts an attach point").fix("use pos"));
            }
            if fields.contains_key("axis") && ["yaw", "pitch", "roll"].iter().any(|key| fields.contains_key(*key)) {
                self.push(
                    Issue::error(at("axis"), Code::Conflict, "axis and yaw/pitch/roll both turn the socket").fix("keep one"),
                );
            }
            sockets.insert(name.clone(), socket);
        }
        sockets
    }

    /// Attach targets resolve to parts, and no part's position depends on itself.
    fn attachments(&mut self, parts: &BTreeMap<String, Node>, raw: &Map<String, Value>, sockets: &BTreeMap<String, Socket>) {
        let mut edges: BTreeMap<Vec<String>, Vec<Vec<String>>> = BTreeMap::new();
        let mut stack = vec![(Vec::<String>::new(), parts, raw)];
        while let Some((parent, children, raw)) = stack.pop() {
            for (name, node) in children {
                let mut own = parent.clone();
                own.push(name.clone());
                let path = node_path(&own);
                if let Some(attach) = &node.attach {
                    let field = if attach.at.is_none() && attach.own.is_none() {
                        join(&path, "attach")
                    } else {
                        join(&path, "attach.to")
                    };
                    let target = self.resolve(&field, parts, Some(raw), &own, &attach.to);
                    let inside =
                        |outer: &[String], inner: &[String]| inner.len() >= outer.len() && inner[..outer.len()] == *outer;
                    let target = match target {
                        Some(target) if target == own => {
                            self.push(
                                Issue::error(&field, Code::Cycle, "a part cannot attach to itself")
                                    .fix("attach it to another part"),
                            );
                            None
                        }
                        Some(target) if inside(&target, &own) => {
                            self.push(
                                Issue::error(&field, Code::Cycle, format!("{} is a group around this part", target.join(".")))
                                    .fix("attach to a part outside it"),
                            );
                            None
                        }
                        Some(target) if inside(&own, &target) => {
                            self.push(
                                Issue::error(&field, Code::Cycle, format!("{} is inside this group", target.join(".")))
                                    .fix("place the group with pos, or attach it to a part outside it"),
                            );
                            None
                        }
                        other => other,
                    };
                    if let Some(target) = target {
                        if let Some(Anchor::Word(word)) = &attach.at {
                            let target_node = find(parts, &target);
                            if (word == "from" || word == "to") && target_node.is_some_and(|t| t.from.is_none()) {
                                self.push(
                                    Issue::error(
                                        join(&path, "attach.at"),
                                        Code::Conflict,
                                        format!("at:\"{word}\" needs a target placed with from/to"),
                                    )
                                    .fix("use a side such as top or bottom"),
                                );
                            }
                        }
                        edges.entry(own.clone()).or_default().extend(dependencies(parts, &parent, &own, node, &target));
                    }
                }
                if let (Some(grandchildren), Some(Value::Object(raw_children))) =
                    (node.children(), raw.get(name).and_then(|v| v.get("parts")))
                {
                    stack.push((own, grandchildren, raw_children));
                }
            }
        }
        for (name, socket) in sockets {
            if let Some(attach) = &socket.attach {
                let field = if attach.at.is_none() && attach.own.is_none() {
                    format!("sockets.{name}.attach")
                } else {
                    format!("sockets.{name}.attach.to")
                };
                self.resolve(&field, parts, None, &[], &attach.to);
            }
        }
        // Depth-first search for a placement that depends on itself.
        let mut done = BTreeSet::new();
        for start in edges.keys() {
            let mut trail = Vec::new();
            if let Some(cycle) = find_cycle(start, &edges, &mut trail, &mut done) {
                let names: Vec<String> = cycle.iter().chain(cycle.first()).map(|id| id.join(".")).collect();
                self.push(
                    Issue::error(
                        join(&node_path(&cycle[0]), "attach"),
                        Code::Cycle,
                        format!("attach goes round in a circle: {}", names.join(" → ")),
                    )
                    .fix("attach one of them to something else, or place it with pos"),
                );
                break;
            }
        }
    }

    /// The node an attach id names, among the attaching node's siblings (`group.part` reaches
    /// inside one). Copies need their index; mirror copies their `~axes`.
    /// `own` is the attaching node's path (empty for a socket, which sits at the top level).
    fn resolve(
        &mut self,
        field: &str,
        parts: &BTreeMap<String, Node>,
        raw_siblings: Option<&Map<String, Value>>,
        own: &[String],
        id: &str,
    ) -> Option<Vec<String>> {
        let segments: Vec<(&str, Option<usize>, Vec<Axis>)> = id.split('.').filter_map(parse_segment).collect();
        let parent = own.split_last().map_or(&[][..], |(_, parent)| parent);
        let siblings = if parent.is_empty() { Some(parts) } else { find(parts, parent).and_then(Node::children) };
        let (first, ..) = *segments.first()?;
        let (mut current, mut path): (&BTreeMap<String, Node>, Vec<String>) = match siblings {
            Some(siblings) if siblings.contains_key(first) => (siblings, parent.to_vec()),
            _ if parent.iter().any(|group| group == first) => {
                self.push(
                    Issue::error(field, Code::Cycle, format!("{first} is a group around this part"))
                        .fix("attach to a part beside this one"),
                );
                return None;
            }
            _ => {
                if raw_siblings.is_some_and(|raw| raw.contains_key(first)) {
                    return None; // That part has its own errors.
                }
                let names: Vec<&str> = siblings
                    .map(|s| s.keys().map(String::as_str).filter(|name| own.last().is_none_or(|me| me != name)).collect())
                    .unwrap_or_default();
                let fix = issue::suggestion(first, names.iter().copied()).or_else(|| {
                    (!parent.is_empty() && parts.contains_key(first)).then(|| {
                        "attach names a part beside this one (or group.part inside one); move this part next to it".to_owned()
                    })
                });
                self.push(Issue::error(field, Code::UnknownPart, format!("no part \"{first}\" here")).maybe_fix(fix));
                return None;
            }
        };
        for (index, (name, copy, mirror)) in segments.iter().enumerate() {
            let Some(node) = current.get(*name) else {
                let names: Vec<&str> = current.keys().map(String::as_str).collect();
                self.push(
                    Issue::error(field, Code::UnknownPart, format!("no part \"{name}\" in {}", path.join(".")))
                        .expected(names.join(", "))
                        .maybe_fix(issue::suggestion(name, names.iter().copied())),
                );
                return None;
            };
            path.push((*name).to_owned());
            let copies = node.copies();
            match copy {
                Some(i) if *i >= copies => {
                    let expected = if copies == 1 {
                        format!("{name} has no copies: write {name}")
                    } else {
                        format!("{name}[0] to {name}[{}]", copies - 1)
                    };
                    self.push(Issue::error(field, Code::UnknownPart, format!("{name}[{i}] does not exist")).expected(expected));
                    return None;
                }
                None if copies > 1 => {
                    self.push(
                        Issue::error(field, Code::UnknownPart, format!("{name} has {copies} copies"))
                            .fix(format!("name one: {name}[0] to {name}[{}]", copies - 1)),
                    );
                    return None;
                }
                _ => {}
            }
            if let Some(axis) = mirror.iter().find(|axis| !node.mirror.contains(axis)) {
                self.push(Issue::error(field, Code::UnknownPart, format!("{name} has no mirror copy in {}", axis.name())));
                return None;
            }
            if index + 1 < segments.len() {
                match node.children() {
                    Some(children) => current = children,
                    None => {
                        self.push(
                            Issue::error(field, Code::UnknownPart, format!("{name} is not a group"))
                                .fix(format!("attach to {name} itself")),
                        );
                        return None;
                    }
                }
            }
        }
        Some(path)
    }
}

/// `name`, `name[i]`, `name~xz`, `name[i]~x`: the name, the copy and the mirror axes.
fn parse_segment(segment: &str) -> Option<(&str, Option<usize>, Vec<Axis>)> {
    let (rest, mirror) = match segment.split_once('~') {
        Some((rest, axes)) => {
            let axes: Option<Vec<Axis>> = axes.chars().map(|c| Axis::parse(&c.to_string())).collect();
            let axes = axes.filter(|axes| !axes.is_empty() && axes.windows(2).all(|pair| pair[0] < pair[1]))?;
            (rest, axes)
        }
        None => (segment, Vec::new()),
    };
    let (name, copy) = match rest.split_once('[') {
        Some((name, index)) => (name, Some(index.strip_suffix(']')?.parse().ok()?)),
        None => (rest, None),
    };
    validate_part_name(name).ok()?;
    Some((name, copy, mirror))
}

fn node_path(ids: &[String]) -> String {
    ids.iter().map(|id| format!("parts.{id}")).collect::<Vec<_>>().join(".")
}

fn find<'a>(parts: &'a BTreeMap<String, Node>, path: &[String]) -> Option<&'a Node> {
    let (first, rest) = path.split_first()?;
    let mut node = parts.get(first)?;
    for name in rest {
        node = node.children()?.get(name)?;
    }
    Some(node)
}

/// What a node's placement depends on when it attaches to `target`: the target, the target's
/// ancestors below the node's parent (their frames move it), every part inside a target group,
/// and, for a group, its own children (its box is theirs).
fn dependencies(
    parts: &BTreeMap<String, Node>,
    parent: &[String],
    own: &[String],
    node: &Node,
    target: &[String],
) -> Vec<Vec<String>> {
    let mut out = vec![target.to_vec()];
    let common = parent.iter().zip(target).take_while(|(a, b)| a == b).count();
    for length in (common + 1)..target.len() {
        out.push(target[..length].to_vec());
    }
    let mut descend = |root: &[String], node: &Node| {
        let mut stack = vec![(root.to_vec(), node)];
        while let Some((path, node)) = stack.pop() {
            for (name, child) in node.children().into_iter().flatten() {
                let mut child_path = path.clone();
                child_path.push(name.clone());
                out.push(child_path.clone());
                stack.push((child_path, child));
            }
        }
    };
    if let Some(target_node) = find(parts, target) {
        descend(target, target_node);
    }
    descend(own, node);
    out
}

fn find_cycle(
    node: &Vec<String>,
    edges: &BTreeMap<Vec<String>, Vec<Vec<String>>>,
    trail: &mut Vec<Vec<String>>,
    done: &mut BTreeSet<Vec<String>>,
) -> Option<Vec<Vec<String>>> {
    if let Some(start) = trail.iter().position(|seen| seen == node) {
        return Some(trail[start..].to_vec());
    }
    if done.contains(node) {
        return None;
    }
    trail.push(node.clone());
    for next in edges.get(node).into_iter().flatten() {
        if let Some(cycle) = find_cycle(next, edges, trail, done) {
            return Some(cycle);
        }
    }
    trail.pop();
    done.insert(node.clone());
    None
}

/// Authored nodes, groups and their children included.
fn count_nodes(parts: &Map<String, Value>) -> usize {
    parts.values().map(|node| 1 + node.get("parts").and_then(Value::as_object).map_or(0, count_nodes)).sum()
}

/// Why a front-view outline cannot be extruded, if it cannot.
fn polygon_problem(points: &[[f32; 2]]) -> Option<String> {
    let n = points.len();
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    for i in 0..n {
        for j in (i + 1)..n {
            // Neighbouring edges share a point; skip them.
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (points[i], points[(i + 1) % n], points[j], points[(j + 1) % n]);
            let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                return Some(format!("edges {i} and {j} of the outline cross"));
            }
        }
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        / 2.0;
    (area.abs() < 1e-9).then(|| "the outline has no area".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::Doc;

    /// Every problem in a document with these parts and two materials, one line each.
    fn errors(parts: &str) -> Vec<String> {
        document(&format!(
            r##"{{"format":2,"name":"probe","unit":"cm","materials":{{"wood":{{"color":"#8a5a35"}},"iron":{{"color":"#3a3532"}}}},"parts":{{{parts}}}}}"##
        ))
    }

    fn document(text: &str) -> Vec<String> {
        match Doc::read(text) {
            Ok(_) => Vec::new(),
            Err(issues) => issues.iter().map(Issue::to_string).collect(),
        }
    }

    fn check(cases: &[(&str, &[&str])]) {
        for (parts, expected) in cases {
            assert_eq!(errors(parts), *expected, "{parts}");
        }
    }

    #[test]
    fn common_guesses_get_their_fix() {
        check(&[
            (
                r##""a":{"type":"box","size":[10,10,10],"colour":"#ff0000"}"##,
                &[r#"parts.a.colour: unknown key — spelled "color""#],
            ),
            (
                r#""a":{"type":"box","size":[10,10,10],"mat":"wood","rotation":[0,45,0]}"#,
                &[
                    "parts.a.rotation: unknown key — use axis (a round part's direction), from/to (a part between two points) or yaw, pitch and roll in degrees",
                ],
            ),
            (
                r#""a":{"type":"box","size":[10,10,10],"mat":"wood","position":[0,5,0]}"#,
                &["parts.a.position: unknown key — use pos:[x,y,z] (the centre)"],
            ),
            (
                r#""a":{"type":"box","dimensions":[10,10,10],"mat":"wood"}"#,
                &["parts.a.dimensions: unknown key — use size:[x,y,z] (the full size)"],
            ),
            (
                r#""a":{"type":"box","half_extents":[5,5,5],"mat":"wood"}"#,
                &[r#"parts.a.half_extents: unknown key — use "size":[10,10,10] (the full size)"#],
            ),
            (r#""a":{"type":"sphere","diameter":10,"mat":"wood"}"#, &[r#"parts.a.diameter: unknown key — use "radius":5"#]),
            (
                r#""a":{"type":"box","size":[10,10,10],"material":"wood"}"#,
                &["parts.a.material: unknown key — use mat (a key in materials)"],
            ),
            (
                r#""a":{"type":"cylinder","radius":2,"length":30,"mat":"wood"}"#,
                &["parts.a.length: unknown key — use height, or from/to for the two ends"],
            ),
            (
                r#""a":{"children":{"b":{"type":"box","size":[1,1,1]}},"mat":"wood"}"#,
                &["parts.a.type: missing (expected type, ref or parts) — a group's children go in parts:{id:{...}}"],
            ),
            (
                r#""a":{"type":"box","size":[10,10,10],"mat":"wood","count":4}"#,
                &["parts.a.count: unknown key — use array:{count,step} (or grid, radial)"],
            ),
            (
                r#""a":{"type":"box","size":[10,10,10],"mat":"wood","collide":false}"#,
                &["parts.a.collide: unknown key — use solid (true: part of the collider)"],
            ),
            (
                r#""a":{"shape":{"type":"box","half":[5,5,5]},"mat":"wood"}"#,
                &[
                    r#"parts.a.type: missing (expected type, ref or parts) — format 2 parts are flat: put type and the shape fields on the part, e.g. "type":"box","size":[...]"#,
                ],
            ),
            (r#""a":{"type":"sphere","radus":5,"mat":"wood"}"#, &[r#"parts.a.radus: unknown key — did you mean "radius"?"#]),
            (
                r#""a":{"type":"box","size":[0.05,1,1],"mat":"wood"}"#,
                &["parts.a.size[0]: 0.05 is outside 0.1 to 100000 cm — lengths are in cm: 0.05 m is 5"],
            ),
        ]);
        let types = "(expected box, rounded_box, sphere, capsule, cylinder, ellipsoid, cone, tube, torus, wedge, lathe, extrude)";
        for (kind, fix) in [
            ("cube", "use box"),
            ("frustum", "use cone with radius_end"),
            ("ring", "use torus (a ring) or tube (a pipe)"),
            ("ramp", "use wedge with peak"),
            ("gable", "use wedge with peak"),
            ("mesh", "imported meshes come from asset_import, which is not available yet"),
        ] {
            let parts = format!(r#""a":{{"type":"{kind}","mat":"wood"}}"#);
            assert_eq!(errors(&parts), [format!(r#"parts.a.type: unknown shape type "{kind}" {types} — {fix}"#)]);
        }
    }

    #[test]
    fn placement_and_size_rules() {
        let ends_only = "from/to place a box, rounded_box, cylinder, cone, tube or capsule: use pos";
        check(&[
            (
                r#""a":{"type":"box","size":[10,4],"from":[0,0,0],"to":[0,50,0],"pos":[0,0,0],"mat":"wood"}"#,
                &["parts.a.pos: from/to already place the part — remove pos"],
            ),
            (
                r#""a":{"type":"box","size":[10,4],"from":[0,0,0],"mat":"wood"}"#,
                &["parts.a.to: missing (expected from and to together)"],
            ),
            (
                r#""a":{"type":"sphere","radius":5,"from":[0,0,0],"to":[0,10,0],"mat":"wood"}"#,
                &[&format!("parts.a.from: unknown key — {ends_only}"), &format!("parts.a.to: unknown key — {ends_only}")],
            ),
            (
                r#""a":{"type":"box","size":[10,10,10],"axis":"x","mat":"wood"}"#,
                &["parts.a.axis: unknown key — a box has no axis: turn it with yaw, pitch and roll"],
            ),
            (
                r#""a":{"type":"box","size":[10,4,6],"from":[0,0,0],"to":[0,50,0],"mat":"wood"}"#,
                &["parts.a.size: a from/to box takes 2 sizes [across,thick], got 3 — the length comes from from/to"],
            ),
            (
                r#""a":{"type":"box","size":[10,4],"mat":"wood"}"#,
                &["parts.a.size: expected 3 sizes [x,y,z], got 2 — [across,thick] is for a part placed with from/to"],
            ),
            (
                r#""a":{"type":"box","size":[10,4],"from":[0,0,0],"to":[0,50,0],"yaw":10,"mat":"wood"}"#,
                &["parts.a.yaw: from/to already set the part's direction — remove it; spin turns the part about its axis"],
            ),
            (
                r#""a":{"type":"cylinder","radius":2,"height":10,"spin":10,"mat":"wood"}"#,
                &["parts.a.spin: spin turns a from/to part about its axis — use yaw, pitch or roll"],
            ),
            (
                r#""a":{"type":"capsule","radius":5,"from":[0,0,0],"to":[0,8,0],"mat":"wood"}"#,
                &["parts.a.to: the ends are 8 long, less than radius + radius_end (10) — lengthen it or use smaller radii"],
            ),
            (
                r#""a":{"type":"capsule","radius":5,"radius_end":4,"height":8,"mat":"wood"}"#,
                &["parts.a.height: height is 8 long, less than radius + radius_end (9) — lengthen it or use smaller radii"],
            ),
            (
                r#""a":{"type":"tube","radius":5,"thickness":6,"height":8,"mat":"wood"}"#,
                &["parts.a.thickness: 6 is more than the radius 5 (expected at most 5)"],
            ),
            (
                r#""a":{"type":"torus","radius":5,"minor_radius":6,"mat":"wood"}"#,
                &["parts.a.minor_radius: 6 is more than the radius 5 (expected at most 5)"],
            ),
            (
                r#""a":{"type":"rounded_box","size":[10,4,6],"radius":3,"mat":"wood"}"#,
                &["parts.a.radius: 3 is more than the smallest half size 2 (expected 0 to 2)"],
            ),
            (
                r#""a":{"type":"cylinder","radius":2,"mat":"wood"}"#,
                &["parts.a.height: missing (expected height, or from and to)"],
            ),
            (
                r#""a":{"type":"cylinder","radius":2,"height":10,"half_height":5,"mat":"wood"}"#,
                &["parts.a.half_height: height and half_height both give the length — keep one"],
            ),
            (
                r#""a":{"type":"cylinder","radius":5,"height":2,"arc":180,"facing":"y","mat":"wood"}"#,
                &["parts.a.facing: facing y is not across the part's axis — pick a direction at right angles to the axis"],
            ),
            (
                r#""a":{"type":"cylinder","radius":2,"height":4,"axis":"+x","mat":"wood"}"#,
                &[r#"parts.a.axis: unknown direction "+x" (expected x, y, z, -x, -y, -z) — write "x""#],
            ),
            (
                r#""a":{"type":"extrude","points":[[0,0],[10,10],[10,0],[0,10]],"depth":1,"mat":"wood"}"#,
                &["parts.a.points: edges 0 and 2 of the outline cross — list the outline once, without crossing lines"],
            ),
        ]);
        // Turned by yaw, an arc's facing is compared in the parent frame.
        assert!(
            errors(r#""a":{"type":"cylinder","radius":5,"height":2,"arc":180,"facing":"x","yaw":90,"mat":"wood"}"#).is_empty()
        );
    }

    #[test]
    fn copies_variation_and_extras() {
        check(&[
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","array":{"count":3,"step":[2,0,0],"span":[4,0,0]}}"#,
                &["parts.a.array.span: step and span both space the copies — keep one"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","array":{"count":3,"step":[2,0,0]},"radial":{"count":3,"radius":5}}"#,
                &[
                    "parts.a.radial: one of array, grid or radial per node; this one has array and radial — nest a group to combine them",
                ],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","array":{"count":3,"spacing":[2,0,0]}}"#,
                &["parts.a.array.spacing: unknown key — use step:[x,y,z] (between copies) or span (first to last)"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","array":{"count":0,"step":[2,0,0]}}"#,
                &["parts.a.array.count: 0 is outside 1 to 64"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","grid":{"count":[20,20],"step":[2,2]}}"#,
                &["parts.a.grid.count: 400 copies (expected at most 256)"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","pick":1}"#,
                &["parts.a.pick: unknown key — pick chooses among a group's children"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","hidden":true,"solid":false}"#,
                &[
                    "parts.a.hidden: a hidden part is collision only, so it must be solid — remove solid:false (here or on an enclosing group), or drop hidden",
                ],
            ),
            (
                r#""g":{"light":{},"parts":{"b":{"type":"box","size":[1,1,1],"mat":"wood"}}}"#,
                &["parts.g.light: unknown key — put light on a part with a type"],
            ),
            (
                r#""a":{"type":"sphere","radius":1,"mat":"wood","light":{},"array":{"count":5,"step":[2,0,0]}}"#,
                &["parts: 5 lights after copies (expected at most 4)"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","mirror":["x","x"]}"#,
                &["parts.a.mirror: an axis appears twice — list each axis once"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","jitter":{"scale":[1.2,0.8]}}"#,
                &["parts.a.jitter.scale: lo is above hi — write [0.8,1.2]"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","fx":{"preset":"fier"}}"#,
                &[
                    r#"parts.a.fx.preset: unknown preset "fier" (expected fire, smoke, sparks, embers, fountain, snow, rain, fireflies, magic, steam, dust, bubbles, confetti) — did you mean "fire"?"#,
                ],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"array":{"count":2,"step":[2,0,0],"cycle":["wood","oak"]}}"#,
                &[r#"parts.a.array.cycle[1]: no material "oak" (expected iron, wood)"#],
            ),
            (
                r#""g1":{"mat":"wood","parts":{"g2":{"parts":{"g3":{"parts":{"g4":{"parts":{"g5":{"parts":{"b":{"type":"box","size":[1,1,1]}}}}}}}}}}}"#,
                &["parts.g1.parts.g2.parts.g3.parts.g4.parts.g5: groups nest 5 deep (expected at most 4)"],
            ),
        ]);
    }

    #[test]
    fn references_resolve_with_did_you_mean() {
        let seat = r#""seat":{"type":"box","size":[10,10,10],"mat":"wood"}"#;
        let pair = r#""seat":{"type":"box","size":[10,10,10],"mat":"wood","array":{"count":2,"step":[20,0,0]}}"#;
        let on = |target: &str| format!(r#""a":{{"type":"box","size":[1,1,1],"mat":"wood","attach":{target}}}"#);
        check(&[
            (
                r#""a":{"type":"sphere","radius":5,"mat":"wod"}"#,
                &[r#"parts.a.mat: no material "wod" (expected iron, wood) — did you mean "wood"?"#],
            ),
            (
                r#""a":{"type":"sphere","radius":5,"mat":"MAT/oak_planks"}"#,
                &[
                    r#"parts.a.mat: no library material "MAT/oak_planks" — library materials are not available yet: write one under materials"#,
                ],
            ),
            (
                r#""a":{"type":"sphere","radius":5}"#,
                &[
                    "parts.a: no colour: nothing gives this part color or mat — add color or mat here, on an enclosing group or at the top of the document",
                ],
            ),
            (&format!("{seat},{}", on(r#""saet""#)), &[r#"parts.a.attach: no part "saet" here — did you mean "seat"?"#]),
            (&format!("{pair},{}", on(r#""seat""#)), &["parts.a.attach: seat has 2 copies — name one: seat[0] to seat[1]"]),
            (
                &format!("{pair},{}", on(r#""seat[2]""#)),
                &["parts.a.attach: seat[2] does not exist (expected seat[0] to seat[1])"],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","attach":"b"},"b":{"type":"box","size":[1,1,1],"mat":"wood","attach":"a"}"#,
                &[
                    "parts.a.attach: attach goes round in a circle: a → b → a — attach one of them to something else, or place it with pos",
                ],
            ),
            (&on(r#""a""#), &["parts.a.attach: a part cannot attach to itself — attach it to another part"]),
            (
                r#""g":{"mat":"wood","parts":{"c":{"type":"box","size":[1,1,1],"attach":"g"}}}"#,
                &["parts.g.parts.c.attach: g is a group around this part — attach to a part beside this one"],
            ),
            (
                &format!("{seat},{}", on(r#"{"to":"seat","at":"to"}"#)),
                &[r#"parts.a.attach.at: at:"to" needs a target placed with from/to — use a side such as top or bottom"#],
            ),
            (
                &format!("{seat},{}", on(r#"{"to":"seat","at":"center"}"#)),
                &[
                    r#"parts.a.attach.at: unknown anchor "center" (expected top, bottom, front, back, left, right, _ combos (top_front), from, to or [u,v,w]) — use [0,0,0] for the centre"#,
                ],
            ),
            (r#""a":{"ref":"BUILTIN/bnech"}"#, &[r#"parts.a.ref: no asset "BUILTIN/bnech" — did you mean "BUILTIN/bench"?"#]),
            (
                r##""a":{"ref":"BUILTIN/bench","color":"#ff0000"}"##,
                &["parts.a.color: unknown key — a ref keeps its own surfaces: remap:{its material: a material here}"],
            ),
            (
                r#""a":{"ref":"BUILTIN/bench","remap":{"wood":"iron"}}"#,
                &[r#"parts.a.remap.wood: BUILTIN/bench has no material "wood""#],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"parts":{}}"#,
                &[
                    "parts.a: a node has one of type, ref, mesh or parts; this one has type and parts — split it: a group's parts can hold the primitive",
                ],
            ),
            (r#""a":{"mat":"wood","parts":{}}"#, &["parts.a.parts: an empty group (expected at least one part)"]),
        ]);
        // Siblings come first: each copy of a group attaches inside itself.
        let sibling = r#""g":{"mat":"wood","array":{"count":2,"step":[50,0,0]},"parts":{"body":{"type":"box","size":[10,10,10]},"lid":{"type":"box","size":[10,2,10],"attach":"body"}}}"#;
        assert!(errors(sibling).is_empty());
        assert!(errors(r#""a":{"ref":"BUILTIN/bench","pos":[0,0,0],"scale":2,"remap":{}}"#).is_empty());
    }

    #[test]
    fn document_fields_duplicates_and_colours() {
        let doc = |fields: &str, parts: &str| {
            document(&format!(r##"{{"format":2,"name":"probe","unit":"cm","color":"#ffffff",{fields}"parts":{{{parts}}}}}"##))
        };
        let cube = r#""a":{"type":"box","size":[1,1,1]}"#;
        assert_eq!(
            document(
                r##"{"format":2,"name":"probe","unit":"mm","parts":{"a":{"type":"box","size":[1,1,1],"color":"#ffffff"}}}"##
            ),
            [r#"unit: unknown unit "mm" (expected cm, m) — use cm (lengths in tenths of a centimetre are fine)"#]
        );
        assert_eq!(
            doc(r#""sockets":{"grip_2":{"pos":[0,0,0]}},"#, cube),
            [
                r#"sockets.grip_2: unknown socket "grip_2" (expected grip, grip2, tip, top, wall_mount, hang, sit) — did you mean "grip2"?"#
            ]
        );
        assert_eq!(
            doc(r#""tags":["Wood","ok"],"category":"furnature","#, cube),
            [
                r#"tags[0]: "Wood" is not a tag (expected 1-24 characters of a-z, 0-9 and _)"#,
                r#"category: unknown category "furnature" (expected furniture, container, architecture, lighting, vegetation, rock, weapon, armour, tool, food, town, dungeon, signage) — did you mean "furniture"?"#,
            ]
        );
        assert_eq!(doc(r#""tags":["ok","ok"],"#, cube), [r#"tags[1]: "ok" appears twice — keep one"#]);
        assert_eq!(
            doc("", r#""g":{"parts":{"a":{"type":"box","size":[1,1,1],"size":[2,2,2]}}}"#),
            [r#"parts.g.parts.a.size: "size" appears twice — keep one; names must be unique"#]
        );
        assert_eq!(doc(r#""params":{},"#, cube), ["params: unknown key — parametric families are not available yet"]);
        assert_eq!(
            document(
                r##"{"format":2,"name":"probe","unit":"cm","materials":{"m":{"color":"#ffffff","layers":[{"type":"plank","size":10}],"texture":"x"},"n":{"look":"cutout"}},"parts":{"a":{"type":"box","size":[1,1,1],"mat":"m"}}}"##
            ),
            [
                "materials.m.texture: unknown key — use layers:[{type,...}]",
                r#"materials.m.layers[0].type: unknown pattern "plank" (expected noise, voronoi, wave, brick, checker, gradient, magic, planks, tiles, cobbles, stripes, dots, shingles, wood) — did you mean "planks"?"#,
                r#"materials.n.look: unknown look "cutout" (expected flat, cel, lit, unlit)"#,
                "materials.n.color: missing — a material needs color",
            ]
        );
        check(&[
            (
                r#""a":{"type":"box","size":[1,1,1],"color":"$wal"}"#,
                &[
                    r#"parts.a.color: unknown role "$wal" (expected $floor, $floor2, $wall, $pillar, $accent, $light) — did you mean "$wall"?"#,
                ],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"color":"red"}"#,
                &[
                    r##"parts.a.color: "red" is not a #rrggbb colour or a role (expected #rrggbb or $floor, $floor2, $wall, $pillar, $accent, $light) — use a hex colour such as "#ff0000""##,
                ],
            ),
            (
                r#""a":{"type":"box","size":[1,1,1],"mat":"wood","look":"toon"}"#,
                &[r#"parts.a.look: unknown look "toon" (expected flat, cel, lit, unlit, cutout) — cel is the toon look"#],
            ),
            (
                r#""a":{"mesh":"kenney/barrel","pos":[0,0,0]}"#,
                &["parts.a.mesh: mesh parts need asset_import, which is not available yet — build the shape from primitives"],
            ),
        ]);
        let many: Vec<String> = (0..300).map(|i| format!(r#""p{i}":{{"type":"box","size":[1,1,1],"mat":"wood"}}"#)).collect();
        assert_eq!(errors(&many.join(",")), ["parts: 300 nodes (expected 1 to 256, groups and refs included)"]);
    }

    #[test]
    fn the_field_guide_covers_every_key() {
        fn guide(rows: &[(&'static str, &str)]) -> BTreeSet<&'static str> {
            rows.iter().map(|(key, _)| *key).collect()
        }
        let node_keys: BTreeSet<&str> = KINDS.iter().chain(NODE_ORDER).copied().collect();
        assert_eq!(guide(NODE_FIELDS), node_keys);
        for (index, (name, fields, line)) in SHAPES.iter().enumerate() {
            assert_eq!(PrimType::NAMES[index], *name, "SHAPES follows PrimType");
            for field in *fields {
                assert!(line.contains(field), "{name} guide names {field}");
            }
        }
        assert_eq!(MATERIAL_FIELDS.len(), 10);
        for key in ["format", "name", "unit", "description", "tags", "category", "source", "fit", "materials", "parts", "sockets"]
        {
            assert!(guide(DOCUMENT_FIELDS).contains(key), "{key}");
        }
        let all = schema(None).unwrap();
        for section in ["legend", "document", "node", "shapes", "materials", "limits"] {
            assert!(all.get(section).is_some(), "{section}");
            assert_eq!(schema(Some(section)).unwrap().as_object().unwrap().len(), 3);
        }
        assert_eq!(all["format"], 2);
        for words in [
            "pos", "from/to", "attach", "size", "height", "axis", "yaw", "spin", "mirror", "array", "grid", "radial", "cycle",
            "mat", "$wall",
        ] {
            assert!(LEGEND.contains(words), "the legend explains {words}");
        }
    }
}
