//! Reading a prop: strict JSON (a repeated key is an error, not a silent overwrite), then a
//! check of the whole document against the field tables below, which reports every problem
//! with its path, the valid values and a fix. The typed model is built only from a document
//! that passed. `schema` renders the same tables as the field guide.

pub mod v2;

use std::cell::RefCell;

use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value, json};

use super::{LEGEND, MAX_PARTS, MAX_POSITION, MAX_SIZE, MIN_SIZE, validate_name, validate_part_name};
use crate::color::Color;
use crate::issue::{self, Code, Issue, join};

/// Document fields and their field-guide lines.
pub const DOCUMENT_FIELDS: &[(&str, &str)] = &[
    ("format", "1 (required)"),
    ("name", "lowercase id, the file name (required)"),
    ("description", "what it is and how it stands; at most 4096 bytes"),
    ("parts", "{part_id: part}, 1-256 (required)"),
];

/// Part fields and their field-guide lines (defaults included).
pub const PART_FIELDS: &[(&str, &str)] = &[
    ("shape", "{type,...} (required): see shapes"),
    ("pos", "[x,y,z] metres, the part's centre; default [0,0,0]"),
    ("yaw", "degrees about y; default 0"),
    ("pitch", "degrees about x; default 0"),
    ("roll", "degrees about z; default 0"),
    ("color", "#rrggbb; default #e8704a"),
    ("look", "flat | cel | lit | unlit | cutout (drawn flat); default cel"),
    ("emissive", "glow 0-32; above 0.5 also lights its surroundings; default 0"),
    ("solid", "part of the collider; default true"),
];

/// Shape types: the fields each needs, and the field-guide line.
pub const SHAPES: &[(&str, &[&str], &str)] = &[
    ("box", &["half"], "half:[x,y,z] half extents (a 1 m cube is half [0.5,0.5,0.5])"),
    ("rounded_box", &["half", "radius"], "half:[x,y,z], radius 0 to the smallest half"),
    ("sphere", &["radius"], "radius"),
    ("capsule", &["radius", "half_height"], "radius, half_height along y; total height 2*(half_height+radius)"),
    ("cylinder", &["radius", "half_height"], "radius, half_height along y; height 2*half_height"),
];

pub const LOOKS: &[&str] = &["flat", "cel", "lit", "unlit", "cutout"];

/// Patch operations: their fields, and the field-guide line.
pub const OPS: &[(&str, &[&str], &str)] = &[
    ("set", &["part", "fields"], "{op,part,fields}: merge fields into the part; a shape is replaced whole"),
    ("add", &["part", "value"], "{op,part,value}: a new part"),
    ("remove", &["part"], "{op,part}: delete the part"),
];

const LIMITS: &str = "1-256 parts; names [a-z][a-z0-9_-]{0,63}; sizes 0.001-1000 m; pos within 10000 m; \
angles within 36000 degrees; 1024 ops per batch";

/// The field guide for `asset_edit action=schema`. Topics: legend, document, part, shapes, ops,
/// limits; none gives all of them.
pub fn schema(topic: Option<&str>) -> Result<Value, String> {
    let sections = vec![
        ("legend", json!(LEGEND)),
        ("document", pairs(DOCUMENT_FIELDS)),
        ("part", pairs(PART_FIELDS)),
        ("shapes", triples(SHAPES)),
        ("ops", triples(OPS)),
        ("limits", json!(LIMITS)),
    ];
    guide(1, sections, topic, "asset_edit action=copy from=BUILTIN/bench name=x, then one patch batch")
}

fn pairs(rows: &[(&str, &str)]) -> Value {
    Value::Object(rows.iter().map(|(k, v)| ((*k).into(), json!(v))).collect())
}

fn triples(rows: &[(&str, &[&str], &str)]) -> Value {
    Value::Object(rows.iter().map(|(k, _, v)| ((*k).into(), json!(v))).collect())
}

/// A field guide: every section, or the one `topic` names.
fn guide(format: u32, sections: Vec<(&str, Value)>, topic: Option<&str>, next: &str) -> Result<Value, String> {
    let mut out = json!({"kind":"prop","format":format});
    match topic {
        None => {
            for (key, value) in sections {
                out[key] = value;
            }
            out["next"] = json!(next);
        }
        Some(topic) => {
            let names: Vec<&str> = sections.iter().map(|(key, _)| *key).collect();
            let (key, value) = sections.into_iter().find(|(key, _)| *key == topic).ok_or_else(|| {
                Issue::error("topic", Code::UnknownValue, format!("unknown topic \"{topic}\""))
                    .expected(names.join(", "))
                    .maybe_fix(issue::suggestion(topic, names.iter().copied()))
                    .to_string()
            })?;
            out[key] = value;
        }
    }
    Ok(out)
}

// --- Formats -------------------------------------------------------------------------------

/// The format a document is read as: 2, or 1 (which includes a missing or malformed format,
/// reported by the format-1 check). A newer format is refused here. Format 1 needs no rewrite
/// to be read as format 2: the typed reader maps its nested shapes and implicit metres.
pub fn migrate(doc: &Value) -> Result<u32, Issue> {
    match doc.get("format") {
        Some(format) if format.as_u64() == Some(2) => Ok(2),
        Some(format) if format.as_u64().is_some_and(|format| format > 2) => {
            Err(Issue::error("format", Code::Format, format!("format {format} needs a newer engine")).expected("1 or 2"))
        }
        _ => Ok(1),
    }
}

// --- Strict JSON ---------------------------------------------------------------------------

/// Reads JSON text. serde_json keeps the last of two equal keys silently; this reports it.
pub fn strict_value(text: &str) -> Result<Value, Issue> {
    let duplicate = RefCell::new(None);
    let mut reader = serde_json::Deserializer::from_str(text);
    let read = Strict { path: String::new(), duplicate: &duplicate }.deserialize(&mut reader).and_then(|value| {
        reader.end()?;
        Ok(value)
    });
    read.map_err(|error| match duplicate.into_inner() {
        Some((path, key)) => {
            Issue::error(path, Code::Duplicate, format!("\"{key}\" appears twice")).fix("keep one; names must be unique")
        }
        None => Issue::error("", Code::Format, format!("not valid JSON: {error}")),
    })
}

struct Strict<'a> {
    path: String,
    duplicate: &'a RefCell<Option<(String, String)>>,
}

impl<'de> DeserializeSeed<'de> for Strict<'_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("JSON")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.into()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) =
            seq.next_element_seed(Strict { path: format!("{}[{}]", self.path, items.len()), duplicate: self.duplicate })?
        {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let path = join(&self.path, &key);
            let value = map.next_value_seed(Strict { path: path.clone(), duplicate: self.duplicate })?;
            if object.insert(key.clone(), value).is_some() {
                *self.duplicate.borrow_mut() = Some((path, key));
                return Err(A::Error::custom("duplicate key"));
            }
        }
        Ok(Value::Object(object))
    }
}

// --- The format-1 check ----------------------------------------------------------------------

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true/false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// `expected a number, got the string "45" — write 45 without quotes`.
fn wrong_type(path: &str, expected: &str, value: &Value) -> Issue {
    let got = match value {
        Value::String(text) => format!("the string \"{text}\""),
        other => kind_of(other).into(),
    };
    let issue = Issue::error(path, Code::InvalidType, format!("expected {expected}, got {got}"));
    match value.as_str().and_then(|text| text.trim().parse::<f64>().ok()) {
        Some(number) if expected.contains("number") => issue.fix(format!("write {number} without quotes")),
        _ => issue,
    }
}

fn range_text(min: f32, max: f32) -> String {
    format!("{min} to {max}")
}

/// Ranges compare in f32, as the engine stores the value.
fn number(path: &str, value: &Value, min: f32, max: f32, issues: &mut Vec<Issue>) -> Option<f32> {
    let Some(x) = value.as_f64().map(|x| x as f32) else {
        issues.push(wrong_type(path, "a number", value));
        return None;
    };
    if !(min..=max).contains(&x) {
        issues.push(
            Issue::error(path, Code::OutOfRange, format!("{value} is outside {}", range_text(min, max)))
                .expected(range_text(min, max)),
        );
        return None;
    }
    Some(x)
}

fn vec3(path: &str, value: &Value, min: f32, max: f32, issues: &mut Vec<Issue>) -> Option<[f32; 3]> {
    let Some(items) = value.as_array() else {
        issues.push(wrong_type(path, "3 numbers [x,y,z]", value));
        return None;
    };
    if items.len() != 3 {
        issues.push(
            Issue::error(path, Code::InvalidType, format!("expected 3 numbers [x,y,z], got {}", items.len())).expected("[x,y,z]"),
        );
        return None;
    }
    let mut out = [0.0; 3];
    let mut ok = true;
    for (index, item) in items.iter().enumerate() {
        match number(&format!("{path}[{index}]"), item, min, max, issues) {
            Some(x) => out[index] = x,
            None => ok = false,
        }
    }
    ok.then_some(out)
}

/// A JSON number written for a fix: shortest, no exponent for ordinary sizes.
fn amount(x: f64) -> String {
    let x = (x * 1e6).round() / 1e6;
    if x == x.trunc() && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") }
}

fn halves(value: &Value) -> Option<String> {
    let items = value.as_array()?;
    let halves: Option<Vec<String>> = items.iter().map(|v| v.as_f64().map(|x| amount(x / 2.0))).collect();
    Some(format!("[{}]", halves?.join(",")))
}

/// Fixes for keys models tend to guess, at each level of a format-1 document.
fn key_hint(level: &str, key: &str, value: &Value) -> Option<String> {
    let half = |what: &str| match value.as_f64() {
        Some(x) => format!("use \"half_height\":{} ({what} = 2*half_height)", amount(x / 2.0)),
        None => format!("use half_height ({what} = 2*half_height)"),
    };
    Some(match (level, key) {
        (_, "colour") => "spelled \"color\"".into(),
        ("part", "rotation" | "rot" | "rotate" | "angle" | "angles" | "orientation") => {
            "use yaw, pitch and roll in degrees".into()
        }
        ("part", "position" | "location" | "offset" | "translation" | "center" | "centre") => "use pos:[x,y,z]".into(),
        ("part", "scale") => "format 1 parts have no scale: change the shape's half or radius".into(),
        ("part", "size" | "dimensions" | "half_extents" | "extents") => {
            "sizes live in shape: a box takes \"shape\":{\"type\":\"box\",\"half\":[x,y,z]}".into()
        }
        ("part", "type" | "kind") => "the type goes inside shape: \"shape\":{\"type\":\"box\",\"half\":[...]}".into(),
        ("part", "half" | "radius" | "half_height") => format!("{key} goes inside shape"),
        ("part", "material" | "mat" | "texture") => "format 1 has no materials: use color, look and emissive".into(),
        ("part", "children" | "parts") => "format 1 parts are flat: give each part its own name in parts".into(),
        ("part", "collide" | "collision" | "collider" | "collidable") => "use solid (true: part of the collider)".into(),
        ("part", "glow" | "emission" | "light") => "use emissive (0-32)".into(),
        ("part", "name" | "id") => "a part's name is its key in parts".into(),
        ("shape", "size" | "dimensions" | "half_extents" | "extents") => match halves(value) {
            Some(halves) => format!("box sizes are half extents: \"half\":{halves}"),
            None => "box sizes are half extents: \"half\":[x/2,y/2,z/2]".into(),
        },
        ("shape", "height") => half("height"),
        ("shape", "length") => half("length"),
        ("shape", "diameter") => match value.as_f64() {
            Some(x) => format!("use \"radius\":{}", amount(x / 2.0)),
            None => "use radius (half the diameter)".into(),
        },
        ("doc", "version") => "use \"format\":1".into(),
        ("doc", "objects" | "children" | "shapes" | "components") => "parts go in \"parts\":{part_id:{...}}".into(),
        ("doc", "title" | "label") => "use description".into(),
        _ => return None,
    })
}

fn shape_type_hint(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "cube" | "cuboid" | "rect" | "block" => "use box",
        "ball" | "orb" => "use sphere",
        "cone" | "frustum" | "pyramid" => "cone needs format 2; in format 1 use a cylinder",
        "tube" | "ring" | "torus" | "pipe" => "tubes and rings need format 2; in format 1 use cylinders",
        "wedge" | "ramp" | "gable" | "prism" => "wedges need format 2; in format 1 use a rotated box",
        "lathe" | "extrude" | "mesh" => "format 1 has only box, rounded_box, sphere, capsule and cylinder",
        "pill" => "use capsule",
        _ => return None,
    })
}

fn look_hint(look: &str) -> Option<&'static str> {
    Some(match look {
        "toon" | "cartoon" | "cel_shaded" | "celshaded" => "cel is the toon look",
        "shaded" | "pbr" | "phong" | "smooth" | "lambert" => "use lit",
        "emissive" | "glow" | "bright" => "use emissive for glow, with look unlit or lit",
        "solid" | "plain" | "matte" => "use flat",
        _ => return None,
    })
}

/// A hex colour for a colour name or a short `#rgb`.
fn color_fix(text: &str) -> Option<String> {
    let named = match text.trim().to_ascii_lowercase().as_str() {
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "white" => "#ffffff",
        "black" => "#000000",
        "grey" | "gray" => "#808080",
        "yellow" => "#ffff00",
        "orange" => "#ffa500",
        "brown" => "#a52a2a",
        "purple" => "#800080",
        "pink" => "#ffc0cb",
        "cyan" => "#00ffff",
        "teal" => "#008080",
        "gold" => "#ffd700",
        "silver" => "#c0c0c0",
        _ => "",
    };
    if !named.is_empty() {
        return Some(format!("use a hex colour such as \"{named}\""));
    }
    let digits = text.trim().trim_start_matches('#');
    if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        let long: String = digits.chars().flat_map(|c| [c, c]).collect();
        return Some(format!("write all six digits: \"#{}\"", long.to_ascii_lowercase()));
    }
    None
}

fn is_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit()) && Color::try_hex(text).is_some()
}

fn unknown_key(path: &str, key: &str, allowed: &[&str], hint: Option<String>) -> Issue {
    let fix = hint.or_else(|| issue::suggestion(key, allowed.iter().copied()));
    Issue::error(join(path, key), Code::UnknownKey, "unknown key").expected(allowed.join(", ")).maybe_fix(fix)
}

fn keys(rows: &'static [(&'static str, &'static str)]) -> Vec<&'static str> {
    rows.iter().map(|(key, _)| *key).collect()
}

/// Check a format-1 document. Every problem is reported, each with its path.
pub fn check(doc: &Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    let Some(object) = doc.as_object() else {
        issues.push(wrong_type("", "an object {format,name,parts}", doc));
        return issues;
    };
    let allowed = keys(DOCUMENT_FIELDS);
    for (key, value) in object {
        if !allowed.contains(&key.as_str()) {
            issues.push(unknown_key("", key, &allowed, key_hint("doc", key, value)));
        }
    }
    match object.get("format") {
        None => issues.push(Issue::error("format", Code::Missing, "missing").fix("add \"format\":1")),
        Some(value) if value.as_u64() == Some(1) => {}
        Some(value) if value.as_u64().is_some_and(|format| format > 2) => {
            issues.push(Issue::error("format", Code::Format, format!("format {value} needs a newer engine")).expected("1 or 2"))
        }
        Some(value) => issues.push(Issue::error("format", Code::UnknownValue, format!("{value} is not a format")).expected("1")),
    }
    match object.get("name") {
        None => issues.push(Issue::error("name", Code::Missing, "missing")),
        Some(Value::String(name)) => {
            if let Err(error) = validate_name(name) {
                issues.push(Issue::error("name", Code::UnknownValue, format!("\"{name}\" is not a usable name: {error}")));
            }
        }
        Some(value) => issues.push(wrong_type("name", "a lowercase name", value)),
    }
    match object.get("description") {
        None => {}
        Some(Value::String(text)) if text.len() > 4096 => issues
            .push(Issue::error("description", Code::Limit, format!("{} bytes is too long", text.len())).expected("at most 4096")),
        Some(Value::String(_)) => {}
        Some(value) => issues.push(wrong_type("description", "text", value)),
    }
    match object.get("parts") {
        None => issues.push(Issue::error("parts", Code::Missing, "missing").fix("add \"parts\":{part_id:{shape,...}}")),
        Some(Value::Object(parts)) => {
            if parts.is_empty() || parts.len() > MAX_PARTS {
                issues.push(
                    Issue::error("parts", Code::Limit, format!("{} parts", parts.len())).expected(format!("1 to {MAX_PARTS}")),
                );
            }
            for (name, part) in parts {
                let path = format!("parts.{name}");
                if let Err(error) = validate_part_name(name) {
                    issues.push(Issue::error(&path, Code::UnknownValue, format!("\"{name}\" is not a part name: {error}")));
                }
                check_part(&path, part, &mut issues);
            }
        }
        Some(value) => issues.push(wrong_type("parts", "an object {part_id: part}", value)),
    }
    issues
}

/// Check one part at `path`.
pub fn check_part(path: &str, part: &Value, issues: &mut Vec<Issue>) {
    let Some(object) = part.as_object() else {
        issues.push(wrong_type(path, "an object {shape,...}", part));
        return;
    };
    let allowed = keys(PART_FIELDS);
    for (key, value) in object {
        if !allowed.contains(&key.as_str()) {
            issues.push(unknown_key(path, key, &allowed, key_hint("part", key, value)));
        }
    }
    let at = |key: &str| join(path, key);
    match object.get("shape") {
        None => issues.push(
            Issue::error(at("shape"), Code::Missing, "missing").fix("add \"shape\":{\"type\":\"box\",\"half\":[0.5,0.5,0.5]}"),
        ),
        Some(shape) => check_shape(&at("shape"), shape, issues),
    }
    if let Some(pos) = object.get("pos") {
        vec3(&at("pos"), pos, -MAX_POSITION, MAX_POSITION, issues);
    }
    for key in ["yaw", "pitch", "roll"] {
        if let Some(angle) = object.get(key) {
            number(&at(key), angle, -36_000.0, 36_000.0, issues);
        }
    }
    match object.get("color") {
        None => {}
        Some(Value::String(color)) if is_color(color) => {}
        Some(Value::String(color)) => issues.push(
            Issue::error(at("color"), Code::UnknownValue, format!("\"{color}\" is not a #rrggbb colour"))
                .expected("#rrggbb")
                .maybe_fix(color_fix(color)),
        ),
        Some(value) => issues.push(wrong_type(&at("color"), "\"#rrggbb\"", value)),
    }
    match object.get("look") {
        None => {}
        Some(Value::String(look)) if LOOKS.contains(&look.as_str()) => {}
        Some(Value::String(look)) => issues.push(
            Issue::error(at("look"), Code::UnknownValue, format!("unknown look \"{look}\""))
                .expected(LOOKS.join(", "))
                .maybe_fix(look_hint(look).map(String::from).or_else(|| issue::suggestion(look, LOOKS.iter().copied()))),
        ),
        Some(value) => issues.push(wrong_type(&at("look"), "one of flat, cel, lit, unlit, cutout", value)),
    }
    if let Some(emissive) = object.get("emissive") {
        number(&at("emissive"), emissive, 0.0, 32.0, issues);
    }
    if let Some(solid) = object.get("solid").filter(|solid| !solid.is_boolean()) {
        issues.push(wrong_type(&at("solid"), "true or false", solid));
    }
}

fn check_shape(path: &str, shape: &Value, issues: &mut Vec<Issue>) {
    let types: Vec<&str> = SHAPES.iter().map(|(kind, _, _)| *kind).collect();
    let Some(object) = shape.as_object() else {
        issues.push(wrong_type(path, "an object {type,...}", shape));
        return;
    };
    let kind = match object.get("type") {
        None => {
            issues.push(
                Issue::error(join(path, "type"), Code::Missing, "missing")
                    .expected(types.join(", "))
                    .fix("give the type and every field, e.g. {\"type\":\"box\",\"half\":[0.5,0.5,0.5]}"),
            );
            return;
        }
        Some(Value::String(kind)) => kind.as_str(),
        Some(value) => {
            issues.push(wrong_type(&join(path, "type"), "a shape type", value));
            return;
        }
    };
    let Some((_, fields, _)) = SHAPES.iter().find(|(name, _, _)| *name == kind) else {
        issues.push(
            Issue::error(join(path, "type"), Code::UnknownValue, format!("unknown shape type \"{kind}\""))
                .expected(types.join(", "))
                .maybe_fix(shape_type_hint(kind).map(String::from).or_else(|| issue::suggestion(kind, types.iter().copied()))),
        );
        return;
    };
    let allowed: Vec<&str> = std::iter::once("type").chain(fields.iter().copied()).collect();
    // A rename hint that already writes a missing field ("height" → "half_height":0.5) is the
    // whole fix; a second "missing" line would repeat it.
    let mut hinted = Vec::new();
    for (key, value) in object {
        if !allowed.contains(&key.as_str()) {
            let issue = unknown_key(path, key, &allowed, key_hint("shape", key, value));
            hinted.extend(
                fields.iter().filter(|field| issue.fix.as_ref().is_some_and(|fix| fix.contains(&format!("\"{field}\"")))),
            );
            issues.push(issue);
        }
    }
    let at = |key: &str| join(path, key);
    for field in *fields {
        if !object.contains_key(*field) && !hinted.contains(&field) {
            issues.push(
                Issue::error(at(field), Code::Missing, "missing").expected(format!("{kind} needs {}", fields.join(" and "))),
            );
        }
    }
    let half = object.get("half").and_then(|half| vec3(&at("half"), half, MIN_SIZE, MAX_SIZE, issues));
    if let Some(radius) = object.get("radius") {
        if kind == "rounded_box" {
            let smallest = half.map(|h| h.into_iter().fold(f32::INFINITY, f32::min)).unwrap_or(MAX_SIZE);
            number(&at("radius"), radius, 0.0, smallest, issues);
        } else {
            number(&at("radius"), radius, MIN_SIZE, MAX_SIZE, issues);
        }
    }
    if let Some(half_height) = object.get("half_height") {
        let min = if kind == "capsule" { 0.0 } else { MIN_SIZE };
        number(&at("half_height"), half_height, min, MAX_SIZE, issues);
    }
}
