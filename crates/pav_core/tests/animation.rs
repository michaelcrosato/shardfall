//! M6 procedural animation: NPC brains, creature rigs (planted feet), secondary chains, hit
//! recoil, foot IK on steps, body-plan switching, dormancy and rewind (tests/anim_room.toml).

use glam::{Vec2, Vec3};
use pav_core::puppet::BodyPlan;
use pav_core::room::RoomDef;
use pav_core::{InputFrame, Sim};

fn room() -> Sim {
    let def = RoomDef::parse(include_str!("anim_room.toml")).expect("room parses");
    let mut sim = Sim::empty(1);
    pav_core::scenes::build_standalone_room(&mut sim, "at", def);
    sim.state.scene = "at".into();
    sim
}

fn npc_pos(sim: &Sim, name: &str) -> Vec3 {
    sim.state.entities.find(name).unwrap_or_else(|| panic!("{name}")).pos
}

#[test]
fn npcs_walk_and_creatures_plant_their_feet() {
    let mut sim = room();
    let names = ["walker", "spider", "lizard", "beetle", "blob"];
    let start: Vec<Vec3> = names.iter().map(|n| npc_pos(&sim, n)).collect();
    sim.run(360, &InputFrame::default());
    for (n, s) in names.iter().zip(&start) {
        let p = npc_pos(&sim, n);
        assert!(p.is_finite() && p.y > -0.5, "{n} at {p}");
        assert!(Vec2::new(p.x - s.x, p.z - s.z).length() > 0.5, "{n} moved: {s} -> {p}");
    }
    let spider = sim.state.entities.find("spider").unwrap().character.as_ref().unwrap();
    let rig = spider.rig.as_ref().expect("spider rig");
    assert_eq!(rig.feet.len(), 8);
    assert!(rig.steps > 10, "spider stepped {} times", rig.steps);
    let feet = npc_pos(&sim, "spider");
    for f in &rig.feet {
        assert!(f.pos.is_finite() && Vec2::new(f.pos.x - feet.x, f.pos.z - feet.z).length() < 1.5, "foot {:?} body {feet}", f.pos);
    }
    // The cutout critter keeps its tail and antennae chains.
    let critter = sim.state.entities.find("critter").unwrap().character.as_ref().unwrap();
    assert_eq!(critter.rig.as_ref().map(|r| r.chains.len()), Some(3));
}

#[test]
fn bombs_make_characters_flinch() {
    let mut sim = room();
    sim.run(30, &InputFrame::default());
    let p = npc_pos(&sim, "critter");
    let mut events = Vec::new();
    sim.explode(p + Vec3::new(1.5, -0.5, 0.0), 1.5, &mut events);
    let mut peak = 0.0f32;
    for _ in 0..20 {
        sim.run(1, &InputFrame::default());
        let a = &sim.state.entities.find("critter").unwrap().character.as_ref().unwrap().anim;
        peak = peak.max(a.hit_side.abs() + a.hit_fwd.abs());
    }
    assert!(peak > 0.15, "flinch {peak}");
}

#[test]
fn feet_find_the_steps() {
    let mut sim = room();
    // Stand on the edge of the second step (0.5 m) with the left foot over the first (0.25 m).
    let id = sim.state.player.unwrap();
    sim.set_position(id, Vec3::new(1.05, 0.5, 3.0));
    sim.run(30, &InputFrame::default());
    let ch = sim.player().unwrap().character.as_ref().unwrap();
    assert!(ch.anim.foot_l.abs() + ch.anim.foot_r.abs() > 0.1, "feet {} {}", ch.anim.foot_l, ch.anim.foot_r);
}

#[test]
fn the_player_can_become_a_spider() {
    let mut sim = room();
    sim.config.puppet.body = BodyPlan::Spider;
    sim.run(5, &InputFrame::default());
    let x0 = sim.player().unwrap().pos.x;
    sim.run(60, &InputFrame { move_dir: Vec2::new(1.0, 0.0), ..Default::default() });
    let p = sim.player().unwrap();
    let ch = p.character.as_ref().unwrap();
    assert!(ch.height() < 1.0, "low capsule: {}", ch.height());
    assert!(p.pos.x - x0 > 3.0, "full speed: {}", p.pos.x - x0);
    assert_eq!(ch.rig.as_ref().map(|r| r.feet.len()), Some(8));
}

#[test]
fn npcs_survive_dormancy_and_rewind() {
    let def = RoomDef::parse(include_str!("anim_room.toml")).unwrap();
    let mut sim = Sim::empty(1);
    sim.build_world(vec![("at".into(), def)], Vec::new());
    sim.teleport_to_room("at");
    sim.run(60, &InputFrame::default());
    let (t, h) = (sim.state.tick, sim.state_hash());
    sim.run(90, &InputFrame { move_dir: Vec2::new(0.4, -1.0), ..Default::default() });
    let (t2, h2) = (sim.state.tick, sim.state_hash());
    assert!(sim.rewind_to(t));
    assert_eq!(sim.state_hash(), h);
    assert!(sim.rewind_to(t2));
    assert_eq!(sim.state_hash(), h2, "re-simulation with NPCs matches");

    let id = sim.state.player.unwrap();
    sim.set_position(id, Vec3::new(400.0, 20.0, 400.0));
    for _ in 0..40 {
        sim.update_streaming(usize::MAX);
        sim.run(15, &InputFrame::default());
    }
    assert!(sim.state.entities.find("spider").is_none(), "room went dormant");
    sim.teleport_to_room("at");
    let s0 = npc_pos(&sim, "spider");
    sim.run(240, &InputFrame::default());
    let s1 = npc_pos(&sim, "spider");
    assert!(s1.is_finite() && s1.y > -0.5 && (s1 - s0).length() > 0.3, "spider awake and walking: {s0} -> {s1}");
}
