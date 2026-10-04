//! Top-down arena shooter: survive waves of runners, shooters and brutes. Shows: a top-down
//! level with cover, mouse aim and projectiles, enemy brains (chase, keep distance, shoot)
//! steering around walls, health, invulnerability, pickups, dash, waves, restarting from
//! inside the game, a HUD with bars, and a bot that plays it.

use std::collections::BTreeMap;

use pavlite::prelude::*;

const LEVEL: &str = r##"
name = "The Pit"
sky = "#2a3350"
horizon = "#4a4f6a"

[params]
"movement.speed" = 7
"movement.allow_jump" = false
"movement.face_aim" = true
"camera.tilt" = 64
"camera.distance" = 24
"camera.height" = 0
"env.sun_elevation" = 62
"env.ambient" = 0.55

[[layer]]
map = """
################################
#S.............S..............S#
#..............................#
#..............................#
#....OO..................OO....#
#....OO..................OO....#
#..............................#
#...........--------...........#
#..............................#
#..............................#
#S.......P....++++.............#
#.............++++............S#
#..............................#
#..............................#
#...........--------...........#
#..............................#
#....OO..................OO....#
#....OO..................OO....#
#..............................#
#..............................#
#S..............S.............S#
################################
"""

[legend]
"#" = { blocks = [{ y0 = -0.5, y1 = 2, color = "#5a5f73" }] }
"." = { block = { y0 = -0.5, y1 = 0, color = "#c9b48f" } }
"P" = { marker = "player", block = { y0 = -0.5, y1 = 0, color = "#c9b48f" } }
"S" = { marker = "spawn", block = { y0 = -0.5, y1 = 0, color = "#8f5a4a" } }
"O" = { blocks = [{ y0 = -0.5, y1 = 2.6, color = "#7d8296", inset = 0.1 }] }
"-" = { blocks = [{ y0 = -0.5, y1 = 0.8, color = "#9a7a52" }] }
"+" = { block = { y0 = -0.5, y1 = 0.3, color = "#b8a07a" } }
"##;

const PLAYER_TEAM: u8 = 1;
const ENEMY_TEAM: u8 = 2;

/// Per-enemy brain state (game state lives in the game struct, keyed by entity id).
#[derive(Clone, Debug, Default)]
struct Brain {
    /// Seconds until it may attack again.
    cooldown: f32,
    /// Strafe direction for shooters (-1 / +1).
    side: f32,
    /// Waypoints around walls when the player is out of sight, and when to plan again.
    path: Vec<Vec3>,
    repath: f32,
}

#[derive(Clone, Default)]
pub struct Arena {
    wave: u32,
    score: u32,
    kills: u32,
    /// Enemies still to spawn this wave, and seconds until the next one.
    queue: Vec<&'static str>,
    spawn_timer: f32,
    /// Seconds before the next wave starts.
    rest: f32,
    fire_cd: f32,
    nova_cd: f32,
    dash_cd: f32,
    over: bool,
    brains: BTreeMap<Id, Brain>,
    /// Damage of each weapon (tunable: `game.damage`).
    damage: f32,
    /// Where walkers fit (built once from the level).
    nav: Option<NavGrid>,
}

impl Arena {
    fn start_wave(&mut self, w: &mut World) {
        self.wave += 1;
        let n = 3 + self.wave * 2;
        self.queue = (0..n)
            .map(|i| match (self.wave, i % 5) {
                (w, 4) if w >= 3 => "brute",
                (w, 2 | 3) if w >= 2 => "shooter",
                _ => "runner",
            })
            .collect();
        self.spawn_timer = 0.5;
        w.shake(0.3);
    }

    fn spawn_enemy(&mut self, w: &mut World, kind: &'static str) {
        let spots = w.markers_named("spawn");
        let at = spots[w.rng.below(spots.len() as u32) as usize];
        let scale = 1.0 + self.wave as f32 * 0.06;
        let sp = match kind {
            "runner" => {
                Spawn::character(kind, at).size(1.0, 0.38).agility(0.78, 0.0).hp(3.0 * scale).puppet(Puppet::beast("#c0583a"))
            }
            "shooter" => Spawn::character(kind, at)
                .agility(0.5, 0.0)
                .hp(4.0 * scale)
                .puppet(Puppet::biped("#6a3ad8").held(Held::Staff).hat("#2a1a5a").colors("#c9a0f0", "#2a2140")),
            _ => Spawn::character(kind, at)
                .size(1.5, 0.7)
                .agility(0.38, 0.0)
                .hp(14.0 * scale)
                .puppet(Puppet::blob("#3aa860").scale(1.7)),
        };
        let id = w.spawn(sp.team(ENEMY_TEAM));
        let side = if w.rng.chance(0.5) { 1.0 } else { -1.0 };
        self.brains.insert(id, Brain { cooldown: 1.0, side, ..Default::default() });
        w.burst(at + Vec3::Y, "#ff9a5a", 16, 4.0);
    }

    fn restart(&mut self, w: &mut World) {
        *w = World::new(w.seed + 1);
        *self = Arena::default();
        self.setup(w);
    }
}

/// A direction toward `to` that is not blocked by walls: straight, else turned 30-90 degrees.
fn steer(w: &World, me: Id, from: Vec3, to: Vec3) -> Vec2 {
    let d = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let dist = d.length();
    if dist < 0.01 {
        return Vec2::ZERO;
    }
    let base = d / dist;
    for deg in [0.0f32, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0] {
        let dir = Quat::from_rotation_y(deg.to_radians()) * base;
        let blocked = w
            .raycast(from + Vec3::Y * 0.5, dir, dist.min(1.6), Some(me))
            .is_some_and(|h| w.get(h.id).is_some_and(|e| e.character.is_none()));
        if !blocked {
            return Vec2::new(dir.x, dir.z);
        }
    }
    Vec2::new(base.x, base.z)
}

impl Game for Arena {
    fn setup(&mut self, w: &mut World) {
        w.load_level(LEVEL).expect("level");
        let start = w.marker("player").unwrap_or(Vec3::ZERO);
        let hero = Spawn::character("player", start).hp(10.0).team(PLAYER_TEAM).puppet(Puppet::biped("#3a8fd8").held(Held::Gun));
        w.player = Some(w.spawn(hero));
        self.damage = 1.0;
        self.rest = 1.0;
        self.nav = Some(NavGrid::build(w, Vec3::ZERO, Vec3::new(32.0, 0.0, 22.0), 0.5, 0.45));
    }

    fn update(&mut self, w: &mut World, input: &Input) {
        let dt = w.dt;
        let Some(pid) = w.player else { return };
        if self.over {
            if input.just(buttons::USE) || input.just(buttons::JUMP) || input.just(buttons::FIRE) {
                self.restart(w);
            }
            w.drive(pid, Input::default());
            return;
        }
        for ev in w.events.clone() {
            match ev {
                Event::Killed { id, .. } if Some(id) != w.player => {
                    let Some(e) = w.get(id) else { continue };
                    let (pos, kind) = (e.center(), e.kind.clone());
                    self.score += match kind.as_str() {
                        "brute" => 50,
                        "shooter" => 20,
                        _ => 10,
                    };
                    self.kills += 1;
                    w.burst(pos, "#ffb04a", 30, 6.0);
                    w.despawn(id);
                    self.brains.remove(&id);
                    if w.rng.chance(0.18) {
                        let heart = Spawn::new("heart", Vec3::new(pos.x, 0.6, pos.z))
                            .ball(0.28)
                            .body(Body::Trigger)
                            .color("#ff4a6a")
                            .look(Look::Glow)
                            .life(12.0);
                        w.spawn(heart);
                    }
                }
                Event::Hit { target, pos, .. } if Some(target) == w.player => {
                    w.burst(pos, "#ff4a4a", 10, 3.0);
                    w.shake(0.5);
                    if let Some(p) = w.get_mut(target) {
                        p.invuln = p.invuln.max(0.4);
                    }
                }
                Event::Hit { pos, .. } => w.burst(pos, "#9ef0ff", 6, 2.5),
                Event::Enter { trigger, other } if Some(other) == w.player => {
                    if w.get(trigger).is_some_and(|e| e.kind == "heart") {
                        w.despawn(trigger);
                        if let Some(p) = w.get_mut(other) {
                            p.hp = (p.hp + 3.0).min(p.max_hp);
                        }
                        w.burst(w.get(other).map(|p| p.center()).unwrap_or_default(), "#ff4a6a", 20, 3.0);
                    }
                }
                _ => {}
            }
        }

        // The player: shoot where the mouse aims, nova burst, dash.
        let Some(p) = w.get(pid) else { return };
        let (ppos, pc) = (p.pos, p.center());
        let facing = p.character.as_ref().map(|c| c.forward()).unwrap_or(Vec3::Z);
        if p.hp <= 0.0 {
            self.over = true;
            w.burst(pc, "#ff4a4a", 60, 7.0);
            w.act(pid, Act::Cheer, 0.1);
            return;
        }
        self.fire_cd -= dt;
        self.nova_cd -= dt;
        self.dash_cd -= dt;
        let aim_dir = input.aim.map(|a| Vec3::new(a.x - pc.x, 0.0, a.z - pc.z).normalize_or(facing)).unwrap_or(facing);
        if input.down(buttons::FIRE) && self.fire_cd <= 0.0 {
            self.fire_cd = 0.11;
            let from = pc + Vec3::Y * 0.25 + aim_dir * 0.5;
            w.shoot(Shot::new(from, aim_dir * 24.0).owner(pid).team(PLAYER_TEAM).damage(self.damage).knockback(2.0).life(1.2));
            w.act(pid, Act::Shoot, 0.15);
        }
        if input.just(buttons::ALT) && self.nova_cd <= 0.0 {
            self.nova_cd = 3.0;
            for i in 0..20 {
                let d = Quat::from_rotation_y(i as f32 / 20.0 * std::f32::consts::TAU) * Vec3::Z;
                w.shoot(
                    Shot::new(pc + d * 0.5, d * 16.0)
                        .owner(pid)
                        .team(PLAYER_TEAM)
                        .damage(self.damage * 2.0)
                        .radius(0.18)
                        .color("#ffd34d")
                        .knockback(6.0)
                        .life(0.8),
                );
            }
            w.shake(0.4);
        }
        if input.just(buttons::DASH) && self.dash_cd <= 0.0 {
            self.dash_cd = 0.8;
            let dir = if input.move_dir.length() > 0.1 { input.move3() } else { aim_dir };
            w.dash(pid, dir, 17.0, 0.18);
            if let Some(p) = w.get_mut(pid) {
                p.invuln = p.invuln.max(0.3);
            }
        }

        // Waves.
        if !self.queue.is_empty() {
            self.spawn_timer -= dt;
            if self.spawn_timer <= 0.0 {
                let kind = self.queue.remove(0);
                self.spawn_enemy(w, kind);
                self.spawn_timer = 0.6;
            }
        } else if self.brains.is_empty() {
            self.rest -= dt;
            if self.rest <= 0.0 {
                self.rest = 3.0;
                self.start_wave(w);
            }
        }

        // Enemy brains: runners and brutes chase and hit, shooters keep their distance and fire.
        let ids: Vec<Id> = self.brains.keys().copied().collect();
        for id in ids {
            let Some(e) = w.get(id) else {
                self.brains.remove(&id);
                continue;
            };
            let (pos, center, kind, radius) =
                (e.pos, e.center(), e.kind.clone(), e.character.as_ref().map(|c| c.radius).unwrap_or(0.4));
            let brain = self.brains.get_mut(&id).unwrap();
            brain.cooldown -= dt;
            let to_player = Vec3::new(ppos.x - pos.x, 0.0, ppos.z - pos.z);
            let dist = to_player.length();
            let mut input = Input { aim: Some(pc), ..Default::default() };
            if kind == "shooter" {
                let side = brain.side;
                let sees = w.can_see(center, pc, &[id, pid]);
                let goal = if dist < 6.0 {
                    pos - to_player
                } else if dist > 10.0 || !sees {
                    ppos
                } else {
                    pos + Quat::from_rotation_y(side * 1.2) * to_player.normalize_or_zero() * 2.0
                };
                input.move_dir = steer(w, id, pos, goal);
                let brain = self.brains.get_mut(&id).unwrap();
                if sees && dist < 14.0 && brain.cooldown <= 0.0 {
                    brain.cooldown = 1.7;
                    let dir = to_player.normalize_or(Vec3::Z);
                    let shot = Shot::new(center + Vec3::Y * 0.3 + dir * 0.6, dir * 9.0)
                        .owner(id)
                        .team(ENEMY_TEAM)
                        .radius(0.26)
                        .color("#c84aff")
                        .damage(1.0)
                        .knockback(3.0)
                        .life(2.5);
                    w.shoot(shot);
                    w.act(id, Act::Shoot, 0.3);
                }
            } else {
                // Straight at the player when in sight, else along a path around the walls.
                let brain = self.brains.get_mut(&id).unwrap();
                brain.repath -= dt;
                let mut goal = ppos;
                if !w.can_see(center, pc, &[id, pid]) {
                    if brain.repath <= 0.0 || brain.path.is_empty() {
                        brain.repath = 0.5;
                        brain.path = self.nav.as_ref().and_then(|n| n.path(pos, ppos)).unwrap_or_default();
                    }
                    while brain.path.len() > 1 && brain.path[0].distance(Vec3::new(pos.x, 0.0, pos.z)) < 0.5 {
                        brain.path.remove(0);
                    }
                    if let Some(next) = brain.path.first() {
                        goal = *next;
                    }
                } else {
                    brain.path.clear();
                }
                input.move_dir = steer(w, id, pos, goal);
                let reach = radius + 0.32 + 0.35;
                let brain = self.brains.get_mut(&id).unwrap();
                if dist < reach && brain.cooldown <= 0.0 {
                    brain.cooldown = 0.9;
                    let dmg = if kind == "brute" { 2.0 } else { 1.0 };
                    w.act(id, Act::Swing, 0.35);
                    if w.damage(pid, dmg) {
                        w.burst(pc, "#ff4a4a", 30, 6.0);
                    }
                    if w.get(pid).is_some_and(|p| p.invuln <= 0.0) {
                        let push = to_player.normalize_or(Vec3::Z) * if kind == "brute" { 11.0 } else { 6.0 };
                        w.push(pid, push, 0.2);
                        if let Some(p) = w.get_mut(pid) {
                            p.invuln = 0.5;
                        }
                        w.shake(0.6);
                    }
                }
            }
            w.drive(id, input);
        }
    }

    fn draw(&self, w: &World, d: &mut Draw) {
        let Some(p) = w.player() else { return };
        for e in w.entities.values().filter(|e| e.team == ENEMY_TEAM && e.max_hp > 0.0) {
            let top = e.pos + Vec3::Y * (e.character.as_ref().map(|c| c.height).unwrap_or(1.0) + 0.3);
            d.health(top, e.hp / e.max_hp, "#ff5a4a");
        }
        if let Some(a) = p.character.as_ref().and_then(|c| c.input.aim) {
            d.ring(Vec3::new(a.x, 0.02, a.z), 0.35, "#9ef0ff");
        }
        d.text(10.0, 10.0, 16.0, "#ffffff", &format!("Wave {}   Score {}", self.wave, self.score));
        d.bar(10.0, 32.0, 140.0, 10.0, p.hp / p.max_hp.max(1.0), "#ff4a6a");
        d.text(156.0, 32.0, 8.0, "#ffffff", &format!("{:.0}/{:.0}", p.hp.max(0.0), p.max_hp));
        let ready = |cd: f32| if cd <= 0.0 { "#9ef0ff" } else { "#5a6070" };
        d.text(10.0, 48.0, 8.0, ready(self.nova_cd), "ALT: nova");
        d.text(90.0, 48.0, 8.0, ready(self.dash_cd), "SHIFT: dash");
        if self.over {
            d.rect(0.0, 130.0, d.width, 90.0, "#101216", 0.7);
            d.title(145.0, 32.0, "#ff5a4a", "GAME OVER");
            d.title(
                190.0,
                8.0,
                "#ffffff",
                &format!("wave {}  score {}  kills {}  -  fire to restart", self.wave, self.score, self.kills),
            );
        } else if self.queue.is_empty() && self.brains.is_empty() && self.wave > 0 {
            d.title(150.0, 16.0, "#ffd34d", &format!("Wave {} cleared!", self.wave));
        }
    }

    fn status(&self, w: &World) -> Value {
        json!({
            "wave": self.wave, "score": self.score, "kills": self.kills, "over": self.over,
            "hp": w.player().map(|p| p.hp).unwrap_or(0.0),
            "enemies": self.brains.len(), "to_spawn": self.queue.len(),
        })
    }

    fn params(&mut self, v: &mut dyn ParamVisitor) {
        v.float("damage", &mut self.damage, 0.1, 20.0, "Player weapon damage");
    }

    /// Shoots the nearest visible enemy while keeping 6-9 m away and circling; dashes away
    /// when cornered, novas when crowded, grabs hearts when hurt, restarts after dying.
    fn bot(&mut self, w: &World) -> Option<Input> {
        let p = w.player()?;
        let (pos, pc) = (p.pos, p.center());
        if self.over {
            return Some(Input { pressed: buttons::FIRE, held: buttons::FIRE, ..Default::default() });
        }
        let enemies: Vec<&Entity> = w.entities.values().filter(|e| e.team == ENEMY_TEAM && e.hp > 0.0).collect();
        let near = |r: f32| enemies.iter().filter(|e| e.flat_dist(pos) < r).count();
        let target = enemies
            .iter()
            .filter(|e| w.can_see(pc, e.center(), &[p.id, e.id]))
            .min_by(|a, b| a.flat_dist(pos).total_cmp(&b.flat_dist(pos)))
            .or_else(|| enemies.iter().min_by(|a, b| a.flat_dist(pos).total_cmp(&b.flat_dist(pos))));
        let mut input = Input::default();
        let heart = w.nearest("heart", pos, 30.0).and_then(|h| w.get(h)).map(|h| h.pos);
        let goal = match (heart, target) {
            (Some(h), _) if p.hp < p.max_hp * 0.6 => h,
            (_, Some(t)) => {
                input.aim = Some(t.center());
                input.held |= buttons::FIRE;
                let away = Vec3::new(pos.x - t.pos.x, 0.0, pos.z - t.pos.z).normalize_or(Vec3::X);
                let d = t.flat_dist(pos);
                if d < 6.0 {
                    pos + away * 3.0 + Quat::from_rotation_y(1.2) * away * 2.0
                } else if d > 9.0 {
                    t.pos
                } else {
                    pos + Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * away * 3.0
                }
            }
            _ => w.marker("player").unwrap_or(pos),
        };
        input.move_dir = steer(w, p.id, pos, goal);
        if near(2.2) > 0 && self.dash_cd <= 0.0 {
            input.pressed |= buttons::DASH;
        }
        if near(5.0) >= 4 && self.nova_cd <= 0.0 {
            input.pressed |= buttons::ALT;
        }
        Some(input)
    }
}

#[cfg(test)]
mod tests {
    use pavlite::sim::Sim;

    #[test]
    fn bot_clears_waves() {
        let mut sim = Sim::new(crate::games::def("arena"), 1);
        for _ in 0..60 * 60 {
            let input = sim.game.bot(&sim.world).unwrap();
            sim.step(&input);
        }
        let st = sim.game.status(&sim.world);
        assert!(st["wave"].as_u64().unwrap() >= 3, "{st}");
        assert!(st["kills"].as_u64().unwrap() >= 10, "{st}");
    }
}
