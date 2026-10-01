//! Shardfall juice that is also rules: crushing blows burst monsters into physics chunks, kill
//! streaks pay bonus experience, and a boss makes an entrance the first time it sees you.

use glam::Vec3;
use pav_core::arpg::combat::{Damage, Rarity};
use pav_core::arpg::data::data;
use pav_core::{InputFrame, Sim};

fn quiet_arena(seed: u64) -> Sim {
    let mut sim = Sim::new("arena", seed).unwrap();
    sim.state.game.as_mut().unwrap().arena = None;
    sim.run(3, &InputFrame::default());
    sim
}

fn hit(sim: &mut Sim, target: pav_core::EntityId, amount: f32, crit: bool) {
    let mut g = sim.state.game.take().unwrap();
    let dmg = Damage {
        amount: [amount, 0.0, 0.0, 0.0, 0.0],
        crit,
        ailment: [0.0; 5],
        ailment_mult: 1.0,
        knockback: 1.0,
        source: g.hero_id,
        skill: 0,
        attack: true,
        melee: true,
    };
    let mut ev = Vec::new();
    pav_core::arpg::debug_hit(sim, &mut g, target, &dmg, &mut ev);
    sim.state.game = Some(g);
}

fn ghoul(sim: &mut Sim, at: Vec3) -> pav_core::EntityId {
    let d = data();
    let spec = d.family("ghoul").unwrap().spec();
    let mut g = sim.state.game.take().unwrap();
    let id = pav_core::arpg::spawn_spec_into(sim, &mut g, &spec, 1, Rarity::Normal, at, 50).unwrap();
    sim.state.game = Some(g);
    id
}

#[test]
fn crushing_blows_burst_monsters_into_chunks() {
    let mut sim = quiet_arena(1);
    let id = ghoul(&mut sim, Vec3::new(4.0, 0.1, 0.0));
    hit(&mut sim, id, 1.0e6, true);
    let gibs = sim.state.entities.iter().filter(|e| e.name == "~gib").count();
    assert!(gibs >= 4, "{gibs} chunks");
    sim.run(60 * 4, &InputFrame::default());
    assert_eq!(sim.state.entities.iter().filter(|e| e.name == "~gib").count(), 0, "they fade away");
}

#[test]
fn kill_streaks_pay_a_bonus() {
    let mut sim = quiet_arena(2);
    let ids: Vec<_> = (0..10).map(|i| ghoul(&mut sim, Vec3::new(3.0 + i as f32, 0.1, 5.0))).collect();
    let xp0 = sim.state.game.as_ref().unwrap().hero.xp;
    for id in &ids {
        hit(&mut sim, *id, 1.0e6, false);
        sim.run(10, &InputFrame::default());
    }
    let mid = sim.state.game.as_ref().unwrap().hero.xp;
    sim.run(120, &InputFrame::default());
    let g = sim.state.game.as_ref().unwrap();
    assert!(g.hero.level > 1 || g.hero.xp > mid, "a bonus came after the chain ({xp0} -> {mid} -> {})", g.hero.xp);
    assert!(g.floaters.iter().any(|f| f.text.starts_with("Rampage")) || g.hero.level > 1);
}

#[test]
fn a_boss_makes_an_entrance() {
    let mut sim = quiet_arena(3);
    let mut g = sim.state.game.take().unwrap();
    let id = g.spawn_boss(&mut sim, "hollow_king", 10, Vec3::new(0.0, 0.1, -17.0)).unwrap();
    g.actors.get_mut(&id).unwrap().brain.as_mut().unwrap().aggro = false;
    sim.state.game = Some(g);
    sim.run(2, &InputFrame::default());
    assert!(!sim.state.game.as_ref().unwrap().actors[&id].boss.as_ref().unwrap().met);
    sim.state.game.as_mut().unwrap().actors.get_mut(&id).unwrap().brain.as_mut().unwrap().aggro = true;
    sim.run(2, &InputFrame::default());
    let g = sim.state.game.as_ref().unwrap();
    assert!(g.actors[&id].boss.as_ref().unwrap().met);
    assert!(g.message.as_ref().is_some_and(|m| m.0.contains("Hollow King")), "{:?}", g.message);
}
