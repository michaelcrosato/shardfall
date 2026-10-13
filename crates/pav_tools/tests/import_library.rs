//! The licence gate and catalog lines through `asset_import` (plan §8.7, §9.1): a pinned CC0 pack
//! translates into `lib/<pack>/` with its `PACK.json`, credits, index and the catalog's category
//! and tags, so `assets find=` ranks it; a catalog that keeps it local, or a share-alike licence,
//! sends it to `local/`; an unknown licence and a bad catalog line are refused.

mod common;

use std::path::Path;

use common::{MeshBuilder, cuboid, temp_dir};
use pav_tools::{Output, Session, asset_library, call};
use serde_json::{Value, json};

fn cube(size: f32) -> Vec<u8> {
    let mut b = MeshBuilder::new();
    let (p, _, i) = cuboid([-size / 2.0, 0.0, -size / 2.0], [size / 2.0, size, size / 2.0]);
    let m = b.colour([0.4, 0.25, 0.1]);
    let prim = b.tri_prim(&p, &i, Some(m));
    let mesh = b.mesh(vec![prim]);
    b.node(json!({"name": "model", "mesh": mesh}));
    b.glb(&[0])
}

fn run(s: &mut Session, tool: &str, args: Value) -> anyhow::Result<Value> {
    match call(s, tool, args.as_object().unwrap())? {
        Output::Json(v) => Ok(v),
        Output::Image { meta, .. } => Ok(meta),
    }
}

fn catalog(dir: &Path, source: &str, lines: &str) {
    let text = format!(
        r#"{{"$library": "testlib",
  "$sources": {{"pack1": {{"label": "Test Pack", "credit": "Tester", "url": "https://example.invalid/pack1.zip", "sha256": "5a5a",
    "licence_file": "LICENSE.txt", "licence_sha256": "7e7e", "models": "models/*.glb", "pack": "test_pack", {source}}}}},
  "$skip": {{"pack1/c": "not a prop"}},
  // one line per model
  {lines}}}"#
    );
    std::fs::write(dir.join("catalogs/testlib.json"), text).unwrap();
}

#[test]
fn packs_go_where_their_licence_says_with_their_catalog_lines() {
    let dir = temp_dir("import_library");
    let models = dir.join("cache/testlib/pack1/models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::create_dir_all(dir.join("catalogs")).unwrap();
    for (name, size) in [("a", 0.4), ("b", 0.6), ("c", 0.2)] {
        std::fs::write(models.join(format!("{name}.glb")), cube(size)).unwrap();
    }
    // What `get` records: the pin the pack was fetched under and the licence its text says.
    let fetched = dir.join("cache/testlib/pack1/SOURCE.json");
    let record = r#"{"sha256": "5a5a", "licence": "CC0-1.0", "licence_sha256": "7e7e", "pin": {"url": "https://example.invalid/pack1.zip",
      "sha256": "5a5a", "licence_file": "LICENSE.txt", "models": "models/*.glb"}}"#;
    std::fs::write(&fetched, record).unwrap();
    // SAFETY: the only test in this binary; nothing else reads the variables.
    unsafe {
        std::env::set_var("PAV_ASSET_CACHE", dir.join("cache"));
        std::env::set_var("PAV_CATALOGS", dir.join("catalogs"));
        std::env::set_var("PAV_ASSETS", dir.join("props"));
    }
    let lines = r#""pack1/a": ["container crate wood", ""], "pack1/b": ["furniture table wood", "A plain block."]"#;
    let mut s = Session::new("empty", 1).unwrap();
    let props = dir.join("props");

    // CC0: lib/, with its licence, credits and index; the catalog's skip is skipped. Only from
    // the pack the catalog pins: a cache fetched under another pin is refused.
    catalog(&dir, r#""license": "CC0-1.0""#, lines);
    let pin = std::fs::read_to_string(&fetched).unwrap();
    std::fs::write(&fetched, pin.replace("pack1.zip", "pack0.zip")).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("not the one the catalog pins: asset_import get=testlib/pack1"), "{e}");
    std::fs::write(&fetched, pin).unwrap();
    let r = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap();
    assert_eq!((r["to"].as_str(), r["written"].as_u64()), (Some("lib"), Some(2)), "{r}");
    assert!(r["rows"].to_string().contains("c skipped: not a prop"), "{r}");
    let lib = props.join("lib/test_pack");
    let pack = asset_library::read_pack(&lib).unwrap();
    assert_eq!((pack.license.as_str(), pack.credit.as_str()), ("CC0-1.0", "Tester"));
    let a = std::fs::read_to_string(lib.join("a.json")).unwrap();
    assert!(a.contains(r#""tags":["crate","wood"]"#) && a.contains(r#""category":"container""#), "{a}");
    assert!(std::fs::read_to_string(props.join("CREDITS.md")).unwrap().contains("## LIB/test_pack: Test Pack"));
    let found = run(&mut s, "assets", json!({"find": "crate wood"})).unwrap();
    assert!(found["rows"].as_array().unwrap().iter().any(|r| r[0] == "LIB/test_pack/a"), "{found}");
    // out= never writes into the library: the licence gate does.
    for into_lib in [props.join("lib/elsewhere"), props.join("local/../lib/elsewhere")] {
        let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1", "out": into_lib.to_str().unwrap()})).unwrap_err();
        assert!(e.to_string().contains("is inside the library"), "{e}");
    }
    // A pack read by ranges (no archive hash checked) is not committed.
    let unhashed = record.replacen(r#""sha256": "5a5a", "#, "", 1);
    std::fs::write(&fetched, &unhashed).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("get could not check the archive's SHA-256"), "{e}");
    std::fs::write(&fetched, record).unwrap();
    // A part of the pack may not change its details, and a renamed pack leaves no old folder.
    let pack_file = lib.join("PACK.json");
    let details = std::fs::read_to_string(&pack_file).unwrap();
    std::fs::write(&pack_file, details.replace("Tester", "Someone")).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": models.join("b.glb").to_str().unwrap()})).unwrap_err().to_string();
    assert!(e.contains("the pack's licence, credit or source changed"), "{e}");
    std::fs::write(&pack_file, details).unwrap();
    catalog(&dir, r#""license": "CC0-1.0", "pack": "test_pack2""#, lines);
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("holds this source under its old name"), "{e}");
    catalog(&dir, r#""license": "CC0-1.0""#, lines);
    // A model that cannot be read this time keeps its last translation.
    let a_glb = models.join("a.glb");
    std::fs::write(&a_glb, b"not a model").unwrap();
    let r = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap();
    assert!(r["written"] == 1 && lib.join("a.json").exists() && lib.join("b.json").exists(), "{r}");
    std::fs::write(&a_glb, cube(0.4)).unwrap();

    // Kept local by its catalog: the lib/ copy moves to local/, its notes kept.
    let index = lib.join("index.tsv");
    let noted = std::fs::read_to_string(&index).unwrap().replace("\t\tA plain block.", "\tseen\tA plain block.");
    std::fs::write(&index, noted).unwrap();
    catalog(&dir, r#""license": "CC0-1.0", "local": "pilot: too few editable""#, lines);
    std::fs::write(&a_glb, b"not a model").unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("a did not translate, and the pack changes folder"), "{e}");
    std::fs::write(&a_glb, cube(0.4)).unwrap();
    let r = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap();
    assert_eq!((r["to"].as_str(), r["why"].as_str()), (Some("local"), Some("pilot: too few editable")), "{r}");
    assert!(!lib.exists() && props.join("local/test_pack/b.json").exists());
    assert!(std::fs::read_to_string(props.join("local/test_pack/index.tsv")).unwrap().contains("\tseen\t"));
    assert!(!std::fs::read_to_string(props.join("CREDITS.md")).unwrap().contains("test_pack"));
    // Back to lib/ only whole: one model of it would sit behind the local copy.
    catalog(&dir, r#""license": "CC0-1.0""#, lines);
    let e = run(&mut s, "asset_import", json!({"from": models.join("b.glb").to_str().unwrap()})).unwrap_err().to_string();
    assert!(e.contains("translate the whole pack to move it"), "{e}");

    // Share-alike stays local; an unknown licence and a line without a category are refused.
    catalog(&dir, r#""license": "CC-BY-SA-4.0""#, lines);
    let r = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap();
    assert!(r["why"].as_str().unwrap().contains("CC-BY-SA-4.0"), "{r}");
    catalog(&dir, r#""license": "WTFPL""#, lines);
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("unknown licence WTFPL"), "{e}");
    catalog(&dir, r#""license": "CC0-1.0""#, r#""pack1/a": ["crate wood", ""]"#);
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("\"pack1/a\": the first word is a category"), "{e}");
    catalog(&dir, r#""license": "CC0-1.0""#, r#""pack1/a": "container crate wood""#);
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("\"pack1/a\": a model line is"), "{e}");
    // Two sources may not write one pack.
    let two = r#"{"$library": "testlib", "$sources": {
      "pack1": {"url": "https://example.invalid/pack1.zip", "models": "models/*.glb", "pack": "test_pack", "license": "CC0-1.0"},
      "pack2": {"url": "https://example.invalid/pack2.zip", "models": "models/*.glb", "pack": "test_pack", "license": "CC0-1.0"}}}"#;
    std::fs::write(dir.join("catalogs/testlib.json"), two).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("testlib/pack1 and testlib/pack2 both translate into the pack test_pack"), "{e}");
    std::fs::write(dir.join("catalogs/testlib.json"), two.replace(r#""pack": "test_pack""#, r#""pack": "import""#)).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("the pack import holds loose files"), "{e}");
    // A lib/ pack pins its bytes and licence text.
    let loose_pin = r#"{"$library": "testlib", "$sources": {"pack1": {"url": "https://example.invalid/pack1.zip",
      "sha256": null, "models": "models/*.glb", "pack": "test_pack", "license": "CC0-1.0"}}}"#;
    std::fs::write(dir.join("catalogs/testlib.json"), loose_pin).unwrap();
    let e = run(&mut s, "asset_import", json!({"from": "testlib/pack1"})).unwrap_err().to_string();
    assert!(e.contains("add sha256, licence_file, licence_sha256 to its source"), "{e}");

    // Loose files share local/import, and one import keeps the others.
    let loose = dir.join("loose");
    std::fs::create_dir_all(&loose).unwrap();
    for (name, size) in [("stool", 0.4), ("crate", 0.6)] {
        let file = loose.join(format!("{name}.glb"));
        std::fs::write(&file, cube(size)).unwrap();
        run(&mut s, "asset_import", json!({"from": file.to_str().unwrap()})).unwrap();
    }
    assert!(props.join("local/import/stool.json").exists() && props.join("local/import/crate.json").exists());
}
