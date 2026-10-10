use pav_tools::{Output, Session};
use serde_json::{Value, json};

fn command(s: &mut Session, args: Value) -> anyhow::Result<Value> {
    match pav_tools::preview_tools::t_anim_preview(s, args.as_object().unwrap())? {
        Output::Json(value) => Ok(value),
        _ => panic!("preview controls return data"),
    }
}

#[test]
fn the_agent_and_native_control_handler_preserves_camera_and_reads_joints() {
    let mut s = Session::new("empty", 7).unwrap();
    let camera = s.camera.params.clone();
    let original = s.sim.state_hash();
    command(&mut s, json!({"clip":"QUATERNIUS/Idle_Loop","time":0.5})).unwrap();
    s.camera.params.yaw = -42.0;
    let pose = command(&mut s, json!({"clip":"QUATERNIUS/Idle_Loop","action":"pose"})).unwrap();
    assert_eq!(pose["time"], json!(0.5));
    assert_eq!(pose["playing"], json!(false));
    assert_eq!(s.camera.params.yaw, -42.0);
    assert!(pose["key"].is_object());
    assert_eq!(pose["joints"]["handR"].as_array().unwrap().len(), 3);
    command(&mut s, json!({"close":true})).unwrap();
    assert_eq!(s.camera.params, camera);
    assert_eq!(s.sim.state_hash(), original);
}

#[test]
fn a_bad_open_does_not_leave_a_stage_or_camera_change() {
    let mut s = Session::new("empty", 2).unwrap();
    let camera = s.camera.params.clone();
    assert!(command(&mut s, json!({"clip":"QUATERNIUS/Idle_Loop","speed":-1})).is_err());
    assert!(s.sim.preview_info().is_none());
    assert_eq!(s.camera.params, camera);
    assert!(command(&mut s, json!({"clip":"not/a/clip"})).is_err());
    assert!(s.sim.preview_info().is_none());
}

#[test]
fn animation_stage_builds_through_the_normal_view_without_a_gpu() {
    let mut s = Session::new("empty", 3).unwrap();
    command(&mut s, json!({"clip":"QUATERNIUS/Idle_Loop","time":0.5})).unwrap();
    let frame = s.sim.frame();
    s.camera.snap(frame.focus);
    let mut builder = pav_view::ViewBuilder::new();
    // This is the CPU scene-building path used by both the window and headless captures.
    // It must handle the subject and scenery IDs without overflowing outline-group IDs.
    let scene = builder.build(&frame, &frame, 1.0, &s.camera, 16.0 / 9.0, &s.view_for(&frame), frame.focus);
    let groups: std::collections::HashSet<u32> = scene
        .meshes
        .iter()
        .map(|m| m.group)
        .chain(scene.sdfs.iter().map(|s| s.group))
        .chain(scene.dynamic.iter().map(|d| d.group))
        .collect();
    let subject = frame.player.unwrap();
    let floor = frame.objects.iter().find(|o| o.scenery).unwrap();
    assert!(groups.contains(&(subject.0 + 2)), "the subject reaches the normal renderer");
    assert!(groups.contains(&(floor.id.0 + 2)), "the stage floor reaches the normal renderer");
    assert!(s.gpu.is_none(), "scene building must not need a GPU device");
}
