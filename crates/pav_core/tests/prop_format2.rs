//! Format 2's conformance corpus (asset plan §5.9): the five examples and three probes, inline
//! and in canonical form. Map entries are in key order, so the examples' authored order (the
//! bench's seat first) reads to this text too. The tests check reading, writing, revisions and
//! the resolved goldens: positions in cm within 0.01 cm, angles within 0.05 degrees.

use glam::{EulerRot, Vec3};
use pav_core::issue::render;
use pav_core::props::compile::compile;
use pav_core::props::{CompiledProp, Doc, PropAsset, ResolvedPart, get};

const BENCH: &str = r##"{"format":2,"name":"bench","unit":"cm",
"description":"A wooden bench: broad seat, four legs, backrest leaning 12 degrees back on the rear legs.",
"tags":["furniture","seat","wood","town"],
"materials":{
"wood":{"color":"#bc8150"},
"wood_dark":{"color":"#594635"}},
"parts":{
"backrest":{"type":"rounded_box","size":[220,8],"radius":2.5,"from":[0,92,-38],"to":[0,141,-48],"mat":"wood"},
"crossbar":{"type":"box","size":[6,6],"from":[-80,31,-33],"to":[80,31,-33],"mat":"wood_dark"},
"leg_back":{"type":"box","size":[10,8],"from":[85,0,-26],"to":[85,141,-56],"mat":"wood_dark","mirror":"x"},
"leg_front":{"type":"box","size":[10,8],"from":[85,0,25],"to":[85,71,25],"mat":"wood_dark","mirror":"x"},
"seat":{"type":"rounded_box","size":[220,18,76],"radius":2.5,"pos":[0,80,0],"mat":"wood"}},
"sockets":{
"sit":{"pos":[0,89,4]}}}
"##;

const GREATSWORD: &str = r##"{"format":2,"name":"greatsword","unit":"cm",
"description":"A two-handed steel sword: long tapered blade with a dark fuller, upswept brass crossguard, leather grip with brass rings, oval pommel. Origin: crossguard centre; the blade points +y.",
"tags":["weapon","sword","two_handed","steel"],
"materials":{
"brass":{"color":"#b08a3e"},
"leather":{"color":"#4a2f1e"},
"steel":{"color":"#c9d0d6","look":"lit"},
"steel_dark":{"color":"#6e767d"}},
"parts":{
"blade":{"type":"extrude","points":[[-3,0],[3,0],[2.4,100],[0,113],[-2.4,100]],"depth":0.8,"pos":[0,1.5,0],"mat":"steel"},
"fuller":{"type":"box","size":[0.9,72,1],"pos":[0,40,0],"mat":"steel_dark"},
"grip":{"type":"cylinder","radius":1.6,"from":[0,-1.5,0],"to":[0,-38,0],"mat":"leather"},
"guard":{"type":"capsule","radius":1.5,"from":[0,0,0],"to":[15,2.5,0],"mat":"brass","mirror":"x"},
"pommel":{"type":"ellipsoid","size":[6.5,5,4.5],"attach":{"to":"grip","at":"to"},"mat":"brass"},
"rings":{"type":"tube","radius":1.8,"thickness":0.3,"height":1,"pos":[0,-19,0],"mat":"brass","array":{"count":3,"step":[0,11,0]}}},
"sockets":{
"grip":{"pos":[0,-6,0]},
"grip2":{"pos":[0,-28,0]},
"tip":{"pos":[0,114.5,0]}}}
"##;

const MARKET_STALL: &str = r##"{"format":2,"name":"market_stall","unit":"cm",
"description":"A wooden market stall: red and cream striped awning with a scalloped valance, plank counter, two tilted crates of apples and oranges.",
"tags":["stall","market","town","wood","cloth"],
"materials":{
"apple":{"color":"#c8302a"},
"cloth_cream":{"color":"#f0e4c8"},
"cloth_red":{"color":"#c63d32"},
"crate":{"color":"#a87545"},
"crate_band":{"color":"#7f5432"},
"orange":{"color":"#e88a1a"},
"planks":{"color":"#8a5a35","layers":[{"type":"planks","size":[240,24],"gap":1.5,"color":"#5a3a22"}]},
"post":{"color":"#6b4a2e"},
"trim":{"color":"#5a3a22"},
"wood":{"color":"#b07a4a"}},
"parts":{
"awning":{"solid":false,"array":{"count":6,"step":[43.3,0,0],"cycle":["cloth_red","cloth_cream"]},"parts":{
  "band":{"type":"box","size":[43.3,21.6,1.6],"pos":[0,204.2,96.2]},
  "scallop":{"type":"cylinder","radius":21.6,"height":1.6,"arc":180,"facing":"-y","pos":[0,193.4,95],"axis":"z"},
  "stripe":{"type":"box","size":[43.3,3],"from":[0,250,-75],"to":[0,215,95]}}},
"counter_front":{"type":"box","size":[240,96,5],"pos":[0,48,71.5],"mat":"planks"},
"counter_side":{"type":"box","size":[5,96,60],"pos":[118,48,42],"mat":"planks","mirror":"x"},
"counter_top":{"type":"rounded_box","size":[250,8,64],"radius":1.5,"pos":[0,100,42],"mat":"wood"},
"crate":{"pos":[0,119.8,42],"pitch":15,"array":{"count":2,"step":[120,0,0],"cycle":["apple","orange"]},"parts":{
  "body":{"type":"box","size":[60,22,40],"mat":"crate"},
  "fruit":{"type":"sphere","radius":8.5,"pos":[0,12,0],"solid":false,"grid":{"count":[3,2],"step":[18,18]},"jitter":{"color":0.08}},
  "hoops":{"type":"box","size":[62,4,42],"pos":[0,1,0],"mat":"crate_band","array":{"count":2,"step":[0,16,0]}},
  "riser":{"type":"box","size":[56,10,10],"pos":[0,-14.8,-13.6],"mat":"trim"}}},
"post_back":{"type":"box","size":[10,10],"from":[128,0,-60],"to":[128,247,-60],"mat":"post","mirror":"x"},
"post_front":{"type":"box","size":[10,10],"from":[128,0,74],"to":[128,220,74],"mat":"post","mirror":"x"}},
"sockets":{
"top":{"pos":[0,104,42]}}}
"##;

const WALL_TORCH: &str = r##"{"format":2,"name":"wall_torch","unit":"cm","solid":false,
"description":"An iron wall bracket holding a wooden torch tilted 15 degrees out from the wall, with a flame in the theme's light colour. Origin: wall surface; +z points away from the wall.",
"tags":["light","torch","wall","dungeon","iron"],
"materials":{
"flame":{"color":"$light","look":"unlit","emissive":3},
"iron":{"color":"#3a3532"},
"wood":{"color":"#6b4a2e"}},
"parts":{
"arm":{"type":"box","size":[3,3],"from":[0,-7,2],"to":[0,-3,13],"mat":"iron"},
"plate":{"type":"box","size":[12,20,2],"pos":[0,0,1],"mat":"iron"},
"torch":{"pos":[0,-2,15],"pitch":15,"parts":{
  "collar":{"type":"torus","radius":3.3,"minor_radius":0.8,"mat":"iron"},
  "cup":{"type":"cone","radius":3,"radius_end":5,"from":[0,14,0],"to":[0,21,0],"mat":"iron"},
  "flame":{"type":"capsule","radius":4.5,"radius_end":0.8,"from":[0,17.5,0],"to":[0,34.8,0],"mat":"flame","light":{"color":"$light","radius":600,"intensity":1.6,"flicker":0.5},"fx":{"preset":"fire","size":0.5}},
  "handle":{"type":"cone","radius":2,"radius_end":2.8,"from":[0,-18,0],"to":[0,14,0],"mat":"wood"}}}},
"sockets":{
"wall_mount":{"axis":"-z"}}}
"##;

const WELL: &str = r##"{"format":2,"name":"well","unit":"cm",
"description":"A round stone well with a shingled wooden gable roof, a rope crank and a hanging bucket.",
"tags":["well","town","stone","wood"],
"materials":{
"bucket":{"color":"#8b5a2b"},
"iron":{"color":"#4a4a4a"},
"mortar":{"color":"#5f6266"},
"pad":{"color":"#6f7276"},
"rim":{"color":"#a7a9ab"},
"roof":{"color":"#8a4b2f"},
"rope":{"color":"#c9a66b"},
"shingle":{"color":"#6f3a22"},
"stone":{"color":"$wall"},
"void":{"color":"#1f272d","look":"flat"},
"wood":{"color":"#6b4a2e"},
"wood_dark":{"color":"#5a3a22"}},
"parts":{
"bucket":{"pos":[0,102,0],"solid":false,"parts":{
  "bail":{"type":"torus","radius":13,"minor_radius":0.8,"arc":180,"pos":[0,9,0],"axis":"z","mat":"iron"},
  "band":{"type":"tube","radius":13.6,"thickness":1.2,"height":2.4,"pos":[0,6,0],"mat":"iron"},
  "body":{"type":"cone","radius":11.5,"radius_end":13.5,"height":18,"mat":"bucket"},
  "rope":{"type":"cylinder","radius":1.2,"from":[0,22,0],"to":[0,33,0],"mat":"rope"}}},
"crank":{"pos":[0,145,0],"mat":"wood_dark","parts":{
  "arm":{"type":"box","size":[5,26,5],"pos":[84,-12,0]},
  "axle":{"type":"cylinder","radius":3.5,"height":164,"axis":"x"},
  "drum":{"type":"cylinder","radius":10,"height":60,"axis":"x","mat":"rope"},
  "handle":{"type":"cylinder","radius":2.5,"height":18,"pos":[93,-24,0],"axis":"x"}}},
"pad":{"type":"cylinder","radius":95,"height":6,"pos":[0,3,0],"mat":"pad"},
"post":{"type":"box","size":[12,12],"from":[70,90,0],"to":[70,230,0],"mat":"wood","mirror":"x"},
"ridge":{"type":"cylinder","radius":5,"height":190,"pos":[0,237,0],"axis":"x","mat":"wood_dark"},
"rim":{"type":"tube","radius":78,"thickness":20,"height":10,"attach":"wall","mat":"rim"},
"roof":{"pos":[0,207.3,41],"pitch":35,"mirror":"z","parts":{
  "shingles":{"type":"box","size":[184,2.4,12],"pos":[0,3.7,0],"mat":"shingle","solid":false,"array":{"count":3,"step":[0,0,30]}},
  "slab":{"type":"box","size":[180,5,100],"mat":"roof"}}},
"stones":{"pos":[0,40,0],"array":{"count":2,"step":[0,52,0]},"parts":{
  "ring":{"type":"box","size":[40,23,8],"mat":"stone","radial":{"count":10,"radius":69},"jitter":{"color":0.12}}}},
"stones_mid":{"type":"box","size":[40,23,8],"pos":[0,40,0],"mat":"stone","radial":{"count":10,"radius":69,"start":18},"jitter":{"color":0.12}},
"wall":{"type":"tube","radius":70,"thickness":10,"height":80,"pos":[0,40,0],"mat":"mortar"},
"water":{"type":"cylinder","radius":60,"height":1,"pos":[0,30,0],"mat":"void","solid":false}}}
"##;

const PROBE_ATTACH: &str = r##"{"format":2,"name":"probe_attach","unit":"cm",
"description":"Knob on a grip drawn top-down; cube on a horizontal from/to beam (default at=top); capsule by height vs from/to; radial order around y and z.",
"materials":{
"blue":{"color":"#3050d0"},
"green":{"color":"#30c040"},
"grey":{"color":"#8a8f99"},
"red":{"color":"#d04030"}},
"parts":{
"beam":{"type":"box","size":[6,6],"from":[-40,120,0],"to":[40,120,0],"mat":"grey"},
"cap_fromto":{"type":"capsule","radius":5,"from":[60,0,0],"to":[60,30,0],"mat":"green"},
"cap_height":{"type":"capsule","radius":5,"height":30,"pos":[40,15,0],"mat":"blue"},
"cube":{"type":"box","size":[10,10,10],"attach":"beam","mat":"blue"},
"grip":{"type":"cylinder","radius":3,"from":[0,100,0],"to":[0,40,0],"mat":"grey"},
"knob":{"type":"sphere","radius":6,"attach":"grip","mat":"red"},
"ruler":{"type":"box","size":[2,30,2],"pos":[50,15,0],"mat":"grey"},
"spokes_y":{"type":"box","size":[6,6,6],"pos":[-60,10,0],"radial":{"count":4,"radius":20,"cycle":["red","blue","grey","grey"]}},
"spokes_z":{"type":"box","size":[6,6,6],"pos":[-60,70,0],"radial":{"count":4,"radius":20,"axis":"z","cycle":["red","blue","grey","grey"]}}}}
"##;

const PROBE_FRAMES: &str = r##"{"format":2,"name":"probe_frames","unit":"cm",
"description":"Three 40x4 boards: exactly vertical, leaning 1 degree toward +x, leaning 1 degree toward +z.",
"materials":{
"a":{"color":"#c08040"},
"b":{"color":"#40a0c0"},
"c":{"color":"#a040c0"}},
"parts":{
"board_lean_x":{"type":"box","size":[40,4],"from":[0,0,0],"to":[1.75,100,0],"mat":"b"},
"board_lean_z":{"type":"box","size":[40,4],"from":[70,0,0],"to":[70,100,1.75],"mat":"c"},
"board_vertical":{"type":"box","size":[40,4],"from":[-70,0,0],"to":[-70,100,0],"mat":"a"}}}
"##;

const PROBE_MIRROR: &str = r##"{"format":2,"name":"probe_mirror","unit":"cm",
"description":"Mirrored group with off-axis children (hand, outward flag), mirrored rolled bar, nested surface defaults.",
"materials":{
"blue":{"color":"#3050d0"},
"metal":{"color":"#8a8f99"},
"red":{"color":"#d04030"},
"wood":{"color":"#8a5a35"}},
"parts":{
"arm":{"pos":[10,80,0],"mirror":"x","parts":{
  "beam":{"type":"box","size":[40,4,4],"pos":[20,0,0],"mat":"metal"},
  "flag":{"type":"extrude","points":[[0,0],[20,6],[0,12]],"depth":1,"pos":[40,4,0],"mat":"blue"},
  "hand":{"type":"sphere","radius":5,"pos":[40,0,0],"mat":"red"}}},
"outer":{"pos":[0,0,30],"mat":"wood","parts":{
  "inner":{"color":"#30c040","parts":{
    "cube":{"type":"box","size":[12,12,12],"pos":[0,6,0]}}}}},
"post":{"type":"box","size":[6,100,6],"pos":[0,50,0],"mat":"metal"},
"tilted":{"type":"box","size":[30,4,4],"pos":[25,30,0],"roll":20,"mat":"blue","mirror":"x"}}}
"##;

/// As written in the asset plan, before canonical order and layout.
const BENCH_AUTHORED: &str = r##"{"format":2,"name":"bench","unit":"cm",
"description":"A wooden bench: broad seat, four legs, backrest leaning 12 degrees back on the rear legs.",
"tags":["furniture","seat","wood","town"],
"materials":{
"wood":{"color":"#bc8150"},
"wood_dark":{"color":"#594635"}},
"parts":{
"seat":{"type":"rounded_box","size":[220,18,76],"radius":2.5,"pos":[0,80,0],"mat":"wood"},
"backrest":{"type":"rounded_box","size":[220,8],"radius":2.5,"from":[0,92,-38],"to":[0,141,-48],"mat":"wood"},
"leg_front":{"type":"box","size":[10,8],"from":[85,0,25],"to":[85,71,25],"mat":"wood_dark","mirror":"x"},
"leg_back":{"type":"box","size":[10,8],"from":[85,0,-26],"to":[85,141,-56],"mat":"wood_dark","mirror":"x"},
"crossbar":{"type":"box","size":[6,6],"from":[-80,31,-33],"to":[80,31,-33],"mat":"wood_dark"}},
"sockets":{
"sit":{"pos":[0,89,4]}}}
"##;

/// As written in the asset plan, before canonical order and layout.
const PROBE_ATTACH_AUTHORED: &str = r##"{"format":2,"name":"probe_attach","unit":"cm",
"description":"Knob on a grip drawn top-down; cube on a horizontal from/to beam (default at=top); capsule by height vs from/to; radial order around y and z.",
"materials":{
"grey":{"color":"#8a8f99"},
"red":{"color":"#d04030"},
"blue":{"color":"#3050d0"},
"green":{"color":"#30c040"}
},
"parts":{
"grip":{"type":"cylinder","radius":3,"from":[0,100,0],"to":[0,40,0],"mat":"grey"},
"knob":{"type":"sphere","radius":6,"attach":"grip","mat":"red"},
"ruler":{"type":"box","size":[2,30,2],"pos":[50,15,0],"mat":"grey"},
"cap_height":{"type":"capsule","radius":5,"height":30,"pos":[40,15,0],"mat":"blue"},
"cap_fromto":{"type":"capsule","radius":5,"from":[60,0,0],"to":[60,30,0],"mat":"green"},
"spokes_y":{"type":"box","size":[6,6,6],"pos":[-60,10,0],"radial":{"count":4,"radius":20,"cycle":["red","blue","grey","grey"]}},
"spokes_z":{"type":"box","size":[6,6,6],"pos":[-60,70,0],"radial":{"count":4,"radius":20,"axis":"z","cycle":["red","blue","grey","grey"]}},
"beam":{"type":"box","size":[6,6],"from":[-40,120,0],"to":[40,120,0],"mat":"grey"},
"cube":{"type":"box","size":[10,10,10],"attach":"beam","mat":"blue"}
}}
"##;

/// As written in the asset plan, before canonical order and layout.
const PROBE_FRAMES_AUTHORED: &str = r##"{"format":2,"name":"probe_frames","unit":"cm",
"description":"Three 40x4 boards: exactly vertical, leaning 1 degree toward +x, leaning 1 degree toward +z.",
"materials":{"a":{"color":"#c08040"},"b":{"color":"#40a0c0"},"c":{"color":"#a040c0"}},
"parts":{
"board_vertical":{"type":"box","size":[40,4],"from":[-70,0,0],"to":[-70,100,0],"mat":"a"},
"board_lean_x":{"type":"box","size":[40,4],"from":[0,0,0],"to":[1.75,100,0],"mat":"b"},
"board_lean_z":{"type":"box","size":[40,4],"from":[70,0,0],"to":[70,100,1.75],"mat":"c"}}}
"##;
const FIXTURES: &[(&str, &str, &str)] = &[
    ("bench", BENCH, "prop2-ecd3e7f43a56498c"),
    ("greatsword", GREATSWORD, "prop2-dae0ac4735a82c29"),
    ("market_stall", MARKET_STALL, "prop2-991518049f89137d"),
    ("wall_torch", WALL_TORCH, "prop2-e375629375618585"),
    ("well", WELL, "prop2-07996d0d52b61d3b"),
    ("probe_attach", PROBE_ATTACH, "prop2-e641b38a56732089"),
    ("probe_frames", PROBE_FRAMES, "prop2-fbf4b62f1afb409f"),
    ("probe_mirror", PROBE_MIRROR, "prop2-7434749012a511e7"),
];

fn read(name: &str, text: &str) -> Doc {
    Doc::read(text).unwrap_or_else(|issues| panic!("{name}:\n{}", render(&issues)))
}

#[test]
fn fixtures_rewrite_byte_identically_and_keep_their_revisions() {
    for (name, text, revision) in FIXTURES {
        let doc = read(name, text);
        assert_eq!(doc.name, *name);
        assert_eq!(doc.needed_format(), 2, "{name}");
        assert_eq!(doc.text(), *text, "{name}");
        assert_eq!(doc.revision(), *revision, "{name}");
        let again = read(name, &doc.text());
        assert_eq!(again, doc, "{name} reads back to the same document");
    }
    // Authored nodes, groups included.
    let nodes: Vec<usize> = FIXTURES.iter().map(|(name, text, _)| read(name, text).node_count()).collect();
    assert_eq!(nodes, [5, 6, 14, 7, 22, 9, 3, 9]);
}

#[test]
fn authored_order_and_other_layouts_read_to_the_canonical_text() {
    for (authored, canonical) in
        [(BENCH_AUTHORED, BENCH), (PROBE_ATTACH_AUTHORED, PROBE_ATTACH), (PROBE_FRAMES_AUTHORED, PROBE_FRAMES)]
    {
        assert_ne!(authored, canonical);
        let doc = read("authored", authored);
        assert_eq!(doc.text(), canonical);
        assert_eq!(doc.revision(), read("canonical", canonical).revision(), "layout and order are not content");
    }
    // Whitespace, key order inside a part and an explicit default are content-neutral or kept.
    let loose = r##"{ "unit":"cm", "name":"peg", "format":2,
      "parts": { "peg": { "mat":"wood", "pos":[0, 5.0, 0], "height":10, "radius":1, "type":"cylinder" } },
      "materials": { "wood": { "color":"#8a5a35" } } }"##;
    assert_eq!(
        read("loose", loose).text(),
        "{\"format\":2,\"name\":\"peg\",\"unit\":\"cm\",\n\"materials\":{\n\"wood\":{\"color\":\"#8a5a35\"}},\n\"parts\":{\n\
         \"peg\":{\"type\":\"cylinder\",\"radius\":1,\"height\":10,\"pos\":[0,5,0],\"mat\":\"wood\"}}}\n"
    );
}

#[test]
fn format1_files_keep_their_revisions_through_the_source_model() {
    let golden = [
        ("BUILTIN/box", "prop1-501ba5dc44b112fd"),
        ("BUILTIN/crate", "prop1-464f1ac1ee324075"),
        ("BUILTIN/bench", "prop1-6feb81dece124dfd"),
        ("BUILTIN/lantern", "prop1-fc44b95ee89f262e"),
    ];
    let mut assets: Vec<(PropAsset, &str)> =
        golden.iter().map(|(name, revision)| ((*get(name).unwrap()).clone(), *revision)).collect();
    for (text, revision) in [
        (include_str!("fixtures/props/dogfood_stall_v1.json"), "prop1-bd43f9ba44a259b0"),
        (include_str!("fixtures/props/dogfood_well_v1.json"), "prop1-368eeb091c94f947"),
    ] {
        assets.push((PropAsset::parse(text).unwrap(), revision));
    }
    for (asset, revision) in &assets {
        let doc = Doc::from_v1(asset);
        assert_eq!(doc.needed_format(), 1, "{}", asset.name);
        assert_eq!(doc.revision(), *revision, "{}", asset.name);
        assert_eq!(doc.text(), asset.text(), "{}", asset.name);
        assert_eq!(doc.to_v1().as_ref(), Some(asset));
        assert_eq!(read(&asset.name, &asset.text()), doc, "format 1 text reads into the same document");
    }
}

#[test]
fn format2_that_format1_holds_is_written_as_format1() {
    let text = r##"{"format":2,"name":"marker","unit":"m","parts":{
"post":{"type":"cylinder","radius":0.05,"half_height":0.6,"pos":[0,0.6,0],"color":"#26313e"},
"light":{"type":"sphere","radius":0.16,"pos":[0,1.3,0],"color":"#79debd","emissive":2,"solid":false}}}"##;
    let doc = read("marker", text);
    assert_eq!(doc.needs_format2(), None);
    assert!(doc.text().starts_with("{\"format\":1,\"name\":\"marker\",\n\"parts\":{"), "{}", doc.text());
    assert!(doc.revision().starts_with("prop1-"));
    let asset = PropAsset::parse(text).unwrap();
    assert_eq!((asset.revision(), asset.text()), (doc.revision(), doc.text()));
}

#[test]
fn format2_content_is_checked_but_not_built_yet() {
    assert_eq!(
        PropAsset::parse(BENCH).unwrap_err(),
        r#"unit: unit "cm" needs format 2, which this engine checks but cannot build yet — write format 1 (asset_edit action=schema)"#
    );
    let pair = r##"{"format":2,"name":"pair","unit":"m","parts":{
"leg":{"type":"box","half":[0.05,0.4,0.05],"pos":[0.5,0.4,0],"color":"#594635","mirror":"x"}}}"##;
    assert_eq!(
        PropAsset::parse(pair).unwrap_err(),
        "parts.leg.mirror: mirror needs format 2, which this engine checks but cannot build yet — write format 1 (asset_edit action=schema)"
    );
    // A broken format-2 document reports its problems, not the refusal.
    let broken = BENCH.replace("\"unit\":\"cm\",", "");
    assert_eq!(
        PropAsset::parse(&broken).unwrap_err(),
        r#"unit: missing (expected cm or m) — add "unit":"cm" (the house unit) or "unit":"m""#
    );
}

/// Every format-2 field at least once, in canonical form.
const SINK: &str = r##"{"format":2,"name":"sink","unit":"cm","color":"#8a8f99","look":"lit","emissive":0,"solid":true,
"description":"Every format-2 field once: a test document.",
"tags":["test","sink"],
"category":"town",
"source":{"library":"own","url":"https://example.invalid/sink","license":"CC0-1.0","author":"shardfall","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","title":"Sink"},
"fit":{"mean_mm":1.5,"worst_mm":4,"iou":0.93,"de":2.1,"mesh_pct":0,"parts_hash":"abc123"},
"materials":{
"stone":{"color":"$wall","look":"flat","emissive":0.5,"roughness":0.8,"metallic":0,"map":"world","scale":2,"turn":true,"layers":[{"type":"cobbles","size":30,"jitter":0.5,"gap":3,"bevel":2,"vary":0.2,"cells":["#808080","#707070"],"cavity":0.3,"axis":"x","angle":15,"color":"#606060","strength":0.8,"glow":0,"facing":"up","masked_by":"prev","seed":4294967295},{"type":"noise","size":80,"detail":2,"distort":0.5,"cover":0.4,"soft":0.3,"shade":0.7}]},
"wood":{"color":"#8a5a35"}},
"parts":{
"arch":{"type":"torus","radius":40,"minor_radius":5,"arc":180,"facing":"y","axis":"z","mat":"stone"},
"bead":{"type":"sphere","radius":3,"attach":{"to":"post","at":"top_front","self":[0,-1,0]},"offset":[0,1,0],"color":"#ffcc00","emissive":2,"light":{"color":"$light","radius":300,"intensity":2,"flicker":0.3,"pulse":0.5,"shadows":true},"fx":{"preset":"embers","rate":4,"color":"#ff8800","size":0.5,"speed":1.2,"life":0.8,"area":[2,2,2]},"chance":0.5},
"bowl":{"type":"lathe","profile":[[0,0],[10,2],[12,8]],"segments":16,"pos":[50,0,0],"mat":"wood","sway":"leaves"},
"cap":{"type":"cone","radius":6,"radius_end":2,"height":5,"segments":8,"pos":[0,120,0],"mat":"wood"},
"crate":{"ref":"BUILTIN/crate","pos":[80,0,0],"yaw":45,"array":{"count":2,"span":[0,0,60]},"scale":0.5,"remap":{}},
"egg":{"type":"ellipsoid","size":[6,8,6],"pos":[-50,4,0],"mat":"wood","jitter":{"pos":[1,0,1],"rot":[10,0,0],"scale":[0.9,1.1],"color":0.1}},
"flag":{"type":"extrude","points":[[0,0],[20,6],[0,12]],"depth":1,"pos":[0,100,0],"axis":"x","yaw":10,"pitch":5,"roll":2,"mat":"wood"},
"hoop":{"type":"tube","radius":8,"thickness":1,"height":3,"arc":270,"facing":"z","pos":[0,60,0],"mat":"stone"},
"leg":{"type":"capsule","radius":2,"radius_end":1.5,"from":[30,0,30],"to":[30,40,25],"spin":15,"mat":"wood","mirror":["x","z"]},
"pane":{"type":"rounded_box","half":[10,10,1],"radius":0.5,"pos":[0,30,-40],"look":"unlit","solid":false,"hidden":false},
"post":{"type":"cylinder","radius":3,"half_height":50,"segments":12,"pos":[0,50,0],"mat":"wood","hidden":true},
"ring":{"type":"box","size":[2,2,2],"mat":"stone","radial":{"count":6,"radius":20,"start":15,"arc":180,"axis":"-z","cycle":["stone","wood"]}},
"row":{"pos":[0,0,80],"mat":"wood","grid":{"count":[2,2],"step":[10,10],"plane":"xy","cycle":["wood","#ff0000","$accent"]},"pick":2,"parts":{
  "a":{"type":"box","size":[4,4,4]},
  "b":{"type":"box","size":[4,4,4],"pos":[0,5,0]},
  "c":{"type":"wedge","size":[4,4,4],"peak":-0.5,"pos":[0,10,0]}}},
"slab":{"type":"box","size":[40,4],"from":[-20,0,-60],"to":[20,0,-60],"mat":"stone"}},
"sockets":{
"grip":{"pos":[0,50,0],"axis":"-y"},
"tip":{"attach":{"to":"post","at":"top"},"offset":[0,5,0],"yaw":90,"pitch":0,"roll":0},
"top":{"attach":"cap"}}}
"##;

#[test]
fn every_field_reads_and_writes_back() {
    let doc = read("sink", SINK);
    assert_eq!(doc.text(), SINK);
    assert_eq!(read("sink", &doc.text()), doc);
    // One changed value is a new revision; a layout change is not.
    let changed = read("sink", &SINK.replace("\"peak\":-0.5", "\"peak\":-0.25"));
    assert_ne!(changed.revision(), doc.revision());
    assert_eq!(read("sink", &SINK.replace(",\n", ",")).revision(), doc.revision());
}

fn compiled(name: &str, text: &str) -> CompiledProp {
    compile(&read(name, text), &|_| None, 0).unwrap_or_else(|issues| panic!("{name}:\n{}", render(&issues)))
}

fn part<'a>(prop: &'a CompiledProp, id: &str) -> &'a ResolvedPart {
    prop.parts
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("no part {id} in {:?}", prop.parts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>()))
}

/// A resolved centre in cm, within 0.01 cm; NaN skips an axis.
fn at(prop: &CompiledProp, id: &str, cm: [f32; 3]) {
    let pos = part(prop, id).pos * 100.0;
    for axis in 0..3 {
        if !cm[axis].is_nan() {
            assert!((pos[axis] - cm[axis]).abs() <= 0.01, "{id} is at {pos} cm, expected {cm:?}");
        }
    }
}

/// Resolved yaw and pitch (YXZ, degrees), within 0.05 degrees.
fn turned(prop: &CompiledProp, id: &str, yaw: f32, pitch: f32) {
    let (y, p, _) = part(prop, id).rot.to_euler(EulerRot::YXZ);
    let near = |a: f32, b: f32| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs() <= 0.05;
    assert!(near(y.to_degrees(), yaw) && near(p.to_degrees(), pitch), "{id}: yaw {} pitch {}", y.to_degrees(), p.to_degrees());
}

#[test]
fn the_corpus_resolves_to_its_goldens() {
    let counts: Vec<(&str, usize)> =
        FIXTURES.iter().take(5).map(|(name, text, _)| (*name, compiled(name, text).parts.len())).collect();
    assert_eq!(counts, [("bench", 7), ("greatsword", 9), ("market_stall", 46), ("wall_torch", 6), ("well", 53)]);

    let bench = compiled("bench", BENCH);
    at(&bench, "leg_back~x", [-85.0, 70.5, -41.0]);
    turned(&bench, "leg_back~x", 0.0, -12.01);

    let well = compiled("well", WELL);
    at(&well, "rim", [0.0, 85.0, 0.0]);
    at(&well, "stones[1].ring[3]", [65.62, 66.0, -21.32]);
    turned(&well, "stones[1].ring[3]", 108.0, 0.0);
    at(&well, "stones_mid[0]", [21.32, 40.0, 65.62]);
    at(&well, "roof~z.slab", [0.0, 207.3, -41.0]);
    turned(&well, "roof~z.slab", 180.0, 35.0);

    let stall = compiled("market_stall", MARKET_STALL);
    at(&stall, "awning[0].stripe", [-108.25, f32::NAN, f32::NAN]);
    at(&stall, "crate[1].fruit[5]", [78.0, 129.06, 53.8]);

    let sword = compiled("greatsword", GREATSWORD);
    at(&sword, "pommel", [f32::NAN, -40.5, f32::NAN]);

    let mirror = compiled("probe_mirror", PROBE_MIRROR);
    at(&mirror, "arm~x.hand", [-50.0, 80.0, 0.0]);
    assert_eq!(part(&mirror, "outer.inner.cube").color, "#30c040", "the nearest level wins");

    let attach = compiled("probe_attach", PROBE_ATTACH);
    at(&attach, "cube", [0.0, 128.0, 0.0]);
    at(&attach, "knob", [0.0, 106.0, 0.0]);
    at(&attach, "spokes_z[0]", [-60.0, 90.0, 0.0]);
    at(&attach, "spokes_z[1]", [-80.0, 70.0, 0.0]);
    let size = |id: &str| part(&attach, id).aabb.size();
    assert!((size("cap_height") - size("cap_fromto")).abs().max_element() < 1e-5, "a capsule's height and its ends agree");
}

#[test]
fn compiling_twice_gives_identical_bytes() {
    for (name, text, _) in FIXTURES {
        let doc = read(name, text);
        let bytes = |seed| serde_json::to_vec(&compile(&doc, &|_| None, seed).unwrap()).unwrap();
        assert_eq!(bytes(0), bytes(0), "{name}");
        assert_eq!(bytes(7), bytes(7), "{name}");
    }
}

#[test]
fn extras_sockets_and_warnings_resolve() {
    let torch = compiled("wall_torch", WALL_TORCH);
    let [light] = torch.lights.as_slice() else { panic!("one light") };
    assert_eq!(
        (light.part.as_str(), light.color.as_str(), light.radius, light.intensity, light.flicker),
        ("torch.flame", "$light", 6.0, 1.6, 0.5)
    );
    assert!((light.pos - part(&torch, "torch.flame").pos).length() < 1e-6, "a light sits at its part");
    assert!(!part(&torch, "torch.flame").glow_light, "a part's own light replaces its glow light");
    let [fx] = torch.fx.as_slice() else { panic!("one emitter") };
    assert_eq!((fx.preset.as_str(), fx.size), ("fire", 0.5));
    assert!((torch.sockets["wall_mount"].rot * Vec3::Y - Vec3::NEG_Z).length() < 1e-6, "+y points into the wall");
    let sword = compiled("greatsword", GREATSWORD);
    assert!((sword.sockets["tip"].pos * 100.0 - Vec3::new(0.0, 114.5, 0.0)).length() < 0.01);
    for (name, text, _) in FIXTURES.iter().take(5) {
        assert_eq!(compiled(name, text).warnings, [], "{name} is clean");
    }
    let frames: Vec<String> = compiled("probe_frames", PROBE_FRAMES)
        .warnings
        .into_iter()
        .filter(|w| w.code == "frame_turn")
        .flat_map(|w| w.ids)
        .collect();
    assert_eq!(frames, ["board_lean_x"], "only the board leaning sideways turns");
}
