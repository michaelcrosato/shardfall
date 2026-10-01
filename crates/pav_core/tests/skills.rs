//! Shardfall builds: every hero skill works against monsters, tree tweaks change skills,
//! channels last while held, war cries buff, blink moves, the passive tree allocates /
//! refunds / respecs through commands, keystones and masteries apply, and a bot that spends
//! its points plays on.

use glam::{Vec2, Vec3};
use pav_core::arpg::GameCmd;
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use pav_core::arpg::tree::node_id;
use pav_core::input::buttons;
use pav_core::{InputFrame, Sim};

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

fn cmd(sim: &mut Sim, c: GameCmd) {
    sim.step(&InputFrame { cmd: Some(c), ..Default::default() });
}

/// A quiet arena (no waves) with a strong hero.
fn arena(seed: u64) -> Sim {
    let mut sim = Sim::new("arena", seed).unwrap();
    {
        let g = sim.state.game.as_mut().unwrap();
        g.arena = None;
        g.hero.level = 30;
        g.hero.gold = 1_000_000;
    }
    let mut g = sim.state.game.take().unwrap();
    pav_core::arpg::refresh_hero(&mut sim, &mut g, true);
    sim.state.game = Some(g);
    sim.run(3, &InputFrame::default());
    sim
}

fn monster_life(sim: &Sim) -> f32 {
    game(sim).actors.values().filter(|a| a.team == Team::Monster).map(|a| if a.dead { 0.0 } else { a.life }).sum()
}

#[test]
fn every_hero_skill_hurts_monsters() {
    let d = data();
    let skills = d.hero_skills();
    assert_eq!(skills.len(), 16, "sixteen hero skills");
    for id in skills {
        let def = d.skill(id).clone();
        let mut sim = arena(40 + id as u64);
        // Put the skill on the left mouse button.
        sim.state.game.as_mut().unwrap().hero.bar[0] = def.key.clone();
        let at = hero_feet(&sim) + Vec3::new(3.2, 0.0, 0.0);
        for i in 0..3 {
            let m =
                sim.spawn_monster("bonecrusher", 5, Rarity::Normal, at + Vec3::new(0.0, 0.0, i as f32 * 0.9 - 0.9), 7).unwrap();
            if let Some(b) = sim.state.game.as_mut().unwrap().actors.get_mut(&m).and_then(|a| a.brain.as_mut()) {
                b.aggro = false;
            }
        }
        let before = monster_life(&sim);
        let aim = at + Vec3::Y;
        for t in 0..150 {
            let held = buttons::PRIMARY;
            let pressed = if t % 20 == 0 { buttons::PRIMARY } else { 0 };
            sim.step(&InputFrame { aim: Some(aim), held, pressed, ..Default::default() });
            let g = sim.state.game.as_mut().unwrap();
            if let Some(h) = g.hero_actor().map(|h| h.sheet.mana_max) {
                let hid = g.hero_id.unwrap();
                g.actors.get_mut(&hid).unwrap().mana = h;
            }
        }
        let after = monster_life(&sim);
        // (War cries mostly buff; a shove is enough.)
        let need = if def.behavior == pav_core::arpg::data::Behavior::Buff { 0.999 } else { 0.97 };
        assert!(after < before * need, "{} did no damage ({before} -> {after})", def.key);
    }
}

#[test]
fn tree_tweaks_change_skills() {
    let mut sim = arena(60);
    let d = data();
    let fireball = d.skill_id("fireball").unwrap();
    // Twin Flames: two fireballs per cast.
    let twin = node_id("arcana.skill.fireball.1");
    let path = d.tree.path_to(&game(&sim).hero.tree, twin).unwrap();
    for p in path {
        cmd(&mut sim, GameCmd::Allocate(p));
    }
    assert!(game(&sim).hero.tree.contains(&twin));
    let h = game(&sim).hero_actor().unwrap();
    assert_eq!(pav_core::arpg::skills::skill_of(h, fireball).count, 2);
    sim.state.game.as_mut().unwrap().hero.bar[1] = "fireball".into();
    let f = InputFrame {
        aim: Some(hero_feet(&sim) + Vec3::new(8.0, 0.0, 0.0)),
        held: buttons::SECONDARY,
        pressed: buttons::SECONDARY,
        ..Default::default()
    };
    sim.step(&f);
    for _ in 0..40 {
        sim.step(&InputFrame::default());
        if !game(&sim).shots.is_empty() {
            break;
        }
    }
    assert_eq!(game(&sim).shots.len(), 2, "two fireballs");
}

#[test]
fn whirlwind_lasts_while_held() {
    let mut sim = arena(61);
    sim.state.game.as_mut().unwrap().hero.bar[2] = "whirlwind".into();
    let hold = InputFrame { held: buttons::SKILL3, pressed: buttons::SKILL3, ..Default::default() };
    sim.step(&hold);
    for _ in 0..90 {
        sim.step(&InputFrame { held: buttons::SKILL3, ..Default::default() });
    }
    let c = game(&sim).hero_actor().unwrap().cast.clone().expect("still spinning");
    assert!(c.pulses >= 4, "pulses {}", c.pulses);
    sim.step(&InputFrame::default());
    assert!(game(&sim).hero_actor().unwrap().cast.is_none(), "let go: stops");
}

#[test]
fn war_cry_buffs_and_blink_moves() {
    let mut sim = arena(62);
    {
        let g = sim.state.game.as_mut().unwrap();
        g.hero.bar[3] = "war_cry".into();
        g.hero.bar[4] = "blink".into();
    }
    let dmg = game(&sim).hero_actor().unwrap().sheet.mods.get(pav_core::arpg::stats::Stat::DamageInc);
    sim.step(&InputFrame { held: buttons::SKILL4, pressed: buttons::SKILL4, ..Default::default() });
    sim.run(40, &InputFrame::default());
    let h = game(&sim).hero_actor().unwrap();
    assert!(h.buffs.iter().any(|b| b.name == "War Cry"));
    assert!(h.sheet.mods.get(pav_core::arpg::stats::Stat::DamageInc) > dmg + 20.0);
    let before = hero_feet(&sim);
    let target = before + Vec3::new(0.0, 0.0, 6.0);
    sim.step(&InputFrame { aim: Some(target), held: buttons::SKILL5, pressed: buttons::SKILL5, ..Default::default() });
    sim.run(30, &InputFrame::default());
    let after = hero_feet(&sim);
    assert!((after - before).length() > 4.5, "blinked {before} -> {after}");
}

#[test]
fn passive_tree_through_commands() {
    let mut sim = arena(63);
    let d = data();
    let t = &d.tree;
    let points = game(&sim).hero.points();
    assert_eq!(points, 29);
    // Not adjacent: refused.
    cmd(&mut sim, GameCmd::Allocate(node_id("might.road.3")));
    assert!(game(&sim).hero.tree.is_empty());
    // The road to Unbowed (keystone) and its power.
    let key = node_id("might.keystone");
    for p in t.path_to(&game(&sim).hero.tree, key).unwrap() {
        cmd(&mut sim, GameCmd::Allocate(p));
    }
    let h = game(&sim).hero_actor().unwrap();
    assert!(h.powers.iter().any(|p| p.kind == pav_core::arpg::powers::PowerKind::StandFirm));
    let life = h.sheet.life_max;
    // A wheel's mastery: allocate through the notable, then choose Vitality (+10% life).
    let mastery = node_id("might.wheel0.mastery");
    for p in t.path_to(&game(&sim).hero.tree, mastery).unwrap() {
        cmd(&mut sim, GameCmd::Allocate(p));
    }
    cmd(&mut sim, GameCmd::Mastery(mastery, 0));
    assert!(game(&sim).hero_actor().unwrap().sheet.life_max > life * 1.08);
    // The same option can't be taken twice in the sector.
    let other = node_id("might.wheel1.mastery");
    for p in t.path_to(&game(&sim).hero.tree, other).unwrap() {
        cmd(&mut sim, GameCmd::Allocate(p));
    }
    cmd(&mut sim, GameCmd::Mastery(other, 0));
    assert!(!game(&sim).hero.masteries.contains_key(&other));
    // Refunding the middle of the road is refused; the keystone can go back (for gold).
    let gold = game(&sim).hero.gold;
    cmd(&mut sim, GameCmd::Refund(node_id("might.road.1")));
    assert!(game(&sim).hero.tree.contains(&node_id("might.road.1")));
    cmd(&mut sim, GameCmd::Refund(key));
    assert!(!game(&sim).hero.tree.contains(&key));
    assert!(game(&sim).hero.gold < gold);
    assert!(!game(&sim).hero_actor().unwrap().powers.iter().any(|p| p.kind == pav_core::arpg::powers::PowerKind::StandFirm));
    // A full reset.
    cmd(&mut sim, GameCmd::Respec);
    assert!(game(&sim).hero.tree.is_empty() && game(&sim).hero.masteries.is_empty());
    assert_eq!(game(&sim).hero.points(), 29);
}

#[test]
fn avatar_of_flame_converts_damage() {
    let mut sim = arena(64);
    let d = data();
    let key = node_id("bridge.bulwark.might.keystone");
    assert!(d.tree.node(key).is_some_and(|n| n.name == "Avatar of Flame"));
    for p in d.tree.path_to(&game(&sim).hero.tree, key).unwrap() {
        cmd(&mut sim, GameCmd::Allocate(p));
    }
    assert!(game(&sim).hero.tree.contains(&key), "{} points", game(&sim).hero.points());
    let at = hero_feet(&sim) + Vec3::new(1.6, 0.0, 0.0);
    let m = sim.spawn_monster("bonecrusher", 3, Rarity::Normal, at, 9).unwrap();
    let hid = game(&sim).hero_id.unwrap();
    let def = pav_core::arpg::skills::skill_of(game(&sim).actors.get(&hid).unwrap(), d.skill_id("slash").unwrap());
    let g = sim.state.game.take().unwrap();
    let dmg = pav_core::arpg::skills::roll_damage(&g, &mut sim, hid, &def, 1.0);
    sim.state.game = Some(g);
    assert_eq!(dmg.amount[0], 0.0, "no physical left");
    assert!(dmg.amount[1] > 0.0, "all fire");
    let _ = m;
}

#[test]
fn a_bot_spends_points_and_keeps_winning() {
    let mut sim = Sim::new("arena", 65).unwrap();
    let mut bot = pav_core::arpg::bot::Bot::default();
    let stats = bot.run(&mut sim, 60 * 70);
    let g = game(&sim);
    assert!(stats.kills >= 15, "{stats:?}");
    assert!(g.hero.level >= 3, "level {}", g.hero.level);
    assert_eq!(g.hero.points(), 0, "spent all points: {:?}", g.hero.tree.len());
    assert!(!g.hero.tree.is_empty());
    let _ = Vec2::ZERO;
}
