use glam::Vec3;
use pav_core::prop_preview::StudioMode;
use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn preview(s: &mut Session, a: Value) -> anyhow::Result<Value> {
    match pav_tools::asset_preview_tools::t_asset_preview(s, a.as_object().unwrap())? {
        Output::Json(value) => Ok(value),
        _ => panic!("preview returns data"),
    }
}
fn spawn(s: &mut Session, a: Value) -> anyhow::Result<Value> {
    match pav_tools::asset_preview_tools::t_asset_spawn(s, a.as_object().unwrap())? {
        Output::Json(value) => Ok(value),
        _ => panic!("placement returns data"),
    }
}
fn animation(s: &mut Session, a: Value) {
    pav_tools::preview_tools::t_anim_preview(s, a.as_object().unwrap()).unwrap();
}

#[test]
fn tabs_preserve_world_animation_and_prop_cameras_and_controls() {
    let mut s = Session::new("empty", 3).unwrap();
    let world_camera = s.camera.params.clone();
    let world_hash = s.sim.state_hash();
    animation(&mut s, json!({"clip":"QUATERNIUS/Idle_Loop","time":0.4,"mirror":true}));
    s.camera.params.yaw = -42.0;
    preview(&mut s, json!({"name":"BUILTIN/bench","time":2.0,"turntable":true})).unwrap();
    s.camera.params.yaw = 111.0;
    animation(&mut s, json!({"action":"open"}));
    assert_eq!(s.camera.params.yaw, -42.0);
    assert_eq!(s.sim.preview_info().unwrap().time, 0.4);
    assert_eq!(preview(&mut s, json!({})).unwrap()["open"], false);
    assert_eq!(s.sim.studio_mode(), StudioMode::Animation, "status does not switch tabs");
    preview(&mut s, json!({"action":"open"})).unwrap();
    assert_eq!(s.camera.params.yaw, 111.0);
    assert_eq!(s.sim.prop_preview_info().unwrap().time, 2.0);
    preview(&mut s, json!({"close":true})).unwrap();
    assert_eq!(s.camera.params, world_camera);
    assert_eq!(s.sim.state_hash(), world_hash);
    assert!(s.sim.state.animation_preview.is_some() && s.sim.state.prop_preview.is_some());
}

#[test]
fn paused_props_render_parts_through_the_cpu_adapter_and_ticket_skips_old_transforms() {
    let mut s = Session::new("empty", 5).unwrap();
    preview(&mut s, json!({"name":"BUILTIN/bench","playing":false})).unwrap();
    let frame = s.sim.frame();
    let subject = frame.objects.iter().find(|o| o.prop.is_some()).unwrap();
    let expected = subject.prop.as_ref().unwrap().definition.compiled.parts.len();
    let mut builder = pav_view::ViewBuilder::new();
    let scene = builder.build(&frame, &frame, 1.0, &s.camera, 16.0 / 9.0, &s.view_for(&frame), frame.focus);
    let parts = scene.meshes.iter().filter(|p| p.group == subject.id.0 + 2).count()
        + scene.sdfs.iter().filter(|p| p.group == subject.id.0 + 2).count();
    assert_eq!(parts, expected);
    assert!(s.gpu.is_none());
    let mut revised = frame.clone();
    revised.live_edit_ticket += 1;
    revised.objects.iter_mut().find(|o| o.prop.is_some()).unwrap().pos += Vec3::X;
    let shown = pav_view::build::interpolate(&frame, &revised, 0.0);
    assert_eq!(
        shown.iter().find(|o| o.prop.is_some()).unwrap().pos,
        subject.pos + Vec3::X,
        "an acknowledged ticket includes the complete edit"
    );
}

#[test]
fn placement_validation_is_atomic_and_only_prop_roots_can_be_updated_or_removed() {
    let mut s = Session::new("empty", 7).unwrap();
    let hash = s.sim.state_hash();
    assert!(spawn(&mut s, json!({"action":"add","name":"BUILTIN/bench"})).is_err());
    assert!(spawn(&mut s, json!({"action":"add","name":"BUILTIN/bench","pos":[1,"bad",3]})).is_err());
    assert!(spawn(&mut s, json!({"action":"add","name":"BUILTIN/bench","pos":[1,0,3],"colour":"#ffffff"})).is_err());
    assert!(preview(&mut s, json!({"name":"BUILTIN/bench","scale":0})).is_err());
    assert!(preview(&mut s, json!({"name":"BUILTIN/bench","colour":"#ffffff"})).is_err());
    assert_eq!(s.sim.state_hash(), hash);
    assert!(!s.sim.studio_active());
    let made = spawn(
        &mut s,
        json!({"action":"add","name":"BUILTIN/bench","pos":[1,0,3],"yaw":45,"scale":1.5,"scene":"empty","seed":7,"ticks":0}),
    )
    .unwrap();
    let id = made["instance"]["id"].as_u64().unwrap();
    let updated = spawn(&mut s, json!({"action":"update","id":id,"pos":[2,0,4],"collide":false})).unwrap();
    assert_eq!(updated["instance"]["id"], id);
    assert_eq!(updated["instance"]["scale"], 1.5);
    assert_eq!(updated["instance"]["collide"], false);
    assert_eq!(spawn(&mut s, json!({"action":"list"})).unwrap()["instances"].as_array().unwrap().len(), 1);
    spawn(&mut s, json!({"action":"remove","id":id})).unwrap();
    let character = s.sim.spawn_character("test", Vec3::ZERO);
    assert!(spawn(&mut s, json!({"action":"remove","id":character.0})).is_err());
    assert!(spawn(&mut s, json!({"action":"update","id":character.0,"scale":2})).is_err());
}

fn image(s: &mut Session, tool: &str, a: Value) -> (Vec<u8>, Value) {
    match pav_tools::call(s, tool, a.as_object().unwrap()).unwrap() {
        Output::Image { png, meta, .. } => {
            let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
            let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut rgba).unwrap();
            assert_eq!(reader.info().color_type, png::ColorType::Rgba);
            (rgba, meta)
        }
        Output::Json(value) => panic!("{tool} returned data: {value}"),
    }
}

/// Unlit magenta parts, so a tile's subject pixels are easy to find.
fn magenta(name: &str, parts: Value) {
    let mut doc = json!({"format": 1, "name": name, "description": "framing test", "parts": parts});
    for part in doc["parts"].as_object_mut().unwrap().values_mut() {
        part["color"] = json!("#ff00ff");
        part["look"] = json!("unlit");
    }
    let def = pav_core::props::PropDef::new(pav_core::props::Doc::read(&doc.to_string()).unwrap()).unwrap();
    pav_core::props::install(&format!("WORKSHOP/{name}"), def).unwrap();
}

/// The share of a square tile that the subject's larger side spans.
fn fill(rgba: &[u8], width: usize, tile: usize, column: usize) -> f32 {
    let (mut lo, mut hi) = ([usize::MAX; 2], [0; 2]);
    for y in 0..tile {
        for x in 0..tile {
            let i = (y * width + column * tile + x) * 4;
            let (r, g, b) = (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32);
            if r > g + 80 && b > g + 80 {
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x), hi[1].max(y)];
            }
        }
    }
    assert!(lo[0] <= hi[0], "tile {column} shows no subject");
    (hi[0] - lo[0] + 1).max(hi[1] - lo[1] + 1) as f32 / tile as f32
}

#[test]
fn contact_sheets_frame_every_side_and_leave_the_stage_and_camera_as_they_were() {
    // Sheets go to out/ under the test's working directory; remove it even if an assertion fails.
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all("out");
        }
    }
    let _cleanup = Cleanup;
    let mut s = Session::new("empty", 9).unwrap();
    magenta("sheet_wide", json!({"top": {"shape": {"type": "box", "half": [0.6, 0.15, 0.25]}, "pos": [0, 0.15, 0]}}));
    magenta("sheet_tall", json!({"pole": {"shape": {"type": "cylinder", "half_height": 1.0, "radius": 0.08}, "pos": [0, 1, 0]}}));
    magenta("sheet_ball", json!({"ball": {"shape": {"type": "sphere", "radius": 0.3}, "pos": [0, 0.3, 0]}}));
    magenta("sheet_rug", json!({"rug": {"shape": {"type": "box", "half": [1.0, 0.005, 0.7]}, "pos": [0, 0.005, 0]}}));
    magenta(
        "sheet_bead",
        json!({"bead": {"shape": {"type": "capsule", "half_height": 0.01, "radius": 0.008}, "pos": [0, 0.018, 0]}}),
    );
    preview(&mut s, json!({"name":"BUILTIN/bench","time":2.0,"turntable":true})).unwrap();
    s.camera.params.yaw = 111.0;
    let (camera, stage, mode) = (s.camera.params.clone(), s.sim.prop_preview_info().unwrap(), s.sim.studio_mode());
    let world = s.sim.state_hash();
    for name in ["sheet_wide", "sheet_tall", "sheet_ball", "sheet_rug", "sheet_bead"] {
        let views = json!(["front", "right", "back", "iso", "top", 30]);
        let (rgba, meta) = image(&mut s, "asset_preview", json!({"name": name, "views": views, "tile": 160}));
        assert_eq!((meta["width"].as_u64(), meta["height"].as_u64()), (Some(640), Some(320)), "{name}: {meta}");
        for tile in 0..6 {
            let (column, row) = (tile % 4, tile / 4);
            let share = fill(&rgba[row * 160 * 640 * 4..], 640, 160, column);
            assert!((0.55..=0.9).contains(&share), "{name} view {}: the subject spans {share:.2} of its tile", views[tile]);
        }
    }
    assert_eq!(s.camera.params, camera, "the human's camera is back");
    assert_eq!(s.sim.studio_mode(), mode);
    let after = s.sim.prop_preview_info().unwrap();
    assert_eq!((after.name, after.time, after.turntable), (stage.name, stage.time, stage.turntable));
    assert_eq!(s.sim.state_hash(), world, "a sheet changes nothing");

    let (_, meta) = image(&mut s, "asset_preview", json!({"name": "BUILTIN/bench", "views": true, "figure": true}));
    assert_eq!(
        meta,
        json!({"path": "out/sheet-bench.png", "width": 960, "height": 240, "views": ["front", "right", "back", "iso"], "figure": true})
    );
    let (_, meta) = image(&mut s, "asset_preview", json!({"name": "sheet_bead", "views": "front,90", "figure": true}));
    assert_eq!(meta["views"], json!(["front", 90]));
    assert!(meta["figure"].as_str().is_some_and(|note| note.contains("0.5-20 m")), "{meta}");
    let started = std::time::Instant::now();
    image(&mut s, "asset_preview", json!({"name": "BUILTIN/bench", "views": true}));
    assert!(started.elapsed().as_secs_f32() < 1.0, "a warm 4-tile sheet took {:?}", started.elapsed());
}

#[test]
fn render_stats_count_the_triangles_the_cost_line_predicts() {
    let mut s = Session::new("empty", 11).unwrap();
    preview(&mut s, json!({"name":"BUILTIN/bench"})).unwrap();
    let checked =
        match pav_tools::call(&mut s, "asset_edit", json!({"action":"check","name":"BUILTIN/bench"}).as_object().unwrap()) {
            Ok(Output::Json(value)) => value,
            _ => panic!("check returns data"),
        };
    let asset: usize = checked["cost"].as_str().unwrap().split(' ').next().unwrap().parse().unwrap();
    let stats = match pav_tools::call(&mut s, "capture", json!({"width":64,"height":64,"stats":"only"}).as_object().unwrap()) {
        Ok(Output::Json(value)) => value,
        _ => panic!("stats=only returns data"),
    };
    // The stage: a floor, 34 grid lines and 2 axes, all boxes.
    let cube = pav_render::mesh::MeshKey::Cube.generate().unwrap().indices.len() / 3;
    assert_eq!(stats["triangles"], json!(asset + 37 * cube), "{stats}");
    assert_eq!(stats["mesh_instances"], json!(37 + 7));
    assert_eq!(stats["sdf"], json!(0));
}
