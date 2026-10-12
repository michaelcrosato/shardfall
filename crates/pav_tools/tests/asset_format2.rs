//! Format 2 through the tools before it compiles: documents are read and checked whole, a
//! document format 1 holds is saved as format 1, and the rest are refused without saving.

use pav_tools::Session;
use serde_json::{Value, json};

#[test]
fn format2_creates_are_checked_and_refused_until_they_build() {
    let root = std::env::temp_dir().join(format!("pav-asset-format2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // SAFETY: this test binary has one test, and it sets the variable before any tool runs.
    unsafe { std::env::set_var("PAV_ASSETS", &root) };
    let mut s = Session::new("empty", 1).unwrap();
    let mut create = |name: &str, asset: Value, dry_run: bool| {
        let args = json!({"action":"create","name":name,"asset":asset,"preview":false,"dry_run":dry_run});
        pav_tools::call(&mut s, "asset_edit", args.as_object().unwrap()).map(|_| ()).map_err(|error| format!("{error:#}"))
    };
    let stool = json!({"format":2,"unit":"cm","materials":{"wood":{"color":"#8a5a35"}},"parts":{
        "seat":{"type":"cylinder","radius":18,"height":4,"pos":[0,45,0],"mat":"wood"},
        "leg":{"type":"cylinder","radius":2,"from":[12,0,0],"to":[12,43,0],"mat":"wood","radial":{"count":3,"radius":0}}}});
    let refusal = r#"unit: unit "cm" needs format 2, which this engine checks but cannot build yet — write format 1 (asset_edit action=schema)"#;
    assert_eq!(create("stool", stool.clone(), false).unwrap_err(), refusal);
    assert_eq!(create("stool", stool.clone(), true).unwrap_err(), refusal);
    let mut broken = stool.clone();
    broken["parts"]["seat"]["mat"] = json!("wod");
    assert_eq!(
        create("stool", broken, false).unwrap_err(),
        r#"parts.seat.mat: no material "wod" (expected wood) — did you mean "wood"?"#
    );
    assert!(pav_core::props::get("WORKSHOP/stool").is_none());
    assert!(!root.join("workshop/stool.json").exists());

    // Metres and format 1's fields: saved, in format 1's canonical layout.
    let marker = json!({"format":2,"unit":"m","parts":{
        "post":{"type":"cylinder","radius":0.05,"half_height":0.6,"pos":[0,0.6,0],"color":"#26313e"}}});
    create("marker", marker, false).unwrap();
    let saved = std::fs::read_to_string(root.join("workshop/marker.json")).unwrap();
    assert_eq!(
        saved,
        "{\"format\":1,\"name\":\"marker\",\n\"parts\":{\n\"post\":{\"shape\":{\"type\":\"cylinder\",\"radius\":0.05,\"half_height\":0.6},\"pos\":[0,0.6,0],\"color\":\"#26313e\"}}}\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
