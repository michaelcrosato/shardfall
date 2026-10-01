//! M9 vehicles: a raycast-vehicle drift car and an arcade helicopter the player can get into
//! (interact), drive, slide, fly and leave; rewind stays exact (tests/vehicle_room.toml).

use glam::{Vec2, Vec3};
use pav_core::input::buttons;
use pav_core::room::RoomDef;
use pav_core::{InputFrame, Sim};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("vehicle_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "vt", def);
    sim.state.scene = "vt".into();
    sim.run(30, &InputFrame::default());
    sim
}

fn press(b: u32) -> InputFrame {
    InputFrame { pressed: b, held: b, ..Default::default() }
}

fn hold(dir: Vec2, held: u32) -> InputFrame {
    InputFrame { move_dir: dir, held, ..Default::default() }
}

fn get_in(sim: &mut Sim, name: &str) -> pav_core::EntityId {
    let v = sim.state.entities.find(name).unwrap();
    let (vid, vpos) = (v.id, v.pos);
    let pid = sim.state.player.unwrap();
    sim.set_position(pid, vpos + Vec3::new(1.6, -0.8, 0.0));
    sim.run(2, &InputFrame::default());
    sim.run(1, &press(buttons::INTERACT));
    assert_eq!(sim.player().unwrap().character.as_ref().unwrap().riding, Some(vid), "got into the {name}");
    vid
}

fn speed(sim: &Sim, name: &str) -> f32 {
    let e = sim.state.entities.find(name).unwrap();
    sim.state.physics.bodies[e.body.unwrap()].linvel().length()
}

#[test]
fn the_car_drives_turns_and_drifts() {
    let mut sim = room();
    get_in(&mut sim, "car");
    let start = sim.state.entities.find("car").unwrap().pos;
    // Facing north (-Z): drive north.
    sim.run(120, &hold(Vec2::new(0.0, -1.0), 0));
    let p = sim.state.entities.find("car").unwrap().pos;
    assert!(start.z - p.z > 8.0, "drove north: {start} -> {p}");
    assert!(speed(&sim, "car") > 6.0, "speed {}", speed(&sim, "car"));
    // Steer east with the handbrake: the tail slides out.
    let mut slide = 0.0f32;
    for _ in 0..50 {
        sim.run(1, &hold(Vec2::new(1.0, -0.3), buttons::JUMP));
        slide = slide.max(sim.state.entities.find("car").unwrap().vehicle.as_ref().unwrap().drift);
    }
    assert!(slide > 2.0, "drifted sideways at {slide} m/s");
    // The player rides along and the camera follows.
    let car = sim.state.entities.find("car").unwrap().pos;
    assert!(sim.player().unwrap().pos.distance(car) < 1.5);
    assert!(sim.state.focus.distance(car) < 1.5);
}

#[test]
fn the_helicopter_takes_off_and_flies() {
    let mut sim = room();
    get_in(&mut sim, "heli");
    let start = sim.state.entities.find("heli").unwrap().pos;
    sim.run(90, &InputFrame::default()); // rotor spins up
    sim.run(90, &hold(Vec2::ZERO, buttons::JUMP));
    let up = sim.state.entities.find("heli").unwrap().pos;
    assert!(up.y - start.y > 3.0, "climbed: {start} -> {up}");
    sim.run(90, &hold(Vec2::new(1.0, 0.0), 0));
    let p = sim.state.entities.find("heli").unwrap().pos;
    assert!(p.x - up.x > 5.0, "flew east: {up} -> {p}");
    assert!((p.y - up.y).abs() < 1.5, "held altitude (climb momentum fades): {up} -> {p}");
}

#[test]
fn getting_out_puts_you_beside_it() {
    let mut sim = room();
    let vid = get_in(&mut sim, "car");
    sim.run(1, &press(buttons::INTERACT));
    let p = sim.player().unwrap();
    assert_eq!(p.character.as_ref().unwrap().riding, None);
    let car = sim.state.entities.get(vid).unwrap().pos;
    let d = Vec2::new(p.pos.x - car.x, p.pos.z - car.z).length();
    assert!(d > 1.0 && d < 2.5, "beside the car: {d}");
    sim.run(30, &hold(Vec2::new(-1.0, 0.0), 0));
    assert!(sim.player().unwrap().character.as_ref().unwrap().grounded, "walking again");
}

#[test]
fn rewind_with_vehicles_is_repeatable() {
    let mut sim = room();
    get_in(&mut sim, "car");
    sim.run(20, &hold(Vec2::new(0.0, -1.0), 0));
    let (t, h) = (sim.state.tick, sim.state_hash());
    sim.run(60, &hold(Vec2::new(0.6, -1.0), buttons::JUMP));
    let (t2, h2) = (sim.state.tick, sim.state_hash());
    assert!(sim.rewind_to(t));
    assert_eq!(sim.state_hash(), h);
    assert!(sim.rewind_to(t2));
    assert_eq!(sim.state_hash(), h2, "re-simulation with a car matches");
}
