//! Props and statics wear materials (asset plan MAT-4): materials tables with library names
//! and inline materials, `mat` on parts and groups, explicit-only part colours, edits that add
//! a material and use it at once, snapshots, and `check` on names the library lacks.

use glam::{Quat, Vec3};
use pav_core::material::Param;
use pav_core::props::lint::{self, Options};
use pav_core::props::{self, Doc, PropAsset, PropDef, ResolvedPart};
use pav_core::statics::Block;
use pav_core::{Color, Sim};
use serde_json::json;

const BARREL: &str = include_str!("fixtures/props/materials/banded_barrel.json");

fn part<'a>(def: &'a PropDef, id: &str) -> &'a ResolvedPart {
    def.compiled.parts.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("no part {id}"))
}

fn mat(part: &ResolvedPart) -> Option<&str> {
    part.mat.as_ref().map(|m| m.name.as_str())
}

#[test]
fn parts_wear_library_and_inline_materials() {
    let text = r##"{"format":2,"name":"mat_probe","unit":"cm",
"materials":{"wood":"MAT/oak_planks",
"moss":{"base":"MAT/cobblestone","color":"#556b2f","layers":[{"type":"noise","size":40,"cover":0.4,"color":"#334422"}]}},
"parts":{
"a":{"type":"box","size":[50,50,50],"mat":"wood"},
"b":{"type":"box","size":[50,50,50],"pos":[60,0,0],"mat":"MAT/wrought_iron"},
"c":{"type":"box","size":[50,50,50],"pos":[120,0,0],"mat":"wood","color":"#ff0000"},
"g":{"mat":"moss","pos":[0,0,100],"parts":{
"d":{"type":"sphere","radius":20},
"e":{"type":"cylinder","radius":10,"height":40,"color":"#00ff00"}}}}}"##;
    let doc = Doc::read(text).unwrap();
    let def = PropDef::new(doc.clone()).unwrap();
    assert!(def.compiled.warnings.iter().all(|w| w.code == "floating"), "{:?}", def.compiled.warnings);
    // The colour is the part's own only when it writes one; else its material's.
    let (a, b, c) = (part(&def, "a"), part(&def, "b"), part(&def, "c"));
    assert_eq!((a.color.as_deref(), mat(a), a.surface_color()), (None, Some("MAT/oak_planks"), "#a0703f"));
    assert_eq!((b.color.as_deref(), mat(b), b.surface_color()), (None, Some("MAT/wrought_iron"), "#3b3735"));
    assert_eq!((c.color.as_deref(), mat(c)), (Some("#ff0000"), Some("MAT/oak_planks")), "a colour keeps the patterns");
    // An inline material is the asset's own, read in its unit, its base's layers replaced.
    let (d, e) = (part(&def, "g.d"), part(&def, "g.e"));
    assert_eq!((d.color.as_deref(), mat(d), d.surface_color()), (None, Some("#moss"), "#556b2f"));
    assert_eq!((e.color.as_deref(), mat(e)), (Some("#00ff00"), Some("#moss")));
    let moss = d.mat.as_ref().unwrap().material.as_ref().unwrap();
    assert_eq!(moss.layers.len(), 1);
    assert_eq!(moss.layers[0].params.get("size"), Some(&Param::Num(0.4)), "40 cm");
    assert_eq!(moss.layers_from, "", "its own layers, not the cobblestone's");
    // It writes back as written and reads to the same revision.
    let written = doc.text();
    assert!(
        written.contains(r##""moss":{"base":"MAT/cobblestone","color":"#556b2f","layers":[{"type":"noise","size":40,"cover":0.4,"color":"#334422"}]}"##),
        "{written}"
    );
    assert!(written.contains(r#""wood":"MAT/oak_planks""#), "{written}");
    assert_eq!(Doc::read(&written).unwrap().revision(), doc.revision());
    // The demo barrel compiles clean: staves, iron hoops and a plank lid.
    let barrel = PropDef::new(Doc::read(BARREL).unwrap()).unwrap();
    let names: Vec<_> = barrel.compiled.parts.iter().map(|p| (p.id.as_str(), mat(p).unwrap())).collect();
    assert_eq!(
        names,
        [
            ("body", "MAT/barrel_staves"),
            ("bung", "MAT/wrought_iron"),
            ("hoop_high", "MAT/wrought_iron"),
            ("hoop_low", "MAT/wrought_iron"),
            ("lid", "MAT/oak_planks")
        ]
    );
    let found = lint::lint(&barrel.compiled, &Options::default());
    assert!(found.is_empty(), "{:?}", found.iter().map(|f| &f.line).collect::<Vec<_>>());
}

#[test]
fn format1_colours_stay_explicit_and_revisions_hold() {
    // Format 1's default colour is written into the source model as the part's own: the
    // revision, the drawn colour and the default_color finding are what they were.
    let implicit = r#"{"format":1,"name":"plain","parts":{"p":{"shape":{"type":"box","half":[0.5,0.5,0.5]}}}}"#;
    let explicit =
        r##"{"format":1,"name":"plain","parts":{"p":{"shape":{"type":"box","half":[0.5,0.5,0.5]},"color":"#e8704a"}}}"##;
    let (a, b) = (Doc::read(implicit).unwrap(), Doc::read(explicit).unwrap());
    assert_eq!(a.parts["p"].surface.color.as_deref(), Some("#e8704a"));
    assert_eq!(a.revision(), b.revision());
    assert_eq!(a.revision(), PropAsset::parse(implicit).unwrap().revision(), "prop1- over format 1's own layout");
    let def = PropDef::new(a).unwrap();
    assert_eq!(def.compiled.parts[0].color.as_deref(), Some("#e8704a"));
    assert!(lint::lint(&def.compiled, &Options::default()).iter().any(|f| f.code == "default_color"));
    // The templates keep their revisions, and every part its own colour and no material.
    for (name, revision) in [
        ("BUILTIN/box", "prop1-501ba5dc44b112fd"),
        ("BUILTIN/crate", "prop1-464f1ac1ee324075"),
        ("BUILTIN/bench", "prop1-1dec2d266aa7c89d"),
        ("BUILTIN/lantern", "prop1-fc44b95ee89f262e"),
    ] {
        let def = props::get(name).unwrap();
        assert_eq!(def.revision, revision);
        assert!(def.compiled.parts.iter().all(|p| p.color.is_some() && p.mat.is_none()), "{name}");
    }
}

#[test]
fn check_reports_names_the_library_lacks() {
    let text = r##"{"format":2,"name":"typo","unit":"cm",
"materials":{"rough":{"base":"MAT/cobblestones","scale":0.5}},
"parts":{"a":{"type":"box","size":[50,50,50],"mat":"MAT/oak_plank"},
"b":{"type":"box","size":[50,50,50],"pos":[50,0,0],"mat":"MAT/oak_plank"},
"c":{"type":"box","size":[50,50,50],"pos":[100,0,0],"mat":"rough"}}}"##;
    let def = PropDef::new(Doc::read(text).unwrap()).expect("an unknown name still compiles: snapshots keep loading");
    let lines: Vec<String> = lint::lint(&def.compiled, &Options::default())
        .into_iter()
        .filter(|f| f.code == "unknown_material")
        .map(|f| f.line)
        .collect();
    assert_eq!(
        lines,
        [
            r#"unknown_material: MAT/oak_plank is not in the library (a, b) — did you mean "MAT/oak_planks"?"#,
            r#"unknown_material: materials.rough does not resolve (c): base: base "MAT/cobblestones" is not in the library — did you mean "MAT/cobblestone"?"#,
        ]
    );
    assert_eq!(part(&def, "a").surface_color(), "#ff00ff", "a missing material shows");
    // A name that is not written as one is a reader error.
    let bad = pav_core::issue::render(&Doc::read(&text.replace("MAT/oak_plank\"", "MAT/Oak\"")).unwrap_err());
    assert!(bad.contains(r#"parts.a.mat: "MAT/Oak" is not a library material name"#), "{bad}");
}

#[test]
fn one_edit_adds_a_material_and_uses_it() {
    let base = Doc::read(BARREL).unwrap();
    let ops = json!([
        {"op": "add", "material": "brass", "value": {"base": "MAT/pb/gold", "roughness": 0.4}},
        {"op": "set", "part": "bung", "fields": {"mat": "brass"}},
        {"op": "set", "material": "lid", "fields": {"scale": 0.8}},
    ]);
    let patched = props::patch_document(&base, &ops, &props::get).unwrap();
    let def = PropDef::new(patched.doc.clone()).unwrap();
    assert_eq!(mat(part(&def, "bung")), Some("#brass"));
    assert_eq!(part(&def, "bung").surface_color(), "#d1a400");
    // A library name written as an entry becomes an inline material based on it when set.
    let lid = part(&def, "lid").mat.as_ref().unwrap();
    assert_eq!((lid.name.as_str(), lid.material.as_ref().unwrap().scale), ("#lid", 0.8));
    // Referencing a material the batch does not add rejects the whole batch.
    let ops = json!([
        {"op": "set", "part": "lid", "fields": {"color": "#000000"}},
        {"op": "set", "part": "bung", "fields": {"mat": "brass"}},
    ]);
    let errors = pav_core::issue::render(&props::patch_document(&base, &ops, &props::get).unwrap_err());
    assert!(errors.contains(r#"ops[1] bung.mat: no material "brass""#), "{errors}");
}

#[test]
fn snapshots_keep_props_that_wear_materials() {
    let mut sim = Sim::empty(5);
    let mut doc = Doc::read(BARREL).unwrap();
    doc.name = "snapshot_barrel".into();
    let def = PropDef::new(doc).unwrap();
    let compiled = def.compiled.clone();
    props::install("WORKSHOP/snapshot_barrel", def).unwrap();
    let id = sim.spawn_prop("WORKSHOP/snapshot_barrel", Vec3::ZERO, Quat::IDENTITY, 1.0, true, None).unwrap();
    let hash = sim.state_hash();
    let mut snapshot = Vec::new();
    ciborium::into_writer(&sim.snapshot(), &mut snapshot).unwrap();
    props::remove("WORKSHOP/snapshot_barrel").unwrap();
    sim.restore(ciborium::from_reader(snapshot.as_slice()).unwrap());
    assert_eq!(sim.state_hash(), hash);
    let prop = sim.state.entities.get(id).unwrap().prop.as_ref().unwrap();
    assert_eq!(*prop.parts(), *compiled, "the same parts, materials and all");
}

#[test]
fn statics_name_materials_only_when_they_wear_one() {
    let plain = Block::new(Vec3::ZERO, Vec3::ONE, Color::hex("#808080"));
    let worn = plain.clone().with_material("MAT/flagstones");
    let text = |b: &Block| serde_json::to_string(b).unwrap();
    assert!(!text(&plain).contains("material"), "snapshots without materials keep their bytes");
    assert!(text(&worn).contains(r#""material":"MAT/flagstones""#));
    let back: Block = serde_json::from_str(&text(&worn)).unwrap();
    assert_eq!(back.material.as_deref(), Some("MAT/flagstones"));
}

/// Only the nearest material counts: a part wearing its own does not glow with an outer one's
/// emissive, while a part without one takes it.
#[test]
fn an_outer_materials_glow_stops_at_a_parts_own_material() {
    let text = r##"{"format":2,"name":"glow_probe","unit":"cm","mat":"glow",
"materials":{"glow":{"color":"#40c0ff","emissive":1.1}},
"parts":{
"base":{"type":"box","size":[60,20,60],"pos":[0,10,0],"mat":"MAT/cobblestone"},
"shard":{"type":"box","size":[10,60,10],"pos":[0,50,0]}}}"##;
    let def = PropDef::new(Doc::read(text).unwrap()).unwrap();
    assert_eq!(part(&def, "base").emissive, 0.0, "its own material writes no glow");
    assert_eq!(part(&def, "shard").emissive, 1.1, "the document's material glows");
}
