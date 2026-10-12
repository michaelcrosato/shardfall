//! `kind=material` on the shared asset tools: the listing, inspect and a swatch rendered
//! headless with its per-layer legibility lines.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, tool: &str, args: Value) -> anyhow::Result<Output> {
    pav_tools::call(s, tool, args.as_object().unwrap())
}

#[test]
fn material_swatches_render_and_say_how_each_layer_reads() {
    // The swatch goes to out/ under the test's working directory.
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all("out");
        }
    }
    let _cleanup = Cleanup;
    let mut s = Session::new("empty", 3).unwrap();
    let Output::Json(listed) = call(&mut s, "assets", json!({"kind": "material"})).unwrap() else { panic!() };
    assert!(listed["total"].as_u64().unwrap() >= 12, "{listed}");
    let Output::Json(inspected) = call(&mut s, "asset_edit", json!({"kind": "material", "name": "MAT/brick_wall"})).unwrap()
    else {
        panic!()
    };
    assert_eq!((inspected["layers"].clone(), inspected["cost"].clone()), (json!(2), json!("2 units")), "{inspected}");

    let camera = s.camera.params.clone();
    let args = json!({"kind": "material", "name": "MAT/oak_planks", "compare": "MAT/brick_wall,MAT/mossy_stone", "view": "game"});
    let Output::Image { png, meta, .. } = call(&mut s, "asset_preview", args).unwrap() else { panic!("a swatch is an image") };
    let reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (960, 540));
    assert_eq!(meta["path"], "out/swatch-oak_planks.png");
    let planks = meta["layers"]["MAT/oak_planks"][0].as_str().unwrap();
    assert!(planks.starts_with("planks 20 cm, 19 px at 1080p (9.5 at 540p), covers ") && planks.ends_with(": clear"), "{planks}");
    assert_eq!(meta["layers"]["MAT/mossy_stone"].as_array().unwrap().len(), 3, "{meta}");
    assert_eq!(s.camera.params.tilt, camera.tilt, "the session's camera is as it was");

    let error = |s: &mut Session, args: Value| format!("{:#}", call(s, "asset_preview", args).err().expect("an error"));
    assert_eq!(
        error(&mut s, json!({"kind": "material", "name": "MAT/oak_plank"})),
        r#"name: no material "MAT/oak_plank" — did you mean "MAT/oak_planks"?"#
    );
    let seven = "MAT/brick_wall,MAT/cobblestone,MAT/flagstones,MAT/mossy_stone,MAT/roof_shingles,MAT/pb/gold";
    assert_eq!(
        error(&mut s, json!({"kind": "material", "name": "MAT/oak_planks", "compare": seven})),
        "compare: 7 materials (expected at most 6 rows)"
    );
}
