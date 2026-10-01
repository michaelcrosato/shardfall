//! M9 genre mechanics: the blaster, shootable enemies (score, signals), boss phases and a boss
//! kill finishing the course, a small player hitbox, and stealth guards (seen in plain sight,
//! hidden behind a wall or by crouching). Fixture: tests/genre_room.toml.

use glam::{Vec2, Vec3};
use pav_core::character::Weapon;
use pav_core::input::buttons;
use pav_core::room::RoomDef;
use pav_core::{InputFrame, Sim};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("genre_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "gt", def);
    sim.state.scene = "gt".into();
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

fn fire_at(target: Vec3) -> InputFrame {
    InputFrame { held: buttons::PRIMARY, aim: Some(target), ..Default::default() }
}

#[test]
fn the_blaster_destroys_enemies_and_scores() {
    let mut sim = room();
    sim.config.bombs.weapon = Weapon::Blaster;
    // Start the course: walk out of the START zone.
    put(&mut sim, at(2.5, 2.5));
    sim.run(5, &InputFrame::default());
    sim.run(40, &InputFrame { move_dir: Vec2::new(0.0, 1.0), ..Default::default() });
    assert!(sim.state.courses.run.as_ref().is_some_and(|r| r.started), "course running");
    put(&mut sim, at(5.5, 9.5));
    let drone = sim.state.entities.find("drone").unwrap().pos;
    sim.run(60, &fire_at(drone));
    assert!(sim.state.entities.find("drone").is_none(), "drone destroyed");
    assert_eq!(sim.state.courses.run.as_ref().unwrap().score, 50);
    // Boss: switches pattern below half health, and its death finishes the course.
    put(&mut sim, at(24.5, 9.0));
    let mut phased = false;
    for _ in 0..400 {
        let Some(b) = sim.state.entities.find("boss") else { break };
        let p = b.pos;
        phased |= matches!(b.behavior, pav_core::entity::Behavior::Spin { .. });
        sim.run(1, &fire_at(p));
    }
    assert!(phased, "boss entered its second phase");
    assert!(sim.state.entities.find("boss").is_none(), "boss destroyed");
    let r = sim.state.courses.last.as_ref().expect("course finished by the boss");
    assert_eq!(r.score, 550);
}

#[test]
fn a_small_hitbox_lets_bullets_graze_past() {
    let mut sim = room();
    let p = at(20.5, 4.5);
    put(&mut sim, p);
    sim.run(5, &InputFrame::default());
    let feet = sim.player().unwrap().pos;
    let shoot = |sim: &mut Sim| {
        sim.state.projectiles.spawn(pav_core::projectile::Projectile {
            pos: Vec3::new(feet.x - 3.0, feet.y, feet.z + 0.3),
            vel: Vec3::X * 10.0,
            radius: 0.08,
            life: 2.0,
            color: pav_core::Color::WHITE,
            knockback: 3.0,
            gravity: 0.0,
            owner: None,
            team: Default::default(),
            damage: 1.0,
        });
        sim.run(40, &InputFrame::default());
        sim.player().unwrap().character.as_ref().unwrap().stun > 0.0 || sim.player().unwrap().pos.distance(feet) > 0.3
    };
    sim.config.movement.hitbox = 0.12;
    assert!(!shoot(&mut sim), "0.3 m off passes a 0.12 m hitbox");
    put(&mut sim, p);
    sim.run(5, &InputFrame::default());
    sim.config.movement.hitbox = 0.32;
    assert!(shoot(&mut sim), "but hits a 0.32 m one");
}

#[test]
fn guards_spot_you_in_plain_sight_but_not_behind_walls_or_low() {
    // In front of the guard (it faces south / +Z), 4 m away: spotted and sent back.
    let mut sim = room();
    put(&mut sim, at(11.5, 9.0));
    sim.run(60, &InputFrame::default());
    assert!(sim.state.courses.message.as_ref().is_some_and(|(m, _)| m.contains("SPOTTED")), "spotted in plain sight");
    // Behind the wall (row 10) at 6 m: hidden.
    let mut sim = room();
    put(&mut sim, at(11.5, 12.5));
    sim.run(90, &InputFrame::default());
    assert!(sim.state.courses.message.is_none(), "hidden behind the wall");
    // 5.4 m away, off to the side the guard keeps glancing at: crouching (range 7 x 0.6 =
    // 4.2 m) stays unseen, standing gets spotted.
    let mut sim = room();
    put(&mut sim, at(16.0, 9.5));
    sim.run(240, &InputFrame { held: buttons::CROUCH, ..Default::default() });
    assert!(sim.state.courses.message.is_none(), "crouching out of reach");
    let mut sim = room();
    put(&mut sim, at(16.0, 9.5));
    sim.run(240, &InputFrame::default());
    assert!(sim.state.courses.message.is_some(), "standing there is seen");
}

#[test]
fn the_boss_bar_only_shows_in_the_bosses_room() {
    let mut sim = Sim::new("world", 1).unwrap();
    assert!(sim.teleport_to_room("bullet_hell"));
    sim.run(30, &InputFrame::default());
    assert!(sim.boss_bar().is_some(), "bullet hell shows its boss");
    assert!(sim.teleport_to_room("stealth"));
    sim.run(30, &InputFrame::default());
    assert!(sim.boss_bar().is_none(), "other rooms do not");
}
