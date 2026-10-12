//! Format-2 patch operations (asset plan §5.7): each operation on the conformance fixtures,
//! the errors that name the operation, and the operations that must not move anything
//! (`expand`, `unit`) checked against the compiled parts.

use std::collections::BTreeMap;

use glam::Vec3;
use pav_core::issue::render;
use pav_core::props::compile::compile;
use pav_core::props::doc::{Kind, PrimType, Replicate, Unit};
use pav_core::props::{CompiledProp, Doc, ResolvedPart, get, patch_document};
use serde_json::{Value, json};

const BENCH: &str = include_str!("fixtures/props/format2/bench.json");
const GREATSWORD: &str = include_str!("fixtures/props/format2/greatsword.json");
const MARKET_STALL: &str = include_str!("fixtures/props/format2/market_stall.json");
const WALL_TORCH: &str = include_str!("fixtures/props/format2/wall_torch.json");
const WELL: &str = include_str!("fixtures/props/format2/well.json");
const PROBE_ATTACH: &str = include_str!("fixtures/props/format2/probe_attach.json");
const PROBE_FRAMES: &str = include_str!("fixtures/props/format2/probe_frames.json");
const PROBE_MIRROR: &str = include_str!("fixtures/props/format2/probe_mirror.json");

const FIXTURES: &[&str] = &[BENCH, GREATSWORD, MARKET_STALL, WALL_TORCH, WELL, PROBE_ATTACH, PROBE_FRAMES, PROBE_MIRROR];

fn doc(text: &str) -> Doc {
    Doc::read(text).unwrap_or_else(|issues| panic!("{}", render(&issues)))
}

fn patch(doc: &Doc, ops: Value) -> Result<Doc, String> {
    patch_document(doc, &ops, &get).map(|patched| patched.doc).map_err(|issues| render(&issues))
}

fn parts(doc: &Doc) -> CompiledProp {
    compile(doc, &get, 0).unwrap_or_else(|issues| panic!("{}", render(&issues)))
}

fn node<'a>(doc: &'a Doc, id: &str) -> &'a pav_core::props::doc::Node {
    let mut names = id.split('.');
    let mut node = &doc.parts[names.next().unwrap()];
    for name in names {
        node = &node.children().unwrap()[name];
    }
    node
}

/// Two compilations draw the same parts: `rename` maps an id of `a` to its id in `b`. Each
/// part's hull (its shape, size, place and turn) agrees within 0.01 cm.
fn same_parts(a: &CompiledProp, b: &CompiledProp, rename: impl Fn(&str) -> String) {
    assert_eq!(a.parts.len(), b.parts.len());
    let by_id: BTreeMap<&str, &ResolvedPart> = b.parts.iter().map(|part| (part.id.as_str(), part)).collect();
    for part in &a.parts {
        let id = rename(&part.id);
        let other = by_id.get(id.as_str()).unwrap_or_else(|| panic!("{} became {id}, which is missing", part.id));
        assert_eq!(
            (part.mirrored, &part.color, part.solid, part.hidden),
            (other.mirrored, &other.color, other.solid, other.hidden),
            "{id}"
        );
        for (p, q) in part.hull(1.0).into_iter().zip(other.hull(1.0)) {
            assert!(p.distance(q) < 1e-4, "{} → {id}: {p} against {q}", part.id);
        }
    }
}

#[test]
fn set_merges_dotted_fields_deletes_nulls_and_replaces_shapes() {
    let well = doc(WELL);
    let changed = patch(
        &well,
        json!([
            {"op":"set","part":"stones.ring","fields":{"radial.count":12,"jitter":null}},
            {"op":"set","part":"pad","fields":{"type":"box","size":[190,6,190]}},
            {"op":"set","part":"ridge","fields":{"half_height":95}},
            {"op":"set","part":"rim","fields":{"attach.at":"top"}}
        ]),
    )
    .unwrap();
    let ring = node(&changed, "stones.ring");
    assert!(matches!(&ring.replicate, Some(Replicate::Radial(radial)) if radial.count == 12 && radial.radius == 69.0));
    assert_eq!(ring.jitter, None);
    let pad = node(&changed, "pad").prim().unwrap();
    assert_eq!((pad.kind, pad.size.clone(), pad.radius, pad.height), (PrimType::Box, Some(vec![190.0, 6.0, 190.0]), None, None));
    let ridge = node(&changed, "ridge").prim().unwrap();
    assert_eq!((ridge.height, ridge.half_height), (None, Some(95.0)), "one spelling of a size replaces the other");
    let rim = node(&changed, "rim").attach.as_ref().unwrap();
    assert_eq!((rim.to.as_str(), rim.at.is_some()), ("wall", true), "attach:\"wall\" opens up to {{to,at}}");
    // Format 1's nested shape is spread out flat.
    let changed = patch(&well, json!([{"op":"set","part":"water","fields":{"shape":{"type":"sphere","radius":20}}}])).unwrap();
    let water = node(&changed, "water").prim().unwrap();
    assert_eq!((water.kind, water.radius, water.height), (PrimType::Sphere, Some(20.0), None));
}

#[test]
fn targets_reach_materials_sockets_and_the_document() {
    let bench = doc(BENCH);
    let changed = patch(
        &bench,
        json!([
            {"op":"set","material":"wood","fields":{"color":"#c08a58","roughness":0.7}},
            {"op":"add","material":"iron","value":{"color":"#4a4a4a"}},
            {"op":"set","part":"crossbar","fields":{"mat":"iron"}},
            {"op":"set","socket":"sit","fields":{"pos":[0,90,4]}},
            {"op":"add","socket":"top","value":{"pos":[0,141,-48]}},
            {"op":"set","fields":{"description":"A bench with an iron crossbar.","tags":["bench"]}}
        ]),
    )
    .unwrap();
    let pav_core::props::doc::MaterialEntry::Inline(wood) = &changed.materials["wood"] else { panic!() };
    assert_eq!((wood.color.as_deref(), wood.roughness), (Some("#c08a58"), Some(0.7)));
    assert_eq!(node(&changed, "crossbar").surface.mat.as_deref(), Some("iron"));
    assert_eq!(changed.sockets["sit"].pos, Some(Vec3::new(0.0, 90.0, 4.0)));
    assert!(changed.sockets.contains_key("top"));
    assert_eq!(
        (changed.description.as_str(), changed.tags.clone()),
        ("A bench with an iron crossbar.", vec!["bench".to_string()])
    );
    for (key, fix) in [("unit", "use op:unit"), ("name", "copy the asset"), ("parts", "address parts with part")] {
        let error = patch(&bench, json!([{"op":"set","fields":{key:"x"}}])).unwrap_err();
        assert!(error.starts_with(&format!("ops[0].fields.{key}: not set this way — {fix}")), "{error}");
    }
}

#[test]
fn add_remove_rename_and_copy_keep_references_whole() {
    let attach = doc(PROBE_ATTACH);
    // Renaming a part moves what attaches to it; removing parts and adding into a group.
    let changed = patch(
        &attach,
        json!([
            {"op":"rename","part":"beam","to":"rail"},
            {"op":"remove","parts":["ruler","cap_height"]},
            {"op":"copy","part":"knob","to":"knob_big","fields":{"radius":9,"attach.at":"bottom"}}
        ]),
    )
    .unwrap();
    assert_eq!(node(&changed, "cube").attach.as_ref().unwrap().to, "rail");
    assert!(!changed.parts.contains_key("ruler") && !changed.parts.contains_key("cap_height"));
    let big = node(&changed, "knob_big");
    assert_eq!((big.prim().unwrap().radius, big.attach.as_ref().unwrap().to.as_str()), (Some(9.0), "grip"));
    // The cube still sits on the renamed beam, where it sat before.
    let (before, after) = (parts(&attach), parts(&changed));
    let cube = |parts: &CompiledProp| parts.parts.iter().find(|part| part.id == "cube").unwrap().pos;
    assert!(cube(&before).distance(cube(&after)) < 1e-6);

    // Into a group, and a material renamed everywhere it is named.
    let well = doc(WELL);
    let changed = patch(
        &well,
        json!([
            {"op":"add","part":"roof.ridge_cap","value":{"type":"box","size":[184,4,8],"pos":[0,52,0],"mat":"roof"}},
            {"op":"rename","material":"stone","to":"granite"}
        ]),
    )
    .unwrap();
    assert!(node(&changed, "roof").children().unwrap().contains_key("ridge_cap"));
    assert!(changed.materials.contains_key("granite") && !changed.materials.contains_key("stone"));
    assert_eq!(node(&changed, "stones.ring").surface.mat.as_deref(), Some("granite"));
    assert_eq!(node(&changed, "stones_mid").surface.mat.as_deref(), Some("granite"));
    // Cycle entries follow a material's new name too.
    let changed = patch(&attach, json!([{"op":"rename","material":"grey","to":"slate"}])).unwrap();
    let Some(Replicate::Radial(radial)) = &node(&changed, "spokes_y").replicate else { panic!() };
    assert_eq!(radial.cycle.as_deref().unwrap(), ["red", "blue", "slate", "slate"]);
}

#[test]
fn structure_ops_mirror_array_move_and_scale() {
    let attach = doc(PROBE_ATTACH);
    let changed = patch(
        &attach,
        json!([
            {"op":"mirror","part":"ruler","axis":"xz"},
            {"op":"array","part":"spokes_y","count":3,"step":[10,0,0],"cycle":["red","blue","grey"]},
            {"op":"move","parts":["grip","cap_height"],"by":[0,10,0]},
            {"op":"move","part":"cube","by":[5,0,0]},
            {"op":"scale","part":"cap_fromto","by":2}
        ]),
    )
    .unwrap();
    assert_eq!(node(&changed, "ruler").mirror.len(), 2);
    let spokes = node(&changed, "spokes_y");
    assert!(
        matches!(&spokes.replicate, Some(Replicate::Array(array)) if array.count == 3 && array.step == Some(Vec3::new(10.0, 0.0, 0.0)))
    );
    let grip = node(&changed, "grip");
    assert_eq!((grip.from, grip.to), (Some(Vec3::new(0.0, 110.0, 0.0)), Some(Vec3::new(0.0, 50.0, 0.0))));
    assert_eq!(node(&changed, "cap_height").pos, Some(Vec3::new(40.0, 25.0, 0.0)));
    assert_eq!(node(&changed, "cube").offset, Some(Vec3::new(5.0, 0.0, 0.0)), "an attached part moves by its offset");
    // Scaled about its own centre: twice the radius and length, the same middle.
    let capsule = node(&changed, "cap_fromto");
    assert_eq!(capsule.prim().unwrap().radius, Some(10.0));
    assert_eq!((capsule.from, capsule.to), (Some(Vec3::new(60.0, -15.0, 0.0)), Some(Vec3::new(60.0, 45.0, 0.0))));
    let removed = patch(&doc(PROBE_MIRROR), json!([{"op":"mirror","part":"tilted","axis":null}])).unwrap();
    assert!(node(&removed, "tilted").mirror.is_empty());
}

/// Every replicated node of every fixture, expanded, draws what it drew (ids aside).
#[test]
fn expand_keeps_every_copy_where_it_was() {
    let mut expanded = 0;
    for text in FIXTURES {
        let original = doc(text);
        let before = parts(&original);
        fn replicated(nodes: &BTreeMap<String, pav_core::props::doc::Node>, prefix: &str, out: &mut Vec<String>) {
            for (name, node) in nodes {
                let jittered = node.jitter.as_ref().is_some_and(|j| j.pos.is_some() || j.rot.is_some() || j.scale.is_some());
                if node.replicate.is_some() && !jittered && node.chance.is_none() {
                    out.push(format!("{prefix}{name}"));
                }
                if let Kind::Group(children) = &node.kind {
                    replicated(children, &format!("{prefix}{name}."), out);
                }
            }
        }
        let mut ids = Vec::new();
        replicated(&original.parts, "", &mut ids);
        for id in ids {
            let changed =
                patch(&original, json!([{"op":"expand","part":id}])).unwrap_or_else(|e| panic!("{}: {e}", original.name));
            assert!(node(&changed, &format!("{id}_0")).replicate.is_none());
            // `stones[1].ring[3]` is `stones[1].ring_3` once stones.ring is expanded.
            let segments: Vec<&str> = id.split('.').collect();
            let rename = |resolved: &str| {
                let mut parts: Vec<String> = resolved.split('.').map(String::from).collect();
                if parts.len() >= segments.len() {
                    let last = &parts[segments.len() - 1];
                    if let Some(rest) = last.strip_prefix(&format!("{}[", segments[segments.len() - 1])) {
                        let (index, suffix) = rest.split_once(']').unwrap();
                        parts[segments.len() - 1] = format!("{}_{index}{suffix}", segments[segments.len() - 1]);
                    }
                }
                parts.join(".")
            };
            same_parts(&before, &parts(&changed), rename);
            expanded += 1;
        }
    }
    assert_eq!(expanded, 11, "array, grid and radial nodes across the fixtures");
}

/// Radial copies of parts placed by their ends, turned by yaw/pitch/roll, attached, or cut to
/// an arc keep their place, turn and facing once expanded.
#[test]
fn expand_turns_ends_angles_arcs_and_attached_copies() {
    let doc = doc(r##"{"format":2,"name":"turns","unit":"cm","color":"#8a8f99","parts":{
"base":{"type":"box","size":[40,4,40],"pos":[0,2,0]},
"slats":{"type":"box","size":[10,4],"from":[0,10,0],"to":[25,40,5],"spin":15,"radial":{"count":5,"radius":30}},
"fins":{"type":"wedge","size":[8,6,3],"pos":[0,60,0],"yaw":10,"pitch":20,"roll":30,"radial":{"count":3,"radius":12,"axis":"x","start":5}},
"tabs":{"type":"box","size":[4,4,4],"attach":"base","radial":{"count":4,"radius":15,"arc":90}},
"shells":{"type":"cylinder","radius":8,"height":10,"arc":120,"facing":"x","pos":[0,80,0],"radial":{"count":4,"radius":20}}}}"##);
    let before = parts(&doc);
    for (id, copies) in [("slats", 5), ("fins", 3), ("tabs", 4), ("shells", 4)] {
        let expanded = patch(&doc, json!([{"op":"expand","part":id}])).unwrap_or_else(|e| panic!("{id}: {e}"));
        let rename = |resolved: &str| match resolved.strip_prefix(&format!("{id}[")) {
            Some(rest) => format!("{id}_{}", rest.trim_end_matches(']')),
            None => resolved.to_string(),
        };
        same_parts(&before, &parts(&expanded), rename);
        assert!((0..copies).all(|i| expanded.parts.contains_key(&format!("{id}_{i}"))));
    }
    // The ends stay ends; an arc turned between the axes cannot name its facing.
    let slats = patch(&doc, json!([{"op":"expand","part":"slats"}])).unwrap();
    assert!(node(&slats, "slats_3").from.is_some() && node(&slats, "slats_3").pos.is_none());
    let error = patch(&doc, json!([{"op":"set","part":"shells","fields":{"radial.count":6}},{"op":"expand","part":"shells"}]))
        .unwrap_err();
    assert_eq!(error, "ops[1] shells: shells[1]: its arc would face between the axes; only quarter turns keep an arc's facing");
}

#[test]
fn expand_follows_attachments_and_refuses_what_it_cannot_keep() {
    let attach = doc(PROBE_ATTACH);
    let changed = patch(
        &attach,
        json!([
            {"op":"set","part":"knob","fields":{"attach":"spokes_y[2]"}},
            {"op":"expand","part":"spokes_y"}
        ]),
    )
    .unwrap();
    assert_eq!(node(&changed, "knob").attach.as_ref().unwrap().to, "spokes_y_2");
    assert_eq!(node(&changed, "spokes_y_2").surface.mat.as_deref(), Some("grey"), "the cycle's entry for that copy");
    let error = patch(
        &attach,
        json!([
            {"op":"set","part":"knob","fields":{"attach":"spokes_y"}},
            {"op":"expand","part":"spokes_y"}
        ]),
    )
    .unwrap_err();
    assert_eq!(error, "ops[1] spokes_y: knob attaches to spokes_y as a whole — attach it to one copy (spokes_y[0]) first");
    let error = patch(&attach, json!([{"op":"expand","part":"grip"}])).unwrap_err();
    assert_eq!(error, "ops[0] grip: it has no array, grid or radial to expand");
    let error = patch(&attach, json!([{"op":"add","part":"spokes_y_1","value":{"type":"box","size":[1,1,1],"mat":"red"}},{"op":"expand","part":"spokes_y"}]))
        .unwrap_err();
    assert_eq!(error, "ops[1] spokes_y: a part \"spokes_y_1\" is already beside it; rename it first");
}

/// Converting the unit, or scaling the whole asset about the origin, keeps or scales what is
/// drawn exactly.
#[test]
fn unit_and_whole_scale_keep_the_shape() {
    for text in FIXTURES {
        let original = doc(text);
        let before = parts(&original);
        let metres = patch(&original, json!([{"op":"unit","to":"m"}])).unwrap();
        assert_eq!(metres.unit, Unit::M);
        same_parts(&before, &parts(&metres), str::to_string);
        let back = patch(&metres, json!([{"op":"unit","to":"cm"}])).unwrap();
        same_parts(&before, &parts(&back), str::to_string);

        let doubled = parts(&patch(&original, json!([{"op":"scale","by":2}])).unwrap());
        let (a, b) = (before.bounds, doubled.bounds);
        assert!((a.min * 2.0).distance(b.min) < 1e-4 && (a.max * 2.0).distance(b.max) < 1e-4, "{}", original.name);
    }
}

#[test]
fn errors_name_the_operation_and_suggest_a_fix() {
    let well = doc(WELL);
    let error = |ops: Value| patch(&well, ops).unwrap_err();
    assert_eq!(
        error(json!([{"op":"set","part":"stones.ring","fields":{"radial.count":0}}])),
        "ops[0] stones.ring.radial.count: 0 is outside 1 to 64"
    );
    assert_eq!(
        error(json!([{"op":"set","part":"stones.rings","fields":{}}])),
        "ops[0] stones.rings: no part \"stones.rings\" — did you mean \"ring\"?"
    );
    assert_eq!(
        error(json!([{"op":"set","part":"stones[1].ring","fields":{}}])),
        "ops[0] stones[1].ring: \"stones[1]\" names a copy of stones — edit stones, whose copies follow it, or expand it first"
    );
    assert_eq!(
        error(json!([{"op":"update","part":"pad","fields":{}}])),
        "ops[0].op: unknown op \"update\" (expected set, add, remove, rename, copy, mirror, array, expand, move, scale, unit) — use op:set"
    );
    assert_eq!(
        error(json!([{"op":"move","part":"pad","to":[0,1,0]}])),
        "ops[0].to: unknown key for op move (expected part, parts, by) — move takes by:[x,y,z]"
    );
    assert_eq!(
        error(json!([{"op":"set","part":"pad","material":"pad","fields":{}}])),
        "ops[0].material: one target per operation, not part and material"
    );
    assert_eq!(error(json!([{"op":"mirror","axis":"x"}])), "ops[0]: mirror needs a target (expected part or parts)");
    assert_eq!(error(json!([{"op":"unit","to":"mm"}])), "ops[0].to: unknown unit \"mm\" (expected cm, m)");
    assert_eq!(error(json!([{"op":"rename","part":"pad","to":"Pad"}])).split(':').next().unwrap(), "ops[0].to");
    assert!(error(json!({"op":"set"})).starts_with("ops: expected an array of operations"));
    let many: Vec<Value> = (0..1025).map(|_| json!({"op":"set","fields":{"tags":["a"]}})).collect();
    assert_eq!(error(Value::Array(many)), "ops: 1025 operations (expected at most 1024)");
    // Every failing operation is reported, and nothing is applied.
    let errors = error(json!([
        {"op":"set","part":"pad","fields":{"radius":-1}},
        {"op":"remove","material":"marble"}
    ]));
    assert!(errors.contains("ops[1] materials.marble: no material \"marble\""), "{errors}");
}

/// A format-1 document takes the new operations through its format-2 view, and saves in
/// format 1 while it still fits.
#[test]
fn format1_documents_take_format2_ops() {
    let bench = get("BUILTIN/bench").unwrap();
    let changed =
        patch(&bench.doc, json!([{"op":"rename","part":"seat","to":"top"},{"op":"move","part":"top","by":[0,0.02,0]}])).unwrap();
    assert!(changed.parts.contains_key("top") && !changed.parts.contains_key("seat"));
    assert!(changed.text().starts_with("{\"format\":1,"), "{}", changed.text());
    let mirrored = patch(&bench.doc, json!([{"op":"mirror","part":"top","axis":"x"}])).unwrap_err();
    assert!(mirrored.starts_with("ops[0] top: no part \"top\""), "{mirrored}");
}
