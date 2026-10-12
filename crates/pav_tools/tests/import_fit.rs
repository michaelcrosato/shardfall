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
    let o = Options { mode, pack: "test_fit".into(), name: name.into(), source: Default::default() };
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
}
