//! Motion aim: a DualSense in 52in1 turns the pad's aim by turning the controller (52in1
//! catalogue v1, "The DualSense", Motion). It adds to the right stick, never takes its place:
//!
//! - The stick pushed: the aim is where the stick points, plus what the controller has turned
//!   since the push began (a fresh push starts from the stick again; letting go drops it).
//! - The stick at rest: without motion the aim follows the hero (its facing). A turn of the
//!   controller turns the aim away from the facing by as much, and the aim keeps following the
//!   hero with that offset. Once the controller has been still for `SETTLE` seconds and no
//!   skill is held, the offset eases back to nothing: a turn is a nudge for the next attacks,
//!   never an aim left behind in an old direction.
//! - Turns slower than `STILL` (a hand's tremor, a sensor's drift) add nothing.
//! - The amount: the catalogue's rule, gyro (degrees a second) times the frame's seconds times
//!   the sensitivity. Its `y` turns the controller left (+), which turns the aim left on screen:
//!   a larger yaw (`dir = (sin yaw, cos yaw)`) whatever the camera's yaw.
//!
//! Shardfall reads the gyro itself (once a frame, `read_gyro_y`), so while its Motion aim is on
//! the hub leaves the right stick alone; with it off the game never reads the gyro and the hub's
//! own Motion aim (the player's 52in1 setting) turns the stick again. Natively there is no hub:
//! nothing here runs.

use serde::{Deserialize, Serialize};

/// Below this many degrees a second the controller counts as held still (a hand's tremor, a
/// sensor's drift): it adds no turn, and the offset may settle.
pub const STILL: f32 = 2.0;
/// Seconds of the controller still, with no skill held, before the offset eases back.
pub const SETTLE: f32 = 0.4;
/// How fast it eases back then (seconds for it to fall to about a third).
pub const EASE: f32 = 0.3;

/// Settings: how far the aim turns for a turn of the controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    Low,
    #[default]
    Medium,
    High,
}

impl Sensitivity {
    pub const ALL: [Sensitivity; 3] = [Sensitivity::Low, Sensitivity::Medium, Sensitivity::High];

    /// The aim's turn for one of the controller's (a wrist turns about 45 degrees each way;
    /// the stick does the big turns).
    pub fn factor(self) -> f32 {
        match self {
            Sensitivity::Low => 1.0,
            Sensitivity::Medium => 2.0,
            Sensitivity::High => 3.0,
        }
    }

    /// The setting's name in the player's language.
    pub fn name(self) -> String {
        match self {
            Sensitivity::Low => pav_core::t!("settings.motion.low"),
            Sensitivity::Medium => pav_core::t!("settings.motion.medium"),
            Sensitivity::High => pav_core::t!("settings.motion.high"),
        }
    }
}

/// The player's 52in1 settings allow motion: a DualSense is connected and its motion is the
/// game's to read.
pub fn allowed(hub: Option<&crate::hub::Settings>) -> bool {
    hub.is_some_and(|h| h.dualsense.connected && h.dualsense.motion)
}

/// The game's own Motion aim as it stands: the player's choice, or (until they make one) on
/// when their 52in1 Motion aim is.
pub fn chosen(hub: Option<&crate::hub::Settings>, pref: Option<bool>) -> bool {
    pref.unwrap_or_else(|| hub.is_some_and(|h| h.dualsense.aim))
}

/// Motion aim runs (and the game reads the gyro): allowed, and the game's setting on.
pub fn on(hub: Option<&crate::hub::Settings>, pref: Option<bool>) -> bool {
    allowed(hub) && chosen(hub, pref)
}

/// The aim's turn this frame, in radians (+ = left on screen): gyro times seconds times the
/// sensitivity.
pub fn turn(gyro_y: f32, secs: f32, sensitivity: f32) -> f32 {
    if !gyro_y.is_finite() {
        return 0.0;
    }
    gyro_y.to_radians() * secs * sensitivity
}

/// What the aim knows this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// The right stick's world yaw when it is pushed past its dead zone.
    pub stick: Option<f32>,
    /// The hero's yaw (where the aim points without motion while the stick rests).
    pub facing: f32,
    /// This frame's turn (`turn`).
    pub turn: f32,
    /// The controller turns faster than `STILL`.
    pub turning: bool,
    /// A skill or attack is held.
    pub busy: bool,
    /// The frame's seconds.
    pub secs: f32,
}

/// The aim's state between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionAim {
    /// While the stick rests: the turn added to the hero's facing.
    rest: f32,
    /// While the stick is pushed: the turn added to the stick since the push began.
    push: f32,
    /// The stick was pushed last frame.
    pushed: bool,
    /// Seconds the controller has been still with no skill held.
    still: f32,
}

/// Offsets smaller than this (radians, about a quarter of a degree) are none.
const NONE: f32 = 0.004;

/// An angle in (-pi, pi].
fn wrap(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let w = (a + PI).rem_euclid(TAU) - PI;
    if w <= -PI { w + TAU } else { w }
}

impl MotionAim {
    /// One frame: the aim's yaw with motion, or None where the aim is as it would be without
    /// motion (the stick's, or the hero's facing).
    pub fn step(&mut self, f: Frame) -> Option<f32> {
        let turn = if f.turning { f.turn } else { 0.0 };
        if f.turning || f.busy {
            self.still = 0.0;
        } else {
            self.still += f.secs;
        }
        if let Some(s) = f.stick {
            if !self.pushed {
                // A fresh push: the stick says where, motion adds from here.
                self.push = 0.0;
                self.pushed = true;
            }
            self.push = wrap(self.push + turn);
            return (self.push.abs() > NONE).then(|| wrap(s + self.push));
        }
        // The stick let go: the push's turn goes with it.
        self.pushed = false;
        self.push = 0.0;
        self.rest = wrap(self.rest + turn);
        if self.still >= SETTLE {
            self.rest *= (-f.secs / EASE).exp();
        }
        if self.rest.abs() <= NONE {
            self.rest = 0.0;
            return None;
        }
        Some(wrap(f.facing + self.rest))
    }

    /// Motion is off, or the pad isn't in use: the aim is the stick's and the hero's again.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The DualSense's turn left (+) in degrees a second, from 52in1 (`hub.dualsense.gyro.y`):
/// reading it is what tells the hub the game does its own motion. 0 natively.
pub fn read_gyro_y() -> f32 {
    #[cfg(target_arch = "wasm32")]
    return js::gyro_y() as f32;
    #[cfg(not(target_arch = "wasm32"))]
    0.0
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = gyroY)]
        pub fn gyro_y() -> f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn hub(motion: bool, aim: bool, connected: bool) -> crate::hub::Settings {
        let mut s = crate::hub::Settings::default();
        s.dualsense.connected = connected;
        s.dualsense.motion = motion;
        s.dualsense.aim = aim;
        s
    }

    #[test]
    fn gating_truth_table() {
        // No hub (a native build, the game's own page): never.
        for pref in [None, Some(true), Some(false)] {
            assert!(!on(None, pref));
            assert!(!allowed(None));
        }
        // (connected, motion, hub aim, the game's choice) -> on.
        let table = [
            (true, true, true, None, true),
            (true, true, false, None, false),
            (true, true, false, Some(true), true),
            (true, true, true, Some(false), false),
            (true, false, true, Some(true), false),
            (true, false, true, None, false),
            (false, true, true, Some(true), false),
            (false, true, true, None, false),
        ];
        for (connected, motion, aim, pref, want) in table {
            let h = hub(motion, aim, connected);
            assert_eq!(on(Some(&h), pref), want, "connected {connected} motion {motion} aim {aim} pref {pref:?}");
        }
        // The setting shows the hub's Motion aim until the player picks.
        assert!(chosen(Some(&hub(true, true, true)), None));
        assert!(!chosen(Some(&hub(true, false, true)), None));
        assert!(!chosen(Some(&hub(true, true, true)), Some(false)));
    }

    #[test]
    fn turn_is_gyro_times_seconds_times_sensitivity() {
        // 90 degrees a second for half a second at 2x: 90 degrees.
        assert!((turn(90.0, 0.5, 2.0) - FRAC_PI_2).abs() < 1e-5);
        // Turning right turns the aim right (smaller yaw).
        assert!(turn(-30.0, 0.1, 1.0) < 0.0);
        assert_eq!(turn(f32::NAN, 0.1, 1.0), 0.0);
        assert_eq!(turn(45.0, 0.0, 3.0), 0.0);
        let f: Vec<f32> = Sensitivity::ALL.iter().map(|s| s.factor()).collect();
        assert!(f.windows(2).all(|w| w[0] < w[1]) && Sensitivity::default() == Sensitivity::Medium);
    }

    #[test]
    fn turning_left_turns_the_aim_left_on_screen() {
        // The stick's screen directions (x right, y up) through the camera: a push a little
        // left of up is the up push turned left, which is a larger yaw, at any camera yaw.
        for cam_yaw in [0.0, 37.0, 90.0, 200.0, -135.0] {
            let rig = pav_view::camera::CameraRig::new(pav_view::camera::CameraParams { yaw: cam_yaw, ..Default::default() });
            let yaw = |v: glam::Vec2| {
                let d = rig.relative_move(v);
                d.x.atan2(d.y)
            };
            let e = 0.1f32;
            let up = yaw(glam::Vec2::new(0.0, 1.0));
            let left_of_up = yaw(glam::Vec2::new(-e.sin(), e.cos()));
            assert!((wrap(left_of_up - up) - e).abs() < 1e-4, "camera yaw {cam_yaw}");
        }
    }

    fn rest(turn: f32) -> Frame {
        Frame { facing: 1.0, turn, turning: turn.abs() > 0.0, secs: 0.1, ..Default::default() }
    }

    #[test]
    fn at_rest_motion_nudges_the_hero_aim_and_settles_back() {
        let mut m = MotionAim::default();
        // The controller still: the aim is the hero's, as without motion.
        assert_eq!(m.step(rest(0.0)), None);
        // Turned: the hero's facing plus the turn.
        assert!((m.step(rest(0.2)).unwrap() - 1.2).abs() < 1e-5);
        // The hero turns: the aim follows with the offset (not left in an old direction).
        let f = Frame { facing: -2.0, ..rest(0.0) };
        assert!((m.step(f).unwrap() - (-1.8)).abs() < 1e-5);
        // A turn below STILL adds nothing.
        let f = Frame { turn: 0.01, turning: false, ..rest(0.0) };
        assert!((m.step(f).unwrap() - 1.2).abs() < 1e-5);
        // A skill held keeps the offset however long (the player is aiming it).
        for _ in 0..100 {
            let f = Frame { busy: true, ..rest(0.0) };
            assert!((m.step(f).unwrap() - 1.2).abs() < 1e-5);
        }
        // Still and idle: after SETTLE it eases back to the hero's aim within a second or so.
        let mut frames = 0;
        while m.step(rest(0.0)).is_some() {
            frames += 1;
            assert!(frames < 30, "settles back");
        }
        assert!(frames as f32 * 0.1 >= SETTLE, "not before SETTLE: {frames}");
    }

    #[test]
    fn a_pushed_stick_gets_the_turns_added() {
        let push =
            |stick: f32, turn: f32| Frame { stick: Some(stick), turn, turning: turn != 0.0, secs: 0.1, ..Default::default() };
        let mut m = MotionAim::default();
        // The stick alone: its own aim (None = as without motion).
        assert_eq!(m.step(push(0.5, 0.0)), None);
        // Turned while pushed: the stick plus the turn, and it stays added while pushed.
        assert!((m.step(push(0.5, 0.3)).unwrap() - 0.8).abs() < 1e-5);
        assert!((m.step(push(0.6, 0.0)).unwrap() - 0.9).abs() < 1e-5);
        // Let go: the hero's aim again (the push's turn goes with the push).
        assert_eq!(m.step(rest(0.0)), None);
        // A fresh push starts from the stick again.
        assert_eq!(m.step(push(-1.0, 0.0)), None);
    }

    #[test]
    fn angles_stay_wrapped() {
        let mut m = MotionAim::default();
        let mut last = 0.0;
        for _ in 0..200 {
            last = m.step(Frame { busy: true, ..rest(0.5) }).unwrap();
            assert!(last > -std::f32::consts::PI - 1e-5 && last <= std::f32::consts::PI + 1e-5);
        }
        assert!((wrap(last - (1.0 + 200.0 * 0.5))).abs() < 1e-3);
        m.reset();
        assert_eq!(m, MotionAim::default());
    }
}
