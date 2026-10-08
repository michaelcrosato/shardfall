//! M4 mechanics on the Feel Lab room: courses, pads, ledges, swimming, pits, movement models,
//! projectiles and moving platforms.

use glam::{Quat, Vec2, Vec3};
use pav_core::character::MovementModel;
use pav_core::entity::{EmitterDef, Hazard, MoverDef, Pattern};
use pav_core::input::buttons;
use pav_core::room::RoomDef;
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

#[test]
fn walk_across_a_kinematic_slab() {
    let mut sim = Sim::new("empty", 1).unwrap();
    sim.spawn(
        Spawn::new("slab", Vec3::new(0.0, 1.9, 0.0))
            .visual(Visual::new(Shape::Box { half: Vec3::new(6.0, 0.2, 6.0) }, Color::WHITE))
            .body(BodyKind::Kinematic),
    );
    let id = sim.state.player.unwrap();
    sim.set_position(id, Vec3::new(-4.0, 2.1, 0.0));
    go(&mut sim, Vec2::ZERO, 30);
    go(&mut sim, Vec2::new(1.0, 0.0), 60);
    let f = feet(&sim);
    assert!(f.x > 1.5, "walked across the slab: {f}");
    assert!(f.y > 2.05, "on top: {f}");
}

/// A 42 m flat floor in two materials that meet at x = 0, and beside it an eastward belt
/// (2.7 m/s) running into the east wall, with a walker on it heading west: a treadmill.
const FLATS: &str = r##"
name = "FLATS"
entrance = { at = [21, 7], facing = "north" }
[layout]
origin = [-22.0, 0.0, -4.0]
[[layout.layer]]
map = '''
############################################
#aaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbb#
#aaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbb#
#..........................................#
#..>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>#
#..>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>#
#..........................................#
#####################.######################
'''
[layout.legend.'.']
blocks = [{ y0 = -0.5, y1 = 0, color = "#cfd6c4" }]
[layout.legend.'a']
blocks = [{ y0 = -0.5, y1 = 0, color = "#a8d8ea" }]
[layout.legend.'b']
blocks = [{ y0 = -0.5, y1 = 0, color = "#c5d3b8" }]
[layout.legend.'#']
blocks = [{ y0 = -0.5, y1 = 0, color = "#cfd6c4" }, { y0 = 0, y1 = 3, color = "#b9c3cf" }]
[layout.legend.'>']
blocks = [{ y0 = -0.5, y1 = 0, color = "#4a4f57" }]
zone = { kind = "conveyor", facing = "east", speed = 2.7 }

[[npc]]
name = "treadmill"
pos = [41.5, 0, 5.0]
ai = { type = "patrol", points = [[3.0, 5.0], [41.5, 5.0]], pause = 0.0 }
speed = 0.5
"##;

fn flats() -> Sim {
    let def = RoomDef::parse(FLATS).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "flats", def);
    sim.state.scene = "flats".into();
    go(&mut sim, Vec2::ZERO, 10);
    sim
}

#[test]
fn slides_keep_their_speed_across_a_flat_floor() {
    // Rapier's controller now and then dropped a whole tick of motion when a move pressed down
    // into level ground: an ice slide (momentum) stopped dead mid-floor, a walk (instant) hitched.
    for model in [MovementModel::Momentum, MovementModel::Instant] {
        let mut sim = flats();
        let m = &mut sim.config.movement;
        m.model = model;
        (m.accel, m.decel, m.skid_decel) = (10.0, 3.0, 14.0); // the Slalom's ICE pad
        let top = if model == MovementModel::Momentum { m.max_speed } else { m.speed };
        put(&mut sim, Vec3::new(-20.0, 0.5, -2.0));
        let hz = 1.0 / sim.dt();
        let mut x = feet(&sim).x;
        let mut slowest = f32::MAX;
        for t in 0..240 {
            go(&mut sim, Vec2::new(1.0, 0.0), 1);
            let nx = feet(&sim).x;
            if t >= 60 {
                slowest = slowest.min((nx - x) * hz);
            }
            x = nx;
        }
        assert!(x > 2.0, "{model:?} crossed where the floors meet: x = {x}");
        assert!(slowest > top * 0.95, "{model:?} kept its speed: slowest tick {slowest:.2} m/s of {top}");
    }
}

#[test]
fn walking_against_a_treadmill_keeps_the_stride() {
    let mut sim = flats();
    go(&mut sim, Vec2::ZERO, 60);
    let start = sim.state.entities.find("treadmill").unwrap().pos;
    // The walker strides at its own 3 m/s (legs and all) while the belt holds it nearly still.
    let mut cycles = 0.0;
    for _ in 0..60 {
        let before = sim.state.entities.find("treadmill").unwrap().character.as_ref().unwrap().anim.phase;
        go(&mut sim, Vec2::ZERO, 1);
        let w = sim.state.entities.find("treadmill").unwrap();
        let ch = w.character.as_ref().unwrap();
        assert!(ch.vel.x < -2.8, "walker strides at its own speed: {}", ch.vel);
        cycles += (ch.anim.phase - before).rem_euclid(1.0);
    }
    assert!(cycles > 1.0, "legs walk: {cycles:.2} cycles in a second");
    let w = sim.state.entities.find("treadmill").unwrap();
    let crept = start.x - w.pos.x;
    assert!(crept > 0.1 && crept < 1.0, "the belt holds it back: crept {crept:.2} m in a second");

    // The player too: full speed against the belt, only the difference over the ground.
    let top = sim.config.movement.speed;
    put(&mut sim, Vec3::new(17.0, 0.5, 1.0));
    let x0 = feet(&sim).x;
    go(&mut sim, Vec2::new(-1.0, 0.0), 60);
    let ch = sim.player().unwrap().character.as_ref().unwrap();
    assert!(ch.vel.x < -top * 0.95, "player strides at full speed: {}", ch.vel);
    let net = x0 - feet(&sim).x;
    assert!(net > (top - 2.7) * 0.8 && net < (top - 2.7) * 1.2, "net {net:.2} m in 1 s");

    // Walking with the belt into the wall is blocked, never walking backwards.
    put(&mut sim, Vec3::new(20.0, 0.5, 1.0));
    for _ in 0..30 {
        go(&mut sim, Vec2::new(1.0, 0.0), 1);
        let v = sim.player().unwrap().character.as_ref().unwrap().vel;
        assert!(v.x > -0.01, "pinned against the wall, not backing off: {v}");
    }
}
