//! M5 physics features on a small test room (tests/physics_room.toml): rope bridge, chain,
//! soft bodies, conveyor, bounce pad, crumbling and breakable tiles, spawner signals, rewind.

use glam::{Vec2, Vec3};
use pav_core::room::RoomDef;
use pav_core::{BodyKind, Color, InputFrame, Shape, Sim, Spawn, Visual};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("physics_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "pt", def);
    sim.state.scene = "pt".into();
    sim.run(30, &InputFrame::default());
    sim
}

fn feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

fn go(sim: &mut Sim, dir: Vec2, ticks: u64) {
    sim.run(ticks, &InputFrame { move_dir: dir, ..Default::default() });
}

fn put(sim: &mut Sim, p: Vec3) {
    let id = sim.state.player.unwrap();
    sim.set_position(id, p);
}

/// Layout cell centre (origin -12, 0, -10).
fn cell(c: f32, r: f32) -> Vec3 {
    Vec3::new(-12.0 + c + 0.5, 0.0, -10.0 + r + 0.5)
}

#[test]
fn rope_bridge_carries_the_player_across() {
    let mut sim = room();
    put(&mut sim, Vec3::new(-10.0, 0.0, -1.5));
    go(&mut sim, Vec2::ZERO, 120);
    let mut lowest = f32::MAX;
    for _ in 0..140 {
        go(&mut sim, Vec2::new(1.0, 0.0), 1);
        lowest = lowest.min(feet(&sim).y);
    }
    let f = feet(&sim);
    assert!(f.x > 2.0, "crossed the bridge: {f}");
    assert!(f.y > -0.5 && lowest > -1.0, "stayed on it (lowest {lowest}): {f}");
}

#[test]
fn chain_holds_the_wrecking_ball() {
    let mut sim = room();
    go(&mut sim, Vec2::ZERO, 240);
    let ball = sim.state.entities.find("ball").unwrap().pos;
    assert!(ball.y > 0.8 && ball.y < 2.2, "ball hangs from the chain: {ball}");
}

#[test]
fn soft_bodies_keep_their_shape_and_pins() {
    let mut sim = room();
    go(&mut sim, Vec2::ZERO, 240);
    let jelly = sim.state.entities.find("jelly").unwrap();
    assert!(jelly.pos.y > 0.35 && jelly.pos.y < 1.0, "jelly rests on the floor: {}", jelly.pos);
    let flag = sim.state.entities.find("flag").unwrap();
    assert!(flag.pos.y > 1.5, "flag hangs from its pinned top: {}", flag.pos);
    let rope = sim.state.entities.find("rope").unwrap();
    assert!(rope.pos.y > 1.0, "rope hangs: {}", rope.pos);
}

#[test]
fn conveyor_and_bounce_pad() {
    let mut sim = room();
    put(&mut sim, cell(3.0, 4.0));
    go(&mut sim, Vec2::ZERO, 60);
    assert!(feet(&sim).x - cell(3.0, 4.0).x > 2.0, "carried east by the belt: {}", feet(&sim));
    put(&mut sim, cell(17.5, 5.5));
    let mut top = 0.0f32;
    for _ in 0..40 {
        go(&mut sim, Vec2::ZERO, 1);
        top = top.max(feet(&sim).y);
    }
    assert!(top > 1.5, "bounced up to {top}");
}

#[test]
fn crumbling_tiles_fall_and_regrow() {
    let mut sim = room();
    let blocks = sim.state.statics.block_count();
    put(&mut sim, cell(4.0, 2.0));
    go(&mut sim, Vec2::ZERO, 70);
    assert_eq!(sim.state.statics.block_count(), blocks - 1, "a tile fell");
    put(&mut sim, cell(12.0, 16.0));
    go(&mut sim, Vec2::ZERO, 300);
    assert_eq!(sim.state.statics.block_count(), blocks, "and grew back");
}

#[test]
fn heavy_impact_breaks_glass() {
    let mut sim = room();
    let blocks = sim.state.statics.block_count();
    let p = cell(14.0, 2.0) + Vec3::Y * 6.0;
    sim.spawn(
        Spawn::new("weight", p)
            .visual(Visual::new(Shape::Box { half: Vec3::splat(0.4) }, Color::WHITE))
            .body(BodyKind::Dynamic)
            .density(300.0),
    );
    go(&mut sim, Vec2::ZERO, 120);
    assert!(sim.state.statics.block_count() < blocks, "glass broke");
}

#[test]
fn spawn_pad_drops_props() {
    let mut sim = room();
    let n = sim.state.entities.len();
    put(&mut sim, cell(3.0, 14.0));
    go(&mut sim, Vec2::ZERO, 30);
    assert!(sim.state.entities.len() >= n + 20, "{} -> {}", n, sim.state.entities.len());
}

#[test]
fn rewind_with_soft_bodies_and_joints_is_repeatable() {
    let mut sim = room();
    go(&mut sim, Vec2::new(0.3, -1.0), 40);
    let (t, h) = (sim.state.tick, sim.state_hash());
    go(&mut sim, Vec2::new(1.0, 0.2), 90);
    let (t2, h2) = (sim.state.tick, sim.state_hash());
    assert!(sim.rewind_to(t));
    assert_eq!(sim.state_hash(), h);
    assert!(sim.rewind_to(t2));
    assert_eq!(sim.state_hash(), h2, "re-simulation matches");
}

#[test]
fn joints_and_soft_bodies_survive_dormancy() {
    let def = RoomDef::parse(include_str!("physics_room.toml")).unwrap();
    let mut sim = Sim::empty(1);
    sim.build_world(vec![("pt".into(), def)], Vec::new());
    sim.teleport_to_room("pt");
    go(&mut sim, Vec2::ZERO, 120);
    let ball0 = sim.state.entities.find("ball").unwrap().pos;
    // Leave far away so the room goes dormant, then come back.
    let id = sim.state.player.unwrap();
    sim.set_position(id, Vec3::new(400.0, 20.0, 400.0));
    for _ in 0..40 {
        sim.update_streaming(usize::MAX);
        go(&mut sim, Vec2::ZERO, 15);
    }
    assert!(sim.state.entities.find("ball").is_none(), "room went dormant");
    sim.teleport_to_room("pt");
    go(&mut sim, Vec2::ZERO, 240);
    let ball = sim.state.entities.find("ball").unwrap().pos;
    assert!((ball - ball0).length() < 0.6, "chain still holds the ball: {ball0} -> {ball}");
    let rope = sim.state.entities.find("rope").unwrap().pos;
    assert!(rope.y > 1.0, "rope still hangs: {rope}");
    let lowest_plank = sim.state.entities.iter().filter(|e| e.name == "~plank").map(|e| e.pos.y).fold(f32::MAX, f32::min);
    assert!(lowest_plank > -1.0, "bridge still spans: {lowest_plank}");
}
