//! End-to-end movement checks on the playground room (the M2 "done when" list, headless).

use glam::{Vec2, Vec3};
use pav_core::input::buttons;
use pav_core::{InputFrame, Sim};

fn feet(sim: &Sim) -> Vec3 {
    let p = sim.player().expect("player");
    let ch = p.character.as_ref().unwrap();
    p.pos - Vec3::Y * ch.height() * 0.5
}

fn go(sim: &mut Sim, dir: Vec2, ticks: u64) {
    sim.run(ticks, &InputFrame { move_dir: dir, ..Default::default() });
}

fn press(sim: &mut Sim, dir: Vec2, b: u32) {
    sim.step(&InputFrame { move_dir: dir, held: b, pressed: b, ..Default::default() });
}

fn cell(col: f32, row: f32) -> Vec3 {
    // playground origin is (-14, 0, -10); cell centres are +0.5
    Vec3::new(-14.0 + col + 0.5, 0.0, -10.0 + row + 0.5)
}

fn playground() -> Sim {
    let mut sim = Sim::new("playground", 1).unwrap();
    go(&mut sim, Vec2::ZERO, 20); // settle
    sim
}

#[test]
fn walk_and_jump_onto_crate() {
    let mut sim = playground();
    let start = feet(&sim);
    assert!(start.y.abs() < 0.1, "starts on the ground: {start}");
    go(&mut sim, Vec2::new(1.0, 0.0), 30);
    let after = feet(&sim);
    assert!(after.x - start.x > 2.5, "walked east: {start} -> {after}");
    // Crate '2' (1 m) is at column 5, row 4. Stand north of it and jump south onto it.
    let id = sim.state.player.unwrap();
    sim.set_position(id, cell(5.0, 2.6));
    go(&mut sim, Vec2::ZERO, 10);
    press(&mut sim, Vec2::new(0.0, 1.0), buttons::JUMP);
    for _ in 0..40 {
        sim.step(&InputFrame { move_dir: Vec2::new(0.0, 1.0), held: buttons::JUMP, ..Default::default() });
        let f = feet(&sim);
        if f.z > cell(5.0, 4.0).z - 0.1 {
            break;
        }
    }
    go(&mut sim, Vec2::ZERO, 30);
    let f = feet(&sim);
    assert!((f.y - 1.0).abs() < 0.1, "standing on the 1 m crate: {f}");
}

#[test]
fn climb_ladder_bomb_floor_and_drop() {
    let mut sim = playground();
    let id = sim.state.player.unwrap();
    // Ladder at column 18, row 10 (wall to the north).
    sim.set_position(id, cell(18.0, 11.2));
    go(&mut sim, Vec2::ZERO, 5);
    go(&mut sim, Vec2::new(0.0, -1.0), 140);
    let f = feet(&sim);
    assert!((f.y - 3.0).abs() < 0.15, "climbed onto the upper floor: {f}");
    // Walk north onto the destructible floor and throw a bomb at the tiles ahead.
    go(&mut sim, Vec2::new(0.0, -1.0), 10);
    let blocks_before = sim.state.statics.block_count();
    let target = cell(18.0, 6.0) + Vec3::Y * 3.0;
    sim.step(&InputFrame { aim: Some(target), pressed: buttons::USE, held: buttons::USE, ..Default::default() });
    go(&mut sim, Vec2::ZERO, 120);
    let destroyed = blocks_before - sim.state.statics.block_count();
    assert!(destroyed >= 3, "bomb removed floor tiles: {destroyed}");
    // Walk into the hole and fall to the ground floor.
    go(&mut sim, Vec2::new(0.0, -1.0), 60);
    go(&mut sim, Vec2::ZERO, 60);
    let f = feet(&sim);
    assert!(f.y < 0.2, "dropped through the hole: {f}");
}

#[test]
fn crawl_under_low_ceiling() {
    let mut sim = playground();
    let id = sim.state.player.unwrap();
    // Tunnel runs east-west on row 14 from column 17 to 22 under a 0.8 m ceiling.
    sim.set_position(id, cell(15.0, 14.0));
    go(&mut sim, Vec2::ZERO, 5);
    go(&mut sim, Vec2::new(1.0, 0.0), 40);
    assert!(feet(&sim).x < cell(17.0, 14.0).x, "standing cannot enter the tunnel");
    press(&mut sim, Vec2::ZERO, buttons::CRAWL);
    go(&mut sim, Vec2::new(1.0, 0.0), 260);
    let f = feet(&sim);
    assert!(f.x > cell(23.0, 14.0).x, "crawled through: {f}");
}

#[test]
fn rewind_is_repeatable() {
    let mut sim = playground();
    go(&mut sim, Vec2::new(1.0, 0.3), 30);
    let t_mid = sim.state.tick;
    let h_mid = sim.state_hash();
    press(&mut sim, Vec2::ZERO, buttons::JUMP);
    go(&mut sim, Vec2::new(-0.5, 1.0), 90);
    let t_end = sim.state.tick;
    let h_end = sim.state_hash();
    assert!(sim.rewind_to(t_mid));
    assert_eq!(sim.state.tick, t_mid);
    assert_eq!(sim.state_hash(), h_mid, "rewound state matches");
    assert!(sim.rewind_to(t_end));
    assert_eq!(sim.state_hash(), h_end, "re-simulated future matches the original");
}

#[test]
fn snapshot_file_roundtrip() {
    let mut sim = playground();
    go(&mut sim, Vec2::new(1.0, 0.0), 20);
    let dir = std::env::temp_dir().join("pav_snapshot_test.bin");
    sim.save_state(&dir).unwrap();
    let h = sim.state_hash();
    go(&mut sim, Vec2::new(0.0, 1.0), 20);
    sim.load_state(&dir).unwrap();
    assert_eq!(sim.state_hash(), h);
    go(&mut sim, Vec2::new(0.0, 1.0), 5); // still steps fine after loading
}
