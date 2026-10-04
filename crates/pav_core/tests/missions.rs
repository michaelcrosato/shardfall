//! Room mission systems: uplinks (hack zones), switches that shut things down or bring them
//! online on signals (all of them needed), the `spotted` signal, pickups, followers walking the
//! player's trail, and rewind across all of it. Fixture: tests/mission_room.toml.

use glam::Vec3;
use pav_core::room::RoomDef;
use pav_core::{InputFrame, Sim};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("mission_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "mt", def);
    sim.state.scene = "mt".into();
    sim.run(5, &InputFrame::default());
    sim
}

/// Layout -> world (origin -15, 0, -10).
fn at(x: f32, z: f32) -> Vec3 {
    Vec3::new(-15.0 + x, 0.0, -10.0 + z)
}

fn put(sim: &mut Sim, p: Vec3) {
    let id = sim.state.player.unwrap();
    sim.set_position(id, p);
}

fn has(sim: &Sim, name: &str) -> bool {
    sim.state.entities.find(name).is_some()
}

fn msg(sim: &Sim) -> String {
    sim.state.courses.message.as_ref().map(|m| m.0.clone()).unwrap_or_default()
}

fn score(sim: &Sim) -> u32 {
    sim.state.courses.run.as_ref().map(|r| r.score).unwrap_or(0)
}

/// Enters START so a course run (and its score) exists.
fn start(sim: &mut Sim) {
    put(sim, at(2.5, 2.5));
    sim.run(5, &InputFrame::default());
    assert!(sim.state.courses.run.is_some(), "course run");
}

#[test]
fn an_uplink_fills_while_you_stand_in_it_and_switches_things() {
    let mut sim = room();
    start(&mut sim);
    assert!(has(&sim, "grid_gun") && !has(&sim, "loot"), "turret up, cache sealed");
    let labels = sim.state.statics.region_labels(pav_core::statics::RegionKey::Room(0));
    assert!(!labels.iter().any(|l| l.text == "THE GRID IS DOWN"), "story label held back");
    put(&mut sim, at(12.5, 3.5));
    sim.run(30, &InputFrame::default());
    let u = sim.uplink_hud().expect("standing in the uplink");
    assert_eq!(u.label, "GRID");
    assert!(u.progress > 0.15 && u.progress < 0.4 && !u.done, "half a second of two: {}", u.progress);
    // Stepping out keeps the progress (no decay by default) ...
    put(&mut sim, at(12.5, 5.5));
    sim.run(30, &InputFrame::default());
    assert!(sim.uplink_hud().is_none());
    put(&mut sim, at(12.5, 3.5));
    sim.run(100, &InputFrame::default());
    assert!(sim.uplink_hud().is_some_and(|u| u.done), "done after two seconds in total");
    sim.run(2, &InputFrame::default());
    assert!(msg(&sim).contains("HACKED") || score(&sim) == 40, "hack message: {}", msg(&sim));
    assert_eq!(score(&sim), 40, "uplink pays its score once");
    assert!(!has(&sim, "grid_gun"), "the turret shut down");
    assert!(has(&sim, "loot"), "the cache came online");
    let labels = &sim.state.statics.region_labels(pav_core::statics::RegionKey::Room(0));
    assert!(labels.iter().any(|l| l.text == "THE GRID IS DOWN"), "the story label was revealed");
    // Pickups: walk over them.
    let loot = sim.state.entities.find("loot").unwrap().pos;
    put(&mut sim, Vec3::new(loot.x, 0.0, loot.z));
    sim.run(3, &InputFrame::default());
    assert!(!has(&sim, "loot"), "picked up");
    assert_eq!(score(&sim), 70);
    put(&mut sim, at(8.5, 5.5));
    sim.run(3, &InputFrame::default());
    assert_eq!(score(&sim), 95, "the loose chip pays 25");
    // A finished uplink stays finished.
    put(&mut sim, at(12.5, 3.5));
    sim.run(200, &InputFrame::default());
    assert_eq!(score(&sim), 95);
}

#[test]
fn a_hit_while_hacking_drops_the_trace() {
    let mut sim = room();
    put(&mut sim, at(16.5, 3.5));
    sim.run(90, &InputFrame::default());
    let before = sim.uplink_hud().unwrap().progress;
    assert!(before > 0.2, "1.5 s of 6: {before}");
    let feet = sim.player().unwrap().pos;
    sim.state.projectiles.spawn(pav_core::projectile::Projectile {
        pos: Vec3::new(feet.x - 3.0, feet.y, feet.z),
        vel: Vec3::X * 12.0,
        radius: 0.15,
        life: 2.0,
        color: pav_core::Color::WHITE,
        knockback: 0.5,
        gravity: 0.0,
        owner: None,
        team: Default::default(),
        damage: 1.0,
    });
    let mut lost = false;
    for _ in 0..40 {
        sim.run(1, &InputFrame::default());
        lost |= msg(&sim) == "TRACE LOST";
    }
    assert!(lost, "trace lost message");
    let after = sim.uplink_hud().map(|u| u.progress).unwrap_or(0.0);
    assert!(after < before * 0.5, "progress dropped: {before} -> {after}");
}

#[test]
fn a_switch_needs_all_of_its_signals() {
    let mut sim = room();
    put(&mut sim, at(20.5, 3.5));
    sim.run(75, &InputFrame::default());
    assert!(has(&sim, "door"), "one key is not enough");
    put(&mut sim, at(24.5, 3.5));
    sim.run(75, &InputFrame::default());
    assert!(!has(&sim, "door"), "both keys open it");
    let ev = sim.drain_events();
    assert!(ev.iter().any(|e| matches!(e, pav_core::SimEvent::Switched { on: false, .. })), "switch event");
    assert!(ev.iter().filter(|e| matches!(e, pav_core::SimEvent::Hacked { .. })).count() >= 2, "two hacks");
}

#[test]
fn being_spotted_brings_reinforcements_and_counts_alarms() {
    let mut sim = room();
    start(&mut sim);
    assert!(!has(&sim, "backup"), "held back");
    put(&mut sim, at(2.5, 18.0));
    sim.run(60, &InputFrame::default());
    assert!(msg(&sim).contains("SPOTTED"), "{}", msg(&sim));
    sim.run(3, &InputFrame::default());
    assert!(has(&sim, "backup"), "reinforcements arrived");
    assert_eq!(sim.state.courses.run.as_ref().unwrap().alarms, 1);
    // The crew regrouped behind the player at the checkpoint.
    let p = sim.player().unwrap().pos;
    let crew = sim.state.entities.find("crew").unwrap().pos;
    assert!(crew.distance(p) < 3.0, "crew at the checkpoint: {}", crew.distance(p));
}

#[test]
fn followers_walk_the_trail_round_the_wall() {
    let mut sim = room();
    put(&mut sim, at(5.5, 8.5));
    sim.run(10, &InputFrame::default());
    // East along the wall, round its end, back west below it.
    let legs = [
        (at(25.5, 8.5), glam::Vec2::new(1.0, 0.0)),
        (at(25.5, 13.5), glam::Vec2::new(0.0, 1.0)),
        (at(8.5, 13.5), glam::Vec2::new(-1.0, 0.0)),
    ];
    for (goal, dir) in legs {
        for _ in 0..600 {
            let p = sim.player().unwrap().pos;
            let d = Vec3::new(goal.x - p.x, 0.0, goal.z - p.z);
            if d.length() < 0.4 {
                break;
            }
            sim.run(1, &InputFrame { move_dir: dir, ..Default::default() });
        }
    }
    sim.run(240, &InputFrame::default());
    let p = sim.player().unwrap().pos;
    let crew = sim.state.entities.find("crew").unwrap().pos;
    assert!(crew.z > at(0.0, 10.5).z, "crew got below the wall: {crew} (player {p})");
    assert!(crew.distance(p) < 4.0, "crew caught up: {}", crew.distance(p));
}

#[test]
fn rewind_replays_uplinks_and_switches_exactly() {
    let mut sim = room();
    start(&mut sim);
    let snap = sim.snapshot();
    let tick0 = sim.state.tick;
    let script = |sim: &mut Sim| {
        put(sim, at(12.5, 3.5));
        sim.run(140, &InputFrame::default());
        put(sim, at(20.5, 3.5));
        sim.run(70, &InputFrame::default());
        sim.state_hash()
    };
    let a = script(&mut sim);
    assert!(!has(&sim, "grid_gun"));
    sim.restore(snap);
    assert_eq!(sim.state.tick, tick0);
    assert!(has(&sim, "grid_gun") && !has(&sim, "loot"), "restored");
    let b = script(&mut sim);
    assert_eq!(a, b, "same inputs, same state");
}
