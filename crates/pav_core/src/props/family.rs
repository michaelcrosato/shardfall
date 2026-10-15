//! Parametric families (format-2 `params` and `presets`, plan §9.4): knobs that set fields at
//! target paths, with no expressions. The file's own values are the defaults, so a family is
//! also a concrete asset; `apply_params` runs after the check and before the compile, and a
//! placed copy names its knob values (`params`) or a preset (`NAME:preset`).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use super::doc::{Doc, Param};
use super::{CompiledProp, PropDef, canonical, compile, fnv, read, resolve_ref, validate_part_name};
use crate::issue::{self, Code, Issue};

/// Knob values: a number for a range, a choice's name for choices.
pub type Params = BTreeMap<String, Value>;

pub const MAX_PARAMS: usize = 8;
pub const MAX_TARGETS: usize = 8;
pub const MAX_CHOICES: usize = 8;
pub const MAX_PRESETS: usize = 16;

/// Knob variants a definition keeps.
const VARIANTS: usize = 256;

/// One knob target in a document: where it is and what the file writes there.
pub(crate) struct Target {
    /// Keys and indices from the document root: `post.pos.0` is `parts/post/pos/0`.
    tokens: Vec<String>,
    /// The value written there (None: not written).
    pub literal: Option<Value>,
}

/// A target path in `doc`: a part (group children by dotted name) or `materials.<key>`, then
/// field keys and list indices (`roof.ridge.size.1`, `rail.array.span.0`). The issue has no
/// path; the caller sets it.
pub(crate) fn target(doc: &Value, path: &str) -> Result<Target, Issue> {
    let fail = |code, message: String| Issue::error("", code, message);
    let segments: Vec<&str> = path.split('.').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(fail(Code::UnknownValue, format!("\"{path}\" has an empty name")));
    }
    let (mut tokens, mut node, rest, part) = if segments[0] == "materials" {
        let materials = doc.get("materials").and_then(Value::as_object);
        let key = segments.get(1).copied().unwrap_or("");
        let Some(entry) = materials.and_then(|m| m.get(key)) else {
            return Err(fail(Code::UnknownMaterial, format!("no material \"{key}\""))
                .maybe_fix(materials.and_then(|m| issue::suggestion(key, m.keys().map(String::as_str)))));
        };
        if !entry.is_object() {
            return Err(fail(Code::InvalidType, format!("material \"{key}\" is a library name"))
                .fix(format!("write it as {{\"base\":{entry}}} to set its fields")));
        }
        (vec!["materials".to_string(), key.to_string()], entry, &segments[2.min(segments.len())..], false)
    } else {
        let parts = doc.get("parts").and_then(Value::as_object);
        let Some(mut node) = parts.and_then(|p| p.get(segments[0])) else {
            return Err(fail(Code::UnknownPart, format!("no part \"{}\"", segments[0]))
                .maybe_fix(parts.and_then(|p| issue::suggestion(segments[0], p.keys().map(String::as_str)))));
        };
        let mut tokens = vec!["parts".to_string(), segments[0].to_string()];
        let mut at = 1;
        while at + 1 < segments.len() {
            let Some(child) = node.get("parts").and_then(|children| children.get(segments[at])) else { break };
            tokens.extend(["parts".to_string(), segments[at].to_string()]);
            node = child;
            at += 1;
        }
        (tokens, node, &segments[at..], true)
    };
    let Some((last, walk)) = rest.split_last() else {
        return Err(fail(Code::Missing, format!("\"{path}\" names a {}, not a field", if part { "part" } else { "material" }))
            .fix("add the field, e.g. .pos.0, .size.1 or .color"));
    };
    if part && (matches!(rest[0], "type" | "ref" | "mesh" | "parts") || rest == ["attach"] || rest.starts_with(&["attach", "to"]))
    {
        return Err(fail(Code::Conflict, format!("a family cannot change {}", rest[..rest.len().min(2)].join(".")))
            .fix("knobs set sizes, positions, counts, chance and surfaces; attach, mirror, array.span and chance place what depends on them"));
    }
    fn step<'v>(node: &'v Value, key: &str) -> Option<&'v Value> {
        match node {
            Value::Object(map) => map.get(key),
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        }
    }
    for (depth, key) in walk.iter().enumerate() {
        node = step(node, key).ok_or_else(|| {
            fail(Code::Missing, format!("{} is not written", walk[..=depth].join(".")))
                .fix("write it in the file first: its value is the default")
        })?;
        tokens.push(key.to_string());
    }
    let literal = match node {
        Value::Object(map) => map.get(*last).cloned(),
        Value::Array(items) => Some(step(node, last).cloned().ok_or_else(|| {
            fail(Code::OutOfRange, format!("{path}: no entry {last}")).expected(format!("0 to {}", items.len().saturating_sub(1)))
        })?),
        _ => return Err(fail(Code::InvalidType, format!("{} is not an object or a list", walk.join(".")))),
    };
    tokens.push(last.to_string());
    Ok(Target { tokens, literal })
}

fn write(doc: &mut Value, tokens: &[String], value: Value) {
    let Some((last, parents)) = tokens.split_last() else { return };
    let mut node = doc;
    for key in parents {
        let next = match node {
            Value::Object(map) => map.get_mut(key),
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
            _ => None,
        };
        let Some(next) = next else { return };
        node = next;
    }
    match node {
        Value::Object(map) => {
            map.insert(last.clone(), value);
        }
        Value::Array(items) => {
            if let Some(slot) = last.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                *slot = value;
            }
        }
        _ => {}
    }
}

/// Fields that take whole numbers: a knob's product is rounded there.
fn whole_field(path: &str) -> bool {
    path.rsplit('.').find(|key| key.parse::<usize>().is_err()).is_some_and(|key| matches!(key, "count" | "pick" | "segments"))
}

/// A number as a knob writes it: whole numbers as integers.
pub(crate) fn number_value(x: f64, whole: bool) -> Value {
    let x = if whole { x.round() } else { x };
    if x.fract() == 0.0 && x.abs() < 1e15 { Value::from(x as i64) } else { Value::from(x) }
}

fn number_of(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok())).filter(|x: &f64| x.is_finite())
}

/// `doc` with the named knobs set and `params`/`presets` removed. `values` are checked ones.
pub(crate) fn apply_value(doc: &Value, params: &BTreeMap<String, Param>, values: &Params) -> Result<Value, String> {
    let mut out = doc.clone();
    if let Some(map) = out.as_object_mut() {
        map.remove("params");
        map.remove("presets");
    }
    for (name, value) in values {
        let param = params.get(name).ok_or_else(|| format!("no param \"{name}\""))?;
        let writes: Vec<(&String, Value)> = match param {
            Param::Range { set, .. } => {
                let x = number_of(value).ok_or_else(|| format!("params.{name}: {value} is not a number"))?;
                set.iter().map(|(path, factor)| (path, number_value(x * *factor as f64, whole_field(path)))).collect()
            }
            Param::Choices { choices, set } => {
                let index = value
                    .as_str()
                    .and_then(|c| choices.iter().position(|choice| choice == c))
                    .ok_or_else(|| format!("params.{name}: {value} is not one of {}", choices.join(", ")))?;
                set.iter().map(|(path, values)| (path, values[index].clone())).collect()
            }
        };
        for (path, value) in writes {
            let target = target(doc, path).map_err(|issue| format!("params.{name}.set.{path}: {}", issue.message))?;
            write(&mut out, &target.tokens, value);
        }
    }
    Ok(out)
}

/// The document a placement with these knob values compiles: the family's targets set, its
/// `params` and `presets` gone. `values` come from `PropDef::param_values`.
pub fn apply_params(doc: &Doc, values: &Params, deps: compile::Deps) -> Result<Doc, Vec<Issue>> {
    if values.is_empty() {
        return Ok(doc.clone());
    }
    let value: Value = serde_json::from_str(&doc.write(true)).expect("the writer writes JSON");
    let applied = apply_value(&value, &doc.params, values).map_err(|e| vec![Issue::error("params", Code::UnknownValue, e)])?;
    read::v2::doc(&applied, deps)
}

/// The cache key part for knob values (0: none).
pub fn params_hash(values: &Params) -> u64 {
    if values.is_empty() { 0 } else { fnv(serde_json::to_string(values).expect("values serialize").as_bytes()) }
}

/// `NAME:preset` → (`NAME`, Some(`preset`)).
pub fn split_preset(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once(':') {
        Some((base, preset)) => (base, Some(preset)),
        None => (name, None),
    }
}

/// `canonical` for a placed name: `NAME:preset` keeps its preset (`LIB/core/fence:low`).
pub fn canonical_placed(name: &str) -> Result<String, String> {
    let (base, preset) = split_preset(name);
    let base = canonical(base)?;
    match preset {
        None => Ok(base),
        Some(preset) => {
            validate_part_name(preset).map_err(|e| format!("\"{preset}\" is not a preset name: {e}"))?;
            Ok(format!("{base}:{preset}"))
        }
    }
}

/// A placed name (`NAME` or `NAME:preset`) with knob values over the preset's: the asset's
/// canonical name, its definition and the checked values (defaults left out). The variant
/// must compile.
pub fn resolve_placed(name: &str, given: &Params) -> Result<(String, Arc<PropDef>, Params), String> {
    let (base, preset) = split_preset(name);
    let canonical = canonical(base)?;
    let def = resolve_ref(&canonical)?;
    let values = def.param_values(preset, given)?;
    def.try_variant(&values, 0).map_err(|issues| format!("{name}: {}", issue::render(&issues)))?;
    Ok((canonical, def, values))
}

/// The parts a placed name (`NAME` or `NAME:preset`) draws at `seed`.
pub fn resolve_parts(name: &str, seed: u32) -> Result<Arc<CompiledProp>, String> {
    let (_, def, values) = resolve_placed(name, &Params::new())?;
    Ok(def.variant_with(&values, seed))
}

impl PropDef {
    /// Checked knob values for a placement: a preset's, then `given` over them. A value equal
    /// to its default is left out, so the default placement is the plain asset.
    pub fn param_values(&self, preset: Option<&str>, given: &Params) -> Result<Params, String> {
        let doc = &self.doc;
        let names = |keys: Vec<&str>| if keys.is_empty() { "none".to_string() } else { keys.join(", ") };
        if doc.params.is_empty() && (preset.is_some() || !given.is_empty()) {
            let at = if preset.is_some() { "preset" } else { "params" };
            return Err(Issue::error(at, Code::UnknownKey, format!("{} has no params", doc.name))
                .fix("place it by its name; asset_edit action=schema topic=document explains params")
                .to_string());
        }
        let mut wanted = Params::new();
        if let Some(preset) = preset {
            let values = doc.presets.get(preset).ok_or_else(|| {
                let keys: Vec<&str> = doc.presets.keys().map(String::as_str).collect();
                Issue::error("preset", Code::UnknownValue, format!("{} has no preset \"{preset}\"", doc.name))
                    .expected(names(keys.clone()))
                    .maybe_fix(issue::suggestion(preset, keys))
                    .to_string()
            })?;
            wanted.extend(values.clone());
        }
        wanted.extend(given.iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut out = Params::new();
        for (name, value) in wanted {
            let path = format!("params.{name}");
            let Some(param) = doc.params.get(&name) else {
                let keys: Vec<&str> = doc.params.keys().map(String::as_str).collect();
                return Err(Issue::error(path, Code::UnknownKey, "unknown param")
                    .expected(names(keys.clone()))
                    .maybe_fix(issue::suggestion(&name, keys))
                    .to_string());
            };
            match param {
                Param::Range { range: [lo, hi], int, default, .. } => {
                    let range = format!("{} to {}", read::amount(*lo as f64), read::amount(*hi as f64));
                    let Some(x) = number_of(&value) else {
                        return Err(Issue::error(path, Code::InvalidType, format!("expected a number, got {value}"))
                            .expected(range)
                            .to_string());
                    };
                    if !(*lo as f64..=*hi as f64).contains(&x) {
                        return Err(
                            Issue::error(path, Code::OutOfRange, format!("{} is outside {range}", read::amount(x))).to_string()
                        );
                    }
                    if *int && x.fract() != 0.0 {
                        return Err(Issue::error(path, Code::InvalidType, format!("expected a whole number, got {x}"))
                            .fix(format!("use {} or {}", x.floor(), x.ceil()))
                            .to_string());
                    }
                    if (x - *default as f64).abs() > 1e-4 * (*default as f64).abs().max(1.0) {
                        out.insert(name, number_value(x, *int));
                    }
                }
                Param::Choices { choices, .. } => {
                    let index = value.as_str().and_then(|c| choices.iter().position(|choice| choice == c));
                    let Some(index) = index else {
                        let text = value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string());
                        return Err(Issue::error(path, Code::UnknownValue, format!("unknown choice {value}"))
                            .expected(choices.join(", "))
                            .maybe_fix(issue::suggestion(&text, choices.iter().map(String::as_str)))
                            .to_string());
                    };
                    if index > 0 {
                        out.insert(name, Value::String(choices[index].clone()));
                    }
                }
            }
        }
        Ok(out)
    }

    /// The parts for knob values and a seed; the cache key is (params hash, seed). Values come
    /// from `param_values`; none is `variant(seed)`.
    pub fn try_variant(&self, values: &Params, seed: u32) -> Result<Arc<CompiledProp>, Vec<Issue>> {
        if values.is_empty() {
            return Ok(self.variant(seed));
        }
        let key = (params_hash(values), seed);
        if let Some(variant) = self.families.lock().unwrap().get(&key) {
            return Ok(variant.clone());
        }
        let deps = |name: &str| self.dep(name);
        let doc = apply_params(&self.doc, values, &deps)?;
        let variant = Arc::new(compile::compile(&doc, &deps, seed)?);
        let mut families = self.families.lock().unwrap();
        if families.len() >= VARIANTS {
            let oldest = *families.keys().next().expect("full");
            families.remove(&oldest);
        }
        families.insert(key, variant.clone());
        Ok(variant)
    }

    /// `try_variant`, falling back to the seed's plain variant when the values no longer fit
    /// the definition (an edit removed a knob or narrowed its range).
    pub fn variant_with(&self, values: &Params, seed: u32) -> Arc<CompiledProp> {
        self.try_variant(values, seed).unwrap_or_else(|_| self.variant(seed))
    }
}
