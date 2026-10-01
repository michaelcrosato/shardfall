//! Feel metrics for the player, measured live from the simulation: how long it takes to reach
//! full speed, to stop and to turn around, response delay in ticks, jump height and air time.
//! Display-side input latency is measured by the app (it owns the clock of input events).

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FeelReport {
    /// Current horizontal speed (m/s).
    pub speed: f32,
    /// Steady speed reached in the last run-up (m/s).
    pub top_speed: f32,
    /// Input start -> 90% of steady speed (ms).
    pub accel_ms: f32,
    /// Input release -> standing still (ms) and the distance slid (m).
    pub stop_ms: f32,
    pub stop_dist: f32,
    /// Reversing direction -> 90% speed the other way (ms).
    pub turn_ms: f32,
    /// Input start -> first movement (simulation ticks).
    pub response_ticks: u32,
    /// Last jump: apex above take-off (m), time in the air (ms), horizontal distance (m).
    pub jump_height: f32,
    pub air_ms: f32,
    pub jump_dist: f32,
    /// Ticks measured so far (for "has data" checks).
    pub samples: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeelMeter {
    pub report: FeelReport,
    moving: bool,
    start_tick: Option<u64>,
    response_pending: bool,
    /// (ticks since start, speed) during a run-up.
    run: Vec<(u32, f32)>,
    accel_done: bool,
    stop_start: Option<(u64, Vec3)>,
    turn_start: Option<u64>,
    turn_target: f32,
    air: Option<(u64, Vec3, f32)>, // take-off tick, position, max height
    last_dir: Vec2,
}

impl FeelMeter {
    /// Call once per tick for the player. `wish` is the movement input on the ground plane.
    pub fn update(&mut self, tick: u64, dt: f32, wish: Vec2, vel: Vec3, feet: Vec3, grounded: bool) {
        let r = &mut self.report;
        r.samples += 1;
        let hv = Vec2::new(vel.x, vel.z);
        let speed = hv.length();
        r.speed = speed;
        let ms = |ticks: u64| ticks as f32 * dt * 1000.0;
        let input = wish.length() > 0.3;

        if input && !self.moving {
            self.start_tick = Some(tick);
            self.response_pending = true;
            self.run.clear();
            self.accel_done = false;
            if let Some((t0, p0)) = self.stop_start.take() {
                // Interrupted stop: still report what we have.
                let _ = (t0, p0);
            }
        }
        if !input && self.moving && speed > 0.5 {
            self.stop_start = Some((tick, feet));
        }
        self.moving = input;

        if self.response_pending && speed > 0.05 {
            if let Some(t0) = self.start_tick {
                r.response_ticks = (tick - t0) as u32;
            }
            self.response_pending = false;
        }
        if input && !self.accel_done {
            if let Some(t0) = self.start_tick {
                self.run.push(((tick - t0) as u32, speed));
                let n = self.run.len();
                // Steady once speed stopped growing for 10 ticks.
                if n > 12 {
                    let recent = self.run[n - 10..].iter().map(|s| s.1).fold(0.0, f32::max);
                    let before = self.run[n - 11].1;
                    if recent <= before * 1.005 + 0.01 && before > 0.3 {
                        let steady = before;
                        r.top_speed = steady;
                        if let Some(first) = self.run.iter().find(|s| s.1 >= steady * 0.9) {
                            r.accel_ms = ms(first.0 as u64 + 1);
                        }
                        self.accel_done = true;
                    }
                }
                if n > 600 {
                    self.accel_done = true;
                }
            }
        }
        if let Some((t0, p0)) = self.stop_start {
            if input {
                self.stop_start = None;
            } else if speed < 0.1 {
                r.stop_ms = ms(tick - t0);
                r.stop_dist = Vec2::new(feet.x - p0.x, feet.z - p0.z).length();
                self.stop_start = None;
            }
        }
        // Turnaround: input points against the current velocity.
        if input && speed > 1.0 {
            let wd = wish.normalize();
            if self.turn_start.is_none() && hv.normalize().dot(wd) < -0.5 {
                self.turn_start = Some(tick);
                self.turn_target = speed;
            }
        }
        if let Some(t0) = self.turn_start {
            if !input {
                self.turn_start = None;
            } else {
                let along = hv.dot(wish.normalize());
                if along >= self.turn_target * 0.9 {
                    r.turn_ms = ms(tick - t0);
                    self.turn_start = None;
                } else if tick - t0 > 300 {
                    self.turn_start = None;
                }
            }
        }
        // Jumps and falls.
        match (&mut self.air, grounded) {
            (None, false) => self.air = Some((tick, feet, feet.y)),
            (Some(a), false) => a.2 = a.2.max(feet.y),
            (Some((t0, p0, top)), true) => {
                let air_ms = ms(tick - *t0);
                if air_ms > 120.0 {
                    r.jump_height = *top - p0.y;
                    r.air_ms = air_ms;
                    r.jump_dist = Vec2::new(feet.x - p0.x, feet.z - p0.z).length();
                }
                self.air = None;
            }
            (None, true) => {}
        }
        if speed > 0.1 {
            self.last_dir = hv / speed;
        }
    }
}
