//! The smallest complete game: walk around a yard, push crates, collect coins. Copy this file
//! to start a new game (then add it to `GAMES` in `mod.rs`).
use pavlite::prelude::*;

// The level as text: an ASCII map plus a legend (see AGENTS.md, "Levels").
const LEVEL: &str = r##"
name = "Template yard"

[[layer]]
map = """
##########
#P.......#
#..c..B..#
#........#
#..B..c..#
#........#
##########
"""

[legend]
"#" = { block = { y0 = 0, y1 = 1.5, color = "#8a8f99" } }
"." = { block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }
"P" = { marker = "player", block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }
"c" = { spawn = { kind = "coin", shape = "cylinder", size = [0.1, 0.35], body = "trigger", color = "#ffd34d", look = "glow", y = 0.8, rot = [90, 0, 0], spin = 120 }, block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }
"B" = { spawn = { kind = "crate", size = 0.8, color = "#a0703c" }, block = { y0 = -0.5, y1 = 0, color = "#d9cbb0" } }
"##;

/// All game state lives here (it is cloned for rewinds, so keep it plain data).
#[derive(Clone, Default)]
pub struct Template {
    score: u32,
}

impl Game for Template {
    /// Build the world: level, then the player.
    fn setup(&mut self, w: &mut World) {
        w.load_level(LEVEL).expect("level");
        let start = w.marker("player").unwrap_or(Vec3::ZERO);
        w.player = Some(w.spawn(Spawn::character("player", start)));
    }

    /// Rules, once per tick (60 per second). `w.events` says what happened last tick.
    fn update(&mut self, w: &mut World, _input: &Input) {
        // Coins the player touched last tick (for everything else, read `w.events`).
        for coin in w.player_entered("coin") {
            let pos = w.get(coin).unwrap().pos;
            w.despawn(coin);
            w.burst(pos, "#ffd34d", 20, 3.0);
            self.score += 1;
        }
    }

    /// HUD and extra drawing (read-only).
    fn draw(&self, w: &World, d: &mut Draw) {
        d.text(8.0, 8.0, 16.0, "#ffffff", &format!("Coins: {}", self.score));
        if w.count("coin") == 0 {
            d.title(160.0, 24.0, "#ffd34d", "All coins!");
        }
    }

    /// What agents see in `status`.
    fn status(&self, w: &World) -> Value {
        json!({ "score": self.score, "coins_left": w.count("coin") })
    }

    /// A scripted player for `autoplay` and tests: walk to the nearest coin.
    fn bot(&mut self, w: &World) -> Option<Input> {
        let me = w.player()?.pos;
        match w.nearest("coin", me, 100.0).and_then(|id| w.get(id)) {
            Some(coin) => Some(Input::toward(me, coin.pos)),
            None => Some(Input::default()), // all collected: stand still
        }
    }
}

#[cfg(test)]
mod tests {
    use pavlite::sim::Sim;

    #[test]
    fn bot_collects_every_coin() {
        let mut sim = Sim::new(crate::games::def("template"), 1);
        for _ in 0..60 * 20 {
            let input = sim.game.bot(&sim.world).unwrap_or_default();
            sim.step(&input);
        }
        assert_eq!(sim.game.status(&sim.world)["coins_left"], 0);
    }
}
