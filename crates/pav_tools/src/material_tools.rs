//! Procedural materials as an asset kind (asset plan §6.6). `assets kind=material` lists the
//! core and workshop materials; `asset_edit kind=material` creates, copies, inspects and
//! patches workshop materials (`MAT/workshop/x`, saved as `assets/materials/workshop/x.json`,
//! revisions `mat1-…`) with the props' transaction: the editor lock, if_revision, a 32-step
//! history, an atomic save, then the library is rebuilt and every view re-packs the rows that
//! changed. `asset_preview kind=material` renders a swatch (sphere, cube, cylinder and a
//! world-mapped 2×2 m slab per material) and says how each layer reads at the game camera.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};

use anyhow::{Result, anyhow, bail};
use glam::Vec3;
use pav_core::issue::{self, Code, Issue, Severity};
use pav_core::material::schema::{FIELDS, FILE_FIELDS, own_params};
use pav_core::material::{Library, Material, PREFIX, read_doc, report, write};
use pav_core::props::doc::Pattern;
use pav_core::props::{self, Doc, PropDef};
use serde_json::{Map, Value, json};

use crate::asset_tools::{History, atomic_write, json_arg, lock_editor, push_history, read_history, summary};
use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_str};

static INITIALIZED: Once = Once::new();

/// Material files use this root (`PAV_MATERIALS` overrides it); the workshop is `workshop/`.
pub fn root() -> PathBuf {
    std::env::var_os("PAV_MATERIALS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/materials"))
}

/// Load the workshop materials once per process, before props compile against them.
pub fn initialize_authored() -> Result<()> {
    let mut result = Ok(());
    INITIALIZED.call_once(|| result = reload_authored_at(&root()).map(|_| ()));
    result
}

/// Read every workshop material file into the library. A file with errors stays out (its
/// errors are returned); the others load.
pub fn reload_authored_at(root: &Path) -> Result<Vec<String>> {
    let folder = root.join("workshop");
    if !folder.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&folder)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    let mut lib = Library::current();
    for path in paths {
        let Some(leaf) = path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|e| e == "json"))
        else {
            continue;
        };
        if leaf.starts_with('.') || check_leaf(leaf).is_err() {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        lib = Library::set_workshop(&format!("workshop/{leaf}"), Some(text));
        names.push(format!("{PREFIX}workshop/{leaf}"));
    }
    let broken: Vec<String> = names
        .iter()
        .filter_map(|name| {
            let errors: Vec<String> =
                lib.issues().get(name)?.iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
            (!errors.is_empty()).then(|| format!("{name}: {}", errors.join("; ")))
        })
        .collect();
    if !broken.is_empty() {
        bail!("some workshop materials did not load: {}", broken.join(" | "));
    }
    Ok(names)
}

fn check_leaf(leaf: &str) -> Result<()> {
    props::validate_name(leaf).map_err(|e| anyhow!(e))?;
    if leaf.contains('-') {
        bail!("material names use a-z, 0-9 and _: '{leaf}'");
    }
    Ok(())
}

/// `x`, `workshop/x` or `MAT/workshop/x` is the workshop's `x`; other `MAT/…` names are the
/// core library's, read-only. Returns the full name and, for the workshop, the leaf.
fn canonical(name: &str) -> Result<(String, Option<String>)> {
    let bare = name.strip_prefix(PREFIX).unwrap_or(name);
    let leaf = match bare.strip_prefix("workshop/") {
        Some(leaf) => Some(leaf),
        None if !bare.contains('/') && !name.starts_with(PREFIX) => Some(bare),
        None => None,
    };
    match leaf {
        Some(leaf) => {
            check_leaf(leaf)?;
            Ok((format!("{PREFIX}workshop/{leaf}"), Some(leaf.to_owned())))
        }
        None if pav_core::material::is_library_name(&format!("{PREFIX}{bare}")) => Ok((format!("{PREFIX}{bare}"), None)),
        None => bail!("material names are MAT/name (core) or MAT/workshop/name"),
    }
}

/// A material to read: as `canonical`, but a bare name the workshop lacks may be a core one
/// (`oak_planks` → `MAT/oak_planks`).
fn read_name(name: &str) -> Result<(String, Option<String>)> {
    let (full, leaf) = canonical(name)?;
    let lib = Library::current();
    if leaf.is_some() && !name.contains('/') && lib.get(&full).is_none() && lib.get(&format!("{PREFIX}{name}")).is_some() {
        return Ok((format!("{PREFIX}{name}"), None));
    }
    Ok((full, leaf))
}

/// An unknown material, with the nearest names.
fn unknown(name: &str, lib: &Library) -> anyhow::Error {
    anyhow!(
        "{}",
        Issue::error("name", Code::UnknownMaterial, format!("no material \"{name}\""))
            .maybe_fix(issue::suggestion(name, lib.names()).or_else(|| Some("assets kind=material lists them".into())))
    )
}

/// A material's file as JSON: the workshop's from disk, the core's as embedded.
fn document(root: &Path, name: &str, leaf: Option<&str>) -> Result<Option<Map<String, Value>>> {
    let text = match leaf {
        Some(leaf) => {
            let path = root.join("workshop").join(format!("{leaf}.json"));
            if !path.exists() {
                return Ok(None);
            }
            std::fs::read_to_string(&path)?
        }
        None => match pav_core::material::library::MATERIALS.iter().find(|(n, _)| format!("{PREFIX}{n}") == name) {
            Some((_, text)) => text.to_string(),
            None => return Ok(None),
        },
    };
    match props::strict_value(&text).map_err(|issue| anyhow!("{name}: {issue}"))? {
        Value::Object(map) => Ok(Some(map)),
        _ => bail!("{name} is not a JSON object"),
    }
}

/// A number to a tenth, without a trailing `.0`.
fn num(x: f32) -> String {
    format!("{}", (x * 10.0).round() / 10.0)
}

fn revision(doc: Option<&Map<String, Value>>) -> String {
    doc.map(write::revision).unwrap_or_else(|| "absent".into())
}

/// Cost and warnings of a loaded material: `cost`, `layers` and `check` reply fields.
fn summary_of(material: &Material, issues: &[Issue], out: &mut Value) {
    out["layers"] = json!(material.layers.len());
    let cost = material.cost();
    out["cost"] = json!(format!("{} unit{}", num(cost), if cost == 1.0 { "" } else { "s" }));
    let check: Vec<String> = issues.iter().map(Issue::to_string).collect();
    if !check.is_empty() {
        out["check"] = json!(check);
    }
}

// --- assets kind=material ---------------------------------------------------------------

/// `assets kind=material [find=] [limit=] [page=]`: one row per material.
pub fn t_materials(_: &mut Session, a: &Args) -> Result<Output> {
    let accepted = ["find", "limit", "page"];
    for key in a.keys() {
        if !accepted.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()) {
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, "not an assets argument for kind=material")
                    .expected(["kind"].iter().chain(&accepted).copied().collect::<Vec<_>>().join(", "))
                    .maybe_fix(issue::suggestion(key, accepted))
            );
        }
    }
    let number = |key: &str, default: u64| -> Result<usize> {
        match a.get(key) {
            None => Ok(default as usize),
            Some(v) => v
                .as_u64()
                .or_else(|| v.as_str().and_then(|t| t.parse().ok()))
                .filter(|n| *n >= 1 && *n <= 1000)
                .map(|n| n as usize)
                .ok_or_else(|| anyhow!("{key} must be an integer from 1 to 1000")),
        }
    };
    let (limit, page) = (number("limit", 30)?, number("page", 1)?);
    let find = get_str(a, "find").unwrap_or("");
    let terms = pav_core::store::terms(find);
    let lib = Library::current();
    // Each word matches by any of its forms or synonyms anywhere in the name, description or
    // tags; rows matching every word come first, ranked by how many words their name holds.
    let mut scored: Vec<(usize, usize, &str, &Arc<Material>)> = lib
        .names()
        .filter_map(|name| lib.get(name).map(|m| (name, m)))
        .map(|(name, m)| {
            let lower = name.to_ascii_lowercase();
            let text = lib.doc(name).map(|d| format!("{} {}", d.description, d.tags.join(" "))).unwrap_or_default();
            let text = format!("{lower} {}", text.to_ascii_lowercase());
            // A word matches a form when it starts with it, or ends with it as a compound
            // ("flagstones" holds "stone"; "wrought" does not hold "rough").
            let hit = |term: &pav_core::store::Term, haystack: &str| {
                haystack.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).any(|word| {
                    let one = pav_core::store::singular(word);
                    term.forms
                        .iter()
                        .chain(&term.synonyms)
                        .any(|form| one.starts_with(form.as_str()) || (form.len() >= 4 && one.ends_with(form.as_str())))
                })
            };
            let matched = terms.iter().filter(|term| hit(term, &text)).count();
            let in_name = terms.iter().filter(|term| hit(term, &lower)).count();
            (matched, in_name, name, m)
        })
        .filter(|(matched, ..)| terms.is_empty() || *matched > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then_with(|| a.2.cmp(b.2)));
    let whole = scored.iter().filter(|row| row.0 == terms.len()).count();
    if !terms.is_empty() && whole == 0 {
        // Nothing matches every word: rows matching some, or none with the nearest and a route.
        if scored.is_empty() {
            let distance = |name: &str| {
                let leaf = name.rsplit('/').next().unwrap_or(name);
                terms.iter().map(|term| issue::edit_distance(&term.forms[0], leaf)).min().unwrap_or(usize::MAX)
            };
            let mut near: Vec<&str> = lib.names().collect();
            near.sort_by(|x, y| distance(x).cmp(&distance(y)).then_with(|| x.cmp(y)));
            near.truncate(3);
            let names: Vec<String> = near.iter().map(|name| name.to_string()).collect();
            crate::asset_search::log_miss(&crate::asset_search::store_root(), "material", find, &names);
            let slug = crate::asset_search::slug(&terms);
            return Ok(Output::Json(json!({
                "cols": "name layers cost desc",
                "none": format!("nothing matches \"{find}\""),
                "nearest": near.iter().filter_map(|name| lib.get(name).map(|m| material_row(&lib, name, m))).collect::<Vec<_>>(),
                "route": format!("make it: asset_edit kind=material action=copy from=<one of the {} in assets kind=material> name={slug} and change its layers, or action=create name={slug}; asset_edit kind=material action=schema is the field guide", lib.names().count()),
            })));
        }
    }
    let partial = !terms.is_empty() && whole == 0;
    let matched: Vec<(&str, &Arc<Material>)> = scored.iter().map(|(_, _, name, m)| (*name, *m)).collect();
    let skip = (page - 1) * limit;
    let rows: Vec<Value> = matched.iter().skip(skip).take(limit).map(|(name, m)| material_row(&lib, name, m)).collect();
    let mut out = json!({"cols": "name layers cost desc", "rows": rows, "total": lib.names().count(), "matched": matched.len()});
    if partial {
        out["partial"] = json!("no material matches every word; these match some");
    }
    if skip + limit < matched.len() {
        out["next"] = json!(page + 1);
    }
    Ok(Output::Json(out))
}

fn material_row(lib: &Library, name: &str, m: &Material) -> Value {
    let desc = lib.doc(name).map(|d| summary(&d.description)).unwrap_or_default();
    let layers: Vec<&str> = m.layers.iter().map(|l| l.pattern.name()).collect();
    json!([name, layers.join("+"), num(m.cost()), desc])
}

// --- asset_edit kind=material ----------------------------------------------------------------

const ACTIONS: &[&str] = &["create", "copy", "inspect", "patch", "replace", "undo", "redo", "schema"];
const OPS_HELP: &str = "{op:set,fields} | {op:set,layer:i,fields} | {op:add,layer:{type,...}} | {op:remove,layer:i}";

fn check_args(a: &Args) -> Result<&str> {
    let action = get_str(a, "action").unwrap_or("inspect");
    let extra: &[&str] = match action {
        "create" => &["asset", "description", "dry_run"],
        "copy" => &["from", "description", "dry_run"],
        "inspect" => &[],
        "patch" => &["ops", "if_revision", "dry_run"],
        "replace" => &["asset", "if_revision", "dry_run"],
        "undo" | "redo" => &["if_revision"],
        "schema" => &["topic"],
        _ => bail!(
            "{}",
            Issue::error("action", Code::UnknownValue, format!("unknown action \"{action}\" for kind=material"))
                .expected(ACTIONS.join(", "))
                .maybe_fix(issue::suggestion(action, ACTIONS.iter().copied()))
        ),
    };
    let accepted: Vec<&str> = ["action", "name", "preview"].into_iter().chain(extra.iter().copied()).collect();
    for key in a.keys() {
        if !accepted.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()) {
            bail!(
                "{}",
                Issue::error(
                    key.as_str(),
                    Code::UnknownKey,
                    format!("not an asset_edit kind=material argument for action={action}")
                )
                .expected(accepted.join(", "))
                .maybe_fix(issue::suggestion(key, accepted.iter().copied()))
            );
        }
    }
    Ok(action)
}

/// The material field guide.
fn schema(topic: Option<&str>) -> Result<Value> {
    let pairs = |rows: &[(&str, &str)]| Value::Object(rows.iter().map(|(k, v)| (k.to_string(), json!(v))).collect());
    let layers: Map<String, Value> = Pattern::NAMES
        .iter()
        .map(|name| {
            let pattern = Pattern::parse(name).expect("listed");
            let mut params: Vec<&str> = own_params(pattern).to_vec();
            if pav_core::material::schema::is_cellular(pattern) {
                params.extend(["vary", "cells", "cavity"]);
                if pattern != Pattern::Dots {
                    params.push("bevel");
                }
            }
            (name.to_string(), json!(params.join(" ")))
        })
        .collect();
    let sections = [
        ("fields", pairs(FIELDS)),
        ("file", pairs(FILE_FIELDS)),
        ("layers", Value::Object(layers)),
        (
            "every_layer",
            json!(
                "color or shade (0-2, default 0.6), strength 0-1, glow 0-8, facing up|down|side, masked_by prev|not_prev, seed"
            ),
        ),
        ("ops", json!(OPS_HELP)),
        (
            "legibility",
            json!("features under 5 cm or gaps under 1.5 cm blur at the game camera; at most 3 layers and 6 cost units"),
        ),
    ];
    match topic {
        None => Ok(Value::Object(sections.into_iter().map(|(k, v)| (k.into(), v)).collect())),
        Some(topic) => match sections.iter().find(|(k, _)| *k == topic) {
            Some((k, v)) => Ok(json!({ *k: v })),
            None => bail!(
                "{}",
                Issue::error("topic", Code::UnknownValue, format!("unknown topic \"{topic}\""))
                    .expected(sections.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", "))
            ),
        },
    }
}

/// What a patch batch touched, so a problem in the result names the op that caused it.
#[derive(Default)]
struct Touched {
    paths: Vec<(String, usize)>,
}

impl Touched {
    fn readdress(&self, issue: &Issue) -> String {
        let op = self
            .paths
            .iter()
            .rev()
            .filter(|(path, _)| {
                issue.path == *path || issue.path.starts_with(&format!("{path}.")) || issue.path.starts_with(&format!("{path}["))
            })
            .max_by_key(|(path, _)| path.len())
            .map(|(_, op)| *op);
        match op {
            Some(op) => format!("ops[{op}] {issue}"),
            None => issue.to_string(),
        }
    }
}

/// Merge `fields` into `object`: null deletes a key.
fn merge(object: &mut Map<String, Value>, fields: &Map<String, Value>) {
    for (key, value) in fields {
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
}

fn apply_ops(doc: &mut Map<String, Value>, ops: &Value) -> Result<Touched> {
    let ops = ops.as_array().ok_or_else(|| anyhow!("ops: expected a list of ops ({OPS_HELP})"))?;
    if ops.is_empty() || ops.len() > props::MAX_OPS {
        bail!("ops: 1 to {} ops", props::MAX_OPS);
    }
    let mut touched = Touched::default();
    for (i, op) in ops.iter().enumerate() {
        let at = |key: &str| format!("ops[{i}].{key}");
        let fail = |issue: Issue| anyhow!("{issue}");
        let op = op.as_object().ok_or_else(|| anyhow!("ops[{i}]: expected an object ({OPS_HELP})"))?;
        let kind = op.get("op").and_then(Value::as_str).ok_or_else(|| anyhow!("ops[{i}].op: missing ({OPS_HELP})"))?;
        let allowed: &[&str] = match kind {
            "set" => &["op", "layer", "fields"],
            "add" => &["op", "layer"],
            "remove" => &["op", "layer"],
            _ => {
                return Err(fail(
                    Issue::error(at("op"), Code::UnknownValue, format!("unknown op \"{kind}\""))
                        .expected("set, add, remove")
                        .maybe_fix(issue::suggestion(kind, ["set", "add", "remove"])),
                ));
            }
        };
        if let Some(key) = op.keys().find(|k| !allowed.contains(&k.as_str())) {
            let fix = match key.as_str() {
                "part" | "material" => Some("a material op has no target: set fields, or name a layer index".to_owned()),
                "value" => Some("op:add takes layer:{type,...}".to_owned()),
                _ => issue::suggestion(key, allowed.iter().copied()),
            };
            return Err(fail(Issue::error(at(key), Code::UnknownKey, "unknown key").expected(allowed.join(", ")).maybe_fix(fix)));
        }
        let layers_len = doc.get("layers").and_then(Value::as_array).map_or(0, Vec::len);
        let index = |required: bool| -> Result<Option<usize>> {
            match op.get("layer") {
                None if required => Err(fail(Issue::error(at("layer"), Code::Missing, "missing").expected("a layer index"))),
                None => Ok(None),
                Some(v) => match v.as_u64().map(|n| n as usize) {
                    Some(n) if n < layers_len => Ok(Some(n)),
                    _ => Err(fail(Issue::error(at("layer"), Code::OutOfRange, format!("no layer {v}")).expected(
                        if layers_len == 0 { "no layers yet: op:add".into() } else { format!("0 to {}", layers_len - 1) },
                    ))),
                },
            }
        };
        match kind {
            "set" => {
                let fields = op
                    .get("fields")
                    .and_then(Value::as_object)
                    .ok_or_else(|| fail(Issue::error(at("fields"), Code::Missing, "missing").expected("an object of fields")))?;
                match index(false)? {
                    Some(n) => {
                        let layer = doc["layers"][n].as_object_mut().ok_or_else(|| anyhow!("layers[{n}] is not an object"))?;
                        merge(layer, fields);
                        touched.paths.push((format!("layers[{n}]"), i));
                    }
                    None => {
                        for key in ["format", "name", "unit"] {
                            if fields.contains_key(key) {
                                let fix = if key == "unit" {
                                    "send the whole material in the new unit (action=replace)"
                                } else {
                                    "it is fixed"
                                };
                                return Err(fail(
                                    Issue::error(at(&format!("fields.{key}")), Code::Conflict, "cannot be set").fix(fix),
                                ));
                            }
                        }
                        merge(doc, fields);
                        touched.paths.extend(fields.keys().map(|key| (key.clone(), i)));
                    }
                }
            }
            "add" => {
                let layer = op.get("layer").filter(|v| v.is_object()).ok_or_else(|| {
                    fail(Issue::error(at("layer"), Code::Missing, "missing").expected("the new layer {type,...}"))
                })?;
                let layers = doc.entry("layers").or_insert_with(|| json!([]));
                let layers = layers.as_array_mut().ok_or_else(|| anyhow!("layers is not a list"))?;
                layers.push(layer.clone());
                touched.paths.push((format!("layers[{}]", layers.len() - 1), i));
                touched.paths.push(("layers".into(), i));
            }
            _ => {
                let n = index(true)?.expect("required");
                let layers = doc["layers"].as_array_mut().expect("checked");
                layers.remove(n);
                if layers.is_empty() {
                    doc.remove("layers");
                }
            }
        }
    }
    Ok(touched)
}

/// The library with `name` (`workshop/x`) set to `text` (None: removed), not installed.
fn candidate(name: &str, text: Option<&str>) -> Library {
    let mut sources: Vec<(String, String)> =
        pav_core::material::library::MATERIALS.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect();
    let current = Library::current();
    for full in current.names().chain(current.broken()).filter(|n| n.starts_with(&format!("{PREFIX}workshop/"))) {
        let short = &full[PREFIX.len()..];
        if short != name {
            if let Some(text) = Library::workshop_text(short) {
                sources.push((short.to_owned(), text));
            }
        }
    }
    if let Some(text) = text {
        sources.push((name.to_owned(), text.to_owned()));
    }
    sources.sort();
    sources.dedup_by(|a, b| a.0 == b.0);
    let pairs: Vec<(&str, &str)> = sources.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    Library::from_sources(&pairs)
}

fn edit_at(root: &Path, a: &Args) -> Result<Value> {
    let action = check_args(a)?;
    if action == "schema" {
        return schema(get_str(a, "topic"));
    }
    let given = get_str(a, "name").ok_or_else(|| anyhow!("name= names the material, e.g. MAT/workshop/moss"))?;
    let (name, leaf) = if action == "inspect" { read_name(given)? } else { canonical(given)? };
    if action == "inspect" {
        let lib = Library::current();
        let doc = document(root, &name, leaf.as_deref())?.ok_or_else(|| unknown(&name, &lib))?;
        let mut out = json!({"name": name, "revision": write::revision(&doc), "material": doc});
        if let Some(material) = lib.get(&name) {
            summary_of(material, lib.issues().get(&name).map(Vec::as_slice).unwrap_or_default(), &mut out);
        } else if let Some(issues) = lib.issues().get(&name) {
            out["errors"] = json!(issues.iter().map(Issue::to_string).collect::<Vec<_>>());
        }
        return Ok(out);
    }
    let Some(leaf) = leaf else {
        bail!("core materials are read-only; action=copy from={name} name=your_name makes an editable one");
    };
    let short = format!("workshop/{leaf}");
    let _lock = lock_editor(root)?;
    let before = document(root, &name, Some(&leaf))?;
    let current_revision = revision(before.as_ref());
    if let Some(expected) = get_str(a, "if_revision") {
        if expected != current_revision {
            bail!(
                "revision conflict for {name}: expected {expected}, current {current_revision}; inspect it and apply the edit again"
            );
        }
    }
    if matches!(action, "create" | "copy") && before.is_some() {
        bail!("{name} already exists; patch it, or send the whole material with action=replace");
    }
    let history_file = root.join(".editor").join(format!("{leaf}.json"));
    let mut history = read_history(&history_file, &current_revision)?;
    let history_reset = history.head != current_revision;
    if history_reset {
        if matches!(action, "undo" | "redo") {
            bail!("the material changed outside this editor; its old history does not match revision {current_revision}");
        }
        history = History { head: current_revision.clone(), ..Default::default() };
    }
    let before_value = before.clone().map(Value::Object);
    let mut touched = Touched::default();
    let next: Option<Map<String, Value>> = match action {
        "create" | "replace" => {
            let mut doc = match json_arg(a, "asset")? {
                Value::Object(map) => map,
                _ => bail!("asset: expected a material object {{color, layers, ...}}"),
            };
            if action == "replace" && before.is_none() {
                bail!("no material {name}; use action=create");
            }
            doc.entry("format").or_insert(json!(1));
            doc.entry("unit").or_insert(json!("cm"));
            match doc.get("name").and_then(Value::as_str) {
                Some(given) if given != short && given != leaf && given != name => {
                    bail!("asset.name must be \"{short}\" (or left out); use action=copy for a new name")
                }
                _ => doc.insert("name".into(), json!(short)),
            };
            if let Some(description) = get_str(a, "description") {
                doc.insert("description".into(), json!(description));
            }
            Some(doc)
        }
        "copy" => {
            let lib = Library::current();
            let (from, from_leaf) =
                read_name(get_str(a, "from").ok_or_else(|| anyhow!("from= names the source, e.g. MAT/oak_planks"))?)?;
            let mut doc = document(root, &from, from_leaf.as_deref())?.ok_or_else(|| unknown(&from, &lib))?;
            doc.insert("name".into(), json!(short));
            if let Some(description) = get_str(a, "description") {
                doc.insert("description".into(), json!(description));
            }
            Some(doc)
        }
        "patch" => {
            let mut doc = before.clone().ok_or_else(|| anyhow!("no material {name}; use action=create or action=copy first"))?;
            touched = apply_ops(&mut doc, &json_arg(a, "ops")?)?;
            Some(doc)
        }
        "undo" | "redo" => {
            let (from, to) =
                if action == "undo" { (&mut history.undo, &mut history.redo) } else { (&mut history.redo, &mut history.undo) };
            let step = from.pop().ok_or_else(|| anyhow!("no edit to {action} for {name}"))?;
            push_history(to, before_value.clone());
            match step {
                Some(Value::Object(map)) => Some(map),
                Some(_) => bail!("saved history for {name} is not a material"),
                None => None,
            }
        }
        _ => unreachable!("checked"),
    };
    // The whole document is read again and resolved in the library it would join.
    let text = next.as_ref().map(write::text);
    if let Some(doc) = &next {
        if let Err(issues) = read_doc(&Value::Object(doc.clone()), Some(&short)) {
            let lines: Vec<String> = issues.iter().map(|i| touched.readdress(i)).collect();
            bail!("{}", lines.join("\n"));
        }
    }
    let lib = candidate(&short, text.as_deref());
    let mut problems: Vec<Issue> = lib.issues().get(&name).cloned().unwrap_or_default();
    // A workshop material based on this one must still load.
    for (other, issues) in lib.issues() {
        if other != &name && issues.iter().any(|i| i.severity == Severity::Error) && Library::current().get(other).is_some() {
            problems.push(
                Issue::error("", Code::Conflict, format!("{other} would stop loading")).fix("it uses this material as its base"),
            );
        }
    }
    let errors: Vec<String> = problems.iter().filter(|i| i.severity == Severity::Error).map(|i| touched.readdress(i)).collect();
    if !errors.is_empty() {
        bail!("{}", errors.join("\n"));
    }
    let warnings: Vec<Issue> = problems.into_iter().filter(|i| i.severity != Severity::Error).collect();
    let next_revision = revision(next.as_ref());
    let mut out = json!({"name": name, "revision": current_revision, "saved": false});
    if get_bool(a, "dry_run", false)? {
        out["revision_next"] = json!(next_revision);
        if let Some(material) = lib.get(&name) {
            summary_of(material, &warnings, &mut out);
        }
        return Ok(out);
    }
    if next_revision != current_revision {
        if !matches!(action, "undo" | "redo") {
            push_history(&mut history.undo, before_value);
            history.redo.clear();
        }
        history.head = next_revision.clone();
        let path = root.join("workshop").join(format!("{leaf}.json"));
        atomic_write(&history_file, &serde_json::to_vec(&history)?)?;
        match &text {
            Some(text) => atomic_write(&path, text.as_bytes())?,
            None => std::fs::remove_file(&path)?,
        }
        Library::set_workshop(&short, text);
        out["saved"] = json!(true);
        out["revision"] = json!(next_revision);
        if matches!(action, "create" | "copy") {
            out["file"] = json!(path);
        }
    }
    for (key, steps) in [("undo", &history.undo), ("redo", &history.redo)] {
        if !steps.is_empty() {
            out[key] = json!(steps.len());
        }
    }
    if history_reset {
        out["history_reset"] = json!(true);
    }
    if let Some(material) = Library::current().get(&name) {
        summary_of(material, &warnings, &mut out);
    }
    Ok(out)
}

/// `asset_edit kind=material`.
pub fn t_material_edit(_: &mut Session, a: &Args) -> Result<Output> {
    Ok(Output::Json(edit_at(&root(), a)?))
}

// --- asset_preview kind=material -------------------------------------------------------------

/// Swatch rows at most.
const MAX_ROWS: usize = 6;

/// A swatch prop: one row per material, front to back: a sphere, a cube and a cylinder in
/// the material, then a 2×2 m slab laid in world space.
fn swatch(names: &[String]) -> Result<PropDef> {
    let mut materials = Map::new();
    let mut parts = Map::new();
    let n = names.len() as f32;
    for (i, name) in names.iter().enumerate() {
        let z = ((n - 1.0) * 0.5 - i as f32) * 260.0;
        materials.insert(format!("m{i}"), json!(name));
        materials.insert(format!("w{i}"), json!({"base": name, "map": "world"}));
        let m = format!("m{i}");
        parts.insert(format!("sphere{i}"), json!({"type": "sphere", "radius": 40, "pos": [-230, 40, z], "mat": m}));
        parts.insert(format!("cube{i}"), json!({"type": "box", "size": [80, 80, 80], "pos": [-110, 40, z], "mat": m}));
        parts
            .insert(format!("cylinder{i}"), json!({"type": "cylinder", "radius": 35, "height": 80, "pos": [0, 40, z], "mat": m}));
        parts.insert(
            format!("slab{i}"),
            json!({"type": "box", "size": [200, 6, 200], "pos": [170, 3, z], "mat": format!("w{i}")}),
        );
    }
    let value = json!({"format": 2, "name": "swatch", "unit": "cm", "materials": materials, "parts": parts});
    let doc = Doc::from_value(&value).map_err(|issues| anyhow!(issue::render(&issues)))?;
    PropDef::new(doc).map_err(|issues| anyhow!(issue::render(&issues)))
}

/// Lines for each layer: feature size, pixels at the game camera, coverage, verdict.
fn layer_lines(material: &Material) -> Vec<String> {
    report::layers(material)
        .iter()
        .map(|l| {
            let size = match (l.feature, l.px) {
                (Some(m), Some(px)) => {
                    format!(
                        " {} cm, {} px at 1080p ({} at 540p)",
                        num(m * 100.0),
                        px.round(),
                        (m / report::PIXEL_540P * 10.0).round() / 10.0
                    )
                }
                _ => String::new(),
            };
            format!("{}{size}, covers {}%: {}", l.pattern, (l.cover * 100.0).round(), l.verdict)
        })
        .collect()
}

/// `asset_preview kind=material name= [compare=a,b] [view=close|game]`.
pub fn t_material_preview(s: &mut Session, a: &Args) -> Result<Output> {
    let accepted = ["name", "compare", "view"];
    for key in a.keys() {
        if !accepted.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()) {
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, "not an asset_preview kind=material argument")
                    .expected(accepted.join(", "))
                    .maybe_fix(issue::suggestion(key, accepted))
            );
        }
    }
    let lib = Library::current();
    let mut names = Vec::new();
    if let Some(name) = get_str(a, "name") {
        names.push(name.to_owned());
    }
    match a.get("compare") {
        None => {}
        Some(Value::String(list)) => {
            names.extend(list.trim_matches(['[', ']']).split(',').map(|t| t.trim().trim_matches('"').to_owned()))
        }
        Some(Value::Array(items)) => names.extend(items.iter().filter_map(Value::as_str).map(String::from)),
        Some(_) => bail!("compare: a list of material names"),
    }
    names.retain(|n| !n.is_empty());
    if names.is_empty() {
        bail!("{}", Issue::error("name", Code::Missing, "missing").fix("name=MAT/oak_planks, and compare=a,b for more rows"));
    }
    if names.len() > MAX_ROWS {
        bail!(
            "{}",
            Issue::error("compare", Code::Limit, format!("{} materials", names.len()))
                .expected(format!("at most {MAX_ROWS} rows"))
        );
    }
    let names: Vec<String> = names
        .iter()
        .map(|n| {
            let (full, _) = read_name(n)?;
            lib.get(&full).ok_or_else(|| unknown(&full, &lib))?;
            Ok(full)
        })
        .collect::<Result<_>>()?;
    let game = match get_str(a, "view").unwrap_or("close") {
        "close" => false,
        "game" => true,
        other => {
            bail!("{}", Issue::error("view", Code::UnknownValue, format!("unknown view \"{other}\"")).expected("close, game"))
        }
    };
    let def = Arc::new(swatch(&names)?);
    let frame =
        pav_core::prop_preview::stage_frame(Arc::new(s.sim.config.clone()), "SWATCH/material", def.clone(), Default::default());
    let (width, height) = (960, 540);
    let lift = -Vec3::Y * def.compiled.bounds.min.y;
    let points: Vec<Vec3> = def.compiled.parts.iter().flat_map(|p| p.hull(1.0)).map(|p| p + lift).collect();
    let camera = if game {
        let mut rig = pav_view::CameraRig::new(pav_view::CameraParams {
            tilt: 56.0,
            yaw: 0.0,
            distance: 15.5,
            fov: 40.0,
            ortho: false,
            follow_lag: 0.0,
            height_offset: 0.0,
        });
        rig.snap(def.compiled.bounds.center() + lift);
        rig
    } else {
        crate::asset_preview_tools::sheet_camera(&points, -20.0, 35.0, 30.0)
    };
    let saved = std::mem::replace(&mut s.camera, camera);
    let rgba = s.render_frame(&frame, width, height);
    s.camera = saved;
    let png = pav_render::capture::encode_png(width, height, &rgba?)?;
    let leaf = names[0].rsplit('/').next().unwrap_or("material");
    let path = PathBuf::from(format!("out/swatch-{leaf}.png"));
    std::fs::create_dir_all("out")?;
    std::fs::write(&path, &png)?;
    let mut layers = Map::new();
    for name in &names {
        let material = lib.get(name).expect("checked");
        layers.insert(name.clone(), json!(layer_lines(material)));
    }
    let meta = json!({
        "path": path,
        "rows": "front to back: sphere, cube, cylinder, 2x2 m world slab",
        "view": if game { "game: tilt 56, 15.5 m" } else { "close" },
        "layers": layers,
    });
    Ok(Output::Image { png, path: Some(path), meta })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Args {
        value.as_object().unwrap().clone()
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pav-materials-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn edit(root: &Path, value: Value) -> Value {
        edit_at(root, &args(value)).unwrap()
    }

    fn error(root: &Path, value: Value) -> String {
        format!("{:#}", edit_at(root, &args(value)).unwrap_err())
    }

    #[test]
    fn workshop_materials_create_patch_undo_and_redo() {
        let root = temp_root("edit");
        let name = "MAT/workshop/test_edit_moss";
        let created = edit(
            &root,
            json!({"action":"create","name":"test_edit_moss","asset":{"color":"#5a6a3a","layers":[{"type":"noise","size":30,"cover":0.6,"color":"#3a4a2a"}]}}),
        );
        let first = created["revision"].as_str().unwrap().to_owned();
        assert!(first.starts_with("mat1-") && created["saved"] == true, "{created}");
        assert_eq!((created["layers"].clone(), created["cost"].clone()), (json!(1), json!("1.5 units")));
        let text = std::fs::read_to_string(root.join("workshop/test_edit_moss.json")).unwrap();
        assert_eq!(
            text,
            "{\"format\":1,\"name\":\"workshop/test_edit_moss\",\"unit\":\"cm\",\n\"color\":\"#5a6a3a\",\n\"layers\":[\n {\"type\":\"noise\",\"size\":30,\"cover\":0.6,\"color\":\"#3a4a2a\"}]}\n"
        );
        assert_eq!(Library::current().get(name).unwrap().color, "#5a6a3a", "the library has it at once");
        // One batch: the document's fields, a layer's, a new layer and a removed one.
        let patched = edit(
            &root,
            json!({"action":"patch","name":name,"if_revision":first,"ops":[
                {"op":"set","fields":{"color":"#4a5a30","roughness":0.8}},
                {"op":"add","layer":{"type":"cobbles","size":60,"gap":4}},
                {"op":"set","layer":0,"fields":{"size":40,"cover":null}},
                {"op":"add","layer":{"type":"noise","size":80,"shade":0.8}},
                {"op":"remove","layer":2}]}),
        );
        let second = patched["revision"].as_str().unwrap().to_owned();
        assert_ne!(second, first);
        let material = Library::current().get(name).unwrap().clone();
        assert_eq!((material.color.as_str(), material.roughness, material.layers.len()), ("#4a5a30", Some(0.8), 2));
        assert_eq!(material.layers[0].params.get("size"), Some(&pav_core::material::Param::Num(0.4)));
        // A problem names the op that caused it; a stale revision and a core edit are refused.
        assert_eq!(
            error(
                &root,
                json!({"action":"patch","name":name,"ops":[{"op":"set","fields":{"look":"cel"}},{"op":"set","layer":1,"fields":{"sise":20}}]})
            ),
            r#"ops[1] layers[1].sise: unknown key — did you mean "size"?"#
        );
        assert_eq!(
            error(&root, json!({"action":"patch","name":name,"ops":[{"op":"remove","layer":5}]})),
            "ops[0].layer: no layer 5 (expected 0 to 1)"
        );
        assert!(
            error(&root, json!({"action":"patch","name":name,"if_revision":first,"ops":[]})).starts_with("revision conflict")
        );
        assert!(error(&root, json!({"action":"patch","name":"MAT/oak_planks","ops":[]})).contains("read-only"));
        // A dry run writes nothing.
        let dry = edit(&root, json!({"action":"patch","name":name,"dry_run":true,"ops":[{"op":"set","fields":{"scale":2}}]}));
        assert_eq!((dry["saved"].clone(), dry["revision"].clone()), (json!(false), json!(second)));
        // Undo and redo walk the history.
        let undone = edit(&root, json!({"action":"undo","name":name}));
        assert_eq!((undone["revision"].as_str().unwrap(), undone["redo"].clone()), (first.as_str(), json!(1)));
        assert_eq!(Library::current().get(name).unwrap().color, "#5a6a3a");
        let redone = edit(&root, json!({"action":"redo","name":name}));
        assert_eq!(redone["revision"].as_str().unwrap(), second);
        // A copy of a core material starts from its file; inspect shows the document.
        let copy = edit(&root, json!({"action":"copy","name":"test_edit_oak","from":"MAT/oak_planks"}));
        assert_eq!(copy["layers"], 2);
        let inspected = edit(&root, json!({"action":"inspect","name":"MAT/workshop/test_edit_oak"}));
        assert_eq!(inspected["material"]["name"], "workshop/test_edit_oak");
        assert_eq!(inspected["revision"], copy["revision"]);
        // Bases resolve in the library the edit would make: no circles.
        edit(&root, json!({"action":"create","name":"test_edit_child","asset":{"base":"MAT/workshop/test_edit_oak","scale":2}}));
        let broken = error(
            &root,
            json!({"action":"patch","name":"test_edit_oak","ops":[{"op":"set","fields":{"base":"MAT/workshop/test_edit_child"}}]}),
        );
        assert!(broken.contains("ops[0] base: bases go round in a circle"), "{broken}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_listing_and_the_field_guide() {
        let mut s = Session::new("empty", 1).unwrap();
        let Output::Json(listed) = t_materials(&mut s, &args(json!({"find":"oak"}))).unwrap() else { panic!() };
        let rows = listed["rows"].as_array().unwrap();
        assert!(rows.iter().any(|row| row[0] == "MAT/oak_planks" && row[1] == "planks+wood" && row[2] == "2.5"), "{listed}");
        let guide = schema(None).unwrap();
        assert!(guide["layers"]["planks"].as_str().unwrap().starts_with("size gap axis vary cells cavity bevel"), "{guide}");
        assert!(schema(Some("opps")).is_err());
    }
}
