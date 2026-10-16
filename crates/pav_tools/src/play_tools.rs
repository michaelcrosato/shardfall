//! Tools for games built on the engine (`pav_core::games`): the starter and any game an agent
//! adds there.

use anyhow::{Result, bail};
use pav_core::Sim;
use pav_core::games::{GAMES, Outcome};
use serde_json::json;

use crate::session::Session;
use crate::tools::{Args, Output, get_bool, get_u64};

/// `play`: the running game's HUD, outcome and state; `bot=true` lets its bot play on;
/// `seeds=N` plays fresh runs on seeds 1..N with the bot and leaves the session alone.
pub fn t_play(s: &mut Session, a: &Args) -> Result<Output> {
    let keys = || GAMES.iter().map(|g| g.key).collect::<Vec<_>>().join(", ");
    let Some(key) = s.sim.play().map(|g| g.key()) else {
        bail!("no game is running: `load scene=<key>` with one of: {}", keys());
    };
    let bot = get_bool(a, "bot", false)?;
    let ticks = get_u64(a, "ticks", 3600)?;
    let hz = s.sim.config.tick_rate.hz() as f64;
    if a.contains_key("seeds") {
        if !bot {
            bail!("seeds= runs the bot: add bot=true");
        }
        let n = get_u64(a, "seeds", 1)?.clamp(1, 200);
        let scene = s.sim.state.scene.clone();
        let (mut runs, mut not_won) = (Vec::new(), Vec::new());
        let mut wins = 0;
        for seed in 1..=n {
            let mut sim = Sim::new(&scene, seed)?;
            sim.config = s.sim.config.clone();
            let (outcome, played) = sim.run_bot(ticks);
            runs.push(json!([seed, outcome, round2(played as f64 / hz)]));
            if outcome == Outcome::Won {
                wins += 1;
            } else {
                // The state where the bot lost or ran out of time, to see why.
                not_won.push(json!({ "seed": seed, "status": sim.play().map(|g| g.status(&sim)) }));
            }
        }
        return Ok(Output::Json(json!({
            "game": key,
            "wins": format!("{wins}/{n}"),
            "cols": "seed outcome seconds",
            "runs": runs,
            "not_won": not_won,
        })));
    }
    let mut played = None;
    if bot {
        let (_, t) = s.sim.run_bot(ticks);
        s.keep_events();
        s.sync_camera();
        played = Some(round2(t as f64 / hz));
    }
    let g = s.sim.play().expect("the game is still running");
    Ok(Output::Json(json!({
        "game": key,
        "outcome": g.outcome(),
        "bot_seconds": played,
        "time": round2(s.sim.time()),
        "hud": g.hud(&s.sim),
        "status": g.status(&s.sim),
    })))
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
