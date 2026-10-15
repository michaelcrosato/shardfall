//! Parametric families (format-2 `params` and `presets`, plan §9.4, row LIB-4): the fence
//! family's variants, placement by preset or knob values, snapshots and rooms, and refusals.

use glam::{Quat, Vec3};
use pav_core::prop_instance::PropInstance;
use pav_core::props::{self, Doc, Params, PropDef};
use pav_core::{EntityId, Sim, issue};
use serde_json::{Value, json};

const FENCE: &str = include_str!("fixtures/props/families/fence.json");

/// The fence family installed as `WORKSHOP/<name>`.
fn fence(name: &str) -> String {
    let mut doc = Doc::read(FENCE).unwrap();
    doc.name = name.into();
    let asset = format!("WORKSHOP/{name}");
    props::install(&asset, PropDef::new(doc).unwrap()).unwrap();
    asset
}

fn params(values: Value) -> Params {
    values.as_object().unwrap().clone().into_iter().collect()
}

fn shapes_of(sim: &Sim, id: EntityId) -> usize {
    let body = sim.state.entities.get(id).unwrap().body.unwrap();
    let collider = sim.state.physics.bodies.get(body).unwrap().colliders()[0];
    sim.state.physics.colliders.get(collider).unwrap().shape().as_compound().unwrap().shapes().len()
}

#[test]
fn fence_family_default_low_and_long_are_13_11_and_17_parts() {
    let doc = Doc::read(FENCE).unwrap();
    assert_eq!((doc.params.len(), doc.presets.len()), (4, 3));
    // The canonical text ends with params, then presets, and reads back to the same family.
    let text = doc.text();
    let at = |key: &str| text.find(&format!("\n\"{key}\":")).unwrap_or_else(|| panic!("{key} in {text}"));
    assert!(at("parts") < at("params") && at("params") < at("presets"), "{text}");
    assert_eq!(Doc::read(&text).unwrap(), doc);
    let asset = fence("family_fence");
    let parts = |name: &str| props::resolve_parts(name, 0).unwrap().parts.len();
    assert_eq!([parts(&asset), parts(&format!("{asset}:low")), parts(&format!("{asset}:long"))], [13, 11, 17]);
    // `ruined` leaves out pickets and rails by seed; some seed draws fewer than 13.
    let ruined = (0..16).map(|seed| props::resolve_parts(&format!("{asset}:ruined"), seed).unwrap().parts.len());
    assert!(ruined.clone().all(|n| n <= 13) && ruined.min().unwrap() < 13);

    // Knob values: a default is left out, and the variant follows the rest.
    let def = props::get(&asset).unwrap();
    let values = def.param_values(None, &params(json!({"pickets": 9, "height": 140}))).unwrap();
    assert_eq!(values, params(json!({"height": 140})));
    let tall = def.try_variant(&values, 0).unwrap().bounds_at(1.0).size().y;
    assert!((tall - 1.4).abs() < 1e-3 && (def.compiled.bounds_at(1.0).size().y - 1.1).abs() < 1e-3, "{tall}");
    // Over a preset; the cache keeps one compile per (params, seed).
    let (name, _, values) = props::resolve_placed(&format!("{asset}:low"), &params(json!({"pickets": 5}))).unwrap();
    assert_eq!((name.as_str(), values.clone()), (asset.as_str(), params(json!({"height": 70, "pickets": 5}))));
    assert!(std::sync::Arc::ptr_eq(&def.variant_with(&values, 0), &def.variant_with(&values, 0)));
    assert_eq!(def.variant_with(&values, 0).parts.len(), 9);
}

#[test]
fn placements_name_presets_and_knobs_or_are_refused_with_a_fix() {
    let asset = fence("family_fence_placing");
    let error = |name: &str, given: Value| props::resolve_placed(name, &params(given)).unwrap_err();
    assert!(
        error(&format!("{asset}:lo"), json!({}))
            .contains("no preset \"lo\" (expected long, low, ruined) — did you mean \"low\"?")
    );
    assert!(
        error(&asset, json!({"heigth": 70})).contains("params.heigth: unknown param")
            && error(&asset, json!({"heigth": 70})).contains("did you mean \"height\"?")
    );
    assert!(error(&asset, json!({"height": 200})).contains("params.height: 200 is outside 60 to 140"));
    assert!(error(&asset, json!({"pickets": 7.5})).contains("use 7 or 8"));
    assert!(error(&asset, json!({"broken": "maybe"})).contains("unknown choice \"maybe\" (expected no, yes)"));
    assert!(error("BUILTIN/bench:low", json!({})).contains("bench has no params"));
    assert!(props::canonical_placed("WORKSHOP/x:Low").is_err());
}

#[test]
fn a_placed_family_copy_keeps_its_knobs_through_snapshots_and_rooms() {
    let asset = fence("family_fence_snapshot");
    let mut sim = Sim::empty(43);
    let def = props::get(&asset).unwrap();
    let knobs = params(json!({"length": 300, "pickets": 13}));
    let copy = PropInstance::new(&asset, def, 1.0, true).unwrap().with_params(&knobs).unwrap();
    let id = sim.place_prop(copy, Vec3::ZERO, Quat::IDENTITY, None).unwrap();
    let low = sim.spawn_prop(&format!("{asset}:low"), Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY, 1.0, true, None).unwrap();
    assert_eq!((shapes_of(&sim, id), shapes_of(&sim, low)), (17, 11), "each copy collides with its own variant");
    let drawn: Vec<usize> = sim.frame().objects.iter().filter_map(|o| Some(o.prop.as_ref()?.parts.parts.len())).collect();
    assert_eq!(drawn, [17, 11]);
    let hash = sim.state_hash();
    let mut snapshot = Vec::new();
    ciborium::into_writer(&sim.snapshot(), &mut snapshot).unwrap();
    // The snapshot carries the family once; the copies name their knobs.
    props::remove(&asset).unwrap();
    sim.restore(ciborium::from_reader(snapshot.as_slice()).unwrap());
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.state.prop_defs.len(), 1);
    let prop = sim.state.entities.get(id).unwrap().prop.as_ref().unwrap();
    assert_eq!((&prop.params, prop.parts().parts.len(), shapes_of(&sim, id)), (&knobs, 17, 17));
    let prop = sim.state.entities.get(low).unwrap().prop.as_ref().unwrap();
    assert_eq!((prop.asset.as_str(), &prop.params), (asset.as_str(), &params(json!({"height": 70, "pickets": 7}))));

    // A room names the preset and knobs over it; saving writes the knobs back.
    let asset = fence("family_fence_room");
    let source = pav_core::room::load_sources(None).into_iter().find(|r| r.key == "sandbox").unwrap();
    let object: pav_core::room::ObjectDef = toml::from_str(&format!(
        "name = \"fence\"\nasset = \"{asset}:low\"\npos = [3.0, 0.0, 4.0]\nbody = \"fixed\"\nparams = {{ broken = \"yes\", pickets = 9 }}\n"
    ))
    .unwrap();
    let text = pav_core::world::object_toml(&object);
    assert!(text.ends_with("params = { broken = \"yes\", pickets = 9 }\n"), "{text}");
    let room = pav_core::room::RoomDef::parse(&pav_core::world::rewrite_room_objects(&source.text, &[object])).unwrap();
    let mut sim = Sim::empty(47);
    sim.build_world(vec![("fence_room".into(), room)], Vec::new());
    sim.teleport_to_room("fence_room");
    let placed = sim.state.entities.find("fence").unwrap().prop.as_ref().unwrap();
    // pickets 9 is the default, so only the preset's height stays beside broken.
    assert_eq!(placed.params, params(json!({"broken": "yes", "height": 70})));
    let saved = &sim.room_objects(sim.state.world.room("fence_room").unwrap().id)[0];
    assert_eq!((saved.asset.as_deref(), &saved.params), (Some(asset.as_str()), &placed.params));
}

#[test]
fn families_refuse_bad_targets_disagreeing_defaults_and_limits() {
    let fence: Value = serde_json::from_str(FENCE).unwrap();
    let issues = |edit: &dyn Fn(&mut Value)| {
        let mut doc = fence.clone();
        edit(&mut doc);
        Doc::from_value(&doc).map(|_| String::new()).unwrap_or_else(|found| issue::render(&found))
    };
    let has = |text: String, wanted: &[&str]| {
        for want in wanted {
            assert!(text.contains(want), "{want:?} not in:\n{text}");
        }
    };
    // Targets cannot change a part's kind or what it attaches to.
    has(
        issues(&|d| d["params"]["broken"]["set"]["picket.type"] = json!(["box", "box"])),
        &["params.broken.set.picket.type: a family cannot change type — knobs set sizes"],
    );
    has(issues(&|d| d["params"]["broken"]["set"]["rail.attach.to"] = json!(["post", "post"])), &["cannot change attach.to"]);
    has(
        issues(&|d| d["params"]["length"]["set"]["pickt.pos.0"] = json!(1)),
        &["params.length.set.pickt.pos.0: no part \"pickt\" — did you mean \"picket\"?"],
    );
    // A target must agree with the default the others imply, and a range target be written.
    has(
        issues(&|d| d["params"]["length"]["set"]["picket.array.span.0"] = json!(0.5)),
        &[
            "params.length.set.picket.array.span.0: 168 implies length 336, but rail.size.0 implies 200 — write 100 there, or use factor 0.84",
        ],
    );
    has(issues(&|d| d["params"]["height"]["set"]["post.yaw"] = json!(1)), &["post.yaw is not written — write it in the file"]);
    has(
        issues(&|d| d["params"]["broken"]["set"]["post.size.2"] = json!([5, 8])),
        &["the file says 10, the first choice (the default) 5"],
    );
    // A range end that breaks the asset is caught when the family is read.
    has(
        issues(&|d| d["params"]["pickets"]["range"] = json!([3, 100])),
        &["params.pickets: pickets 100 breaks the asset: parts.picket.array.count: 100 is outside 1 to 64"],
    );
    // Limits: 8 params, 8 targets each, 8 choices, 16 presets.
    has(
        issues(&|d| {
            for i in 0..5 {
                d["params"][format!("k{i}")] = json!({"choices": ["a", "b"], "set": {format!("post.pos.{}", i % 3): [0, 0]}});
            }
        }),
        &["params: 9 params (expected at most 8)"],
    );
    has(
        issues(&|d| {
            for i in 0..9 {
                d["params"]["broken"]["set"][format!("materials.wood.k{i}")] = json!([1, 1]);
            }
        }),
        &["params.broken.set: 11 targets (expected 1 to 8)"],
    );
    has(
        issues(&|d| d["params"]["broken"]["choices"] = json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"])),
        &["params.broken.choices: expected 2-8 names, got 9"],
    );
    has(
        issues(&|d| {
            for i in 0..14 {
                d["presets"][format!("p{i}")] = json!({"pickets": 5});
            }
        }),
        &["presets: 17 presets (expected at most 16)"],
    );
    has(issues(&|d| d["presets"]["low"]["heigth"] = json!(70)), &["presets.low.heigth: unknown key", "did you mean \"height\"?"]);
    has(issues(&|d| d["presets"]["low"]["height"] = json!(50)), &["presets.low.height: 50 is outside 60 to 140"]);
}

/// `doc` installed as `WORKSHOP/<name>`.
fn installed(mut doc: Doc, name: &str) -> String {
    doc.name = name.into();
    let asset = format!("WORKSHOP/{name}");
    props::install(&asset, PropDef::new(doc).unwrap()).unwrap();
    asset
}

#[test]
fn scale_unit_and_move_ops_carry_a_familys_knobs() {
    let size = |name: &str| props::resolve_parts(name, 0).unwrap().bounds_at(1.0).size();
    let patched = |doc: &Doc, ops: Value, name: &str| {
        let patched = props::patch_document(doc, &ops, &props::get).unwrap_or_else(|e| panic!("{name}: {}", issue::render(&e)));
        installed(patched.doc, name)
    };
    let close = |a: Vec3, b: Vec3| (a - b).abs().max_element() < 2e-3;
    let fence = Doc::read(FENCE).unwrap();
    let whole = installed(fence.clone(), "family_fence_whole");
    // A document scale (what size_band suggests) scales the ranges and the presets, so every
    // variant scales; a unit change keeps every variant's size.
    let half = patched(&fence, json!([{"op": "scale", "by": 0.5}]), "family_fence_half");
    let def = props::get(&half).unwrap();
    let props::doc::Param::Range { range, .. } = &def.doc.params["length"] else { panic!("{:?}", def.doc.params) };
    assert_eq!(*range, [50.0, 150.0]);
    assert_eq!(def.doc.presets["long"]["length"], json!(150.0));
    for preset in ["", ":low", ":long"] {
        assert!(close(size(&format!("{half}{preset}")), size(&format!("{whole}{preset}")) * 0.5), "{preset}");
    }
    let metres = patched(&fence, json!([{"op": "unit", "to": "m"}]), "family_fence_metres");
    assert!(close(size(&format!("{metres}:long")), size(&format!("{whole}:long"))));
    // A choice whose values are lengths goes through the op too: the chair's high back.
    let chair = props::get("LIB/core/chair").unwrap();
    let small = patched(&chair.doc, json!([{"op": "scale", "by": 0.5}]), "family_chair_half");
    assert!(close(size(&format!("{small}:high_back")), size("LIB/core/chair:high_back") * 0.5));
    // A move keeps the file's new values the defaults: the fence lifted 10 cm still places.
    let ops = json!([{"op": "move", "parts": ["post", "rail", "picket"], "by": [0, 10, 0]}]);
    let lifted = patched(&fence, ops, "family_fence_lifted");
    assert!((size(&lifted).y - 1.1).abs() < 1e-3 && props::resolve_parts(&format!("{lifted}:low"), 0).is_ok());
}
