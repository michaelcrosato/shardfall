//! Shardfall monsters: generated creatures of every archetype spawn and fight, bombers burst,
//! summoners call broods, rares carry affixes, bosses move through their phases, the arena's
//! tenth wave is a boss, the Menagerie shows and releases creatures, and rewind stays exact.

use glam::Vec3;
use pav_core::arpg::combat::{Rarity, Team};
use pav_core::arpg::data::data;
use pav_core::arpg::genome::{Genome, GenomeOpts};
use pav_core::arpg::{GameCmd, SpotKind};
use pav_core::{InputFrame, Sim};

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

/// A quiet arena with a sturdy hero standing still.
fn arena(seed: u64) -> Sim {
    let mut sim = Sim::new("arena", seed).unwrap();
    {
        let g = sim.state.game.as_mut().unwrap();
        g.arena = None;
        g.hero.level = 20;
    }
    let mut g = sim.state.game.take().unwrap();
    pav_core::arpg::refresh_hero(&mut sim, &mut g, true);
    sim.state.game = Some(g);
    sim.run(3, &InputFrame::default());
    sim
}

fn spawn_genome(sim: &mut Sim, arch: &str, seed: u64, at: Vec3) -> pav_core::EntityId {
    let d = data();
    let g = Genome::generate(&d, seed, 10, &GenomeOpts { archetype: Some(arch.into()), ..Default::default() }).unwrap();
    let spec = g.spec(&d);
    let mut gm = sim.state.game.take().unwrap();
    let id = pav_core::arpg::spawn_spec_into(sim, &mut gm, &spec, 10, Rarity::Normal, at, 50).unwrap();
    gm.actors.get_mut(&id).unwrap().brain.as_mut().unwrap().aggro = true;
    sim.state.game = Some(gm);
    id
}

#[test]
fn every_archetype_fights() {
    let d = data();
    for (i, arch) in d.genome.archetype.keys().enumerate() {
        let mut sim = arena(70 + i as u64);
        let at = hero_feet(&sim) + Vec3::new(7.0, 0.0, 0.0);
        let id = spawn_genome(&mut sim, arch, 900 + i as u64, at);
        let life0 = game(&sim).hero_actor().unwrap().life;
        let mut acted = false;
        for _ in 0..60 * 12 {
            sim.step(&InputFrame::default());
            let g = game(&sim);
            let h = g.hero_actor().unwrap();
            if h.life < life0 || g.actors.get(&id).is_none_or(|a| a.dead) {
                acted = true;
                break;
            }
            // Keep the hero alive and still.
            let hid = g.hero_id.unwrap();
            let g = sim.state.game.as_mut().unwrap();
            let h = g.actors.get_mut(&hid).unwrap();
            h.life = h.sheet.life_max;
        }
        assert!(acted, "archetype '{arch}' never hurt the hero");
    }
}

#[test]
fn bombers_burst_and_summoners_call_their_brood() {
    let mut sim = arena(80);
    let at = hero_feet(&sim) + Vec3::new(6.0, 0.0, 0.0);
    let bomber = spawn_genome(&mut sim, "bomber", 5, at);
    let mut burst = false;
    for _ in 0..60 * 10 {
        sim.step(&InputFrame::default());
        if game(&sim).actors.get(&bomber).is_none_or(|a| a.dead) {
            burst = true;
            break;
        }
    }
    assert!(burst, "the bomber reached the hero and burst");
    assert!(
        game(&sim).effects.iter().any(|e| e.dmg.is_some())
            || game(&sim).hero_actor().unwrap().life < game(&sim).hero_actor().unwrap().sheet.life_max
    );
    let at = hero_feet(&sim) + Vec3::new(-9.0, 0.0, 0.0);
    let summoner = spawn_genome(&mut sim, "summoner", 6, at);
    sim.run(60 * 9, &InputFrame::default());
    let brood = game(&sim).actors.values().filter(|a| a.master == Some(summoner)).count();
    assert!(brood >= 2, "brood {brood}");
}

#[test]
fn rares_have_affixes_and_names() {
    let mut sim = arena(81);
    let d = data();
    let spec = d.family("ghoul").unwrap().spec();
    let mut gm = sim.state.game.take().unwrap();
    let id = pav_core::arpg::spawn_spec_into(&mut sim, &mut gm, &spec, 30, Rarity::Rare, Vec3::new(5.0, 0.0, 5.0), 9).unwrap();
    sim.state.game = Some(gm);
    let a = &game(&sim).actors[&id];
    assert!((2..=3).contains(&a.affixes.len()), "{:?}", a.affixes);
    assert_ne!(a.name, "Ghoul", "rares have their own names");
}

#[test]
fn bosses_go_through_their_phases() {
    let d = data();
    let keys: Vec<String> = d.bosses.iter().map(|b| b.key.clone()).chain(["gen:77".to_string()]).collect();
    for key in keys {
        let mut sim = arena(82);
        let at = hero_feet(&sim) + Vec3::new(0.0, 0.0, -9.0);
        let mut gm = sim.state.game.take().unwrap();
        let id = gm.spawn_boss(&mut sim, &key, 15, at).unwrap_or_else(|| panic!("boss {key}"));
        sim.state.game = Some(gm);
        let phases = pav_core::arpg::boss::boss_def(&d, &key, 15).unwrap().phases;
        let before = game(&sim).actors.len();
        for p in &phases {
            {
                let g = sim.state.game.as_mut().unwrap();
                let b = g.actors.get_mut(&id).unwrap();
                b.life = b.sheet.life_max * (p.at - 0.02);
            }
            sim.run(2, &InputFrame::default());
        }
        let g = game(&sim);
        let b = &g.actors[&id];
        assert_eq!(b.boss.as_ref().unwrap().phase as usize, phases.len(), "{key}");
        if phases.iter().any(|p| p.summon.is_some()) {
            assert!(g.actors.len() > before, "{key} summoned");
        }
        assert!(g.frame(&sim).boss.is_some(), "{key}: boss bar");
    }
}

#[test]
fn the_tenth_wave_is_a_boss() {
    let mut sim = Sim::new("arena", 83).unwrap();
    {
        let g = sim.state.game.as_mut().unwrap();
        let ar = g.arena.as_mut().unwrap();
        ar.wave = 9;
        ar.next_in = 0.1;
    }
    sim.run(20, &InputFrame::default());
    let g = game(&sim);
    assert_eq!(g.arena.as_ref().unwrap().wave, 10);
    assert!(g.actors.values().any(|a| a.boss.is_some()), "a boss came");
}

#[test]
fn the_menagerie_shows_and_releases() {
    let mut sim = Sim::new("lab", 84).unwrap();
    let g = game(&sim);
    let exhibits: Vec<usize> = g.spots.iter().enumerate().filter(|(_, s)| s.kind == SpotKind::Exhibit).map(|(i, _)| i).collect();
    assert_eq!(exhibits.len(), 20);
    assert_eq!(g.monsters_alive(), 0, "exhibits are not fighting");
    let names: Vec<String> = g.spots.iter().filter(|s| s.kind == SpotKind::Exhibit).map(|s| s.name.clone()).collect();
    sim.step(&InputFrame { cmd: Some(GameCmd::Release(exhibits[7] as u32)), ..Default::default() });
    sim.run(5, &InputFrame::default());
    assert_eq!(game(&sim).monsters_alive(), 1, "one let loose");
    sim.step(&InputFrame { cmd: Some(GameCmd::Reroll), ..Default::default() });
    let after: Vec<String> = game(&sim).spots.iter().filter(|s| s.kind == SpotKind::Exhibit).map(|s| s.name.clone()).collect();
    assert_eq!(after.len(), 20);
    assert_ne!(after, names, "new creatures");
}

#[test]
fn rewind_is_exact_with_generated_monsters() {
    let mut sim = arena(85);
    sim.history.enabled = true;
    for (i, arch) in ["summoner", "caster", "bomber", "stalker"].iter().enumerate() {
        let at = hero_feet(&sim) + Vec3::new(8.0 * (i as f32 - 1.5), 0.0, -8.0);
        spawn_genome(&mut sim, arch, 300 + i as u64, at);
    }
    let mut gm = sim.state.game.take().unwrap();
    gm.spawn_boss(&mut sim, "mother_of_swarms", 12, Vec3::new(0.0, 0.0, 12.0));
    sim.state.game = Some(gm);
    sim.run(60 * 6, &InputFrame::default());
    let tick = sim.state.tick;
    let hash = sim.state_hash();
    assert!(sim.rewind_to(tick - 120));
    assert!(sim.rewind_to(tick));
    assert_eq!(sim.state_hash(), hash);
    assert!(game(&sim).actors.values().filter(|a| a.team == Team::Monster).count() >= 4);
}
