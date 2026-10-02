//! Side-view platformer: run right, jump gaps and spikes, stomp blobs, ride the lift, collect
//! coins, touch checkpoints, reach the goal. Shows: a side-view ASCII level (`plane = "xy"`),
//! parameter overrides from the level, triggers and events, a moving platform, simple enemy
//! brains driving characters, drawing extra shapes and a HUD, and a bot that finishes the level.

use std::collections::BTreeMap;

use pavlite::prelude::*;
use pavlite::render::Prim;

const LEVEL: &str = r##"
name = "Blob Hills"
sky = "#6fb6ec"
horizon = "#e3f1e0"

[params]
"movement.model" = "momentum"
"movement.speed" = 7.5
"movement.accel" = 60
"movement.decel" = 45
"movement.skid" = 110
"movement.air_control" = 0.85
"movement.jump_height" = 2.6
"movement.gravity" = 40
"movement.jump_buffer" = 0.15
"movement.lock_axis" = "z"
"camera.tilt" = 8
"camera.distance" = 17
"camera.fov" = 38
"camera.height" = 1.6
"sim.kill_y" = -6

[[layer]]
plane = "xy"
map = """
#                                                                                    #
#                                                     ccc                            #
#                    ccc                             gggg               cccc         #
#         cc        =====                  cc       g####               ====         #
# P  ccc  gg              e      k ^^           k  g#####  e      ^^  e          F   #
gggggggggg##ggg   gggggggggggg  ggggggg ~~     gggg######gggg   gggggggggggggggggggggg
"""

# Hills far behind the play plane (decoration: nothing walks back there).
[[layer]]
plane = "xy"
z = -9
map = """
          hh                         hhh                        hh
       hhhhhhh                    hhhhhhhhh       hh         hhhhhhhh
   hhhhhhhhhhhhhh      hhhh    hhhhhhhhhhhhhhh  hhhhhh    hhhhhhhhhhhhhhh       hhhhh
hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh
"""

[legend]
"#" = { block = { color = "#7a5434", z0 = -7 } }
"g" = { block = { color = "#5fa83a", z0 = -7 } }
"h" = { block = { color = "#9cc78a", look = "flat", z0 = -2, z1 = 2 } }
"=" = { block = { y0 = 0.8, color = "#b07a43", oneway = true } }
"~" = { block = { y0 = 0.8, color = "#e0a83a", kind = "lift", move = [4, 0, 0], period = 5, hold = 0.6 } }
"^" = { trigger = { kind = "spikes", y1 = 0.5 } }
"c" = { spawn = { kind = "coin", shape = "cylinder", size = [0.12, 0.3], body = "trigger", color = "#ffd34d", look = "glow", y = 0.6, rot = [90, 0, 0], spin = 180 } }
"k" = { spawn = { kind = "checkpoint", size = [0.25, 1.6, 0.25], body = "trigger", color = "#4a8fe8", look = "glow" } }
"F" = { spawn = { kind = "goal", size = [0.3, 3.0, 0.3], body = "trigger", color = "#f2f2f2" } }
"e" = { marker = "enemy" }
"P" = { marker = "player" }
"##;

#[derive(Clone, Default)]
pub struct Platformer {
    coins: u32,
    total: u32,
    deaths: u32,
    stomps: u32,
    /// Feet position to come back to.
    checkpoint: Vec3,
    /// Time the goal was reached.
    won: Option<f32>,
    /// Blob walking directions (-1 left, +1 right).
    patrol: BTreeMap<Id, f32>,
    /// Bot: airborne on purpose across a gap (don't brake mid-air).
    bot_leap: bool,
}

impl Platformer {
    /// Bot: does a fall from `pos` with velocity `vel` end in a pit or on spikes?
    fn bad_landing(&self, w: &World, pos: Vec3, vel: Vec3) -> bool {
        let g = w.config.movement.gravity;
        let (mut p, mut v) = (pos, vel);
        for _ in 0..120 {
            v.y -= g / 30.0;
            p += v / 30.0;
            if p.y < w.config.kill_y {
                return true;
            }
            if w.ground_at(p.x, 0.0, p.y + 0.5).is_some_and(|gy| gy >= p.y - 0.05) {
                return w
                    .each("spikes")
                    .any(|s| (s.pos.x - p.x).abs() < s.shape.half_extents().x + 0.3 && (s.pos.y - p.y).abs() < 1.0);
            }
        }
        false
    }

    fn die(&mut self, w: &mut World, pid: Id) {
        if let Some(p) = w.get(pid) {
            let at = p.center();
            w.burst(at, "#e8704a", 30, 5.0);
        }
        self.deaths += 1;
        w.set_pos(pid, self.checkpoint);
        w.shake(0.6);
    }
}

impl Game for Platformer {
    fn setup(&mut self, w: &mut World) {
        w.load_level(LEVEL).expect("level");
        self.checkpoint = w.marker("player").unwrap_or(Vec3::Y);
        let hero = Spawn::character("player", self.checkpoint).puppet(Puppet::biped("#e8704a").hat("#3a6fd8"));
        w.player = Some(w.spawn(hero));
        for at in w.markers_named("enemy") {
            let blob = Spawn::character("blob", at).size(0.9, 0.42).agility(0.35, 0.0).puppet(Puppet::blob("#8a4ad8"));
            w.spawn(blob);
        }
        self.total = w.count("coin") as u32;
    }

    fn update(&mut self, w: &mut World, _input: &Input) {
        let Some(pid) = w.player else { return };
        for ev in w.events.clone() {
            match ev {
                Event::Enter { trigger, other } if other == pid => {
                    let Some(t) = w.get(trigger) else { continue };
                    let (kind, pos, half) = (t.kind.clone(), t.pos, t.shape.half_extents());
                    match kind.as_str() {
                        "coin" => {
                            w.despawn(trigger);
                            w.burst(pos, "#ffd34d", 16, 3.0);
                            self.coins += 1;
                        }
                        "spikes" => self.die(w, pid),
                        "checkpoint" => {
                            self.checkpoint = pos - Vec3::Y * half.y;
                            if let Some(e) = w.get_mut(trigger) {
                                e.color = Color::hex("#5ce06a");
                            }
                        }
                        "goal" if self.won.is_none() => {
                            self.won = Some(w.time());
                            w.act(pid, Act::Cheer, 3.0);
                            w.burst(pos + Vec3::Y * 1.5, "#ffd34d", 60, 7.0);
                        }
                        _ => {}
                    }
                }
                Event::Fell { id } if id == pid => self.die(w, pid),
                Event::Fell { id } => {
                    w.despawn(id);
                }
                _ => {}
            }
        }
        if self.won.is_some() {
            w.drive(pid, Input::default());
        }

        // Blobs walk back and forth, turning at walls and edges. Landing on one squashes it;
        // touching it from the side sends you back to the checkpoint.
        let Some(p) = w.get(pid) else { return };
        let (ppos, pvel) = (p.pos, p.vel);
        for id in w.ids("blob") {
            let pos = w.get(id).unwrap().pos;
            let mut dir = *self.patrol.entry(id).or_insert(-1.0);
            let wall = w.raycast(pos + Vec3::Y * 0.4, Vec3::X * dir, 0.7, Some(id)).is_some_and(|h| Some(h.id) != w.player);
            let edge = w.ground_at(pos.x + dir * 0.7, pos.z, pos.y + 0.5).is_none_or(|y| y < pos.y - 0.6);
            if wall || edge {
                dir = -dir;
                self.patrol.insert(id, dir);
            }
            w.drive(id, Input { move_dir: Vec2::new(dir, 0.0), ..Default::default() });
            let d = ppos - pos;
            if d.x.abs() < 0.8 && d.y > 0.45 && d.y < 1.3 && pvel.y <= 0.5 {
                w.despawn(id);
                w.burst(pos + Vec3::Y * 0.4, "#b07af0", 24, 4.0);
                w.set_vel(pid, Vec3::new(pvel.x, 11.0, 0.0));
                self.stomps += 1;
            } else if d.x.abs() < 0.7 && d.y.abs() < 0.8 && self.won.is_none() {
                self.die(w, pid);
                break;
            }
        }
    }

    fn draw(&self, w: &World, d: &mut Draw) {
        // Spikes are invisible triggers in the level; draw a row of cones on each.
        for s in w.each("spikes") {
            let half = s.shape.half_extents();
            let n = (half.x * 2.0 / 0.33).round().max(1.0) as i32;
            for i in 0..n {
                let x = s.pos.x - half.x + (i as f32 + 0.5) * half.x * 2.0 / n as f32;
                let base = Vec3::new(x, s.pos.y - half.y, 0.0);
                d.shape(Prim::Cone { a: base, b: base + Vec3::Y * 0.55, ra: 0.16, rb: 0.0 }, "#c9ccd6", Look::Lit);
            }
        }
        for g in w.each("goal") {
            let top = g.pos + Vec3::Y * 1.3;
            d.cube(top + Vec3::new(0.45, 0.0, 0.0), Vec3::new(0.8, 0.5, 0.06), "#e8402a", Look::Cel);
        }
        let time = self.won.unwrap_or(w.time());
        let hud = format!("Coins {}/{}   Deaths {}   Time {:.1}", self.coins, self.total, self.deaths, time);
        d.text(10.0, 10.0, 16.0, "#ffffff", &hud);
        if let Some(t) = self.won {
            d.title(120.0, 32.0, "#ffd34d", "GOAL!");
            d.title(165.0, 16.0, "#ffffff", &format!("{t:.1} s   coins {}/{}   deaths {}", self.coins, self.total, self.deaths));
        }
    }

    fn status(&self, w: &World) -> Value {
        let x = w.player().map(|p| p.pos.x).unwrap_or(0.0);
        let goal = w.each("goal").next().map(|g| g.pos.x).unwrap_or(1.0);
        json!({
            "coins": self.coins, "total": self.total, "deaths": self.deaths, "stomps": self.stomps,
            "won": self.won.is_some(), "time": self.won.unwrap_or(w.time()),
            "progress": (x / goal).clamp(0.0, 1.0),
        })
    }

    /// Runs right; jumps at walls, spikes, blobs and gaps it can clear; waits at gaps it
    /// can't (until the lift comes).
    fn bot(&mut self, w: &World) -> Option<Input> {
        let p = w.player()?;
        let ch = p.character.as_ref()?;
        if self.won.is_some() {
            return Some(Input::default());
        }
        let pos = p.pos;
        let mut input = Input { move_dir: Vec2::X, ..Default::default() };
        if !ch.grounded {
            if ch.vel.y > 0.0 {
                input.held = buttons::JUMP;
            }
            // Hopping a hazard: predict where we come down and brake if that is a pit or
            // spikes (a full jump is 5 m long).
            if !self.bot_leap && self.bad_landing(w, pos, ch.vel) {
                input.move_dir = -Vec2::X;
            }
            return Some(input);
        }
        self.bot_leap = false;
        // Somewhere to stand at dx ahead (not too high, not a deadly drop), and whether it moves.
        let ground = |dx: f32| -> Option<bool> {
            let x = pos.x + dx;
            let y = w.ground_at(x, 0.0, pos.y + 2.5).filter(|y| *y > pos.y - 6.0 && *y > w.config.kill_y)?;
            let top = w.raycast(Vec3::new(x, y + 0.05, 0.0), Vec3::NEG_Y, 0.2, None).and_then(|h| w.get(h.id));
            Some(top.is_some_and(|e| e.mover.is_some()))
        };
        let samples: Vec<Option<bool>> = (1..=11).map(|k| ground(k as f32 * 0.5)).collect();
        let gap_at = samples.iter().position(|g| g.is_none()).map(|i| (i + 1) as f32 * 0.5);
        // The first place to stand after the gap: (distance, is it the lift?).
        let landing = gap_at.and_then(|g| {
            samples.iter().enumerate().skip((g / 0.5) as usize).find_map(|(i, s)| s.map(|m| ((i + 1) as f32 * 0.5, m)))
        });
        let wall = [0.4, 1.5].iter().any(|h| {
            w.raycast(pos + Vec3::Y * *h, Vec3::X, 1.0, Some(p.id))
                .is_some_and(|hit| w.get(hit.id).is_some_and(|e| e.character.is_none() && e.body != Body::Trigger))
        });
        // Hazards: jump when their near edge is this close (jumping earlier overshoots).
        let danger = |kind: &str, reach: f32| {
            w.each(kind).any(|e| {
                let near = e.pos.x - e.shape.half_extents().x - pos.x;
                near > -0.3 && near < reach && (e.pos.y - pos.y).abs() < 1.0
            })
        };
        let mut jump = wall || danger("spikes", 1.0) || danger("blob", 1.6);
        if let Some(g) = gap_at {
            if g <= 1.0 {
                match landing {
                    // A lift keeps moving: only jump when it is right next to the edge.
                    Some((d, true)) if d > 2.0 => input.move_dir = Vec2::ZERO,
                    Some(_) => {
                        jump = true;
                        self.bot_leap = true;
                    }
                    None => input.move_dir = Vec2::ZERO, // wait for the lift
                }
            }
        }
        if jump {
            input.pressed = buttons::JUMP;
            input.held = buttons::JUMP;
        }
        Some(input)
    }
}

#[cfg(test)]
mod tests {
    use pavlite::sim::Sim;

    #[test]
    fn bot_reaches_the_goal() {
        let mut sim = Sim::new(crate::games::def("platformer"), 1);
        for _ in 0..60 * 120 {
            let input = sim.game.bot(&sim.world).unwrap();
            sim.step(&input);
            if sim.game.status(&sim.world)["won"] == true {
                break;
            }
        }
        let st = sim.game.status(&sim.world);
        assert_eq!(st["won"], true, "{st}");
    }
}
