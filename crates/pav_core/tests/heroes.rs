//! Playable characters (game/heroes.toml): the Hall of Heroes holds them all, performing; a hero
//! of each keeps their own progress while another is played and the stash goes along; each
//! strikes in their own way (captured attacks and moves per combo swing), dodges their own way,
//! and looks their own way.

use glam::{Vec2, Vec3};
use pav_core::arpg::data::data;
use pav_core::arpg::hall::Beat;
use pav_core::arpg::{GameCmd, SpotKind};
use pav_core::clips;
use pav_core::input::buttons;
use pav_core::moves::MoveId;
use pav_core::params::ChoiceParam;
use pav_core::puppet::{PerSwing, PuppetDef};
use pav_core::{InputFrame, Sim};

fn game(sim: &Sim) -> &pav_core::arpg::Game {
    sim.state.game.as_ref().unwrap()
}

fn cmd(sim: &mut Sim, c: GameCmd) {
    sim.step(&InputFrame { cmd: Some(c), ..Default::default() });
}

fn index(key: &str) -> u8 {
    data().character_index(key).unwrap() as u8
}

fn anim(sim: &Sim) -> pav_core::puppet::PuppetState {
    sim.player().unwrap().character.as_ref().unwrap().anim
}

/// A quiet arena (no waves) with the hero played as `key`.
fn arena_as(key: &str) -> Sim {
    let mut sim = Sim::new("arena", 3).unwrap();
    let mut g = sim.state.game.take().unwrap();
    g.arena = None;
    if key != "wanderer" {
        pav_core::arpg::hall::switch(&mut g, &mut sim, key).unwrap();
    }
    g.hero.level = 20;
    pav_core::arpg::refresh_hero(&mut sim, &mut g, true);
    sim.state.game = Some(g);
    sim.run(5, &InputFrame::default());
    sim
}

#[test]
fn there_are_two_new_characters_besides_the_wanderer() {
    let d = data();
    let keys: Vec<&str> = d.characters.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["wanderer", "kestrel", "ysolde"]);
    let (k, y) = (&d.character("kestrel").unwrap().puppet, &d.character("ysolde").unwrap().puppet);
    // They look different: build, colours, silhouette.
    assert!(y.scale > k.scale && y.leg_length > k.leg_length && k.torso_radius > y.torso_radius);
    assert_ne!(k.shirt, y.shirt);
    assert!(k.parts.iter().any(|p| p.kind.name() == "crest") && y.parts.iter().any(|p| p.kind.name() == "orbs"));
    // And move differently: each has its own idle, run, dodge, captured strikes and moves.
    for p in [k, y] {
        assert!(!p.idle_clip.is_empty() && !p.run_clip.is_empty() && !p.walk_clip.is_empty() && !p.dodge_clip.is_empty());
        assert!(p.attack_clips.len() >= 4 && p.attack_moves.len() >= 4);
        assert!(clips::missing(p).is_empty());
    }
    assert_ne!(k.idle_clip, y.idle_clip);
    assert_ne!(k.dodge_clip, y.dodge_clip);
}

#[test]
fn the_hall_of_heroes_holds_everyone_performing() {
    let mut sim = Sim::new("town", 1).unwrap();
    let d = data();
    let n = d.characters.len();
    let spots: Vec<String> = game(&sim).spots.iter().filter(|s| s.kind == SpotKind::Hero).map(|s| s.name.clone()).collect();
    assert_eq!(spots.len(), n, "a pedestal for each: {spots:?}");
    assert!(game(&sim).spots.iter().filter(|s| s.kind == SpotKind::Hero).all(|s| s.info.len() == 3), "every card filled");
    assert_eq!(game(&sim).performers.len(), n);
    // Every performer works through their reel: captured clips and moves both show.
    let mut clip_seen = vec![false; n];
    let mut move_seen = vec![false; n];
    for _ in 0..60 * 30 {
        sim.step(&InputFrame::default());
        for (i, p) in game(&sim).performers.iter().enumerate() {
            let a = sim.state.entities.get(p.id).unwrap().character.as_ref().unwrap().anim;
            if p.reel.iter().any(|b| matches!(b, Beat::Clip { id, .. } if *id == a.clip)) {
                clip_seen[i] = true;
            }
            if a.act_kind != 0 {
                move_seen[i] = true;
            }
        }
    }
    let names: Vec<&str> = d.characters.iter().map(|c| c.key.as_str()).collect();
    for i in 0..n {
        assert!(move_seen[i], "{} swings a move on the pedestal", names[i]);
    }
    for i in 1..n {
        assert!(clip_seen[i], "{} plays a captured clip on the pedestal", names[i]);
    }
}

#[test]
fn heroes_keep_their_progress_and_share_the_stash() {
    use pav_core::arpg::items::Item;
    let mut sim = Sim::new("town", 2).unwrap();
    let d = data();
    {
        let g = sim.state.game.as_mut().unwrap();
        g.hero.level = 12;
        g.hero.max_depth = 5;
        let id = g.hero.new_id();
        g.hero.stash.push(Item::plain(id, d.base("iron_ring").unwrap(), 10));
    }
    cmd(&mut sim, GameCmd::Character(index("kestrel")));
    let g = game(&sim);
    assert_eq!(g.hero.character, "kestrel");
    assert_eq!((g.hero.name.as_str(), g.hero.level), ("Kestrel", 1), "a new hero");
    assert_eq!(g.hero.weapon.name, "Hooks");
    assert_eq!(g.hero.bar[1], "cleave");
    assert_eq!(g.roster["wanderer"].level, 12, "the Wanderer waits in the hall");
    assert_eq!(g.hero.stash.len(), 1, "the stash came along");
    let mut ids: Vec<u32> = g.hero.equipment.iter().flatten().chain(&g.hero.stash).map(|i| i.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2, "renumbered for the new hero's bags");
    // Looks and moves as Kestrel.
    let look = sim.player().unwrap().character.as_ref().unwrap().puppet.clone().unwrap();
    assert_eq!(look.idle_clip, "CMU/Boxing_Guard_Loop");
    assert_eq!(look.weapon.kind.name(), "claw");
    assert!(look.parts.iter().any(|p| p.kind.name() == "crest"));
    // Kestrel levels up; back to the Wanderer, then Kestrel again: each where they were.
    sim.state.game.as_mut().unwrap().hero.level = 4;
    cmd(&mut sim, GameCmd::Character(index("wanderer")));
    let g = game(&sim);
    assert_eq!((g.hero.level, g.hero.max_depth), (12, 5));
    assert_eq!(g.hero.stash.len(), 1);
    assert_eq!(g.roster["kestrel"].level, 4);
    cmd(&mut sim, GameCmd::Character(index("kestrel")));
    assert_eq!(game(&sim).hero.level, 4);
    // A save's roster comes back into a new town, minus whoever is played.
    let roster = game(&sim).roster.clone();
    let hero = game(&sim).hero.clone();
    let json = serde_json::to_string(&roster).unwrap();
    let mut town = Sim::new("town", 3).unwrap();
    assert!(town.load_hero(hero));
    town.load_roster(serde_json::from_str(&json).unwrap());
    let g = game(&town);
    assert_eq!(g.hero.character, "kestrel");
    assert_eq!(g.roster.keys().collect::<Vec<_>>(), ["wanderer"]);
    let card = g.spots.iter().find(|s| s.kind == SpotKind::Hero && s.name.starts_with("Wanderer")).unwrap();
    assert!(card.info.iter().any(|l| l.contains("level 12")), "{:?}", card.info);
    // Only in town.
    let mut arena = Sim::new("arena", 4).unwrap();
    cmd(&mut arena, GameCmd::Character(index("ysolde")));
    assert_eq!(game(&arena).hero.character_key(&d), "wanderer");
}

#[test]
fn each_character_strikes_their_own_way() {
    // Kestrel: jab, cross, roundhouse kick (captured), one per swing of the slash combo.
    let mut sim = arena_as("kestrel");
    let want: Vec<u32> = ["QUATERNIUS/Punch_Jab", "QUATERNIUS/Punch_Cross", "CMU/Roundhouse_Kick"]
        .iter()
        .map(|c| clips::find(c).unwrap())
        .collect();
    let mut order = Vec::new();
    for _ in 0..150 {
        sim.step(&InputFrame { held: buttons::PRIMARY, aim: Some(Vec3::new(0.0, 0.0, -4.0)), ..Default::default() });
        let c = anim(&sim).clip;
        if want.contains(&c) && order.last() != Some(&c) {
            order.push(c);
        }
    }
    assert!(order.len() >= 3 && order[..3] == want[..], "jab, cross, roundhouse in turn: {order:?}");
    // Cleave is a leg sweep (a move of her own).
    let mut sim = arena_as("kestrel");
    let sweep = MoveId::of("sweep").index();
    let mut swept = false;
    for t in 0..40 {
        let b = if t == 0 { buttons::SECONDARY } else { 0 };
        sim.step(&InputFrame { held: b, pressed: b, ..Default::default() });
        swept |= anim(&sim).act_kind == sweep;
    }
    assert!(swept, "Kestrel cleaves with a leg sweep");
    // Ysolde: staff strikes, a different move each swing.
    let mut sim = arena_as("ysolde");
    let want: Vec<u8> = ["thrust", "backslash", "rising"].iter().map(|m| MoveId::of(m).index()).collect();
    let mut order = Vec::new();
    for _ in 0..150 {
        sim.step(&InputFrame { held: buttons::PRIMARY, aim: Some(Vec3::new(0.0, 0.0, -4.0)), ..Default::default() });
        let k = anim(&sim).act_kind;
        if want.contains(&k) && order.last() != Some(&k) {
            order.push(k);
        }
    }
    assert!(order.len() >= 3 && order[..3] == want[..], "thrust, backhand, rising cut: {order:?}");
    // The Wanderer keeps the skill's own move.
    let mut sim = arena_as("wanderer");
    let slash = MoveId::of("slash").index();
    let mut slashed = false;
    for _ in 0..30 {
        sim.step(&InputFrame { held: buttons::PRIMARY, ..Default::default() });
        slashed |= anim(&sim).act_kind == slash;
    }
    assert!(slashed);
}

#[test]
fn a_captured_dodge_replaces_the_tumble() {
    let dodge = InputFrame { move_dir: Vec2::new(1.0, 0.0), held: buttons::DODGE, pressed: buttons::DODGE, ..Default::default() };
    let go = InputFrame { move_dir: Vec2::new(1.0, 0.0), ..Default::default() };
    for (key, clip) in [("kestrel", "QUATERNIUS/Roll"), ("ysolde", "MESH2MOTION/Glide")] {
        let mut sim = arena_as(key);
        sim.step(&dodge);
        sim.run(6, &go);
        let a = anim(&sim);
        assert_eq!(a.clip, clips::find(clip).unwrap(), "{key} dodges with {clip}");
        assert!(a.roll < 0.01, "{key} doesn't tumble on top");
        // Once only: it plays out and the run takes over.
        sim.run(60, &go);
        assert_ne!(anim(&sim).clip, clips::find(clip).unwrap(), "{key}: the dodge is over");
    }
    // The Wanderer still tumbles.
    let mut sim = arena_as("wanderer");
    sim.step(&dodge);
    sim.run(6, &go);
    assert!(anim(&sim).roll > 0.5, "the procedural roll");
}

#[test]
fn per_swing_names_read_as_one_or_a_list() {
    let p: PuppetDef = toml::from_str(
        "attack_clips = { slash = [\"A/a\", \"B/b\", \"C/c\"], claw = \"D/d@0.4\" }\nattack_moves = { cleave = \"sweep\" }",
    )
    .unwrap();
    let slash = &p.attack_clips["slash"];
    assert_eq!((slash.swing(0), slash.swing(1), slash.swing(2), slash.swing(3)), ("A/a", "B/b", "C/c", "A/a"));
    assert!(slash.per_swing());
    assert_eq!(p.attack_clips["claw"], PerSwing::One("D/d@0.4".into()));
    assert!(!p.attack_moves["cleave"].per_swing());
    // A single move alternates sides like the skill's own; a list plays each as written.
    assert_eq!(pav_core::moves::attack_move(&p, "cleave", 1, -1.0), Some((MoveId::of("sweep"), -1.0)));
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<PuppetDef>(&json).unwrap(), p);
}
