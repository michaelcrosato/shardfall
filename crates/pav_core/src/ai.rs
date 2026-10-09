//! Simple brains for non-player characters: they produce the same `InputFrame` the player's
//! controls do, so NPCs and creatures move with the full character controller.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::input::{InputFrame, buttons};
use crate::rng::Rng;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiDef {
    /// Stands still (facing its start direction).
    #[default]
    Idle,
    /// Walks to random spots within `radius` of home, pausing in between.
    Wander {
        #[serde(default = "four")]
        radius: f32,
        #[serde(default = "pause")]
        pause: [f32; 2],
    },
    /// Walks through `points` (room layout space, [x, z]) in a loop.
    Patrol {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        pause: f32,
    },
    /// Walks round a circle of `radius` about its home.
    Circle {
        #[serde(default = "three")]
        radius: f32,
        #[serde(default)]
        clockwise: bool,
    },
    /// Follows the player, stopping `distance` away.
    Follow {
        #[serde(default = "two")]
        distance: f32,
    },
    /// A stealth guard: patrols `points`, looks around when it stops, and spots the player
    /// within `range` (m) and `angle` (degrees, full cone) in plain sight. Crouching shortens
    /// the range. Spotted = back to the last checkpoint.
    Guard {
        points: Vec<[f32; 2]>,
        #[serde(default = "one")]
        pause: f32,
        #[serde(default = "seven")]
        range: f32,
        #[serde(default = "seventy")]
        angle: f32,
    },
}

fn one() -> f32 {
    1.0
}
fn seven() -> f32 {
    7.0
}
fn seventy() -> f32 {
    70.0
}

fn four() -> f32 {
    4.0
}
fn three() -> f32 {
    3.0
}
fn two() -> f32 {
    2.0
}
fn pause() -> [f32; 2] {
    [0.5, 2.5]
}

/// A performance: motion clips and moves played one after another (`[[npc]] clips`, `moves`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Perform {
    /// Clip ids (`crate::clips`), then moves (`crate::moves` indices), in that order.
    pub clips: Vec<u32>,
    pub moves: Vec<u8>,
    /// Play flags for the clips (`clips::MIRROR`, `UPPER`...).
    pub flags: u8,
    /// Seconds a looping clip plays before the next.
    pub hold: f32,
    /// Speed of everything (1 = as captured).
    pub speed: f32,
    /// Which item is playing (starts past the end, so the first tick starts item 0).
    pub index: usize,
    /// Seconds until the next item.
    pub timer: f32,
    /// Seconds into the current move.
    pub t: f32,
}

impl Perform {
    pub fn new(clips: Vec<u32>, moves: Vec<u8>, flags: u8, hold: f32, speed: f32) -> Self {
        let n = clips.len() + moves.len();
        Self { clips, moves, flags, hold, speed, index: n.saturating_sub(1), timer: 0.0, t: 0.0 }
    }

    /// Advances the performance and poses `anim` for it (`params`: the `anim` settings).
    pub fn tick(&mut self, anim: &mut crate::puppet::PuppetState, dt: f32, params: &crate::clips::AnimParams) {
        let n = self.clips.len() + self.moves.len();
        if n == 0 {
            return;
        }
        let speed = if self.speed > 0.0 { self.speed } else { 1.0 } * params.tempo.max(0.05);
        let flags = if params.mirror { self.flags ^ crate::clips::MIRROR } else { self.flags };
        self.timer -= dt;
        if self.timer <= 0.0 {
            self.index = (self.index + 1) % n;
            self.t = 0.0;
            if let Some(&id) = self.clips.get(self.index) {
                let (dur, looping) = crate::clips::with(id, |c| (c.dur, c.looping)).unwrap_or((1.0, false));
                anim.set_action(crate::moves::MoveId::NONE, 0.0, 0.0, 1.0);
                anim.play_clip(id, flags, speed);
                self.timer = if looping { self.hold.max(dur) } else { dur / speed + 0.8 };
            } else {
                anim.stop_clip();
                let m = self.moves[self.index - self.clips.len()];
                let len = crate::moves::table().get(m).map_or(0.5, |d| d.wind + d.active + d.recover);
                self.timer = len / speed + 0.9;
            }
        }
        if let Some(&id) = self.clips.get(self.index) {
            // Tempo and mirroring follow the settings as they change.
            if anim.clip == id {
                anim.play_clip(id, flags, speed);
            }
        } else {
            // A move at its own timing, then a pause before the next one.
            let m = self.moves[self.index - self.clips.len()];
            let table = crate::moves::table();
            if let Some(d) = table.get(m) {
                let len = (d.wind + d.active + d.recover).max(1e-3);
                self.t += dt * speed;
                if self.t <= len {
                    let hit = (d.wind + d.hit * d.active) / len;
                    anim.set_action(crate::moves::MoveId(m), self.t / len, hit, 1.0);
                } else {
                    anim.set_action(crate::moves::MoveId::NONE, 0.0, 0.0, 1.0);
                }
            }
        }
    }
}

/// A running brain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ai {
    pub def: AiDef,
    /// Home (feet) in world space; patrol points already in world space.
    pub home: Vec3,
    pub points: Vec<Vec3>,
    /// 0..1 fraction of full speed.
    pub speed: f32,
    /// Seconds between hops (0 = never).
    pub hop: f32,
    pub facing: f32,
    #[serde(default)]
    pub target: Option<Vec3>,
    #[serde(default)]
    pub wait: f32,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub hop_timer: f32,
    #[serde(default)]
    pub stuck: f32,
    #[serde(default)]
    pub last: Vec3,
    /// Seconds the jump button is still held (full-height hops).
    #[serde(default)]
    pub jump_hold: f32,
    /// Guards: how close to raising the alarm (0..1), and whether the player is in sight.
    #[serde(default)]
    pub alert: f32,
    #[serde(default)]
    pub sees: bool,
    /// Guards: heading when they stopped (they look around it).
    #[serde(default)]
    pub look: f32,
    /// Clips and moves this character performs in turn.
    #[serde(default)]
    pub perform: Option<Perform>,
}

impl Ai {
    pub fn new(def: AiDef, home: Vec3, points: Vec<Vec3>, speed: f32, hop: f32, facing: f32) -> Self {
        Self {
            def,
            home,
            points,
            speed,
            hop,
            facing,
            target: None,
            wait: 0.5,
            index: 0,
            hop_timer: hop,
            stuck: 0.0,
            last: home,
            jump_hold: 0.0,
            alert: 0.0,
            sees: false,
            look: facing,
            perform: None,
        }
    }

    /// Facing to turn to while standing still (idle brains face their start direction).
    pub fn rest_facing(&self) -> Option<f32> {
        matches!(self.def, AiDef::Idle).then_some(self.facing).filter(|_| self.target.is_none())
    }

    /// Decides this tick's input. `feet` = own position, `player` = the player's feet,
    /// `others` = other characters' feet (kept at arm's length).
    pub fn think(&mut self, feet: Vec3, player: Option<Vec3>, others: &[Vec3], rng: &mut Rng, dt: f32) -> InputFrame {
        let mut out = InputFrame::default();
        let flat = |v: Vec3| Vec2::new(v.x, v.z);
        let mut dir = Vec2::ZERO;
        match &self.def {
            AiDef::Idle => {
                // Knocked away: walk back to the spot.
                let d = flat(self.home - feet);
                if d.length() > if self.target.is_some() { 0.15 } else { 0.35 } {
                    self.target = Some(self.home);
                    dir = d.normalize();
                } else {
                    self.target = None;
                }
            }
            AiDef::Wander { radius, pause } => {
                if self.wait > 0.0 {
                    self.wait -= dt;
                } else {
                    let t = *self.target.get_or_insert_with(|| {
                        let a = rng.range(0.0, std::f32::consts::TAU);
                        let r = radius * rng.range(0.2, 1.0).sqrt();
                        self.home + Vec3::new(a.cos() * r, 0.0, a.sin() * r)
                    });
                    let d = flat(t - feet);
                    if d.length() < 0.4 || self.stuck > 1.0 {
                        self.target = None;
                        self.stuck = 0.0;
                        self.wait = rng.range(pause[0], pause[1].max(pause[0]));
                    } else {
                        dir = d.normalize();
                    }
                }
            }
            AiDef::Patrol { pause, .. } | AiDef::Guard { pause, .. } => {
                if self.wait > 0.0 {
                    self.wait -= dt;
                } else if !self.points.is_empty() {
                    let t = self.points[self.index % self.points.len()];
                    let d = flat(t - feet);
                    if d.length() < 0.35 || self.stuck > 2.0 {
                        self.index = (self.index + 1) % self.points.len();
                        self.wait = *pause;
                        self.stuck = 0.0;
                        // A single point on the spot keeps the start facing (atan2(0, 0) = south).
                        self.look = if d.length() > 0.05 { d.x.atan2(d.y) } else { self.facing };
                    } else {
                        dir = d.normalize();
                    }
                }
            }
            AiDef::Circle { radius, clockwise } => {
                // Steer to the point a little further round the circle.
                let off = flat(feet - self.home);
                let ang = off.y.atan2(off.x);
                let step = if *clockwise { -0.5 } else { 0.5 };
                let next = Vec2::new((ang + step).cos(), (ang + step).sin()) * *radius;
                let d = next - off;
                dir = d.normalize_or_zero();
            }
            AiDef::Follow { distance } => {
                if let Some(p) = player {
                    let d = flat(p - feet);
                    if d.length() > *distance {
                        dir = d.normalize();
                    }
                }
            }
        }
        // Keep apart from other characters (followers would pile up on each other).
        let mut push = Vec2::ZERO;
        for o in others {
            let d = flat(feet - *o);
            let len = d.length();
            if len > 1e-3 && len < 1.1 {
                push += d / len * (1.0 - len / 1.1);
            }
        }
        if push != Vec2::ZERO && (dir != Vec2::ZERO || push.length() > 0.45) {
            dir = (dir + push * 1.2).clamp_length_max(1.0);
        }
        // Stuck detection (walking but not getting anywhere).
        if dir != Vec2::ZERO && flat(feet - self.last).length() < 0.3 * self.speed.max(0.2) * dt {
            self.stuck += dt;
        } else {
            self.stuck = (self.stuck - dt).max(0.0);
        }
        self.last = feet;
        out.move_dir = dir * self.speed.clamp(0.05, 1.0);
        if self.hop > 0.0 {
            self.hop_timer -= dt;
            if self.hop_timer <= 0.0 {
                self.hop_timer = self.hop;
                self.jump_hold = 0.35;
                out.pressed |= buttons::JUMP;
            }
        }
        if self.jump_hold > 0.0 {
            self.jump_hold -= dt;
            out.held |= buttons::JUMP;
        }
        out
    }
}
