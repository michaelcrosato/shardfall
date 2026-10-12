//! Format 2 through the tools: a document is read, checked and compiled whole, saved in the
//! lowest format that holds it, drawn and collided from its compiled parts, and undone like any
//! other edit, whole or by patch.

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
        "leg":{"type":"cylinder","radius":2,"from":[0,0,0],"to":[0,43,0],"mat":"wood","radial":{"count":3,"radius":12}},
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

    // A copy can carry a variant and a palette; the stage shows a variant too.
    let styled = json!({"action":"add","name":"WORKSHOP/stool","pos":[6,0,0],"seed":3,"roles":{"$wall":"#405060"}});
    let styled = call(&mut s, "asset_spawn", styled).unwrap();
    assert_eq!((styled["instance"]["seed"].clone(), styled["instance"]["roles"].clone()), (json!(3), json!({"$wall":"#405060"})));
    call(&mut s, "asset_spawn", json!({"action":"remove","id":styled["instance"]["id"]})).unwrap();
    let wrong = json!({"action":"add","name":"WORKSHOP/stool","pos":[9,0,0],"roles":{"$wal":"#405060"}});
    assert!(call(&mut s, "asset_spawn", wrong).unwrap_err().contains("did you mean \"$wall\"?"));
    let stage = call(&mut s, "asset_preview", json!({"name":"WORKSHOP/stool","seed":2})).unwrap();
    assert_eq!(stage["seed"], 2, "{stage}");
    call(&mut s, "asset_preview", json!({"close":true})).unwrap();

    // A replace refreshes the placed copy, and so does a patch.
    let mut taller = stool.clone();
    taller["name"] = json!("stool");
    taller["parts"]["seat"]["height"] = json!(6);
    let replaced = edit(&mut s, json!({"action":"replace","name":"stool","asset":taller,"preview":false})).unwrap();
    assert_eq!((replaced["changed_parts"].clone(), replaced["refreshed_instances"].clone()), (json!(["seat"]), json!(1)));
    let ops = json!([{"op":"set","part":"seat","fields":{"radius":20}},{"op":"rename","part":"ring","to":"hoop"}]);
    let patched = edit(&mut s, json!({"action":"patch","name":"stool","ops":ops,"preview":false})).unwrap();
    assert_eq!(
        (patched["changed_parts"].clone(), patched["refreshed_instances"].clone()),
        (json!(["hoop", "ring", "seat"]), json!(1))
    );
    assert_eq!(
        edit(&mut s, json!({"action":"patch","name":"stool","ops":[{"op":"set","part":"hoop","fields":{"thickness":-1}}]}))
            .unwrap_err(),
        "ops[0] hoop.thickness: -1 is outside 0.1 to 100000 cm"
    );

    // Resolved parts in the asset's unit: the legs by their ends, the seat by its place.
    let inspected = edit(&mut s, json!({"action":"inspect","name":"stool","resolved":true,"preview":false})).unwrap();
    assert_eq!(
        inspected["resolved"]["leg[1]"],
        json!({"type":"cylinder","size":[4,43,4],"from":[10.39,0,-6],"to":[10.39,43,-6]})
    );
    assert_eq!(inspected["resolved"]["seat"], json!({"type":"cylinder","size":[40,6,40],"pos":[0,45,0]}));
    let guide = edit(&mut s, json!({"action":"schema","format":2,"topic":"ops"})).unwrap();
    assert!(guide["ops"]["expand"].as_str().unwrap().contains("<id>_<i>"), "{guide}");

    // Undo and redo restore format-2 documents exactly.
    let unpatched = edit(&mut s, json!({"action":"undo","name":"stool","preview":false})).unwrap();
    assert_eq!(unpatched["revision"], replaced["revision"]);
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

    // A stool holding the pair that holds it is refused before its file or history changes.
    let files = || ["workshop/stool.json", ".editor/stool.json"].map(|file| std::fs::read(root.join(file)).unwrap());
    let (saved, revision) = (files(), pav_core::props::get("WORKSHOP/stool").unwrap().revision.clone());
    let mut looped = stool.clone();
    looped["name"] = json!("stool");
    looped["parts"]["pair"] = json!({"ref":"WORKSHOP/stools","pos":[0,100,0]});
    assert_eq!(
        edit(&mut s, json!({"action":"replace","name":"stool","asset":looped,"preview":false})).unwrap_err(),
        "WORKSHOP/stool would reference itself"
    );
    assert_eq!((files(), pav_core::props::get("WORKSHOP/stool").unwrap().revision.clone()), (saved, revision));

    // A collision-only blocker draws nothing, so it has no contact sheet to frame.
    let blocker = json!({"format":2,"unit":"cm","parts":{
        "wall":{"type":"box","size":[100,100,10],"pos":[0,50,0],"hidden":true,"color":"#26313e"}}});
    edit(&mut s, json!({"action":"create","name":"blocker","asset":blocker,"preview":false})).unwrap();
    assert_eq!(
        call(&mut s, "asset_preview", json!({"name":"WORKSHOP/blocker","views":true})).unwrap_err(),
        "name: WORKSHOP/blocker draws nothing: every part is hidden or left out at seed 0 — asset_edit action=inspect bounds=true measures the hidden parts"
    );

    // The stage frames the variant it shows, and an arc placed by its ends lists them on its axis.
    let posts = json!({"format":2,"unit":"cm","color":"#8a8f99","parts":{
        "base":{"type":"box","size":[40,4,40],"pos":[0,2,0]},
        "post":{"type":"box","size":[4,200,4],"pos":[0,104,0],"chance":0.5},
        "shell":{"type":"cylinder","radius":10,"arc":180,"from":[0,4,10],"to":[0,30,10]}}});
    edit(&mut s, json!({"action":"create","name":"posts","asset":posts,"preview":false})).unwrap();
    let def = pav_core::props::get("WORKSHOP/posts").unwrap();
    let height = |seed: u32| def.variant(seed).bounds_at(1.0).size().y;
    let seed = (1..64).find(|seed| height(*seed) != height(0)).unwrap();
    call(&mut s, "asset_preview", json!({"name":"WORKSHOP/posts","seed":seed})).unwrap();
    let focus = s.sim.state.prop_preview.as_ref().unwrap().focus;
    assert!((focus.y - height(seed) * 0.5).abs() < 1e-5, "{focus} frames seed {seed}, {} m tall", height(seed));
    call(&mut s, "asset_preview", json!({"close":true})).unwrap();
    let inspected = edit(&mut s, json!({"action":"inspect","name":"posts","resolved":true,"preview":false})).unwrap();
    let shell = &inspected["resolved"]["shell"];
    assert_eq!((&shell["from"], &shell["to"]), (&json!([0, 4, 10]), &json!([0, 30, 10])), "{shell}");

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

    // A room that places a misspelled asset reads, and room_check names the asset it meant.
    let source = pav_core::room::load_sources(None).into_iter().find(|room| room.key == "sandbox").unwrap();
    let object = "[[object]]\nname = \"seat\"\nasset = \"WORKSHOP/stoool\"\npos = [3.0, 0.0, 4.0]\nbody = \"fixed\"\nseed = 4\n";
    let room = root.join("stool_room.toml");
    std::fs::write(&room, format!("{}\n{object}", source.text)).unwrap();
    let checked = call(&mut s, "room_check", json!({"path": room})).unwrap();
    assert_eq!(checked["ok"], false, "{checked}");
    assert_eq!(checked["error"], "object 'seat': no prop 'WORKSHOP/stoool' — did you mean \"WORKSHOP/stool\"?");
    let _ = std::fs::remove_dir_all(&root);
}
