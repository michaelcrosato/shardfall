//! Shared-world co-op (arpg::party): several heroes in one world, each driven by its own
//! input, and the same inputs giving the same world (what keeps players' copies together).

use glam::Vec2;
use pav_core::InputFrame;
use pav_core::arpg::round::round_sim;
use pav_core::input::buttons;

fn walk(x: f32, z: f32) -> InputFrame {
    InputFrame { move_dir: Vec2::new(x, z), ..Default::default() }
}

#[test]
fn party_heroes_walk_by_their_own_input() {
    let mut sim = round_sim(42, 1, "wanderer");
    sim.add_party_hero(1, "kestrel");
    sim.add_party_hero(2, "ysolde");
    let ids = sim.state.game.as_ref().unwrap().hero_ids();
    assert_eq!(ids.len(), 3, "three heroes");
    let start: Vec<_> = ids.iter().map(|id| sim.feet(*id).unwrap()).collect();
    for _ in 0..90 {
        sim.set_party_input(1, walk(1.0, 0.0));
        sim.set_party_input(2, walk(-1.0, 0.0));
        sim.step(&InputFrame::default());
    }
    let end: Vec<_> = ids.iter().map(|id| sim.feet(*id).unwrap()).collect();
    assert!((end[0] - start[0]).length() < 0.6, "the host stood still: {:?} -> {:?}", start[0], end[0]);
    assert!(end[1].x - start[1].x > 1.5, "seat 1 walked east: {:?} -> {:?}", start[1], end[1]);
    assert!(start[2].x - end[2].x > 1.5, "seat 2 walked west: {:?} -> {:?}", start[2], end[2]);
}

#[test]
fn a_party_member_attacks_and_gets_back_up() {
    let mut sim = round_sim(5, 1, "wanderer");
    sim.add_party_hero(1, "kestrel");
    let id = sim.state.game.as_ref().unwrap().party[0].id.unwrap();
    // Swinging at the air: the member's own cast starts and mana or cooldowns move.
    let aim = sim.feet(id).unwrap() + glam::Vec3::X * 2.0;
    sim.set_party_input(
        1,
        InputFrame { aim: Some(aim), held: buttons::PRIMARY, pressed: buttons::PRIMARY, ..Default::default() },
    );
    sim.step(&InputFrame::default());
    let g = sim.state.game.as_ref().unwrap();
    assert!(g.actors[&id].cast.is_some(), "the member's swing started");
    // Down, then up again at the start a few seconds later.
    let mut g = sim.state.game.take().unwrap();
    g.actors.get_mut(&id).unwrap().iframes = 0.0;
    let dmg = pav_core::arpg::combat::Damage { amount: [1.0e6, 0.0, 0.0, 0.0, 0.0], ..Default::default() };
    pav_core::arpg::debug_hit(&mut sim, &mut g, id, &dmg, &mut Vec::new());
    assert!(g.actors[&id].dead && g.party[0].respawn > 0.0, "the member is down with a timer of their own");
    assert!(!g.actors[&g.hero_id.unwrap()].dead, "the host's hero is fine");
    sim.state.game = Some(g);
    for _ in 0..240 {
        sim.step(&InputFrame::default());
    }
    assert!(!sim.state.game.as_ref().unwrap().actors[&id].dead, "the member got back up");
}

#[test]
fn same_inputs_same_world() {
    let run = || {
        let mut sim = round_sim(9, 1, "wanderer");
        sim.add_party_hero(1, "kestrel");
        let mut hashes = Vec::new();
        for t in 0..600u32 {
            let a = (t as f32 * 0.03).sin();
            let host = InputFrame {
                move_dir: Vec2::new(a, 0.3),
                held: if t % 40 < 8 { buttons::PRIMARY } else { 0 },
                ..Default::default()
            };
            sim.set_party_input(1, walk(-a, (t as f32 * 0.05).cos()));
            sim.step(&host);
            if t % 100 == 99 {
                hashes.push(sim.world_hash());
            }
        }
        hashes
    };
    assert_eq!(run(), run());
}
