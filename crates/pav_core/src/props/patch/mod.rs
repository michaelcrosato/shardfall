//! Patches: a batch of named-part operations applied to the document's JSON, then the whole
//! result is checked like a file. The batch is atomic, and every problem names the operation
//! that caused it: `ops[2] seat.color: "grey" is not a #rrggbb colour — ...`.

use std::collections::BTreeMap;

use serde_json::Value;

use super::read::{OPS, check};
use super::{PropAsset, validate_part_name};
use crate::issue::{self, Code, Issue};

pub mod v2;

pub const MAX_OPS: usize = 1024;

impl PropAsset {
    /// Apply a named-part transaction. The original stays unchanged if any operation fails.
    pub fn patch(&self, ops: &Value) -> Result<Self, String> {
        self.patch_issues(ops).map_err(|issues| issue::render(&issues))
    }

    pub fn patch_issues(&self, ops: &Value) -> Result<Self, Vec<Issue>> {
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
        let mut doc = serde_json::to_value(self).expect("prop definition serializes");
        // Which operation last touched each part, and whether it replaced the shape.
        let mut touched: BTreeMap<String, (usize, bool)> = BTreeMap::new();
        let mut issues = Vec::new();
        for (index, op) in ops.iter().enumerate() {
            let parts = doc["parts"].as_object_mut().expect("parts are an object");
            match apply(index, op, parts) {
                Ok(Some((part, shape))) => {
                    touched.insert(part, (index, shape));
                }
                Ok(None) => {}
                Err(issue) => issues.push(issue),
            }
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let issues: Vec<Issue> = check(&doc).into_iter().map(|issue| readdress(issue, &touched)).collect();
        if !issues.is_empty() {
            return Err(issues);
        }
        Self::from_value(doc)
    }
}

/// `parts.seat.color` → `ops[2] seat.color` when op 2 last touched `seat`.
fn readdress(mut issue: Issue, touched: &BTreeMap<String, (usize, bool)>) -> Issue {
    let Some(rest) = issue.path.strip_prefix("parts.") else { return issue };
    let part = rest.split(['.', '[']).next().unwrap_or(rest);
    if let Some((index, shape)) = touched.get(part) {
        if *shape && rest[part.len()..].starts_with(".shape") && issue.code == Code::Missing {
            let fix = issue.fix.take().unwrap_or_else(|| "give its type and every field".into());
            issue.fix = Some(format!("set replaces the whole shape: {fix}"));
        }
        issue.path = format!("ops[{index}] {rest}");
    }
    issue
}

fn op_keys(kind: &str) -> Option<&'static [&'static str]> {
    OPS.iter().find(|(name, _, _)| *name == kind).map(|(_, keys, _)| *keys)
}

/// Apply one operation to the parts map. Returns the part it touched and whether it set a
/// shape.
fn apply(index: usize, op: &Value, parts: &mut serde_json::Map<String, Value>) -> Result<Option<(String, bool)>, Issue> {
    let at = |key: &str| format!("ops[{index}].{key}");
    let names: Vec<&str> = OPS.iter().map(|(name, _, _)| *name).collect();
    let object = op.as_object().ok_or_else(|| {
        Issue::error(format!("ops[{index}]"), Code::InvalidType, "expected an operation {op,part,...}").expected(names.join(", "))
    })?;
    let kind = match object.get("op") {
        Some(Value::String(kind)) => kind.as_str(),
        Some(_) => {
            return Err(Issue::error(at("op"), Code::InvalidType, "expected the operation name").expected(names.join(", ")));
        }
        None => return Err(Issue::error(at("op"), Code::Missing, "missing").expected(names.join(", "))),
    };
    let Some(keys) = op_keys(kind) else {
        let fix = match kind {
            "mirror" | "copy" | "rename" | "array" | "move" | "scale" | "duplicate" => {
                Some("format 1 batches use set, add and remove: add the new part with op:add".to_string())
            }
            "update" | "modify" | "edit" | "change" => Some("use op:set".to_string()),
            "delete" | "del" => Some("use op:remove".to_string()),
            "insert" | "create" | "new" => Some("use op:add".to_string()),
            _ => issue::suggestion(kind, names.iter().copied()),
        };
        return Err(Issue::error(at("op"), Code::UnknownValue, format!("unknown op \"{kind}\""))
            .expected(names.join(", "))
            .maybe_fix(fix));
    };
    if let Some(key) = object.keys().find(|key| *key != "op" && !keys.contains(&key.as_str())) {
        let fix = match (kind, key.as_str()) {
            (_, "path" | "id" | "name" | "target") => Some("props address a part with \"part\"".to_string()),
            ("set", "value") => Some("set takes \"fields\" to merge".to_string()),
            ("add", "fields") => Some("add takes the whole part as \"value\"".to_string()),
            _ => issue::suggestion(key, keys.iter().copied()),
        };
        return Err(Issue::error(at(key), Code::UnknownKey, format!("unknown key for op {kind}"))
            .expected(keys.join(", "))
            .maybe_fix(fix));
    }
    let part = match object.get("part") {
        Some(Value::String(part)) => part.clone(),
        Some(_) => return Err(Issue::error(at("part"), Code::InvalidType, "expected a part name")),
        None => return Err(Issue::error(at("part"), Code::Missing, "missing")),
    };
    let existing = |parts: &serde_json::Map<String, Value>| issue::suggestion(&part, parts.keys().map(String::as_str));
    let where_ = format!("ops[{index}] {part}");
    match kind {
        "set" => {
            if !parts.contains_key(&part) {
                return Err(Issue::error(where_, Code::UnknownPart, format!("no part \"{part}\"")).maybe_fix(existing(parts)));
            }
            // An earlier op in this batch may have added a malformed part.
            let Some(target) = parts.get_mut(&part).and_then(Value::as_object_mut) else {
                return Err(Issue::error(where_, Code::InvalidType, format!("part \"{part}\" is not an object {{shape,...}}")));
            };
            let fields = match object.get("fields") {
                Some(Value::Object(fields)) => fields,
                Some(_) => return Err(Issue::error(at("fields"), Code::InvalidType, "expected an object of part fields")),
                None => return Err(Issue::error(at("fields"), Code::Missing, "missing")),
            };
            for (key, value) in fields {
                target.insert(key.clone(), value.clone());
            }
            Ok(Some((part, fields.contains_key("shape"))))
        }
        "add" => {
            if let Err(error) = validate_part_name(&part) {
                return Err(Issue::error(where_, Code::UnknownValue, format!("\"{part}\" is not a part name: {error}")));
            }
            if parts.contains_key(&part) {
                return Err(Issue::error(where_, Code::Duplicate, format!("part \"{part}\" already exists"))
                    .fix("use op:set to change it"));
            }
            let value = object.get("value").ok_or_else(|| Issue::error(at("value"), Code::Missing, "missing"))?;
            parts.insert(part.clone(), value.clone());
            Ok(Some((part, false)))
        }
        "remove" => {
            if parts.remove(&part).is_none() {
                return Err(
                    Issue::error(where_, Code::UnknownPart, format!("no part \"{part}\" to remove")).maybe_fix(existing(parts))
                );
            }
            Ok(None)
        }
        _ => unreachable!("operation names come from OPS"),
    }
}
