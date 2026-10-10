//! Prop roots must survive snapshots, edits and streaming without becoming loose parts.

use std::sync::Arc;

use glam::{Quat, Vec3};
use pav_core::animation_preview::PreviewChange;
use pav_core::prop_preview::{PropPreviewChange, StudioMode};
use pav_core::props;
use pav_core::{EntityId, InputFrame, Sim};

fn compound_parts(sim: &Sim, id: EntityId) -> usize {
    let body = sim.state.entities.get(id).unwrap().body.unwrap();
    let collider = sim.state.physics.bodies.get(body).unwrap().colliders()[0];
    let collider = sim.state.physics.colliders.get(collider).unwrap();
    assert_eq!(pav_core::physics::entity_from_tag(collider.user_data), Some(id.0));
    collider.shape().as_compound().expect("parts share a compound collider").shapes().len()
}

#[test]
fn one_root_embeds_its_revision_and_refresh_preserves_transform() {
    let name = "WORKSHOP/runtime_snapshot_bench";
    let mut original = (*props::get("BUILTIN/bench").unwrap()).clone();
    original.name = "runtime_snapshot_bench".into();
    let original = Arc::new(original);
    props::install(name, original.clone()).unwrap();
    let mut sim = Sim::empty(5);
    let pos = Vec3::new(4.0, 0.0, -3.0);
    let rot = Quat::from_rotation_y(0.7);
    let id = sim.spawn_prop(name, pos, rot, 1.7, true, None).unwrap();
    let body = sim.state.entities.get(id).unwrap().body;
    assert_eq!(sim.state.entities.len(), 1);
    assert_eq!(compound_parts(&sim, id), original.parts.len());
    let mut snapshot = Vec::new();
    ciborium::into_writer(&sim.snapshot(), &mut snapshot).unwrap();

    let mut revised = (*original).clone();
    revised.parts.remove("crossbar");
    revised.parts.get_mut("seat").unwrap().pos.y = 1.2;
    let revised = Arc::new(revised);
    props::install(name, revised.clone()).unwrap();
    assert_eq!(
        sim.frame().objects[0].prop.as_ref().unwrap().definition.revision(),
        original.revision(),
        "publishing an asset cannot silently alter a snapshot instance"
    );
    assert_eq!(sim.refresh_prop_instances(name, revised.clone()), 1);
    assert_eq!(sim.state.entities.get(id).unwrap().body, body, "refresh preserves attached joints and physics handles");
    assert_eq!(sim.state.entities.get(id).unwrap().pos, pos);
    assert_eq!(sim.state.entities.get(id).unwrap().rot, rot);
    assert_eq!(compound_parts(&sim, id), revised.parts.len());
    sim.update_prop_instance(id, pos, rot, 2.0, false).unwrap();
    assert!(sim.state.entities.get(id).unwrap().body.is_none());
    sim.update_prop_instance(id, pos, rot, 2.0, true).unwrap();
    assert_eq!(compound_parts(&sim, id), revised.parts.len());

    sim.restore(ciborium::from_reader(snapshot.as_slice()).unwrap());
    assert_eq!(sim.frame().objects[0].prop.as_ref().unwrap().definition.revision(), original.revision());
    assert_eq!(compound_parts(&sim, id), original.parts.len());
    props::remove(name).unwrap();
    assert_eq!(sim.frame().objects.len(), 1, "a missing library asset does not erase an embedded instance");
}

#[test]
fn room_reference_roundtrip_and_sleeping_refresh_rebuild_the_compound() {
    let source = pav_core::room::load_sources(None).into_iter().find(|r| r.key == "sandbox").unwrap();
    let object: pav_core::room::ObjectDef = toml::from_str(
        r#"
name = "studio_bench"
asset = "BUILTIN/bench"
scale = 1.25
pos = [3.0, 0.0, 4.0]
yaw = 35.0
pitch = 10.0
roll = -3.0
body = "fixed"
"#,
    )
    .unwrap();
    let text = pav_core::world::rewrite_room_objects(&source.text, &[object]);
    let room = pav_core::room::RoomDef::parse(&text).unwrap();
    assert_eq!(room.objects[0].asset.as_deref(), Some("BUILTIN/bench"));
    assert_eq!(room.objects[0].scale, 1.25);
    let mut sim = Sim::empty(11);
    sim.build_world(vec![("prop_room".into(), room)], Vec::new());
    sim.teleport_to_room("prop_room");
    let id = sim.state.entities.find("studio_bench").unwrap().id;
    let pos = sim.state.entities.get(id).unwrap().pos;
    let parts = compound_parts(&sim, id);
    let room_id = sim.state.world.room("prop_room").unwrap().id;
    let saved = sim.room_objects(room_id);
    assert_eq!(saved[0].asset.as_deref(), Some("BUILTIN/bench"));
    assert!((saved[0].pos - Vec3::new(3.0, 0.0, 4.0)).length() < 0.002);
    assert!((saved[0].yaw - 35.0).abs() < 0.1);
    assert!((saved[0].pitch - 10.0).abs() < 0.1);

    sim.set_position(sim.state.player.unwrap(), Vec3::new(400.0, 30.0, 400.0));
    sim.update_streaming(usize::MAX);
    assert!(sim.state.entities.get(id).is_none(), "the room goes dormant");
    let mut revised = (*props::get("BUILTIN/bench").unwrap()).clone();
    revised.parts.remove("crossbar");
    assert_eq!(sim.refresh_prop_instances("BUILTIN/bench", Arc::new(revised)), 1);
    sim.teleport_to_room("prop_room");
    assert_eq!(sim.state.entities.get(id).unwrap().pos, pos);
    assert_eq!(compound_parts(&sim, id), parts - 1);
}

#[test]
fn studio_modes_retain_both_clocks_and_infer_legacy_animation_snapshots() {
    let mut sim = Sim::empty(19);
    let before = sim.state_hash();
    sim.preview_open(Some("QUATERNIUS/Idle_Loop"), None).unwrap();
    sim.preview_change(&PreviewChange { time: Some(0.4), mirror: Some(true), ..Default::default() }).unwrap();
    sim.prop_preview_open(Some("BUILTIN/bench")).unwrap();
    sim.prop_preview_change(&PropPreviewChange { turntable: Some(true), ..Default::default() }).unwrap();
    sim.run(60, &InputFrame::default());
    assert_eq!(sim.state_hash(), before);
    assert_eq!(sim.state.animation_preview.as_ref().unwrap().time, 0.4);
    assert!((sim.prop_preview_info().unwrap().time - 1.0).abs() < 0.001);
    sim.live_edit_ticket = 27;
    assert_eq!(sim.frame().live_edit_ticket, 27);
    sim.prop_preview_close();
    assert_eq!(sim.studio_mode(), StudioMode::World);
    assert_eq!(sim.frame().live_edit_ticket, 27);
    sim.preview_open(None, None).unwrap();
    assert_eq!(sim.preview_info().unwrap().time, 0.4);
    assert!(sim.preview_info().unwrap().mirror);
    assert_eq!(sim.frame().live_edit_ticket, 27);
    let mut legacy = sim.snapshot();
    legacy.studio_mode = None;
    sim.restore(legacy);
    assert_eq!(sim.studio_mode(), StudioMode::Animation);
    assert_eq!(sim.preview_info().unwrap().time, 0.4);
}
