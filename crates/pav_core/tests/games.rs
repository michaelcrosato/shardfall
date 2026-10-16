//! Games built on the engine (`pav_core::games`): the starter's bot wins on many seeds, the
//! starter can be lost, and a game's state survives snapshot files, rewind and replays like
//! the rest of the simulation. Copy these tests for a new game.

use pav_core::games::{GAMES, Outcome};
use pav_core::{InputFrame, Sim, SimState};

#[test]
fn the_starter_bot_wins_on_ten_seeds() {
    for seed in 1..=10 {
        let mut sim = Sim::new("starter", seed).unwrap();
        let (outcome, ticks) = sim.run_bot(60 * 60);
        let status = sim.play().unwrap().status(&sim);
        assert_eq!(outcome, Outcome::Won, "seed {seed}, {ticks} ticks: {status}");
    }
}

#[test]
fn standing_still_gets_caught() {
    let mut sim = Sim::new("starter", 1).unwrap();
    assert_eq!(sim.frame().hud.lines, ["Shards 0/5", "Lives 3"]);
    for _ in 0..60 * 60 {
        sim.step(&InputFrame::default());
    }
    assert_eq!(sim.outcome(), Outcome::Lost);
    assert_eq!(sim.frame().hud.lines, ["Shards 0/5", "Lives 0", "Game over"]);
}

#[test]
fn snapshots_rewind_and_replays_are_exact() {
    let mut sim = Sim::new("starter", 3).unwrap();
    for _ in 0..240 {
        let f = sim.bot_input();
        sim.step(&f);
    }
    // A snapshot file (CBOR) restores the game, and both copies then play on the same.
    let mut bytes = Vec::new();
    ciborium::into_writer(&sim.snapshot(), &mut bytes).unwrap();
    let state: SimState = ciborium::from_reader(bytes.as_slice()).unwrap();
    let mut copy = Sim::new("starter", 3).unwrap();
    copy.restore(state);
    assert_eq!(copy.state_hash(), sim.state_hash());
    for _ in 0..240 {
        let (a, b) = (sim.bot_input(), copy.bot_input());
        sim.step(&a);
        copy.step(&b);
    }
    assert_eq!(copy.state_hash(), sim.state_hash());

    let (tick, hash) = (sim.state.tick, sim.state_hash());
    assert!(sim.rewind_to(tick - 90));
    assert!(sim.rewind_to(tick));
    assert_eq!(sim.state_hash(), hash);

    let mut again = Sim::new("starter", 3).unwrap();
    for f in sim.recording.iter() {
        again.step(f);
    }
    assert_eq!(again.state_hash(), hash);
}

#[test]
fn game_keys_name_one_scene_each() {
    let names = pav_core::scenes::names();
    for g in GAMES {
        assert_eq!(names.iter().filter(|n| *n == g.key).count(), 1, "'{}' is also a scene or room key", g.key);
        assert!(!g.key.contains('/'), "'{}': `<key>/<rest>` passes the rest to the game", g.key);
        assert!(Sim::new(g.key, 1).unwrap().play().is_some_and(|p| p.key() == g.key));
    }
}
