//! Reading materials (asset plan §6.1-6.2): strict JSON (a repeated key is an error), then a
//! check of every field against the tables below. Every problem is an `Issue` with its path
//! (`layers[1].size`), the valid values and, where there is one, a fix. Lengths are converted
//! to metres as they are read, so documents in cm and m resolve alike and `base` chains may
//! mix units.

use std::collections::BTreeMap;

use glam::Vec3;
use serde_json::{Map, Value};

use crate::issue::{self, Code, Issue, join};
use crate::params::ChoiceParam;
use crate::props::doc::{Axis, MapMode, Pattern, ROLES, Unit};
use crate::props::{amount, color_fix, is_color, look_hint, number, wrong_type};
use crate::shape::Look;

/// Layers per material (the GPU row holds three).
pub const MAX_LAYERS: usize = 3;
/// Colours in a layer's `cells`.
pub const MAX_CELLS: usize = 4;
/// The format material files declare.
pub const FORMAT: u64 = 1;

/// Material fields (files and inline materials) and their field-guide lines.
pub const FIELDS: &[(&str, &str)] = &[
    ("base", "MAT/name: start from a library material; layers given here replace its layers"),
    ("color", "#rrggbb or a role (required unless base)"),
    ("look", "flat | cel | lit | unlit; default cel"),
    ("emissive", "glow 0-32"),
    ("roughness", "0-1"),
    ("metallic", "0-1; adds a highlight"),
    ("map", "object | world | axis: how patterns are laid; default by part"),
    ("scale", "0.1-10: multiplies every layer size"),
    ("turn", "true: 2D patterns turn 90 degrees"),
    ("layers", "up to 3 pattern layers, bottom first: [{type,...}]"),
];

/// Fields only material files take.
pub const FILE_FIELDS: &[(&str, &str)] = &[
    ("format", "1 (required)"),
    ("name", "the file's name: oak_planks, pb/gold (required)"),
    ("unit", "cm | m (required); cm is the house unit"),
    ("description", "what it looks like and where it is used"),
    ("tags", "[words] for search"),
    ("source", "{library,id,url,license,author,title,sha256}: where it came from"),
    ("fit", "{metric: number}: how well a fitted texture matches (tools write it)"),
];

/// Every layer parameter, in canonical order.
pub const LAYER_KEYS: &[&str] = &[
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

/// Parameters every layer takes.
const COMMON: &[&str] = &["color", "shade", "strength", "glow", "facing", "masked_by", "seed"];
/// Parameters cellular patterns add.
const CELLULAR: &[&str] = &["vary", "cells", "cavity", "bevel"];

/// A pattern's own parameters (besides `COMMON`, and `CELLULAR` for cellular ones).
pub fn own_params(pattern: Pattern) -> &'static [&'static str] {
    use Pattern::*;
    match pattern {
        Noise => &["size", "detail", "distort", "cover", "soft"],
        Voronoi => &["size", "jitter", "feature", "cover", "gap", "axis"],
        Wave => &["size", "axis", "rings", "distort", "cover", "soft"],
        Brick => &["size", "offset", "gap", "axis"],
        Checker => &["size"],
        Gradient => &["axis", "from", "to"],
        Magic => &["size", "depth", "distort", "cover", "soft"],
        Planks | Tiles => &["size", "gap", "axis"],
        Cobbles => &["size", "jitter", "gap", "axis"],
        Stripes => &["size", "cover", "axis", "soft"],
        Dots => &["size", "radius", "offset", "jitter", "axis"],
        Shingles => &["size", "shape", "gap", "axis"],
        Wood => &["size", "axis", "distort", "cover", "soft"],
    }
}

/// Cellular patterns: each cell takes its own colour, variation and cavity.
pub fn is_cellular(pattern: Pattern) -> bool {
    use Pattern::*;
    matches!(pattern, Voronoi | Brick | Planks | Tiles | Cobbles | Dots | Shingles)
}

/// Patterns laid along one direction, which may be any vector.
fn is_linear(pattern: Pattern) -> bool {
    matches!(pattern, Pattern::Wave | Pattern::Stripes | Pattern::Gradient | Pattern::Wood)
}

/// Patterns whose size may be `[w,h]`.
fn takes_pair(pattern: Pattern) -> bool {
    use Pattern::*;
    matches!(pattern, Brick | Planks | Tiles | Shingles)
}

/// The parameters a layer of `pattern` takes, in canonical order.
pub fn layer_params(pattern: Pattern) -> Vec<&'static str> {
    let own = own_params(pattern);
    // Dots have no border to shade: no cavity or bevel.
    let cellular = |key: &str| {
        is_cellular(pattern) && CELLULAR.contains(&key) && !(pattern == Pattern::Dots && matches!(key, "cavity" | "bevel"))
    };
    LAYER_KEYS
        .iter()
        .copied()
        .filter(|key| *key == "type" || own.contains(key) || COMMON.contains(key) || cellular(key))
        .collect()
}

/// A checked parameter. Lengths are in metres.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Num(f32),
    Pair([f32; 2]),
    Int(u32),
    Flag(bool),
    Word(&'static str),
    /// `#rrggbb` or a role.
    Color(String),
    Colors(Vec<String>),
    Vector(Vec3),
}

/// A pattern layer as written: its checked parameters, without defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDoc {
    pub pattern: Pattern,
    pub params: BTreeMap<&'static str, Param>,
}

impl LayerDoc {
    pub fn num(&self, key: &str, default: f32) -> f32 {
        match self.params.get(key) {
            Some(Param::Num(x)) => *x,
            Some(Param::Int(n)) => *n as f32,
            _ => default,
        }
    }

    /// `[w,h]`; a single number is a square.
    pub fn size2(&self, default: [f32; 2]) -> [f32; 2] {
        match self.params.get("size") {
            Some(Param::Pair(pair)) => *pair,
            Some(Param::Num(x)) => [*x, *x],
            _ => default,
        }
    }

    pub fn int(&self, key: &str, default: u32) -> u32 {
        match self.params.get(key) {
            Some(Param::Int(n)) => *n,
            _ => default,
        }
    }

    pub fn word(&self, key: &str) -> Option<&'static str> {
        match self.params.get(key) {
            Some(Param::Word(word)) => Some(word),
            _ => None,
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.params.get(key), Some(Param::Flag(true)))
    }

    pub fn color(&self) -> Option<&str> {
        match self.params.get("color") {
            Some(Param::Color(color)) => Some(color),
            _ => None,
        }
    }

    pub fn cells(&self) -> &[String] {
        match self.params.get("cells") {
            Some(Param::Colors(colors)) => colors,
            _ => &[],
        }
    }

    pub fn axis(&self) -> Option<&Param> {
        self.params.get("axis")
    }
}

/// Material fields as written (lengths in metres). `None` is "not given": the base or the
/// default applies.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    /// `MAT/…`.
    pub base: Option<String>,
    pub color: Option<String>,
    pub look: Option<Look>,
    pub emissive: Option<f32>,
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    pub map: Option<MapMode>,
    pub scale: Option<f32>,
    pub turn: Option<bool>,
    pub layers: Option<Vec<LayerDoc>>,
}

/// A material file.
#[derive(Clone, Debug, PartialEq)]
pub struct MatDoc {
    /// `oak_planks`, `pb/gold`: the library name without `MAT/`.
    pub name: String,
    pub unit: Unit,
    pub description: String,
    pub tags: Vec<String>,
    pub source: Option<Map<String, Value>>,
    pub fit: Option<Map<String, Value>>,
    pub fields: Fields,
}

/// Reads a material file's text. `name` is its library name (`oak_planks`, `pb/gold`), which
/// the file's `name` must repeat.
pub fn read_file(name: &str, text: &str) -> Result<MatDoc, Vec<Issue>> {
    let value = crate::props::strict_value(text).map_err(|issue| vec![issue])?;
    read_doc(&value, Some(name))
}

/// Reads a material file's JSON.
pub fn read_doc(value: &Value, expected_name: Option<&str>) -> Result<MatDoc, Vec<Issue>> {
    let mut reader = Reader { issues: Vec::new(), unit: Unit::Cm };
    let doc = reader.file(value, expected_name);
    match doc {
        Some(doc) if reader.issues.is_empty() => Ok(doc),
        _ if reader.issues.is_empty() => Err(vec![Issue::error("", Code::Format, "not a readable material")]),
        _ => Err(reader.issues),
    }
}

/// Reads a material written inline in an asset (`materials.<key>` in a prop), whose lengths are
/// in the asset's unit. Paths start at `path`.
pub fn read_inline(value: &Value, path: &str, unit: Unit) -> Result<Fields, Vec<Issue>> {
    let mut reader = Reader { issues: Vec::new(), unit };
    let all: Vec<&str> = FIELDS.iter().map(|(key, _)| *key).collect();
    let fields = reader.object(path, value, &all, false).map(|object| reader.fields(path, object));
    match fields {
        Some(fields) if reader.issues.is_empty() => Ok(fields),
        _ => Err(reader.issues),
    }
}

struct Reader {
    issues: Vec<Issue>,
    unit: Unit,
}

impl Reader {
    fn push(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    fn object<'v>(&mut self, path: &str, value: &'v Value, allowed: &[&str], file: bool) -> Option<&'v Map<String, Value>> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, &format!("an object {{{}}}", allowed.join(",")), value));
            return None;
        };
        for key in object.keys() {
            if !allowed.contains(&key.as_str()) {
                let fix = material_hint(key, file).or_else(|| issue::suggestion(key, allowed.iter().copied()));
                let issue = Issue::error(join(path, key), Code::UnknownKey, "unknown key");
                self.push(match fix {
                    Some(fix) => issue.fix(fix),
                    None => issue.expected(allowed.join(", ")),
                });
            }
        }
        Some(object)
    }

    fn ranged(&mut self, path: &str, value: &Value, min: f32, max: f32) -> Option<f32> {
        number(path, value, min, max, &mut self.issues)
    }

    /// A length in the document's unit, returned in metres. A range error names the unit, and
    /// a length written in metres in a cm document gets the conversion.
    fn length(&mut self, path: &str, value: &Value, zero: bool) -> Option<f32> {
        let per_metre = self.unit.per_metre();
        self.measure(path, value, if zero { 0.0 } else { 0.001 * per_metre }, 1_000.0 * per_metre)
    }

    /// A length checked in the document's unit (a range error names the unit), in metres.
    fn measure(&mut self, path: &str, value: &Value, min: f32, max: f32) -> Option<f32> {
        let before = self.issues.len();
        if let Some(x) = self.ranged(path, value, min, max) {
            return Some(metres(value, x, self.unit));
        }
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
        None
    }

    fn integer(&mut self, path: &str, value: &Value, min: u32, max: u32) -> Option<u32> {
        match value.as_u64() {
            Some(n) if (min as u64..=max as u64).contains(&n) => Some(n as u32),
            _ if value.as_i64().is_some() || value.as_u64().is_some() => {
                self.push(
                    Issue::error(path, Code::OutOfRange, format!("{value} is outside {min} to {max}"))
                        .expected(format!("{min} to {max}")),
                );
                None
            }
            _ if value.is_number() => {
                self.push(Issue::error(path, Code::InvalidType, format!("expected a whole number, got {value}")));
                None
            }
            _ => {
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
    fn word(
        &mut self,
        path: &str,
        value: &Value,
        what: &str,
        names: &[&'static str],
        hint: fn(&str) -> Option<&'static str>,
    ) -> Option<&'static str> {
        let Some(text) = value.as_str() else {
            self.push(wrong_type(path, &format!("one of {}", names.join(", ")), value));
            return None;
        };
        let found = names.iter().copied().find(|name| *name == text);
        if found.is_none() {
            self.push(
                Issue::error(path, Code::UnknownValue, format!("unknown {what} \"{text}\""))
                    .expected(names.join(", "))
                    .maybe_fix(hint(text).map(String::from).or_else(|| issue::suggestion(text, names.iter().copied()))),
            );
        }
        found
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
        let issue = if text.starts_with('$') {
            Issue::error(path, Code::UnknownValue, format!("unknown role \"{text}\""))
                .expected(ROLES.join(", "))
                .maybe_fix(issue::suggestion(text, ROLES.iter().copied()))
        } else {
            Issue::error(path, Code::UnknownValue, format!("\"{text}\" is not a #rrggbb colour or a role"))
                .expected(format!("#rrggbb or {}", ROLES.join(", ")))
                .maybe_fix(color_fix(text))
        };
        self.push(issue);
        None
    }

    fn file(&mut self, value: &Value, expected_name: Option<&str>) -> Option<MatDoc> {
        let allowed: Vec<&str> = FILE_FIELDS.iter().chain(FIELDS).map(|(key, _)| *key).collect();
        let object = self.object("", value, &allowed, true)?;
        match object.get("format") {
            None => self.push(Issue::error("format", Code::Missing, "missing").fix(format!("add \"format\":{FORMAT}"))),
            Some(format) if format.as_u64() == Some(FORMAT) => {}
            Some(format) if format.as_u64().is_some_and(|format| format > FORMAT) => {
                self.push(Issue::error("format", Code::Format, format!("format {format} needs a newer engine")).expected("1"))
            }
            Some(format) => self.push(Issue::error("format", Code::Format, format!("unknown format {format}")).expected("1")),
        }
        match object.get("unit") {
            None => self.push(Issue::error("unit", Code::Missing, "missing").fix("add \"unit\":\"cm\" (the house unit)")),
            Some(value) => {
                let hint = |text: &str| match text {
                    "cms" | "centimetres" | "centimeters" | "centimetre" | "centimeter" => Some("write \"cm\""),
                    "metres" | "meters" | "metre" | "meter" => Some("write \"m\""),
                    _ => None,
                };
                if let Some(unit) = self.word("unit", value, "unit", Unit::NAMES, hint) {
                    self.unit = Unit::parse(unit).unwrap_or(Unit::Cm);
                }
            }
        }
        let name = match object.get("name") {
            None => {
                let fix =
                    expected_name.map_or("add \"name\" (the file's name)".into(), |name| format!("add \"name\":\"{name}\""));
                self.push(Issue::error("name", Code::Missing, "missing").fix(fix));
                String::new()
            }
            Some(value) => {
                let name = self.text("name", value).unwrap_or_default();
                if let Some(expected) = expected_name.filter(|expected| !name.is_empty() && *expected != name) {
                    self.push(
                        Issue::error("name", Code::Conflict, format!("\"{name}\" is not the file's name"))
                            .fix(format!("use \"name\":\"{expected}\"")),
                    );
                }
                name
            }
        };
        let description = object.get("description").and_then(|v| self.text("description", v)).unwrap_or_default();
        let tags = object
            .get("tags")
            .and_then(|value| {
                let Some(items) = value.as_array() else {
                    self.push(wrong_type("tags", "[words]", value));
                    return None;
                };
                Some(items.iter().enumerate().filter_map(|(i, v)| self.text(&format!("tags[{i}]"), v)).collect())
            })
            .unwrap_or_default();
        let source = object.get("source").and_then(|value| {
            let keys = ["library", "id", "url", "license", "author", "title", "sha256"];
            let source = self.object("source", value, &keys, true)?;
            for (key, value) in source {
                self.text(&join("source", key), value);
            }
            Some(source.clone())
        });
        let fit = object.get("fit").and_then(|value| {
            let Some(fit) = value.as_object() else {
                self.push(wrong_type("fit", "an object {metric: number}", value));
                return None;
            };
            for (key, value) in fit {
                self.ranged(&join("fit", key), value, 0.0, 1e6);
            }
            Some(fit.clone())
        });
        let fields = self.fields("", object);
        Some(MatDoc { name, unit: self.unit, description, tags, source, fit, fields })
    }

    fn fields(&mut self, path: &str, object: &Map<String, Value>) -> Fields {
        let at = |key: &str| join(path, key);
        let fields = Fields {
            base: object.get("base").and_then(|value| {
                let name = self.text(&at("base"), value)?;
                if !name.starts_with(super::PREFIX) {
                    self.push(
                        Issue::error(at("base"), Code::UnknownMaterial, format!("\"{name}\" is not a library material"))
                            .fix(format!("library names start with MAT/, e.g. \"MAT/{name}\"")),
                    );
                    return None;
                }
                Some(name)
            }),
            color: object.get("color").and_then(|v| self.color(&at("color"), v)),
            look: object.get("look").and_then(|value| {
                let names = ["flat", "cel", "lit", "unlit"];
                let word = self.word(&at("look"), value, "look", &names, look_hint)?;
                Some(Look::from_index(<Look as ChoiceParam>::NAMES.iter().position(|name| *name == word).unwrap_or(1)))
            }),
            emissive: object.get("emissive").and_then(|v| self.ranged(&at("emissive"), v, 0.0, 32.0)),
            roughness: object.get("roughness").and_then(|v| self.ranged(&at("roughness"), v, 0.0, 1.0)),
            metallic: object.get("metallic").and_then(|v| self.ranged(&at("metallic"), v, 0.0, 1.0)),
            map: object.get("map").and_then(|value| {
                let word = self.word(&at("map"), value, "map", MapMode::NAMES, |_| None)?;
                MapMode::parse(word)
            }),
            scale: object.get("scale").and_then(|v| self.ranged(&at("scale"), v, 0.1, 10.0)),
            turn: object.get("turn").and_then(|v| self.boolean(&at("turn"), v)),
            layers: object.get("layers").and_then(|value| self.layers(&at("layers"), value)),
        };
        // A misspelt color ("colour") is reported once, as the unknown key.
        let misspelt = self.issues.iter().any(|issue| {
            issue.code == Code::UnknownKey
                && issue.path.rsplit_once('.').map_or("", |(parent, _)| parent) == path
                && issue.fix.as_deref() == Some("did you mean \"color\"?")
        });
        if !object.contains_key("color") && !object.contains_key("base") && !misspelt {
            self.push(
                Issue::error(at("color"), Code::Missing, "missing").fix("a material needs color, or a base to inherit it from"),
            );
        }
        fields
    }

    fn layers(&mut self, path: &str, value: &Value) -> Option<Vec<LayerDoc>> {
        let Some(items) = value.as_array() else {
            self.push(wrong_type(path, "a list of layers [{type,...}]", value));
            return None;
        };
        if items.len() > MAX_LAYERS {
            self.push(
                Issue::error(path, Code::Limit, format!("{} layers; a material has at most {MAX_LAYERS}", items.len()))
                    .fix("merge or drop layers"),
            );
            return None;
        }
        let mut layers = Vec::new();
        for (index, value) in items.iter().enumerate() {
            let path = format!("{path}[{index}]");
            if let Some(layer) = self.layer(&path, value) {
                if index == 0 && layer.params.contains_key("masked_by") {
                    self.push(
                        Issue::error(join(&path, "masked_by"), Code::Conflict, "the first layer has no layer below it")
                            .fix("drop masked_by, or put this layer above the one that masks it"),
                    );
                }
                layers.push(layer);
            }
        }
        (layers.len() == items.len()).then_some(layers)
    }

    fn layer(&mut self, path: &str, value: &Value) -> Option<LayerDoc> {
        let Some(object) = value.as_object() else {
            self.push(wrong_type(path, "an object {type,...}", value));
            return None;
        };
        let at = |key: &str| join(path, key);
        let pattern = match object.get("type") {
            None => {
                let fix =
                    ["kind", "pattern"].iter().find(|key| object.contains_key(**key)).map(|key| format!("use type, not {key}"));
                self.push(Issue::error(at("type"), Code::Missing, "missing").expected(Pattern::NAMES.join(", ")).maybe_fix(fix));
                return None;
            }
            Some(value) => Pattern::parse(self.word(&at("type"), value, "pattern", Pattern::NAMES, pattern_hint)?)?,
        };
        let allowed = layer_params(pattern);
        for key in object.keys() {
            if !allowed.contains(&key.as_str()) {
                let fix = layer_hint(pattern, key, &allowed).or_else(|| issue::suggestion(key, allowed.iter().copied()));
                let issue = Issue::error(join(path, key), Code::UnknownKey, "unknown key");
                self.push(match fix {
                    Some(fix) => issue.fix(fix),
                    None => issue.expected(allowed[1..].join(", ")),
                });
            }
        }
        let mut params = BTreeMap::new();
        let before = self.issues.len();
        for key in allowed.iter().copied().filter(|key| *key != "type") {
            let Some(value) = object.get(key) else { continue };
            if let Some(param) = self.param(pattern, &at(key), key, value) {
                params.insert(key, param);
            }
        }
        if object.contains_key("color") && object.contains_key("shade") {
            self.push(
                Issue::error(at("shade"), Code::Conflict, "a layer paints color or darkens by shade, not both").fix("keep one"),
            );
        }
        if pattern == Pattern::Gradient {
            let (from, to) = (params.get("from").cloned(), params.get("to").cloned());
            let from = if let Some(Param::Num(x)) = from { x } else { 0.0 };
            let to = if let Some(Param::Num(x)) = to { x } else { 1.0 };
            if from == to {
                self.push(Issue::error(at("to"), Code::Conflict, "from and to are equal").fix("give the gradient a length"));
            }
        }
        (self.issues.len() == before).then_some(LayerDoc { pattern, params })
    }

    fn param(&mut self, pattern: Pattern, path: &str, key: &'static str, value: &Value) -> Option<Param> {
        use Pattern::*;
        let per_metre = self.unit.per_metre();
        let reach = 10_000.0 * per_metre;
        Some(match key {
            "size" if takes_pair(pattern) && value.is_array() => {
                let items = value.as_array()?;
                if items.len() != 2 {
                    self.push(Issue::error(path, Code::InvalidType, format!("expected 2 sizes [w,h], got {}", items.len())));
                    return None;
                }
                let w = self.length(&format!("{path}[0]"), &items[0], false);
                let h = self.length(&format!("{path}[1]"), &items[1], false);
                Param::Pair([w?, h?])
            }
            "from" | "to" => Param::Num(self.measure(path, value, -reach, reach)?),
            "size" => Param::Num(self.length(path, value, false)?),
            "radius" | "gap" | "bevel" => Param::Num(self.length(path, value, true)?),
            "detail" | "depth" => Param::Int(self.integer(path, value, 1, 4)?),
            "seed" => Param::Int(self.integer(path, value, 0, u32::MAX)?),
            "distort" => Param::Num(self.ranged(path, value, 0.0, 10.0)?),
            "shade" => Param::Num(self.ranged(path, value, 0.0, 2.0)?),
            "glow" => Param::Num(self.ranged(path, value, 0.0, 8.0)?),
            // Wood's rings are a smooth step, which needs a width.
            "soft" if pattern == Wood => Param::Num(self.ranged(path, value, 0.01, 1.0)?),
            "rings" => Param::Flag(self.boolean(path, value)?),
            "color" => Param::Color(self.color(path, value)?),
            "cells" => {
                let Some(items) = value.as_array() else {
                    self.push(wrong_type(path, "2-4 colours", value));
                    return None;
                };
                if !(2..=MAX_CELLS).contains(&items.len()) {
                    let fix = if items.len() < 2 { "one colour: use color" } else { "keep at most 4" };
                    self.push(
                        Issue::error(path, Code::Limit, format!("{} cell colours; cells takes 2-{MAX_CELLS}", items.len()))
                            .fix(fix),
                    );
                    return None;
                }
                let colors: Vec<Option<String>> =
                    items.iter().enumerate().map(|(i, v)| self.color(&format!("{path}[{i}]"), v)).collect();
                Param::Colors(colors.into_iter().collect::<Option<Vec<_>>>()?)
            }
            "feature" => Param::Word(self.word(path, value, "feature", &["spots", "edges", "cells"], |_| None)?),
            "shape" => Param::Word(self.word(path, value, "shape", &["round", "square", "point"], |_| None)?),
            "facing" => Param::Word(self.word(path, value, "facing", &["up", "down", "side"], facing_hint)?),
            "masked_by" => Param::Word(self.word(path, value, "mask", &["prev", "not_prev"], |_| None)?),
            "axis" if is_linear(pattern) && value.is_array() => {
                let items = value.as_array()?;
                if items.len() != 3 {
                    self.push(Issue::error(path, Code::InvalidType, format!("expected 3 numbers [x,y,z], got {}", items.len())));
                    return None;
                }
                let v: Vec<Option<f32>> =
                    items.iter().enumerate().map(|(i, v)| self.ranged(&format!("{path}[{i}]"), v, -1.0, 1.0)).collect();
                let v = Vec3::from_slice(&v.into_iter().collect::<Option<Vec<_>>>()?);
                if v.length() < 1e-3 {
                    self.push(
                        Issue::error(path, Code::OutOfRange, "the axis has no direction").fix("e.g. [1,1,0] for a diagonal"),
                    );
                    return None;
                }
                Param::Vector(v)
            }
            "axis" => {
                let what = if is_linear(pattern) { "axis (x, y, z or [x,y,z])" } else { "axis" };
                Param::Word(self.word(path, value, what, Axis::NAMES, axis_hint)?)
            }
            // offset, jitter, vary, cavity, cover, soft, strength: fractions.
            _ => Param::Num(self.ranged(path, value, 0.0, 1.0)?),
        })
    }
}

/// A length read in `unit`, in metres. Converted from the JSON's f64, so 120 cm and 1.2 m give
/// the same f32.
fn metres(value: &Value, checked: f32, unit: Unit) -> f32 {
    value.as_f64().map_or(checked, |x| (x / unit.per_metre() as f64) as f32)
}

/// Fixes for keys models tend to guess at the material level.
fn material_hint(key: &str, file: bool) -> Option<String> {
    Some(match key {
        "texture" | "pattern" | "patterns" | "layer" => "use layers:[{type,...}]".into(),
        "inherit" | "extends" | "parent" | "material" | "mat" => "use base (a library material, MAT/…)".into(),
        "metalness" | "metal" => "use metallic (0-1)".into(),
        "rough" | "gloss" | "glossiness" | "shininess" => "use roughness (0-1)".into(),
        "emission" | "glow" => "use emissive (0-32); a layer's glow lights only where it paints".into(),
        "type" | "size" | "shade" | "cover" => format!("{key} belongs in a layer: layers:[{{type,...}}]"),
        "format" | "name" | "unit" | "description" | "tags" | "source" | "fit" if !file => {
            "only material files take this; an inline material uses the asset's unit".into()
        }
        _ => return None,
    })
}

/// Fixes for keys models tend to guess in a layer.
fn layer_hint(pattern: Pattern, key: &str, allowed: &[&str]) -> Option<String> {
    let own = || allowed[1..].join(", ");
    Some(match key {
        "kind" | "pattern" => "use type".into(),
        "scale" | "turn" | "map" => format!("{key} is the material's, not a layer's"),
        "opacity" | "alpha" | "amount" | "mix" | "intensity" => "use strength (0-1)".into(),
        "density" | "coverage" => "use cover (0-1)".into(),
        "darken" | "darkness" | "dark" | "tint" => "use shade (multiplies brightness; 0.6 darkens)".into(),
        "emissive" | "emission" => "use glow (0-8)".into(),
        "angle" | "rotation" | "rotate" => "angle is not available yet: use axis, or turn:true on the material".into(),
        "colors" | "colours" | "palette" if is_cellular(pattern) => "use cells (2-4 colours, one per cell)".into(),
        "colors" | "colours" | "palette" | "cells" => format!("a {} has no cells; use color", pattern.name()),
        "octaves" => "use detail (1-4)".into(),
        "width" | "height" | "length" => format!("use size{}", if takes_pair(pattern) { " [w,h]" } else { "" }),
        _ if LAYER_KEYS.contains(&key) => {
            let takers: Vec<&str> = Pattern::NAMES
                .iter()
                .copied()
                .filter(|name| Pattern::parse(name).is_some_and(|p| layer_params(p).contains(&key)))
                .collect();
            format!("{key} is for {}; a {} takes {}", takers.join(", "), pattern.name(), own())
        }
        _ => return None,
    })
}

fn pattern_hint(text: &str) -> Option<&'static str> {
    Some(match text {
        "perlin" | "fbm" | "clouds" | "simplex" | "musgrave" => "use noise",
        "worley" | "cell" | "cells" | "cellular" => "use voronoi",
        "bricks" | "masonry" => "use brick",
        "grid" | "tile" => "use tiles",
        "plank" | "boards" | "floorboards" => "use planks",
        "stones" | "cobblestone" | "flagstones" | "paving" => "use cobbles (flagstones: size 60-80 cm)",
        "stripe" | "bands" | "lines" => "use stripes",
        "rings" | "waves" => "use wave",
        "grain" | "wood_grain" => "use wood",
        "scales" | "roof" | "tiles_roof" => "use shingles (scales: size [8,6] cm)",
        "polka" | "spots" | "dot" => "use dots",
        "checkerboard" | "checkers" | "check" => "use checker",
        "ramp" | "fade" => "use gradient",
        _ => return None,
    })
}

fn facing_hint(text: &str) -> Option<&'static str> {
    Some(match text {
        "top" | "+y" | "y" => "use up",
        "bottom" | "-y" => "use down",
        "sides" | "wall" | "walls" => "use side",
        _ => return None,
    })
}

fn axis_hint(text: &str) -> Option<&'static str> {
    Some(match text {
        "horizontal" | "+x" | "-x" => "write \"x\"",
        "vertical" | "up" | "+y" | "-y" => "write \"y\"",
        "+z" | "-z" | "depth" => "write \"z\"",
        _ => return None,
    })
}
