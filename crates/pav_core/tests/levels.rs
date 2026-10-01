//! Shardfall levels: every designed level and a run of endless depths build and run, each
//! level's mechanics are in it and work (shrines, kegs, spikes, gates, wind, totems, lava,
//! ice, crumbling floors, wells, chests, time bubbles), the way down opens when the boss
//! falls, waypoints and travel work, and rewind stays exact in a level.

use glam::Vec3;
use pav_core::arpg::combat::Team;
use pav_core::arpg::data::data;
use pav_core::arpg::mechanics::{ChestState, FeatureKind};
use pav_core::arpg::world::{self, Mechanic};
use pav_core::arpg::{GameCmd, Place, SpotKind};
use pav_core::{InputFrame, Sim};

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

fn level(sim: &Sim) -> &pav_core::arpg::mechanics::LevelState {
    game(sim).level.as_ref().unwrap()
}

/// A level with a sturdy hero who can take a few hits.
fn level_sim(depth: u32, seed: u64) -> Sim {
    let mut sim = Sim::new(&format!("level/{depth}"), seed).unwrap();
    let mut g = sim.state.game.take().unwrap();
    g.hero.level = 30;
    pav_core::arpg::refresh_hero(&mut sim, &mut g, true);
    sim.state.game = Some(g);
    sim
}

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

fn put_hero(sim: &mut Sim, at: Vec3) {
    let id = game(sim).hero_id.unwrap();
    sim.set_position(id, at);
}

fn has(sim: &Sim, pred: impl Fn(&FeatureKind) -> bool) -> bool {
    level(sim).features.iter().any(|f| pred(&f.kind))
}

#[test]
fn every_designed_level_builds_with_its_mechanics() {
    let d = data();
    assert_eq!(world::designed(&d), 12);
    for depth in 1..=12 {
        let sim = level_sim(depth, 100 + depth as u64);
        let g = game(&sim);
        assert_eq!(g.place, Place::Level(depth));
        let lv = level(&sim);
        let plan = world::plan(&d, depth);
        assert_eq!(lv.name, plan.name);
        assert!(g.spots.iter().any(|s| s.kind == SpotKind::Exit), "level {depth}: a way down");
        assert!(g.spots.iter().any(|s| s.kind == SpotKind::Portal), "level {depth}: a portal home");
        assert!(g.monsters_alive() >= 10, "level {depth}: {} monsters", g.monsters_alive());
        for m in &plan.mechanics {
            let present = match m {
                Mechanic::Shrines => has(&sim, |k| matches!(k, FeatureKind::Shrine { .. })),
                Mechanic::PowderKeg => has(&sim, |k| matches!(k, FeatureKind::Keg { .. })),
                Mechanic::Gauntlet => has(&sim, |k| matches!(k, FeatureKind::Spikes { .. })),
                Mechanic::RiftGates => has(&sim, |k| matches!(k, FeatureKind::Gate { .. })),
                Mechanic::Windways => has(&sim, |k| matches!(k, FeatureKind::Wind { .. })),
                Mechanic::Totems => has(&sim, |k| matches!(k, FeatureKind::Totem)),
                Mechanic::MoltenFloor => has(&sim, |k| matches!(k, FeatureKind::Lava { .. })),
                Mechanic::FrozenLake => has(&sim, |k| matches!(k, FeatureKind::Ice { .. })),
                Mechanic::CrumblingHalls => has(&sim, |k| matches!(k, FeatureKind::Crumble { .. })),
                Mechanic::LightlessDeep => has(&sim, |k| matches!(k, FeatureKind::Well { .. })),
                Mechanic::CursedVaults => has(&sim, |k| matches!(k, FeatureKind::Chest { .. })),
                Mechanic::TimeRift => has(&sim, |k| matches!(k, FeatureKind::Bubble { .. })),
            };
            assert!(present, "level {depth} ({}) is missing {:?}", plan.name, m);
        }
        assert_eq!(lv.boss.is_some(), plan.boss.is_some(), "level {depth}: boss");
        assert_eq!(lv.exit_open, plan.boss.is_none());
    }
}

#[test]
fn endless_depths_are_new_combinations_forever() {
    let d = data();
    let mut names = std::collections::BTreeSet::new();
    for depth in 13..=40 {
        let p = world::plan(&d, depth);
        assert!(p.endless);
        assert!(p.mechanics.len() >= 2, "depth {depth}: mechanics are combined");
        assert_eq!(p.monster_level, world::monster_level(&d, depth));
        assert_eq!(world::plan(&d, depth), p, "depth {depth}: the same depth is always the same place");
        names.insert(p.name.clone());
    }
    assert!(names.len() > 20, "names repeat too much: {names:?}");
    assert!(world::monster_level(&d, 40) > world::monster_level(&d, 13));
    assert!((13..=40).any(|n| world::plan(&d, n).boss.is_some()), "bosses in the depths");
    // A few actually build and run.
    for depth in [13, 18, 25, 61] {
        let mut sim = level_sim(depth, depth as u64);
        sim.run(30, &InputFrame::default());
        assert!(game(&sim).monsters_alive() > 0, "depth {depth}");
    }
}

#[test]
fn shrines_grant_boons() {
    let mut sim = level_sim(1, 7);
    let at = level(&sim).features.iter().find(|f| matches!(f.kind, FeatureKind::Shrine { .. })).unwrap().pos;
    put_hero(&mut sim, at + Vec3::new(0.0, 0.05, 0.6));
    sim.run(3, &InputFrame::default());
    let h = game(&sim).hero_actor().unwrap();
    assert!(!h.buffs.is_empty(), "a boon");
    assert!(has(&sim, |k| matches!(k, FeatureKind::Shrine { used: true, .. })));
}

#[test]
fn kegs_blow_up_monsters_and_each_other() {
    let mut sim = level_sim(2, 8);
    // Find a keg with a monster near it.
    let lv = level(&sim);
    let kegs: Vec<(pav_core::EntityId, Vec3)> =
        lv.features.iter().filter(|f| matches!(f.kind, FeatureKind::Keg { .. })).map(|f| (f.entity.unwrap(), f.pos)).collect();
    assert!(kegs.len() >= 4, "kegs: {}", kegs.len());
    let (keg, at) = kegs[0];
    let mut gm = sim.state.game.take().unwrap();
    let id = sim_spawn(&mut sim, &mut gm, at + Vec3::new(1.5, 0.1, 0.0));
    let life0 = gm.actors[&id].life;
    sim.state.game = Some(gm);
    // The keg dies (as if hit), then blows.
    let mut g = sim.state.game.take().unwrap();
    let mut ev = Vec::new();
    let dmg = pav_core::arpg::combat::Damage {
        amount: [10.0, 0.0, 0.0, 0.0, 0.0],
        crit: false,
        ailment: [0.0; 5],
        ailment_mult: 1.0,
        knockback: 0.0,
        source: g.hero_id,
        skill: 0,
        attack: true,
        melee: true,
    };
    pav_core::arpg::debug_hit(&mut sim, &mut g, keg, &dmg, &mut ev);
    sim.state.game = Some(g);
    sim.run(20, &InputFrame::default());
    let g = game(&sim);
    assert!(!g.actors.contains_key(&keg), "the keg is gone");
    let hurt = g.actors.get(&id).is_none_or(|a| a.dead || a.life < life0 * 0.5);
    assert!(hurt, "the monster next to it took the blast");
}

fn sim_spawn(sim: &mut Sim, g: &mut pav_core::arpg::Game, at: Vec3) -> pav_core::EntityId {
    let d = data();
    let spec = d.family("ghoul").unwrap().spec();
    pav_core::arpg::spawn_spec_into(sim, g, &spec, 10, pav_core::arpg::combat::Rarity::Normal, at, 999).unwrap()
}

#[test]
fn rift_gates_fold_the_map() {
    let mut sim = level_sim(4, 9);
    let lv = level(&sim);
    let (i, gate) = lv.features.iter().enumerate().find(|(_, f)| matches!(f.kind, FeatureKind::Gate { .. })).unwrap();
    let FeatureKind::Gate { to, .. } = gate.kind else { unreachable!() };
    assert_ne!(to, i);
    let (from, dest) = (gate.pos, lv.features[to].pos);
    put_hero(&mut sim, from + Vec3::Y * 0.05);
    sim.run(3, &InputFrame::default());
    let now = hero_feet(&sim);
    assert!((now - dest).length() < 4.0, "teleported next to the partner gate: {now} vs {dest}");
}

#[test]
fn wind_carries_and_lava_burns() {
    let mut sim = level_sim(7, 10);
    let lv = level(&sim);
    let wind = lv.features.iter().find_map(|f| match &f.kind {
        FeatureKind::Wind { min, max, dir, .. } => Some(((*min + *max) * 0.5, *dir)),
        _ => None,
    });
    let lava = lv.features.iter().find(|f| matches!(f.kind, FeatureKind::Lava { .. })).map(|f| f.pos).unwrap();
    if let Some((c, dir)) = wind {
        put_hero(&mut sim, Vec3::new(c.x, 0.05, c.y) - dir * 2.0);
        let before = hero_feet(&sim);
        sim.run(20, &InputFrame::default());
        let moved = (hero_feet(&sim) - before).dot(dir);
        assert!(moved > 0.8, "the wind carried the hero {moved} m");
    }
    put_hero(&mut sim, lava + Vec3::Y * 0.05);
    let life0 = game(&sim).hero_actor().unwrap().life;
    sim.run(60, &InputFrame::default());
    assert!(game(&sim).hero_actor().unwrap().life < life0, "lava burns");
}

#[test]
fn totems_ward_monsters_until_broken() {
    let mut sim = level_sim(6, 11);
    let lv = level(&sim);
    let (totem, at) =
        lv.features.iter().find_map(|f| matches!(f.kind, FeatureKind::Totem).then(|| (f.entity.unwrap(), f.pos))).unwrap();
    let mut gm = sim.state.game.take().unwrap();
    let id = sim_spawn(&mut sim, &mut gm, at + Vec3::new(2.0, 0.1, 0.0));
    sim.state.game = Some(gm);
    sim.run(3, &InputFrame::default());
    assert!(game(&sim).actors[&id].buffs.iter().any(|b| b.name == "Warded"), "warded near the totem");
    assert_eq!(game(&sim).actors[&totem].team, Team::Monster);
}

#[test]
fn ice_slides_and_frozen_monsters_shatter() {
    let mut sim = level_sim(8, 12);
    let lv = level(&sim);
    let (min, max) = lv
        .features
        .iter()
        .find_map(|f| match f.kind {
            FeatureKind::Ice { min, max } => Some((min, max)),
            _ => None,
        })
        .unwrap();
    let c = (min + max) * 0.5;
    let at = Vec3::new(c.x, 0.1, c.y);
    let mut gm = sim.state.game.take().unwrap();
    let id = sim_spawn(&mut sim, &mut gm, at);
    gm.actors.get_mut(&id).unwrap().ailments.freeze = 3.0;
    sim.state.game = Some(gm);
    sim.run(3, &InputFrame::default());
    let slide = sim.state.entities.get(id).unwrap().character.as_ref().unwrap().slide;
    assert!(slide > 0.5, "on ice: {slide}");
    let mut g = sim.state.game.take().unwrap();
    let dmg = pav_core::arpg::combat::Damage {
        amount: [1.0, 0.0, 0.0, 0.0, 0.0],
        crit: false,
        ailment: [0.0; 5],
        ailment_mult: 1.0,
        knockback: 0.0,
        source: g.hero_id,
        skill: 0,
        attack: true,
        melee: true,
    };
    let mut ev = Vec::new();
    pav_core::arpg::debug_hit(&mut sim, &mut g, id, &dmg, &mut ev);
    assert!(g.actors[&id].dead, "frozen on ice: shattered by a tiny hit");
    sim.state.game = Some(g);
}

#[test]
fn falling_through_crumbled_floor() {
    let mut sim = level_sim(9, 13);
    let safe = level(&sim).safe;
    let mut gm = sim.state.game.take().unwrap();
    let id = sim_spawn(&mut sim, &mut gm, Vec3::new(500.0, -6.0, 500.0));
    sim.state.game = Some(gm);
    put_hero(&mut sim, Vec3::new(500.0, -6.0, 503.0));
    sim.run(3, &InputFrame::default());
    let g = game(&sim);
    assert!(g.actors.get(&id).is_none_or(|a| a.dead), "the monster fell to its death");
    assert!((hero_feet(&sim) - safe).length() < 3.0, "the hero climbed back out");
}

#[test]
fn wells_light_the_dark() {
    let mut sim = level_sim(10, 14);
    let lv = level(&sim);
    assert!(lv.mood.sun < 0.1, "a dark level");
    let at = lv.features.iter().find(|f| matches!(f.kind, FeatureKind::Well { .. })).unwrap().pos;
    put_hero(&mut sim, at + Vec3::new(1.2, 0.05, 0.0));
    sim.run(3, &InputFrame::default());
    assert!(has(&sim, |k| matches!(k, FeatureKind::Well { lit: true })));
    assert!(game(&sim).hero_actor().unwrap().buffs.iter().any(|b| b.name == "Kindled"));
    assert!(level(&sim).lantern.is_some(), "the hero carries a light");
}

#[test]
fn cursed_chests_call_keepers_then_pay_out() {
    let mut sim = level_sim(11, 15);
    let g = game(&sim);
    let spot = g.spots.iter().position(|s| s.kind == SpotKind::Chest).unwrap();
    let at = g.spots[spot].pos;
    put_hero(&mut sim, at + Vec3::new(1.6, 0.05, 0.0));
    sim.run(2, &InputFrame::default());
    sim.step(&InputFrame { cmd: Some(GameCmd::Use(spot as u32)), ..Default::default() });
    for wave in 1..=3 {
        sim.run(80, &InputFrame::default());
        let keepers: Vec<pav_core::EntityId> = level(&sim)
            .features
            .iter()
            .find_map(|f| match &f.kind {
                FeatureKind::Chest { spot: s, keepers, wave: w, .. } if *s == spot => {
                    assert_eq!(*w, wave);
                    Some(keepers.clone())
                }
                _ => None,
            })
            .unwrap();
        assert!(!keepers.is_empty(), "wave {wave} came");
        let g = sim.state.game.as_mut().unwrap();
        for k in keepers {
            if let Some(a) = g.actors.get_mut(&k) {
                a.life = 0.0;
                a.dead = true;
            }
        }
    }
    let loot0 = game(&sim).loot.len();
    sim.run(90, &InputFrame::default());
    assert!(has(&sim, |k| matches!(k, FeatureKind::Chest { state: ChestState::Open, .. })));
    assert!(game(&sim).loot.len() >= loot0 + 3, "treasure");
}

#[test]
fn time_bubbles_slow_monsters() {
    let mut sim = level_sim(12, 16);
    let lv = level(&sim);
    let at = lv.features.iter().find(|f| matches!(f.kind, FeatureKind::Bubble { .. })).unwrap().pos;
    let mut gm = sim.state.game.take().unwrap();
    let id = sim_spawn(&mut sim, &mut gm, at + Vec3::new(1.0, 0.1, 0.0));
    sim.state.game = Some(gm);
    sim.run(3, &InputFrame::default());
    assert!(game(&sim).actors[&id].ailments.chill.0 >= 0.6, "slowed");
}

#[test]
fn the_way_down_opens_when_the_boss_falls_and_waypoints_remember() {
    let mut sim = level_sim(4, 17);
    let g = game(&sim);
    let exit = g.spots.iter().position(|s| s.kind == SpotKind::Exit).unwrap();
    let at = g.spots[exit].pos;
    assert!(!level(&sim).exit_open);
    put_hero(&mut sim, at + Vec3::new(0.0, 0.05, 2.0));
    sim.step(&InputFrame { cmd: Some(GameCmd::Use(exit as u32)), ..Default::default() });
    assert_eq!(game(&sim).place, Place::Level(4), "sealed while the boss lives");
    let boss = level(&sim).boss.unwrap();
    sim.state.game.as_mut().unwrap().actors.get_mut(&boss).unwrap().life = 0.0;
    sim.state.game.as_mut().unwrap().actors.get_mut(&boss).unwrap().dead = true;
    sim.run(2, &InputFrame::default());
    assert!(level(&sim).exit_open);
    let points = game(&sim).hero.bonus_points;
    sim.step(&InputFrame { cmd: Some(GameCmd::Use(exit as u32)), ..Default::default() });
    let g = game(&sim);
    assert_eq!(g.place, Place::Level(5));
    assert_eq!(g.hero.max_depth, 5);
    assert_eq!(g.hero.bonus_points, points + 1, "a boss level pays a passive point");
    // Home, and back by waypoint.
    sim.step(&InputFrame { cmd: Some(GameCmd::Travel(Place::Town.code())), ..Default::default() });
    assert_eq!(game(&sim).place, Place::Town);
    sim.step(&InputFrame { cmd: Some(GameCmd::Travel(Place::Level(9).code())), ..Default::default() });
    assert_eq!(game(&sim).place, Place::Town, "level 9 is not reached yet");
    sim.step(&InputFrame { cmd: Some(GameCmd::Travel(Place::Level(3).code())), ..Default::default() });
    assert_eq!(game(&sim).place, Place::Level(3));
}

#[test]
fn rewind_is_exact_in_a_level() {
    let mut sim = level_sim(6, 18);
    sim.history.enabled = true;
    // Walk into the first room and let things happen.
    let inp = InputFrame { move_dir: glam::Vec2::new(0.0, 1.0), ..Default::default() };
    sim.run(60 * 4, &inp);
    let tick = sim.state.tick;
    let hash = sim.state_hash();
    assert!(sim.rewind_to(tick - 100));
    assert!(sim.rewind_to(tick));
    assert_eq!(sim.state_hash(), hash);
}
