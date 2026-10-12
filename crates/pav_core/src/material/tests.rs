use glam::Vec3;
use serde_json::{Value, json};

use super::eval::*;
use super::resolve::{MAX_COST, MAX_DEPTH};
use super::*;
use crate::issue::{Code, Issue, Severity};
use crate::props::doc::MapMode;

fn file(name: &str, mut body: Value) -> String {
    let object = body.as_object_mut().unwrap();
    object.entry("format").or_insert(json!(1));
    object.entry("name").or_insert(json!(name));
    object.entry("unit").or_insert(json!("cm"));
    body.to_string()
}

fn problems(body: Value) -> Vec<String> {
    read_file("x", &file("x", body)).unwrap_err().iter().map(Issue::to_string).collect()
}

fn library(sources: &[(&str, Value)]) -> Library {
    let texts: Vec<(String, String)> = sources.iter().map(|(name, body)| (name.to_string(), file(name, body.clone()))).collect();
    let pairs: Vec<(&str, &str)> = texts.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
    Library::from_sources(&pairs)
}

fn issues_of(lib: &Library, name: &str) -> Vec<String> {
    lib.issues().get(name).map(|issues| issues.iter().map(Issue::to_string).collect()).unwrap_or_default()
}

#[test]
fn errors_name_the_path_and_a_fix() {
    let got = problems(json!({
        "colour": "#808080",
        "layers": [
            {"type": "brik", "size": [40, 20]},
            {"type": "noise", "sise": 30},
            {"type": "stripes", "rings": true, "color": "red"},
        ],
    }));
    assert_eq!(
        got,
        [
            r#"colour: unknown key — did you mean "color"?"#,
            r#"layers[0].type: unknown pattern "brik" (expected noise, voronoi, wave, brick, checker, gradient, magic, planks, tiles, cobbles, stripes, dots, shingles, wood) — did you mean "brick"?"#,
            r#"layers[1].sise: unknown key — did you mean "size"?"#,
            "layers[2].rings: unknown key — rings is for wave; a stripes takes size, axis, cover, soft, color, shade, strength, glow, facing, masked_by, seed",
            r##"layers[2].color: "red" is not a #rrggbb colour or a role (expected #rrggbb or $floor, $floor2, $wall, $pillar, $accent, $light) — use a hex colour such as "#ff0000""##,
        ]
    );
    let got = problems(json!({
        "color": "$wal", "look": "toon", "scale": 20,
        "layers": [
            {"type": "brick", "size": [0.05, 20], "masked_by": "prev", "cells": ["#101010", "#202020", "#303030", "#404040", "#505050"]},
            {"type": "noise", "color": "#101010", "shade": 0.5, "detail": 6},
            {"type": "wood", "soft": 0, "angle": 45},
        ],
    }));
    assert_eq!(
        got,
        [
            r#"color: unknown role "$wal" (expected $floor, $floor2, $wall, $pillar, $accent, $light) — did you mean "$wall"?"#,
            r#"look: unknown look "toon" (expected flat, cel, lit, unlit) — cel is the toon look"#,
            "scale: 20 is outside 0.1 to 10",
            "layers[0].size[0]: 0.05 is outside 0.1 to 100000 cm — lengths are in cm: 0.05 m is 5",
            "layers[0].cells: 5 cell colours; cells takes 2-4 — keep at most 4",
            "layers[1].detail: 6 is outside 1 to 4",
            "layers[1].shade: a layer paints color or darkens by shade, not both — keep one",
            "layers[2].angle: unknown key — angle is not available yet: use axis, or turn:true on the material",
            "layers[2].soft: 0 is outside 0.01 to 1",
        ]
    );
    // masked_by on the first layer is reported once the layer itself reads.
    assert!(problems(json!({"color": "#808080", "layers": [{"type": "tiles", "masked_by": "prev"}]}))
        .contains(&"layers[0].masked_by: the first layer has no layer below it — drop masked_by, or put this layer above the one that masks it".to_string()));
    // Dots have no border, so no cavity or bevel.
    assert_eq!(
        problems(json!({"color": "#808080", "layers": [{"type": "dots", "cavity": 0.3}]})),
        [
            "layers[0].cavity: unknown key — cavity is for voronoi, brick, planks, tiles, cobbles, shingles; a dots takes size, radius, offset, jitter, vary, cells, axis, color, shade, strength, glow, facing, masked_by, seed"
        ]
    );
    let four = json!({"color": "#808080", "layers": [{"type": "checker"}, {"type": "checker"}, {"type": "checker"}, {"type": "checker"}]});
    assert_eq!(problems(four), ["layers: 4 layers; a material has at most 3 — merge or drop layers"]);
    let bare = read_file("pb/gold", r##"{"name":"gold","color":"#d1a400"}"##).unwrap_err();
    let bare: Vec<String> = bare.iter().map(Issue::to_string).collect();
    assert_eq!(
        bare,
        [
            "format: missing — add \"format\":1",
            "unit: missing — add \"unit\":\"cm\" (the house unit)",
            "name: \"gold\" is not the file's name — use \"name\":\"pb/gold\"",
        ]
    );
}

#[test]
fn duplicate_keys_are_rejected_with_their_path() {
    let text = r##"{"format":1,"name":"x","unit":"cm","color":"#808080","layers":[{"type":"noise","size":10,"size":20}]}"##;
    let issues = read_file("x", text).unwrap_err();
    assert_eq!(issues[0].code, Code::Duplicate);
    assert_eq!(issues[0].to_string(), r#"layers[0].size: "size" appears twice — keep one; names must be unique"#);
}

#[test]
fn inline_materials_read_in_the_assets_unit() {
    let value = json!({"base": "MAT/oak_planks", "layers": [{"type": "tiles", "size": 0.5}]});
    let fields = read_inline(&value, "materials.floor", crate::props::doc::Unit::M).unwrap();
    assert_eq!(fields.layers.unwrap()[0].size2([0.0; 2]), [0.5, 0.5]);
    let issues =
        read_inline(&json!({"name": "x", "colr": "#808080"}), "materials.floor", crate::props::doc::Unit::Cm).unwrap_err();
    let issues: Vec<String> = issues.iter().map(Issue::to_string).collect();
    assert_eq!(
        issues,
        [
            r#"materials.floor.colr: unknown key — did you mean "color"?"#,
            "materials.floor.name: unknown key — only material files take this; an inline material uses the asset's unit",
        ]
    );
}

#[test]
fn bases_merge_shallowly_and_replace_layers_wholesale() {
    let core = Library::core();
    let oak = core.get("MAT/oak_planks").unwrap();
    let staves = core.get("MAT/barrel_staves").unwrap();
    assert_eq!((staves.color.as_str(), staves.turn, staves.roughness), (oak.color.as_str(), true, Some(0.7)));
    assert_eq!((&staves.layers, staves.layers_from.as_str()), (&oak.layers, "MAT/oak_planks"));
    // Two levels: gold_trim keeps pb/gold's colour and metal, overrides roughness, adds a layer.
    let trim = core.get("MAT/gold_trim").unwrap();
    assert_eq!((trim.color.as_str(), trim.metallic, trim.roughness, trim.layers.len()), ("#d1a400", 1.0, Some(0.3), 1));

    let lib = library(&[
        ("root", json!({"color": "#808080", "look": "lit", "layers": [{"type": "checker"}, {"type": "noise"}]})),
        ("one", json!({"base": "MAT/root", "layers": [{"type": "tiles"}]})),
        ("none", json!({"base": "MAT/root", "layers": []})),
        ("tint", json!({"base": "MAT/one", "color": "#203040"})),
    ]);
    let one = lib.get("MAT/one").unwrap();
    assert_eq!((one.look, one.layers.len(), one.layers[0].pattern.name()), (crate::shape::Look::Lit, 1, "tiles"));
    assert!(lib.get("MAT/none").unwrap().layers.is_empty());
    let tint = lib.get("MAT/tint").unwrap();
    assert_eq!((tint.color.as_str(), tint.layers_from.as_str()), ("#203040", "MAT/one"));
}

#[test]
fn base_chains_stop_at_four_hops_and_cycles() {
    let mut sources = vec![("d0".to_string(), json!({"color": "#808080"}))];
    for i in 1..=MAX_DEPTH + 1 {
        sources.push((format!("d{i}"), json!({"base": format!("MAT/d{}", i - 1)})));
    }
    sources.push(("a".into(), json!({"base": "MAT/b"})));
    sources.push(("b".into(), json!({"base": "MAT/a"})));
    sources.push(("typo".into(), json!({"base": "MAT/d"})));
    sources.push(("loose".into(), json!({"base": "oak_planks"})));
    sources.push(("broken".into(), json!({"color": "#80808"})));
    sources.push(("child".into(), json!({"base": "MAT/broken"})));
    let refs: Vec<(&str, Value)> = sources.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
    let lib = library(&refs);
    assert!(lib.get(&format!("MAT/d{MAX_DEPTH}")).is_some(), "{MAX_DEPTH} hops resolve");
    assert_eq!(
        issues_of(&lib, "MAT/d5"),
        [
            "base: 5 levels of base: MAT/d5 → MAT/d4 → MAT/d3 → MAT/d2 → MAT/d1 → MAT/d0 (expected at most 4) — start from a material nearer the root"
        ]
    );
    assert_eq!(
        issues_of(&lib, "MAT/a"),
        ["base: bases go round in a circle: MAT/a → MAT/b → MAT/a — one of them must not have a base"]
    );
    assert_eq!(issues_of(&lib, "MAT/b")[0].split(':').next(), Some("base"));
    assert_eq!(issues_of(&lib, "MAT/typo"), [r#"base: base "MAT/d" is not in the library — did you mean "MAT/d0"?"#]);
    assert_eq!(
        issues_of(&lib, "MAT/loose"),
        [r#"base: "oak_planks" is not a library material — library names start with MAT/, e.g. "MAT/oak_planks""#]
    );
    assert_eq!(issues_of(&lib, "MAT/child"), [r#"base: base "MAT/broken" does not load — fix the errors in MAT/broken first"#]);
    let broken: Vec<&str> = lib.broken().collect();
    assert_eq!(broken, ["MAT/a", "MAT/b", "MAT/broken", "MAT/child", "MAT/d5", "MAT/loose", "MAT/typo"]);
}

#[test]
fn cm_and_m_pack_alike() {
    let layers_cm = json!([{"type": "planks", "size": [120, 20], "gap": 2}, {"type": "gradient", "from": 0, "to": 60}, {"type": "dots", "size": 15, "radius": 4}]);
    let layers_m = json!([{"type": "planks", "size": [1.2, 0.2], "gap": 0.02}, {"type": "gradient", "from": 0, "to": 0.6}, {"type": "dots", "size": 0.15, "radius": 0.04}]);
    let lib = library(&[
        ("in_cm", json!({"color": "#a0703f", "layers": layers_cm})),
        ("in_m", json!({"unit": "m", "color": "#a0703f", "layers": layers_m})),
    ]);
    let pack = |name: &str| lib.get(name).unwrap().pack(MapMode::Object, &[]).gpu;
    assert_eq!(pack("MAT/in_cm"), pack("MAT/in_m"));
    assert_eq!(pack("MAT/in_cm").layers[0].a, [1.2, 0.2, 0.02, 0.0]);
}

#[test]
fn every_core_material_loads_clean_within_budget() {
    let core = Library::core();
    assert!(core.issues().is_empty(), "{:#?}", core.issues());
    assert_eq!(core.names().count(), super::library::MATERIALS.len());
    assert!(core.get("MAT/pb/gold").is_some() && core.get("MAT/oak_planks").is_some());
    for name in core.names() {
        let material = core.get(name).unwrap();
        assert!(material.cost() <= MAX_COST, "{name} costs {}", material.cost());
    }
    assert_eq!(core.get("MAT/mossy_stone").unwrap().cost(), 5.5);
}

#[test]
fn budgets_and_legibility_warn() {
    let lib = library(&[
        ("heavy", json!({"color": "#808080", "layers": [{"type": "cobbles"}, {"type": "voronoi"}, {"type": "noise"}]})),
        (
            "fine",
            json!({"color": "#808080", "scale": 0.5, "layers": [{"type": "brick", "size": [8, 6], "gap": 1}, {"type": "dots", "radius": 2}]}),
        ),
        // A spot's feature is its diameter, 2·sqrt(cover/π)·size; edges are a cell across.
        (
            "spots",
            json!({"color": "#808080", "layers": [{"type": "voronoi", "size": 7}, {"type": "voronoi", "size": 7, "feature": "edges"}]}),
        ),
    ]);
    assert!(lib.get("MAT/heavy").is_some(), "warnings keep the material");
    assert_eq!(
        issues_of(&lib, "MAT/heavy"),
        [
            "layers: costs 6.5 units (cobbles 2.5 + voronoi 2.5 + noise 1.5); keep it at 6 or less — drop a layer, or lower a noise's detail"
        ]
    );
    let fine = &lib.issues()["MAT/fine"];
    assert!(fine.iter().all(|issue| issue.severity == Severity::Warning && issue.code == Code::Legibility));
    let fine: Vec<String> = fine.iter().map(Issue::to_string).collect();
    assert_eq!(
        fine,
        [
            "layers[0].size: 3 cm features are under 5 cm: about 1.4 px at the game camera at 540p, where they fade to flat colour — make them 5 cm or larger, or drop the layer",
            "layers[0].gap: 0.5 cm gaps are under 1.5 cm: they vanish at the game camera at 540p — make gap 1.5 cm or wider, or 0 for none",
            "layers[1].radius: 2 cm features are under 5 cm: about 1.0 px at the game camera at 540p, where they fade to flat colour — make them 5 cm or larger, or drop the layer",
        ]
    );
    assert_eq!(
        issues_of(&lib, "MAT/spots"),
        [
            "layers[0].size: 4.3 cm features are under 5 cm: about 2.1 px at the game camera at 540p, where they fade to flat colour — make them 5 cm or larger, or drop the layer"
        ]
    );
}

#[test]
fn packs_the_gpu_row() {
    assert_eq!(std::mem::size_of::<GpuLayer>(), 112);
    assert_eq!((std::mem::size_of::<GpuMaterial>(), MATERIAL_BYTES), (368, 368));
    let core = Library::core();
    let oak = core.get("MAT/oak_planks").unwrap().pack(MapMode::World, &[]);
    assert_eq!((oak.gpu.head, oak.gpu.pbr), ([2, 1, 0, 0], [0.7, 0.0, 1.0, 0.0]));
    let planks = oak.gpu.layers[0];
    assert_eq!((planks.head[0] & 0xff, planks.head[0] >> 8 & 0xf, planks.a), (kernel::GRID, 1, [1.2, 0.2, 0.02, 0.0]));
    assert_eq!(oak.gpu.layers[1].head[0] & 0xff, kernel::WOOD);
    let words = oak.gpu.words();
    assert_eq!((words.len(), words[0], f32::from_bits(words[4]), f32::from_bits(words[8 + 8])), (92, 2, 0.7, 1.2));
    // `turn` swaps u and v in the kernel (head z) and turns 1D directions x to y here.
    let staves = core.get("MAT/barrel_staves").unwrap().pack(MapMode::Axis, &[]).gpu;
    assert_eq!(
        (staves.head, oak.gpu.layers[1].dir, staves.layers[1].dir),
        ([2, 2, 1, 0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0])
    );
    // Roles come from the theme's palette, else the props defaults.
    let flag = core.get("MAT/flagstones").unwrap();
    let srgb = |c: Vec3| crate::color::to_hex(crate::Color(c.to_array()));
    assert_eq!(srgb(flag.pack(MapMode::World, &[]).base), "#7a7268");
    assert_eq!(srgb(flag.pack(MapMode::World, &[("$floor", "#4a3f3a")]).base), "#4a3f3a");
}

/// Samples per material for the golden: four ways of looking at it, 16 points each.
fn golden_samples(material: &Material) -> String {
    let rnd = |h: u32| (pcg(h) >> 8) as f32 / 16_777_216.0;
    let configs = [
        // A world-mapped floor at the game camera at 1080p.
        (MapMode::World, 0u32, 0.0104f32),
        // A 1 m box's side, close up.
        (MapMode::Object, 7, 0.0021),
        // A cylinder's side (radius 0.4 m), axis map.
        (MapMode::Axis, 3, 0.0104),
        // A far world-mapped wall: most layers fade to their means.
        (MapMode::World, 0, 0.08),
    ];
    let mut out = String::new();
    for (index, (map, seed, footprint)) in configs.into_iter().enumerate() {
        let packed = material.pack(map, &[]);
        for i in 0..16u32 {
            let h = i * 977 + index as u32 * 131 + 5;
            let (a, b) = (rnd(h) * 2.0 - 1.0, rnd(h ^ 0x5bd1e995) * 2.0 - 1.0);
            let (p, n, ext) = match index {
                0 => (Vec3::new(a * 2.4, 0.0, b * 2.4), Vec3::Y, Vec3::splat(2.0)),
                1 => (Vec3::new(a * 0.5, b * 0.5, 0.5), Vec3::Z, Vec3::splat(0.5)),
                2 => {
                    let t = a * std::f32::consts::PI;
                    let n = Vec3::new(t.sin(), 0.0, t.cos());
                    (n * 0.4 + Vec3::Y * b * 0.55, n, Vec3::new(0.4, 0.55, 0.4))
                }
                _ => (Vec3::new(a * 6.0, b * 1.9 + 1.9, 0.0), Vec3::Z, Vec3::splat(2.0)),
            };
            let sample = Sample { local: p, local_normal: n, world: p, world_normal: n, half_extents: ext, seed, footprint };
            let shaded = packed.eval(&sample);
            out += &crate::color::to_hex(crate::Color(shaded.rgb.to_array()));
            out += &format!("{} ", (shaded.glow * 32.0).round());
        }
    }
    out
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// FNV-1a of each core material's golden samples (8-bit sRGB per sample, as the GPU writes
/// them). A change here changes what the renderer must match: re-bless only on purpose.
const GOLDEN: &[(&str, u64)] = &[
    ("MAT/awning_stripes", 0x0438a3d4446e2dca),
    ("MAT/barrel_staves", 0xbe3057b135eda4b4),
    ("MAT/brick_wall", 0x2ff7d39f887982f9),
    ("MAT/cobblestone", 0xf5a4e74c229f65a3),
    ("MAT/dungeon_wall", 0xbc6cb81ab0397aa4),
    ("MAT/flagstones", 0x80ca651a11649d03),
    ("MAT/gold_trim", 0x7cf5d2f192751542),
    ("MAT/mossy_stone", 0x81a073633dadeb53),
    ("MAT/oak_planks", 0x24e8dc8cba3f359a),
    ("MAT/pb/gold", 0x06cda2e1d5a834e5),
    ("MAT/pb/iron", 0x8487fe0051770aa5),
    ("MAT/pillar_stone", 0x3f58124d9aad6b27),
    ("MAT/roof_shingles", 0x0f43271947bdca04),
    ("MAT/wall_cap", 0xfe848ae2c0758dda),
    ("MAT/wrought_iron", 0x20497af1f56ddb24),
];

#[test]
fn eval_matches_its_golden() {
    let core = Library::core();
    let got: Vec<(String, u64)> =
        core.names().map(|name| (name.to_string(), fnv(&golden_samples(core.get(name).unwrap())))).collect();
    let text: Vec<String> = got.iter().map(|(name, hash)| format!("    ({name:?}, {hash:#018x}),")).collect();
    let want: Vec<(String, u64)> = GOLDEN.iter().map(|(name, hash)| (name.to_string(), *hash)).collect();
    assert_eq!(
        got,
        want,
        "new goldens:\n{}\nfirst samples of oak_planks: {}",
        text.join("\n"),
        &golden_samples(core.get("MAT/oak_planks").unwrap())[..120]
    );
}

#[test]
fn layers_fade_to_their_mean_below_a_pixel() {
    let cobble = Library::core().get("MAT/cobblestone").unwrap().pack(MapMode::World, &[]);
    let at = |x: f32, z: f32, footprint: f32| cobble.eval(&Sample::at(Vec3::new(x, 0.0, z), Vec3::Y, footprint)).rgb;
    let points = [(0.1, 0.2), (1.3, -0.7), (-2.2, 0.9), (0.55, 3.1)];
    // At 1 m per pixel both layers (30 cm cobbles, 80 cm noise) are below a pixel.
    let far: Vec<Vec3> = points.iter().map(|&(x, z)| at(x, z, 1.0)).collect();
    assert!(far.iter().all(|c| *c == far[0]), "{far:?}");
    // At the game camera they are resolved.
    let near: Vec<Vec3> = points.iter().map(|&(x, z)| at(x, z, 0.0104)).collect();
    assert!(near.iter().any(|c| (*c - near[0]).abs().max_element() > 0.01), "{near:?}");
}

#[test]
fn cell_colours_fade_to_their_mean() {
    let tiles = json!({"type": "tiles", "size": 20, "gap": 0, "vary": 0, "cavity": 0, "cells": ["#ff0000", "#0000ff"]});
    let lib = library(&[("cells", json!({"color": "#808080", "layers": [tiles]}))]);
    let packed = lib.get("MAT/cells").unwrap().pack(MapMode::World, &[]);
    let at = |footprint: f32| packed.eval(&Sample::at(Vec3::new(0.13, 0.0, 0.41), Vec3::Y, footprint)).rgb;
    let mean = Vec3::new(0.5, 0.0, 0.5);
    assert_eq!(packed.gpu.layers[0].b, [0.0, 0.5, 0.0, 0.5], "b.yzw carries the mean");
    // 20 cm cells at 50 cm per pixel are skipped: the mean, not the grey base.
    assert!((at(0.5) - mean).abs().max_element() < 1e-6, "{}", at(0.5));
    // Just resolved (1.05 px per cell), the fade starts from the mean.
    assert!((at(0.19) - mean).abs().max_element() < 0.01, "{}", at(0.19));
    // Fully resolved: one of the cells' colours.
    assert!([Vec3::X, Vec3::Z].contains(&at(0.001)), "{}", at(0.001));
}

#[test]
fn voronoi_spots_resolve_by_their_diameter() {
    // 30 cm cells, cover 0.3: spots 18.5 cm across (radius 9.3 cm).
    let lib = library(&[("spots", json!({"color": "#808080", "layers": [{"type": "voronoi", "size": 30}]}))]);
    let packed = lib.get("MAT/spots").unwrap().pack(MapMode::World, &[]);
    let points = [(0.1, 0.2), (1.3, -0.7), (-2.2, 0.9), (0.55, 3.1), (0.8, 0.05), (-0.4, -1.6)];
    let shades = |footprint: f32| -> Vec<Vec3> {
        points.iter().map(|&(x, z)| packed.eval(&Sample::at(Vec3::new(x, 0.0, z), Vec3::Y, footprint)).rgb).collect()
    };
    // 1.24 px per spot: resolved (it was skipped while the radius stood for the size).
    let near = shades(0.15);
    assert!(near.iter().any(|c| *c != near[0]), "{near:?}");
    // 0.93 px per spot: skipped, one flat colour.
    let far = shades(0.2);
    assert!(far.iter().all(|c| *c == far[0]), "{far:?}");
}

#[test]
fn cover_tables_regenerate() {
    let (noise, magic) = cover_tables();
    let text = |name: &str, t: &[[f32; 21]; 4]| {
        let rows: Vec<String> = t
            .iter()
            .map(|row| format!("    [{}],", row.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ")))
            .collect();
        format!("pub const {name}: [[f32; 21]; 4] = [\n{}\n];", rows.join("\n"))
    };
    let bits = |t: &[[f32; 21]; 4]| t.map(|row| row.map(f32::to_bits));
    assert!(
        bits(&noise) == bits(&NOISE_COVER) && bits(&magic) == bits(&MAGIC_COVER),
        "the cover tables changed; if on purpose, paste into eval.rs:\n{}\n{}",
        text("NOISE_COVER", &noise),
        text("MAGIC_COVER", &magic)
    );
}
