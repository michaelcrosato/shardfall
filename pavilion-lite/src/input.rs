//! Device-independent input for one tick. The host (window, agent tool, replay, bot) fills it;
//! the simulation never sees devices. Directions are already in world space.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Button bits. Keyboard: Space jump, J / left mouse fire, K / right mouse alt, E use,
/// C / Ctrl crouch, Shift dash.
pub mod buttons {
    pub const JUMP: u32 = 1 << 0;
    pub const FIRE: u32 = 1 << 1;
    pub const ALT: u32 = 1 << 2;
    pub const USE: u32 = 1 << 3;
    pub const CROUCH: u32 = 1 << 4;
    pub const DASH: u32 = 1 << 5;

    pub const NAMES: &[(&str, u32)] =
        &[("jump", JUMP), ("fire", FIRE), ("alt", ALT), ("use", USE), ("crouch", CROUCH), ("dash", DASH)];

    pub fn from_name(n: &str) -> Option<u32> {
        NAMES.iter().find(|(k, _)| k.eq_ignore_ascii_case(n.trim())).map(|(_, v)| *v)
    }

    /// "jump,fire" -> bits. Unknown names are an error.
    pub fn parse(list: &str) -> Result<u32, String> {
        let mut bits = 0;
        for n in list.split(',').filter(|s| !s.trim().is_empty()) {
            bits |= from_name(n).ok_or_else(|| {
                let all: Vec<&str> = NAMES.iter().map(|(k, _)| *k).collect();
                format!("unknown button '{n}' (buttons: {})", all.join(", "))
            })?;
        }
        Ok(bits)
    }

    pub fn names(bits: u32) -> Vec<&'static str> {
        NAMES.iter().filter(|(_, v)| bits & v != 0).map(|(k, _)| *k).collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// Wanted movement on the ground plane: x = world +X (east), y = world +Z (south).
    /// Length <= 1. The window converts WASD relative to the camera.
    #[serde(default)]
    pub move_dir: Vec2,
    /// World-space aim point (the mouse on the plane at the player's feet).
    #[serde(default)]
    pub aim: Option<Vec3>,
    /// Buttons held this tick.
    #[serde(default)]
    pub held: u32,
    /// Buttons that went down since the previous tick (never lost, even for short taps).
    #[serde(default)]
    pub pressed: u32,
}

impl Input {
    pub fn down(&self, b: u32) -> bool {
        self.held & b != 0
    }
    pub fn just(&self, b: u32) -> bool {
        self.pressed & b != 0
    }
    /// Movement toward a point (full speed, on the ground plane).
    pub fn toward(from: Vec3, to: Vec3) -> Input {
        let d = Vec2::new(to.x - from.x, to.z - from.z);
        Input { move_dir: d.normalize_or_zero(), ..Default::default() }
    }
    /// The movement as a 3D direction (y = 0).
    pub fn move3(&self) -> Vec3 {
        Vec3::new(self.move_dir.x, 0.0, self.move_dir.y)
    }
}
