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
fn the_car_turns_toward_the_stick() {
    for east in [true, false] {
        let mut sim = room();
        get_in(&mut sim, "car");
        // Facing north (-Z): get moving, then push the stick east (or west).
        sim.run(60, &hold(Vec2::new(0.0, -1.0), 0));
        let x = if east { 1.0 } else { -1.0 };
        sim.run(60, &hold(Vec2::new(x, 0.0), 0));
        let e = sim.state.entities.find("car").unwrap();
        let vel = sim.state.physics.bodies[e.body.unwrap()].linvel();
        assert!(vel.x * x > 3.0, "stick {} turned the car that way: velocity {vel}", if east { "east" } else { "west" });
    }
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

/// A straight 218 m strip, the car at its south end facing north: room to reach top speed.
fn strip() -> Sim {
    let (w, len) = (12, 220);
    let mut map = format!("{}\n", "#".repeat(w));
    for _ in 0..len - 2 {
        map += &format!("#{}#\n", ".".repeat(w - 2));
    }
    map += &format!("{0}..{0}\n", "#".repeat(w / 2 - 1));
    let src = format!(
        r##"name = "Strip"
entrance = {{ at = [{door}, {last}], facing = "south" }}
[layout]
origin = [-6.0, 0.0, -110.0]
[[layout.layer]]
map = '''
{map}'''
[layout.legend.'.']
blocks = [{{ y0 = -0.5, y1 = 0, color = "#8d939c" }}]
[layout.legend.'#']
blocks = [{{ y0 = -0.5, y1 = 0, color = "#8d939c" }}, {{ y0 = 0, y1 = 2, color = "#b9c3cf" }}]

[[object]]
name = "car"
pos = [6.0, 0.0, 200.0]
yaw = 180
vehicle = {{ kind = "car" }}
"##,
        door = w / 2 - 1,
        last = len - 1
    );
    let def = RoomDef::parse(&src).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "strip", def);
    sim.state.scene = "strip".into();
    sim.run(30, &InputFrame::default());
    get_in(&mut sim, "car");
    sim
}

fn gear(sim: &Sim) -> i8 {
    sim.state.entities.find("car").unwrap().vehicle.as_ref().unwrap().gearbox.gear
}

/// Runs `ticks` of `input`, returning each gear the box went into (in order) and the speed then.
fn shifts(sim: &mut Sim, ticks: u32, input: &InputFrame) -> Vec<(i8, f32)> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        let before = gear(sim);
        sim.run(1, input);
        if gear(sim) != before {
            out.push((gear(sim), speed(sim, "car")));
        }
    }
    out
}

#[test]
fn the_gearbox_climbs_through_the_gears_and_comes_back_down() {
    let mut sim = strip();
    assert_eq!(gear(&sim), 1, "a parked car waits in first");
    let max = sim.state.entities.find("car").unwrap().vehicle.as_ref().unwrap().def.max_speed;
    // Flat out, north: up through the gears in order, each near the top of the one before.
    let north = hold(Vec2::new(0.0, -1.0), 0);
    let mut seen = Vec::new();
    let mut to_95 = None;
    for t in 0..240 {
        seen.extend(shifts(&mut sim, 1, &north));
        if to_95.is_none() && speed(&sim, "car") > 0.95 * max {
            to_95 = Some((t + 1) as f32 / 60.0);
        }
    }
    let gears: Vec<i8> = seen.iter().map(|s| s.0).collect();
    assert_eq!(gears, [2, 3, 4, 5], "shifted up in order: {seen:?}");
    for ((g, at), top) in seen.iter().zip(pav_core::vehicle::GEAR_TOPS) {
        assert!((at / max - top).abs() < 0.04, "into {g} at {at:.1} m/s, near the top of the gear below ({top})");
    }
    // Top speed as with one gear (22 m/s), and as soon: 95% of it took 2.8 s before the gearbox.
    let top = speed(&sim, "car");
    assert!((top - max).abs() < 0.03 * max, "top speed {top:.2} of {max}");
    let to_95 = to_95.expect("reached 95% of top speed");
    assert!(to_95 < 3.0, "95% of top speed after {to_95:.2} s");
    // Braking (and nothing more) brings it back down one gear at a time, to first.
    let down = shifts(&mut sim, 240, &hold(Vec2::ZERO, buttons::CROUCH));
    let gears: Vec<i8> = down.iter().map(|s| s.0).collect();
    assert_eq!(gears, [4, 3, 2, 1], "shifted down in order: {down:?}");
    assert!(speed(&sim, "car") < 0.5, "stopped");
}

#[test]
fn backing_up_selects_reverse() {
    let mut sim = strip();
    // Pull back from rest (south, behind the car): reverse, and the car backs up.
    let z = sim.state.entities.find("car").unwrap().pos.z;
    let back = shifts(&mut sim, 90, &hold(Vec2::new(0.0, 1.0), 0));
    assert_eq!(back.first().map(|s| s.0), Some(-1), "into reverse: {back:?}");
    assert_eq!(gear(&sim), -1);
    assert!(sim.state.entities.find("car").unwrap().pos.z - z > 2.0, "backed up");
    // Push ahead again: first gear (the car brakes, stops and drives off forward).
    let ahead = shifts(&mut sim, 90, &hold(Vec2::new(0.0, -1.0), 0));
    assert_eq!(ahead.first().map(|s| s.0), Some(1), "out of reverse: {ahead:?}");
}

#[test]
fn the_gearbox_does_not_hunt() {
    use pav_core::vehicle::Gearbox;
    // Speed wobbling round the shift from first to second, throttle on: one shift, no more.
    let mut gb = Gearbox::default();
    let mut changes = 0;
    for t in 0..240 {
        let speed = 0.24 * 22.0 + 0.6 * (t as f32 * 0.4).sin();
        let g = gb.gear;
        gb.update(speed, 22.0, 1.0, false, 1.0 / 60.0);
        changes += (gb.gear != g) as u32;
    }
    assert_eq!((gb.gear, changes), (2, 1));
    // A shift cuts the pull for a moment, then the new gear pulls.
    let mut gb = Gearbox::default();
    assert_eq!(gb.update(5.8, 22.0, 1.0, false, 1.0 / 60.0), 0.0, "cut while shifting into second");
    assert!(gb.shifting > 0.0 && gb.gear == 2);
    for _ in 0..8 {
        gb.update(5.8, 22.0, 1.0, false, 1.0 / 60.0);
    }
    assert_eq!(gb.update(5.8, 22.0, 1.0, false, 1.0 / 60.0), pav_core::vehicle::GEAR_PULL[1], "pulls again after the cut");
    // Stopped dead by a wall in fifth with the throttle held: straight to first, pulling at once.
    let mut gb = Gearbox { gear: 5, ..Default::default() };
    let pull = gb.update(1.0, 22.0, 1.0, false, 1.0 / 60.0);
    assert_eq!((gb.gear, gb.shifting), (1, 0.0), "one step down to first, no cut");
    assert_eq!(pull, pav_core::vehicle::GEAR_PULL[0]);
    // The brake held with the throttle, or the throttle against the roll: the old one-gear pull.
    let mut gb = Gearbox::default();
    assert_eq!(gb.update(0.0, 22.0, 1.0, true, 1.0 / 60.0), 1.0, "braking: first's extra pull is off");
    let mut gb = Gearbox { gear: -1, ..Default::default() };
    gb.update(-3.0, 22.0, 1.0, false, 1.0 / 60.0);
    assert_eq!(gb.gear, 1);
    assert_eq!(gb.update(-3.0, 22.0, 1.0, false, 1.0 / 60.0), 1.0, "pushing ahead to stop a car rolling back");
}

#[test]
fn the_same_drive_shifts_the_same_way_twice() {
    let drive = |sim: &mut Sim| {
        let mut log = Vec::new();
        for (ticks, input) in [
            (150, hold(Vec2::new(0.0, -1.0), 0)),
            (40, hold(Vec2::new(0.4, -1.0), buttons::JUMP)),
            (60, hold(Vec2::new(0.0, -1.0), 0)),
            (50, hold(Vec2::ZERO, buttons::CROUCH)),
            (60, hold(Vec2::new(0.0, 1.0), 0)),
        ] {
            log.extend(shifts(sim, ticks, &input).into_iter().map(|(g, s)| (g, s.to_bits())));
        }
        (log, sim.state_hash())
    };
    let (a, b) = (drive(&mut strip()), drive(&mut strip()));
    assert!(a.0.len() >= 6, "the drive shifts: {:?}", a.0);
    assert_eq!(a, b, "same inputs, same gears at the same speeds, same state");
}
