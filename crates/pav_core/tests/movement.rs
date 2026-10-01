//! M4 mechanics on the Feel Lab room: courses, pads, ledges, swimming, pits, movement models,
//! projectiles and moving platforms.

use glam::{Quat, Vec2, Vec3};
use pav_core::character::MovementModel;
use pav_core::entity::{EmitterDef, Hazard, MoverDef, Pattern};
use pav_core::input::buttons;
use pav_core::{Behavior, BodyKind, Color, InputFrame, Shape, Sim, Spawn, Visual};

fn feet(sim: &Sim) -> Vec3 {
    let p = sim.player().expect("player");
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

fn go(sim: &mut Sim, dir: Vec2, ticks: u64) {
    sim.run(ticks, &InputFrame { move_dir: dir, ..Default::default() });
}

fn hold(sim: &mut Sim, dir: Vec2, b: u32, ticks: u64) {
    for i in 0..ticks {
        sim.step(&InputFrame { move_dir: dir, held: b, pressed: if i == 0 { b } else { 0 }, ..Default::default() });
    }
}

/// Feel Lab origin is (-16, 0, -13); cell centres are +0.5.
fn cell(col: f32, row: f32) -> Vec3 {
    Vec3::new(-16.0 + col + 0.5, 0.0, -13.0 + row + 0.5)
}

fn lab() -> Sim {
    let mut sim = Sim::new("feel_lab", 1).unwrap();
    go(&mut sim, Vec2::ZERO, 10);
    sim
}

fn put(sim: &mut Sim, p: Vec3) {
    let id = sim.state.player.unwrap();
    sim.set_position(id, p);
    go(sim, Vec2::ZERO, 5);
}

#[test]
fn sprint_course_times_a_run() {
    let mut sim = lab();
    put(&mut sim, cell(1.0, 3.0));
    assert!(sim.state.courses.run.is_some(), "standing in START arms the course");
    go(&mut sim, Vec2::new(1.0, 0.0), 360);
    let r = sim.state.courses.last.clone().expect("finished");
    // 26 m at 6 m/s ≈ 4.3 s.
    assert!(r.time > 3.5 && r.time < 5.5, "sprint time {}", r.time);
    assert!(r.new_best);
    assert!(sim.state.courses.best.values().any(|b| (*b - r.time).abs() < 1e-4));
}

#[test]
fn pads_switch_the_movement_model() {
    let mut sim = lab();
    assert_eq!(sim.config.movement.model, MovementModel::Instant);
    put(&mut sim, cell(8.0, 21.0));
    assert_eq!(sim.config.movement.model, MovementModel::Momentum);
    put(&mut sim, cell(21.0, 21.0));
    assert_eq!(sim.config.movement.model, MovementModel::Grid);
}

#[test]
fn jump_and_grab_the_ledge() {
    let mut sim = lab();
    // The 2.4 m wall spans columns 13-17, rows 12-14; stand south of it facing north.
    put(&mut sim, cell(15.0, 15.6));
    hold(&mut sim, Vec2::new(0.0, -1.0), buttons::JUMP, 12);
    let mut grabbed = false;
    for _ in 0..90 {
        go(&mut sim, Vec2::new(0.0, -1.0), 1);
        grabbed |= sim.player().unwrap().character.as_ref().unwrap().hang.is_some();
        if feet(&sim).y > 2.3 && sim.player().unwrap().character.as_ref().unwrap().grounded {
            break;
        }
    }
    go(&mut sim, Vec2::ZERO, 10);
    assert!(grabbed, "grabbed the ledge");
    let f = feet(&sim);
    assert!((f.y - 2.4).abs() < 0.15, "pulled up onto the ledge: {f}");
}

#[test]
fn swim_in_the_pool() {
    let mut sim = lab();
    put(&mut sim, cell(4.0, 15.0) + Vec3::Y * 0.5);
    go(&mut sim, Vec2::ZERO, 120);
    let p = sim.player().unwrap();
    let ch = p.character.as_ref().unwrap();
    assert!(ch.swimming, "swimming (depth {})", ch.water_depth);
    let f = feet(&sim);
    assert!(f.y > -1.8 && f.y < -1.2, "floating with the head out: {f}");
    // Dive.
    hold(&mut sim, Vec2::ZERO, buttons::CROUCH, 60);
    assert!(feet(&sim).y < f.y - 0.3, "dived");
}

#[test]
fn falling_into_a_pit_respawns() {
    let mut sim = lab();
    put(&mut sim, cell(18.0, 8.0) + Vec3::Y * 1.0);
    let checkpoint = sim.state.courses.checkpoint;
    put(&mut sim, cell(19.0, 8.0) + Vec3::Y * 1.0);
    go(&mut sim, Vec2::ZERO, 60);
    let f = feet(&sim);
    assert!(f.y > -1.0, "respawned instead of lying in the pit: {f}");
    assert_eq!(sim.state.courses.checkpoint, checkpoint);
}

#[test]
fn grid_model_steps_tile_by_tile() {
    let mut sim = lab();
    sim.config.movement.model = MovementModel::Grid;
    put(&mut sim, cell(15.0, 18.0));
    go(&mut sim, Vec2::new(1.0, 0.0), 31);
    go(&mut sim, Vec2::ZERO, 20);
    let f = feet(&sim);
    let frac = (f.x - f.x.floor() - 0.5).abs();
    assert!(frac < 0.05, "stopped on a tile centre: {f}");
    assert!(f.x - cell(15.0, 18.0).x >= 2.9, "moved a few tiles: {f}");
    assert!((f.z - cell(15.0, 18.0).z).abs() < 0.05, "stayed in the row");
}

#[test]
fn committed_model_rolls() {
    let mut sim = lab();
    sim.config.movement.model = MovementModel::Committed;
    put(&mut sim, cell(15.0, 18.0));
    go(&mut sim, Vec2::new(1.0, 0.0), 30);
    hold(&mut sim, Vec2::new(1.0, 0.0), buttons::CROUCH, 3);
    let ch = sim.player().unwrap().character.as_ref().unwrap();
    assert!(ch.roll > 0.0, "rolling");
    assert!(ch.height() < 1.0, "low while rolling");
}

#[test]
fn projectiles_hit_and_crouching_dodges() {
    let mut sim = lab();
    put(&mut sim, cell(15.0, 18.0));
    let p = feet(&sim);
    let em = EmitterDef { pattern: Pattern::Aimed, interval: 0.25, speed: 10.0, height: 0.0, ..Default::default() };
    // Head-height emitter 6 m east.
    sim.spawn(
        Spawn::new("turret", p + Vec3::new(6.0, 1.4, 0.0))
            .visual(Visual::new(Shape::Sphere { radius: 0.2 }, Color::WHITE))
            .behavior(Behavior::Emitter(em)),
    );
    // Crouching: shots fly over.
    hold(&mut sim, Vec2::ZERO, buttons::CROUCH, 120);
    let ch = sim.player().unwrap().character.as_ref().unwrap();
    assert!(ch.invuln <= 0.0 && ch.stun <= 0.0, "crouching dodged the shots");
    // Standing: hit.
    let mut hit = false;
    for _ in 0..90 {
        go(&mut sim, Vec2::ZERO, 1);
        hit |= sim.player().unwrap().character.as_ref().unwrap().invuln > 0.0;
    }
    assert!(hit, "standing gets hit");
}

#[test]
fn moving_platform_carries_the_player() {
    let mut sim = lab();
    put(&mut sim, cell(15.0, 18.0));
    let p = feet(&sim);
    let half = Vec3::new(1.0, 0.1, 1.0);
    sim.spawn(
        Spawn::new("platform", p + Vec3::new(0.0, 0.3, 0.0))
            .visual(Visual::new(Shape::Box { half }, Color::WHITE))
            .body(BodyKind::Kinematic)
            .rot(Quat::IDENTITY)
            .behavior(Behavior::Move(MoverDef {
                offset: Vec3::new(4.0, 0.0, 0.0),
                period: 4.0,
                smooth: false,
                ..Default::default()
            })),
    );
    put(&mut sim, p + Vec3::Y * 0.5);
    let start = feet(&sim);
    go(&mut sim, Vec2::ZERO, 60);
    let f = feet(&sim);
    assert!(f.x - start.x > 1.5, "carried east: {start} -> {f}");
    assert!((f.y - 0.4).abs() < 0.1, "still on top: {f}");
}

#[test]
fn hazards_knock_back() {
    let mut sim = lab();
    put(&mut sim, cell(15.0, 18.0));
    let p = feet(&sim);
    let mut sp = Spawn::new("spikes", p + Vec3::new(1.2, 0.5, 0.0))
        .visual(Visual::new(Shape::Box { half: Vec3::splat(0.5) }, Color::WHITE))
        .body(BodyKind::Fixed);
    sp.hazard = Some(Hazard::default());
    sim.spawn(sp);
    go(&mut sim, Vec2::new(1.0, 0.0), 20);
    go(&mut sim, Vec2::ZERO, 20);
    let f = feet(&sim);
    assert!(f.x < p.x, "knocked back west: {p} -> {f}");
}
