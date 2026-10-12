//! Material files' canonical text and revisions (asset plan §6.1): the header fields, then the
//! material fields in the schema's order, each layer on its own line with `type` first, numbers
//! as format-2 props write them. A revision is `mat1-` and FNV-1a over the compact text, so it
//! does not depend on how a document arrived.

use serde_json::{Map, Value};

use crate::props::{MaterialText, material_entries};

/// Keys a file's text starts with, in this order.
const HEADER: &[&str] = &["format", "name", "unit", "description", "tags", "source", "fit"];

/// The file text: a header line, one line per field, and one per layer.
pub fn text(file: &Map<String, Value>) -> String {
    let entries = material_entries(file, HEADER);
    let mut header = Vec::new();
    let mut lines = Vec::new();
    for (key, value) in &entries {
        let key_text = serde_json::to_string(key).expect("strings serialize");
        let line = match value {
            MaterialText::Layers(layers) if !layers.is_empty() => format!("{key_text}:[\n {}]", layers.join(",\n ")),
            value => format!("{key_text}:{value}"),
        };
        if ["format", "name", "unit"].contains(&key.as_str()) {
            header.push(line);
        } else {
            lines.push(line);
        }
    }
    let mut out = format!("{{{}", header.join(","));
    for line in lines {
        out.push_str(",\n");
        out.push_str(&line);
    }
    out.push_str("}\n");
    out
}

/// The compact text a revision hashes.
pub fn compact(file: &Map<String, Value>) -> String {
    let entries = material_entries(file, HEADER);
    let parts: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}:{value}", serde_json::to_string(key).expect("strings serialize")))
        .collect();
    format!("{{{}}}", parts.join(","))
}

/// `mat1-` and 16 hex digits.
pub fn revision(file: &Map<String, Value>) -> String {
    format!("mat1-{:016x}", crate::props::fnv(compact(file).as_bytes()))
}
