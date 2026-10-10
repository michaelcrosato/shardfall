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
    let expected = subject.prop.as_ref().unwrap().definition.parts.len();
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
