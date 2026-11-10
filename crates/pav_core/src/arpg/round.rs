//! Rounds on the AI Hub (Session Protocol v1, docs/AI_HUB.md): the game's side. A round
//! starts every player on depth 1 with a fresh hero; every level is built from the round's
//! seed and its depth alone, so each player's copy has the same levels. The app
//! (`pav_app::round`) runs the round: the hub's clock, scores, ghosts and the shared world.

use super::{Game, Place, hero::Hero};
use crate::Sim;

/// The characters by seat: each player's hero looks different.
pub const SEAT_CHARACTERS: [&str; 4] = ["wanderer", "kestrel", "ysolde", "wanderer"];

/// A round's world: depth `depth`, a fresh hero of `character`, levels from `seed`.
pub fn round_sim(seed: u64, depth: u32, character: &str) -> Sim {
    let mut sim = Sim::empty(seed);
    let d = super::data::data();
    let hero = Hero::new_character(&d, character);
    let mut g = Game::new(hero);
    g.round_seed = Some(seed);
    super::world::build_level(&mut sim, depth.max(1), Some(g));
    sim.state.scene = format!("level/{}", depth.max(1));
    sim.recording.scene = sim.state.scene.clone();
    sim
}

/// How far a player got: the depth they are on and the monsters they slew.
pub fn standing(g: &Game) -> (u32, u64) {
    let depth = match g.place {
        Place::Level(n) => n,
        _ => 0,
    };
    (depth, g.hero.kills)
}

/// A standing as a score the hub ranks: depth first, then kills.
pub fn score(depth: u32, kills: u64) -> f64 {
    depth as f64 * 1000.0 + kills.min(999) as f64
}
