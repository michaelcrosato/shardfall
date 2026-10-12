//! Translation into props (plan §8.5, §8.10): models built in code are fitted, written, read back
//! by the engine's reader, compiled and checked, and measured (§8.6). No binaries are committed.

mod common;

use std::path::{Path, PathBuf};

use common::{MeshBuilder, cuboid, extrusion, revolve, temp_dir};
use pav_core::props::mesh::{MAX_PALETTE, MAX_TRIS, MeshFile};
use pav_core::props::{Doc, lint};
use pav_tools::import::fit::{self, Mode, Options, Translation};
use pav_tools::import::normalize::{self, Norm};
use pav_tools::import::read_model;
use pav_tools::{Output, Session, call};
use serde_json::{Value, json};

fn write(dir: &Path, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// One model of these (positions, indices, linear colour) parts, in one node.
fn model(parts: &[(Vec<[f32; 3]>, Vec<u32>, [f32; 3])]) -> Vec<u8> {
    let mut b = MeshBuilder::new();
    let prims: Vec<Value> = parts
        .iter()
        .map(|(pos, idx, rgb)| {
            let m = b.colour(*rgb);
            b.tri_prim(pos, idx, Some(m))
        })
        .collect();
    let m = b.mesh(prims);
    b.node(json!({"name": "model", "mesh": m}));
    b.glb(&[0])
}

fn boxed(min: [f32; 3], max: [f32; 3], rgb: [f32; 3]) -> (Vec<[f32; 3]>, Vec<u32>, [f32; 3]) {
    let (p, _, i) = cuboid(min, max);
    (p, i, rgb)
}

/// Translates a model file the way `asset_import` does (default placement).
fn translate(path: &Path, name: &str, mode: Mode) -> Translation {
    let mut s = read_model(path).unwrap();
    normalize::normalize(&mut s, &Norm::default());
    let o = Options { mode, pack: "test_fit".into(), name: name.into(), ..Default::default() };
    fit::translate(&s, &o).unwrap()
}

/// The engine's strict reader (with format 2's check), compiler and lint take the text as it is.
fn engine_accepts(t: &Translation) {
    let doc = Doc::read(&t.text).unwrap_or_else(|e| panic!("{}", pav_core::issue::render(&e)));
    assert_eq!(doc.text(), t.text, "canonical");
    let def = pav_core::props::PropDef::new(doc).unwrap_or_else(|e| panic!("{}", pav_core::issue::render(&e)));
    let _ = lint::lint(&def.compiled, &lint::Options::default());
}

fn kinds(t: &Translation) -> Vec<String> {
    let v: Value = serde_json::from_str(&t.text).unwrap();
    v["parts"]
        .as_object()
        .unwrap()
        .values()
        .map(|p| p["type"].as_str().or(p["mesh"].as_str().map(|_| "mesh")).unwrap().to_string())
        .collect()
}

const BROWN: [f32; 3] = [0.40, 0.18, 0.08];
const DARK: [f32; 3] = [0.12, 0.06, 0.03];

/// A table: a top and four legs.
fn table() -> Vec<u8> {
    let mut parts = vec![boxed([-0.6, 0.7, -0.4], [0.6, 0.76, 0.4], BROWN)];
    for (x, z) in [(0.5, 0.3), (-0.5, 0.3), (0.5, -0.3), (-0.5, -0.3)] {
        parts.push(boxed([x - 0.03, 0.0, z - 0.03], [x + 0.03, 0.7, z + 0.03], DARK));
    }
    model(&parts)
}

#[test]
fn a_table_is_a_top_and_mirrored_legs() {
    let dir = temp_dir("fit_table");
    let t = translate(&write(&dir, "table.glb", table()), "table", Mode::Auto);
    let doc: Value = serde_json::from_str(&t.text).unwrap();
    let parts = doc["parts"].as_object().unwrap();
    assert_eq!(parts.len(), 2, "{}", t.text);
    let legs = parts.values().find(|p| p.get("mirror").is_some()).expect("the legs fold into one");
    assert_eq!(legs["mirror"], json!(["x", "z"]), "{}", t.text);
    assert_eq!(legs["size"], json!([6, 70, 6]));
    assert!(t.fidelity.iou >= 0.99, "{:?}", t.fidelity);
    assert!(t.fidelity.de < 1.0 && t.mesh_pct == 0.0 && t.tier == "fit", "{:?} {}", t.fidelity, t.tier);
    engine_accepts(&t);
    // Deterministic: the same model gives the same bytes.
    let again = translate(&dir.join("table.glb"), "table", Mode::Auto);
    assert_eq!(again.text, t.text);
}

#[test]
fn a_vase_is_a_lathe_and_a_blade_an_extrusion() {
    let dir = temp_dir("fit_round");
    let (pos, idx) = revolve(&[[0.10, 0.0], [0.16, 0.08], [0.18, 0.18], [0.12, 0.32], [0.07, 0.40], [0.09, 0.45]], 48);
    let vase = translate(&write(&dir, "vase.glb", model(&[(pos, idx, [0.2, 0.3, 0.6])])), "vase", Mode::Auto);
    assert_eq!(kinds(&vase), ["lathe"], "{}", vase.text);
    assert!(vase.fidelity.iou >= 0.95, "{:?}", vase.fidelity);
    engine_accepts(&vase);

    let (pos, idx) = extrusion(&[[-0.03, 0.0], [0.03, 0.0], [0.03, 0.5], [0.0, 0.62], [-0.03, 0.5]], 0.01);
    let blade = translate(&write(&dir, "blade.glb", model(&[(pos, idx, [0.6, 0.62, 0.66])])), "blade", Mode::Auto);
    assert_eq!(kinds(&blade), ["extrude"], "{}", blade.text);
    assert!(blade.fidelity.iou >= 0.95, "{:?}", blade.fidelity);
    engine_accepts(&blade);
}

/// A lumpy ball of 9,600 triangles in 400 shades: no primitive fits it.
fn blob() -> Vec<u8> {
    let (rings, segs) = (60usize, 80usize);
    let mut b = MeshBuilder::new();
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..=rings {
        let lat = std::f32::consts::PI * (i as f32 / rings as f32 - 0.5);
        for j in 0..segs {
            let lon = std::f32::consts::TAU * j as f32 / segs as f32;
            let r = 0.3 * (1.0 + 0.45 * (3.0 * lon).sin() * (2.0 * lat).cos() + 0.2 * (5.0 * lat).sin());
            pos.push([r * lat.cos() * lon.sin(), 0.4 + r * lat.sin(), r * lat.cos() * lon.cos()]);
            let shade = ((i * 7 + j * 3) % 20) as f32 / 20.0;
            col.extend_from_slice(&[(60.0 + 150.0 * shade) as u8, (90 + (i % 20) * 6) as u8, (40 + (j % 20) * 9) as u8, 255]);
        }
    }
    for i in 0..rings as u32 {
        for j in 0..segs as u32 {
            let (a, c) = (i * segs as u32 + j, i * segs as u32 + (j + 1) % segs as u32);
            let (d, e) = (a + segs as u32, c + segs as u32);
            idx.extend_from_slice(&[a, c, e, a, e, d]);
        }
    }
    let (pa, ia, ca) = (b.vec3(&pos), b.indices(&idx), b.unorm8(&col, "VEC4"));
    let p = b.prim(pa, Some(ia), &[("COLOR_0", ca)], None);
    let m = b.mesh(vec![p]);
    b.node(json!({"mesh": m}));
    b.glb(&[0])
}

#[test]
fn a_noisy_blob_is_a_mesh_part_within_budget() {
    let dir = temp_dir("fit_blob");
    let t = translate(&write(&dir, "blob.glb", blob()), "blob", Mode::Auto);
    assert_eq!(kinds(&t), ["mesh"], "{}", t.text);
    let (name, mesh) = &t.meshes[0];
    assert_eq!(name, "test_fit/blob");
    assert!(
        mesh.f.len() <= MAX_TRIS && mesh.palette.len() <= MAX_PALETTE,
        "{} tris {} colours",
        mesh.f.len(),
        mesh.palette.len()
    );
    assert_eq!(MeshFile::parse(&mesh.text()).unwrap(), *mesh, "the mesh file reads back");
    assert!(t.mesh_pct > 99.0 && t.fidelity.iou >= 0.95, "{} {:?}", t.mesh_pct, t.fidelity);
    engine_accepts(&t);
    let again = translate(&dir.join("blob.glb"), "blob", Mode::Auto);
    assert_eq!((again.text, again.meshes[0].1.text()), (t.text.clone(), mesh.text()));
}

/// A model of one material (`material` is its glTF JSON, given the colour).
fn with_material(pos: &[[f32; 3]], idx: &[u32], material: Value) -> Vec<u8> {
    let mut b = MeshBuilder::new();
    let m = b.material(material);
    let p = b.tri_prim(pos, idx, Some(m));
    let mesh = b.mesh(vec![p]);
    b.node(json!({"mesh": mesh}));
    b.glb(&[0])
}

#[test]
fn see_through_models_are_refused_and_open_double_sided_meshes_show_both_sides() {
    let dir = temp_dir("fit_alpha");
    let (pos, _, idx) = cuboid([-0.2, 0.0, -0.2], [0.2, 0.4, 0.2]);
    let glass = json!({"alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorFactor": [0.3, 0.5, 0.8, 0.4]}});
    let path = write(&dir, "glass.glb", with_material(&pos, &idx, glass));
    let mut s = read_model(&path).unwrap();
    normalize::normalize(&mut s, &Norm::default());
    let o = Options { mode: Mode::Auto, pack: "test_fit".into(), name: "glass".into(), ..Default::default() };
    let e = fit::translate(&s, &o).err().expect("a see-through model is refused");
    assert!(e.to_string().starts_with("alpha: 100% of its surface is see-through"), "{e}");
    // Blend with nothing see-through is opaque.
    let opaque = json!({"alphaMode": "BLEND", "pbrMetallicRoughness": {"baseColorFactor": [0.3, 0.5, 0.8, 1.0]}});
    translate(&write(&dir, "opaque.glb", with_material(&pos, &idx, opaque)), "opaque", Mode::Auto);

    // A sheet (one open quad) seen from both sides: its mesh part draws its back lit.
    let sheet = [[-0.3, 0.0, 0.0], [0.3, 0.0, 0.0], [0.3, 0.6, 0.0], [-0.3, 0.6, 0.0]];
    let banner = |double: bool| json!({"doubleSided": double, "pbrMetallicRoughness": {"baseColorFactor": [0.6, 0.1, 0.1, 1.0]}});
    let two = translate(&write(&dir, "two.glb", with_material(&sheet, &[0, 1, 2, 0, 2, 3], banner(true))), "two", Mode::Mesh);
    assert!(two.meshes[0].1.two_sided && two.meshes[0].1.text().contains(r#""two_sided":true"#));
    let one = translate(&write(&dir, "one.glb", with_material(&sheet, &[0, 1, 2, 0, 2, 3], banner(false))), "one", Mode::Mesh);
    assert!(!one.meshes[0].1.two_sided, "single-sided: its back is not meant to be seen");
    let closed = translate(&write(&dir, "closed.glb", with_material(&pos, &idx, banner(true))), "closed", Mode::Mesh);
    assert!(!closed.meshes[0].1.two_sided, "a closed solid's back is never seen");
    engine_accepts(&two);
}

#[test]
fn thin_small_parts_stay_out_of_the_collider() {
    let dir = temp_dir("fit_solid");
    // A crate with a small plaque on its front; a floor tile alone.
    let crate_ = model(&[boxed([-0.3, 0.0, -0.3], [0.3, 0.6, 0.3], BROWN), boxed([-0.05, 0.25, 0.3], [0.05, 0.35, 0.31], DARK)]);
    let t = translate(&write(&dir, "crate.glb", crate_), "crate", Mode::Auto);
    let doc: Value = serde_json::from_str(&t.text).unwrap();
    let parts = doc["parts"].as_object().unwrap();
    assert_eq!(parts.values().filter(|p| p["solid"] == json!(false)).count(), 1, "{}", t.text);
    assert!(parts.values().any(|p| p["size"] == json!([10, 10, 1]) && p["solid"] == json!(false)), "{}", t.text);
    engine_accepts(&t);
    let tile =
        translate(&write(&dir, "tile.glb", model(&[boxed([-0.5, 0.0, -0.5], [0.5, 0.02, 0.5], BROWN)])), "tile", Mode::Auto);
    assert!(!tile.text.contains("solid"), "a thin part as large as its asset stays solid: {}", tile.text);
}

/// A cube whose faces alternate red and blue (vertex colours, one material).
fn two_colour_cube() -> Vec<u8> {
    let (pos, _, idx) = cuboid([-0.2, 0.0, -0.2], [0.2, 0.4, 0.2]);
    let mut col = Vec::new();
    for face in 0..6 {
        let c: [u8; 4] = if face % 2 == 0 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
        for _ in 0..4 {
            col.extend_from_slice(&c);
        }
    }
    let mut b = MeshBuilder::new();
    let (pa, ia, ca) = (b.vec3(&pos), b.indices(&idx), b.unorm8(&col, "VEC4"));
    let p = b.prim(pa, Some(ia), &[("COLOR_0", ca)], None);
    let m = b.mesh(vec![p]);
    b.node(json!({"mesh": m}));
    b.glb(&[0])
}

/// One box of `size` metres, for folders of many models.
fn cube(size: f32) -> Vec<u8> {
    model(&[boxed([-size / 2.0, 0.0, -size / 2.0], [size / 2.0, size, size / 2.0], BROWN)])
}

fn title(file: &Path) -> String {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    v["source"]["title"].as_str().unwrap().to_string()
}

fn tool(s: &mut Session, args: Value) -> anyhow::Result<Value> {
    match call(s, "asset_import", args.as_object().unwrap())? {
        Output::Json(v) => Ok(v),
        Output::Image { meta, png, path } => {
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']) && path.is_some_and(|p| p.exists()));
            Ok(json!({"image": meta}))
        }
    }
}

#[test]
fn the_tool_translates_checks_compares_and_surveys() {
    let dir = temp_dir("fit_tool");
    let models = dir.join("models");
    std::fs::create_dir_all(&models).unwrap();
    write(&models, "table.glb", table());
    write(&models, "blob.glb", blob());
    let out = dir.join("props");
    // Compare images go beside the download cache: keep them out of the source tree.
    // SAFETY: only this test reads the variable.
    unsafe { std::env::set_var("PAV_ASSET_CACHE", dir.join("cache/assets")) };
    let mut s = Session::new("empty", 1).unwrap();
    let one = tool(&mut s, json!({"from": models.join("table.glb").to_str().unwrap(), "out": out.to_str().unwrap()})).unwrap();
    assert_eq!((one["tier"].as_str(), one["parts"].as_u64()), (Some("fit"), Some(2)), "{one}");
    let file = out.join("table.json");
    let checked = tool(&mut s, json!({"check": file.to_str().unwrap()})).unwrap();
    assert_eq!(checked["iou"], one["iou"], "{checked}");
    assert_eq!(checked["stored"]["iou"].as_f64().map(|x| (x * 100.0).round()), one["iou"].as_f64().map(|x| (x * 100.0).round()));
    let image = tool(&mut s, json!({"check": file.to_str().unwrap(), "compare": true})).unwrap();
    assert!(image["image"]["image"].as_str().unwrap().contains("compare"), "{image}");
    assert!(dir.join("cache/asset_import/compare/table.png").exists());

    // A folder: every model written, the blob's mesh beside it.
    let all = tool(&mut s, json!({"from": models.to_str().unwrap(), "out": out.to_str().unwrap()})).unwrap();
    assert_eq!(all["written"], 2, "{all}");
    assert!(out.join("meshes/blob.json").exists());
    let blob = tool(&mut s, json!({"check": out.join("blob.json").to_str().unwrap()})).unwrap();
    assert!(blob["iou"].as_f64().unwrap() >= 0.95, "{blob}");

    // The survey fills the ledger's fit columns, the same bytes twice.
    let ledger = dir.join("census.tsv");
    let survey = tool(&mut s, json!({"survey": models.to_str().unwrap(), "out": ledger.to_str().unwrap()})).unwrap();
    assert_eq!(survey["fit"]["translated"], 2, "{survey}");
    let text = std::fs::read_to_string(&ledger).unwrap();
    let row = text.lines().find(|l| l.starts_with("table\t")).unwrap();
    let cols: Vec<&str> = row.split('\t').collect();
    // parts mesh_pct iou iou_mean iou_gpu de chars tier
    assert_eq!((cols[8], cols[9], cols[12], cols[15]), ("2", "0", "", "fit"), "{row}");
    assert!(cols[10].parse::<f64>().unwrap() >= 0.99, "{row}");
    tool(&mut s, json!({"survey": models.to_str().unwrap(), "out": ledger.to_str().unwrap()})).unwrap();
    assert_eq!(std::fs::read_to_string(&ledger).unwrap(), text);
    let census =
        tool(&mut s, json!({"survey": models.to_str().unwrap(), "out": ledger.to_str().unwrap(), "fit": false})).unwrap();
    assert!(census.get("fit").is_none(), "{census}");
    let e = tool(&mut s, json!({"from": models.to_str().unwrap(), "fit": "best"})).unwrap_err();
    assert!(e.to_string().contains("auto, prims or mesh"), "{e}");

    // An import placed another way records it, and check reads the source the same way.
    let placed = dir.join("placed");
    let args = json!({"from": models.join("table.glb").to_str().unwrap(), "out": placed.to_str().unwrap(), "scale": 0.5, "place": "center"});
    let moved = tool(&mut s, args).unwrap();
    let text = std::fs::read_to_string(placed.join("table.json")).unwrap();
    assert!(text.contains(r#""norm":"scale=0.5 place=center""#), "{text}");
    let again = tool(&mut s, json!({"check": placed.join("table.json").to_str().unwrap()})).unwrap();
    assert_eq!(again["iou"], moved["iou"], "{again}");

    // A failed translation of one file is not written either.
    let cube_file = write(&models, "cube.glb", two_colour_cube());
    let fail = dir.join("fail");
    let e =
        tool(&mut s, json!({"from": cube_file.to_str().unwrap(), "out": fail.to_str().unwrap(), "fit": "prims"})).unwrap_err();
    assert!(e.to_string().contains("the translation fails") && e.to_string().contains("not written"), "{e}");
    assert!(!fail.join("cube.json").exists());

    // Ids that make the same name get distinct ones: none is written over.
    let clash = dir.join("clash");
    std::fs::create_dir_all(clash.join("a")).unwrap();
    for (i, f) in ["Foo.glb", "foo.glb", "a/b.glb", "a_b.glb"].iter().enumerate() {
        write(&clash, f, cube(0.2 + 0.1 * i as f32));
    }
    let clash_out = dir.join("clash_out");
    let all = tool(&mut s, json!({"from": clash.to_str().unwrap(), "out": clash_out.to_str().unwrap()})).unwrap();
    assert_eq!(all["written"], 4, "{all}");
    let titles: Vec<String> =
        ["foo", "foo_2", "a_b", "a_b_2"].iter().map(|n| title(&clash_out.join(format!("{n}.json")))).collect();
    assert_eq!(titles, ["foo", "Foo", "a_b", "a/b"]);

    // A folder inside a downloaded pack: only its models, under the pack's catalog.
    let catalogs = dir.join("catalogs");
    std::fs::create_dir_all(&catalogs).unwrap();
    let catalog = r#"{"$library": "testlib", "$sources": {"pack1": {"license": "CC0-1.0", "models": "models/**/*.glb", "pack": "test_pack1"}}}"#;
    write(&catalogs, "testlib.json", catalog);
    // SAFETY: only this test reads the variable.
    unsafe { std::env::set_var("PAV_CATALOGS", &catalogs) };
    let pack = dir.join("cache/assets/testlib/pack1/models");
    for (sub, size) in [("a", 0.2), ("a", 0.3), ("b", 0.4)] {
        std::fs::create_dir_all(pack.join(sub)).unwrap();
        write(&pack.join(sub), &format!("m{}.glb", (size * 10.0) as i32), cube(size));
    }
    let part_out = dir.join("part_out");
    let part = tool(&mut s, json!({"from": pack.join("a").to_str().unwrap(), "out": part_out.to_str().unwrap()})).unwrap();
    assert_eq!((part["written"].as_u64(), part["from"].as_str()), (Some(2), Some("testlib/pack1/models/a")), "{part}");
    assert_eq!(title(&part_out.join("a_m2.json")), "a/m2");
    let ledger = dir.join("part.tsv");
    let census = tool(&mut s, json!({"survey": pack.join("a").to_str().unwrap(), "out": ledger.to_str().unwrap(), "fit": false}));
    assert_eq!(census.unwrap()["models"], 2);
    // The pack's own ledger holds every model: part of the pack goes to out= only.
    let e = tool(&mut s, json!({"survey": pack.join("a/m2.glb").to_str().unwrap()})).unwrap_err();
    assert!(e.to_string().contains("part of testlib/pack1"), "{e}");
}
