//! Format-2 patches: operations on the document's JSON (either format, in its format-2 view),
//! then the result is read and checked whole like a file. The batch is atomic: the original
//! stays unchanged if any operation fails, and each problem names the operation that caused it
//! (`ops[2] crate.fruit.grid.count: 0 is outside 1-32 — ...`).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{EulerRot, Quat, Vec3};
use serde_json::{Map, Value};

use super::MAX_OPS;
use crate::issue::{self, Code, Issue};
use crate::props::compile::{ends_frame, euler};
use crate::props::doc::{Dir, Doc, PrimType, Unit};
use crate::props::{PropDef, validate_part_name};

/// Operations: their fields, and the field-guide line.
pub const OPS: &[(&str, &[&str], &str)] = &[
    (
        "set",
        &["part", "parts", "material", "socket", "fields"],
        "{op,part|parts|material|socket,fields}: merge fields; \"array.count\":8 reaches in, null deletes, type replaces \
         the shape; with no target, document fields",
    ),
    (
        "add",
        &["part", "material", "socket", "value"],
        "{op,part|material|socket,value}: a new entry; part \"roof.ridge\" adds to the group roof",
    ),
    ("remove", &["part", "parts", "material", "socket"], "{op,part|parts|material|socket}: delete it"),
    (
        "rename",
        &["part", "material", "socket", "to"],
        "{op,part|material|socket,to}: a new name; attach targets and mat references follow",
    ),
    (
        "copy",
        &["part", "material", "socket", "to", "fields"],
        "{op,part|material|socket,to,fields?}: a copy beside it, then fields",
    ),
    ("mirror", &["part", "parts", "axis"], "{op,part|parts,axis}: mirrored across x, y or z (\"xz\": both); null removes it"),
    (
        "array",
        &["part", "parts", "count", "step", "span", "cycle"],
        "{op,part|parts,count,step|span,cycle?}: copies along step, centred on the part (replaces grid or radial)",
    ),
    ("expand", &["part", "parts"], "{op,part|parts}: its copies become parts <id>_<i>, placed where they were"),
    ("move", &["part", "parts", "by"], "{op,part|parts,by:[x,y,z]}: shift pos, from/to or an attach offset"),
    (
        "scale",
        &["part", "parts", "by", "about"],
        "{op,by,part|parts?,about?}: multiply lengths, about each part's centre or [x,y,z]; no part: the whole asset about \
         the origin",
    ),
    ("unit", &["to"], "{op,to}: convert every length to cm or m"),
];

/// Shape fields: a `type` in `set` replaces them all.
const SHAPE_KEYS: &[&str] = &[
    "type",
    "size",
    "half",
    "radius",
    "radius_end",
    "minor_radius",
    "thickness",
    "height",
    "half_height",
    "arc",
    "facing",
    "peak",
    "profile",
    "points",
    "depth",
    "segments",
];

/// Document fields that operations address another way.
const NOT_DOCUMENT_FIELDS: &[(&str, &str)] = &[
    ("format", "the format is chosen when the asset is saved"),
    ("name", "copy the asset to give it another name"),
    ("unit", "use op:unit, which converts every length"),
    ("parts", "address parts with part or parts"),
    ("materials", "address materials with material"),
    ("sockets", "address sockets with socket"),
];

/// A patched document, and which operation last touched each entry, so later problems (the
/// compile's too) name it.
#[derive(Debug)]
pub struct Patched {
    pub doc: Doc,
    touched: Touched,
}

impl Patched {
    /// `parts.roof.parts.slab.size` becomes `ops[3] roof.slab.size` when op 3 last touched
    /// roof.slab.
    pub fn readdress(&self, issues: Vec<Issue>) -> Vec<Issue> {
        issues.into_iter().map(|issue| self.touched.readdress(issue)).collect()
    }
}

#[derive(Debug, Default)]
struct Touched {
    parts: BTreeMap<String, usize>,
    materials: BTreeMap<String, usize>,
    sockets: BTreeMap<String, usize>,
    document: Option<usize>,
    /// An operation on every length (`unit`, `scale` without a part).
    everything: Option<usize>,
}

impl Touched {
    fn readdress(&self, mut issue: Issue) -> Issue {
        let path = issue.path.clone();
        let found = if let Some(rest) = path.strip_prefix("parts.") {
            // parts.a.parts.b.size → id a.b, field size.
            let (mut id, mut rest) = split_name(rest);
            let mut ids = vec![id.clone()];
            while let Some(child) = rest.strip_prefix(".parts.") {
                let (name, after) = split_name(child);
                id = format!("{id}.{name}");
                ids.push(id.clone());
                rest = after;
            }
            let owner = ids.iter().rev().find_map(|id| self.parts.get(id).map(|index| (*index, id.clone())));
            owner.map(|(index, owner)| {
                let tail = &path[path.len() - rest.len()..];
                let deeper = &id[owner.len()..];
                format!("ops[{index}] {owner}{deeper}{tail}")
            })
        } else if let Some(rest) = path.strip_prefix("materials.") {
            let (name, _) = split_name(rest);
            self.materials.get(&name).map(|index| format!("ops[{index}] {path}"))
        } else if let Some(rest) = path.strip_prefix("sockets.") {
            let (name, _) = split_name(rest);
            self.sockets.get(&name).map(|index| format!("ops[{index}] {path}"))
        } else if !path.is_empty() {
            self.document.map(|index| format!("ops[{index}] {path}"))
        } else {
            None
        };
        if let Some(found) = found.or_else(|| self.everything.map(|index| format!("ops[{index}] {path}").trim_end().to_string()))
        {
            issue.path = found;
        }
        issue
    }
}

/// `seat.color` → ("seat", ".color").
fn split_name(text: &str) -> (String, &str) {
    let end = text.find(['.', '[']).unwrap_or(text.len());
    (text[..end].to_string(), &text[end..])
}

/// Apply a batch to a document; references are looked up in `assets`.
pub fn patch(doc: &Doc, ops: &Value, assets: &dyn Fn(&str) -> Option<Arc<PropDef>>) -> Result<Patched, Vec<Issue>> {
    let Some(ops) = ops.as_array() else {
        return Err(vec![
            Issue::error("ops", Code::InvalidType, "expected an array of operations")
                .fix("ops:[{\"op\":\"set\",\"part\":\"seat\",\"fields\":{\"color\":\"#4060a0\"}}]"),
        ]);
    };
    if ops.len() > MAX_OPS {
        return Err(vec![
            Issue::error("ops", Code::Limit, format!("{} operations", ops.len())).expected(format!("at most {MAX_OPS}")),
        ]);
    }
    let mut value: Value = serde_json::from_str(&doc.write(true)).expect("canonical text is JSON");
    let mut touched = Touched::default();
    let issues: Vec<Issue> = ops
        .iter()
        .enumerate()
        .filter_map(|(index, op)| Op::new(index, op).and_then(|op| op.apply(&mut value, &mut touched)).err())
        .collect();
    if !issues.is_empty() {
        return Err(issues);
    }
    match Doc::from_value_in(&value, assets) {
        Ok(doc) => Ok(Patched { doc, touched }),
        Err(issues) => Err(issues.into_iter().map(|issue| touched.readdress(issue)).collect()),
    }
}

/// What an operation addresses.
enum Target {
    Document,
    Parts(Vec<String>),
    Material(String),
    Socket(String),
}

struct Op<'a> {
    index: usize,
    kind: &'a str,
    object: &'a Map<String, Value>,
    target: Target,
}

impl<'a> Op<'a> {
    fn new(index: usize, op: &'a Value) -> Result<Self, Issue> {
        let at = |key: &str| format!("ops[{index}].{key}");
        let names: Vec<&str> = OPS.iter().map(|(name, _, _)| *name).collect();
        let object = op.as_object().ok_or_else(|| {
            Issue::error(format!("ops[{index}]"), Code::InvalidType, "expected an operation {op,part,...}")
                .expected(names.join(", "))
        })?;
        let kind = match object.get("op") {
            Some(Value::String(kind)) => kind.as_str(),
            Some(_) => {
                return Err(Issue::error(at("op"), Code::InvalidType, "expected the operation name").expected(names.join(", ")));
            }
            None => return Err(Issue::error(at("op"), Code::Missing, "missing").expected(names.join(", "))),
        };
        let Some((_, keys, _)) = OPS.iter().find(|(name, _, _)| *name == kind) else {
            let fix = match kind {
                "update" | "modify" | "edit" | "change" => Some("use op:set".to_string()),
                "delete" | "del" => Some("use op:remove".to_string()),
                "insert" | "create" | "new" => Some("use op:add".to_string()),
                "duplicate" | "clone" => Some("use op:copy".to_string()),
                "translate" | "offset" | "shift" => Some("use op:move".to_string()),
                "resize" => Some("use op:scale".to_string()),
                "flip" => Some("use op:mirror".to_string()),
                "repeat" => Some("use op:array".to_string()),
                "explode" | "unroll" | "bake" => Some("use op:expand".to_string()),
                "convert" | "units" => Some("use op:unit".to_string()),
                _ => issue::suggestion(kind, names.iter().copied()),
            };
            return Err(Issue::error(at("op"), Code::UnknownValue, format!("unknown op \"{kind}\""))
                .expected(names.join(", "))
                .maybe_fix(fix));
        };
        if let Some(key) = object.keys().find(|key| *key != "op" && !keys.contains(&key.as_str())) {
            let fix = match (kind, key.as_str()) {
                (_, "path" | "id" | "target" | "node") => {
                    Some("address a part with \"part\"; a dotted id reaches into groups".to_string())
                }
                ("set", "value") => Some("set takes \"fields\" to merge".to_string()),
                ("add", "fields") => Some("add takes the whole entry as \"value\"".to_string()),
                ("rename" | "copy", "name" | "new_name" | "as") => Some("the new name goes in \"to\"".to_string()),
                ("move", "to" | "offset" | "delta" | "pos") => Some("move takes by:[x,y,z]".to_string()),
                ("scale", "factor" | "scale" | "amount") => Some("scale takes by".to_string()),
                ("unit", "unit" | "from") => Some("unit takes to:\"cm\" or to:\"m\"".to_string()),
                (_, "material" | "socket") => Some(format!("{kind} works on parts")),
                _ => issue::suggestion(key, keys.iter().copied()),
            };
            return Err(Issue::error(at(key), Code::UnknownKey, format!("unknown key for op {kind}"))
                .expected(keys.join(", "))
                .maybe_fix(fix));
        }
        let given: Vec<&str> =
            ["part", "parts", "material", "socket"].into_iter().filter(|key| object.contains_key(*key)).collect();
        if given.len() > 1 {
            return Err(Issue::error(
                at(given[1]),
                Code::Conflict,
                format!("one target per operation, not {}", given.join(" and ")),
            ));
        }
        let text = |key: &str| match object.get(key) {
            Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
            _ => Err(Issue::error(at(key), Code::InvalidType, format!("expected a {key} name"))),
        };
        let target = match given.first().copied() {
            None => Target::Document,
            Some("part") => Target::Parts(vec![text("part")?]),
            Some("parts") => match object.get("parts") {
                Some(Value::Array(items)) if !items.is_empty() => Target::Parts(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            item.as_str()
                                .map(String::from)
                                .ok_or_else(|| Issue::error(at(&format!("parts[{i}]")), Code::InvalidType, "expected a part id"))
                        })
                        .collect::<Result<_, _>>()?,
                ),
                _ => return Err(Issue::error(at("parts"), Code::InvalidType, "expected a list of part ids")),
            },
            Some("material") => Target::Material(text("material")?),
            Some(_) => Target::Socket(text("socket")?),
        };
        let needs = match kind {
            "set" | "scale" => None,
            "add" | "remove" | "rename" | "copy" => Some("part, material or socket"),
            "unit" => None,
            _ => Some("part or parts"),
        };
        if let (Some(needs), Target::Document) = (needs, &target) {
            return Err(Issue::error(format!("ops[{index}]"), Code::Missing, format!("{kind} needs a target")).expected(needs));
        }
        Ok(Self { index, kind, object, target })
    }

    fn at(&self, key: &str) -> String {
        format!("ops[{}].{key}", self.index)
    }

    /// `ops[3] roof.slab`: where a problem with an addressed entry is reported.
    fn on(&self, what: &str) -> String {
        format!("ops[{}] {what}", self.index)
    }

    fn field(&self, key: &str) -> Result<&'a Value, Issue> {
        self.object.get(key).ok_or_else(|| Issue::error(self.at(key), Code::Missing, "missing"))
    }

    fn new_name(&self) -> Result<String, Issue> {
        match self.field("to")? {
            Value::String(to) => match validate_part_name(to) {
                Ok(()) => Ok(to.clone()),
                Err(error) => Err(Issue::error(self.at("to"), Code::UnknownValue, format!("\"{to}\" is not a name: {error}"))),
            },
            _ => Err(Issue::error(self.at("to"), Code::InvalidType, "expected the new name")),
        }
    }

    fn fields(&self) -> Result<Option<&'a Map<String, Value>>, Issue> {
        match self.object.get("fields") {
            None => Ok(None),
            Some(Value::Object(fields)) => Ok(Some(fields)),
            Some(_) => Err(Issue::error(self.at("fields"), Code::InvalidType, "expected an object of fields")),
        }
    }

    fn vector(&self, key: &str) -> Result<Vec3, Issue> {
        vec3(self.field(key)?).ok_or_else(|| Issue::error(self.at(key), Code::InvalidType, "expected [x,y,z]"))
    }

    fn apply(&self, doc: &mut Value, touched: &mut Touched) -> Result<(), Issue> {
        match (&self.target, self.kind) {
            (Target::Parts(ids), _) => {
                for id in ids {
                    self.part(doc, id, touched)?;
                }
                Ok(())
            }
            (Target::Material(name), _) => {
                self.entry(doc, "materials", name)?;
                touched.materials.insert(self.renamed(name), self.index);
                Ok(())
            }
            (Target::Socket(name), _) => {
                self.entry(doc, "sockets", name)?;
                touched.sockets.insert(self.renamed(name), self.index);
                Ok(())
            }
            (Target::Document, "set") => self.document(doc, touched),
            (Target::Document, "scale") => {
                let by = self.factor()?;
                let about = self.object.get("about").map(|_| self.vector("about")).transpose()?.unwrap_or(Vec3::ZERO);
                scale_document(doc, by, about, true);
                touched.everything = Some(self.index);
                Ok(())
            }
            (Target::Document, _) => self.unit(doc, touched),
        }
    }

    /// The name an entry ends up with: rename and copy make a new one.
    fn renamed(&self, name: &str) -> String {
        match (self.kind, self.object.get("to").and_then(Value::as_str)) {
            ("rename" | "copy", Some(to)) => to.to_string(),
            _ => name.to_string(),
        }
    }

    fn factor(&self) -> Result<f64, Issue> {
        match self.field("by")?.as_f64() {
            Some(by) if by.is_finite() && by > 0.0 && by <= 1000.0 => Ok(by),
            Some(by) => {
                Err(Issue::error(self.at("by"), Code::OutOfRange, format!("{by} is outside 0-1000")).expected("a factor above 0"))
            }
            None => Err(Issue::error(self.at("by"), Code::InvalidType, "expected a factor, e.g. 1.5")),
        }
    }

    /// One part: `id` is dotted through groups.
    fn part(&self, doc: &mut Value, id: &str, touched: &mut Touched) -> Result<(), Issue> {
        let segments = segments(id).map_err(|error| Issue::error(self.on(id), Code::UnknownPart, error.0).maybe_fix(error.1))?;
        let (scope, name) = segments.split_at(segments.len() - 1);
        let name = name[0];
        let parent =
            group_mut(doc, scope).map_err(|error| Issue::error(self.on(id), Code::UnknownPart, error.0).maybe_fix(error.1))?;
        let missing = |parent: &Map<String, Value>| {
            Issue::error(self.on(id), Code::UnknownPart, format!("no part \"{id}\""))
                .maybe_fix(issue::suggestion(name, parent.keys().map(String::as_str)))
        };
        if self.kind == "add" {
            if let Err(error) = validate_part_name(name) {
                return Err(Issue::error(self.on(id), Code::UnknownValue, format!("\"{name}\" is not a part name: {error}")));
            }
            if parent.contains_key(name) {
                return Err(Issue::error(self.on(id), Code::Duplicate, format!("part \"{id}\" already exists"))
                    .fix("use op:set to change it"));
            }
            let mut value = self.field("value")?.clone();
            if let Some(object) = value.as_object_mut() {
                flatten_shape(object);
            }
            parent.insert(name.to_string(), value);
            touched.parts.insert(id.to_string(), self.index);
            return Ok(());
        }
        if !parent.contains_key(name) {
            return Err(missing(parent));
        }
        match self.kind {
            "remove" => {
                parent.remove(name);
                Ok(())
            }
            "rename" | "copy" => {
                let to = self.new_name()?;
                if parent.contains_key(&to) {
                    return Err(Issue::error(self.at("to"), Code::Duplicate, format!("a part \"{to}\" is already beside it")));
                }
                let mut node = parent[name].clone();
                if self.kind == "copy" {
                    if let (Some(fields), Some(object)) = (self.fields()?, node.as_object_mut()) {
                        merge(object, fields).map_err(|error| Issue::error(self.at("fields"), Code::InvalidType, error))?;
                    }
                } else {
                    parent.remove(name);
                }
                parent.insert(to.clone(), node);
                let new_id = scope.iter().map(|s| s.to_string()).chain([to.clone()]).collect::<Vec<_>>().join(".");
                if self.kind == "rename" {
                    let blocked =
                        retarget(doc, &segments, &mut |segment| Some(format!("{to}{}", &segment[base(segment).len()..])));
                    debug_assert!(blocked.is_empty());
                }
                touched.parts.insert(new_id, self.index);
                Ok(())
            }
            "expand" => {
                let names = expand(parent, name).map_err(|error| Issue::error(self.on(id), Code::Conflict, error))?;
                let prefix: String = scope.iter().map(|s| format!("{s}.")).collect();
                // Targets naming one copy follow it; one naming the whole node has nothing to follow.
                let blocked = retarget(doc, &segments, &mut |segment| {
                    let (index, rest) = segment[base(segment).len()..].strip_prefix('[')?.split_once(']')?;
                    Some(format!("{name}_{index}{rest}"))
                });
                if !blocked.is_empty() {
                    return Err(Issue::error(
                        self.on(id),
                        Code::Conflict,
                        format!("{} attaches to {id} as a whole", blocked.join(", ")),
                    )
                    .fix(format!("attach it to one copy ({id}[0]) first")));
                }
                for new in names {
                    touched.parts.insert(format!("{prefix}{new}"), self.index);
                }
                Ok(())
            }
            _ => {
                let node = parent.get_mut(name).and_then(Value::as_object_mut).ok_or_else(|| {
                    Issue::error(self.on(id), Code::InvalidType, format!("part \"{id}\" is not an object {{type,...}}"))
                })?;
                self.edit_node(node, id)?;
                touched.parts.insert(id.to_string(), self.index);
                Ok(())
            }
        }
    }

    /// set, mirror, array, move and scale on one node.
    fn edit_node(&self, node: &mut Map<String, Value>, id: &str) -> Result<(), Issue> {
        match self.kind {
            "set" => {
                let fields = self.fields()?.ok_or_else(|| Issue::error(self.at("fields"), Code::Missing, "missing"))?;
                merge(node, fields).map_err(|error| Issue::error(self.at("fields"), Code::InvalidType, error))
            }
            "mirror" => {
                match self.field("axis")? {
                    Value::Null => {
                        node.remove("mirror");
                    }
                    Value::String(axes) if axes.len() > 1 && axes.chars().all(|c| "xyz".contains(c)) => {
                        node.insert("mirror".into(), Value::Array(axes.chars().map(|c| Value::String(c.to_string())).collect()));
                    }
                    axis => {
                        node.insert("mirror".into(), axis.clone());
                    }
                }
                Ok(())
            }
            "array" => {
                let mut array = Map::new();
                array.insert("count".into(), self.field("count")?.clone());
                match (self.object.get("step"), self.object.get("span")) {
                    (Some(_), Some(_)) => return Err(Issue::error(self.at("span"), Code::Conflict, "step or span, not both")),
                    (Some(step), None) => {
                        array.insert("step".into(), step.clone());
                    }
                    (None, Some(span)) => {
                        array.insert("span".into(), span.clone());
                    }
                    (None, None) => {
                        return Err(
                            Issue::error(self.at("step"), Code::Missing, "missing").expected("step:[x,y,z] or span:[x,y,z]")
                        );
                    }
                }
                if let Some(cycle) = self.object.get("cycle") {
                    array.insert("cycle".into(), cycle.clone());
                }
                node.remove("grid");
                node.remove("radial");
                node.insert("array".into(), Value::Object(array));
                Ok(())
            }
            "move" => {
                let by = self.vector("by")?;
                let shift = |node: &mut Map<String, Value>, key: &str| {
                    let moved = vec3(node.get(key).unwrap_or(&Value::Null)).unwrap_or(Vec3::ZERO) + by;
                    node.insert(key.into(), point(moved));
                };
                if node.contains_key("from") && node.contains_key("to") {
                    shift(node, "from");
                    shift(node, "to");
                } else if node.contains_key("attach") {
                    shift(node, "offset");
                } else {
                    shift(node, "pos");
                }
                Ok(())
            }
            "scale" => {
                let by = self.factor()?;
                let about = match self.object.get("about") {
                    Some(_) => self.vector("about")?,
                    None => centre(node),
                };
                scale_node(node, by, about, true);
                Ok(())
            }
            kind => Err(Issue::error(self.on(id), Code::UnknownValue, format!("{kind} does not apply to parts"))),
        }
    }

    /// A material or socket entry.
    fn entry(&self, doc: &mut Value, section: &str, name: &str) -> Result<(), Issue> {
        let what = &section[..section.len() - 1];
        let root = doc.as_object_mut().expect("a document is an object");
        if !root.get(section).is_some_and(Value::is_object) {
            root.insert(section.into(), Value::Object(Map::new()));
        }
        let path = format!("{section}.{name}");
        let map = root[section].as_object_mut().expect("checked");
        if self.kind == "add" {
            if map.contains_key(name) {
                return Err(Issue::error(self.on(&path), Code::Duplicate, format!("{what} \"{name}\" already exists"))
                    .fix("use op:set to change it"));
            }
            map.insert(name.into(), self.field("value")?.clone());
            return Ok(());
        }
        if !map.contains_key(name) {
            return Err(Issue::error(self.on(&path), Code::UnknownValue, format!("no {what} \"{name}\""))
                .maybe_fix(issue::suggestion(name, map.keys().map(String::as_str))));
        }
        match self.kind {
            "remove" => {
                map.remove(name);
                if map.is_empty() {
                    root.remove(section);
                }
                Ok(())
            }
            "rename" | "copy" => {
                let to = self.new_name()?;
                if map.contains_key(&to) {
                    return Err(Issue::error(self.at("to"), Code::Duplicate, format!("a {what} \"{to}\" already exists")));
                }
                let mut entry = map[name].clone();
                if let Some(fields) = self.fields()? {
                    if section == "materials" {
                        library_inline(&mut entry);
                    }
                    if let Some(object) = entry.as_object_mut() {
                        merge(object, fields).map_err(|error| Issue::error(self.at("fields"), Code::InvalidType, error))?;
                    }
                }
                if self.kind == "rename" {
                    map.remove(name);
                }
                map.insert(to.clone(), entry);
                if self.kind == "rename" && section == "materials" {
                    rename_material(doc, name, &to);
                }
                Ok(())
            }
            "set" => {
                let fields = self.fields()?.ok_or_else(|| Issue::error(self.at("fields"), Code::Missing, "missing"))?;
                let entry = map.get_mut(name).expect("checked");
                if section == "materials" {
                    library_inline(entry);
                }
                let object = entry.as_object_mut().ok_or_else(|| {
                    Issue::error(self.on(&path), Code::InvalidType, format!("{what} \"{name}\" is not an object"))
                })?;
                merge(object, fields).map_err(|error| Issue::error(self.at("fields"), Code::InvalidType, error))
            }
            kind => Err(Issue::error(self.at("op"), Code::UnknownValue, format!("{kind} works on parts"))),
        }
    }

    /// `set` with no target: the document's own fields.
    fn document(&self, doc: &mut Value, touched: &mut Touched) -> Result<(), Issue> {
        let fields = self.fields()?.ok_or_else(|| Issue::error(self.at("fields"), Code::Missing, "missing"))?;
        for key in fields.keys() {
            let first = key.split('.').next().unwrap_or(key);
            if let Some((_, fix)) = NOT_DOCUMENT_FIELDS.iter().find(|(field, _)| *field == first) {
                return Err(Issue::error(self.at(&format!("fields.{key}")), Code::Conflict, "not set this way").fix(*fix));
            }
        }
        merge(doc.as_object_mut().expect("a document is an object"), fields)
            .map_err(|error| Issue::error(self.at("fields"), Code::InvalidType, error))?;
        touched.document = Some(self.index);
        Ok(())
    }

    fn unit(&self, doc: &mut Value, touched: &mut Touched) -> Result<(), Issue> {
        let to = match self.field("to")? {
            Value::String(to) => Unit::parse(to).ok_or_else(|| {
                Issue::error(self.at("to"), Code::UnknownValue, format!("unknown unit \"{to}\"")).expected(Unit::NAMES.join(", "))
            })?,
            _ => return Err(Issue::error(self.at("to"), Code::InvalidType, "expected cm or m")),
        };
        let from = doc.get("unit").and_then(Value::as_str).and_then(Unit::parse).unwrap_or(Unit::Cm);
        scale_document(doc, (to.per_metre() / from.per_metre()) as f64, Vec3::ZERO, false);
        doc["unit"] = Value::String(to.name().into());
        touched.everything = Some(self.index);
        Ok(())
    }
}

/// A dotted id's names. Copies are not nodes: their node is edited, or expanded first.
fn segments(id: &str) -> Result<Vec<&str>, (String, Option<String>)> {
    let segments: Vec<&str> = id.split('.').collect();
    for segment in &segments {
        if segment.is_empty() {
            return Err((format!("\"{id}\" has an empty name"), None));
        }
        let name = base(segment);
        if name.len() != segment.len() {
            return Err((
                format!("\"{segment}\" names a copy of {name}"),
                Some(format!("edit {name}, whose copies follow it, or expand it first")),
            ));
        }
    }
    Ok(segments)
}

/// A resolved id segment without its copy suffixes: `leg[1]~x` → `leg`.
fn base(segment: &str) -> &str {
    &segment[..segment.find(['[', '~']).unwrap_or(segment.len())]
}

/// The children of the group at `scope` (the root's parts for an empty scope).
fn group_mut<'v>(doc: &'v mut Value, scope: &[&str]) -> Result<&'v mut Map<String, Value>, (String, Option<String>)> {
    let mut map =
        doc.get_mut("parts").and_then(Value::as_object_mut).ok_or_else(|| ("the document has no parts".to_string(), None))?;
    for (depth, name) in scope.iter().enumerate() {
        let id = scope[..=depth].join(".");
        match map.get(*name) {
            None => return Err((format!("no part \"{id}\""), issue::suggestion(name, map.keys().map(String::as_str)))),
            Some(node) if !node.get("parts").is_some_and(Value::is_object) => {
                return Err((format!("\"{id}\" is not a group"), None));
            }
            Some(_) => {}
        }
        map = map.get_mut(*name).and_then(|node| node.get_mut("parts")).and_then(Value::as_object_mut).expect("checked");
    }
    Ok(map)
}

/// Merge `fields` into an entry: a dotted key reaches into nested objects and `null` deletes.
/// A `type` replaces the shape fields, and a format-1 `shape` object is spread out flat.
fn merge(target: &mut Map<String, Value>, fields: &Map<String, Value>) -> Result<(), String> {
    let mut fields = fields.clone();
    flatten_shape(&mut fields);
    if fields.contains_key("type") {
        for key in SHAPE_KEYS {
            target.remove(*key);
        }
    }
    // Two spellings of one size: writing one drops the other.
    for (key, other) in [("size", "half"), ("half", "size"), ("height", "half_height"), ("half_height", "height")] {
        if fields.get(key).is_some_and(|value| !value.is_null()) && !fields.contains_key(other) {
            target.remove(other);
        }
    }
    for (key, value) in fields {
        let path: Vec<&str> = key.split('.').collect();
        if path.iter().any(|segment| segment.is_empty()) {
            return Err(format!("\"{key}\" has an empty name"));
        }
        set_path(target, &path, value)?;
    }
    Ok(())
}

fn set_path(object: &mut Map<String, Value>, path: &[&str], value: Value) -> Result<(), String> {
    let (first, rest) = (path[0], &path[1..]);
    if rest.is_empty() {
        if value.is_null() {
            object.remove(first);
        } else {
            object.insert(first.into(), value);
        }
        return Ok(());
    }
    let remove = value.is_null();
    let child = match object.get_mut(first) {
        None if remove => return Ok(()),
        None => object.entry(first).or_insert_with(|| Value::Object(Map::new())),
        Some(child) => child,
    };
    // `attach:"seat"` is short for `attach:{to:"seat"}`.
    if let (Value::String(to), "attach") = (&*child, first) {
        *child = serde_json::json!({ "to": to });
    }
    let Some(inner) = child.as_object_mut() else {
        return Err(format!("{first} is not an object; set it whole"));
    };
    set_path(inner, rest, value)?;
    if inner.is_empty() {
        object.remove(first);
    }
    Ok(())
}

/// Format 1's `shape:{type,...}` written into a format-2 part: its fields, flat.
fn flatten_shape(object: &mut Map<String, Value>) {
    if let Some(Value::Object(shape)) = object.remove("shape") {
        for key in SHAPE_KEYS {
            object.remove(*key);
        }
        object.extend(shape);
    }
}

/// A library material written as a name, inline: `"MAT/oak"` → `{"base":"MAT/oak"}`.
fn library_inline(entry: &mut Value) {
    if let Value::String(name) = entry {
        *entry = serde_json::json!({ "base": name });
    }
}

/// Every `mat`, cycle entry and remap value naming material `from` now names `to`.
fn rename_material(doc: &mut Value, from: &str, to: &str) {
    fn visit(node: &mut Map<String, Value>, from: &str, to: &str) {
        if node.get("mat").and_then(Value::as_str) == Some(from) {
            node.insert("mat".into(), Value::String(to.into()));
        }
        for key in ["array", "grid", "radial"] {
            if let Some(Value::Array(cycle)) = node.get_mut(key).and_then(|r| r.get_mut("cycle")) {
                for entry in cycle.iter_mut().filter(|entry| entry.as_str() == Some(from)) {
                    *entry = Value::String(to.into());
                }
            }
        }
        if let Some(Value::Object(remap)) = node.get_mut("remap") {
            for value in remap.values_mut().filter(|value| value.as_str() == Some(from)) {
                *value = Value::String(to.into());
            }
        }
        if let Some(Value::Object(children)) = node.get_mut("parts") {
            for child in children.values_mut().filter_map(Value::as_object_mut) {
                visit(child, from, to);
            }
        }
    }
    let root = doc.as_object_mut().expect("a document is an object");
    if root.get("mat").and_then(Value::as_str) == Some(from) {
        root.insert("mat".into(), Value::String(to.into()));
    }
    if let Some(Value::Object(parts)) = root.get_mut("parts") {
        for node in parts.values_mut().filter_map(Value::as_object_mut) {
            visit(node, from, to);
        }
    }
}

/// Rewrites every attach target (parts and sockets) that reaches the node at `path`: `change`
/// gets the target's segment naming it (`leg[1]~x`) and gives its new text. Returns the parts
/// whose targets `change` refused.
fn retarget(doc: &mut Value, path: &[&str], change: &mut dyn FnMut(&str) -> Option<String>) -> Vec<String> {
    fn rewrite(attach: &mut Value, scope: &[String], path: &[&str], change: &mut dyn FnMut(&str) -> Option<String>) -> bool {
        let target = match attach {
            Value::String(target) => target,
            Value::Object(object) => match object.get_mut("to") {
                Some(Value::String(target)) => target,
                _ => return true,
            },
            _ => return true,
        };
        if path.len() <= scope.len() || !scope.iter().zip(path).all(|(a, b)| a == b) {
            return true;
        }
        let relative = &path[scope.len()..];
        let mut segments: Vec<String> = target.split('.').map(String::from).collect();
        if segments.len() < relative.len() || !relative.iter().zip(&segments).all(|(name, segment)| base(segment) == *name) {
            return true;
        }
        let last = relative.len() - 1;
        match change(&segments[last]) {
            Some(new) => {
                segments[last] = new;
                *target = segments.join(".");
                true
            }
            None => false,
        }
    }
    fn walk(
        parts: &mut Map<String, Value>,
        scope: &mut Vec<String>,
        path: &[&str],
        change: &mut dyn FnMut(&str) -> Option<String>,
        blocked: &mut Vec<String>,
    ) {
        for (name, node) in parts.iter_mut() {
            let Some(node) = node.as_object_mut() else { continue };
            if let Some(attach) = node.get_mut("attach") {
                if !rewrite(attach, scope, path, change) {
                    blocked.push(scope.iter().cloned().chain([name.clone()]).collect::<Vec<_>>().join("."));
                }
            }
            if let Some(Value::Object(children)) = node.get_mut("parts") {
                scope.push(name.clone());
                walk(children, scope, path, change, blocked);
                scope.pop();
            }
        }
    }
    let mut blocked = Vec::new();
    let root = doc.as_object_mut().expect("a document is an object");
    if let Some(Value::Object(parts)) = root.get_mut("parts") {
        walk(parts, &mut Vec::new(), path, change, &mut blocked);
    }
    if let Some(Value::Object(sockets)) = root.get_mut("sockets") {
        for (name, socket) in sockets.iter_mut() {
            if let Some(attach) = socket.get_mut("attach") {
                if !rewrite(attach, &[], path, change) {
                    blocked.push(format!("socket {name}"));
                }
            }
        }
    }
    blocked
}

/// Replace a replicated node by its copies `<name>_<i>`, each placed where the copy was.
/// Returns the new names.
fn expand(parent: &mut Map<String, Value>, name: &str) -> Result<Vec<String>, String> {
    let node = parent[name].as_object().ok_or_else(|| format!("{name} is not an object"))?.clone();
    let (copies, cycle) = copies(&node)?;
    let names: Vec<String> = copies.iter().map(|copy| format!("{name}_{}", copy.0)).collect();
    for new in &names {
        validate_part_name(new).map_err(|error| format!("{new} is not a part name: {error}"))?;
        if parent.contains_key(new) {
            return Err(format!("a part \"{new}\" is already beside it; rename it first"));
        }
    }
    let mut built = Vec::with_capacity(copies.len());
    for ((index, delta, turn), new) in copies.into_iter().zip(&names) {
        let mut copy = node.clone();
        for key in ["array", "grid", "radial"] {
            copy.remove(key);
        }
        if let Some(cycle) = &cycle {
            let entry = &cycle[index % cycle.len()];
            if entry.starts_with('#') || entry.starts_with('$') {
                copy.insert("color".into(), Value::String(entry.clone()));
                copy.remove("mat");
            } else {
                copy.insert("mat".into(), Value::String(entry.clone()));
            }
        }
        place(&mut copy, delta, turn).map_err(|error| format!("{name}[{index}]: {error}"))?;
        built.push((new.clone(), copy));
    }
    parent.remove(name);
    for (new, copy) in built {
        parent.insert(new, Value::Object(copy));
    }
    Ok(names)
}

/// A node's copies before mirroring, as compile makes them: (index, shift from the node's
/// position, turn about it), and the cycle.
#[allow(clippy::type_complexity)]
fn copies(node: &Map<String, Value>) -> Result<(Vec<(usize, Vec3, Quat)>, Option<Vec<String>>), String> {
    let number = |object: &Value, key: &str| object.get(key).and_then(Value::as_f64).map(|x| x as f32);
    let count =
        |object: &Value| object.get("count").and_then(Value::as_u64).filter(|n| (1..=1024).contains(n)).map(|n| n as usize);
    let cycle = |object: &Value| {
        object
            .get("cycle")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(|i| i.as_str().map(String::from)).collect())
    };
    let unreadable = |kind: &str| format!("its {kind} is not readable; fix it first");
    if let Some(array) = node.get("array") {
        let n = count(array).ok_or_else(|| unreadable("array"))?;
        let step = match (array.get("step").and_then(vec3), array.get("span").and_then(vec3)) {
            (Some(step), _) => step,
            (None, Some(span)) => span / (n as f32 - 1.0).max(1.0),
            _ => return Err(unreadable("array")),
        };
        let copies = (0..n).map(|i| (i, step * (i as f32 - (n as f32 - 1.0) * 0.5), Quat::IDENTITY)).collect();
        return Ok((copies, cycle(array)));
    }
    if let Some(grid) = node.get("grid") {
        let pair = |key: &str| grid.get(key).and_then(Value::as_array).filter(|items| items.len() == 2).cloned();
        let (Some(counts), Some(steps)) = (pair("count"), pair("step")) else { return Err(unreadable("grid")) };
        let [na, nb] = [0, 1].map(|i| counts[i].as_u64().unwrap_or(0) as usize);
        let [sa, sb] = [0, 1].map(|i| steps[i].as_f64().unwrap_or(0.0) as f32);
        let plane = grid.get("plane").and_then(Value::as_str).unwrap_or("xz");
        let axis = |c: char| match c {
            'x' => Some(Vec3::X),
            'y' => Some(Vec3::Y),
            'z' => Some(Vec3::Z),
            _ => None,
        };
        let mut letters = plane.chars();
        let (Some(a), Some(b)) = (letters.next().and_then(axis), letters.next().and_then(axis)) else {
            return Err(unreadable("grid"));
        };
        let mut copies = Vec::with_capacity(na * nb);
        for ib in 0..nb {
            for ia in 0..na {
                let offset = a * sa * (ia as f32 - (na as f32 - 1.0) * 0.5) + b * sb * (ib as f32 - (nb as f32 - 1.0) * 0.5);
                copies.push((ia + na * ib, offset, Quat::IDENTITY));
            }
        }
        return Ok((copies, cycle(grid)));
    }
    if let Some(radial) = node.get("radial") {
        let n = count(radial).ok_or_else(|| unreadable("radial"))?;
        let radius = number(radial, "radius").ok_or_else(|| unreadable("radial"))?;
        let axis = radial.get("axis").and_then(Value::as_str).and_then(Dir::parse).unwrap_or(Dir::Y);
        let zero = if matches!(axis, Dir::Y | Dir::NegY) { Vec3::Z } else { Vec3::Y };
        let arc = number(radial, "arc").unwrap_or(360.0);
        let step = if arc >= 360.0 { arc / n as f32 } else { arc / (n as f32 - 1.0).max(1.0) };
        let start = number(radial, "start").unwrap_or(0.0);
        let copies = (0..n)
            .map(|i| {
                let turn = Quat::from_axis_angle(axis.vector(), (start + step * i as f32).to_radians());
                (i, turn * (zero * radius), turn)
            })
            .collect();
        return Ok((copies, cycle(radial)));
    }
    Err("it has no array, grid or radial to expand".into())
}

/// Place one expanded copy: shifted by `delta` and turned by `turn` about its own position, in
/// the form it was written (from/to stay endpoints; pos and attach take yaw, pitch and roll).
fn place(copy: &mut Map<String, Value>, delta: Vec3, turn: Quat) -> Result<(), String> {
    let turned = turn.angle_between(Quat::IDENTITY) > 1e-6;
    let degrees = |key: &str, copy: &Map<String, Value>| copy.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if let (Some(from), Some(to)) = (copy.get("from").and_then(vec3), copy.get("to").and_then(vec3)) {
        let centre = (from + to) * 0.5;
        let (new_from, new_to) = (centre + delta + turn * (from - centre), centre + delta + turn * (to - centre));
        copy.insert("from".into(), point(new_from));
        copy.insert("to".into(), point(new_to));
        if turned {
            // The ends fix the axis; the spin about it keeps the copy's cross-section turned with it.
            let spin = degrees("spin", copy);
            let wanted = turn * ends_frame(from, to) * Quat::from_rotation_y(spin.to_radians());
            let about = ends_frame(new_from, new_to).inverse() * wanted;
            set_angle(copy, "spin", 2.0 * about.y.atan2(about.w).to_degrees());
        }
        return Ok(());
    }
    let key = if copy.contains_key("attach") { "offset" } else { "pos" };
    let moved = copy.get(key).and_then(vec3).unwrap_or(Vec3::ZERO) + delta;
    copy.insert(key.into(), point(moved));
    if !turned {
        return Ok(());
    }
    let rotation = turn * euler(degrees("yaw", copy), degrees("pitch", copy), degrees("roll", copy));
    let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
    for (key, angle) in [("yaw", yaw), ("pitch", pitch), ("roll", roll)] {
        set_angle(copy, key, angle.to_degrees());
    }
    // An arc keeps facing its copy's way, which only the six axis directions can name.
    let kind = copy.get("type").and_then(Value::as_str).and_then(PrimType::parse);
    let arc = copy.get("arc").and_then(Value::as_f64).unwrap_or(360.0);
    if let Some(kind) = kind.filter(|kind| kind.takes_arc() && arc < 360.0) {
        let authored = copy.get("axis").and_then(Value::as_str).and_then(Dir::parse).unwrap_or(kind.default_axis());
        let default = if matches!(authored, Dir::Y | Dir::NegY) { Dir::Z } else { Dir::Y };
        let facing = copy.get("facing").and_then(Value::as_str).and_then(Dir::parse).unwrap_or(default);
        let wanted = turn * facing.vector();
        let Some(dir) = Dir::NAMES.iter().filter_map(|name| Dir::parse(name)).find(|dir| dir.vector().distance(wanted) < 1e-4)
        else {
            return Err("its arc would face between the axes; only quarter turns keep an arc's facing".into());
        };
        if dir == default {
            copy.remove("facing");
        } else {
            copy.insert("facing".into(), Value::String(dir.name().into()));
        }
    }
    Ok(())
}

/// An angle in degrees, rounded; a zero angle is left out.
fn set_angle(object: &mut Map<String, Value>, key: &str, degrees: f32) {
    let mut degrees = round(degrees);
    if degrees <= -180.0 {
        degrees += 360.0;
    }
    if degrees.abs() < 1e-4 {
        object.remove(key);
    } else {
        object.insert(key.into(), serde_json::json!(degrees));
    }
}

fn round(x: f32) -> f64 {
    (x as f64 * 1e4).round() / 1e4
}

fn point(p: Vec3) -> Value {
    serde_json::json!([round(p.x), round(p.y), round(p.z)])
}

fn vec3(value: &Value) -> Option<Vec3> {
    let items = value.as_array().filter(|items| items.len() == 3)?;
    let x: Vec<f32> = items.iter().map(|item| item.as_f64().map(|x| x as f32)).collect::<Option<_>>()?;
    Some(Vec3::new(x[0], x[1], x[2]))
}

/// Where a node's lengths scale about by default: its position, or its ends' midpoint.
fn centre(node: &Map<String, Value>) -> Vec3 {
    match (node.get("from").and_then(vec3), node.get("to").and_then(vec3)) {
        (Some(from), Some(to)) => (from + to) * 0.5,
        _ => node.get("pos").and_then(vec3).unwrap_or(Vec3::ZERO),
    }
}

fn times(value: Option<&mut Value>, k: f64) {
    if let Some(value) = value {
        match value {
            Value::Number(number) => {
                if let Some(x) = number.as_f64() {
                    *value = serde_json::json!(x * k);
                }
            }
            Value::Array(items) => {
                for item in items {
                    times(Some(item), k);
                }
            }
            _ => {}
        }
    }
}

/// Scale a node about `about` (in its parent's frame): its position, and every length inside.
/// `refs`: a reference's own `scale` grows too (a unit change leaves it).
fn scale_node(node: &mut Map<String, Value>, k: f64, about: Vec3, refs: bool) {
    for key in ["pos", "from", "to"] {
        if let Some(p) = node.get(key).and_then(vec3) {
            let scaled = about.as_dvec3() + (p - about).as_dvec3() * k;
            node.insert(key.into(), serde_json::json!([scaled.x, scaled.y, scaled.z]));
        }
    }
    for key in [
        "size",
        "half",
        "radius",
        "radius_end",
        "minor_radius",
        "thickness",
        "height",
        "half_height",
        "depth",
        "profile",
        "points",
        "offset",
    ] {
        times(node.get_mut(key), k);
    }
    let nested = [
        ("array", "step"),
        ("array", "span"),
        ("grid", "step"),
        ("radial", "radius"),
        ("jitter", "pos"),
        ("light", "radius"),
        ("fx", "area"),
    ];
    for (outer, inner) in nested {
        times(node.get_mut(outer).and_then(|o| o.get_mut(inner)), k);
    }
    if refs && (node.contains_key("ref") || node.contains_key("mesh")) {
        let scale = node.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
        node.insert("scale".into(), serde_json::json!(scale * k));
    }
    if let Some(Value::Object(children)) = node.get_mut("parts") {
        for child in children.values_mut().filter_map(Value::as_object_mut) {
            scale_node(child, k, Vec3::ZERO, refs);
        }
    }
}

/// Every length in the document: the parts about `about`, sockets, and material patterns.
fn scale_document(doc: &mut Value, k: f64, about: Vec3, refs: bool) {
    let root = doc.as_object_mut().expect("a document is an object");
    if let Some(Value::Object(parts)) = root.get_mut("parts") {
        for node in parts.values_mut().filter_map(Value::as_object_mut) {
            scale_node(node, k, about, refs);
        }
    }
    if let Some(Value::Object(sockets)) = root.get_mut("sockets") {
        for socket in sockets.values_mut().filter_map(Value::as_object_mut) {
            if let Some(p) = socket.get("pos").and_then(vec3) {
                let scaled = about.as_dvec3() + (p - about).as_dvec3() * k;
                socket.insert("pos".into(), serde_json::json!([scaled.x, scaled.y, scaled.z]));
            }
            times(socket.get_mut("offset"), k);
        }
    }
    if let Some(Value::Object(materials)) = root.get_mut("materials") {
        for material in materials.values_mut() {
            if let Some(Value::Array(layers)) = material.get_mut("layers") {
                for layer in layers.iter_mut().filter_map(Value::as_object_mut) {
                    for key in ["size", "radius", "gap", "bevel", "from", "to"] {
                        times(layer.get_mut(key), k);
                    }
                }
            }
        }
    }
}
