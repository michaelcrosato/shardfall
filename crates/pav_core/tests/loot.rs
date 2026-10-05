//! Shardfall loot and the town: monsters drop items and gold, the hero equips gear, sells and
//! buys at the smith, stores things in the stash, travels between places, and unique powers
//! work. Everything happens through input-frame commands, so it all replays exactly.

use glam::{Vec2, Vec3};
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use pav_core::arpg::items::{EquipSlot, RollSpec, roll_item, unique_item};
use pav_core::arpg::{GameCmd, Place, SpotKind};
use pav_core::input::buttons;
use pav_core::{InputFrame, Sim};

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

fn cmd(sim: &mut Sim, c: GameCmd) {
    sim.step(&InputFrame { cmd: Some(c), ..Default::default() });
}

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

/// Walks to the nearest monster and attacks it.
fn fight(sim: &Sim) -> InputFrame {
    let g = game(sim);
    let me = hero_feet(sim);
    let target = g
        .actors
        .iter()
        .filter(|(_, a)| a.team == Team::Monster && !a.dead)
        .filter_map(|(id, _)| sim.state.entities.get(*id).map(|e| e.pos))
        .min_by(|a, b| (*a - me).length().total_cmp(&(*b - me).length()));
    // No monsters: walk over the loot.
    let target = target.or_else(|| g.loot.first().map(|l| l.pos));
    let Some(t) = target else { return InputFrame::default() };
    let d = Vec2::new(t.x - me.x, t.z - me.z);
    let mut f = InputFrame { aim: Some(t), ..Default::default() };
    if d.length() > 1.6 || g.monsters_alive() == 0 {
        f.move_dir = d.normalize_or_zero();
    } else {
        f.held = buttons::PRIMARY;
        f.pressed = buttons::PRIMARY;
    }
    f
}

fn give(sim: &mut Sim, rarity: Rarity, level: u32, slot: Option<pav_core::arpg::items::Slot>) -> u32 {
    let d = data();
    let g = sim.state.game.as_mut().unwrap();
    let id = g.hero.new_id();
    let it = roll_item(&d, &mut sim.state.rng, RollSpec { level, rarity: Some(rarity), slot, rarity_bonus: 0.0 }, id).unwrap();
    let g = sim.state.game.as_mut().unwrap();
    g.hero.inventory.push(it);
    id
}

#[test]
fn monsters_drop_loot_that_gets_picked_up() {
    let mut sim = Sim::new("arena", 11).unwrap();
    sim.state.game.as_mut().unwrap().loot_rate = 6.0;
    sim.state.game.as_mut().unwrap().auto_loot = Rarity::Normal;
    for _ in 0..60 * 45 {
        let f = fight(&sim);
        sim.step(&f);
    }
    let g = game(&sim);
    assert!(g.hero.kills >= 6, "kills {}", g.hero.kills);
    assert!(!g.hero.inventory.is_empty() || !g.loot.is_empty(), "something dropped");
    assert!(!g.hero.inventory.is_empty(), "walked over loot and picked it up ({} on the ground)", g.loot.len());
    assert!(g.hero.gold > 0, "gold flew to the hero");
    let ev = sim.drain_events();
    let _ = ev;
}

#[test]
fn equipping_changes_the_hero() {
    let mut sim = Sim::new("arena", 12).unwrap();
    sim.run(3, &InputFrame::default());
    let d = data();
    let before = game(&sim).hero.weapon.clone();
    // A level-40 greatsword: much more damage, two hands.
    let id = {
        let g = sim.state.game.as_mut().unwrap();
        let id = g.hero.new_id();
        let mut it = pav_core::arpg::items::Item::plain(id, d.base("executioner").unwrap(), 40);
        it.rarity = Rarity::Normal;
        g.hero.inventory.push(it);
        id
    };
    let shield = give(&mut sim, Rarity::Magic, 10, Some(pav_core::arpg::items::Slot::Offhand));
    cmd(&mut sim, GameCmd::Equip(shield));
    assert!(game(&sim).hero.worn(EquipSlot::Offhand).is_some());
    cmd(&mut sim, GameCmd::Equip(id));
    let g = game(&sim);
    assert_eq!(g.hero.weapon.kind, pav_core::puppet::WeaponKind::Greatsword);
    assert!(g.hero.weapon.phys[1] > before.phys[1] * 5.0, "{:?} -> {:?}", before.phys, g.hero.weapon.phys);
    assert!(g.hero.worn(EquipSlot::Offhand).is_none(), "two-hander took both hands");
    assert_eq!(g.hero.inventory.len(), 2, "old sword and the shield went to the bag");
    let look = sim.player().unwrap().character.as_ref().unwrap().puppet.clone().unwrap();
    assert_eq!(look.weapon.kind, pav_core::puppet::WeaponKind::Greatsword);
    // Armour shows: a helmet.
    let helm = give(&mut sim, Rarity::Rare, 30, Some(pav_core::arpg::items::Slot::Helmet));
    let armor_before = game(&sim).hero_actor().unwrap().sheet.armor;
    cmd(&mut sim, GameCmd::Equip(helm));
    let g = game(&sim);
    assert!(g.hero_actor().unwrap().sheet.armor > armor_before);
    let look = sim.player().unwrap().character.as_ref().unwrap().puppet.clone().unwrap();
    assert_ne!(look.gear.helm, pav_core::puppet::HelmKind::None);
}

#[test]
fn town_trade_stash_and_travel() {
    let mut sim = Sim::new("arena", 13).unwrap();
    sim.run(3, &InputFrame::default());
    {
        let g = sim.state.game.as_mut().unwrap();
        g.hero.gold = 100_000;
        g.hero.level = 12;
    }
    let junk = give(&mut sim, Rarity::Normal, 5, None);
    let keep = give(&mut sim, Rarity::Rare, 12, None);
    // Selling needs a vendor: not in the arena.
    cmd(&mut sim, GameCmd::Sell(junk));
    assert!(game(&sim).hero.inventory.iter().any(|i| i.id == junk));
    // Home.
    cmd(&mut sim, GameCmd::Travel(Place::Town.code()));
    assert_eq!(sim.state.scene, "town");
    let g = game(&sim);
    assert_eq!(g.place, Place::Town);
    assert_eq!(g.hero.level, 12, "the hero came along");
    assert!(g.hero.inventory.iter().any(|i| i.id == keep), "with their bag");
    assert_eq!(g.vendor.len(), 12, "the smith has wares");
    let smith = g.spots.iter().find(|s| s.kind == SpotKind::Vendor).unwrap().pos;
    let stash = g.spots.iter().find(|s| s.kind == SpotKind::Stash).unwrap().pos;
    let hid = g.hero_id.unwrap();
    sim.set_position(hid, smith + Vec3::new(1.5, 1.0, 0.0));
    sim.run(5, &InputFrame::default());
    let gold = game(&sim).hero.gold;
    cmd(&mut sim, GameCmd::Sell(junk));
    let g = game(&sim);
    assert!(!g.hero.inventory.iter().any(|i| i.id == junk), "sold");
    assert!(g.hero.gold > gold);
    let ware = g.vendor[0].id;
    cmd(&mut sim, GameCmd::Buy(ware));
    assert!(game(&sim).hero.inventory.iter().any(|i| i.id == ware), "bought");
    // Buy back what was sold.
    cmd(&mut sim, GameCmd::Buy(junk));
    assert!(game(&sim).hero.inventory.iter().any(|i| i.id == junk), "bought back");
    // The stash keeps things.
    sim.set_position(hid, stash + Vec3::new(-1.5, 1.0, 0.0));
    sim.run(5, &InputFrame::default());
    cmd(&mut sim, GameCmd::Stash(keep));
    assert!(game(&sim).hero.stash.iter().any(|i| i.id == keep));
    // Out to the arena and back: the stash stays, waves start.
    cmd(&mut sim, GameCmd::Travel(Place::Arena.code()));
    assert_eq!(sim.state.scene, "arena");
    sim.run(60 * 4, &InputFrame::default());
    assert!(game(&sim).monsters_alive() > 0, "waves");
    cmd(&mut sim, GameCmd::Travel(Place::Town.code()));
    assert!(game(&sim).hero.stash.iter().any(|i| i.id == keep));
    assert_eq!(game(&sim).monsters_alive(), 0, "monsters stayed behind");
}

#[test]
fn rewind_and_replay_across_travel_are_exact() {
    let mut sim = Sim::new("arena", 14).unwrap();
    sim.history.enabled = true;
    for _ in 0..60 * 3 {
        let f = fight(&sim);
        sim.step(&f);
    }
    let it = give(&mut sim, Rarity::Magic, 3, Some(pav_core::arpg::items::Slot::Boots));
    let _ = it;
    cmd(&mut sim, GameCmd::Travel(0));
    for _ in 0..90 {
        sim.step(&InputFrame { move_dir: Vec2::new(0.3, -1.0), ..Default::default() });
    }
    let tick = sim.state.tick;
    let hash = sim.state_hash();
    assert!(sim.rewind_to(tick - 120), "back to before the trip");
    assert_eq!(sim.state.scene, "arena");
    assert!(sim.rewind_to(tick));
    assert_eq!(sim.state.scene, "town");
    assert_eq!(sim.state_hash(), hash);
}

#[test]
fn unique_powers_work() {
    let mut sim = Sim::new("arena", 15).unwrap();
    sim.run(3, &InputFrame::default());
    let d = data();
    // Whirling Orrery: blades circle the hero and cut a monster standing next to them.
    let id = {
        let g = sim.state.game.as_mut().unwrap();
        let id = g.hero.new_id();
        let it = unique_item(&d, &mut sim.state.rng, d.unique("whirling_orrery").unwrap(), 30, id);
        sim.state.game.as_mut().unwrap().hero.inventory.push(it);
        id
    };
    cmd(&mut sim, GameCmd::Equip(id));
    sim.run(2, &InputFrame::default());
    let blades = game(&sim).shots.iter().filter(|s| s.orbit.is_some()).count();
    assert_eq!(blades, 3, "three blades orbit");
    let at = hero_feet(&sim) + Vec3::new(3.0, 0.0, 0.0);
    let m = sim.spawn_monster("ghoul", 1, Rarity::Normal, at, 99).unwrap();
    let life0 = game(&sim).actors[&m].life;
    sim.run(90, &InputFrame::default());
    let g = game(&sim);
    assert!(g.actors.get(&m).is_none_or(|a| a.life < life0 || a.dead), "the blades cut it");
    // Unequipping removes them.
    cmd(&mut sim, GameCmd::Unequip(EquipSlot::Amulet.index() as u8));
    sim.run(2, &InputFrame::default());
    assert_eq!(game(&sim).shots.iter().filter(|s| s.orbit.is_some()).count(), 0);
}

#[test]
fn the_bot_picks_up_loot_nearby() {
    // Without this the bot fought level 4's boss in its starting gear (campaign runs).
    let mut sim = Sim::new("level/1", 3).unwrap();
    let at = hero_feet(&sim) + Vec3::new(0.0, 0.5, 4.0);
    let d = data();
    let mut g = sim.state.game.take().unwrap();
    let id = g.hero.new_id();
    let it =
        roll_item(&d, &mut sim.state.rng, RollSpec { level: 1, rarity: Some(Rarity::Normal), slot: None, rarity_bonus: 0.0 }, id)
            .unwrap();
    g.auto_loot = Rarity::Normal;
    g.drop_item(&mut sim, it, at, &mut Vec::new());
    sim.state.game = Some(g);
    let mut bot = pav_core::arpg::bot::Bot::default();
    let stats = bot.run(&mut sim, 60 * 6);
    let g = game(&sim);
    assert!(
        g.hero.inventory.iter().chain(EquipSlot::ALL.iter().filter_map(|s| g.hero.worn(*s))).any(|i| i.id == id),
        "the bot left the item on the ground: {:?} hero {:?} {stats:?}",
        g.loot.iter().map(|l| (l.item.id, l.pos, l.rest)).collect::<Vec<_>>(),
        hero_feet(&sim)
    );
}
