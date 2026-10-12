//! Prop roots must survive snapshots, edits and streaming without becoming loose parts.

use std::sync::Arc;

use glam::{Quat, Vec3};
use pav_core::animation_preview::PreviewChange;
use pav_core::prop_instance::PropInstance;
use pav_core::prop_preview::{PropPreviewChange, StudioMode};
use pav_core::props::{self, Doc, PropAsset, PropDef};
use pav_core::{EntityId, InputFrame, Sim};
use rapier::prelude::{ColliderBuilder, Pose};

fn bench() -> PropAsset {
    (**props::get("BUILTIN/bench").unwrap().v1.as_ref().unwrap()).clone()
}

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
    let mut original = bench();
    original.name = "runtime_snapshot_bench".into();
    let original = Arc::new(original);
    props::install(name, PropDef::from_v1(original.clone()).unwrap()).unwrap();
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
    props::install(name, PropDef::from_v1(revised.clone()).unwrap()).unwrap();
    assert_eq!(
        sim.frame().objects[0].prop.as_ref().unwrap().definition.revision,
        original.revision(),
        "publishing an asset cannot silently alter a snapshot instance"
    );
    assert_eq!(sim.refresh_prop_instances(name, props::get(name).unwrap()), 1);
    assert_eq!(sim.state.entities.get(id).unwrap().body, body, "refresh preserves attached joints and physics handles");
    assert_eq!(sim.state.entities.get(id).unwrap().pos, pos);
    assert_eq!(sim.state.entities.get(id).unwrap().rot, rot);
    assert_eq!(compound_parts(&sim, id), revised.parts.len());
    sim.update_prop_instance(id, pos, rot, 2.0, false).unwrap();
    assert!(sim.state.entities.get(id).unwrap().body.is_none());
    sim.update_prop_instance(id, pos, rot, 2.0, true).unwrap();
    assert_eq!(compound_parts(&sim, id), revised.parts.len());

    sim.restore(ciborium::from_reader(snapshot.as_slice()).unwrap());
    assert_eq!(sim.frame().objects[0].prop.as_ref().unwrap().definition.revision, original.revision());
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
    let mut revised = bench();
    revised.parts.remove("crossbar");
    assert_eq!(sim.refresh_prop_instances("BUILTIN/bench", Arc::new(PropDef::from_v1(Arc::new(revised)).unwrap())), 1);
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

/// A compound's shapes, each as its pose and its exact parameters.
fn shapes(builder: ColliderBuilder) -> Vec<String> {
    let collider = builder.build();
    let compound = collider.shape().as_compound().expect("a compound");
    compound
        .shapes()
        .iter()
        .map(|(pose, shape)| {
            let params = if let Some(b) = shape.as_cuboid() {
                format!("cuboid {:?}", b.half_extents)
            } else if let Some(b) = shape.as_round_cuboid() {
                format!("round cuboid {:?} {:?}", b.inner_shape.half_extents, b.border_radius)
            } else if let Some(b) = shape.as_ball() {
                format!("ball {:?}", b.radius)
            } else if let Some(c) = shape.as_capsule() {
                format!("capsule {:?} {:?}", c.segment, c.radius)
            } else if let Some(c) = shape.as_cylinder() {
                format!("cylinder {:?} {:?}", c.half_height, c.radius)
            } else if let Some(c) = shape.as_cone() {
                format!("cone {:?} {:?}", c.half_height, c.radius)
            } else if let Some(h) = shape.as_convex_polyhedron() {
                format!("hull of {} points", h.points().len())
            } else {
                format!("{:?}", shape.shape_type())
            };
            format!("{pose:?} {params}")
        })
        .collect()
}

#[test]
fn format1_compounds_are_what_format1_built() {
    let mut assets: Vec<(String, PropAsset)> = ["box", "crate", "bench", "lantern"]
        .map(|name| (format!("BUILTIN/{name}"), (**props::get(&format!("BUILTIN/{name}")).unwrap().v1.as_ref().unwrap()).clone()))
        .into();
    for text in [include_str!("fixtures/props/dogfood_stall_v1.json"), include_str!("fixtures/props/dogfood_well_v1.json")] {
        let asset = PropAsset::parse(text).unwrap();
        assets.push((format!("WORKSHOP/{}", asset.name), asset));
    }
    for (name, asset) in assets {
        let def = Arc::new(PropDef::from_v1(Arc::new(asset.clone())).unwrap());
        for scale in [1.0, 0.37, 2.5] {
            let instance = PropInstance::new(&name, def.clone(), scale, true).unwrap();
            let format1 = asset
                .parts
                .values()
                .filter(|part| part.solid)
                .map(|part| {
                    let shape = part.visual(scale).shape.collider().build().shared_shape().clone();
                    (Pose::from_parts(part.pos * scale, part.rotation()), shape)
                })
                .collect();
            assert_eq!(shapes(instance.collider().unwrap()), shapes(ColliderBuilder::compound(format1)), "{name} at {scale}");
        }
    }
}

const FIXTURES: [(&str, &str); 8] = [
    ("bench", include_str!("fixtures/props/format2/bench.json")),
    ("greatsword", include_str!("fixtures/props/format2/greatsword.json")),
    ("market_stall", include_str!("fixtures/props/format2/market_stall.json")),
    ("wall_torch", include_str!("fixtures/props/format2/wall_torch.json")),
    ("well", include_str!("fixtures/props/format2/well.json")),
    ("probe_attach", include_str!("fixtures/props/format2/probe_attach.json")),
    ("probe_frames", include_str!("fixtures/props/format2/probe_frames.json")),
    ("probe_mirror", include_str!("fixtures/props/format2/probe_mirror.json")),
];

#[test]
fn format2_fixtures_place_collide_and_survive_snapshots() {
    let mut sim = Sim::empty(23);
    let (mut placed, mut counts) = (Vec::new(), Vec::new());
    for (i, (name, text)) in FIXTURES.iter().enumerate() {
        let mut doc = Doc::read(text).unwrap();
        doc.name = format!("instance_{name}");
        let asset = format!("WORKSHOP/{}", doc.name);
        let def = PropDef::new(doc).unwrap();
        let expected = def.compiled.collider_shapes();
        props::install(&asset, def).unwrap();
        let id =
            sim.spawn_prop(&asset, Vec3::new(i as f32 * 4.0, 0.0, 0.0), Quat::from_rotation_y(0.3), 1.0, true, None).unwrap();
        assert_eq!(shapes_of(&sim, id), expected, "{name}");
        counts.push(expected);
        placed.push((asset, id, expected));
    }
    // Tubes are walled with 12 boxes, tori ringed with capsules; a decorative torch has none.
    assert_eq!(counts, [7, 42, 16, 0, 64, 15, 3, 10]);
    let hash = sim.state_hash();
    let mut snapshot = Vec::new();
    ciborium::into_writer(&sim.snapshot(), &mut snapshot).unwrap();
    for (asset, _, _) in &placed {
        props::remove(asset).unwrap();
    }
    sim.restore(ciborium::from_reader(snapshot.as_slice()).unwrap());
    assert_eq!(sim.state_hash(), hash, "a format-2 instance saves and loads to the same state");
    for (asset, id, expected) in &placed {
        let prop = sim.state.entities.get(*id).unwrap().prop.as_ref().unwrap();
        assert!(prop.definition.revision.starts_with("prop2-"), "{asset} keeps its embedded document");
        assert_eq!(shapes_of(&sim, *id), *expected, "{asset}");
    }
}

/// Shapes in a placed prop's compound; none without a body.
fn shapes_of(sim: &Sim, id: EntityId) -> usize {
    if sim.state.entities.get(id).unwrap().body.is_none() { 0 } else { compound_parts(sim, id) }
}
