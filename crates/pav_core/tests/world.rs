//! World checks: pavilion layout, streaming, persistence of moved props, rooms.

use glam::{Vec2, Vec3};
use pav_core::statics::RegionKey;
use pav_core::{InputFrame, Sim, SimEvent};

fn feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

#[test]
fn world_builds_and_streams() {
    let mut sim = Sim::new("world", 3).unwrap();
    assert!(sim.state.world.enabled);
    assert!(sim.state.world.room("playground").is_some(), "playground is placed in the pavilion");
    assert!(sim.state.statics.is_active(RegionKey::Hub));
    let chunks = sim.state.statics.chunks.keys().filter(|k| matches!(k, RegionKey::Chunk(..))).count();
    assert!(chunks >= 9, "terrain around the start is loaded ({chunks})");
    sim.run(30, &InputFrame::default());
    let f = feet(&sim);
    assert!(f.y.abs() < 0.2, "player stands on the plaza: {f}");

    // Far away: the start area goes dormant / is dropped, new chunks load.
    let pid = sim.state.player.unwrap();
    sim.set_position(pid, Vec3::new(400.0, 30.0, 0.0));
    for _ in 0..40 {
        sim.update_streaming(usize::MAX);
        sim.step(&InputFrame::default());
    }
    assert!(!sim.state.statics.is_active(RegionKey::Hub), "hub sleeps when far away");
    assert!(sim.state.statics.is_active(RegionKey::chunk_of(Vec3::new(400.0, 0.0, 0.0))));
    // Terrain under the player catches them.
    sim.run(120, &InputFrame::default());
    let f = feet(&sim);
    assert!(sim.player().unwrap().character.as_ref().unwrap().grounded, "standing on generated terrain at {f}");
}

#[test]
fn moved_props_persist_across_unload() {
    let mut sim = Sim::new("world", 3).unwrap();
    // Put a prop in the wilderness, let it settle, move away, come back.
    let spot = Vec3::new(120.0, 0.0, 120.0);
    let pid = sim.state.player.unwrap();
    sim.set_position(pid, spot + Vec3::new(3.0, 20.0, 0.0));
    sim.update_streaming(usize::MAX);
    sim.run(60, &InputFrame::default());
    let ground = feet(&sim).y;
    let id = sim.spawn(
        pav_core::Spawn::new("marker", spot + Vec3::Y * (ground + 1.0))
            .visual(pav_core::Visual::new(pav_core::Shape::Box { half: Vec3::splat(0.3) }, pav_core::Color::hex("#ff00ff")))
            .body(pav_core::BodyKind::Dynamic),
    );
    sim.run(90, &InputFrame::default());
    let rest = sim.state.entities.get(id).unwrap().pos;
    sim.set_position(pid, Vec3::new(-500.0, 30.0, -500.0));
    for _ in 0..10 {
        sim.update_streaming(usize::MAX);
    }
    assert!(sim.state.entities.get(id).is_none(), "prop sleeps with its chunk");
    sim.set_position(pid, spot + Vec3::new(3.0, 20.0, 0.0));
    for _ in 0..10 {
        sim.update_streaming(usize::MAX);
    }
    let back = sim.state.entities.get(id).expect("prop restored").pos;
    assert!(back.distance(rest) < 0.05, "restored where it was: {rest} vs {back}");
}

#[test]
fn rooms_enter_exit_reset() {
    let mut sim = Sim::new("world/playground", 1).unwrap();
    sim.run(5, &InputFrame::default());
    let ev = sim.drain_events();
    assert!(ev.iter().any(|e| matches!(e, SimEvent::EnterRoom { .. })), "entered the room");
    let id = sim.state.world.current_room.expect("in a room");
    let slot = sim.state.world.rooms[id as usize].clone();
    // Walk back out through the door.
    let out = Vec2::new(-slot.inward.x, -slot.inward.z);
    sim.run(90, &InputFrame { move_dir: out, ..Default::default() });
    assert_eq!(sim.state.world.current_room, None, "left the room through the door");
    // Reset rebuilds blocks.
    let before = sim.state.statics.chunks.get(&RegionKey::Room(id)).map(|c| c.blocks.len());
    sim.reset_room(id);
    let after = sim.state.statics.chunks.get(&RegionKey::Room(id)).map(|c| c.blocks.len());
    assert_eq!(before, after);
}

#[test]
fn room_overrides_apply_and_restore() {
    let text = pav_core::room::load_sources(None).into_iter().find(|s| s.key == "playground").unwrap().text;
    let mut def = pav_core::room::RoomDef::parse(&text).unwrap();
    def.params.insert("movement.jump_height".into(), pav_core::params::ParamValue::Float(3.0));
    let mut cfg = pav_core::SimConfig::default();
    let saved = pav_core::world::enter_overrides(&mut cfg, &def);
    assert_eq!(cfg.movement.jump_height, 3.0);
    assert_eq!(saved.len(), 1);
}

#[test]
fn saved_objects_roundtrip() {
    let src = pav_core::room::load_sources(None).into_iter().find(|s| s.key == "sandbox").unwrap();
    let mut sim = Sim::new("world/sandbox", 1).unwrap();
    sim.run(2, &InputFrame::default());
    let id = sim.state.world.current_room.expect("in the sandbox");
    let objs = sim.room_objects(id);
    assert!(!objs.is_empty());
    let text = pav_core::world::rewrite_room_objects(&src.text, &objs);
    let def = pav_core::room::RoomDef::parse(&text).expect("still a valid room");
    assert_eq!(def.objects.len(), objs.len());
    assert!((def.objects[0].pos - objs[0].pos).length() < 1e-3);
    assert!(text.contains("map = '''"), "maps are kept as written");
}
