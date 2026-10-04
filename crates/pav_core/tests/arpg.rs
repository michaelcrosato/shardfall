//! Shardfall combat core: the hero fights arena waves with a simple bot, monsters hit back,
//! skills cost mana and cool down, dodging gives invulnerability, and rewind stays exact.

use glam::{Vec2, Vec3};
use pav_core::arpg::combat::Team;
use pav_core::character::Weapon;
use pav_core::input::buttons;
use pav_core::{InputFrame, Sim, SimEvent};

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

/// Walks to the nearest monster and attacks it.
fn bot(sim: &Sim, button: u32) -> InputFrame {
    let g = sim.state.game.as_ref().unwrap();
    let me = hero_feet(sim);
    let target = g
        .actors
        .iter()
        .filter(|(_, a)| a.team == Team::Monster && !a.dead)
        .filter_map(|(id, _)| sim.state.entities.get(*id).map(|e| e.pos))
        .min_by(|a, b| (*a - me).length().total_cmp(&(*b - me).length()));
    let Some(t) = target else { return InputFrame::default() };
    let d = Vec2::new(t.x - me.x, t.z - me.z);
    let mut f = InputFrame { aim: Some(t), ..Default::default() };
    if d.length() > 2.0 {
        f.move_dir = d.normalize();
    } else {
        f.held = button;
        f.pressed = button;
    }
    f
}

#[test]
fn the_hero_clears_arena_waves_and_levels_up() {
    let mut sim = Sim::new("arena", 1).unwrap();
    for _ in 0..60 * 50 {
        let f = bot(&sim, buttons::PRIMARY);
        sim.step(&f);
    }
    let g = sim.state.game.as_ref().unwrap();
    assert!(g.hero.kills >= 6, "kills {}", g.hero.kills);
    assert!(g.hero.level >= 2, "level {}", g.hero.level);
    assert!(g.arena.as_ref().unwrap().wave >= 2, "wave {}", g.arena.as_ref().unwrap().wave);
    assert!(g.hero.gold > 0);
}

#[test]
fn monsters_fight_back() {
    let mut sim = Sim::new("arena", 2).unwrap();
    // Stand still in the middle: the first wave comes and hurts.
    let mut hurt = false;
    for _ in 0..60 * 20 {
        sim.step(&InputFrame::default());
        let g = sim.state.game.as_ref().unwrap();
        let h = g.hero_actor().unwrap();
        if h.life < h.sheet.life_max * 0.95 || h.dead {
            hurt = true;
            break;
        }
    }
    assert!(hurt, "monsters reached and hit the hero");
}

#[test]
fn primary_attacks_do_not_fire_engine_weapons() {
    for weapon in [Weapon::Bombs, Weapon::Blaster] {
        let mut sim = Sim::new("arena", 6).unwrap();
        sim.state.game.as_mut().unwrap().arena = None;
        sim.config.bombs.weapon = weapon;
        sim.run(5, &InputFrame::default());
        sim.drain_events();
        let aim = hero_feet(&sim) + Vec3::X * 4.0;
        sim.step(&InputFrame { aim: Some(aim), held: buttons::PRIMARY, pressed: buttons::PRIMARY, ..Default::default() });
        let g = sim.state.game.as_ref().unwrap();
        let skill = pav_core::arpg::data::data().skill_id(&g.hero.bar[0]).unwrap();
        assert_eq!(g.hero_actor().unwrap().cast.as_ref().expect("primary skill started").skill, skill);
        // Hold to repeat, then click again: neither path may fire the demo weapon.
        for t in 0..90 {
            sim.step(&InputFrame {
                aim: Some(aim),
                held: buttons::PRIMARY,
                pressed: if t % 30 == 0 { buttons::PRIMARY } else { 0 },
                ..Default::default()
            });
        }
        let events = sim.drain_events();
        assert!(events.iter().filter(|e| matches!(e, SimEvent::Swing { .. })).count() >= 2, "sword swings repeat");
        assert!(
            !events.iter().any(|e| matches!(e, SimEvent::Throw { .. } | SimEvent::Shot { .. })),
            "primary attack also fired the engine's {weapon:?}"
        );
        assert!(sim.state.entities.iter().all(|e| e.bomb.is_none()), "no bomb entities");
    }
}

#[test]
fn skills_cost_mana_and_cool_down() {
    let mut sim = Sim::new("arena", 3).unwrap();
    sim.run(5, &InputFrame::default());
    let mana = sim.state.game.as_ref().unwrap().hero_actor().unwrap().mana;
    // Frost Nova on R (slot 5): costs mana and goes on cooldown.
    let press =
        InputFrame { held: buttons::SKILL5, pressed: buttons::SKILL5, aim: Some(Vec3::new(3.0, 0.0, 0.0)), ..Default::default() };
    sim.step(&press);
    let g = sim.state.game.as_ref().unwrap();
    let h = g.hero_actor().unwrap();
    assert!(h.mana < mana - 10.0, "paid mana {} -> {}", mana, h.mana);
    assert!(h.cast.is_some());
    let nova = pav_core::arpg::data::data().skill_id("frost_nova").unwrap();
    assert!(h.cooldown(nova) > 3.0);
}

#[test]
fn dodging_rolls_away_untouchable() {
    let mut sim = Sim::new("arena", 4).unwrap();
    sim.run(5, &InputFrame::default());
    let before = hero_feet(&sim);
    let f = InputFrame { move_dir: Vec2::new(1.0, 0.0), held: buttons::DODGE, pressed: buttons::DODGE, ..Default::default() };
    sim.step(&f);
    assert!(sim.state.game.as_ref().unwrap().hero_actor().unwrap().iframes > 0.1);
    sim.run(20, &InputFrame { move_dir: Vec2::ZERO, ..Default::default() });
    let after = hero_feet(&sim);
    assert!(after.x - before.x > 3.5, "dodged {before} -> {after}");
}

#[test]
fn rewind_is_exact_in_battle() {
    let mut sim = Sim::new("arena", 5).unwrap();
    for _ in 0..60 * 8 {
        let f = bot(&sim, buttons::PRIMARY);
        sim.step(&f);
    }
    let tick = sim.state.tick;
    let hash = sim.state_hash();
    assert!(sim.rewind_to(tick - 90));
    assert!(sim.rewind_to(tick));
    assert_eq!(sim.state_hash(), hash);
}
