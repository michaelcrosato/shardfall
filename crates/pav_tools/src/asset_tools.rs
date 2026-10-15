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

pub(crate) const HISTORY_LIMIT: usize = 32;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static INITIALIZED: Once = Once::new();

/// Undo and redo steps hold documents as JSON: format 1 or 2, and older files' typed format-1
/// assets read the same way.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct History {
    pub(crate) head: String,
    pub(crate) undo: Vec<Option<Value>>,
    pub(crate) redo: Vec<Option<Value>>,
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
    props::disk_root()
}

/// A workshop asset's file: `workshop/<leaf>.json`, or `local/workshop/` for WORKSHOP_LOCAL.
fn asset_path(root: &Path, name: &str) -> PathBuf {
    props::store::disk_file(root, name).expect("an editable name")
}

fn history_path(root: &Path, name: &str) -> PathBuf {
    let folder = if name.starts_with("WORKSHOP_LOCAL/") { root.join("local").join(".editor") } else { root.join(".editor") };
    folder.join(format!("{}.json", props::leaf(name)))
}

pub(crate) fn lock_editor(root: &Path) -> Result<File> {
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

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
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

/// A workshop file's definition, named `<set>/<leaf>` (WORKSHOP or WORKSHOP_LOCAL).
fn file_definition(path: &Path, set: &str) -> Result<(String, PropDef)> {
    let leaf = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| anyhow!("invalid asset file name"))?;
    props::validate_name(leaf).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let doc = Doc::read(&text).map_err(|issues| anyhow!("{}: {}", path.display(), issue::render(&issues)))?;
    if doc.name != leaf {
        bail!("{} must contain name '{leaf}', but it contains '{}'", path.display(), doc.name);
    }
    let def = compiled(doc).map_err(|e| anyhow!("{}: {e}", path.display()))?;
    Ok((format!("{set}/{leaf}"), def))
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
    let (name, def) = file_definition(path, "WORKSHOP")?;
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
    let folders = [(root.join("workshop"), "WORKSHOP"), (root.join("local").join("workshop"), "WORKSHOP_LOCAL")];
    if folders.iter().all(|(folder, _)| !folder.exists()) {
        return Ok(Vec::new());
    }
    let _lock = lock_editor(root)?;
    let mut paths = Vec::new();
    for (folder, set) in folders.iter().filter(|(folder, _)| folder.exists()) {
        for entry in std::fs::read_dir(folder).with_context(|| format!("read {}", folder.display()))? {
            let path = entry?.path();
            if path.is_file()
                && path.extension().is_some_and(|ext| ext == "json")
                && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.'))
            {
                paths.push((path, *set));
            }
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
        for (path, set) in std::mem::take(&mut paths) {
            match file_definition(&path, set).and_then(|(name, def)| publish_if_changed(&name, def)) {
                Ok(published) => changed.extend(published),
                Err(error) => {
                    errors.push(format!("{error:#}"));
                    paths.push((path, set));
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

/// A workshop asset as its file holds it; templates and library packs from the library.
fn read_asset(root: &Path, name: &str) -> Result<Option<Arc<PropDef>>> {
    if !props::editable(name) {
        return props::resolve_in(root, name).map_err(|e| anyhow!(e));
    }
    let path = asset_path(root, name);
    if !path.exists() {
        return Ok(None);
    }
    let set = name.split_once('/').expect("canonical name").0;
    file_definition(&path, set).map(|(_, def)| Some(Arc::new(def)))
}

fn revision(def: Option<&PropDef>) -> String {
    def.map(|def| def.revision.clone()).unwrap_or_else(|| "absent".into())
}

pub(crate) fn read_history(path: &Path, current: &str) -> Result<History> {
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

pub(crate) fn push_history(stack: &mut Vec<Option<Value>>, document: Option<Value>) {
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
    if def.is_none() && (!props::editable(name) || !history_file.is_file()) {
        bail!("no asset '{name}'; assets find=WORDS searches the library");
    }
    let history =
        if props::editable(name) { read_history(&history_file, &revision(def.as_deref()))? } else { History::default() };
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
        // The legend of the format the document is written in.
        let format1 = def.as_ref().is_some_and(|def| def.doc.needed_format() == 1);
        result["legend"] = json!(if format1 { props::LEGEND } else { props::format2::LEGEND });
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
pub(crate) fn json_arg(a: &Args, key: &str) -> Result<Value> {
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

const ACTIONS: &[&str] = &["create", "copy", "inspect", "check", "patch", "replace", "undo", "redo", "schema", "publish"];

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
        "publish" => &["review", "if_revision"],
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
                "review",
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
        if ["action", "name", "from", "template", "description", "if_revision", "topic", "target", "review"]
            .contains(&key.as_str())
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
    let leaf = props::leaf(name);
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

/// The lint (with the compile warnings), the store's archetype checks (size band, sockets) and
/// the budget findings that need the renderer's meshes.
fn findings(def: &PropDef, target: Option<[Option<f32>; 3]>) -> (Vec<Finding>, Cost) {
    let compiled = &def.compiled;
    let mut found = lint::lint(compiled, &lint::Options { target_cm: target });
    found.retain(|f| !pav_core::figure::exempt(def, f));
    found.extend(lint::archetype(def));
    let cost = Cost::of(compiled);
    let mut over = |code: &'static str, fact: String| found.push(Finding { code, ids: vec![], line: format!("{code}: {fact}") });
    if cost.triangles > 20_000 {
        over("heavy", format!("{} triangles (budget 20000)", cost.triangles));
    }
    // A figure's body draws one mesh per segment (pav_core::figure): 16 at most.
    let figure = def.doc.category == Some(pav_core::props::doc::Category::Character);
    let mesh_budget = if figure { pav_core::figure::Seg::ALL.len() } else { 8 };
    if cost.meshes > mesh_budget {
        over(
            "meshes",
            format!(
                "{} distinct meshes (budget {mesh_budget}); reuse the sizes of rounded boxes and generated shapes",
                cost.meshes
            ),
        );
    }
    if cost.lights > 4 {
        over("lights", format!("{} lights from light fields and parts above emissive 0.5 (budget 4)", cost.lights));
    }
    (found, cost)
}

/// `check`'s findings for `def` (lint, archetype and cost budgets), as `check` lists them.
pub fn check_lines(def: &PropDef) -> Vec<String> {
    lint::lines(&findings(def, None).0)
}

/// A figure's own findings (`pav_core::figure::check`) for the body `name` (`def`) under the
/// props root `root`: its rig file read beside it, then the rig's segments, meshes, floor and
/// proportions checked. One line each, `figure: <where>: <what>`.
pub fn figure_lines(root: &std::path::Path, name: &str, def: std::sync::Arc<PropDef>) -> Vec<String> {
    let Some(path) = pav_core::figure::rig_file(root, name) else {
        return vec![format!("figure: no rig file: rigs/{}.json beside the body (asset_import writes it)", props::leaf(name))];
    };
    let rig = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| pav_core::figure::RigFile::parse(&t)) {
        Ok(rig) => rig,
        Err(e) => return vec![format!("figure: {}: {e}", path.display())],
    };
    match pav_core::figure::Figure::new(name, rig, def) {
        Ok(figure) => pav_core::figure::check(&figure)
            .iter()
            .map(|i| {
                let at = if i.path.is_empty() { "figure".to_string() } else { format!("figure: {}", i.path) };
                let expected = i.expected.as_ref().map(|e| format!(" (expected {e})")).unwrap_or_default();
                format!("{at}: {}{expected}", i.message)
            })
            .collect(),
        Err(e) => vec![format!("figure: {e}")],
    }
}

/// Findings of `after` that `before` did not have: what an edit introduced.
fn new_findings(before: Option<&PropDef>, after: Option<&PropDef>) -> Vec<String> {
    let Some(after) = after else { return Vec::new() };
    let old: BTreeSet<String> =
        before.map(|before| findings(before, None).0.iter().map(Finding::key).collect()).unwrap_or_default();
    let new: Vec<Finding> = findings(after, None).0.into_iter().filter(|f| !old.contains(&f.key())).collect();
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
        // Format 2 is the house format; format 1's guide stays for its files.
        let guide = match a.get("format") {
            Some(format) if format == 1 => props::schema(get_str(a, "topic")),
            None => props::format2::schema(get_str(a, "topic")),
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
        let (found, cost) = findings(&def, target_arg(a)?);
        let mut result =
            json!({"name": name, "revision": def.revision, "size_cm": def.bounds(1.0).size_cm(), "cost": cost.line()});
        let mut lines: Vec<String> =
            if get_bool(a, "all", false)? { found.iter().map(|f| f.line.clone()).collect() } else { lint::lines(&found) };
        if def.doc.category == Some(pav_core::props::doc::Category::Character) {
            // A figure's body: its rig is checked too.
            lines.extend(figure_lines(root, &name, def.clone()));
            if let Some(rig) = pav_core::figure::rig_file(root, &name)
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|t| pav_core::figure::RigFile::parse(&t).ok())
            {
                result["figure"] = json!({"table": rig.table, "look": rig.look, "segments": rig.segments.len()});
            }
        }
        if !lines.is_empty() {
            result["check"] = json!(lines);
        }
        return read_only(result);
    }
    if action == "publish" {
        let def = read_asset(root, &name)?.ok_or_else(|| anyhow!("no asset '{name}'"))?;
        let review = get_str(a, "review").ok_or_else(|| anyhow!("review= says what the sheet review found"))?;
        let reviewed =
            get_str(a, "if_revision").ok_or_else(|| anyhow!("if_revision= is the revision whose sheet you reviewed"))?;
        let found = check_lines(&def);
        return read_only(crate::asset_library::publish(root, &name, &def, reviewed, found, props::store::CORE, review)?);
    }
    if !props::editable(&name) {
        let what = if name.starts_with("BUILTIN/") { "built-in templates are" } else { "library assets are" };
        bail!("{what} read-only; use action=copy from={name} name=your_name first");
    }
    let leaf = props::leaf(&name);
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
    if action == "create" && before.is_some() {
        bail!("{name} already exists; send the whole document with action=replace, or patch it");
    }
    if action == "copy" && before.is_some() {
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
                let def = read_asset(root, &source)?.ok_or_else(|| anyhow!("no source asset '{source}'"))?;
                let mut doc = def.doc.clone();
                // A fork records what it was made from (docs/AGENT_ASSET_STORE.md §9). A copy of
                // a core asset under its own leaf is that asset's next revision (publish puts it
                // back), so it keeps the asset's lineage rather than naming itself as a parent.
                let next_revision = props::store::pack(&source) == Some(props::store::CORE) && props::leaf(&source) == leaf;
                if action == "copy" && !next_revision {
                    doc.lineage = crate::asset_library::fork_lineage(root, &source, &def);
                }
                doc
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
                "replace" => {
                    // The whole document is new, but not what the asset was made from.
                    let mut doc = definition(json_arg(a, "asset")?, &name, false)?;
                    if current.doc.lineage.is_some() {
                        doc.lineage = current.doc.lineage.clone();
                    }
                    doc
                }
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
pub(crate) fn summary(text: &str) -> String {
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
        assert_eq!((copied["parts"].as_u64(), copied["size_cm"].as_str()), (Some(7), Some("220x142x98")));
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
        assert_eq!(full["bounds"], json!([[-1.1, 0, -0.599], [1.1, 1.418, 0.38]]));
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
        assert_eq!(dry["check"], json!(["floating: lamp (50 cm from the rest)"]));
        assert_eq!(std::fs::read(&path).unwrap(), file);
        assert_eq!(std::fs::read(history_path(&root, &canonical)).unwrap(), history);
        assert_eq!(props::get(&canonical).unwrap().revision, copied["revision"]);
        let real = edit(&root, json!({"action":"patch","name":name,"ops":lamp}));
        assert_eq!(real["revision"], dry["revision_next"], "a dry run predicts the revision");
        assert_eq!(real["check"], dry["check"]);
        let later =
            edit(&root, json!({"action":"patch","name":name,"ops":[{"op":"set","part":"seat","fields":{"color":"#3e9a9e"}}]}));
        assert!(later.get("check").is_none(), "findings already reported are not repeated");
        let checked = edit(&root, json!({"action":"check","name":name,"target":"200x-x95"}));
        assert_eq!(checked["check"], json!(["floating: lamp (50 cm from the rest)"]));
        assert_eq!(checked["cost"], "2004 tris, 3 meshes, 1 sdf, 0 lights, 80 KB generated");
        assert!(edit_at(&root, &args(json!({"action":"check","name":name,"target":"2x2"}))).is_err());
        assert!(edit_at(&root, &args(json!({"action":"undo","name":name,"dry_run":true}))).is_err(), "undo has no dry run");
        props::remove(&canonical).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_assets_are_read_only_and_copy_into_either_workshop() {
        let root = temp_root("library");
        let pack = root.join("lib").join("unitpack");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(
            pack.join("post.json"),
            r##"{"format":2,"name":"post","unit":"cm","tags":["wood"],"parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040"}}}"##,
        )
        .unwrap();
        // A pack on disk loads on first use.
        let inspected = edit(&root, json!({"action":"inspect","name":"lib/UnitPack/post"}));
        assert_eq!((inspected["name"].as_str(), inspected["size_cm"].as_str()), (Some("LIB/unitpack/post"), Some("10x100x10")));
        let error = edit_at(&root, &args(json!({"action":"patch","name":"LIB/unitpack/post","ops":[]}))).err().unwrap();
        assert_eq!(
            error.to_string(),
            "library assets are read-only; use action=copy from=LIB/unitpack/post name=your_name first"
        );
        // WORKSHOP_LOCAL saves under local/ (git-ignored), with its own history.
        let copied = edit(&root, json!({"action":"copy","from":"LIB/unitpack/post","name":"WORKSHOP_LOCAL/unit_post"}));
        assert_eq!(copied["name"], "WORKSHOP_LOCAL/unit_post");
        assert!(root.join("local/workshop/unit_post.json").is_file() && root.join("local/.editor/unit_post.json").is_file());
        assert!(!root.join("workshop").exists());
        props::remove("WORKSHOP_LOCAL/unit_post").unwrap();
        assert_eq!(reload_authored_at(&root).unwrap().len(), 1, "a restart loads the local workshop too");
        assert_eq!(props::get("WORKSHOP_LOCAL/unit_post").unwrap().revision, copied["revision"]);
        assert!(props::remove("LIB/unitpack/post").is_err(), "library assets stay");
        props::remove("WORKSHOP_LOCAL/unit_post").unwrap();
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
    #[test]
    fn publish_gates_a_workshop_asset_then_files_it_with_its_review() {
        let root = temp_root("publish");
        let name = "test_asset_publish_post";
        let mut post = json!({"format":2,"name":name,"unit":"cm","description":"A wooden post.",
            "parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040"}}});
        let first = edit(&root, json!({"action":"create","name":name,"asset":post}));
        let publish = |review: &str, revision: &Value| {
            let a = json!({"action":"publish","name":name,"review":review,"if_revision":revision});
            edit_at(&root, &args(a)).err().unwrap().to_string()
        };
        assert!(publish("ok", &first["revision"]).contains("PACK.json"), "a pack needs its licence");
        std::fs::create_dir_all(root.join("lib/core")).unwrap();
        let licence = r#"{"pack":"core","title":"Core","license":"CC0-1.0","credit":"Test","source":"own"}"#;
        std::fs::write(root.join("lib/core/PACK.json"), licence).unwrap();
        let error = publish("ok", &first["revision"]);
        assert!(error.contains("missing: category") && error.contains("missing: 2-6 tags"), "{error}");
        post["category"] = json!("town");
        post["tags"] = json!(["wood", "fence"]);
        post["source"] = json!({"library":"somewhere"});
        let sourced = edit(&root, json!({"action":"replace","name":name,"asset":post}));
        assert!(publish("ok", &sourced["revision"]).contains("license: its source names no licence"));
        post.as_object_mut().unwrap().remove("source");
        let current = edit(&root, json!({"action":"replace","name":name,"asset":post}));
        assert!(publish(" ", &current["revision"]).contains("review="), "a review is required");
        assert!(publish("ok", &first["revision"]).contains("not the reviewed"), "the reviewed revision is published");
        // A ledger that does not read stops the publish before anything is written.
        std::fs::write(root.join("lib/core/review.tsv"), "name\trevision\treview\nLIB/core/x\tprop2-1\t\n").unwrap();
        assert!(publish("ok", &current["revision"]).contains("a review"));
        assert!(!root.join(format!("lib/core/{name}.json")).exists());
        std::fs::remove_file(root.join("lib/core/review.tsv")).unwrap();
        let done = edit(
            &root,
            json!({"action":"publish","name":name,"review":"ok: reads at game distance","if_revision":current["revision"]}),
        );
        let published = format!("LIB/core/{name}");
        assert_eq!(done["published"], published.as_str());
        assert!(root.join(format!("lib/core/{name}.json")).is_file());
        let ledger = crate::asset_library::reviews(&root.join("lib/core")).unwrap();
        let revision = done["revision"].as_str().unwrap().to_owned();
        assert_eq!(ledger, [(published.clone(), revision, "ok: reads at game distance".to_owned())]);
        let index = std::fs::read_to_string(root.join("lib/core/index.tsv")).unwrap();
        assert!(index.contains(&format!("{published}\ttown")), "{index}");
        assert!(std::fs::read_to_string(root.join("CREDITS.md")).unwrap().contains("## LIB/core: Core"));
        props::remove(&format!("WORKSHOP/{name}")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copies_record_lineage_and_keep_it_through_edits_and_publish() {
        use props::doc::{Method, Origin};
        let root = temp_root("lineage");
        let pack = root.join("lib/importpack");
        std::fs::create_dir_all(&pack).unwrap();
        let licence = r#"{"pack":"importpack","title":"Posts","license":"CC0-1.0","credit":"Post Maker (example.org)","source":"https://example.org/posts.zip"}"#;
        std::fs::write(pack.join("PACK.json"), licence).unwrap();
        std::fs::write(
            pack.join("post.json"),
            r##"{"format":2,"name":"post","unit":"cm","description":"A wooden post.","tags":["wood","post"],"category":"town",
                "source":{"library":"maker/posts","license":"CC0-1.0","title":"Post"},
                "parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040"}}}"##,
        )
        .unwrap();
        let lineage = |name: &str| props::get(name).unwrap().doc.lineage.clone().unwrap();
        let parent = edit(&root, json!({"action":"inspect","name":"LIB/importpack/post"}));
        // A fork of an import: its parent at its revision, edited, origin import, and the
        // pack's licence and credit.
        edit(&root, json!({"action":"copy","from":"LIB/importpack/post","name":"test_lineage_import"}));
        let fork = lineage("WORKSHOP/test_lineage_import");
        assert_eq!((fork.method, fork.origin), (Method::Edited, Origin::Import));
        assert_eq!(fork.parents, [format!("LIB/importpack/post@{}", parent["revision"].as_str().unwrap())]);
        assert_eq!((fork.license.as_deref(), fork.credit.as_deref()), (Some("CC0-1.0"), Some("Post Maker (example.org)")));
        let text = std::fs::read_to_string(asset_path(&root, "WORKSHOP/test_lineage_import")).unwrap();
        assert!(
            text.contains("\n\"lineage\":{\"method\":\"edited\",\"origin\":\"import\",\"parents\":[\"LIB/importpack/post@prop2-"),
            "{text}"
        );
        // A fork of own work: origin agent, CC0 and the own-work credit.
        let chair = props::get("LIB/core/chair").unwrap().revision.clone();
        edit(&root, json!({"action":"copy","from":"LIB/core/chair","name":"test_lineage_core"}));
        let own = lineage("WORKSHOP/test_lineage_core");
        assert_eq!(
            (own.method, own.origin, own.parents),
            (Method::Edited, Origin::Agent, vec![format!("LIB/core/chair@{chair}")])
        );
        assert_eq!((own.license.as_deref(), own.credit.as_deref()), (Some("CC0-1.0"), Some(pav_core::store::OWN_CREDIT)));
        // A fork of a fork inherits the origin, licence and credit; its parent is the fork.
        let first = edit(&root, json!({"action":"inspect","name":"WORKSHOP/test_lineage_import"}));
        edit(&root, json!({"action":"copy","from":"WORKSHOP/test_lineage_import","name":"test_lineage_grand"}));
        let grand = lineage("WORKSHOP/test_lineage_grand");
        assert_eq!(grand.parents, [format!("WORKSHOP/test_lineage_import@{}", first["revision"].as_str().unwrap())]);
        assert_eq!((grand.origin, grand.credit), (Origin::Import, fork.credit.clone()));
        // A copy of a format-1 template stays format 1, which holds no lineage.
        edit(&root, json!({"action":"copy","from":"BUILTIN/bench","name":"test_lineage_bench"}));
        let bench = props::get("WORKSHOP/test_lineage_bench").unwrap();
        assert!(bench.v1.is_some() && bench.doc.lineage.is_none());
        // Patches and replace keep it; a patch cannot set it.
        let patched = edit(
            &root,
            json!({"action":"patch","name":"test_lineage_import","ops":[{"op":"set","part":"post","fields":{"color":"#705030"}}]}),
        );
        assert_eq!(lineage("WORKSHOP/test_lineage_import"), fork);
        let ops = json!([{"op":"set","fields":{"lineage":{"method":"authored","origin":"human"}}}]);
        let error = edit_at(&root, &args(json!({"action":"patch","name":"test_lineage_import","ops":ops}))).err().unwrap();
        assert!(error.to_string().contains("not set this way"), "{error}");
        let mut whole: Value = serde_json::from_str(&text).unwrap();
        whole.as_object_mut().unwrap().remove("lineage");
        whole["description"] = json!("A weathered wooden post.");
        let replaced =
            edit(&root, json!({"action":"replace","name":"test_lineage_import","asset":whole,"if_revision":patched["revision"]}));
        assert_eq!(lineage("WORKSHOP/test_lineage_import"), fork);
        // Publish keeps it in the library file, and the index row says edited.
        std::fs::create_dir_all(root.join("lib/core")).unwrap();
        let core = r#"{"pack":"core","title":"Core","license":"CC0-1.0","credit":"Test","source":"own"}"#;
        std::fs::write(root.join("lib/core/PACK.json"), core).unwrap();
        let review = "ok: reads at game distance";
        edit(&root, json!({"action":"publish","name":"test_lineage_import","review":review,"if_revision":replaced["revision"]}));
        let published = std::fs::read_to_string(root.join("lib/core/test_lineage_import.json")).unwrap();
        let doc = props::Doc::read(&published).unwrap();
        assert_eq!(doc.lineage, Some(fork));
        let index = std::fs::read_to_string(root.join("lib/core/index.tsv")).unwrap();
        assert!(index.contains("\tprop\tCC0-1.0\tedited\tstandard\t"), "{index}");
        for name in ["test_lineage_import", "test_lineage_core", "test_lineage_grand", "test_lineage_bench"] {
            props::remove(&format!("WORKSHOP/{name}")).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
