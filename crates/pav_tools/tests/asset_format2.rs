//! Format 2 through the tools: a document is read, checked and compiled whole, saved in the
//! lowest format that holds it, drawn and collided from its compiled parts, and undone like any
//! other edit. Patch ops cover format 1 only until row 2.4.

use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, tool: &str, args: Value) -> Result<Value, String> {
    match pav_tools::call(s, tool, args.as_object().unwrap()) {
        Ok(Output::Json(value)) => Ok(value),
        Ok(Output::Image { meta, .. }) => Ok(meta),
        Err(error) => Err(format!("{error:#}")),
    }
}

#[test]
fn format2_assets_are_created_drawn_placed_and_undone() {
    let root = std::env::temp_dir().join(format!("pav-asset-format2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // SAFETY: this test binary has one test, and it sets the variable before any tool runs.
    unsafe { std::env::set_var("PAV_ASSETS", &root) };
    let mut s = Session::new("empty", 1).unwrap();
    let edit = |s: &mut Session, args: Value| call(s, "asset_edit", args);
    let stool = json!({"format":2,"unit":"cm","materials":{"wood":{"color":"#8a5a35"}},"parts":{
        "seat":{"type":"cylinder","radius":18,"height":4,"pos":[0,45,0],"mat":"wood"},
        "leg":{"type":"cylinder","radius":2,"from":[12,0,0],"to":[12,43,0],"mat":"wood","radial":{"count":3,"radius":0}},
        "ring":{"type":"tube","radius":13,"thickness":1.5,"height":2,"pos":[0,15,0],"mat":"wood"}}});

    // A dry run predicts the save and writes nothing.
    let dry = edit(&mut s, json!({"action":"create","name":"stool","asset":stool,"preview":false,"dry_run":true})).unwrap();
    assert!(dry["revision_next"].as_str().unwrap().starts_with("prop2-"), "{dry}");
    assert_eq!(dry["parts"], 5, "the seat, three legs and the ring");
    assert!(!root.join("workshop/stool.json").exists() && pav_core::props::get("WORKSHOP/stool").is_none());

    // Saved in format 2's canonical layout, clean, with the generated ring in the cost line.
    let created = edit(&mut s, json!({"action":"create","name":"stool","asset":stool,"preview":false})).unwrap();
    assert_eq!(created["revision"], dry["revision_next"]);
    assert!(created.get("check").is_none(), "{created}");
    let saved = std::fs::read_to_string(root.join("workshop/stool.json")).unwrap();
    assert!(saved.starts_with("{\"format\":2,\"name\":\"stool\",\"unit\":\"cm\",\n"), "{saved}");
    let checked = edit(&mut s, json!({"action":"check","name":"stool"})).unwrap();
    assert!(checked.get("check").is_none(), "{checked}");
    let cost = checked["cost"].as_str().unwrap();
    assert!(cost.contains(" 2 meshes, 0 sdf, 0 lights"), "a cylinder and the ring's generated mesh: {cost}");

    // The renderer draws what the cost line counts, the ring as an uploaded generated mesh.
    call(&mut s, "asset_preview", json!({"name":"WORKSHOP/stool"})).unwrap();
    let stats = call(&mut s, "capture", json!({"width":64,"height":64,"stats":"only"})).unwrap();
    let cube = pav_render::mesh::MeshKey::Cube.generate().unwrap().indices.len() / 3;
    let triangles: usize = cost.split(' ').next().unwrap().parse().unwrap();
    assert_eq!(stats["triangles"], json!(triangles + 37 * cube), "{stats}");
    assert_eq!((stats["mesh_instances"].clone(), stats["custom_meshes"].clone()), (json!(37 + 5), json!(1)), "{stats}");

    // A placed copy collides with every solid part: a tube is walled with 12 boxes.
    let placed = call(&mut s, "asset_spawn", json!({"action":"add","name":"WORKSHOP/stool","pos":[3,0,0]})).unwrap();
    let id = pav_core::EntityId(placed["instance"]["id"].as_u64().unwrap() as u32);
    let shapes = |s: &Session| {
        let body = s.sim.state.entities.get(id).unwrap().body.unwrap();
        let collider = s.sim.state.physics.bodies.get(body).unwrap().colliders()[0];
        s.sim.state.physics.colliders.get(collider).unwrap().shape().as_compound().unwrap().shapes().len()
    };
    assert_eq!(shapes(&s), 1 + 3 + 12);

    // A replace refreshes the placed copy; patch ops wait for format 2 (row 2.4).
    let mut taller = stool.clone();
    taller["name"] = json!("stool");
    taller["parts"]["seat"]["height"] = json!(6);
    let replaced = edit(&mut s, json!({"action":"replace","name":"stool","asset":taller,"preview":false})).unwrap();
    assert_eq!((replaced["changed_parts"].clone(), replaced["refreshed_instances"].clone()), (json!(["seat"]), json!(1)));
    assert_eq!(
        edit(&mut s, json!({"action":"patch","name":"stool","ops":[{"op":"set","part":"seat","fields":{"radius":20}}]}))
            .unwrap_err(),
        "ops: patch ops edit format-1 assets; this one is format 2 — send the whole document with action=replace"
    );

    // Undo and redo restore format-2 documents exactly.
    let undone = edit(&mut s, json!({"action":"undo","name":"stool","preview":false})).unwrap();
    assert_eq!(undone["revision"], created["revision"]);
    assert_eq!(std::fs::read_to_string(root.join("workshop/stool.json")).unwrap(), saved);
    let removed = edit(&mut s, json!({"action":"undo","name":"stool","preview":false})).unwrap();
    assert_eq!(removed["revision"], "absent");
    let redone = edit(&mut s, json!({"action":"redo","name":"stool","if_revision":"absent","preview":false})).unwrap();
    assert_eq!(redone["revision"], created["revision"]);

    // References: a pair of stools follows the stool's edits, placed copies included, and the
    // stool cannot be undone away while the pair uses it.
    let pair = json!({"format":2,"unit":"cm","parts":{
        "a":{"ref":"WORKSHOP/stool","pos":[-30,0,0]},"b":{"ref":"WORKSHOP/stool","pos":[30,0,0],"yaw":90}}});
    assert_eq!(edit(&mut s, json!({"action":"create","name":"stools","asset":pair,"preview":false})).unwrap()["parts"], 10);
    call(&mut s, "asset_spawn", json!({"action":"add","name":"WORKSHOP/stools","pos":[-3,0,0]})).unwrap();
    let mut taller = taller.clone();
    taller["parts"]["seat"]["height"] = json!(8);
    let replaced = edit(&mut s, json!({"action":"replace","name":"stool","asset":taller,"preview":false})).unwrap();
    assert_eq!(replaced["refreshed_instances"], 2, "the stool and the pair of stools");
    let seat = |name: &str, id: &str| {
        let def = pav_core::props::get(name).unwrap();
        def.compiled.parts.iter().find(|p| p.id == id).unwrap().bounds(1.0).size().y
    };
    assert!((seat("WORKSHOP/stools", "a.seat") - 0.08).abs() < 1e-5);
    edit(&mut s, json!({"action":"undo","name":"stool","preview":false})).unwrap();
    assert_eq!(
        edit(&mut s, json!({"action":"undo","name":"stool","preview":false})).unwrap_err(),
        "WORKSHOP/stool is referenced by WORKSHOP/stools; change or remove those first"
    );
    assert!(root.join("workshop/stool.json").exists() && pav_core::props::get("WORKSHOP/stool").is_some());

    // A broken document reports its problems and saves nothing.
    let mut broken = stool.clone();
    broken["parts"]["seat"]["mat"] = json!("wod");
    assert_eq!(
        edit(&mut s, json!({"action":"create","name":"stump","asset":broken,"preview":false})).unwrap_err(),
        r#"parts.seat.mat: no material "wod" (expected wood) — did you mean "wood"?"#
    );
    assert!(pav_core::props::get("WORKSHOP/stump").is_none() && !root.join("workshop/stump.json").exists());

    // Metres and format 1's fields: saved, in format 1's canonical layout.
    let marker = json!({"format":2,"unit":"m","parts":{
        "post":{"type":"cylinder","radius":0.05,"half_height":0.6,"pos":[0,0.6,0],"color":"#26313e"}}});
    edit(&mut s, json!({"action":"create","name":"marker","asset":marker,"preview":false})).unwrap();
    let saved = std::fs::read_to_string(root.join("workshop/marker.json")).unwrap();
    assert_eq!(
        saved,
        "{\"format\":1,\"name\":\"marker\",\n\"parts\":{\n\"post\":{\"shape\":{\"type\":\"cylinder\",\"radius\":0.05,\"half_height\":0.6},\"pos\":[0,0.6,0],\"color\":\"#26313e\"}}}\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}
