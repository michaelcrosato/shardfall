//! Emberwatch: the townsfolk are there and alive (villagers walk their rounds, the dog
//! follows the hero, the gambler's coin flies), the gambler sells mystery items, the alchemist
//! brews potion upgrades, and the portal lists the depths reached.

use glam::Vec3;
use pav_core::arpg::scene::NpcRole;
use pav_core::arpg::{GameCmd, Place, SpotKind};
use pav_core::{InputFrame, Sim};

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

fn feet(sim: &Sim, id: pav_core::EntityId) -> Vec3 {
    let e = sim.state.entities.get(id).unwrap();
    e.pos - Vec3::Y * e.character.as_ref().unwrap().height() * 0.5
}

fn npc(sim: &Sim, role: NpcRole) -> pav_core::EntityId {
    game(sim).npcs.iter().find(|n| n.role == role).unwrap().id
}

fn walk_to(sim: &mut Sim, at: Vec3) {
    let id = game(sim).hero_id.unwrap();
    sim.set_position(id, at);
    sim.run(2, &InputFrame::default());
}

#[test]
fn townsfolk_live_their_lives() {
    let mut sim = Sim::new("town", 1).unwrap();
    let roles: Vec<NpcRole> = game(&sim).npcs.iter().map(|n| n.role).collect();
    for r in [NpcRole::Smith, NpcRole::Gambler, NpcRole::Alchemist, NpcRole::Captain, NpcRole::Villager, NpcRole::Dog] {
        assert!(roles.contains(&r), "{r:?} in town");
    }
    let villager = npc(&sim, NpcRole::Villager);
    let dog = npc(&sim, NpcRole::Dog);
    let v0 = feet(&sim, villager);
    // The hero walks off east; the dog follows.
    let hero = game(&sim).hero_id.unwrap();
    sim.set_position(hero, Vec3::new(12.0, 0.05, 6.0));
    sim.run(60 * 6, &InputFrame::default());
    assert!((feet(&sim, villager) - v0).length() > 2.0, "the villager walked their round");
    let d = (feet(&sim, dog) - feet(&sim, hero)).length();
    assert!(d < 4.0, "the dog came along: {d} m away");
    // The gambler's coin is in the air at some point.
    let coin = game(&sim).npcs.iter().find(|n| n.role == NpcRole::Gambler).unwrap().prop.unwrap();
    let mut top: f32 = 0.0;
    let base = sim.state.entities.get(coin).unwrap().pos.y;
    for _ in 0..200 {
        sim.step(&InputFrame::default());
        top = top.max(sim.state.entities.get(coin).unwrap().pos.y - base);
    }
    assert!(top > 0.5, "coin flipped {top} m");
}

#[test]
fn gambling_and_brewing() {
    let mut sim = Sim::new("town", 2).unwrap();
    sim.state.game.as_mut().unwrap().hero.gold = 100_000;
    let g = game(&sim);
    let gamble = g.spots.iter().find(|s| s.kind == SpotKind::Gamble).unwrap().pos;
    let brew = g.spots.iter().find(|s| s.kind == SpotKind::Alchemist).unwrap().pos;
    // Too far: nothing.
    sim.step(&InputFrame { cmd: Some(GameCmd::Gamble(0)), ..Default::default() });
    assert!(game(&sim).hero.inventory.is_empty());
    walk_to(&mut sim, gamble + Vec3::new(0.0, 0.05, 1.0));
    for slot in 0..9u8 {
        sim.step(&InputFrame { cmd: Some(GameCmd::Gamble(slot)), ..Default::default() });
    }
    let g = game(&sim);
    assert_eq!(g.hero.inventory.len(), 9, "one item per slot");
    assert!(g.hero.gold < 100_000);
    walk_to(&mut sim, brew + Vec3::new(0.5, 0.05, 0.5));
    let pots = game(&sim).hero.potion_max;
    sim.step(&InputFrame { cmd: Some(GameCmd::Brew(0)), ..Default::default() });
    sim.step(&InputFrame { cmd: Some(GameCmd::Brew(1)), ..Default::default() });
    let g = game(&sim);
    assert_eq!(g.hero.potion_max, pots + 1);
    assert!(g.hero_actor().unwrap().sheet.potion > 1.15, "stronger potions");
    for _ in 0..10 {
        sim.step(&InputFrame { cmd: Some(GameCmd::Brew(0)), ..Default::default() });
    }
    assert_eq!(game(&sim).hero.potion_max, 6, "six at most");
}

#[test]
fn the_portal_goes_down_to_the_levels() {
    let mut sim = Sim::new("town", 3).unwrap();
    sim.step(&InputFrame { cmd: Some(GameCmd::Travel(Place::Level(1).code())), ..Default::default() });
    let g = game(&sim);
    assert_eq!(g.place, Place::Level(1), "level 1 is always open");
    assert!(g.level.is_some());
    assert!(g.npcs.is_empty(), "the townsfolk stay home");
    sim.step(&InputFrame { cmd: Some(GameCmd::Travel(Place::Town.code())), ..Default::default() });
    assert_eq!(game(&sim).npcs.len(), 7);
    assert!(game(&sim).level.is_none());
}
