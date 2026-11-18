//! Moments for the 52in1 hub (`arpg::moments`): every copy builds the same moment from the
//! round's seed, a fallen hero fails it, its goals clear it, and each can be cleared in time
//! by playing it (the gold run by a greedy walk; the fights by `autoplay` in the tools).

use glam::{Vec2, Vec3};
use pav_core::arpg::moments::{Goal, MOMENTS};
use pav_core::{InputFrame, Sim};

fn moment(sim: &Sim) -> pav_core::arpg::moments::MomentView {
    let g = sim.state.game.as_ref().unwrap();
    g.moment.as_ref().unwrap().view(g)
}

fn hero_feet(sim: &Sim) -> Vec3 {
    let p = sim.player().unwrap();
    p.pos - Vec3::Y * p.character.as_ref().unwrap().height() * 0.5
}

#[test]
fn every_copy_builds_the_same_moment() {
    for m in MOMENTS {
        let scene = format!("moment/{}", m.id);
        let mut a = Sim::new(&scene, 77).unwrap();
        let mut b = Sim::new(&scene, 77).unwrap();
        for _ in 0..30 {
            a.step(&InputFrame::default());
            b.step(&InputFrame::default());
        }
        assert_eq!(a.state_hash(), b.state_hash(), "moment {} differs between copies", m.id);
        let v = moment(&a);
        assert_eq!(v.outcome, None, "{} decided before play", m.id);
        assert!(v.left < v.seconds && v.left > v.seconds - 1.0, "{}: the clock runs with play", m.id);
        let g = a.state.game.as_ref().unwrap();
        assert!(g.spots.is_empty(), "{}: no portal home in a moment", m.id);
        match m.goal {
            Goal::Defeat | Goal::Survive => assert!(g.monsters_alive() > 0, "{}: monsters to fight", m.id),
            Goal::Collect => assert!(!g.gold.is_empty() && v.total as usize == g.gold.len(), "{}: gold to grab", m.id),
        }
        assert!(m.id.len() <= 24 && m.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert!((3..=30).contains(&m.seconds), "{}: 3 to 30 seconds", m.id);
        assert!(m.goal_en.len() <= 60, "{}: the hub's goal is at most 60 characters", m.id);
    }
}

#[test]
fn a_fallen_hero_fails_the_moment_and_stays_down() {
    let mut sim = Sim::new("moment/pack", 3).unwrap();
    sim.step(&InputFrame::default());
    {
        let g = sim.state.game.as_mut().unwrap();
        let hid = g.hero_id.unwrap();
        g.actors.get_mut(&hid).unwrap().life = 0.0;
        g.actors.get_mut(&hid).unwrap().dead = true;
    }
    for _ in 0..60 * 5 {
        sim.step(&InputFrame::default());
    }
    assert_eq!(moment(&sim).outcome, Some(false));
    let g = sim.state.game.as_ref().unwrap();
    assert!(g.hero_actor().unwrap().dead, "no respawn in a moment");
    assert!(g.loot.is_empty(), "a moment drops nothing");
}

#[test]
fn surviving_to_the_end_clears_a_survival_moment_and_only_that() {
    for (scene, cleared) in [("moment/swarm", true), ("moment/pack", false)] {
        let mut sim = Sim::new(scene, 5).unwrap();
        sim.step(&InputFrame::default());
        sim.state.game.as_mut().unwrap().moment.as_mut().unwrap().left = 0.05;
        for _ in 0..10 {
            sim.step(&InputFrame::default());
        }
        assert_eq!(moment(&sim).outcome, Some(cleared), "{scene}");
    }
}

#[test]
fn the_gold_can_be_grabbed_in_time() {
    // A greedy walk to the nearest pile clears it with time to spare, on several seeds.
    for seed in 1..=5 {
        let mut sim = Sim::new("moment/gold", seed).unwrap();
        let limit = moment(&sim).seconds;
        let mut ticks = 0;
        while moment(&sim).outcome.is_none() && ticks < 60 * 40 {
            let me = hero_feet(&sim);
            let g = sim.state.game.as_ref().unwrap();
            let next = g.gold.iter().map(|p| p.pos).min_by(|a, b| (*a - me).length().total_cmp(&(*b - me).length()));
            let mut f = InputFrame::default();
            if let Some(t) = next {
                let d = Vec2::new(t.x - me.x, t.z - me.z);
                if d.length() > 0.2 {
                    f.move_dir = d.normalize();
                }
            }
            sim.step(&f);
            ticks += 1;
        }
        let v = moment(&sim);
        assert_eq!(v.outcome, Some(true), "seed {seed}: {} piles left after {:.1} s", v.remaining, ticks as f32 / 60.0);
        assert!((ticks as f32 / 60.0) < limit * 0.8, "seed {seed}: took {:.1} of {limit} s", ticks as f32 / 60.0);
    }
}

#[test]
fn the_pages_moment_tags_name_the_games_moments() {
    // web/hub.json writes one hub:moment tag per moment: ID VERB SECONDS GOAL.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/hub.json");
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let tags: Vec<String> = cfg["moments"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let ours: Vec<String> = MOMENTS.iter().map(|m| format!("{} {} {} {}", m.id, m.verb, m.seconds, m.goal_en)).collect();
    assert_eq!(tags, ours, "web/hub.json's moments and arpg::moments::MOMENTS differ");
    assert!(cfg["multiplayer"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().starts_with("moments ")));
}
