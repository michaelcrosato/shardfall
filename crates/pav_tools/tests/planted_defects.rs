//! The five planted defects of docs/AGENT_ASSET_STORE.md §7 (S2), each caught by what an agent
//! runs: `asset_edit action=check` finds a 32 cm table and a 48 cm chair (`size_band`), a prop
//! sunk 76 cm (`below_floor`) and a sword with no grip (`socket_missing`); `asset_import` refuses
//! a model whose texture is missing.

mod common;

use common::{MeshBuilder, temp_dir};
use pav_core::props::{self, Doc, PropDef};
use pav_tools::asset_tools::check_lines;
use pav_tools::{Output, Session, call};
use serde_json::{Value, json};

fn def(doc: Doc) -> PropDef {
    PropDef::new(doc).unwrap_or_else(|e| panic!("{}", pav_core::issue::render(&e)))
}

/// A copy of a core asset with `ops` applied.
fn patched(name: &str, ops: Value) -> PropDef {
    let core = props::get(name).unwrap();
    def(props::patch_document(&core.doc, &ops, &props::get).unwrap_or_else(|e| panic!("{}", pav_core::issue::render(&e))).doc)
}

/// A copy of a core asset scaled to `cm` tall.
fn scaled(name: &str, cm: f32) -> PropDef {
    let by = cm / (props::get(name).unwrap().compiled.bounds.size().y * 100.0);
    patched(name, json!([{"op": "scale", "by": by}]))
}

/// The defect's finding or error, if the check an agent runs catches it.
fn caught(found: &[String], code: &str) -> Option<String> {
    found.iter().find(|line| line.starts_with(&format!("{code}: "))).cloned()
}

#[test]
fn five_planted_defects_are_caught() {
    let mut report = Vec::new();

    // 1-2. Core furniture scaled to doll size.
    let table = check_lines(&scaled("LIB/core/table", 32.0));
    report.push(("a 32 cm table", caught(&table, "size_band")));
    let chair = check_lines(&scaled("LIB/core/chair", 48.0));
    report.push(("a 48 cm chair", caught(&chair, "size_band")));

    // 3. A core prop sunk 76 cm (the barrel: a 78 cm table sunk 76 cm would hang from y=2 cm).
    let barrel = props::get("LIB/core/barrel").unwrap();
    let ops: Vec<Value> = barrel.doc.parts.keys().map(|part| json!({"op": "move", "part": part, "by": [0, -76, 0]})).collect();
    let sunk = check_lines(&patched("LIB/core/barrel", json!(ops)));
    report.push(("a barrel sunk 76 cm", caught(&sunk, "below_floor")));

    // 4. A sword with no grip socket; with one it is clean.
    let mut sword = json!({"format": 2, "name": "sword", "unit": "cm", "description": "A plain arming sword.",
        "tags": ["sword", "weapon"], "category": "weapon", "parts": {
            "pommel": {"type": "sphere", "radius": 2, "pos": [0, 2, 0], "color": "#8a7040"},
            "grip": {"type": "cylinder", "radius": 1.5, "height": 14, "pos": [0, 11, 0], "color": "#4a3020"},
            "guard": {"type": "box", "size": [18, 3, 3], "pos": [0, 19.5, 0], "color": "#8a7040"},
            "blade": {"type": "box", "size": [5, 75, 1], "pos": [0, 58.5, 0], "color": "#c8ccd4"}}});
    let read = |v: &Value| def(Doc::read(&v.to_string()).unwrap_or_else(|e| panic!("{}", pav_core::issue::render(&e))));
    let gripless = check_lines(&read(&sword));
    assert_eq!(
        gripless,
        ["socket_missing: a sword needs a grip socket (sockets: grip on the handle, +y along the blade or shaft)"]
    );
    report.push(("a sword with no grip", caught(&gripless, "socket_missing")));
    sword["sockets"] = json!({"grip": {"pos": [0, 11, 0]}});
    assert_eq!(check_lines(&read(&sword)), Vec::<String>::new(), "the fix clears it");

    // 5. A model whose base texture is missing: the import fails, naming it, and writes nothing.
    let dir = temp_dir("planted_texture");
    let mut b = MeshBuilder::new();
    b.images.push(json!({"uri": "crate_wood.png"}));
    b.textures.push(json!({"source": 0}));
    let mat = b.material(json!({"name": "wood", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}}}));
    let p = b.box_prim([-0.3, 0.0, -0.3], [0.3, 0.6, 0.3], Some(mat));
    let m = b.mesh(vec![p]);
    b.node(json!({"name": "crate", "mesh": m}));
    let model = dir.join("crate.gltf");
    std::fs::write(&model, b.gltf_embedded(&[0])).unwrap();
    let mut s = Session::new("empty", 1).unwrap();
    let out = dir.join("out");
    let args = json!({"from": model.to_str().unwrap(), "out": out.to_str().unwrap()});
    let imported = call(&mut s, "asset_import", args.as_object().unwrap());
    let error = match imported {
        Ok(Output::Json(v)) => panic!("translated: {v}"),
        Ok(Output::Image { .. }) => panic!("translated"),
        Err(e) => e.to_string(),
    };
    assert!(!out.join("crate.json").exists());
    report.push(("a missing texture", error.contains("texture crate_wood.png").then_some(error)));

    let lines: Vec<String> =
        report.iter().map(|(what, by)| format!("{what}: {}", by.as_deref().unwrap_or("not caught"))).collect();
    let n = report.iter().filter(|(_, by)| by.is_some()).count();
    println!("planted defects caught: {n}/5\n{}", lines.join("\n"));
    assert_eq!(n, 5, "{lines:#?}");
}
