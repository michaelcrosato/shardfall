//! Visual-only effects attached to objects (they never affect the simulation): lights,
//! particle emitters and screen distortion. The view turns them into renderer data.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// A point light carried by an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightDef {
    pub color: String,
    /// Reach (m).
    pub radius: f32,
    pub intensity: f32,
    /// Random flicker amount (0 = steady, 1 = candle).
    pub flicker: f32,
    /// Slow pulse (Hz, 0 = none).
    pub pulse: f32,
    /// Casts shadows (a few lights at a time).
    pub shadows: bool,
    /// Offset from the object (its own frame ignored: world axes).
    pub offset: Vec3,
}

impl Default for LightDef {
    fn default() -> Self {
        Self { color: "#ffd9a0".into(), radius: 6.0, intensity: 1.5, flicker: 0.0, pulse: 0.0, shadows: false, offset: Vec3::ZERO }
    }
}

/// A particle emitter carried by an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterDef {
    /// fire | smoke | sparks | embers | fountain | snow | rain | fireflies | magic | steam | dust |
    /// bubbles | confetti
    pub preset: String,
    /// Particles per second (0 = the preset's rate).
    pub rate: f32,
    /// Colour override ("" = the preset's).
    pub color: String,
    /// Multipliers on the preset.
    pub size: f32,
    pub speed: f32,
    pub life: f32,
    /// Spawn box half-extents (zero = the preset's).
    pub area: Vec3,
    pub offset: Vec3,
    /// Height of the ground below the object (e.g. -5 for an emitter 5 m up): particles land
    /// there. None = falling presets land at the object's own height.
    pub ground: Option<f32>,
}

impl Default for EmitterDef {
    fn default() -> Self {
        Self {
            preset: "fire".into(),
            rate: 0.0,
            color: String::new(),
            size: 1.0,
            speed: 1.0,
            life: 1.0,
            area: Vec3::ZERO,
            offset: Vec3::ZERO,
            ground: None,
        }
    }
}

/// A screen distortion source carried by an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DistortDef {
    /// haze | lens | ripple | ring (a repeating shockwave)
    pub kind: String,
    pub radius: f32,
    /// Offset as a fraction of the disc's screen size (0.3-0.6 reads well).
    pub strength: f32,
    /// Seconds between rings.
    pub period: f32,
    pub offset: Vec3,
}

impl Default for DistortDef {
    fn default() -> Self {
        Self { kind: "haze".into(), radius: 1.5, strength: 0.4, period: 1.5, offset: Vec3::ZERO }
    }
}
