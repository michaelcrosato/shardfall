//! Persistent authoring for procedural props. One validated batch is one file replacement,
//! history step and library publication. Preview control is separate from that transaction.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};

use anyhow::{Context, Result, anyhow, bail};
use pav_core::issue::{self, Code, Issue};
use pav_core::props::geom::{Generated, Primitive};
use pav_core::props::lint::{self, Finding};
use pav_core::props::{self, CompiledProp, Doc, PropDef};
use pav_render::MeshKey;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_str};

const HISTORY_LIMIT: usize = 32;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static INITIALIZED: Once = Once::new();

/// Undo and redo steps hold documents as JSON: format 1 or 2, and older files' typed format-1
/// assets read the same way.
#[derive(Default, Serialize, Deserialize)]
struct History {
    head: String,
    undo: Vec<Option<Value>>,
    redo: Vec<Option<Value>>,
}

struct Edited {
    report: Value,
    def: Option<Arc<PropDef>>,
    /// Assets that reference the edited one and were recompiled with it.
    dependents: Vec<String>,
    mutation: bool,
    preview: bool,
}

/// A document or the problems that keep it from building, one per line.
fn compiled(doc: Doc) -> Result<PropDef> {
    PropDef::new(doc).map_err(|issues| anyhow!(issue::render(&issues)))
}

/// A definition's document as its canonical file writes it.
fn document(def: &PropDef) -> Result<Value> {
    serde_json::from_str(&def.doc.text()).context("canonical asset text")
}

/// Asset files use this root in the app, CLI, live bridge and file watcher.
pub fn root() -> PathBuf {
    std::env::var_os("PAV_ASSETS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets/props"))
}

fn asset_path(root: &Path, name: &str) -> PathBuf {
    root.join("workshop").join(format!("{}.json", name.split_once('/').expect("canonical name").1))
}

fn history_path(root: &Path, name: &str) -> PathBuf {
    root.join(".editor").join(format!("{}.json", name.split_once('/').expect("canonical name").1))
}

fn lock_editor(root: &Path) -> Result<File> {
    let dir = root.join(".editor");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    file.lock().context("lock the asset editor")?;
    Ok(file)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let name = path.file_name().ok_or_else(|| anyhow!("{} has no file name", path.display()))?.to_string_lossy();
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.{}.{id}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes).with_context(|| format!("write {}", temp.display()))?;
        file.sync_all().with_context(|| format!("flush {}", temp.display()))?;
        drop(file);
        std::fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn file_definition(path: &Path) -> Result<(String, PropDef)> {
    let leaf = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| anyhow!("invalid asset file name"))?;
    props::validate_name(leaf).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let doc = Doc::read(&text).map_err(|issues| anyhow!("{}: {}", path.display(), issue::render(&issues)))?;
    if doc.name != leaf {
        bail!("{} must contain name '{leaf}', but it contains '{}'", path.display(), doc.name);
    }
    let def = compiled(doc).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    Ok((format!("WORKSHOP/{leaf}"), def))
}

/// Install a definition unless the library has it already. Returns it and the definitions
/// that reference it, recompiled with it: every one whose placed copies need a refresh.
fn publish_if_changed(name: &str, def: PropDef) -> Result<Vec<(String, Arc<PropDef>)>> {
    if props::get(name).is_some_and(|current| current.key == def.key) {
        return Ok(Vec::new());
    }
    let dependents = props::install(name, def).map_err(|e| anyhow!(e))?;
    Ok(std::iter::once(name.to_owned()).chain(dependents).filter_map(|name| Some((name.clone(), props::get(&name)?))).collect())
}

/// Read one workshop JSON file. Invalid content keeps the last good library definition.
/// Ignored paths and definitions already in the library return nothing; otherwise the asset
/// comes first, then the assets that reference it. The lock also prevents a watcher from
/// publishing an older read after an accepted tool edit.
pub fn reload_file(path: &Path) -> Result<Vec<(String, Arc<PropDef>)>> {
    let Some(folder) = path.parent() else { return Ok(Vec::new()) };
    if folder.file_name().is_none_or(|name| name != "workshop")
        || path.extension().is_none_or(|ext| ext != "json")
        || path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return Ok(Vec::new());
    }
    let root = folder.parent().ok_or_else(|| anyhow!("{} has no asset root", path.display()))?;
    let _lock = lock_editor(root)?;
    if !path.try_exists().with_context(|| format!("check {}", path.display()))? {
        let leaf = path.file_stem().and_then(|name| name.to_str()).ok_or_else(|| anyhow!("invalid asset file name"))?;
        props::validate_name(leaf).map_err(|e| anyhow!(e))?;
        if props::get(&format!("WORKSHOP/{leaf}")).is_none() {
            // A tool undo already removed this definition. Its watcher event is redundant.
            return Ok(Vec::new());
        }
    }
    let (name, def) = file_definition(path)?;
    publish_if_changed(&name, def)
}

/// Initialize disk assets once per process, before the first scene is constructed. Later
/// temporary sessions must not consume a file change that the live watcher will apply.
pub fn initialize_authored() -> Result<()> {
    let mut result = Ok(());
    INITIALIZED.call_once(|| result = reload_authored().map(|_| ()));
    result
}

/// Load saved definitions after a restart. Each file is independent: valid files load, and
/// invalid files retain their prior definitions. The error lists files that need a fix.
pub fn reload_authored() -> Result<Vec<(String, Arc<PropDef>)>> {
    reload_authored_at(&root())
}

fn reload_authored_at(root: &Path) -> Result<Vec<(String, Arc<PropDef>)>> {
    let folder = root.join("workshop");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let _lock = lock_editor(root)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&folder).with_context(|| format!("read {}", folder.display()))? {
        let path = entry?.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "json")
            && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.'))
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut changed = Vec::new();
    // A file whose references load later in the folder waits for them: passes repeat while
    // they make progress.
    let mut errors = Vec::new();
    loop {
        errors.clear();
        let tried = paths.len();
        for path in std::mem::take(&mut paths) {
            match file_definition(&path).and_then(|(name, def)| publish_if_changed(&name, def)) {
                Ok(published) => changed.extend(published),
                Err(error) => {
                    errors.push(format!("{error:#}"));
                    paths.push(path);
                }
            }
        }
        if paths.is_empty() || paths.len() == tried {
            break;
        }
    }
    if !errors.is_empty() {
        bail!("some asset files did not load; their last good definitions remain: {}", errors.join("; "));
    }
    Ok(changed)
}

fn read_asset(root: &Path, name: &str) -> Result<Option<Arc<PropDef>>> {
    if name.starts_with("BUILTIN/") {
        return Ok(props::get(name));
    }
    let path = asset_path(root, name);
    if !path.exists() {
        return Ok(None);
    }
    file_definition(&path).map(|(_, def)| Some(Arc::new(def)))
}

fn revision(def: Option<&PropDef>) -> String {
    def.map(|def| def.revision.clone()).unwrap_or_else(|| "absent".into())
}

fn read_history(path: &Path, current: &str) -> Result<History> {
    if !path.exists() {
        return Ok(History { head: current.into(), ..Default::default() });
    }
    let history: History =
        serde_json::from_slice(&std::fs::read(path)?).with_context(|| format!("read asset history {}", path.display()))?;
    if history.undo.len() > HISTORY_LIMIT || history.redo.len() > HISTORY_LIMIT {
        bail!("asset history {} exceeds {HISTORY_LIMIT} steps", path.display());
    }
    Ok(history)
}

fn push_history(stack: &mut Vec<Option<Value>>, document: Option<Value>) {
    stack.push(document);
    if stack.len() > HISTORY_LIMIT {
        stack.remove(0);
    }
}

/// The edit reply contract: identity, size and the undo/redo steps available (left out when
/// none). Callers add what the action changed.
fn report(name: &str, def: Option<&PropDef>, history: &History, saved: bool) -> Value {
    let revision = revision(def);
    let current = history.head == revision;
    let mut result = json!({"name": name, "revision": revision, "saved": saved});
    for (key, steps) in [("undo", &history.undo), ("redo", &history.redo)] {
        if current && !steps.is_empty() {
            result[key] = json!(steps.len());
        }
    }
    if let Some(def) = def {
        result["size_cm"] = json!(def.bounds(1.0).size_cm());
    }
    result
}

/// Bounds in metres, rounded to millimetres: `[[min x,y,z],[max x,y,z]]`.
fn bounds_mm(bounds: props::Bounds) -> Value {
    let mm = |v: glam::Vec3| v.to_array().map(|x| (x * 1000.0).round() / 1000.0);
    json!([mm(bounds.min), mm(bounds.max)])
}

/// What `inspect` adds on request.
struct Extras {
    legend: bool,
    bounds: bool,
    resolved: bool,
}

/// The definition as its canonical file writes it: defaults left out. The legend, bounds and
/// resolved parts are opt-in.
fn inspect(root: &Path, name: &str, preview: bool, extras: Extras) -> Result<Edited> {
    let def = read_asset(root, name)?;
    let history_file = history_path(root, name);
    if def.is_none() && (!name.starts_with("WORKSHOP/") || !history_file.is_file()) {
        bail!("no asset '{name}'; assets find=WORDS searches the library");
    }
    let history =
        if name.starts_with("WORKSHOP/") { read_history(&history_file, &revision(def.as_deref()))? } else { History::default() };
    let mut result = report(name, def.as_deref(), &history, false);
    result.as_object_mut().expect("report is an object").remove("saved");
    result["asset"] = match &def {
        Some(def) => document(def)?,
        None => Value::Null,
    };
    if let (Some(def), true) = (&def, extras.bounds) {
        // Resolved parts: a format-1 part is its own name; copies and mirrors carry suffixes.
        result["bounds"] = bounds_mm(def.bounds(1.0));
        result["part_bounds"] =
            Value::Object(def.compiled.parts.iter().map(|part| (part.id.clone(), bounds_mm(part.bounds(1.0)))).collect());
    }
    if let (Some(def), true) = (&def, extras.resolved) {
        result["resolved"] = resolved(def);
    }
    if extras.legend {
        result["legend"] = json!(props::LEGEND);
    }
    Ok(Edited { report: result, def, dependents: Vec::new(), mutation: false, preview })
}

/// Each resolved part in the document's unit: its type and size, and its ends for a part
/// placed by them, else its position and yaw, pitch and roll (zeros left out). A mirror copy
/// says so: no turn draws it.
fn resolved(def: &PropDef) -> Value {
    let per_metre = def.doc.unit.per_metre();
    let digits = if def.doc.unit == props::doc::Unit::Cm { 100.0 } else { 10_000.0 };
    let length = |x: f32| ((x * per_metre) as f64 * digits).round() / digits;
    let point = |v: glam::Vec3| json!(v.to_array().map(length));
    let angle = |x: f32| (x.to_degrees() as f64 * 100.0).round() / 100.0;
    let parts = def.compiled.parts.iter().map(|part| {
        let (centre, half) = part.prim.local_box();
        let mut row = json!({"type": part.prim.type_name(), "size": point(half * 2.0 * part.scale)});
        if part.ends {
            // The ends lie on the axis, wherever an arc's box sits beside it.
            let end = |sign: f32| part.pos + part.rot * (glam::Vec3::Y * (centre.y + half.y * sign) * part.scale);
            row["from"] = point(end(-1.0));
            row["to"] = point(end(1.0));
        } else {
            row["pos"] = point(part.pos);
            let (yaw, pitch, roll) = part.rot.to_euler(glam::EulerRot::YXZ);
            for (key, value) in [("yaw", yaw), ("pitch", pitch), ("roll", roll)] {
                if angle(value) != 0.0 {
                    row[key] = json!(angle(value));
                }
            }
        }
        if part.mirrored {
            row["mirrored"] = json!(true);
        }
        (part.id.clone(), row)
    });
    Value::Object(parts.collect())
}

/// A batch format 1 reads as its own: set, add and remove of whole format-1 parts and their
/// fields. Anything else goes through the format-2 view of the document.
fn format1_batch(ops: &Value) -> bool {
    let Some(ops) = ops.as_array() else { return true };
    let part_fields = |object: &serde_json::Map<String, Value>| {
        object.keys().all(|key| props::PART_FIELDS.iter().any(|(field, _)| field == key))
    };
    ops.iter().all(|op| {
        let Some(op) = op.as_object() else { return true };
        let plain_part = op.get("part").and_then(Value::as_str).is_some_and(|part| !part.contains('.'));
        let fields = match (op.get("op").and_then(Value::as_str), op.get("fields"), op.get("value")) {
            (Some("set"), Some(Value::Object(fields)), None) => part_fields(fields),
            (Some("add"), None, Some(Value::Object(value))) => part_fields(value),
            (Some("remove"), None, None) => true,
            _ => false,
        };
        plain_part && fields && op.keys().all(|key| ["op", "part", "fields", "value"].contains(&key.as_str()))
    })
}

/// A JSON argument: an object, or text read strictly (a repeated key is an error).
fn json_arg(a: &Args, key: &str) -> Result<Value> {
    match a.get(key).ok_or_else(|| anyhow!("{key}= is required"))? {
        Value::String(text) => props::strict_value(text).map_err(|mut issue| {
            if key == "ops" {
                issue.path = format!("ops{}", if issue.path.starts_with('[') { issue.path.clone() } else { String::new() });
            }
            anyhow!("{issue}")
        }),
        value => Ok(value.clone()),
    }
}

const ACTIONS: &[&str] = &["create", "copy", "inspect", "check", "patch", "replace", "undo", "redo", "schema"];

fn check_args(a: &Args) -> Result<(&str, bool)> {
    let preview = get_bool(a, "preview", true)?;
    if a.get("action").is_some_and(|value| !value.is_string()) {
        bail!("action must be text");
    }
    let action = get_str(a, "action").unwrap_or("inspect");
    let extra: &[&str] = match action {
        "create" => &["template", "asset", "description", "if_revision", "dry_run"],
        "copy" => &["from", "description", "if_revision", "dry_run"],
        "inspect" => &["legend", "bounds", "resolved"],
        "check" => &["target", "all"],
        "patch" => &["ops", "if_revision", "dry_run"],
        "replace" => &["asset", "if_revision", "dry_run"],
        "undo" | "redo" => &["if_revision"],
        "schema" => &["topic", "format"],
        _ => bail!(
            "{}",
            Issue::error("action", Code::UnknownValue, format!("unknown action \"{action}\""))
                .expected(ACTIONS.join(", "))
                .maybe_fix(issue::suggestion(action, ACTIONS.iter().copied()))
        ),
    };
    let accepted: Vec<&str> = ["action", "name", "preview"].into_iter().chain(extra.iter().copied()).collect();
    for (key, value) in a {
        if !accepted.contains(&key.as_str()) && !["scene", "seed", "ticks"].contains(&key.as_str()) {
            let known = [
                "from",
                "template",
                "asset",
                "description",
                "if_revision",
                "ops",
                "legend",
                "bounds",
                "resolved",
                "topic",
                "format",
                "dry_run",
                "target",
                "all",
            ];
            let fix = if known.contains(&key.as_str()) {
                Some(format!("{key} belongs to another action"))
            } else {
                issue::suggestion(key, accepted.iter().copied())
            };
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, format!("not an asset_edit argument for action={action}"))
                    .expected(accepted.join(", "))
                    .maybe_fix(fix)
            );
        }
        if ["action", "name", "from", "template", "description", "if_revision", "topic", "target"].contains(&key.as_str())
            && !value.is_string()
        {
            bail!("{key} must be text");
        }
    }
    if a.contains_key("template") && a.contains_key("asset") {
        bail!("create accepts template or asset, not both");
    }
    Ok((action, preview))
}

fn definition(value: Value, name: &str, creating: bool) -> Result<Doc> {
    let mut object = value.as_object().ok_or_else(|| anyhow!("asset must be a JSON object"))?.clone();
    let leaf = name.split_once('/').expect("canonical name").1;
    if creating {
        object.entry("format").or_insert(json!(1));
        object.insert("name".into(), json!(leaf));
    }
    let doc = Doc::from_value(&Value::Object(object)).map_err(|issues| anyhow!(issue::render(&issues)))?;
    if doc.name != leaf {
        bail!("asset.name must stay '{leaf}'; use action=copy to make a new asset");
    }
    Ok(doc)
}

/// What drawing an asset costs on the CPU side: triangles of its meshes, distinct meshes, SDF
/// parts, lights, and the kilobytes of meshes generated for it.
struct Cost {
    triangles: usize,
    meshes: usize,
    sdf: usize,
    lights: usize,
    generated_kb: usize,
}

impl Cost {
    fn of(compiled: &CompiledProp) -> Self {
        let mut meshes: BTreeMap<MeshKey, usize> = BTreeMap::new();
        let mut cost = Cost { triangles: 0, meshes: 0, sdf: 0, lights: compiled.lights.len(), generated_kb: 0 };
        let mut generated_bytes = 0;
        let vertex = std::mem::size_of::<pav_render::mesh::Vertex>();
        for part in compiled.parts.iter().filter(|part| !part.hidden) {
            let generated = Generated::of(&part.prim, part.mirrored);
            let key = match (&generated, &part.prim) {
                (Some(generated), _) => Some(MeshKey::Custom(generated.key)),
                (None, Primitive::Box { .. }) => Some(MeshKey::Cube),
                (None, Primitive::RoundedBox { half, radius }) => {
                    Some(MeshKey::rounded_box(*half * part.scale, radius * part.scale))
                }
                (None, Primitive::Ellipsoid { .. }) => Some(MeshKey::Sphere),
                (None, Primitive::Cylinder { radius_end, .. }) if *radius_end == 0.0 => Some(MeshKey::Cone),
                (None, Primitive::Cylinder { .. }) => Some(MeshKey::Cylinder),
                _ => None,
            };
            match key {
                Some(key) => {
                    let triangles = *meshes.entry(key).or_insert_with(|| match (&generated, key.generate()) {
                        (Some(generated), _) => {
                            let mesh = generated.build();
                            generated_bytes += mesh.positions.len() * vertex + mesh.indices.len() * 4;
                            mesh.triangles()
                        }
                        (None, Some(mesh)) => {
                            if matches!(key, MeshKey::RoundedBox { .. }) {
                                generated_bytes += mesh.vertices.len() * vertex + mesh.indices.len() * 4;
                            }
                            mesh.indices.len() / 3
                        }
                        (None, None) => 0,
                    });
                    cost.triangles += triangles;
                }
                None => cost.sdf += 1,
            }
            cost.lights += usize::from(part.glow_light && part.emissive > 0.5);
        }
        cost.meshes = meshes.len();
        cost.generated_kb = generated_bytes.div_ceil(1024);
        cost
    }

    fn line(&self) -> String {
        format!(
            "{} tris, {} meshes, {} sdf, {} lights, {} KB generated",
            self.triangles, self.meshes, self.sdf, self.lights, self.generated_kb
        )
    }
}

/// The lint (with the compile warnings) plus the budget findings that need the renderer's
/// meshes.
fn findings(compiled: &CompiledProp, target: Option<[Option<f32>; 3]>) -> (Vec<Finding>, Cost) {
    let mut found = lint::lint(compiled, &lint::Options { target_cm: target });
    let cost = Cost::of(compiled);
    let mut over = |code: &'static str, fact: String| found.push(Finding { code, ids: vec![], line: format!("{code}: {fact}") });
    if cost.triangles > 20_000 {
        over("heavy", format!("{} triangles (budget 20000)", cost.triangles));
    }
    if cost.meshes > 8 {
        over(
            "meshes",
            format!("{} distinct meshes (budget 8); reuse the sizes of rounded boxes and generated shapes", cost.meshes),
        );
    }
    if cost.lights > 4 {
        over("lights", format!("{} lights from light fields and parts above emissive 0.5 (budget 4)", cost.lights));
    }
    (found, cost)
}

/// Findings of `after` that `before` did not have: what an edit introduced.
fn new_findings(before: Option<&PropDef>, after: Option<&PropDef>) -> Vec<String> {
    let Some(after) = after else { return Vec::new() };
    let old: BTreeSet<String> =
        before.map(|before| findings(&before.compiled, None).0.iter().map(Finding::key).collect()).unwrap_or_default();
    let new: Vec<Finding> = findings(&after.compiled, None).0.into_iter().filter(|f| !old.contains(&f.key())).collect();
    lint::lines(&new)
}

/// `"220x90x-"`: a target size in cm; `-` skips an axis.
fn target_arg(a: &Args) -> Result<Option<[Option<f32>; 3]>> {
    let Some(text) = get_str(a, "target") else { return Ok(None) };
    let axes: Vec<&str> = text.split(['x', 'X']).collect();
    let bad = || anyhow!("target is WxHxD in cm, e.g. 200x78x80 (- skips an axis)");
    if axes.len() != 3 {
        return Err(bad());
    }
    let mut out = [None; 3];
    for (slot, axis) in out.iter_mut().zip(axes) {
        if axis.trim() != "-" {
            *slot = Some(axis.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0).ok_or_else(bad)?);
        }
    }
    Ok(Some(out))
}

/// The authored parts (top-level nodes) an edit added, removed or changed.
fn changed_parts(before: Option<&Doc>, after: Option<&Doc>) -> Vec<String> {
    let names = before
        .into_iter()
        .flat_map(|doc| doc.parts.keys())
        .chain(after.into_iter().flat_map(|doc| doc.parts.keys()))
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .filter(|name| before.and_then(|doc| doc.parts.get(*name)) != after.and_then(|doc| doc.parts.get(*name)))
        .cloned()
        .collect()
}

/// Disk editing stays independent of a GPU or active preview.
fn edit_at(root: &Path, a: &Args) -> Result<Edited> {
    let (action, preview) = check_args(a)?;
    let read_only = |report| Ok(Edited { report, def: None, dependents: Vec::new(), mutation: false, preview: false });
    if action == "schema" {
        let guide = match a.get("format") {
            None => props::schema(get_str(a, "topic")),
            Some(format) if format == 1 => props::schema(get_str(a, "topic")),
            Some(format) if format == 2 => props::format2::schema(get_str(a, "topic")),
            Some(format) => {
                bail!("{}", Issue::error("format", Code::UnknownValue, format!("format {format}")).expected("1 or 2"))
            }
        };
        return read_only(guide.map_err(|e| anyhow!(e))?);
    }
    let name = props::canonical(get_str(a, "name").ok_or_else(|| anyhow!("name= names the asset"))?).map_err(|e| anyhow!(e))?;
    if action == "inspect" {
        let extras = Extras {
            legend: get_bool(a, "legend", false)?,
            bounds: get_bool(a, "bounds", false)?,
            resolved: get_bool(a, "resolved", false)?,
        };
        return inspect(root, &name, preview, extras);
    }
    if action == "check" {
        let def = read_asset(root, &name)?.ok_or_else(|| anyhow!("no asset '{name}'; assets find=WORDS searches the library"))?;
        let (found, cost) = findings(&def.compiled, target_arg(a)?);
        let mut result =
            json!({"name": name, "revision": def.revision, "size_cm": def.bounds(1.0).size_cm(), "cost": cost.line()});
        if !found.is_empty() {
            result["check"] = if get_bool(a, "all", false)? {
                json!(found.iter().map(|f| &f.line).collect::<Vec<_>>())
            } else {
                json!(lint::lines(&found))
            };
        }
        return read_only(result);
    }
    if !name.starts_with("WORKSHOP/") {
        bail!("built-in templates are read-only; use action=copy from={name} name=your_name first");
    }
    let leaf = name.split_once('/').expect("canonical name").1;
    let _lock = lock_editor(root)?;
    let before = read_asset(root, &name)?;
    let current_revision = revision(before.as_deref());
    if let Some(expected) = get_str(a, "if_revision") {
        if expected != current_revision {
            bail!(
                "revision conflict for {name}: expected {expected}, current {current_revision}; inspect the asset and apply the edit again"
            );
        }
    }
    if matches!(action, "create" | "copy") && before.is_some() {
        bail!("{name} already exists; choose a new name or edit the existing asset");
    }
    let history_file = history_path(root, &name);
    let old_history = if history_file.is_file() { Some(std::fs::read(&history_file)?) } else { None };
    let mut history = read_history(&history_file, &current_revision)?;
    let history_reset = history.head != current_revision;
    if history_reset {
        if matches!(action, "undo" | "redo") {
            bail!("the asset changed outside this editor; its old history does not match revision {current_revision}");
        }
        history = History { head: current_revision.clone(), ..Default::default() };
    }
    let before_document = before.as_deref().map(document).transpose()?;
    // A restored step runs through migrate and the checks like any other document.
    let restore = |step: Option<Value>| -> Result<Option<Doc>> {
        step.map(|value| Doc::from_value(&value).map_err(|issues| anyhow!("saved history: {}", issue::render(&issues))))
            .transpose()
    };
    // A format-2 batch, kept to name its operations in compile errors too.
    let mut patched: Option<props::Patched> = None;
    let next = match action {
        "create" | "copy" => {
            let mut doc = if action == "create" && a.contains_key("asset") {
                definition(json_arg(a, "asset")?, &name, true)?
            } else {
                let source = if action == "copy" {
                    props::canonical(get_str(a, "from").ok_or_else(|| anyhow!("from= names the source asset"))?)
                        .map_err(|e| anyhow!(e))?
                } else {
                    let template = get_str(a, "template").unwrap_or("box");
                    let template = if template.contains('/') { template.into() } else { format!("BUILTIN/{template}") };
                    let template = props::canonical(&template).map_err(|e| anyhow!(e))?;
                    if !template.starts_with("BUILTIN/") {
                        bail!("template names a BUILTIN asset; use action=copy for workshop assets");
                    }
                    template
                };
                let source = read_asset(root, &source)?.ok_or_else(|| anyhow!("no source asset '{source}'"))?;
                source.doc.clone()
            };
            doc.name = leaf.into();
            if let Some(description) = get_str(a, "description") {
                doc.description = description.into();
            }
            Some(doc)
        }
        "undo" => {
            let next = history.undo.pop().ok_or_else(|| anyhow!("no edit to undo for {name}"))?;
            push_history(&mut history.redo, before_document.clone());
            restore(next)?
        }
        "redo" => {
            let next = history.redo.pop().ok_or_else(|| anyhow!("no edit to redo for {name}"))?;
            push_history(&mut history.undo, before_document.clone());
            restore(next)?
        }
        action => {
            let current = before.as_ref().ok_or_else(|| anyhow!("no asset {name}; use action=create or action=copy first"))?;
            Some(match action {
                "patch" => {
                    let ops = json_arg(a, "ops")?;
                    match &current.v1 {
                        // Format 1's own batches keep format 1's checks and messages.
                        Some(asset) if format1_batch(&ops) => Doc::from_v1(&asset.patch(&ops).map_err(|e| anyhow!(e))?),
                        _ => {
                            let result = props::patch_document(&current.doc, &ops, &props::get)
                                .map_err(|issues| anyhow!(issue::render(&issues)))?;
                            let doc = result.doc.clone();
                            patched = Some(result);
                            doc
                        }
                    }
                }
                "replace" => definition(json_arg(a, "asset")?, &name, false)?,
                _ => unreachable!("action was checked"),
            })
        }
    };
    if next.as_ref().is_some_and(|doc| doc.name != leaf) {
        bail!("saved history has a different asset name; cannot restore it as {name}");
    }
    let next = match &patched {
        Some(patched) => {
            next.map(|doc| PropDef::new(doc).map_err(|issues| anyhow!(issue::render(&patched.readdress(issues))))).transpose()?
        }
        None => next.map(compiled).transpose()?,
    };
    // What the registry would refuse (a reference back to this asset, or removing one that
    // others reference) changes nothing on disk either.
    match &next {
        Some(def) => props::check_install(&name, def),
        None if before.is_some() => props::check_remove(&name),
        None => Ok(()),
    }
    .map_err(|e| anyhow!(e))?;
    let next_revision = revision(next.as_ref());
    let changed_parts = changed_parts(before.as_ref().map(|def| &def.doc), next.as_ref().map(|def| &def.doc));
    let check = new_findings(before.as_deref(), next.as_ref());
    if get_bool(a, "dry_run", false)? {
        // Everything an accepted edit would report; nothing is written or published.
        let mut result = report(&name, before.as_deref(), &history, false);
        result["revision_next"] = json!(next_revision);
        if let Some(def) = &next {
            result["size_cm"] = json!(def.bounds(1.0).size_cm());
        }
        if matches!(action, "create" | "copy") {
            result["parts"] = json!(next.as_ref().map(|def| def.compiled.parts.len()).unwrap_or(0));
        } else {
            result["changed_parts"] = json!(changed_parts);
        }
        if !check.is_empty() {
            result["check"] = json!(check);
        }
        return read_only(result);
    }
    if next_revision == current_revision {
        let mut result = report(&name, before.as_deref(), &history, false);
        result["changed_parts"] = json!(changed_parts);
        return Ok(Edited { report: result, def: before, dependents: Vec::new(), mutation: true, preview });
    }
    if !matches!(action, "undo" | "redo") {
        push_history(&mut history.undo, before_document);
        history.redo.clear();
    }
    history.head = next_revision;
    let path = asset_path(root, &name);
    let old_asset = if path.is_file() { Some(std::fs::read(&path)?) } else { None };
    // A failed save or publish puts the files back: the history always, the asset once written.
    let put_back = |error: anyhow::Error, asset_written: bool| -> anyhow::Error {
        let restore = |file: &Path, old: Option<&[u8]>| match old {
            Some(bytes) => atomic_write(file, bytes),
            None => std::fs::remove_file(file).map_err(anyhow::Error::from),
        };
        let restored = restore(&history_file, old_history.as_deref());
        if !asset_written {
            return match restored {
                Ok(()) => error,
                Err(restore_error) => error.context(format!("the asset is unchanged; history restore failed: {restore_error}")),
            };
        }
        match restored.and_then(|()| restore(&path, old_asset.as_deref())) {
            Ok(()) => error,
            Err(restore_error) => error.context(format!("restoring the asset file and its history failed: {restore_error}")),
        }
    };
    atomic_write(&history_file, &serde_json::to_vec(&history)?)?;
    let save = match &next {
        Some(def) => atomic_write(&path, def.doc.text().as_bytes()),
        None => std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display())),
    };
    if let Err(error) = save {
        return Err(put_back(error, false));
    }
    let parts = next.as_ref().map(|def| def.compiled.parts.len()).unwrap_or(0);
    let published = match next {
        Some(def) => props::install(&name, def),
        None => props::remove(&name).map(|_| Vec::new()),
    };
    let dependents = published.map_err(|error| put_back(anyhow!(error), true))?;
    let def = props::get(&name);
    let mut result = report(&name, def.as_deref(), &history, true);
    if matches!(action, "create" | "copy") {
        result["parts"] = json!(parts);
        result["file"] = json!(path);
    } else {
        result["changed_parts"] = json!(changed_parts);
    }
    if history_reset {
        result["history_reset"] = json!(true);
    }
    if !check.is_empty() {
        result["check"] = json!(check);
    }
    Ok(Edited { report: result, def, dependents, mutation: true, preview })
}

/// A listing row's description: the start, cut at a word.
fn summary(text: &str) -> String {
    const MAX: usize = 80;
    if text.len() <= MAX {
        return text.into();
    }
    let mut end = MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = text[..end].rfind(' ').filter(|at| *at > MAX / 2).unwrap_or(end);
    format!("{}…", text[..cut].trim_end_matches([',', ';', ':', ' ']))
}

/// Search the registry. Rows are compact; `asset_edit inspect` returns one definition.
pub fn t_assets(_: &mut Session, a: &Args) -> Result<Output> {
    for key in a.keys() {
        if !["find", "limit", "page", "scene", "seed", "ticks"].contains(&key.as_str()) {
            let accepted = ["kind", "find", "limit", "page"];
            bail!(
                "{}",
                Issue::error(key.as_str(), Code::UnknownKey, "not an assets argument")
                    .expected(accepted.join(", "))
                    .maybe_fix(issue::suggestion(key, accepted))
            );
        }
    }
    if a.get("find").is_some_and(|value| !value.is_string()) {
        bail!("find must be text");
    }
    let limit = match a.get("limit") {
        None => 30,
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .filter(|value| (1..=1000).contains(value))
            .ok_or_else(|| anyhow!("limit must be an integer from 1 to 1000"))?,
    } as usize;
    let page = match a.get("page") {
        None => 1,
        Some(value) => value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .filter(|value| *value >= 1)
            .ok_or_else(|| anyhow!("page must be an integer from 1"))?,
    } as usize;
    let find = get_str(a, "find").unwrap_or("").to_ascii_lowercase();
    let library = props::library();
    let matched = library
        .defs
        .iter()
        .filter(|(name, def)| {
            name.to_ascii_lowercase().contains(&find) || def.doc.description.to_ascii_lowercase().contains(&find)
        })
        .collect::<Vec<_>>();
    let skip = (page - 1).saturating_mul(limit);
    let rows = matched
        .iter()
        .skip(skip)
        .take(limit)
        .map(|(name, def)| json!([name, def.bounds(1.0).size_cm(), def.compiled.parts.len(), summary(&def.doc.description)]))
        .collect::<Vec<_>>();
    let mut result = json!({
        "cols": "name size_cm parts desc",
        "rows": rows,
        "total": library.defs.len(),
        "matched": matched.len(),
    });
    if skip.saturating_add(limit) < matched.len() {
        result["next"] = json!(page + 1);
    }
    Ok(Output::Json(result))
}

/// Create, inspect and alter an asset. Options are parsed before disk mutation; a later
/// preview failure is reported separately from an accepted, saved definition.
pub fn t_asset_edit(s: &mut Session, a: &Args) -> Result<Output> {
    let Edited { mut report, def, dependents, mutation, preview } = edit_at(&root(), a)?;
    if mutation {
        let name = report["name"].as_str().expect("report has canonical name").to_owned();
        let saved = report["saved"] == true;
        let mut refreshed = 0;
        if saved {
            refreshed += def.as_ref().map(|def| s.sim.refresh_prop_instances(&name, def.clone())).unwrap_or(0);
            // Assets that reference this one were recompiled with it; so are their copies.
            for dependent in &dependents {
                if let Some(def) = props::get(dependent) {
                    refreshed += s.sim.refresh_prop_instances(dependent, def);
                }
            }
        }
        if refreshed > 0 {
            report["refreshed_instances"] = json!(refreshed);
        }
        let mut args = Args::new();
        if def.is_some() && preview {
            args.insert("name".into(), json!(name));
        } else if def.is_none() && saved {
            // Undo of creation leaves placed snapshot copies intact and closes only this
            // missing asset's active stage. A different preview remains selected.
            if let Ok(Output::Json(status)) = crate::asset_preview_tools::t_asset_preview(s, &Args::new()) {
                if status["open"] == true && status["name"] == name {
                    args.insert("close".into(), json!(true));
                }
            }
        }
        if !args.is_empty() {
            if let Err(error) = crate::asset_preview_tools::t_asset_preview(s, &args) {
                report["preview_error"] = json!(format!("{error:#}"));
            }
        }
    }
    Ok(Output::Json(report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> Args {
        value.as_object().unwrap().clone()
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("pav-asset-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn edit(root: &Path, value: Value) -> Value {
        edit_at(root, &args(value)).unwrap().report
    }

    #[test]
    fn batches_are_atomic_guarded_and_reversible_across_restart() {
        let root = temp_root("transactions");
        let name = "test_asset_transactions";
        let canonical = format!("WORKSHOP/{name}");
        let first = edit(&root, json!({"action":"create","name":name,"template":"bench"}));
        let path = asset_path(&root, &canonical);
        let original_bytes = std::fs::read(&path).unwrap();
        let history_bytes = std::fs::read(history_path(&root, &canonical)).unwrap();
        let original = props::get(&canonical).unwrap();
        let failed = edit_at(
            &root,
            &args(json!({"action":"patch","name":name,"ops":[
                {"op":"set","part":"seat","fields":{"color":"#ffffff"}},
                {"op":"remove","part":"missing"}
            ]})),
        );
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        assert_eq!(std::fs::read(history_path(&root, &canonical)).unwrap(), history_bytes);
        assert!(Arc::ptr_eq(&original, &props::get(&canonical).unwrap()));
        let changed = edit(
            &root,
            json!({"action":"patch","name":name,"if_revision":first["revision"],"ops":[
                {"op":"set","part":"seat","fields":{"color":"#4060a0"}},
                {"op":"set","part":"backrest","fields":{"pitch":-20}},
                {"op":"add","part":"brace","value":{"shape":{"type":"box","half":[0.6,0.05,0.05]}}}
            ]}),
        );
        assert_eq!(changed["undo"], 2, "one batch is one history entry");
        assert_eq!(changed["changed_parts"], json!(["backrest", "brace", "seat"]));
        assert!(changed.get("asset").is_none(), "edit replies omit complete definitions");
        let stale = edit_at(&root, &args(json!({"action":"patch","name":name,"if_revision":first["revision"],"ops":[]})));
        assert!(stale.err().unwrap().to_string().contains("revision conflict"));
        props::remove(&canonical).unwrap();
        assert!(props::get(&canonical).is_none());
        let loaded = reload_authored_at(&root).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(props::get(&canonical).unwrap().revision, changed["revision"]);
        assert!(reload_file(&path).unwrap().is_empty(), "a watcher does not publish the tool edit twice");
        let undone = edit(&root, json!({"action":"undo","name":name,"if_revision":changed["revision"]}));
        assert_eq!(undone["revision"], first["revision"]);
        let removed = edit(&root, json!({"action":"undo","name":name,"if_revision":undone["revision"]}));
        assert_eq!(removed["revision"], "absent");
        assert!(!path.exists() && props::get(&canonical).is_none());
        assert!(reload_file(&path).unwrap().is_empty(), "a tool removal does not become a watcher error");
        let absent = edit(&root, json!({"action":"inspect","name":name}));
        assert!(absent["asset"].is_null());
        assert_eq!(absent["redo"], 2);
        let restored = edit(&root, json!({"action":"redo","name":name,"if_revision":"absent"}));
        assert_eq!(restored["revision"], first["revision"]);
        let final_state = edit(&root, json!({"action":"redo","name":name,"if_revision":restored["revision"]}));
        assert_eq!(final_state["revision"], changed["revision"]);
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_edits_keep_last_good_data_and_reset_only_matching_history() {
        let root = temp_root("external");
        let name = "test_asset_external";
        let canonical = format!("WORKSHOP/{name}");
        let created = edit(&root, json!({"action":"copy","name":name,"from":"BUILTIN/lantern"}));
        let builtin = props::get("BUILTIN/lantern").unwrap();
        let path = asset_path(&root, &canonical);
        std::fs::write(&path, "{ unfinished").unwrap();
        assert!(reload_file(&path).is_err());
        assert_eq!(props::get(&canonical).unwrap().revision, created["revision"]);
        let mut external = (**props::get(&canonical).unwrap().v1.as_ref().unwrap()).clone();
        external.parts.get_mut("post").unwrap().color = "#112233".into();
        std::fs::write(&path, external.text()).unwrap();
        assert_eq!(reload_file(&path).unwrap().len(), 1);
        let undo = edit_at(&root, &args(json!({"action":"undo","name":name,"if_revision":external.revision()})));
        assert!(undo.err().unwrap().to_string().contains("outside this editor"));
        let next = edit(
            &root,
            json!({"action":"patch","name":name,"if_revision":external.revision(),"ops":[
                {"op":"set","part":"post","fields":{"color":"#446688"}}
            ]}),
        );
        assert_eq!(next["history_reset"], true);
        assert_eq!(next["undo"], 1);
        let undo = edit(&root, json!({"action":"undo","name":name}));
        assert_eq!(undo["revision"], external.revision());
        assert!(Arc::ptr_eq(&builtin, &props::get("BUILTIN/lantern").unwrap()));
        assert!(reload_file(&root.join(".editor/unrelated.json")).unwrap().is_empty());
        assert!(reload_file(&root.join("workshop/unfinished.tmp")).unwrap().is_empty());
        assert!(reload_file(&root.join("workshop/.hidden.json")).unwrap().is_empty());
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replies_carry_identity_size_and_only_what_changed() {
        let root = temp_root("replies");
        let name = "test_asset_replies";
        let keys = |value: &Value| value.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        let copied = edit(&root, json!({"action":"copy","name":name,"from":"BUILTIN/bench"}));
        assert_eq!(keys(&copied), ["file", "name", "parts", "revision", "saved", "size_cm", "undo"]);
        assert_eq!((copied["parts"].as_u64(), copied["size_cm"].as_str()), (Some(7), Some("220x143x86")));
        let patched =
            edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"seat","fields":{"color":"#3e9a9e"}}]}));
        assert_eq!(keys(&patched), ["changed_parts", "name", "revision", "saved", "size_cm", "undo"]);
        let inspected = edit(&root, json!({"action":"inspect","name":"BUILTIN/bench"}));
        assert_eq!(keys(&inspected), ["asset", "name", "revision", "size_cm"]);
        let seat = &inspected["asset"]["parts"]["seat"];
        assert_eq!(keys(seat), ["color", "pos", "shape"], "defaults are left out");
        let mut full = edit(&root, json!({"action":"inspect","name":"BUILTIN/bench","legend":true,"bounds":true}));
        crate::reply::tidy(&mut full);
        assert_eq!(full["legend"], props::LEGEND);
        assert_eq!(full["bounds"], json!([[-1.1, 0, -0.476], [1.1, 1.428, 0.38]]));
        assert_eq!(full["part_bounds"]["seat"], json!([[-1.1, 0.71, -0.38], [1.1, 0.89, 0.38]]));
        assert!(edit_at(&root, &args(json!({"action":"patch","name":name,"legend":true,"ops":[]}))).is_err());
        props::remove(&format!("WORKSHOP/{name}")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dry_runs_write_nothing_and_edits_report_only_new_findings() {
        let root = temp_root("lint");
        let name = "test_asset_lint";
        let canonical = format!("WORKSHOP/{name}");
        let copied = edit(&root, json!({"action":"copy","name":name,"from":"BUILTIN/bench"}));
        assert!(copied.get("check").is_none(), "the bench is clean");
        let path = asset_path(&root, &canonical);
        let (file, history) = (std::fs::read(&path).unwrap(), std::fs::read(history_path(&root, &canonical)).unwrap());
        let lamp =
            json!([{"op":"add","part":"lamp","value":{"shape":{"type":"sphere","radius":0.08},"pos":[0,2,0],"color":"#ffd58c"}}]);
        let dry = edit(&root, json!({"action":"patch","name":name,"dry_run":true,"ops":lamp}));
        assert_eq!((dry["saved"].as_bool(), dry["revision"].clone()), (Some(false), copied["revision"].clone()));
        assert_eq!(dry["check"], json!(["floating: lamp (49 cm from the rest)"]));
        assert_eq!(std::fs::read(&path).unwrap(), file);
        assert_eq!(std::fs::read(history_path(&root, &canonical)).unwrap(), history);
        assert_eq!(props::get(&canonical).unwrap().revision, copied["revision"]);
        let real = edit(&root, json!({"action":"patch","name":name,"ops":lamp}));
        assert_eq!(real["revision"], dry["revision_next"], "a dry run predicts the revision");
        assert_eq!(real["check"], dry["check"]);
        let later =
            edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"seat","fields":{"color":"#3e9a9e"}}]}));
        assert!(later.get("check").is_none(), "findings already reported are not repeated");
        let checked = edit(&root, json!({"action":"check","name":name,"target":"200x-x80"}));
        assert_eq!(checked["check"], json!(["floating: lamp (49 cm from the rest)"]));
        assert_eq!(checked["cost"], "2004 tris, 3 meshes, 1 sdf, 0 lights, 80 KB generated");
        assert!(edit_at(&root, &args(json!({"action":"check","name":name,"target":"2x2"}))).is_err());
        assert!(edit_at(&root, &args(json!({"action":"undo","name":name,"dry_run":true}))).is_err(), "undo has no dry run");
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_is_bounded_and_bad_arguments_cannot_write() {
        let root = temp_root("history");
        let name = "test_asset_history";
        let canonical = format!("WORKSHOP/{name}");
        for bad in [
            json!({"action":"create","name":name,"preview":"perhaps"}),
            json!({"action":"create","name":name,"if_revision":42}),
            json!({"action":"create","name":name,"if_revison":"absent"}),
            json!({"action":"create","name":name,"template":"box","asset":{}}),
            json!({"action":"create","name":"WORKSHOP/../../outside"}),
            json!({"action":"patch","name":"BUILTIN/box","ops":[]}),
        ] {
            assert!(edit_at(&root, &args(bad)).is_err());
        }
        assert!(!root.join("workshop").exists());
        // The history write happens first. A failed asset write must restore that history
        // and leave the library unchanged, including for the first save into a new root.
        std::fs::write(root.join("workshop"), "this path is a file").unwrap();
        assert!(edit_at(&root, &args(json!({"action":"create","name":name}))).is_err());
        assert!(!history_path(&root, &canonical).exists());
        assert!(props::get(&canonical).is_none());
        std::fs::remove_file(root.join("workshop")).unwrap();
        edit(&root, json!({"action":"create","name":name,"scene":"empty","seed":1,"ticks":0}));
        for angle in 1..=35 {
            edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"body","fields":{"yaw":angle}}]}));
        }
        let current = edit(&root, json!({"action":"inspect","name":name}));
        assert_eq!(current["undo"], HISTORY_LIMIT);
        edit(&root, json!({"action":"undo","name":name}));
        let branch = edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"body","fields":{"yaw":-10}}]}));
        assert!(branch.get("redo").is_none(), "a new branch has no redo steps");
        let no_op = edit(&root, json!({"action":"patch","name":name,"ops":[]}));
        assert_eq!(no_op["revision"], branch["revision"]);
        assert_eq!(no_op["saved"], false);
        assert_eq!(no_op["undo"], branch["undo"]);
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
